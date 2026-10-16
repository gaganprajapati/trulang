//! High-Performance Non-Blocking Asynchronous HTTP Server Engine.
//!
//! Orchestrates the OS readiness loop (kqueue/epoll), zero-copy HTTP/1.1 wire parser,
//! Region A per-request lifecycle, and bounded Service Actor backpressure.

use crate::http::{HttpParser, HttpRequest, HttpResponse, ParseResult};
use crate::poller::{set_nodelay, set_nonblocking, set_reuseaddr, Event, Interest, Poller};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
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
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            active_connections: AtomicUsize::new(0),
            total_region_a_resets: AtomicU64::new(0),
            total_bytes_allocated_region_a: AtomicU64::new(0),
        }
    }
}

struct Connection {
    fd: RawFd,
    read_buf: Vec<u8>,
    write_buf: Vec<u8>,
    keep_alive: bool,
}

impl Connection {
    fn new(fd: RawFd) -> Self {
        Self {
            fd,
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

    pub fn stop_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_running)
    }

    /// Runs the HTTP event loop on the current thread, blocking until stopped.
    pub fn run(&self) -> io::Result<()> {
        let listener = TcpListener::bind(self.bind_addr)?;
        let server_fd = listener.as_raw_fd();
        set_nonblocking(server_fd)?;
        set_reuseaddr(server_fd)?;

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

        while self.is_running.load(Ordering::Relaxed) {
            let timeout = Some(Duration::from_millis(50));
            let num_events = match poller.poll(&mut events, timeout) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };

            for i in 0..num_events {
                let ev = events[i];

                if ev.token == LISTENER_TOKEN {
                    // Accept new incoming connections in a loop until EAGAIN/EWOULDBLOCK
                    loop {
                        match listener.accept() {
                            Ok((stream, _peer_addr)) => {
                                let client_fd = stream.into_raw_fd();
                                let _ = set_nonblocking(client_fd);
                                let _ = set_nodelay(client_fd);

                                let token = next_token;
                                next_token += 1;
                                if next_token == usize::MAX {
                                    next_token = 1;
                                }

                                if poller.register(client_fd, token, Interest::Both).is_ok() {
                                    connections.insert(token, Connection::new(client_fd));
                                    self.metrics
                                        .active_connections
                                        .fetch_add(1, Ordering::Relaxed);
                                } else {
                                    unsafe {
                                        libc::close(client_fd);
                                    }
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
                        close_connection(&poller, conn.fd);
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
                            let mut stream = unsafe { TcpStream::from_raw_fd(conn.fd) };
                            let read_res = stream.read(&mut buf);
                            let _ = stream.into_raw_fd(); // Avoid closing fd on drop

                            match read_res {
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
                            close_connection(&poller, conn.fd);
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
                                    conn.keep_alive = req.keep_alive;

                                    self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);

                                    // Check actor mailbox backpressure
                                    let active =
                                        self.metrics.active_connections.load(Ordering::Relaxed);
                                    let response = if active > self.mailbox_capacity {
                                        HttpResponse::new(
                                            503,
                                            r#"{"error":"Mailbox full: Service under high backpressure"}"#,
                                        )
                                        .with_header("content-type", "application/json")
                                        .with_header("retry-after", "5")
                                    } else {
                                        self.execute_request(&req, &shared_region_b)
                                    };

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
                        let mut stream = unsafe { TcpStream::from_raw_fd(conn.fd) };
                        let write_res = stream.write(&conn.write_buf);
                        let _ = stream.into_raw_fd();

                        match write_res {
                            Ok(n) => {
                                conn.write_buf.drain(0..n);
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                            Err(_) => {
                                close_connection(&poller, conn.fd);
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
                        close_connection(&poller, conn.fd);
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
            close_connection(&poller, conn.fd);
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

fn close_connection(poller: &Poller, fd: RawFd) {
    let _ = poller.deregister(fd);
    unsafe {
        libc::close(fd);
    }
}
