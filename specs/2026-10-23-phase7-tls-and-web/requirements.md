# Feature Requirements: Phase 7 - Native TLS, WebSockets, Web Framework & DBs

> **Date:** 2026-10-23  
> **Status:** Approved / Ready for Implementation  
> **Target Release:** `v1.1.0`  
> **Target Crates & Tools:** `crates/net` (`trulang/tls`, `trulang/ws`, `trulang/web`), `crates/db` (`trulang/postgres`, `trulang/sqlite`), `crates/ffi`, `editors/vscode`  
> **Parent Roadmap:** [02-ROADMAP-2.0-SCALE.md](../../roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md#phase-7-native-tlshttps-websockets-web-framework--relational-dbs-next-milestone)  

---

## 1. Problem Statement & Motivation
While TruLang 1.0 GA delivered the core async HTTP engine on raw OS readiness pollers (`kqueue`/`epoll`/`WSAPoll`), real-world production microservices require:
1. **Encrypted Transport:** Native TLS 1.3 termination without forcing developers to manage external Nginx or Envoy sidecars.
2. **Real-Time Duplex Streaming:** WebSockets (RFC 6455) and Server-Sent Events (SSE) for modern live dashboards and LLM token streaming.
3. **Ergonomic Routing & Middleware:** A declarative router with path parameter extraction (`/api/v1/users/:id`), CORS, JWT auth, and zero-copy JSON body parsing.
4. **Relational Persistence:** Non-blocking PostgreSQL wire protocol client and embedded SQLite driver with connection pooling in `crates/db`.
5. **IDE Distribution:** Official release of `trulang-1.0.0.vsix` to the Microsoft Visual Studio Code Marketplace and Open VSX Registry.

---

## 2. Scope & Key Architectural Decisions (ADRs)

### In Scope:
- **TLS:** Pure-Rust `rustls` 0.23+ integration with zero OpenSSL dependencies. Scoped strictly to ALPN `http/1.1` (`config.alpn_protocols = vec![b"http/1.1".to_vec()]`) for robust compatibility across browsers and curl without broken handshakes; full binary `h2` framing is deferred.
- **WebSockets:** RFC 6455 upgrade handshake over `trulang_net::server`. Per-frame arena allocation.
- **SSE:** Native `text/event-stream` response generator with automatic keep-alive ping frames.
- **Web Router:** Composable middleware chain (`cors`, `jwt_auth`, `logger`) and parameterized routes (`GET /users/:id`).
- **Databases (`crates/db`):** Non-blocking PostgreSQL wire protocol parser (v3.0) and embedded SQLite driver with background worker thread offloading to prevent event-loop stalls.
- **VS Code Extension:** Publish existing `editors/vscode/trulang-1.0.0.vsix`.

### Key Architectural Decisions:
- **ADR-7.1 (Zero-OpenSSL & Dynamic TLS State Machine):**
  - Use `rustls` 0.23+ and `webpki-roots`. Eliminates C runtime vulnerabilities and simplifies cross-compilation on Windows and Linux.
  - **Poller Interest State Machine:** New TLS connections start in `ConnectionState::TlsHandshaking`. When `conn.wants_write()` is true, register for `Interest::Both` (`POLLIN | POLLOUT`). Once `write_tls()` flushes and `conn.wants_write()` is false, downgrade interest to `Interest::Readable` (`POLLIN`) to prevent busy-polling CPU spin. Upon handshake completion (`!conn.is_handshaking()`), transition to `ConnectionState::TlsEstablished`.
  - **Anti-Slowloris & DDoS Defense:**
    1. Enforce a hard 3.0-second TLS handshake timeout; abort and reclaim the file descriptor if exceeded.
    2. Cap maximum concurrent in-flight handshakes at 1,024 to prevent memory exhaustion.
    3. Enforce a strict 16 KB max fragment buffer on incoming TLS client records.
- **ADR-7.2 (Region A Frame Recycling):** WebSocket frames and HTTP request bodies are allocated inside transient Region A arenas and recycled in O(1) time when the handler/frame callback finishes.
- **ADR-7.3 (Actor-Bound Connections & Dedicated DB Crate):**
  - Database drivers are isolated in `crates/db`, keeping `crates/net` purely focused on network protocols.
  - Active WebSocket connections and Database Connection Pools are represented as isolated Service Actors.
  - SQLite operations are offloaded to dedicated background worker threads via bounded crossbeam channels to guarantee zero blocking on OS poller threads (`kqueue`/`epoll`). Query rows are decoded into Region A for transient HTTP responses or promoted to Region B for long-lived actor states.
- **ADR-7.4 (Zero-Copy Radix Trie & Linear Interceptors):**
  - Route matching utilizes a compiled Compressed Radix Trie performing $O(k)$ raw ASCII byte scans directly on the request buffer without regular expressions.
  - Path parameters are extracted into an inline stack/arena array of up to 8 `RouteParam<'a>` pairs (`&'static str` key, `&'a str` value slice), strictly forbidding `HashMap` heap allocations.
  - Middleware employs a non-allocating Linear Interceptor Pipeline (`before` and `after` stages) with immediate short-circuiting on early responses (e.g., 401 Unauthorized), completely eliminating async future boxing.

---

## 3. Syntax & API Specifications

### Example TruLang Microservice (`main.tr`):
```trulang
service MicroserviceApp {
  memory_budget: 128KB,

  init {
    // 1. Database Connection Pool Actor
    let db = std::db::postgres("postgres://user:pass@localhost:5432/app", max_connections: 10);

    // 2. Composable Middleware Pipeline
    app.use(std::web::cors());
    app.use(std::web::logger());

    // 3. Declarative Parameterized Routes
    app.get("/api/v1/users/:id", (req) -> {
      let user_id = req.param("id");
      let user = db.query_one("SELECT id, name FROM users WHERE id = $1", [user_id]);
      return Response::json(user);
    });

    // 4. Server-Sent Events (SSE) Streaming
    app.get("/api/v1/live-feed", (req) -> {
      let stream = Response::sse();
      stream.send("heartbeat", { time: std::time::now() });
      return stream;
    });

    // 5. Bidirectional WebSockets (RFC 6455)
    app.ws("/api/v1/chat", (socket, req) -> {
      socket.on_open(() -> {
        socket.send_json({ type: "welcome", client_id: socket.id });
      });
      socket.on_message((msg) -> {
        socket.send_text("Echo: " + msg);
      });
      socket.on_close((code, reason) -> {
        std::println("Socket closed: " + code);
      });
    });

    // 6. Native TLS Listening
    app.listen_tls("0.0.0.0:8443", cert: "cert.pem", key: "key.pem");
  }
}
```

---

## 4. Memory Model & Concurrency Impact

- **WebSocket Flow Control & Backpressure:**
  - Control frames (`Ping`/`Pong`) are handled autonomously at the wire level in `crates/net` without waking the user actor. Incoming `Ping` automatically schedules an identical `Pong` reply. The server sends periodic 30s heartbeats with 10s timeouts to prune dead connections.
  - Outgoing write buffers are bounded with a 64 KB high-watermark. If the outgoing queue exceeds 64 KB, poller `POLLIN` interest for that socket is paused until the outbound buffer drains below 16 KB.
  - If a slow client forces buffered memory to exceed the actor's configured `memory_budget`, the connection is immediately terminated with RFC 6455 code 1008 (Policy Violation).

- **Graceful Shutdown & Connection Drain Protocol (15-second timeout):**
  1. **T = 0.0s (Immediate Ingress Cutoff):** Deregister listener socket from poller; flip `/healthz` and `/readyz` endpoints to HTTP 503 (`{"status":"draining"}`) so upstream load balancers immediately reroute incoming traffic.
  2. **T = 0.1s – 5.0s (Streaming Drain):**
     - *WebSockets:* Broadcast RFC 6455 Close frame (code 1001 Going Away, reason: `"Server shutting down"`). Await client Close frame acknowledgment up to 5.0s, then close socket.
     - *SSE Streams:* Emit final `event: shutdown\ndata: {"status":"draining"}\n\n` followed by terminating chunk `0\r\n\r\n`, then close socket.
  3. **T = 5.0s – 12.0s (In-Flight & DB Pool Drain):**
     - Allow in-flight REST request handlers to complete cleanly within remaining budget.
     - *PostgreSQL:* Send Terminate (`'X'`) protocol packet on all idle and returned pooled connections, closing underlying TCP sockets.
     - *SQLite:* Drain all queued tasks on the background worker channel, issue `PRAGMA wal_checkpoint(TRUNCATE);` to safely flush WAL buffers to disk, and join worker thread.
  4. **T = 15.0s (Hard Deadline):** Force-deregister and close all remaining active sockets, wipe arenas, and cleanly exit process with code 0.

- **Region A (Transient Arena):**
  - TLS decrypted byte slices, HTTP request headers, route path parameter maps, and incoming JSON bodies are allocated exclusively in Region A.
  - **Standard Request Lifecycle:** Automatically reset to offset `0` upon completion of each HTTP request.
  - **Per-Event Arena Reset (Streaming & WebSockets):** For long-lived streams (SSE and WebSockets), Region A reset is decoupled from TCP connection termination. Region A is scoped to each emitted event or incoming frame. When `stream.send()` or a frame handler formats the payload and flushes wire bytes to the non-blocking socket buffer, Region A's bump pointer immediately resets to offset `0`.
- **Region B (Persistent DAG):**
  - Active WebSocket actor session states, long-lived DB pool connection handles, subscription topics, sequence numbers, and cached prepared SQL statements live in Region B.
  - No circular references; managed via DAG reference topology.
  - **The Invariant:** Region B pointers can **never** point to Region A. Any state that must survive across events/frames must be promoted to or allocated in Region B.

---

## 5. Non-Functional Performance Thresholds

- **TLS Handshake Latency:** < 1.0 ms overhead on modern hardware.
- **Throughput:** > 75,000 req/sec per CPU core on parameterized REST routes.
- **Concurrent WebSockets:** 10,000+ idle WebSocket connections with < 25 MB total RSS footprint.
