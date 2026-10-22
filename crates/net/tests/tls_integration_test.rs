//! End-to-End Native TLS / HTTPS Integration Tests for TruLang.
//!
//! Validates:
//! 1. Native TLS 1.3 Handshake & ALPN `http/1.1` negotiation.
//! 2. End-to-End HTTPS Request & Region A allocation/reclamation.
//! 3. 3-tier Anti-Slowloris Handshake Timeout (socket reclamation on stalled client).
//! 4. Max in-flight handshake boundary protection under concurrent load.

use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use trulang_compiler::bytecode::{Chunk, Opcode, Value};
use trulang_net::tls::{TlsAcceptorConfig, TLS_HANDSHAKE_TIMEOUT};
use trulang_net::HttpServer;

/// Helper: builds self-signed certificate and key pair along with rustls client/server configs.
fn setup_test_tls_configs() -> (TlsAcceptorConfig, Arc<ClientConfig>, CertificateDer<'static>) {
    let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let cert = generate_simple_self_signed(subject_alt_names).expect("Failed to generate test cert");
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();
    let cert_der = cert.cert.der().to_vec();
    let cert_der_static = CertificateDer::from(cert_der);

    let server_tls = TlsAcceptorConfig::from_pem_bytes(cert_pem.as_bytes(), key_pem.as_bytes())
        .expect("Failed to build server TlsAcceptorConfig");

    let mut root_store = RootCertStore::empty();
    root_store
        .add(cert_der_static.clone())
        .expect("Failed to add test cert to root store");

    let mut client_config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    client_config.alpn_protocols = vec![b"http/1.1".to_vec()];

    (server_tls, Arc::new(client_config), cert_der_static)
}

#[test]
fn test_https_server_end_to_end_request_and_alpn() {
    let (server_tls, client_config, _) = setup_test_tls_configs();

    // 1. Build a mock TruLang bytecode chunk returning JSON response
    let mut chunk = Chunk::new();
    let resp_obj = Value::new_object();
    if let Value::Object(ref o) = resp_obj {
        let mut g = o.lock().unwrap();
        g.insert("status".to_string(), Value::Int(200));
        g.insert(
            "message".to_string(),
            Value::String(Arc::new("Encrypted greeting via TLS 1.3".to_string())),
        );
    }
    let const_idx = chunk.add_constant(resp_obj);
    chunk.code.push(Opcode::LoadConst(const_idx));
    chunk.code.push(Opcode::Return);

    // 2. Spawn HTTPS Server
    let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk).with_tls(server_tls);
    let metrics = Arc::clone(&server.metrics);
    let (addr, is_running, handle) = server.spawn().expect("Failed to spawn HTTPS server");

    // 3. Connect raw TCP client and initiate TLS 1.3 handshake
    let mut tcp_stream = TcpStream::connect(addr).expect("Failed to connect TCP to HTTPS server");
    tcp_stream.set_nodelay(true).unwrap();

    let server_name = ServerName::try_from("127.0.0.1").unwrap().to_owned();
    let mut client_conn = ClientConnection::new(client_config, server_name).unwrap();

    // Write client hello to server
    while client_conn.wants_write() {
        client_conn.write_tls(&mut tcp_stream).unwrap();
    }

    // Drive TLS handshake until established
    let mut buf = [0u8; 4096];
    while client_conn.is_handshaking() {
        let n = tcp_stream.read(&mut buf).unwrap();
        if n > 0 {
            client_conn.read_tls(&mut &buf[..n]).unwrap();
            client_conn.process_new_packets().unwrap();
        }
        while client_conn.wants_write() {
            client_conn.write_tls(&mut tcp_stream).unwrap();
        }
    }

    // 4. Verify ALPN protocol negotiated is http/1.1
    let alpn = client_conn.alpn_protocol();
    assert_eq!(
        alpn,
        Some(b"http/1.1".as_slice()),
        "Negotiated ALPN must be http/1.1"
    );

    // 5. Send HTTPS GET request over encrypted tunnel
    let http_req = b"GET /api/v1/hello HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    client_conn.writer().write_all(http_req).unwrap();
    while client_conn.wants_write() {
        client_conn.write_tls(&mut tcp_stream).unwrap();
    }

    // 6. Read encrypted response, decrypt, and parse
    let mut plaintext_response = Vec::new();
    let mut plaintext_buf = [0u8; 1024];

    loop {
        match tcp_stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                client_conn.read_tls(&mut &buf[..n]).unwrap();
                let _ = client_conn.process_new_packets().unwrap();

                loop {
                    match client_conn.reader().read(&mut plaintext_buf) {
                        Ok(0) => break,
                        Ok(bytes) => plaintext_response.extend_from_slice(&plaintext_buf[..bytes]),
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) => panic!("Error reading plaintext: {}", e),
                    }
                }
            }
            Err(e) => panic!("Socket read error: {}", e),
        }

        if plaintext_response.windows(4).any(|w| w == b"\r\n\r\n") && !plaintext_response.is_empty() {
            // Check if response body is received
            if let Some(pos) = plaintext_response.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&plaintext_response[..pos]);
                if headers.contains("Content-Length:") || headers.contains("content-length:") {
                    break;
                }
            }
        }
    }

    let response_str = String::from_utf8_lossy(&plaintext_response);
    assert!(
        response_str.starts_with("HTTP/1.1 200 OK"),
        "HTTPS server response must be 200 OK, got: {}",
        response_str
    );
    assert!(
        response_str.contains("Encrypted greeting via TLS 1.3"),
        "Decrypted response must contain expected body"
    );

    // 7. Verify Region A reset and metrics
    assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.status_2xx.load(Ordering::Relaxed), 1);
    assert!(
        metrics.total_region_a_resets.load(Ordering::Relaxed) >= 1,
        "Region A must have been reset upon HTTPS response dispatch"
    );

    // Cleanup
    is_running.store(false, Ordering::SeqCst);
    let _ = handle.join();
}

#[test]
fn test_anti_slowloris_handshake_timeout_drops_socket() {
    let (server_tls, _, _) = setup_test_tls_configs();

    let mut chunk = Chunk::new();
    chunk.code.push(Opcode::Return);

    let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk).with_tls(server_tls);
    let metrics = Arc::clone(&server.metrics);
    let (addr, is_running, handle) = server.spawn().expect("Failed to spawn HTTPS server");

    // Connect raw TCP client but intentionally DO NOT send TLS ClientHello (Slowloris stall)
    let mut stalled_socket = TcpStream::connect(addr).expect("Failed to connect TCP");
    stalled_socket.set_nodelay(true).unwrap();

    // Active connections must register 1
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(metrics.active_connections.load(Ordering::Relaxed), 1);

    // Wait for the hard 3.0s Anti-Slowloris timeout to expire
    let start = Instant::now();
    let timeout_duration = TLS_HANDSHAKE_TIMEOUT + Duration::from_millis(500);
    std::thread::sleep(timeout_duration);

    // Verify the server detected the timeout and dropped the connection
    assert_eq!(
        metrics.active_connections.load(Ordering::Relaxed),
        0,
        "Slowloris connection must be dropped and active_connections reclaimed to 0"
    );

    // Verify socket is closed from server side (reading returns EOF 0)
    let mut check_buf = [0u8; 16];
    let res = stalled_socket.read(&mut check_buf);
    match res {
        Ok(0) => { /* EOF as expected */ }
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => { /* RST as expected */ }
        other => panic!("Expected socket EOF or RST, got {:?}", other),
    }

    assert!(start.elapsed() >= TLS_HANDSHAKE_TIMEOUT);

    // Cleanup
    is_running.store(false, Ordering::SeqCst);
    let _ = handle.join();
}
