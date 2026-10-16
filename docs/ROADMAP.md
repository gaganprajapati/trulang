# 🗺️ TruLang Roadmap: The Path to an Industry Standard

> **Document Version:** 1.0.0  
> **Target Milestone:** TruLang 1.0 Enterprise GA  
> **Status:** Active Reference Architecture & Implementation Plan  
> **Official Website:** [https://gagn.dev/trulang/](https://gagn.dev/trulang/)

---

## 🎯 Executive Summary & Vision

**TruLang v0.1.0** established the foundational computer science breakthrough:
1. **Dual-Region Memory Arenas:** Sub-microsecond per-request bump allocation in **Region A** reclaimed via $O(1)$ constant-time bulk resets (Zero Stop-the-World GC pauses), paired with acyclic reference-counted **Region B** persistent DAGs.
2. **Zero-Lock Actor Concurrency:** Single-threaded service actors eliminating mutex contention, data races, and deadlocks with bounded mailbox backpressure.
3. **Out-of-Process Seqlock Telemetry:** Lockless atomic memory telemetry accessible in real time via shared memory.
4. **Multi-Target Execution:** High-speed bytecode VM, native Ahead-Of-Time (AOT) C compiler, and JIT engine.

To transition from an innovative language runtime into a **global industry standard** adopted by enterprise engineering teams (joining the ranks of Go, Rust, and Node.js for backend microservices), TruLang must deliver a **batteries-included, enterprise-ready cloud production ecosystem**.

This roadmap outlines the **5 Core Pillars** and the **Phased Implementation Milestones** required to reach TruLang 1.0.

---

```
                       ┌────────────────────────────────────────────────────────┐
                       │          TruLang 1.0: Enterprise Standard              │
                       └───────────────────────────┬────────────────────────────┘
                                                   │
         ┌───────────────────┬─────────────────────┼─────────────────────┬───────────────────┐
         ▼                   ▼                     ▼                     ▼                   ▼
   ┌───────────┐      ┌───────────────┐     ┌─────────────┐      ┌───────────────┐     ┌───────────┐
   │ Pillar 1  │      │   Pillar 2    │     │  Pillar 3   │      │   Pillar 4    │     │ Pillar 5  │
   │  Async    │      │  Package Mgr  │     │  Database   │      │ Observability │     │ Developer │
   │  & HTTP   │      │  (`tpm`)      │     │  Protocols  │      │  & Cloud/K8s  │     │  Tooling  │
   └───────────┘      └───────────────┘     └─────────────┘      └───────────────┘     └───────────┘
```

---

## 🏛️ The 5 Pillars to Industry Standard

---

### 🌐 Pillar 1: Non-Blocking Asynchronous I/O & Built-in HTTP Engine

> **The Industry Benchmark:** *Can an engineer write a high-throughput JSON microservice with zero third-party dependencies in under 20 lines of code?*

#### 1. Native Asynchronous Event Loop (OS Poller)
- Implement non-blocking OS event readiness notification (`epoll` on Linux, `kqueue` on macOS, `IOCP` on Windows), analogous to Tokio or Go's netpoller.
- When an actor initiates network I/O or awaits a timer, its task yields execution without blocking kernel threads, resuming automatically when socket readiness events fire.

#### 2. Zero-Copy HTTP/1.1 & HTTP/2 Engine
- Built-in standard library module: `import "trulang/http" as http`.
- **The Core TruLang Advantage:** Sockets read directly into **Region A**. Request headers, parsed JSON parameters, and dynamic routing maps are bump-allocated in Region A. Once the HTTP response is dispatched, the entire request memory is wiped in $O(1)$ bulk reset without any garbage collector scan.
```trulang
service ApiGateway {
    memory_budget request = 16 MB max 64 MB;

    fn handle_request(req: http::Request) -> http::Response {
        let user_id = req.param("id");
        let body = req.json(); // Bump-allocated in Region A
        
        return http::Response::json({
            "status": "success",
            "user": user_id
        });
        // Request finishes: Region A is reset in O(1) with 0.0 ms GC pause!
    }
}
```

#### 3. Native TLS / HTTPS (Zero C Dependency)
- Standard library TLS encryption powered by pure, memory-safe cryptographic primitives (`rustls`), eliminating OpenSSL dynamic linking hurdles in microservice containers.

---

### 📦 Pillar 2: Package Ecosystem & Dependency Manager (`tpm`)

> **The Industry Benchmark:** *Can a developer easily share, import, and lock dependencies with reproducible hermetic builds?*

#### 1. Tru Package Manager CLI (`tpm`)
- First-class commands integrated into the compiler:
  - `trulang new <project_name>` — Scaffolds standard microservice template.
  - `trulang add <dependency>` — Adds package with semver resolution.
  - `trulang publish` — Signs and publishes packages.

#### 2. Project Manifest & Hermetic Lockfile
- `Tru.toml`: Human-readable package manifest:
  ```toml
  [package]
  name = "payment-service"
  version = "1.0.0"
  edition = "2026"

  [dependencies]
  "github.com/trulang/postgres" = "^1.2.0"
  "github.com/trulang/redis" = "^0.8.0"
  ```
- `Tru.lock`: Cryptographic SHA-256 hashes of all dependency trees guaranteeing identical, bit-for-bit reproducible builds across CI/CD pipelines.

#### 3. Decentralized Git-Based Resolution
- Similar to Go modules, dependencies resolve directly from Git repositories:
  ```trulang
  import "github.com/trulang/postgres" as pg;
  import "github.com/trulang/jwt" as jwt;
  ```

---

### 🗄️ Pillar 3: Database & Cloud Connectors ("Bread and Butter")

> **The Industry Benchmark:** *Can TruLang communicate natively with PostgreSQL, MySQL, Redis, and message queues at maximum line speed?*

#### 1. Dual-Region Database Architecture
Database drivers in TruLang leverage both memory regions to achieve unprecedented throughput:
- **Region B (Persistent State):** Long-lived connection pools, SSL session parameters, prepared statement caches, and authentication tokens live in Region B.
- **Region A (Transient Request):** Wire socket packets, binary row parsers, and query response objects are parsed into Region A bump memory and automatically cleared when the query handler returns.

```
Incoming DB Wire Bytes
        │
        ▼
┌────────────────────────────────┐
│ Region A (Bump Allocator)      │ ──► Query Response sent to client
│  • Wire frame decoders         │ ──► Entire Arena wiped in O(1)!
│  • Parsed rows & string columns│
└────────────────────────────────┘
        ▲
        │ Borrowed Connection
┌────────────────────────────────┐
│ Region B (Persistent DAG)      │
│  • Connection Pool (Active Sockets)
│  • Prepared Statement Hashes   │
└────────────────────────────────┘
```

#### 2. Native Wire Protocol Implementations
- **PostgreSQL / MySQL:** Pure TruLang/Rust async client communicating over binary wire protocols (Frontend/Backend Protocol v3.0).
- **Redis Client:** Sub-millisecond RESP3 protocol client supporting pipelining and cluster routing.
- **Message Queues:** Native streaming connectors for Apache Kafka, AWS SQS, and RabbitMQ with actor mailbox backpressure.

---

### 📊 Pillar 4: Production Observability & Kubernetes Readiness

> **The Industry Benchmark:** *Can a DevOps or SRE team deploy TruLang in a 10,000-pod Kubernetes cluster with native Prometheus, OpenTelemetry, and zero-downtime lifecycle hooks?*

#### 1. OpenTelemetry (OTel) Distributed Tracing
- Built-in span creation and context propagation:
  - Injected automatically across incoming HTTP headers (`traceparent`), service actor mailboxes, and outgoing database queries.
  - Zero-allocation spans structured within Region A.

#### 2. Native Prometheus `/metrics` Scraper
- Standard library HTTP metric exporter serving the live lockless Seqlock supervisor data:
  ```text
  # HELP trulang_arena_a_bytes_allocated Total bytes bump-allocated in Region A
  # TYPE trulang_arena_a_bytes_allocated counter
  trulang_arena_a_bytes_allocated 104857600
  
  # HELP trulang_arena_a_resets_total Total number of O(1) arena resets
  # TYPE trulang_arena_a_resets_total counter
  trulang_arena_a_resets_total 50000

  # HELP trulang_actor_mailbox_depth Current queued messages in actor mailboxes
  # TYPE trulang_actor_mailbox_depth gauge
  trulang_actor_mailbox_depth{actor="UserActor"} 3
  ```

#### 3. Kubernetes Container Lifecycle & Graceful Draining
- First-class handlers for `SIGTERM` and `SIGINT`:
  1. Stop accepting new ingress HTTP connections.
  2. Allow in-flight Region A request handlers to complete.
  3. Flush actor mailboxes up to a configurable timeout (e.g. 15s).
  4. Disconnect persistent database connections cleanly and exit with code 0.

---

### 🛠️ Pillar 5: Developer Tooling, Hardening & Language Stability

> **The Industry Benchmark:** *Do developers have enterprise-grade debugging, profiling, and language stability guarantees?*

#### 1. Built-in Test Runner (`trulang test`)
- Testing framework built directly into the CLI toolchain:
  ```bash
  trulang test
  ```
  - Test declarations natively in TruLang:
    ```trulang
    test "elastic budget overflow is safely caught" {
        let mut x = 1;
        assert(x == 1);
    }
    ```
  - Parallel test execution, colored failure diffs, and code coverage analysis.

#### 2. Debug Adapter Protocol (DAP) for VS Code
- Full interactive debugging in Visual Studio Code:
  - Set breakpoints in `.tr` files.
  - Step over, step into, and step out of actor message handlers.
  - Inspect Region A vs Region B variable values in the VS Code debug pane.

#### 3. Visual Memory Profiler (`trulang profile`)
- Live or recorded memory profiling tool generating interactive flamegraphs:
  - Identifies which code paths call `persist()` and promote to Region B.
  - Highlights requests that trigger elastic memory budget burst expansions.

#### 4. TruLang 1.0 Stability RFC & Semantic Versioning Guarantee
- A formal language specification guaranteeing backward compatibility:
  - Code compiled on TruLang 1.0 will continue to compile and run without modification on TruLang 1.x releases for years to come.

---

## 🚀 Phased Implementation Roadmap

| Phase | Milestone | Primary Deliverable | Target Timeline |
| :---: | :--- | :--- | :--- |
| **Phase 1** | **`trulang test` CLI Harness** | Built-in test runner subcommand, assertion macros, colored diffs, and test reporting. | Immediate Next |
| **Phase 2** | **Native Async HTTP Engine** | `trulang/http` standard library, non-blocking socket loop, and Region A zero-copy request lifecycle. | Milestone 2 |
| **Phase 3** | **Package Manager (`tpm`)** | `Tru.toml` manifest, `Tru.lock` SHA-256 verifier, and Git-based module resolution. | Milestone 3 |
| **Phase 4** | **Database Wire Protocols** | Async PostgreSQL and Redis binary wire drivers with Region B pooling. | Milestone 4 |
| **Phase 5** | **Cloud & Kubernetes Hardening** | Prometheus `/metrics`, OpenTelemetry tracing, graceful `SIGTERM` draining, and Docker Alpine OCI. | Milestone 5 |
| **Phase 6** | **DAP Debugger & TruLang 1.0 GA** | Interactive VS Code breakpoints, memory flamegraphs, and backward-compatible 1.0 RFC. | TruLang 1.0 GA |

---

## 📋 Implementation Checklist for Future Turns

When ready to implement any phase, use this checklist as your starting blueprint:

### Phase 1: `trulang test` Harness
- [ ] Add `TestDeclaration` AST node in `crates/syntax/src/ast.rs`
- [ ] Implement `assert(...)` and `assert_eq(a, b)` builtins in `crates/compiler` and `crates/vm`
- [ ] Implement `Commands::Test { path, filter }` in `crates/cli/src/main.rs`
- [ ] Add colored terminal reporting (green passes, red failure diffs with line numbers)
- [ ] Update VS Code extension with a "Run Test" CodeLens above `test "..."` blocks

### Phase 2: Native Async HTTP Engine
- [ ] Add non-blocking socket abstraction in `crates/ffi` or new `crates/net`
- [ ] Implement zero-copy HTTP/1.1 parser streaming into Region A
- [ ] Expose `http::Request` and `http::Response` stdlib types
- [ ] Build high-throughput REST API benchmark vs Go/Node.js

### Phase 3: Package Manager (`tpm`)
- [ ] Implement `Tru.toml` parser and `Tru.lock` generator in `crates/cli`
- [ ] Add `trulang add <git-url>` submodule / cache resolver in `~/.trulang/pkg/`
- [ ] Integrate package imports into `crates/compiler` module resolution

---

*Authored by the TruLang Core Team. Maintained at [github.com/gaganprajapati/trulang](https://github.com/gaganprajapati/trulang).*
