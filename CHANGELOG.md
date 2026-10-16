# Changelog 📜

All notable changes to the TruLang language, compiler, runtime, and supervisor will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.0] - 2026-10-17

### 🎉 TruLang 1.0 General Availability (GA)
TruLang 1.0 marks the formal production milestone, delivering a complete, enterprise-ready cloud runtime ecosystem designed for zero garbage collection pauses and sub-microsecond latency.

### Phase 1: Native Test Runner Harness (`trulang test`)
- Built-in test execution engine integrated directly into compiler and VM with `trulang test [path] [--filter <str>]`.
- Syntax support for test declarations: `test "name" { ... }`.
- Native bytecode opcodes: `Opcode::Assert` and `Opcode::AssertEq` with formatted diff output.
- Isolated per-test execution tracking elapsed milliseconds, assertions executed, and peak Region A memory bytes.
- Automatic recursive test suite discovery (`*_test.tr` and test blocks).

### Phase 2: Native Async HTTP Engine (`trulang/http`)
- Pure non-blocking asynchronous socket event loop backed by OS-level pollers (`kqueue` on macOS, `epoll` on Linux).
- Zero-copy streaming HTTP/1.1 request parser allocating directly into Region A bump arena.
- Request lifecycle with instantaneous $O(1)$ bulk arena resets upon response completion (0.0 ms GC pause).
- Standard library module `trulang/http` providing `http::Request` and `http::Response` types.
- CLI subcommand: `trulang serve <file> [--port <port>]` with graceful signal interruption.
- First-class map/dictionary literal syntax `{ key: value, ... }` with indexing, mutation, and JSON interop.

### Phase 3: Decentralized Package Manager (`tpm`)
- Human-readable `Tru.toml` package manifest specification (`[package]`, `[dependencies]`).
- Cryptographically verifiable `Tru.lock` lockfile with deterministic SHA-256 tree hashing.
- Scaffolding engine `trulang new <project_name>` generating standard microservice templates.
- CLI package management commands:
  - `trulang add <package>` with Git URL, semver branch/tag, or local filesystem path resolution.
  - `trulang install` for bit-for-bit reproducible dependency vendoring.
  - `trulang publish` for package validation and tarball archive generation.
- Multi-module compiler import resolution and cross-module bytecode linking.

### Phase 4: Database Wire Protocols (`trulang/postgres`, `trulang/redis`)
- Zero-dependency, pure binary wire protocol implementations in `crates/db`:
  - **PostgreSQL v3.0 Protocol**: Startup handshake, MD5 password authentication, Simple Query protocol, and DataRow field decoder.
  - **Redis RESP2 / RESP3 Protocol**: Pipelined commands, String, Integer, Hash, and Key/Value primitives.
- **Dual-Region Database Lifecycle**:
  - **Region B**: Long-lived connection pools (`PgPool`, `RedisPool`) and prepared statement caches.
  - **Region A**: Incoming wire frames, row tuples, and response values reclaimed in $O(1)$ time.
- Integrated standard library modules: `import "trulang/postgres" as pg;` and `import "trulang/redis" as redis;`.
- Built-in mock wire servers (`MockPostgresServer`, `MockRedisServer`) for zero-dependency offline unit testing.

### Phase 5: Cloud & Kubernetes Hardening
- Native Prometheus `/metrics` exposition exporter exposing live lockless Seqlock supervisor metrics:
  - Region A allocations, $O(1)$ reset counts, mailbox backpressure depth, and HTTP latency histograms.
- Built-in Kubernetes health probes: `/healthz` (liveness) and `/ready` (readiness).
- Production graceful draining on `SIGTERM` / `SIGINT`:
  - Ingress closure, in-flight request completion, actor mailbox draining, and connection pool cleanup.
- Built-in OpenTelemetry (OTel) distributed tracing with W3C `traceparent` context propagation.
- Multi-stage minimal production `deploy/Dockerfile` (<15 MB Alpine Linux runtime footprint, non-root user).
- Production Kubernetes manifests in `deploy/k8s/`: Deployment (with preStop hook), Service, and Prometheus ServiceMonitor.

### Phase 6: Interactive DAP Debugger, Visual Memory Profiler, & 1.0 Stability
- **Debug Adapter Protocol (DAP)**:
  - Stdio JSON-RPC DAP server via `trulang dap --stdio`.
  - Dynamic source line breakpoints, single-stepping (`next`, `stepIn`, `stepOut`), and multi-frame stack trace inspection.
  - Dual-Region Memory Scope inspection in IDE debug pane: separate `Region A (Arena)` and `Region B (Persistent DAG)` variable inspectors.
- **Visual Dual-Region Memory Profiler**:
  - `trulang profile <file.tr> --output <path> --format <svg|json|folded>`.
  - Interactive SVG flamegraphs highlighting Region A bump allocations, Region B promotions (`persist()`), and elastic burst expansions.
  - Structured JSON and folded stack trace formats for CI/CD and external profiling tools.
- **TruLang 1.0 Stability RFC**:
  - Ratified formal specification in `docs/RFC-0001-STABILITY-1.0.md` with SemVer 2.0.0 backward compatibility guarantee.
- **VS Code Extension 1.0.0**:
  - Packaged extension `editors/vscode/trulang-1.0.0.vsix` with LSP language server, DAP debugger adapter, and memory profiling commands.

### Language Server Protocol & Core Developer Tools
- Integrated `trulang lsp --stdio` subcommand in core CLI binary for seamless IDE connectivity.
- Live syntax and typecheck diagnostics with actionable compiler hints.
- Rich hover tooltips for services, actor fields, memory budgets, functions, and imported modules.
- Document jump-to-definition across service declarations, fields, methods, functions, and imports.
- Automated code formatter (`trulang fmt <file> [--write]`).

### Safe FFI Bridge & Background Worker Pool
- `import "rust:pkg" as mod;` syntax for integrating native Rust and C libraries.
- Hybrid background worker pool (`WorkerPool`) offloading blocking foreign operations without starving actor mailboxes.
- Deterministic RAII destructors (`ExternalHandle<T>`) for foreign resources.
- Safe Region A arena marshaling enforcing memory budget quotas on foreign return values.

### Elastic Memory Burst Headroom & Auto-Promotion
- Tiered memory budgets: `memory_budget request = 16 MB max 64 MB;` with automatic doubling up to ceiling.
- Real-time Seqlock telemetry tracking `ArenaBurst` events.
- Transparent compiler auto-promotion pass eliminating manual `persist(...)` boilerplate on persistent mutations.

### Native AOT Code Generation & JIT Engine
- `trulang build [--release] <file> [-o <out>]`: Native ahead-of-time compilation producing standalone executables with zero external runtime dependencies.
- `trulang run --jit <file>`: High-speed on-the-fly JIT execution.
- Emits clean, optimized C source via `trulang build --emit-c <file>`.

---

## [0.1.0] - 2026-10-14

### Added
- **Dual-Region Execution Arenas**:
  - **Region A**: Fast bump-pointer allocator for transient, request-scoped objects, reclaimed in $O(1)$ constant time with **0.0 ms Garbage Collection pauses**.
  - **Region B**: Reference-counted, acyclic DAG storage for long-lived service state promoted explicitly via `persist(x)`.
- **Zero-Lock Service Actors**:
  - Single-threaded actor execution eliminating data races, mutex lock contention, and deadlocks.
  - Bounded mailbox queues with automatic backpressure load-shedding (`ActorError::MailboxFull`).
- **Real-Time Out-of-Process Seqlock Telemetry**:
  - Independent memory supervisor streaming metrics via lockless atomic shared memory (`/tmp/trulang_shm_active.dat`).
  - Zero-overhead performance profiling on running worker processes.
- **Language Syntax & Static Typing**:
  - Immutable by default (`let` vs `let mut`).
  - Static typechecker with local type inference and nullable types (`T?`).
  - Safe error handling with `try/catch` and deterministic `defer` RAII cleanup.
  - Checked integer arithmetic preventing overflow/underflow exploits.
  - Recursive descent parser with strict recursion depth protection (`MAX_RECURSION_DEPTH = 256`).
- **CLI & Developer Tooling**:
  - `trulang check <file>`: Static syntax, typing, and ownership verification.
  - `trulang run <file>`: High-speed bytecode compiler and virtual machine execution.
  - `trulang monitor`: Real-time streaming supervisor monitor.
  - `trulang bench <file>`: Multi-threaded concurrent benchmarking suite with latency percentile analysis.
  - Version inspection: Support for `trulang -v`, `trulang -V`, `trulang --version`, and `trulang version`.
- **Cross-Platform Releases & CI/CD**:
  - Automated GitHub Actions release pipeline (`.github/workflows/release.yml`) cross-compiling release binaries for:
    - **macOS ARM64** (Apple Silicon M1/M2/M3/M4) -> `trulang-darwin-arm64.tar.gz`
    - **macOS x86_64** (Intel) -> `trulang-darwin-x86_64.tar.gz`
    - **Linux x86_64** (Ubuntu / Debian / CentOS / Alpine) -> `trulang-linux-x86_64.tar.gz`
    - **Windows x64** (MSVC) -> `trulang-windows-x64.zip`
  - Automated 1-line installation script (`scripts/install.sh`) detecting OS/arch and fetching prebuilt standalone binaries directly from GitHub Releases.
- **Cross-Platform Shared Memory (`memmap2`)**:
  - Replaced Unix-specific `libc::mmap` with `memmap2`, enabling full native Windows memory mapping alongside Linux and macOS.
- **Interactive Documentation Portal (`docs/index.html`)**:
  - Complete language specification with interactive sample loader.
  - In-browser bytecode VM playground.
  - Interactive multi-language benchmark visualizer comparing TruLang against Rust, Go, Node.js, Java, and Python.
