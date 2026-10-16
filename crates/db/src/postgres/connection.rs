//! PostgreSQL Physical TCP Connection Handler.
//!
//! Manages socket I/O, handshake negotiation, transaction state, and wire framing.

use super::protocol::*;
use crate::error::DbError;
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PgQueryResult {
    pub fields: Vec<FieldDescription>,
    pub rows: Vec<DataRow>,
    pub command_tag: String,
}

#[derive(Debug, Clone)]
pub struct PgExecuteResult {
    pub command_tag: String,
    pub rows_affected: u64,
}

pub struct PgConnection {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
    pub process_id: u32,
    pub secret_key: u32,
    pub parameters: HashMap<String, String>,
    pub transaction_status: u8,
}

impl std::fmt::Debug for PgConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgConnection")
            .field("process_id", &self.process_id)
            .field("secret_key", &self.secret_key)
            .field("parameters", &self.parameters)
            .field("transaction_status", &self.transaction_status)
            .finish()
    }
}

impl PgConnection {
    /// Connects to a PostgreSQL server over TCP and performs the v3.0 startup handshake.
    pub fn connect(
        host: &str,
        port: u16,
        user: &str,
        database: &str,
        password: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<Self, DbError> {
        let addr_str = format!("{}:{}", host, port);
        let addrs: Vec<SocketAddr> = addr_str
            .to_socket_addrs()
            .map_err(|e| DbError::InvalidUrl(format!("Failed to resolve '{}': {}", addr_str, e)))?
            .collect();

        if addrs.is_empty() {
            return Err(DbError::InvalidUrl(format!(
                "Could not resolve address '{}'",
                addr_str
            )));
        }

        let stream = match timeout {
            Some(d) => TcpStream::connect_timeout(&addrs[0], d)?,
            None => TcpStream::connect(&addrs[0])?,
        };

        stream.set_nodelay(true)?;
        if let Some(t) = timeout {
            stream.set_read_timeout(Some(t))?;
            stream.set_write_timeout(Some(t))?;
        }

        let read_stream = stream.try_clone()?;
        let mut reader = BufReader::new(read_stream);
        let mut writer = BufWriter::new(stream);

        // 1. Send StartupMessage
        write_startup_message(
            &mut writer,
            user,
            database,
            &[
                ("application_name", "TruLang Native Engine"),
                ("client_encoding", "UTF8"),
            ],
        )?;

        let mut process_id = 0;
        let mut secret_key = 0;
        let mut parameters = HashMap::new();
        let transaction_status;

        // 2. Handshake loop until ReadyForQuery
        loop {
            let msg = read_backend_message(&mut reader)?;
            match msg {
                BackendMessage::AuthenticationOk => {
                    // Authenticated successfully
                }
                BackendMessage::AuthenticationCleartextPassword => {
                    let pwd = password.ok_or_else(|| {
                        DbError::AuthenticationFailed(
                            "Server requested password, but no password was supplied".into(),
                        )
                    })?;
                    write_password_message(&mut writer, pwd)?;
                }
                BackendMessage::AuthenticationMd5Password(salt) => {
                    let pwd = password.ok_or_else(|| {
                        DbError::AuthenticationFailed(
                            "Server requested MD5 password, but no password was supplied".into(),
                        )
                    })?;
                    // Hash password as "md5" + md5(md5(password + user) + salt)
                    let user_pwd = format!("{}{}", pwd, user);
                    let inner_hash = to_hex(&compute_md5(user_pwd.as_bytes()));
                    let mut salted = inner_hash.into_bytes();
                    salted.extend_from_slice(&salt);
                    let outer_hash = format!("md5{}", to_hex(&compute_md5(&salted)));
                    write_password_message(&mut writer, &outer_hash)?;
                }
                BackendMessage::BackendKeyData {
                    process_id: pid,
                    secret_key: key,
                } => {
                    process_id = pid;
                    secret_key = key;
                }
                BackendMessage::ParameterStatus { name, value } => {
                    parameters.insert(name, value);
                }
                BackendMessage::ReadyForQuery(status) => {
                    transaction_status = status;
                    break;
                }
                BackendMessage::ErrorResponse(err_fields) => {
                    let code = err_fields
                        .get(&b'C')
                        .cloned()
                        .unwrap_or_else(|| "UNKNOWN".into());
                    let message = err_fields
                        .get(&b'M')
                        .cloned()
                        .unwrap_or_else(|| "Database error".into());
                    return Err(DbError::PgServerError { code, message });
                }
                BackendMessage::NoticeResponse(_) => {}
                other => {
                    return Err(DbError::PgProtocol(format!(
                        "Unexpected backend message during handshake: {:?}",
                        other
                    )));
                }
            }
        }

        Ok(Self {
            reader,
            writer,
            process_id,
            secret_key,
            parameters,
            transaction_status,
        })
    }

    /// Executes a simple query returning all parsed data rows.
    pub fn query(&mut self, sql: &str) -> Result<PgQueryResult, DbError> {
        write_query_message(&mut self.writer, sql)?;

        let mut fields = Vec::new();
        let mut rows = Vec::new();
        let mut command_tag = String::new();

        loop {
            let msg = read_backend_message(&mut self.reader)?;
            match msg {
                BackendMessage::RowDescription(desc) => {
                    fields = desc;
                }
                BackendMessage::DataRow(row) => {
                    rows.push(row);
                }
                BackendMessage::CommandComplete(tag) => {
                    command_tag = tag;
                }
                BackendMessage::EmptyQueryResponse => {}
                BackendMessage::ReadyForQuery(status) => {
                    self.transaction_status = status;
                    break;
                }
                BackendMessage::ErrorResponse(err_fields) => {
                    let code = err_fields
                        .get(&b'C')
                        .cloned()
                        .unwrap_or_else(|| "UNKNOWN".into());
                    let message = err_fields
                        .get(&b'M')
                        .cloned()
                        .unwrap_or_else(|| "Database error".into());
                    return Err(DbError::PgServerError { code, message });
                }
                BackendMessage::NoticeResponse(_) => {}
                other => {
                    return Err(DbError::PgProtocol(format!(
                        "Unexpected message during query: {:?}",
                        other
                    )));
                }
            }
        }

        Ok(PgQueryResult {
            fields,
            rows,
            command_tag,
        })
    }

    /// Executes a command statement (e.g. INSERT, UPDATE, DELETE) returning rows affected.
    pub fn execute(&mut self, sql: &str) -> Result<PgExecuteResult, DbError> {
        let res = self.query(sql)?;
        let rows_affected = parse_rows_affected(&res.command_tag);
        Ok(PgExecuteResult {
            command_tag: res.command_tag,
            rows_affected,
        })
    }

    /// Checks if connection socket is still alive and responsive with a lightweight query.
    pub fn is_healthy(&mut self) -> bool {
        match self.query("/* ping */") {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Sends a Terminate message and shuts down connection.
    pub fn close(&mut self) -> Result<(), DbError> {
        let _ = write_terminate_message(&mut self.writer);
        Ok(())
    }
}

fn parse_rows_affected(tag: &str) -> u64 {
    // Tags: "INSERT 0 1", "DELETE 5", "UPDATE 2", "SELECT 42"
    let parts: Vec<&str> = tag.split_whitespace().collect();
    if parts.len() >= 2 {
        if let Ok(n) = parts.last().unwrap().parse::<u64>() {
            return n;
        }
    }
    0
}

/// Simple RFC 1321 MD5 computation for standard Postgres auth
fn compute_md5(data: &[u8]) -> [u8; 16] {
    // Minimal standard MD5 implementation without external crypto dependencies
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;

    let orig_len_bits = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&orig_len_bits.to_le_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 16];
        for i in 0..16 {
            w[i] = u32::from_le_bytes(chunk[i * 4..(i + 1) * 4].try_into().unwrap());
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;

        let s: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
            5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];

        let k: [u32; 64] = [
            0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
            0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
            0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
            0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
            0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
            0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
            0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
            0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
            0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
            0xeb86d391,
        ];

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };

            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(w[g])).rotate_left(s[i]),
            );
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&h0.to_le_bytes());
    out[4..8].copy_from_slice(&h1.to_le_bytes());
    out[8..12].copy_from_slice(&h2.to_le_bytes());
    out[12..16].copy_from_slice(&h3.to_le_bytes());
    out
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}
