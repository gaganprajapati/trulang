//! Native Non-Blocking Asynchronous Network and HTTP/1.1 Engine for TruLang.
//!
//! Provides the OS event readiness loop (kqueue/epoll), zero-copy wire frame
//! parsing into Region A, and native HTTP request/response abstractions.

pub mod http;
pub mod otel;
pub mod poller;
pub mod router;
pub mod server;
pub mod tls;

pub use http::{HttpMethod, HttpParser, HttpRequest, HttpResponse, ParseResult};
pub use otel::{SpanContext, SpanStatus, TraceSpan};
pub use poller::{Event, Interest, Poller};
pub use router::{
    CorsMiddleware, HeaderValidationMiddleware, LoggerMiddleware, Middleware, RadixRouter,
    RouteHandler, RouteParam, RouteParams, WebApp, MAX_ROUTE_PARAMS,
};
pub use server::{HttpServer, ServerMetrics};
pub use tls::{TlsAcceptorConfig, TlsConnectionState, TransportStream, TLS_HANDSHAKE_TIMEOUT};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use trulang_compiler::bytecode::{Chunk, Opcode, Value};

    #[test]
    fn test_poller_creation_and_registration() {
        let poller = Poller::new().expect("Failed to initialize OS poller");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind listener");
        let fd = poller::get_socket_handle(&listener);

        poller::set_nonblocking(fd).expect("Set nonblocking failed");
        poller
            .register(fd, 42, Interest::Readable)
            .expect("Failed to register fd");
        poller.deregister(fd).expect("Failed to deregister fd");
    }

    #[test]
    fn test_http_server_end_to_end_request_and_region_a_reset() {
        // Construct a mock service chunk that reads param 'id' and returns JSON response
        let mut chunk = Chunk::new();
        // Load response object
        let resp_obj = Value::new_object();
        if let Value::Object(ref o) = resp_obj {
            let mut g = o.lock().unwrap();
            g.insert("status".to_string(), Value::Int(200));
            g.insert(
                "message".to_string(),
                Value::String(std::sync::Arc::new("Hello from TruLang!".to_string())),
            );
        }
        let const_idx = chunk.add_constant(resp_obj);
        chunk.code.push(Opcode::LoadConst(const_idx));
        chunk.code.push(Opcode::Return);

        let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk);
        let metrics = server.metrics.clone();
        let (addr, is_running, handle) = server.spawn().expect("Failed to spawn HTTP server");

        // Send an HTTP GET request
        let mut client = TcpStream::connect(addr).expect("Failed to connect to server");
        client
            .write_all(
                b"GET /api/v1/test?id=99 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .expect("Failed to write request");

        let mut response_buf = Vec::new();
        client
            .read_to_end(&mut response_buf)
            .expect("Failed to read response");
        let response_str = String::from_utf8(response_buf).expect("Valid UTF-8 response");

        assert!(response_str.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response_str.contains("Hello from TruLang!"));
        assert!(response_str.contains("Content-Type: application/json"));

        // Verify that Region A was bulk reset
        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.total_region_a_resets.load(Ordering::Relaxed), 1);
        assert!(
            metrics
                .total_bytes_allocated_region_a
                .load(Ordering::Relaxed)
                > 0
        );

        // Stop server
        is_running.store(false, Ordering::SeqCst);
        let _ = handle.join();
    }

    #[test]
    fn test_http_api_gateway_canonical_roadmap_service() {
        let source = r#"
import "trulang/http" as http;

service ApiGateway {
    memory_budget request = 16 MB max 64 MB;
    memory_budget persistent = 128 MB;

    fn handle_request(req: http::Request) -> http::Response {
        let user_id = req.param("id");
        return http::Response::json({
            "status": "success",
            "user": user_id
        });
    }
}
"#;
        let prog = trulang_syntax::parse(source).expect("Syntax parse failed");
        let chunk = trulang_compiler::compile(&prog).expect("Compile failed");

        let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk);
        let metrics = server.metrics.clone();
        let (addr, is_running, handle) = server.spawn().expect("Failed to spawn server");

        // Send HTTP GET request with query parameter id=emp_90210
        let mut client = TcpStream::connect(addr).expect("Connect failed");
        client.write_all(b"GET /api/v1/users?id=emp_90210 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("Write request failed");

        let mut buf = Vec::new();
        client.read_to_end(&mut buf).expect("Read response failed");
        let resp_str = String::from_utf8(buf).expect("Valid UTF-8");

        assert!(resp_str.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp_str.contains("Content-Type: application/json"));
        assert!(resp_str.contains(r#""status":"success""#));
        assert!(resp_str.contains(r#""user":"emp_90210""#));

        // Region A bulk reset verification
        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.total_region_a_resets.load(Ordering::Relaxed), 1);

        is_running.store(false, Ordering::SeqCst);
        let _ = handle.join();
    }

    #[test]
    fn test_http_keep_alive_consecutive_requests_reclaims_region_a() {
        let mut chunk = Chunk::new();
        let resp_obj = Value::new_object();
        if let Value::Object(ref o) = resp_obj {
            let mut g = o.lock().unwrap();
            g.insert("ok".to_string(), Value::Bool(true));
        }
        let const_idx = chunk.add_constant(resp_obj);
        chunk.code.push(Opcode::LoadConst(const_idx));
        chunk.code.push(Opcode::Return);

        let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk);
        let metrics = server.metrics.clone();
        let (addr, is_running, handle) = server.spawn().expect("Failed to spawn server");

        // Connect a persistent keep-alive client
        let mut client = TcpStream::connect(addr).expect("Connect failed");

        for i in 1..=5 {
            let req = format!(
                "GET /ping?seq={} HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n",
                i
            );
            client
                .write_all(req.as_bytes())
                .expect("Write request failed");

            let mut buf = [0u8; 1024];
            let n = client.read(&mut buf).expect("Read response failed");
            let resp_str = std::str::from_utf8(&buf[..n]).expect("Valid response");
            assert!(resp_str.starts_with("HTTP/1.1 200 OK\r\n"));
            assert!(resp_str.contains("Connection: keep-alive"));
        }

        // Region A was reset exactly 5 times (once per request, 0 memory accumulation)
        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 5);
        assert_eq!(metrics.total_region_a_resets.load(Ordering::Relaxed), 5);

        is_running.store(false, Ordering::SeqCst);
        let _ = handle.join();
    }

    #[test]
    fn test_http_prometheus_metrics_endpoint() {
        let chunk = Chunk::new();
        let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk);
        let (addr, is_running, handle) = server.spawn().expect("Failed to spawn server");

        let mut client = TcpStream::connect(addr).expect("Connect failed");
        let req = "GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        client
            .write_all(req.as_bytes())
            .expect("Write request failed");

        let mut resp_buf = Vec::new();
        client
            .read_to_end(&mut resp_buf)
            .expect("Read response failed");
        let resp_str = String::from_utf8_lossy(&resp_buf);

        assert!(resp_str.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp_str
            .to_ascii_lowercase()
            .contains("content-type: text/plain; version=0.0.4; charset=utf-8"));
        assert!(resp_str.contains("trulang_arena_a_bytes_allocated"));
        assert!(resp_str.contains("trulang_arena_a_resets_total"));
        assert!(resp_str.contains("trulang_http_requests_total"));
        assert!(resp_str.contains("trulang_uptime_seconds"));

        is_running.store(false, Ordering::SeqCst);
        let _ = handle.join();
    }

    #[test]
    fn test_http_kubernetes_healthz_and_ready_probes() {
        let chunk = Chunk::new();
        let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk);
        let (addr, is_running, handle) = server.spawn().expect("Failed to spawn server");

        // 1. Liveness probe /healthz
        let mut client1 = TcpStream::connect(addr).expect("Connect failed");
        client1
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut buf1 = Vec::new();
        client1.read_to_end(&mut buf1).unwrap();
        let resp1 = String::from_utf8_lossy(&buf1);
        assert!(resp1.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp1.contains(r#"{"status":"ok""#));

        // 2. Readiness probe /ready
        let mut client2 = TcpStream::connect(addr).expect("Connect failed");
        client2
            .write_all(b"GET /ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut buf2 = Vec::new();
        client2.read_to_end(&mut buf2).unwrap();
        let resp2 = String::from_utf8_lossy(&buf2);
        assert!(resp2.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp2.contains(r#"{"status":"ready"}"#));

        is_running.store(false, Ordering::SeqCst);
        let _ = handle.join();
    }

    #[test]
    fn test_http_kubernetes_graceful_draining() {
        let chunk = Chunk::new();
        let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk)
            .with_drain_timeout(Duration::from_secs(5));
        let drain_flag = server.drain_handle();
        let (addr, _is_running, handle) = server.spawn().expect("Failed to spawn server");

        // Connect client and send in-flight request
        let mut client = TcpStream::connect(addr).expect("Connect failed");
        client
            .write_all(b"GET /ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();

        // Trigger graceful draining (simulating SIGTERM while request is in flight)
        drain_flag.store(true, Ordering::SeqCst);

        // In draining mode, /ready must return 503 Service Unavailable
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).unwrap();
        let resp = String::from_utf8_lossy(&buf);
        assert!(resp.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        assert!(resp.contains(r#"{"status":"draining"}"#));

        // Server exits cleanly once connections drain
        let res = handle.join().expect("Thread join");
        assert!(res.is_ok());
    }

    #[test]
    fn test_http_w3c_traceparent_propagation() {
        let chunk = Chunk::new();
        let server = HttpServer::new("127.0.0.1:0".parse().unwrap(), chunk);
        let (addr, is_running, handle) = server.spawn().expect("Failed to spawn server");

        let mut client = TcpStream::connect(addr).expect("Connect failed");
        let incoming_traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let req = format!(
            "GET /healthz HTTP/1.1\r\nHost: localhost\r\ntraceparent: {}\r\nConnection: close\r\n\r\n",
            incoming_traceparent
        );
        client.write_all(req.as_bytes()).unwrap();

        let mut buf = Vec::new();
        client.read_to_end(&mut buf).unwrap();
        let resp = String::from_utf8_lossy(&buf);

        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp
            .to_ascii_lowercase()
            .contains("traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-"));
        assert!(resp
            .to_ascii_lowercase()
            .contains("x-trace-id: 4bf92f3577b34da6a3ce929d0e0e4736"));
        assert!(resp
            .to_ascii_lowercase()
            .contains("x-request-id: 4bf92f3577b34da6a3ce929d0e0e4736"));

        is_running.store(false, Ordering::SeqCst);
        let _ = handle.join();
    }
}
