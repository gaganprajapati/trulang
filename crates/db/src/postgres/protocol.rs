//! PostgreSQL Frontend/Backend Protocol v3.0 Wire Implementation.
//!
//! Handles binary framing, startup handshakes, authentication, row descriptions,
//! data rows, and command completion over TCP sockets.

use crate::error::DbError;
use std::collections::HashMap;
use std::io::{Read, Write};

pub const PROTOCOL_VERSION_3_0: i32 = 196608; // (3 << 16) | 0

// Common PostgreSQL Type OIDs
pub const OID_BOOL: u32 = 16;
pub const OID_INT8: u32 = 20;
pub const OID_INT2: u32 = 21;
pub const OID_INT4: u32 = 23;
pub const OID_TEXT: u32 = 25;
pub const OID_FLOAT4: u32 = 700;
pub const OID_FLOAT8: u32 = 701;
pub const OID_VARCHAR: u32 = 1043;
pub const OID_JSON: u32 = 114;
pub const OID_JSONB: u32 = 3802;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescription {
    pub name: String,
    pub table_oid: u32,
    pub column_id: u16,
    pub type_oid: u32,
    pub type_size: i16,
    pub type_modifier: i32,
    pub format_code: i16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataRow {
    pub values: Vec<Option<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendMessage {
    AuthenticationOk,
    AuthenticationCleartextPassword,
    AuthenticationMd5Password([u8; 4]),
    BackendKeyData { process_id: u32, secret_key: u32 },
    ParameterStatus { name: String, value: String },
    ReadyForQuery(u8),
    RowDescription(Vec<FieldDescription>),
    DataRow(DataRow),
    CommandComplete(String),
    ErrorResponse(HashMap<u8, String>),
    NoticeResponse(HashMap<u8, String>),
    EmptyQueryResponse,
}

/// Serializes and writes a PostgreSQL v3.0 StartupMessage.
pub fn write_startup_message<W: Write>(
    writer: &mut W,
    user: &str,
    database: &str,
    params: &[(&str, &str)],
) -> Result<(), DbError> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&PROTOCOL_VERSION_3_0.to_be_bytes());

    payload.extend_from_slice(b"user\0");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0);

    payload.extend_from_slice(b"database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.push(0);

    for (k, v) in params {
        payload.extend_from_slice(k.as_bytes());
        payload.push(0);
        payload.extend_from_slice(v.as_bytes());
        payload.push(0);
    }
    payload.push(0); // Trailing terminator

    let length = (payload.len() + 4) as i32;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Serializes and writes a PasswordMessage ('p').
pub fn write_password_message<W: Write>(writer: &mut W, password: &str) -> Result<(), DbError> {
    let mut payload = Vec::new();
    payload.extend_from_slice(password.as_bytes());
    payload.push(0);

    let length = (payload.len() + 4) as i32;
    writer.write_all(b"p")?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Serializes and writes a Simple Query message ('Q').
pub fn write_query_message<W: Write>(writer: &mut W, query: &str) -> Result<(), DbError> {
    let mut payload = Vec::new();
    payload.extend_from_slice(query.as_bytes());
    payload.push(0);

    let length = (payload.len() + 4) as i32;
    writer.write_all(b"Q")?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Serializes and writes a Terminate message ('X').
pub fn write_terminate_message<W: Write>(writer: &mut W) -> Result<(), DbError> {
    writer.write_all(&[b'X', 0, 0, 0, 4])?;
    writer.flush()?;
    Ok(())
}

/// Reads the next Backend message from the wire.
pub fn read_backend_message<R: Read>(reader: &mut R) -> Result<BackendMessage, DbError> {
    let mut type_buf = [0u8; 1];
    reader.read_exact(&mut type_buf)?;
    let msg_type = type_buf[0];

    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = i32::from_be_bytes(len_buf);
    if len < 4 {
        return Err(DbError::PgProtocol(format!(
            "Invalid message length: {}",
            len
        )));
    }

    let payload_len = (len - 4) as usize;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload)?;

    match msg_type {
        b'R' => parse_authentication(&payload),
        b'K' => parse_backend_key_data(&payload),
        b'S' => parse_parameter_status(&payload),
        b'Z' => {
            if payload.is_empty() {
                return Err(DbError::PgProtocol("Empty ReadyForQuery payload".into()));
            }
            Ok(BackendMessage::ReadyForQuery(payload[0]))
        }
        b'T' => parse_row_description(&payload),
        b'D' => parse_data_row(&payload),
        b'C' => {
            let s = read_null_terminated_string(&payload, 0)?.0;
            Ok(BackendMessage::CommandComplete(s))
        }
        b'E' => parse_error_response(&payload),
        b'N' => parse_notice_response(&payload),
        b'I' => Ok(BackendMessage::EmptyQueryResponse),
        other => Err(DbError::PgProtocol(format!(
            "Unsupported PostgreSQL message type: '{}' ({})",
            other as char, other
        ))),
    }
}

fn parse_authentication(payload: &[u8]) -> Result<BackendMessage, DbError> {
    if payload.len() < 4 {
        return Err(DbError::PgProtocol(
            "Truncated Authentication message".into(),
        ));
    }
    let code = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    match code {
        0 => Ok(BackendMessage::AuthenticationOk),
        3 => Ok(BackendMessage::AuthenticationCleartextPassword),
        5 => {
            if payload.len() < 8 {
                return Err(DbError::PgProtocol(
                    "Truncated MD5 Authentication message".into(),
                ));
            }
            let mut salt = [0u8; 4];
            salt.copy_from_slice(&payload[4..8]);
            Ok(BackendMessage::AuthenticationMd5Password(salt))
        }
        other => Err(DbError::PgProtocol(format!(
            "Unsupported authentication method: {}",
            other
        ))),
    }
}

fn parse_backend_key_data(payload: &[u8]) -> Result<BackendMessage, DbError> {
    if payload.len() < 8 {
        return Err(DbError::PgProtocol(
            "Truncated BackendKeyData message".into(),
        ));
    }
    let pid = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let key = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
    Ok(BackendMessage::BackendKeyData {
        process_id: pid,
        secret_key: key,
    })
}

fn parse_parameter_status(payload: &[u8]) -> Result<BackendMessage, DbError> {
    let (name, next) = read_null_terminated_string(payload, 0)?;
    let (value, _) = read_null_terminated_string(payload, next)?;
    Ok(BackendMessage::ParameterStatus { name, value })
}

fn parse_row_description(payload: &[u8]) -> Result<BackendMessage, DbError> {
    if payload.len() < 2 {
        return Err(DbError::PgProtocol(
            "Truncated RowDescription message".into(),
        ));
    }
    let num_fields = i16::from_be_bytes([payload[0], payload[1]]) as usize;
    let mut offset = 2;
    let mut fields = Vec::with_capacity(num_fields);

    for _ in 0..num_fields {
        let (name, next) = read_null_terminated_string(payload, offset)?;
        offset = next;
        if offset + 18 > payload.len() {
            return Err(DbError::PgProtocol(
                "Truncated field in RowDescription".into(),
            ));
        }

        let table_oid = u32::from_be_bytes(payload[offset..offset + 4].try_into().unwrap());
        let column_id = u16::from_be_bytes(payload[offset + 4..offset + 6].try_into().unwrap());
        let type_oid = u32::from_be_bytes(payload[offset + 6..offset + 10].try_into().unwrap());
        let type_size = i16::from_be_bytes(payload[offset + 10..offset + 12].try_into().unwrap());
        let type_modifier =
            i32::from_be_bytes(payload[offset + 12..offset + 16].try_into().unwrap());
        let format_code = i16::from_be_bytes(payload[offset + 16..offset + 18].try_into().unwrap());
        offset += 18;

        fields.push(FieldDescription {
            name,
            table_oid,
            column_id,
            type_oid,
            type_size,
            type_modifier,
            format_code,
        });
    }

    Ok(BackendMessage::RowDescription(fields))
}

fn parse_data_row(payload: &[u8]) -> Result<BackendMessage, DbError> {
    if payload.len() < 2 {
        return Err(DbError::PgProtocol("Truncated DataRow message".into()));
    }
    let num_cols = i16::from_be_bytes([payload[0], payload[1]]) as usize;
    let mut offset = 2;
    let mut values = Vec::with_capacity(num_cols);

    for _ in 0..num_cols {
        if offset + 4 > payload.len() {
            return Err(DbError::PgProtocol(
                "Truncated column length in DataRow".into(),
            ));
        }
        let col_len = i32::from_be_bytes(payload[offset..offset + 4].try_into().unwrap());
        offset += 4;

        if col_len == -1 {
            values.push(None);
        } else {
            let ulen = col_len as usize;
            if offset + ulen > payload.len() {
                return Err(DbError::PgProtocol(
                    "Truncated column value in DataRow".into(),
                ));
            }
            values.push(Some(payload[offset..offset + ulen].to_vec()));
            offset += ulen;
        }
    }

    Ok(BackendMessage::DataRow(DataRow { values }))
}

fn parse_error_response(payload: &[u8]) -> Result<BackendMessage, DbError> {
    let mut map = HashMap::new();
    let mut offset = 0;
    while offset < payload.len() {
        let field_type = payload[offset];
        if field_type == 0 {
            break;
        }
        offset += 1;
        let (val, next) = read_null_terminated_string(payload, offset)?;
        map.insert(field_type, val);
        offset = next;
    }
    Ok(BackendMessage::ErrorResponse(map))
}

fn parse_notice_response(payload: &[u8]) -> Result<BackendMessage, DbError> {
    let mut map = HashMap::new();
    let mut offset = 0;
    while offset < payload.len() {
        let field_type = payload[offset];
        if field_type == 0 {
            break;
        }
        offset += 1;
        let (val, next) = read_null_terminated_string(payload, offset)?;
        map.insert(field_type, val);
        offset = next;
    }
    Ok(BackendMessage::NoticeResponse(map))
}

fn read_null_terminated_string(bytes: &[u8], start: usize) -> Result<(String, usize), DbError> {
    let mut end = start;
    while end < bytes.len() && bytes[end] != 0 {
        end += 1;
    }
    if end >= bytes.len() {
        return Err(DbError::PgProtocol(
            "Unterminated string in wire payload".into(),
        ));
    }
    let s = String::from_utf8_lossy(&bytes[start..end]).to_string();
    Ok((s, end + 1))
}
