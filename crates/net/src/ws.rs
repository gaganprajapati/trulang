//! Native WebSockets (RFC 6455) and Server-Sent Events (SSE) Engine.
//!
//! Provides:
//! - **RFC 6455 Upgrade Handshake**: SHA-1 + GUID handshake computation over HTTP/1.1.
//! - **Frame Parsing & Serialization**: Masked client frames, unmasked server frames,
//!   Text, Binary, Ping, Pong, and Close control frames.
//! - **Autonomous Wire Protocol**: Wire-level auto-reply to Ping with Pong,
//!   periodic 30s heartbeats, and 10s dead-peer pruning.
//! - **Write Backpressure**: 64 KB high-watermark pausing socket reads, 16 KB resume,
//!   and RFC 6455 code 1008 termination on budget breach.
//! - **Server-Sent Events (`SseStream`)**: Chunked transfer encoding, keep-alive comments,
//!   and per-event Region A arena reset semantics.

use crate::http::{HttpRequest, HttpResponse};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// RFC 6455 WebSocket GUID string.
pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Flow-control watermarks for write backpressure.
pub const WS_WRITE_HIGH_WATERMARK: usize = 64 * 1024; // 64 KB
pub const WS_WRITE_LOW_WATERMARK: usize = 16 * 1024; // 16 KB

/// Heartbeat and timeout intervals.
pub const WS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const WS_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);

/// RFC 6455 Close status codes.
pub mod close_code {
    pub const NORMAL_CLOSURE: u16 = 1000;
    pub const GOING_AWAY: u16 = 1001;
    pub const PROTOCOL_ERROR: u16 = 1002;
    pub const UNSUPPORTED_DATA: u16 = 1003;
    pub const ABNORMAL_CLOSURE: u16 = 1006;
    pub const POLICY_VIOLATION: u16 = 1008;
    pub const MESSAGE_TOO_BIG: u16 = 1009;
    pub const INTERNAL_ERROR: u16 = 1011;
}

/// Computes the RFC 6455 `Sec-WebSocket-Accept` value from client key.
pub fn compute_accept_key(key: &str) -> String {
    let mut ctx = Context::new(&SHA1_FOR_LEGACY_USE_ONLY);
    ctx.update(key.trim().as_bytes());
    ctx.update(WS_GUID.as_bytes());
    let digest = ctx.finish();
    BASE64_STANDARD.encode(digest.as_ref())
}

/// Checks if an incoming HTTP request is a valid RFC 6455 WebSocket upgrade request.
pub fn is_websocket_upgrade(req: &HttpRequest) -> bool {
    let has_upgrade_hdr = req
        .header("upgrade")
        .map(|s| s.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    let has_conn_upgrade = req
        .header("connection")
        .map(|s| s.to_ascii_lowercase().contains("upgrade"))
        .unwrap_or(false);

    let has_ws_key = req.header("sec-websocket-key").is_some();

    has_upgrade_hdr && has_conn_upgrade && has_ws_key
}

/// Generates the HTTP 101 Switching Protocols response for a valid WebSocket handshake.
pub fn create_upgrade_response(req: &HttpRequest) -> Option<HttpResponse> {
    let key = req.header("sec-websocket-key")?;
    let accept_key = compute_accept_key(key);

    Some(
        HttpResponse::new(101, "")
            .with_header("upgrade", "websocket")
            .with_header("connection", "Upgrade")
            .with_header("sec-websocket-accept", accept_key),
    )
}

/// WebSocket Opcode according to RFC 6455 section 5.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsOpcode {
    Continuation = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
}

impl WsOpcode {
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x0 => Some(WsOpcode::Continuation),
            0x1 => Some(WsOpcode::Text),
            0x2 => Some(WsOpcode::Binary),
            0x8 => Some(WsOpcode::Close),
            0x9 => Some(WsOpcode::Ping),
            0xA => Some(WsOpcode::Pong),
            _ => None,
        }
    }

    pub fn is_control(&self) -> bool {
        matches!(self, WsOpcode::Close | WsOpcode::Ping | WsOpcode::Pong)
    }
}

/// An RFC 6455 WebSocket Frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsFrame {
    pub fin: bool,
    pub opcode: WsOpcode,
    pub payload: Vec<u8>,
}

impl WsFrame {
    pub fn text(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            fin: true,
            opcode: WsOpcode::Text,
            payload: payload.into(),
        }
    }

    pub fn binary(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            fin: true,
            opcode: WsOpcode::Binary,
            payload: payload.into(),
        }
    }

    pub fn ping(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            fin: true,
            opcode: WsOpcode::Ping,
            payload: payload.into(),
        }
    }

    pub fn pong(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            fin: true,
            opcode: WsOpcode::Pong,
            payload: payload.into(),
        }
    }

    pub fn close(code: u16, reason: &str) -> Self {
        let mut payload = Vec::with_capacity(2 + reason.len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason.as_bytes());
        Self {
            fin: true,
            opcode: WsOpcode::Close,
            payload,
        }
    }

    /// Extracts close code and reason string if this is a Close frame.
    pub fn parse_close_payload(&self) -> (u16, String) {
        if self.payload.len() < 2 {
            (close_code::NORMAL_CLOSURE, String::new())
        } else {
            let code = u16::from_be_bytes([self.payload[0], self.payload[1]]);
            let reason = String::from_utf8_lossy(&self.payload[2..]).to_string();
            (code, reason)
        }
    }
}

/// WebSocket Parsing Errors.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WsError {
    #[error("Incomplete frame buffer")]
    Incomplete,
    #[error("Client frame was not masked (RFC 6455 violation)")]
    UnmaskedClientFrame,
    #[error("Invalid WebSocket opcode: {0}")]
    InvalidOpcode(u8),
    #[error("Control frame payload exceeded 125 bytes")]
    ControlFrameTooLarge,
    #[error("Fragmented control frame (RFC 6455 violation)")]
    FragmentedControlFrame,
    #[error("Reserved bits RSV1/RSV2/RSV3 set without extension")]
    ReservedBitsSet,
}

/// Parses an RFC 6455 frame from incoming network buffer.
/// Expects client frames to be masked.
pub fn parse_client_frame(buf: &[u8]) -> Result<(WsFrame, usize), WsError> {
    if buf.len() < 2 {
        return Err(WsError::Incomplete);
    }

    let b0 = buf[0];
    let b1 = buf[1];

    let fin = (b0 & 0x80) != 0;
    let rsv = b0 & 0x70;
    if rsv != 0 {
        return Err(WsError::ReservedBitsSet);
    }

    let opcode_byte = b0 & 0x0F;
    let opcode = WsOpcode::from_u8(opcode_byte).ok_or(WsError::InvalidOpcode(opcode_byte))?;

    if opcode.is_control() && !fin {
        return Err(WsError::FragmentedControlFrame);
    }

    let masked = (b1 & 0x80) != 0;
    if !masked {
        return Err(WsError::UnmaskedClientFrame);
    }

    let payload_len_code = b1 & 0x7F;
    let mut header_len = 2;

    let payload_len: usize = match payload_len_code {
        0..=125 => payload_len_code as usize,
        126 => {
            if buf.len() < 4 {
                return Err(WsError::Incomplete);
            }
            header_len += 2;
            u16::from_be_bytes([buf[2], buf[3]]) as usize
        }
        127 => {
            if buf.len() < 10 {
                return Err(WsError::Incomplete);
            }
            header_len += 8;
            u64::from_be_bytes([
                buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9],
            ]) as usize
        }
        _ => unreachable!(),
    };

    if opcode.is_control() && payload_len > 125 {
        return Err(WsError::ControlFrameTooLarge);
    }

    let mask_start = header_len;
    header_len += 4; // Masking key is 4 bytes

    if buf.len() < header_len + payload_len {
        return Err(WsError::Incomplete);
    }

    let mask = [
        buf[mask_start],
        buf[mask_start + 1],
        buf[mask_start + 2],
        buf[mask_start + 3],
    ];

    let raw_payload = &buf[header_len..header_len + payload_len];
    let mut unmasked_payload = Vec::with_capacity(payload_len);
    for (i, &byte) in raw_payload.iter().enumerate() {
        unmasked_payload.push(byte ^ mask[i % 4]);
    }

    let total_consumed = header_len + payload_len;
    Ok((
        WsFrame {
            fin,
            opcode,
            payload: unmasked_payload,
        },
        total_consumed,
    ))
}

/// Encodes an RFC 6455 frame from server to client (unmasked).
pub fn encode_server_frame(frame: &WsFrame) -> Vec<u8> {
    let mut out = Vec::with_capacity(10 + frame.payload.len());

    let mut b0 = frame.opcode as u8;
    if frame.fin {
        b0 |= 0x80;
    }
    out.push(b0);

    let len = frame.payload.len();
    if len <= 125 {
        out.push(len as u8); // Mask bit is 0
    } else if len <= u16::MAX as usize {
        out.push(126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }

    out.extend_from_slice(&frame.payload);
    out
}

// -----------------------------------------------------------------------------
// WebSocket Connection Actor Session
// -----------------------------------------------------------------------------

/// Shared thread-safe outgoing frame queue.
pub type WsQueue = Arc<Mutex<VecDeque<Vec<u8>>>>;

/// Thread-safe client connection handle for WebSocket handlers.
#[derive(Clone)]
pub struct WsSocket {
    pub id: usize,
    outgoing_queue: WsQueue,
    queued_bytes: Arc<AtomicUsize>,
    pub is_closed: Arc<AtomicBool>,
}

impl WsSocket {
    pub fn new(id: usize) -> (Self, WsQueue, Arc<AtomicUsize>) {
        let outgoing_queue = Arc::new(Mutex::new(VecDeque::new()));
        let queued_bytes = Arc::new(AtomicUsize::new(0));
        let socket = Self {
            id,
            outgoing_queue: Arc::clone(&outgoing_queue),
            queued_bytes: Arc::clone(&queued_bytes),
            is_closed: Arc::new(AtomicBool::new(false)),
        };
        (socket, outgoing_queue, queued_bytes)
    }

    /// Sends a UTF-8 text message.
    pub fn send_text(&self, text: impl AsRef<str>) -> bool {
        self.send_frame(WsFrame::text(text.as_ref().as_bytes()))
    }

    /// Sends a binary payload.
    pub fn send_binary(&self, data: impl Into<Vec<u8>>) -> bool {
        self.send_frame(WsFrame::binary(data))
    }

    /// Serializes and sends a JSON payload.
    pub fn send_json<T: serde::Serialize>(&self, val: &T) -> bool {
        if let Ok(json_str) = serde_json::to_string(val) {
            self.send_text(json_str)
        } else {
            false
        }
    }

    /// Initiates an RFC 6455 close handshake.
    pub fn close(&self, code: u16, reason: &str) {
        if !self.is_closed.swap(true, Ordering::SeqCst) {
            self.send_frame(WsFrame::close(code, reason));
        }
    }

    /// Queues an encoded frame for transmission.
    pub fn send_frame(&self, frame: WsFrame) -> bool {
        if self.is_closed.load(Ordering::SeqCst) && frame.opcode != WsOpcode::Close {
            return false;
        }

        let encoded = encode_server_frame(&frame);
        let len = encoded.len();
        self.queued_bytes.fetch_add(len, Ordering::Relaxed);
        let mut queue = self.outgoing_queue.lock().unwrap();
        queue.push_back(encoded);
        true
    }

    pub fn buffered_bytes(&self) -> usize {
        self.queued_bytes.load(Ordering::Relaxed)
    }
}

/// WebSocket Session state tracking backpressure and heartbeats.
pub struct WsSession {
    pub socket: WsSocket,
    pub outgoing_queue: WsQueue,
    pub queued_bytes: Arc<AtomicUsize>,
    pub last_heartbeat: Instant,
    pub awaiting_pong: bool,
    pub is_read_paused: bool,
    pub memory_budget_bytes: usize,
}

impl WsSession {
    pub fn new(id: usize, memory_budget_bytes: usize) -> Self {
        let (socket, outgoing_queue, queued_bytes) = WsSocket::new(id);
        Self {
            socket,
            outgoing_queue,
            queued_bytes,
            last_heartbeat: Instant::now(),
            awaiting_pong: false,
            is_read_paused: false,
            memory_budget_bytes,
        }
    }

    /// Checks if buffered outgoing data exceeds watermarks or memory budget.
    pub fn check_backpressure(&mut self) -> Result<bool, u16> {
        let buffered = self.queued_bytes.load(Ordering::Relaxed);

        // Budget violation check
        if buffered > self.memory_budget_bytes {
            return Err(close_code::POLICY_VIOLATION);
        }

        if buffered >= WS_WRITE_HIGH_WATERMARK {
            self.is_read_paused = true;
            Ok(true) // Pause POLLIN
        } else if self.is_read_paused && buffered <= WS_WRITE_LOW_WATERMARK {
            self.is_read_paused = false;
            Ok(false) // Resume POLLIN
        } else {
            Ok(self.is_read_paused)
        }
    }

    /// Handles autonomous incoming frame processing:
    /// - `Ping`: auto-replies with Pong carrying identical payload
    /// - `Pong`: clears heartbeat await flag
    /// - `Close`: reflects Close frame
    pub fn process_incoming_frame(&mut self, frame: &WsFrame) -> Option<WsFrame> {
        match frame.opcode {
            WsOpcode::Ping => {
                // Autonomous RFC 6455 auto-reply
                Some(WsFrame::pong(frame.payload.clone()))
            }
            WsOpcode::Pong => {
                self.awaiting_pong = false;
                None
            }
            WsOpcode::Close => {
                let (code, reason) = frame.parse_close_payload();
                self.socket.is_closed.store(true, Ordering::SeqCst);
                Some(WsFrame::close(code, &reason))
            }
            _ => None,
        }
    }

    /// Checks if a 30s heartbeat ping is due.
    pub fn check_heartbeat(&mut self) -> Option<WsFrame> {
        if self.awaiting_pong {
            if self.last_heartbeat.elapsed() >= WS_HEARTBEAT_TIMEOUT {
                // Heartbeat timeout: dead peer
                return Some(WsFrame::close(close_code::GOING_AWAY, "Heartbeat timeout"));
            }
        } else if self.last_heartbeat.elapsed() >= WS_HEARTBEAT_INTERVAL {
            self.awaiting_pong = true;
            self.last_heartbeat = Instant::now();
            return Some(WsFrame::ping(b"heartbeat".to_vec()));
        }
        None
    }
}

// -----------------------------------------------------------------------------
// WebSocket Handler Trait & Functional Listener
// -----------------------------------------------------------------------------

/// Trait for handling asynchronous WebSocket events.
pub trait WsHandler: Send + Sync {
    /// Invoked upon successful HTTP/1.1 -> RFC 6455 upgrade.
    fn on_open(&self, socket: &WsSocket, req: &HttpRequest) {
        let _ = (socket, req);
    }

    /// Invoked when a data frame (Text or Binary) is parsed.
    fn on_message(&self, socket: &WsSocket, frame: &WsFrame) {
        let _ = (socket, frame);
    }

    /// Invoked when a client Close frame is received or socket is terminated.
    fn on_close(&self, socket: &WsSocket, code: u16, reason: &str) {
        let _ = (socket, code, reason);
    }
}

pub type WsOpenCallback = Box<dyn Fn(&WsSocket, &HttpRequest) + Send + Sync>;
pub type WsMessageCallback = Box<dyn Fn(&WsSocket, &WsFrame) + Send + Sync>;
pub type WsCloseCallback = Box<dyn Fn(&WsSocket, u16, &str) + Send + Sync>;

/// Functional callback-based builder implementing `WsHandler`.
pub struct WsListener {
    pub(crate) on_open_fn: Option<WsOpenCallback>,
    pub(crate) on_message_fn: Option<WsMessageCallback>,
    pub(crate) on_close_fn: Option<WsCloseCallback>,
}

impl Default for WsListener {
    fn default() -> Self {
        Self::new()
    }
}

impl WsListener {
    pub fn new() -> Self {
        Self {
            on_open_fn: None,
            on_message_fn: None,
            on_close_fn: None,
        }
    }

    pub fn on_open<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&WsSocket, &HttpRequest) + Send + Sync + 'static,
    {
        self.on_open_fn = Some(Box::new(f));
        self
    }

    pub fn on_message<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&WsSocket, &WsFrame) + Send + Sync + 'static,
    {
        self.on_message_fn = Some(Box::new(f));
        self
    }

    pub fn on_close<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&WsSocket, u16, &str) + Send + Sync + 'static,
    {
        self.on_close_fn = Some(Box::new(f));
        self
    }
}

impl WsHandler for WsListener {
    fn on_open(&self, socket: &WsSocket, req: &HttpRequest) {
        if let Some(ref f) = self.on_open_fn {
            f(socket, req);
        }
    }

    fn on_message(&self, socket: &WsSocket, frame: &WsFrame) {
        if let Some(ref f) = self.on_message_fn {
            f(socket, frame);
        }
    }

    fn on_close(&self, socket: &WsSocket, code: u16, reason: &str) {
        if let Some(ref f) = self.on_close_fn {
            f(socket, code, reason);
        }
    }
}

// -----------------------------------------------------------------------------
// Server-Sent Events (SSE) Stream
// -----------------------------------------------------------------------------

/// Server-Sent Events Response Stream.
pub struct SseStream;

impl Default for SseStream {
    fn default() -> Self {
        Self::new()
    }
}

impl SseStream {
    pub fn new() -> Self {
        Self
    }

    /// Formats an SSE event and wraps it in an HTTP/1.1 chunked wire slice.
    pub fn send_event(&mut self, event: &str, data: &str) -> Vec<u8> {
        let mut payload = String::with_capacity(32 + event.len() + data.len());
        if !event.is_empty() {
            payload.push_str("event: ");
            payload.push_str(event);
            payload.push('\n');
        }
        payload.push_str("data: ");
        payload.push_str(data);
        payload.push_str("\n\n");

        Self::encode_chunk(payload.as_bytes())
    }

    /// Formats an SSE keep-alive comment (`: <comment>\n\n`) wrapped in a chunk.
    pub fn send_comment(&mut self, comment: &str) -> Vec<u8> {
        let mut payload = String::with_capacity(16 + comment.len());
        payload.push_str(": ");
        payload.push_str(comment);
        payload.push_str("\n\n");

        Self::encode_chunk(payload.as_bytes())
    }

    /// Generates the terminal zero-length chunk (`0\r\n\r\n`) to cleanly close SSE stream.
    pub fn terminate_chunk() -> &'static [u8] {
        b"0\r\n\r\n"
    }

    fn encode_chunk(payload: &[u8]) -> Vec<u8> {
        let len = payload.len();
        let hex_len = format!("{:X}\r\n", len);
        let mut chunk = Vec::with_capacity(hex_len.len() + len + 2);
        chunk.extend_from_slice(hex_len.as_bytes());
        chunk.extend_from_slice(payload);
        chunk.extend_from_slice(b"\r\n");
        chunk
    }
}

impl HttpResponse {
    /// Constructs an SSE response with chunked transfer encoding and event-stream headers.
    pub fn sse() -> Self {
        HttpResponse::new(200, "")
            .with_header("content-type", "text/event-stream")
            .with_header("cache-control", "no-cache")
            .with_header("connection", "keep-alive")
            .with_header("transfer-encoding", "chunked")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfc6455_accept_key_rfc_test_vector() {
        // RFC 6455 Section 1.2 Test Vector:
        // Key: "dGhlIHNhbXBsZSBub25jZQ=="
        // Accept: "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        let client_key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = compute_accept_key(client_key);
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn test_websocket_upgrade_detection_and_response() {
        let mut req = HttpRequest {
            method: crate::http::HttpMethod::Get,
            uri: "/chat".into(),
            path: "/chat".into(),
            query_params: Default::default(),
            headers: Default::default(),
            body: Vec::new(),
            keep_alive: true,
        };
        req.headers.insert("upgrade".into(), "websocket".into());
        req.headers.insert("connection".into(), "Upgrade".into());
        req.headers.insert(
            "sec-websocket-key".into(),
            "dGhlIHNhbXBsZSBub25jZQ==".into(),
        );
        req.headers
            .insert("sec-websocket-version".into(), "13".into());

        assert!(is_websocket_upgrade(&req));

        let res = create_upgrade_response(&req).expect("Failed to create upgrade response");
        assert_eq!(res.status_code, 101);
        assert_eq!(
            res.headers.get("upgrade").map(|s| s.as_str()),
            Some("websocket")
        );
        assert_eq!(
            res.headers.get("sec-websocket-accept").map(|s| s.as_str()),
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        );
    }

    #[test]
    fn test_ws_client_frame_parse_and_server_encode_roundtrip() {
        // 1. Manually craft masked client text frame: "Hello"
        // FIN=1, Opcode=1 (Text) -> 0x81
        // MASK=1, Len=5 -> 0x85
        // Mask key: [0x12, 0x34, 0x56, 0x78]
        // Payload: "Hello" = [0x48, 0x65, 0x6C, 0x6C, 0x6F]
        let mask = [0x12, 0x34, 0x56, 0x78];
        let raw = b"Hello";
        let mut masked_payload = Vec::new();
        for (i, &b) in raw.iter().enumerate() {
            masked_payload.push(b ^ mask[i % 4]);
        }

        let mut wire = vec![0x81, 0x85];
        wire.extend_from_slice(&mask);
        wire.extend_from_slice(&masked_payload);

        let (frame, consumed) = parse_client_frame(&wire).expect("Failed to parse client frame");
        assert_eq!(consumed, wire.len());
        assert_eq!(frame.opcode, WsOpcode::Text);
        assert!(frame.fin);
        assert_eq!(frame.payload, b"Hello");

        // 2. Encode as unmasked server frame
        let encoded = encode_server_frame(&frame);
        assert_eq!(encoded[0], 0x81);
        assert_eq!(encoded[1], 0x05); // unmasked length 5
        assert_eq!(&encoded[2..], b"Hello");
    }

    #[test]
    fn test_unmasked_client_frame_rejected_with_protocol_error() {
        // Unmasked client frame: bit 7 of byte 1 is 0
        let unmasked_wire = vec![0x81, 0x05, b'H', b'e', b'l', b'l', b'o'];
        let err = parse_client_frame(&unmasked_wire).unwrap_err();
        assert_eq!(err, WsError::UnmaskedClientFrame);
    }

    #[test]
    fn test_autonomous_ping_pong_reply() {
        let mut session = WsSession::new(1, 128 * 1024);
        let ping_frame = WsFrame::ping(b"nonce-123".to_vec());

        let reply = session.process_incoming_frame(&ping_frame);
        assert!(reply.is_some());
        let pong = reply.unwrap();
        assert_eq!(pong.opcode, WsOpcode::Pong);
        assert_eq!(pong.payload, b"nonce-123");
    }

    #[test]
    fn test_write_backpressure_and_budget_breach() {
        let mut session = WsSession::new(1, 64 * 1024); // 64 KB budget

        // 1. Initially no backpressure
        assert_eq!(session.check_backpressure(), Ok(false));

        // 2. Queue 70 KB -> exceeds 64 KB watermark
        let big_payload = vec![0u8; 70 * 1024];
        session.socket.send_binary(big_payload);

        // Should return policy violation because budget is 64 KB
        let res = session.check_backpressure();
        assert_eq!(res, Err(close_code::POLICY_VIOLATION));
    }

    #[test]
    fn test_sse_chunked_event_formatting() {
        let mut stream = SseStream::new();
        let chunk = stream.send_event("update", r#"{"count":42}"#);

        let chunk_str = String::from_utf8(chunk).unwrap();
        assert!(chunk_str.contains("event: update\n"));
        assert!(chunk_str.contains(r#"data: {"count":42}"#));
        assert!(chunk_str.ends_with("\r\n"));

        let term = SseStream::terminate_chunk();
        assert_eq!(term, b"0\r\n\r\n");
    }
}
