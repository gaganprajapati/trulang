//! Native Non-Blocking Asynchronous Network and HTTP/1.1 Engine for TruLang.
//!
//! Provides the OS event readiness loop (kqueue/epoll), zero-copy wire frame
//! parsing into Region A, and native HTTP request/response abstractions.

pub mod http;
pub mod poller;
pub mod server;

pub use http::{HttpMethod, HttpParser, HttpRequest, HttpResponse, ParseResult};
pub use poller::{Event, Interest, Poller};
pub use server::{HttpServer, ServerMetrics};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::atomic::Ordering;
    use trulang_compiler::bytecode::{Chunk, Opcode, Value};

    #[test]
    fn test_poller_creation_and_registration() {
        let poller = Poller::new().expect("Failed to initialize OS poller");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind listener");
        let fd = std::os::unix::io::AsRawFd::as_raw_fd(&listener);

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
}
