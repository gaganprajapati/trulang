use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use trulang_net::{
    close_code, compute_accept_key, HttpResponse, HttpServer, SseStream, WebApp, WsOpcode,
};

fn encode_masked_client_frame(opcode: WsOpcode, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x80 | (opcode as u8));

    let len = payload.len();
    if len <= 125 {
        out.push(0x80 | (len as u8));
    } else if len <= 65535 {
        out.push(0x80 | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }

    out.extend_from_slice(&mask);
    for (i, &b) in payload.iter().enumerate() {
        out.push(b ^ mask[i % 4]);
    }
    out
}

fn read_exact_bytes(stream: &mut TcpStream, count: usize) -> Vec<u8> {
    let mut buf = vec![0u8; count];
    stream
        .read_exact(&mut buf)
        .expect("Failed to read exact bytes from socket");
    buf
}

fn read_server_frame(stream: &mut TcpStream) -> (WsOpcode, Vec<u8>) {
    let header = read_exact_bytes(stream, 2);
    let opcode_num = header[0] & 0x0F;
    let opcode = match opcode_num {
        0x1 => WsOpcode::Text,
        0x2 => WsOpcode::Binary,
        0x8 => WsOpcode::Close,
        0x9 => WsOpcode::Ping,
        0xA => WsOpcode::Pong,
        _ => panic!("Unknown opcode: {}", opcode_num),
    };

    let len_indicator = (header[1] & 0x7F) as usize;
    let payload_len = if len_indicator <= 125 {
        len_indicator
    } else if len_indicator == 126 {
        let len_bytes = read_exact_bytes(stream, 2);
        u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize
    } else {
        let len_bytes = read_exact_bytes(stream, 8);
        u64::from_be_bytes(len_bytes.try_into().unwrap()) as usize
    };

    let payload = if payload_len > 0 {
        read_exact_bytes(stream, payload_len)
    } else {
        Vec::new()
    };

    (opcode, payload)
}

fn read_http_headers(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while let Ok(1) = stream.read(&mut byte) {
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(buf).expect("Valid HTTP headers string")
}

#[test]
fn test_websocket_echo_and_handshake() {
    let mut app = WebApp::new();
    let message_received = Arc::new(AtomicBool::new(false));
    let msg_flag = Arc::clone(&message_received);

    app.ws("/api/v1/chat", move |ws| {
        let flag = Arc::clone(&msg_flag);
        ws.on_open(|socket, req| {
            assert_eq!(req.path, "/api/v1/chat");
            socket.send_text("Welcome to TruLang WS");
        });
        ws.on_message(move |socket, frame| {
            if let Ok(text) = std::str::from_utf8(&frame.payload) {
                flag.store(true, Ordering::SeqCst);
                socket.send_text(format!("Echo: {}", text));
            }
        });
    });

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = HttpServer::from_app(addr, app);
    let (bound_addr, is_running, handle) = server.spawn().expect("Failed to spawn HTTP server");

    let mut client = TcpStream::connect(bound_addr).expect("Failed to connect to server");
    let client_key = "dGhlIHNhbXBsZSBub25jZQ==";
    let expected_accept = compute_accept_key(client_key);

    let upgrade_req = format!(
        "GET /api/v1/chat HTTP/1.1\r\n\
         Host: localhost\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n",
        client_key
    );

    client
        .write_all(upgrade_req.as_bytes())
        .expect("Failed to send upgrade request");

    let headers = read_http_headers(&mut client);
    assert!(headers.starts_with("HTTP/1.1 101 Switching Protocols"));
    assert!(headers
        .to_ascii_lowercase()
        .contains(&format!("sec-websocket-accept: {}", expected_accept).to_ascii_lowercase()));

    // 1. Read welcome message sent in on_open
    let (opcode, payload) = read_server_frame(&mut client);
    assert_eq!(opcode, WsOpcode::Text);
    assert_eq!(
        std::str::from_utf8(&payload).unwrap(),
        "Welcome to TruLang WS"
    );

    // 2. Send masked client message
    let mask = [0x12, 0x34, 0x56, 0x78];
    let msg = b"Hello TruLang WebSocket!";
    let client_frame = encode_masked_client_frame(WsOpcode::Text, msg, mask);
    client
        .write_all(&client_frame)
        .expect("Failed to write client frame");

    // 3. Receive echo message
    let (opcode, payload) = read_server_frame(&mut client);
    assert_eq!(opcode, WsOpcode::Text);
    assert_eq!(
        std::str::from_utf8(&payload).unwrap(),
        "Echo: Hello TruLang WebSocket!"
    );
    assert!(message_received.load(Ordering::SeqCst));

    // 4. Send client Close frame
    let mut close_payload = Vec::new();
    close_payload.extend_from_slice(&close_code::NORMAL_CLOSURE.to_be_bytes());
    close_payload.extend_from_slice(b"Done");
    let close_frame = encode_masked_client_frame(WsOpcode::Close, &close_payload, mask);
    client
        .write_all(&close_frame)
        .expect("Failed to write close frame");

    // 5. Receive reflected server Close frame
    let (opcode, payload) = read_server_frame(&mut client);
    assert_eq!(opcode, WsOpcode::Close);
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    assert_eq!(code, close_code::NORMAL_CLOSURE);

    is_running.store(false, Ordering::SeqCst);
    let _ = handle.join();
}

#[test]
fn test_websocket_autonomous_ping_pong() {
    let mut app = WebApp::new();
    app.ws("/api/v1/ping-test", |_ws| {});

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = HttpServer::from_app(addr, app);
    let (bound_addr, is_running, handle) = server.spawn().expect("Failed to spawn HTTP server");

    let mut client = TcpStream::connect(bound_addr).expect("Failed to connect to server");
    let upgrade_req = "GET /api/v1/ping-test HTTP/1.1\r\n\
         Host: localhost\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n";

    client
        .write_all(upgrade_req.as_bytes())
        .expect("Failed to send upgrade request");
    let headers = read_http_headers(&mut client);
    assert!(headers.starts_with("HTTP/1.1 101 Switching Protocols"));

    // Send masked Ping frame
    let mask = [0xAA, 0xBB, 0xCC, 0xDD];
    let ping_payload = b"heartbeat_wire_test";
    let ping_frame = encode_masked_client_frame(WsOpcode::Ping, ping_payload, mask);
    client
        .write_all(&ping_frame)
        .expect("Failed to write ping frame");

    // Expect autonomous wire-level Pong reply with identical payload
    let (opcode, payload) = read_server_frame(&mut client);
    assert_eq!(opcode, WsOpcode::Pong);
    assert_eq!(&payload, ping_payload);

    is_running.store(false, Ordering::SeqCst);
    let _ = handle.join();
}

#[test]
fn test_websocket_unmasked_client_frame_rejected() {
    let mut app = WebApp::new();
    app.ws("/api/v1/reject", |_ws| {});

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = HttpServer::from_app(addr, app);
    let (bound_addr, is_running, handle) = server.spawn().expect("Failed to spawn HTTP server");

    let mut client = TcpStream::connect(bound_addr).expect("Failed to connect to server");
    let upgrade_req = "GET /api/v1/reject HTTP/1.1\r\n\
         Host: localhost\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n";

    client
        .write_all(upgrade_req.as_bytes())
        .expect("Failed to send upgrade request");
    let headers = read_http_headers(&mut client);
    assert!(headers.starts_with("HTTP/1.1 101 Switching Protocols"));

    // Send UNMASKED frame (illegal for client)
    let illegal_frame = vec![0x81, 0x05, b'H', b'e', b'l', b'l', b'o'];
    client
        .write_all(&illegal_frame)
        .expect("Failed to write illegal frame");

    // Expect Close frame with code 1002 (Protocol Error)
    let (opcode, payload) = read_server_frame(&mut client);
    assert_eq!(opcode, WsOpcode::Close);
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    assert_eq!(code, close_code::PROTOCOL_ERROR);

    is_running.store(false, Ordering::SeqCst);
    let _ = handle.join();
}

#[test]
fn test_sse_streaming_chunked_response() {
    let mut app = WebApp::new();
    app.get("/api/v1/live-feed", |_req, _params| {
        let mut stream = SseStream::new();
        let chunk1 = stream.send_event("init", r#"{"status":"connected"}"#);
        let chunk2 = stream.send_event("metric", r#"{"cpu":14.2}"#);

        let mut res = HttpResponse::sse();
        res.body.extend_from_slice(&chunk1);
        res.body.extend_from_slice(&chunk2);
        res.body.extend_from_slice(SseStream::terminate_chunk());
        res
    });

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = HttpServer::from_app(addr, app);
    let (bound_addr, is_running, handle) = server.spawn().expect("Failed to spawn HTTP server");

    let mut client = TcpStream::connect(bound_addr).expect("Failed to connect to server");
    client
        .write_all(
            b"GET /api/v1/live-feed HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .expect("Failed to send request");

    let mut response_buf = Vec::new();
    client
        .read_to_end(&mut response_buf)
        .expect("Failed to read response");
    let response_str = String::from_utf8(response_buf).expect("Valid UTF-8 response");

    assert!(response_str.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response_str.contains("Content-Type: text/event-stream"));
    assert!(response_str.contains("Transfer-Encoding: chunked"));
    assert!(response_str.contains("event: init\ndata: {\"status\":\"connected\"}\n\n"));
    assert!(response_str.contains("event: metric\ndata: {\"cpu\":14.2}\n\n"));

    is_running.store(false, Ordering::SeqCst);
    let _ = handle.join();
}

#[test]
fn test_websocket_graceful_drain_broadcast() {
    let mut app = WebApp::new();
    app.ws("/api/v1/drain-chat", |_ws| {});

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = HttpServer::from_app(addr, app);
    let (bound_addr, is_running, is_draining, handle) = server
        .spawn_with_drain()
        .expect("Failed to spawn HTTP server");

    let mut client = TcpStream::connect(bound_addr).expect("Failed to connect to server");
    let upgrade_req = "GET /api/v1/drain-chat HTTP/1.1\r\n\
         Host: localhost\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n";

    client
        .write_all(upgrade_req.as_bytes())
        .expect("Failed to send upgrade request");
    let headers = read_http_headers(&mut client);
    assert!(headers.starts_with("HTTP/1.1 101 Switching Protocols"));

    // Trigger graceful drain
    is_draining.store(true, Ordering::SeqCst);

    // Expect broadcast Close frame with code 1001 (Going Away)
    let (opcode, payload) = read_server_frame(&mut client);
    assert_eq!(opcode, WsOpcode::Close);
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    assert_eq!(code, close_code::GOING_AWAY);
    let reason = std::str::from_utf8(&payload[2..]).unwrap();
    assert_eq!(reason, "Server shutting down");

    is_running.store(false, Ordering::SeqCst);
    let _ = handle.join();
}
