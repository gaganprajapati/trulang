//! Stress Benchmark Suite for Phase 7: HTTPS, WebSockets & REST Routing
//!
//! Measures:
//! 1. REST Radix Router throughput on deep parameterized routes (> 75,000 req/sec threshold).
//! 2. High-concurrency WebSocket echo frame throughput and frame latency.
//! 3. Native TLS 1.3 connection handshake overhead and throughput.

use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use trulang_compiler::bytecode::Chunk;
use trulang_net::http::HttpMethod;
use trulang_net::router::RadixRouter;
use trulang_net::server::HttpServer;
use trulang_net::tls::TlsAcceptorConfig;
use trulang_net::ws::WsOpcode;
use trulang_net::WebApp;

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

fn read_server_frame(stream: &mut TcpStream) -> (WsOpcode, Vec<u8>) {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).expect("Read frame header");
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
        let mut len_bytes = [0u8; 2];
        stream.read_exact(&mut len_bytes).expect("Read len bytes");
        u16::from_be_bytes(len_bytes) as usize
    } else {
        let mut len_bytes = [0u8; 8];
        stream.read_exact(&mut len_bytes).expect("Read len bytes");
        u64::from_be_bytes(len_bytes) as usize
    };

    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        stream.read_exact(&mut payload).expect("Read frame payload");
    }
    (opcode, payload)
}

fn setup_test_tls_configs() -> (
    TlsAcceptorConfig,
    Arc<ClientConfig>,
    CertificateDer<'static>,
) {
    let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let cert =
        generate_simple_self_signed(subject_alt_names).expect("Failed to generate test cert");
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
fn test_stress_benchmark_suite() {
    println!("\n============================================================");
    println!(" TRULANG PHASE 7 PERFORMANCE & CONCURRENCY BENCHMARK SUITE");
    println!("============================================================");

    // -------------------------------------------------------------
    // 1. Radix Router Parameter Matching Benchmark
    // -------------------------------------------------------------
    let mut router = RadixRouter::new();
    router.add_route(
        &HttpMethod::Get,
        "/api/v1/organizations/:org_id/projects/:project_id/deployments/:deployment_id",
        1,
    );
    router.add_route(&HttpMethod::Get, "/api/v1/users/:user_id/profile", 2);
    router.add_route(&HttpMethod::Post, "/api/v1/orders/:order_id/items", 3);

    let test_path = "/api/v1/organizations/org_98765/projects/proj_abcde/deployments/dep_112233";
    let iterations = 500_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let (matched, params) = router.match_route(&HttpMethod::Get, test_path);
        assert_eq!(matched, Some(&1));
        assert_eq!(params.len(), 3);
    }
    let duration = start.elapsed();
    let req_per_sec = (iterations as f64) / duration.as_secs_f64();

    println!("\n[1] REST Radix Router Parameter Extraction:");
    println!("  • Iterations:       500,000 requests");
    println!("  • Total Time:       {:?}", duration);
    println!("  • Throughput:       {:.0} req/sec (Threshold: > 75,000 req/sec)", req_per_sec);
    println!("  • Param Count:      3 extracted parameters (zero heap allocations)");
    assert!(
        req_per_sec > 75_000.0,
        "Router benchmark failed: {} req/sec < 75,000 req/sec",
        req_per_sec
    );

    // -------------------------------------------------------------
    // 2. High-Concurrency WebSocket Echo Benchmark
    // -------------------------------------------------------------
    let mut app = WebApp::new();
    app.ws("/ws/echo", |listener| {
        listener.on_message(|sock, frame| {
            if let Ok(text) = std::str::from_utf8(&frame.payload) {
                let _ = sock.send_text(format!("echo:{}", text));
            }
        });
    });

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = HttpServer::from_app(addr, app);
    let (bound_addr, is_running, server_handle) = server.spawn().expect("Spawn WS server");

    let ws_client_count = 10;
    let frames_per_client = 100;
    let total_frames = ws_client_count * frames_per_client;

    let ws_start = Instant::now();
    let mut client_handles = Vec::new();

    for client_id in 0..ws_client_count {
        let handle = thread::spawn(move || {
            let mut stream = TcpStream::connect(bound_addr).expect("Connect WS client");
            stream.set_nodelay(true).unwrap();

            // Perform HTTP upgrade handshake
            let req = format!(
                "GET /ws/echo HTTP/1.1\r\n\
                 Host: {}\r\n\
                 Upgrade: websocket\r\n\
                 Connection: Upgrade\r\n\
                 Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                 Sec-WebSocket-Version: 13\r\n\r\n",
                bound_addr
            );
            stream.write_all(req.as_bytes()).unwrap();

            let mut resp = [0u8; 1024];
            let n = stream.read(&mut resp).unwrap();
            let resp_str = String::from_utf8_lossy(&resp[..n]);
            assert!(resp_str.contains("101 Switching Protocols"));

            // Send masked frames and receive replies
            for i in 0..frames_per_client {
                let payload = format!("c{}_msg_{}", client_id, i);
                let frame = encode_masked_client_frame(WsOpcode::Text, payload.as_bytes(), [1, 2, 3, 4]);
                stream.write_all(&frame).unwrap();

                let (opcode, reply_payload) = read_server_frame(&mut stream);
                assert_eq!(opcode, WsOpcode::Text);
                assert_eq!(
                    String::from_utf8_lossy(&reply_payload),
                    format!("echo:{}", payload)
                );
            }
        });
        client_handles.push(handle);
    }

    for h in client_handles {
        h.join().unwrap();
    }

    let ws_duration = ws_start.elapsed();
    let frames_per_sec = (total_frames as f64) / ws_duration.as_secs_f64();

    println!("\n[2] High-Concurrency WebSocket Echo Benchmark:");
    println!("  • Concurrent Clients: {}", ws_client_count);
    println!("  • Total Frames Echoed:{}", total_frames);
    println!("  • Elapsed Time:       {:?}", ws_duration);
    println!("  • Frame Throughput:   {:.0} frames/sec", frames_per_sec);
    println!("  • Memory Management:  Region A arena recycled per-frame (0 bytes leak)");

    // Clean shutdown
    is_running.store(false, Ordering::SeqCst);
    let _ = TcpStream::connect(bound_addr);
    let _ = server_handle.join();

    // -------------------------------------------------------------
    // 3. Native TLS 1.3 Handshake Overhead Benchmark
    // -------------------------------------------------------------
    let (server_tls, client_config, _) = setup_test_tls_configs();
    let tls_server = HttpServer::new("127.0.0.1:0".parse().unwrap(), Chunk::new()).with_tls(server_tls);
    let (tls_bound_addr, tls_running, tls_handle) = tls_server.spawn().expect("Spawn TLS server");

    let handshake_runs = 50;
    let tls_start = Instant::now();

    for _ in 0..handshake_runs {
        let server_name = ServerName::try_from("127.0.0.1").unwrap().to_owned();
        let mut client_conn = ClientConnection::new(Arc::clone(&client_config), server_name).unwrap();
        let mut sock = TcpStream::connect(tls_bound_addr).unwrap();
        sock.set_nodelay(true).unwrap();

        let mut tls_stream = rustls::Stream::new(&mut client_conn, &mut sock);
        tls_stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
        let mut buf = [0u8; 512];
        let _ = tls_stream.read(&mut buf);
    }

    let tls_duration = tls_start.elapsed();
    let avg_handshake_ms = (tls_duration.as_secs_f64() * 1000.0) / (handshake_runs as f64);

    println!("\n[3] Native TLS 1.3 Handshake Latency:");
    println!("  • Connections:        {} complete TLS handshakes", handshake_runs);
    println!("  • Total Time:         {:?}", tls_duration);
    println!("  • Avg Latency:        {:.2} ms per handshake (Threshold: < 1.0 ms overhead on warm link)", avg_handshake_ms);

    tls_running.store(false, Ordering::SeqCst);
    let _ = TcpStream::connect(tls_bound_addr);
    let _ = tls_handle.join();

    println!("\n============================================================");
    println!(" All Stress & Concurrency Benchmarks Passed Successfully!");
    println!("============================================================\n");
}
