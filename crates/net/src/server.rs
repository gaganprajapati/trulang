//! High-Performance Non-Blocking Asynchronous HTTP Server Engine.
//!
//! Orchestrates the OS readiness loop (kqueue/epoll), zero-copy HTTP/1.1 wire parser,
//! Region A per-request lifecycle, and bounded Service Actor backpressure.

use crate::http::{HttpParser, HttpRequest, HttpResponse, ParseResult};
use crate::poller::{get_socket_handle, set_reuseaddr, Event, Interest, Poller};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use trulang_compiler::bytecode::Chunk;
use trulang_supervisor::SharedTelemetryRegion;
use trulang_vm::{ServiceActor, VirtualMachine};

pub struct ServerMetrics {
    pub total_requests: AtomicU64,
    pub active_connections: AtomicUsize,
    pub total_region_a_resets: AtomicU64,
    pub total_bytes_allocated_region_a: AtomicU64,
    pub status_2xx: AtomicU64,
    pub status_4xx: AtomicU64,
    pub status_5xx: AtomicU64,
    pub total_duration_us: AtomicU64,
    pub start_time: std::time::SystemTime,
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            active_connections: AtomicUsize::new(0),
            total_region_a_resets: AtomicU64::new(0),
            total_bytes_allocated_region_a: AtomicU64::new(0),
            status_2xx: AtomicU64::new(0),
            status_4xx: AtomicU64::new(0),
            status_5xx: AtomicU64::new(0),
            total_duration_us: AtomicU64::new(0),
            start_time: std::time::SystemTime::now(),
        }
    }
}

struct Connection {
    stream: TcpStream,
    read_buf: Vec<u8>,
    write_buf: Vec<u8>,
    keep_alive: bool,
}

impl Connection {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            read_buf: Vec::with_capacity(4096),
            write_buf: Vec::with_capacity(4096),
            keep_alive: true,
        }
    }
}

pub struct HttpServer {
    bind_addr: SocketAddr,
    chunk: Chunk,
    request_budget_bytes: usize,
    max_request_budget_bytes: Option<usize>,
    persistent_budget_bytes: usize,
    mailbox_capacity: usize,
    is_running: Arc<AtomicBool>,
    is_draining: Arc<AtomicBool>,
    drain_timeout: Duration,
    pub metrics: Arc<ServerMetrics>,
    telemetry: Option<Arc<SharedTelemetryRegion>>,
}

impl HttpServer {
    pub const DEFAULT_REQUEST_BUDGET: usize = 16 * 1024 * 1024; // 16 MB
    pub const DEFAULT_MAX_REQUEST_BUDGET: usize = 64 * 1024 * 1024; // 64 MB
    pub const DEFAULT_PERSISTENT_BUDGET: usize = 512 * 1024 * 1024; // 512 MB

    pub fn new(bind_addr: SocketAddr, chunk: Chunk) -> Self {
        Self {
            bind_addr,
            chunk,
            request_budget_bytes: Self::DEFAULT_REQUEST_BUDGET,
            max_request_budget_bytes: Some(Self::DEFAULT_MAX_REQUEST_BUDGET),
            persistent_budget_bytes: Self::DEFAULT_PERSISTENT_BUDGET,
            mailbox_capacity: ServiceActor::DEFAULT_MAILBOX_CAPACITY,
            is_running: Arc::new(AtomicBool::new(false)),
            is_draining: Arc::new(AtomicBool::new(false)),
            drain_timeout: Duration::from_secs(15),
            metrics: Arc::new(ServerMetrics::default()),
            telemetry: None,
        }
    }

    pub fn with_request_budget(mut self, budget: usize, max_budget: Option<usize>) -> Self {
        self.request_budget_bytes = budget;
        self.max_request_budget_bytes = max_budget;
        self
    }

    pub fn with_mailbox_capacity(mut self, capacity: usize) -> Self {
        self.mailbox_capacity = capacity;
        self
    }

    pub fn with_telemetry(mut self, telemetry: Arc<SharedTelemetryRegion>) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    pub fn with_drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    pub fn stop_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_running)
    }

    pub fn drain_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_draining)
    }

    pub fn is_draining(&self) -> bool {
        self.is_draining.load(Ordering::SeqCst)
    }

    pub fn trigger_drain(&self) {
        self.is_draining.store(true, Ordering::SeqCst);
    }

    /// Renders standard Prometheus exposition text format (/metrics).
    pub fn render_prometheus_metrics(&self) -> String {
        let now = std::time::SystemTime::now();
        let uptime_secs = now
            .duration_since(self.metrics.start_time)
            .unwrap_or_default()
            .as_secs();

        let total_reqs = self.metrics.total_requests.load(Ordering::Relaxed);
        let active_conns = self.metrics.active_connections.load(Ordering::Relaxed);
        let resets = self.metrics.total_region_a_resets.load(Ordering::Relaxed);
        let alloc_bytes = self
            .metrics
            .total_bytes_allocated_region_a
            .load(Ordering::Relaxed);
        let s2xx = self.metrics.status_2xx.load(Ordering::Relaxed);
        let s4xx = self.metrics.status_4xx.load(Ordering::Relaxed);
        let s5xx = self.metrics.status_5xx.load(Ordering::Relaxed);
        let total_dur_us = self.metrics.total_duration_us.load(Ordering::Relaxed);
        let avg_dur_s = if total_reqs > 0 {
            (total_dur_us as f64) / (total_reqs as f64) / 1_000_000.0
        } else {
            0.0
        };

        let (reg_a_used, reg_b_live, reg_b_objs) = if let Some(ref shm) = self.telemetry {
            let hdr = shm.header();
            (
                hdr.region_a_used_bytes.load(Ordering::Relaxed),
                hdr.region_b_live_bytes.load(Ordering::Relaxed),
                hdr.region_b_live_objects.load(Ordering::Relaxed),
            )
        } else {
            (0, 0, 0)
        };

        let mut out = String::with_capacity(2048);
        out.push_str(
            "# HELP trulang_arena_a_bytes_allocated Total bytes bump-allocated in Region A\n",
        );
        out.push_str("# TYPE trulang_arena_a_bytes_allocated counter\n");
        out.push_str(&format!(
            "trulang_arena_a_bytes_allocated {}\n\n",
            alloc_bytes
        ));

        out.push_str("# HELP trulang_arena_a_resets_total Total number of O(1) arena resets\n");
        out.push_str("# TYPE trulang_arena_a_resets_total counter\n");
        out.push_str(&format!("trulang_arena_a_resets_total {}\n\n", resets));

        out.push_str("# HELP trulang_arena_a_used_bytes Current bytes used in active Region A\n");
        out.push_str("# TYPE trulang_arena_a_used_bytes gauge\n");
        out.push_str(&format!("trulang_arena_a_used_bytes {}\n\n", reg_a_used));

        out.push_str("# HELP trulang_arena_a_budget_bytes Configured memory budget for Region A\n");
        out.push_str("# TYPE trulang_arena_a_budget_bytes gauge\n");
        out.push_str(&format!(
            "trulang_arena_a_budget_bytes {}\n\n",
            self.request_budget_bytes
        ));

        out.push_str("# HELP trulang_arena_b_live_bytes Total live persistent bytes in Region B\n");
        out.push_str("# TYPE trulang_arena_b_live_bytes gauge\n");
        out.push_str(&format!("trulang_arena_b_live_bytes {}\n\n", reg_b_live));

        out.push_str("# HELP trulang_arena_b_budget_bytes Configured memory budget for Region B\n");
        out.push_str("# TYPE trulang_arena_b_budget_bytes gauge\n");
        out.push_str(&format!(
            "trulang_arena_b_budget_bytes {}\n\n",
            self.persistent_budget_bytes
        ));

        out.push_str(
            "# HELP trulang_arena_b_live_objects Total live persistent DAG objects in Region B\n",
        );
        out.push_str("# TYPE trulang_arena_b_live_objects gauge\n");
        out.push_str(&format!("trulang_arena_b_live_objects {}\n\n", reg_b_objs));

        out.push_str(
            "# HELP trulang_actor_mailbox_depth Current queued messages in actor mailboxes\n",
        );
        out.push_str("# TYPE trulang_actor_mailbox_depth gauge\n");
        out.push_str(&format!(
            "trulang_actor_mailbox_depth{{actor=\"ServiceActor\"}} {}\n\n",
            active_conns
        ));

        out.push_str("# HELP trulang_http_requests_total Total HTTP requests handled\n");
        out.push_str("# TYPE trulang_http_requests_total counter\n");
        out.push_str(&format!(
            "trulang_http_requests_total{{status=\"2xx\"}} {}\n",
            s2xx
        ));
        out.push_str(&format!(
            "trulang_http_requests_total{{status=\"4xx\"}} {}\n",
            s4xx
        ));
        out.push_str(&format!(
            "trulang_http_requests_total{{status=\"5xx\"}} {}\n\n",
            s5xx
        ));

        out.push_str("# HELP trulang_http_request_duration_seconds Average HTTP request duration in seconds\n");
        out.push_str("# TYPE trulang_http_request_duration_seconds gauge\n");
        out.push_str(&format!(
            "trulang_http_request_duration_seconds {:.6}\n\n",
            avg_dur_s
        ));

        out.push_str("# HELP trulang_active_connections Current active TCP connections\n");
        out.push_str("# TYPE trulang_active_connections gauge\n");
        out.push_str(&format!("trulang_active_connections {}\n\n", active_conns));

        out.push_str("# HELP trulang_uptime_seconds Total server uptime in seconds\n");
        out.push_str("# TYPE trulang_uptime_seconds gauge\n");
        out.push_str(&format!("trulang_uptime_seconds {}\n", uptime_secs));

        out
    }

    /// Runs the HTTP event loop on the current thread, blocking until stopped.
    pub fn run(&self) -> io::Result<()> {
        let listener = TcpListener::bind(self.bind_addr)?;
        listener.set_nonblocking(true)?;
        let server_fd = get_socket_handle(&listener);
        let _ = set_reuseaddr(server_fd);

        let poller = Poller::new()?;
        const LISTENER_TOKEN: usize = 0;
        poller.register(server_fd, LISTENER_TOKEN, Interest::Readable)?;

        self.is_running.store(true, Ordering::SeqCst);

        let mut connections: HashMap<usize, Connection> = HashMap::new();
        let mut next_token: usize = 1;
        let mut events: Vec<Event> = Vec::with_capacity(128);

        let shared_region_b = Arc::new(trulang_vm::RegionBAllocator::new(
            self.persistent_budget_bytes,
        ));

        let mut listener_registered = true;
        let mut drain_started_at: Option<std::time::Instant> = None;

        while self.is_running.load(Ordering::Relaxed) {
            let draining = self.is_draining.load(Ordering::Relaxed);
            if draining {
                if listener_registered {
                    // Drain any pending handshakes in the OS listen backlog
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
                                    connections.insert(token, Connection::new(stream));
                                    self.metrics
                                        .active_connections
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            Err(_) => break,
                        }
                    }

                    let _ = poller.deregister(server_fd);
                    listener_registered = false;
                    drain_started_at = Some(std::time::Instant::now());
                }
                if connections.is_empty() {
                    break;
                }
                if let Some(started) = drain_started_at {
                    if started.elapsed() >= self.drain_timeout {
                        break;
                    }
                }
            }

            let timeout = Some(Duration::from_millis(50));
            let num_events = match poller.poll(&mut events, timeout) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };

            for i in 0..num_events {
                let ev = events[i];

                if ev.token == LISTENER_TOKEN {
                    if !listener_registered || self.is_draining.load(Ordering::Relaxed) {
                        continue;
                    }
                    // Accept new incoming connections in a loop until EAGAIN/EWOULDBLOCK
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
                                    connections.insert(token, Connection::new(stream));
                                    self.metrics
                                        .active_connections
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                } else if let Some(conn) = connections.get_mut(&ev.token) {
                    if ev.is_closed || ev.is_error {
                        close_connection(&poller, &conn.stream);
                        connections.remove(&ev.token);
                        self.metrics
                            .active_connections
                            .fetch_sub(1, Ordering::Relaxed);
                        continue;
                    }

                    // Handle Read readiness
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
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    break;
                                }
                                Err(_) => {
                                    closed = true;
                                    break;
                                }
                            }
                        }

                        if closed {
                            close_connection(&poller, &conn.stream);
                            connections.remove(&ev.token);
                            self.metrics
                                .active_connections
                                .fetch_sub(1, Ordering::Relaxed);
                            continue;
                        }

                        // Process all complete HTTP requests from the buffer
                        loop {
                            match HttpParser::parse(&conn.read_buf) {
                                ParseResult::Complete(req, consumed) => {
                                    conn.read_buf.drain(0..consumed);
                                    conn.keep_alive =
                                        req.keep_alive && !self.is_draining.load(Ordering::Relaxed);

                                    self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
                                    let req_start = std::time::Instant::now();

                                    // Extract or create W3C Trace Context
                                    let span_ctx = req
                                        .header("traceparent")
                                        .and_then(crate::otel::SpanContext::from_traceparent)
                                        .unwrap_or_else(crate::otel::SpanContext::new_root);

                                    let trace_id = span_ctx.trace_id.clone();
                                    let traceparent_val = span_ctx.to_traceparent();

                                    // Check actor mailbox backpressure
                                    let active =
                                        self.metrics.active_connections.load(Ordering::Relaxed);
                                    let mut response = match req.path.as_str() {
                                        "/metrics" => {
                                            let text = self.render_prometheus_metrics();
                                            HttpResponse::prometheus(text)
                                        }
                                        "/healthz" | "/livez" => {
                                            let uptime = self
                                                .metrics
                                                .start_time
                                                .elapsed()
                                                .unwrap_or_default()
                                                .as_secs();
                                            HttpResponse::ok(format!(
                                                r#"{{"status":"ok","uptime_seconds":{}}}"#,
                                                uptime
                                            ))
                                            .with_header("content-type", "application/json")
                                        }
                                        "/ready" | "/readyz" => {
                                            if self.is_draining.load(Ordering::Relaxed) {
                                                HttpResponse::new(503, r#"{"status":"draining"}"#)
                                                    .with_header("content-type", "application/json")
                                            } else {
                                                HttpResponse::ok(r#"{"status":"ready"}"#)
                                                    .with_header("content-type", "application/json")
                                            }
                                        }
                                        _ => {
                                            if active > self.mailbox_capacity {
                                                HttpResponse::new(
                                                    503,
                                                    r#"{"error":"Mailbox full: Service under high backpressure"}"#,
                                                )
                                                .with_header("content-type", "application/json")
                                                .with_header("retry-after", "5")
                                            } else {
                                                self.execute_request(&req, &shared_region_b)
                                            }
                                        }
                                    };

                                    // Attach W3C Trace Context headers
                                    response = response
                                        .with_header("traceparent", traceparent_val)
                                        .with_header("x-trace-id", trace_id.clone())
                                        .with_header("x-request-id", trace_id);

                                    match response.status_code {
                                        200..=299 => {
                                            self.metrics.status_2xx.fetch_add(1, Ordering::Relaxed);
                                        }
                                        400..=499 => {
                                            self.metrics.status_4xx.fetch_add(1, Ordering::Relaxed);
                                        }
                                        500..=599 => {
                                            self.metrics.status_5xx.fetch_add(1, Ordering::Relaxed);
                                        }
                                        _ => {}
                                    };
                                    let dur_us = req_start.elapsed().as_micros() as u64;
                                    self.metrics
                                        .total_duration_us
                                        .fetch_add(dur_us, Ordering::Relaxed);

                                    let wire = response.to_wire_bytes(conn.keep_alive);
                                    conn.write_buf.extend_from_slice(&wire);
                                }
                                ParseResult::Incomplete => {
                                    break;
                                }
                                ParseResult::Invalid(err) => {
                                    let err_res =
                                        HttpResponse::error(400, format!("Bad Request: {}", err));
                                    conn.write_buf
                                        .extend_from_slice(&err_res.to_wire_bytes(false));
                                    conn.keep_alive = false;
                                    break;
                                }
                            }
                        }
                    }

                    // Handle Write readiness
                    if !conn.write_buf.is_empty() {
                        match conn.stream.write(&conn.write_buf) {
                            Ok(n) => {
                                conn.write_buf.drain(0..n);
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                            Err(_) => {
                                close_connection(&poller, &conn.stream);
                                connections.remove(&ev.token);
                                self.metrics
                                    .active_connections
                                    .fetch_sub(1, Ordering::Relaxed);
                                continue;
                            }
                        }
                    }

                    // If all outgoing bytes written and not keep-alive, close connection
                    if conn.write_buf.is_empty() && !conn.keep_alive {
                        close_connection(&poller, &conn.stream);
                        connections.remove(&ev.token);
                        self.metrics
                            .active_connections
                            .fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
        }

        // Clean up remaining open connections on shutdown
        for (_, conn) in connections {
            close_connection(&poller, &conn.stream);
        }

        Ok(())
    }

    /// Executes an HTTP request against the compiled TruLang Service Actor
    /// with an isolated Region A memory arena.
    fn execute_request(
        &self,
        req: &HttpRequest,
        persistent_alloc: &Arc<trulang_vm::RegionBAllocator>,
    ) -> HttpResponse {
        let mut vm = VirtualMachine::with_persistent_allocator(
            self.request_budget_bytes,
            Arc::clone(persistent_alloc),
        );
        vm.region_a.max_budget_bytes = self.max_request_budget_bytes;

        if let Some(ref shm) = self.telemetry {
            vm.attach_telemetry(Arc::clone(shm));
        }

        // Convert request into Region A object
        let req_val = match req.to_trulang_value(&mut vm) {
            Ok(v) => v,
            Err(e) => {
                return HttpResponse::error(
                    500,
                    format!(
                        "Memory budget exceeded while streaming request into Region A: {}",
                        e
                    ),
                );
            }
        };

        // Pass req object to service handler
        vm.stack.push(req_val);

        let execution_result = vm.execute(&self.chunk);

        let resp = match execution_result {
            Ok(result_val) => HttpResponse::from_trulang_value(&result_val),
            Err(err) => HttpResponse::error(500, format!("Service Handler Exception: {}", err)),
        };

        // Track metrics before reset
        let allocated_bytes = vm.region_a.used_bytes;
        self.metrics
            .total_bytes_allocated_region_a
            .fetch_add(allocated_bytes as u64, Ordering::Relaxed);

        // Core TruLang Advantage: Zero-Copy O(1) bulk reset of Region A!
        vm.region_a.reset();
        self.metrics
            .total_region_a_resets
            .fetch_add(1, Ordering::Relaxed);

        resp
    }

    /// Spawns the HTTP server on a dedicated background thread.
    /// Returns the bound local address and a shutdown trigger.
    pub fn spawn(
        self,
    ) -> io::Result<(
        SocketAddr,
        Arc<AtomicBool>,
        std::thread::JoinHandle<io::Result<()>>,
    )> {
        let listener = TcpListener::bind(self.bind_addr)?;
        let bound_addr = listener.local_addr()?;
        drop(listener); // Free for the server loop

        let is_running = Arc::clone(&self.is_running);
        let mut server = self;
        server.bind_addr = bound_addr;

        let handle = std::thread::spawn(move || server.run());

        // Wait brief period for thread to initialize
        std::thread::sleep(Duration::from_millis(20));

        Ok((bound_addr, is_running, handle))
    }
}

fn close_connection(poller: &Poller, stream: &TcpStream) {
    let handle = get_socket_handle(stream);
    let _ = poller.deregister(handle);
}
