# Implementation Plan: Phase 7 - Native TLS, WebSockets, Web Framework & DBs

> **Spec:** [requirements.md](requirements.md)  
> **Status:** In Progress  

---

## Task Group 1: Native TLS/HTTPS Termination (`crates/net/src/tls.rs`)
- [ ] 1.1 Add `rustls = "0.23"` and `webpki-roots` to `crates/net/Cargo.toml`.
- [ ] 1.2 Implement `TlsAcceptorConfig` loading X.509 certificates and private keys.
- [ ] 1.3 Integrate non-blocking TLS handshake state machine into `crates/net/src/poller.rs`.
- [ ] 1.4 Write unit test verifying TLS 1.3 handshake and HTTPS echo server.

---

## Task Group 2: Ergonomic Routing & Middleware (`crates/net/src/router.rs`)
- [ ] 2.1 Implement radix-tree or segment-based path routing (`/users/:id`).
- [ ] 2.2 Implement middleware pipeline chain: `Middleware::handle(req, next)`.
- [ ] 2.3 Provide built-in `CorsMiddleware`, `LoggerMiddleware`, and `HeaderValidationMiddleware`.
- [ ] 2.4 Verify zero-copy parameter extraction using Region A string slices.

---

## Task Group 3: Real-Time Protocols: WebSockets & SSE (`crates/net/src/ws.rs`)
- [ ] 3.1 Implement RFC 6455 WebSocket upgrade handshake over HTTP/1.1.
- [ ] 3.2 Implement frame parsing (masked client frames, text/binary/ping/pong frames).
- [ ] 3.3 Implement Server-Sent Events (`HttpResponse::sse()`) with chunked transfer encoding.
- [ ] 3.4 Add integration tests for concurrent WebSocket echo and SSE broadcast.

---

## Task Group 4: Native Relational Database Drivers (`crates/net/src/db/`)
- [ ] 4.1 Implement PostgreSQL wire protocol client (v3.0 startup, query, row decoding).
- [ ] 4.2 Implement embedded SQLite driver wrapper with actor connection pooling.
- [ ] 4.3 Expose non-blocking `query` and `execute` methods to TruLang VM FFI.

---

## Task Group 5: Official VS Code Extension Publishing (`editors/vscode`)
- [ ] 5.1 Verify `editors/vscode/package.json` metadata, icons, and repository links.
- [ ] 5.2 Validate `trulang-1.0.0.vsix` with `vsce package` and test manual local install.
- [ ] 5.3 Prepare Marketplace publisher credentials and publish to Visual Studio Marketplace & Open VSX.

---

## Task Group 6: Workspace Validation & Benchmarking
- [ ] 6.1 Run full workspace tests: `cargo test --workspace`.
- [ ] 6.2 Run workspace linter: `cargo clippy --workspace -- -D warnings`.
- [ ] 6.3 Run HTTPS & WebSocket stress benchmark; record numbers in [validation.md](validation.md).
