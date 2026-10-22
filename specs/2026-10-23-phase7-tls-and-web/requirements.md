# Feature Requirements: Phase 7 - Native TLS, WebSockets, Web Framework & DBs

> **Date:** 2026-10-23  
> **Status:** Approved / Ready for Implementation  
> **Target Release:** `v1.1.0`  
> **Target Crates & Tools:** `crates/net` (`trulang/tls`, `trulang/ws`, `trulang/web`, `trulang/db`), `editors/vscode`  
> **Parent Roadmap:** [02-ROADMAP-2.0-SCALE.md](../../roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md#phase-7-native-tlshttps-websockets-web-framework--relational-dbs-next-milestone)  

---

## 1. Problem Statement & Motivation
While TruLang 1.0 GA delivered the core async HTTP engine on raw OS readiness pollers (`kqueue`/`epoll`/`WSAPoll`), real-world production microservices require:
1. **Encrypted Transport:** Native TLS 1.3 termination without forcing developers to manage external Nginx or Envoy sidecars.
2. **Real-Time Duplex Streaming:** WebSockets (RFC 6455) and Server-Sent Events (SSE) for modern live dashboards and LLM token streaming.
3. **Ergonomic Routing & Middleware:** A declarative router with path parameter extraction (`/api/v1/users/:id`), CORS, JWT auth, and zero-copy JSON body parsing.
4. **Relational Persistence:** Non-blocking PostgreSQL wire protocol client and embedded SQLite driver with connection pooling.
5. **IDE Distribution:** Official release of `trulang-1.0.0.vsix` to the Microsoft Visual Studio Code Marketplace and Open VSX Registry.

---

## 2. Scope & Key Architectural Decisions (ADRs)

### In Scope:
- **TLS:** Pure-Rust `rustls` 0.23+ integration with zero OpenSSL dependencies. Zero-copy ALPN negotiation (`http/1.1`, `h2`).
- **WebSockets:** RFC 6455 upgrade handshake over `trulang_net::server`. Per-frame arena allocation.
- **SSE:** Native `text/event-stream` response generator with automatic keep-alive ping frames.
- **Web Router:** Composable middleware chain (`cors`, `jwt_auth`, `logger`) and parameterized routes (`GET /users/:id`).
- **Databases:** Non-blocking PostgreSQL wire protocol parser (v3.0) and SQLite embedded engine with connection pool actors.
- **VS Code Extension:** Publish existing `editors/vscode/trulang-1.0.0.vsix`.

### Key Architectural Decisions:
- **ADR-7.1 (Zero-OpenSSL):** Use `rustls` + `webpki-roots`. Eliminates C runtime vulnerabilities and simplifies cross-compilation on Windows and Linux.
- **ADR-7.2 (Region A Frame Recycling):** WebSocket frames and HTTP request bodies are allocated inside transient Region A arenas and recycled in O(1) time when the handler/frame callback finishes.
- **ADR-7.3 (Actor-Bound Connections):** Each active WebSocket connection and Database Connection Pool is represented as an isolated Service Actor.

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

    // 5. Native TLS Listening
    app.listen_tls("0.0.0.0:8443", cert: "cert.pem", key: "key.pem");
  }
}
```

---

## 4. Memory Model & Concurrency Impact

- **Region A (Transient Arena):**
  - TLS decrypted byte slices, HTTP request headers, route path parameter maps, and incoming JSON bodies are allocated exclusively in Region A.
  - Automatically reset to offset `0` upon completion of each request.
- **Region B (Persistent DAG):**
  - Active WebSocket actor session states, long-lived DB pool connection handles, and cached prepared SQL statements live in Region B.
  - No circular references; managed via DAG reference topology.

---

## 5. Non-Functional Performance Thresholds

- **TLS Handshake Latency:** < 1.0 ms overhead on modern hardware.
- **Throughput:** > 75,000 req/sec per CPU core on parameterized REST routes.
- **Concurrent WebSockets:** 10,000+ idle WebSocket connections with < 25 MB total RSS footprint.
