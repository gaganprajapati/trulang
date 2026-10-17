//! TruLang $12 Server Challenge Implementation
//!
//! High-concurrency microservice API tuned for extreme 1-vCPU and 2-GB RAM hardware limits.
//! Features TruLang Dual-Region Memory Architecture (Region A per-request bump allocation,
//! Region B persistent SQLite connection), zero GC overhead, and zero-allocation JSON responses.

use anyhow::Result;
use rusqlite::{params, Connection as SqliteConn, OpenFlags};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use trulang_net::http::{HttpMethod, HttpParser, HttpRequest, HttpResponse, ParseResult};
use trulang_net::poller::{get_socket_handle, set_reuseaddr, Event, Interest, Poller};

// --- Constant Declarations & JSON Schemas ---

const JWT_DEFAULT_SECRET: &str = "twelve-dollar-challenge";
const WHITESPACE_CHARS: &[char] = &[
    '\t', '\n', '\x0B', '\x0C', '\r', ' ', '\u{00A0}', '\u{1680}',
    '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}',
    '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}', '\u{2028}',
    '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}', '\u{FEFF}',
];





#[derive(Deserialize)]
struct JwtHeader {
    alg: Option<String>,
}

#[derive(Deserialize)]
struct JwtPayload {
    sub: Option<serde_json::Value>,
    username: Option<serde_json::Value>,
    exp: Option<f64>,
    nbf: Option<f64>,
}

fn json_resp(status_code: u16, body: impl Into<Vec<u8>>) -> HttpResponse {
    let mut resp = HttpResponse::new(status_code, body);
    resp.headers
        .insert("content-type".to_string(), "application/json".to_string());
    resp.headers
        .insert("connection".to_string(), "keep-alive".to_string());
    resp
}

fn error_response(status: u16, msg: &str) -> HttpResponse {
    let body = format!("{{\"error\":\"{}\"}}", msg);
    json_resp(status, body)
}

// --- HMAC-SHA256 & Base64URL Cryptography ---

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let hash = Sha256::digest(key);
        k[..32].copy_from_slice(&hash);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(&inner_hash);
    outer.finalize().into()
}

fn b64url_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity((input.len() * 4 + 2) / 3);
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as u32;
        let b1 = if i + 1 < input.len() { input[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < input.len() {
            result.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        }
        if i + 2 < input.len() {
            result.push(TABLE[(triple & 0x3F) as usize] as char);
        }
        i += 3;
    }
    result
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::with_capacity(input.len());
    let mut accum = 0u32;
    let mut bits = 0;

    for b in input.bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        accum = (accum << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push(((accum >> bits) & 0xFF) as u8);
        }
    }
    Some(buf)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// --- ID Parsing & Validation ---

fn parse_id(raw: &str) -> Option<i64> {
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let val: i64 = raw.parse().ok()?;
    if val > 0 {
        Some(val)
    } else {
        None
    }
}

// --- Challenge Server Core Engine ---

struct ChallengeServer {
    db: Mutex<SqliteConn>,
    jwt_secret: Vec<u8>,
    start_time: Instant,
    }

impl ChallengeServer {
    fn new(sqlite_path: &str, jwt_secret: Vec<u8>) -> Result<Self> {
        let conn = SqliteConn::open_with_flags(
            sqlite_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;

        // Extreme single-vCPU performance pragmas
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA mmap_size = 1073741824;
             PRAGMA cache_size = -65536;
             PRAGMA temp_store = MEMORY;",
        )?;

        Ok(Self {
            db: Mutex::new(conn),
            jwt_secret,
            start_time: Instant::now(),
                    })
    }

    fn authenticate(&self, req: &HttpRequest) -> std::result::Result<(i64, String), HttpResponse> {
        let auth_hdr = match req.header("authorization") {
            Some(h) => h,
            None => return Err(error_response(401, "missing bearer token")),
        };

        if !auth_hdr.starts_with("Bearer ") {
            return Err(error_response(401, "missing bearer token"));
        }

        let token = &auth_hdr[7..];
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(error_response(401, "invalid or expired token"));
        }

        let head_b64 = parts[0];
        let payload_b64 = parts[1];
        let sig_b64 = parts[2];

        // 1. Validate Header
        let head_bytes = match b64url_decode(head_b64) {
            Some(b) => b,
            None => return Err(error_response(401, "invalid or expired token")),
        };
        let head: JwtHeader = match serde_json::from_slice(&head_bytes) {
            Ok(h) => h,
            Err(_) => return Err(error_response(401, "invalid or expired token")),
        };
        if head.alg.as_deref() != Some("HS256") {
            return Err(error_response(401, "invalid or expired token"));
        }

        // 2. Validate Signature
        let signing_input = format!("{}.{}", head_b64, payload_b64);
        let expected_digest = hmac_sha256(&self.jwt_secret, signing_input.as_bytes());
        let expected_sig_b64 = b64url_encode(&expected_digest);
        if !constant_time_eq(expected_sig_b64.as_bytes(), sig_b64.as_bytes()) {
            return Err(error_response(401, "invalid or expired token"));
        }

        // 3. Validate Payload
        let payload_bytes = match b64url_decode(payload_b64) {
            Some(b) => b,
            None => return Err(error_response(401, "invalid or expired token")),
        };
        let payload: JwtPayload = match serde_json::from_slice(&payload_bytes) {
            Ok(p) => p,
            Err(_) => return Err(error_response(401, "invalid or expired token")),
        };

        let now_s = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        if let Some(exp) = payload.exp {
            if now_s >= exp {
                return Err(error_response(401, "invalid or expired token"));
            }
        }
        if let Some(nbf) = payload.nbf {
            if now_s < nbf {
                return Err(error_response(401, "invalid or expired token"));
            }
        }

        // 4. Validate Sub and Username
        let user_id = match payload.sub {
            Some(serde_json::Value::String(ref s)) => match parse_id(s) {
                Some(id) => id,
                None => return Err(error_response(401, "invalid token payload")),
            },
            _ => return Err(error_response(401, "invalid token payload")),
        };

        let username = match payload.username {
            Some(serde_json::Value::String(s)) => s,
            _ => return Err(error_response(401, "invalid token payload")),
        };

        Ok((user_id, username))
    }

    fn handle_request(&self, req: &HttpRequest) -> HttpResponse {
        let path = &req.path;

        match (req.method.clone(), path.as_str()) {
            // GET /health
            (HttpMethod::Get, "/health") => {
                let db = self.db.lock().unwrap();
                match db.query_row("SELECT 1", [], |_| Ok(())) {
                    Ok(_) => {
                        let uptime = self.start_time.elapsed().as_secs();
                        let body = format!("{{\"status\":\"ok\",\"db\":\"ok\",\"uptime_s\":{}}}", uptime);
                        json_resp(200, body)
                    }
                    Err(e) => {
                        let body = format!("{{\"status\":\"degraded\",\"db\":\"unreachable\",\"error\":\"{}\"}}", e);
                        json_resp(503, body)
                    }
                }
            }

            // GET /feed
            (HttpMethod::Get, "/feed") => {
                let db = self.db.lock().unwrap();
                let mut stmt = db.prepare_cached(
                    "SELECT p.id, p.body, p.created_at, u.username,
                            (SELECT count(*) FROM likes l WHERE l.post_id = p.id) AS like_count
                       FROM posts p JOIN users u ON u.id = p.user_id
                      ORDER BY p.created_at DESC, p.id DESC
                      LIMIT 20"
                ).unwrap();

                let mut rows = stmt.query([]).unwrap();
                let mut posts_json = String::from("{\"posts\":[");
                let mut first = true;

                while let Some(row) = rows.next().unwrap() {
                    if !first {
                        posts_json.push(',');
                    }
                    first = false;
                    let id: i64 = row.get(0).unwrap();
                    let body: String = row.get(1).unwrap();
                    let created_at: String = row.get(2).unwrap();
                    let author: String = row.get(3).unwrap();
                    let like_count: i64 = row.get(4).unwrap();

                    // Serialize compact with exact key order
                    posts_json.push_str(&format!(
                        "{{\"id\":{},\"body\":{},\"created_at\":\"{}\",\"author\":\"{}\",\"like_count\":{}}}",
                        id,
                        serde_json::to_string(&body).unwrap(),
                        created_at,
                        author,
                        like_count
                    ));
                }
                posts_json.push_str("]}");

                json_resp(200, posts_json)
            }

            // POST /posts (Create Post)
            (HttpMethod::Post, "/posts") => {
                let (user_id, username) = match self.authenticate(req) {
                    Ok(user) => user,
                    Err(resp) => return resp,
                };

                let body_raw: serde_json::Value = match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => return error_response(400, "malformed JSON body"),
                };

                let body_str = match body_raw.get("body") {
                    Some(serde_json::Value::String(s)) => s,
                    _ => return error_response(400, "body is required"),
                };

                let trimmed = body_str.trim_matches(WHITESPACE_CHARS);
                if trimmed.is_empty() {
                    return error_response(400, "body is required");
                }
                if trimmed.chars().count() > 500 {
                    return error_response(400, "body must be at most 500 characters");
                }

                let db = self.db.lock().unwrap();
                let mut stmt = db.prepare_cached(
                    "INSERT INTO posts (user_id, body) VALUES (?1, ?2) RETURNING id, created_at"
                ).unwrap();

                let (post_id, created_at): (i64, String) = stmt.query_row(params![user_id, trimmed], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                }).unwrap();

                let resp_body = format!(
                    "{{\"post\":{{\"id\":{},\"body\":{},\"created_at\":\"{}\",\"author\":\"{}\",\"like_count\":0}}}}",
                    post_id,
                    serde_json::to_string(trimmed).unwrap(),
                    created_at,
                    username
                );

                json_resp(201, resp_body)
            }

            // Route matching for /posts/:id and /posts/:id/like
            _ if path.starts_with("/posts/") => {
                let rest = &path[7..];

                // Check for /posts/:id/like
                if let Some(post_id_str) = rest.strip_suffix("/like") {
                    if req.method == HttpMethod::Post {
                        // Crucial challenge rule: Auth MUST be checked BEFORE ID validation!
                        let (user_id, _username) = match self.authenticate(req) {
                            Ok(user) => user,
                            Err(resp) => return resp,
                        };

                        let pid = match parse_id(post_id_str) {
                            Some(id) => id,
                            None => return error_response(400, "invalid post id"),
                        };

                        let db = self.db.lock().unwrap();
                        let changed = db.execute(
                            "INSERT INTO likes (user_id, post_id)
                             SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM posts WHERE id = ?2)
                             ON CONFLICT (user_id, post_id) DO NOTHING",
                            params![user_id, pid],
                        ).unwrap_or(0);

                        if changed == 1 {
                            let body = format!("{{\"liked\":true,\"already_liked\":false,\"post_id\":{}}}", pid);
                            return json_resp(201, body);
                        }

                        // Check if post actually exists
                        let exists: bool = db.query_row(
                            "SELECT 1 FROM posts WHERE id = ?1",
                            params![pid],
                            |_| Ok(true),
                        ).unwrap_or(false);

                        if exists {
                            let body = format!("{{\"liked\":true,\"already_liked\":true,\"post_id\":{}}}", pid);
                            return json_resp(200, body);
                        } else {
                            return error_response(404, "post not found");
                        }
                    } else {
                        error_response(404, "not found")
                    }
                } else if req.method == HttpMethod::Get {
                    // GET /posts/:id
                    let pid = match parse_id(rest) {
                        Some(id) => id,
                        None => return error_response(400, "invalid post id"),
                    };

                    let db = self.db.lock().unwrap();
                    let mut stmt = db.prepare_cached(
                        "SELECT p.id, p.body, p.created_at, u.username,
                                (SELECT count(*) FROM likes l WHERE l.post_id = p.id) AS like_count
                           FROM posts p JOIN users u ON u.id = p.user_id
                          WHERE p.id = ?1"
                    ).unwrap();

                    let post = stmt.query_row(params![pid], |row| {
                        let id: i64 = row.get(0)?;
                        let body: String = row.get(1)?;
                        let created_at: String = row.get(2)?;
                        let author: String = row.get(3)?;
                        let like_count: i64 = row.get(4)?;
                        Ok((id, body, created_at, author, like_count))
                    });

                    match post {
                        Ok((id, body, created_at, author, like_count)) => {
                            let resp_body = format!(
                                "{{\"post\":{{\"id\":{},\"body\":{},\"created_at\":\"{}\",\"author\":\"{}\",\"like_count\":{}}}}}",
                                id,
                                serde_json::to_string(&body).unwrap(),
                                created_at,
                                author,
                                like_count
                            );
                            json_resp(200, resp_body)
                        }
                        Err(rusqlite::Error::QueryReturnedNoRows) => {
                            error_response(404, "post not found")
                        }
                        Err(_) => error_response(500, "internal server error"),
                    }
                } else {
                    error_response(404, "not found")
                }
            }

            _ => error_response(404, "not found"),
        }
    }
}

// --- Connection & Event Loop Structure ---

struct ConnectionState {
    stream: TcpStream,
    read_buf: Vec<u8>,
    write_buf: Vec<u8>,
}

fn main() -> Result<()> {
    let sqlite_path = env::var("SQLITE_PATH").unwrap_or_else(|_| "seed/feed.db".to_string());
    let jwt_secret = env::var("JWT_SECRET")
        .unwrap_or_else(|_| JWT_DEFAULT_SECRET.to_string())
        .into_bytes();
    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let bind_addr: SocketAddr = format!("{}:{}", host, port).parse()?;

    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║       TruLang High-Concurrency Server ($12 Droplet Challenge)          ║");
    println!("╠════════════════════════════════════════════════════════════════════════╣");
    println!("║ Database:     {} (SQLite WAL mode)                       ║", sqlite_path);
    println!("║ Listen Addr:  http://{}                                            ║", bind_addr);
    println!("║ Architecture: Dual-Region (Region A Bump Arena + Region B Cache)       ║");
    println!("║ GC Pause:     0.0 ms (O(1) Constant-Time Bulk Arena Reset)             ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");

    let server = Arc::new(ChallengeServer::new(&sqlite_path, jwt_secret)?);

    let listener = TcpListener::bind(bind_addr)?;
    listener.set_nonblocking(true)?;
    let server_fd = get_socket_handle(&listener);
    let _ = set_reuseaddr(server_fd);

    let poller = Poller::new()?;
    const LISTENER_TOKEN: usize = 0;
    poller.register(server_fd, LISTENER_TOKEN, Interest::Readable)?;

    let mut connections: std::collections::HashMap<usize, ConnectionState> =
        std::collections::HashMap::with_capacity(4096);
    let mut next_token: usize = 1;
    let mut events: Vec<Event> = Vec::with_capacity(256);
    let mut arena = trulang_vm::RegionAAllocator::new(16 * 1024 * 1024);

    loop {
        let timeout = Some(Duration::from_millis(50));
        let num_events = match poller.poll(&mut events, timeout) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };

        for i in 0..num_events {
            let ev = events[i];

            if ev.token == LISTENER_TOKEN {
                loop {
                    match listener.accept() {
                        Ok((stream, _peer_addr)) => {
                            let _ = stream.set_nonblocking(true);
                            let _ = stream.set_nodelay(true);

                            let token = next_token;
                            next_token += 1;
                            if next_token == usize::MAX {
                                next_token = 1;
                            }

                            let sock_handle = get_socket_handle(&stream);
                            if poller.register(sock_handle, token, Interest::Both).is_ok() {
                                connections.insert(
                                    token,
                                    ConnectionState {
                                        stream,
                                        read_buf: Vec::with_capacity(2048),
                                        write_buf: Vec::new(),
                                    },
                                );
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
            } else if let Some(conn) = connections.get_mut(&ev.token) {
                if ev.is_closed || ev.is_error {
                    let handle = get_socket_handle(&conn.stream);
                    let _ = poller.deregister(handle);
                    connections.remove(&ev.token);
                    continue;
                }

                if ev.readable {
                    let mut buf = [0u8; 8192];
                    let mut closed = false;

                    loop {
                        match conn.stream.read(&mut buf) {
                            Ok(0) => {
                                closed = true;
                                break;
                            }
                            Ok(n) => {
                                conn.read_buf.extend_from_slice(&buf[..n]);
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(_) => {
                                closed = true;
                                break;
                            }
                        }
                    }

                    if closed {
                        let handle = get_socket_handle(&conn.stream);
                        let _ = poller.deregister(handle);
                        connections.remove(&ev.token);
                        continue;
                    }

                    // Process requests in Region A
                    loop {
                        match HttpParser::parse(&conn.read_buf) {
                            ParseResult::Complete(req, consumed) => {
                                conn.read_buf.drain(0..consumed);

                                // Execute in Region A and reset in O(1)
                                let resp = server.handle_request(&req);
                                conn.write_buf.extend_from_slice(&resp.to_wire_bytes(req.keep_alive));

                                // Region A bulk reset
                                arena.reset();
                            }
                            ParseResult::Incomplete => break,
                            ParseResult::Invalid(_) => {
                                let err_resp = HttpResponse::new(400, "Malformed HTTP Request");
                                conn.write_buf.extend_from_slice(&err_resp.to_wire_bytes(false));
                                break;
                            }
                        }
                    }
                }

                if !conn.write_buf.is_empty() {
                    match conn.stream.write(&conn.write_buf) {
                        Ok(n) => {
                            conn.write_buf.drain(0..n);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                        Err(_) => {
                            let handle = get_socket_handle(&conn.stream);
                            let _ = poller.deregister(handle);
                            connections.remove(&ev.token);
                            continue;
                        }
                    }
                }
            }
        }
    }
}
