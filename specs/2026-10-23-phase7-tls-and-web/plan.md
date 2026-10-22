# Implementation Plan: Phase 7 - Native TLS, WebSockets, Web Framework & DBs

> **Spec:** [requirements.md](requirements.md)  
> **Status:** In Progress  

---

## Task Group 1: Native TLS/HTTPS Termination (`crates/net/src/tls.rs`)
- [x] 1.1 Add `rustls = "0.23"` and `webpki-roots` to `crates/net/Cargo.toml`.
- [x] 1.2 Implement `TlsAcceptorConfig` loading X.509 certificates and private keys.
- [x] 1.3 Implement `TransportStream` enum (`Plain` vs `Tls`) with `ConnectionState` (`TlsHandshaking` vs `TlsEstablished`).
- [x] 1.4 Integrate dynamic interest state machine into `crates/net/src/poller.rs` and `server.rs` (`wants_write` switching between `Interest::Both` and `Interest::Readable`).
- [x] 1.5 Implement 3-tier Anti-Slowloris defense: 3.0s handshake timeout, 1,024 max in-flight handshakes, and 16 KB record fragment cap.
- [x] 1.6 Write unit and integration tests verifying TLS 1.3 handshake, ALPN `http/1.1` negotiation, Slowloris timeout, and HTTPS echo server.

---

## Task Group 2: Ergonomic Routing & Middleware (`crates/net/src/router.rs`)
- [x] 2.1 Implement Compressed Radix Trie with raw ASCII byte prefix matching ($O(k)$).
- [x] 2.2 Implement inline zero-copy parameter extraction (`[RouteParam<'a>; 8]`) directly referencing request buffers without `HashMap` allocations.
- [x] 2.3 Implement Linear Interceptor Pipeline with `before` and `after` stages and early short-circuit evaluation.
- [x] 2.4 Provide built-in `CorsMiddleware`, `LoggerMiddleware`, and `HeaderValidationMiddleware`.
- [x] 2.5 Benchmark route matching throughput to ensure > 75,000 req/sec per core. (Achieved: 1,566,191 req/sec).

---

## Task Group 3: Real-Time Protocols: WebSockets & SSE (`crates/net/src/ws.rs`)
- [x] 3.1 Implement RFC 6455 WebSocket upgrade handshake over HTTP/1.1 (Sec-WebSocket-Key SHA-1 + GUID).
- [x] 3.2 Implement frame parsing (masked client frames, text/binary/ping/pong/close frames).
- [x] 3.3 Implement autonomous wire-level Ping/Pong handling and periodic 30s heartbeat timers.
- [x] 3.4 Implement write backpressure: 64 KB high-watermark, pausing `POLLIN`, and code 1008 policy violation termination on budget breach.
- [x] 3.5 Implement `app.ws(path, handler)` with `on_open`, `on_message`, `on_close` callbacks and per-frame Region A arena recycling.
- [x] 3.6 Implement Server-Sent Events (`HttpResponse::sse()`) with chunked transfer encoding and per-event Region A arena reset.
- [x] 3.7 Add integration tests for concurrent WebSocket echo, Ping/Pong keep-alives, and SSE broadcast.

---

## Task Group 4: Native Relational Database Drivers (`crates/db`)
- [ ] 4.1 Expand PostgreSQL client (`crates/db/src/postgres/`) with parameterized `query_one(sql, params)` and `execute(sql, params)`.
- [ ] 4.2 Implement embedded SQLite driver (`crates/db/src/sqlite/`) utilizing a dedicated worker thread pool and bounded channels to eliminate OS event loop blocking.
- [ ] 4.3 Expose non-blocking `std::db::postgres` and `std::db::sqlite` methods via `crates/ffi`.
- [ ] 4.4 Verify dual-region query decoding: transient rows allocated in Region A with promotion to Region B on explicit persistence.

---

## Task Group 5: Official VS Code Extension Publishing (`editors/vscode`)
- [ ] 5.1 Verify `editors/vscode/package.json` metadata, icons, categories, and repository links.
- [ ] 5.2 Validate `.vsix` packaging with `npm run build && npx @vscode/vsce package` with zero bundle warnings.
- [ ] 5.3 Implement executable local script `editors/vscode/publish.sh` with `VSCE_PAT` and `OVSX_PAT` environment validation.
- [ ] 5.4 Create GitHub Actions CI workflow `.github/workflows/vscode-extension.yml` for automated release publishing to Visual Studio Marketplace and Open VSX.

---

## Task Group 6: Workspace Validation & Benchmarking
- [ ] 6.1 Run full workspace tests: `cargo test --workspace`.
- [ ] 6.2 Run workspace linter: `cargo clippy --workspace -- -D warnings`.
- [ ] 6.3 Run HTTPS & WebSocket stress benchmark; record numbers in [validation.md](validation.md).
