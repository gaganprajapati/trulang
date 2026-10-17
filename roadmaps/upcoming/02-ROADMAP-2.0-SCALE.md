# TruLang 2.0 Roadmap: The Scale, Edge & Distributed Cloud Era 🚀

> **Target Release:** TruLang 2.0  
> **Status:** Active & Open for Implementation  
> **Predecessor Roadmap:** [Phase 1–6: TruLang 1.0 GA (Completed)](../completed/01-ROADMAP-1.0-GA.md)  
> **Master Dashboard:** [Roadmaps Hub](../README.md) • [RFC-0001: 1.0 Stability Promise](../../docs/RFC-0001-STABILITY-1.0.md)

---

## 1. Executive Vision

Having completed the core foundations in TruLang 1.0 GA (Dual-Region Memory, native async HTTP engine, package manager, database drivers, LSP, and DAP interactive debugging), the next milestone transforms TruLang from a single-node microservice runtime into a **massively scalable, edge-native, and distributed cloud platform**.

```
                                      ┌──────────────────────────────────────────┐
                                      │       TruLang 1.0 GA (Released)          │
                                      │ Dual-Region Memory • Native HTTP • LSP   │
                                      └────────────────────┬─────────────────────┘
                                                           │
         ┌─────────────────────────┬───────────────────────┴───────────────────────┬─────────────────────────┐
         ▼                         ▼                                               ▼                         ▼
┌──────────────────┐      ┌──────────────────┐                           ┌──────────────────┐      ┌──────────────────┐
│ Phase 7: TLS &   │      │ Phase 8: WASM &  │                           │ Phase 9: Cluster │      │ Phase 10: Central│
│    WebSockets    │      │    Edge Runtime  │                           │    & Consensus   │      │    Registry & Hub│
├──────────────────┤      ├──────────────────┤                           ├──────────────────┤      ├──────────────────┤
│ • Native HTTPS   │      │ • wasm32-wasip1  │                           │ • Location-      │      │ • tpm.trulang.org│
│   via rustls     │      │ • Edge sandboxes │                           │   transparent    │      │ • In-Browser IDE │
│ • WebSockets &   │      │ • In-Browser VM  │                           │   actor network  │      │ • Automated Doc  │
│   SSE streaming  │      │   Playground     │                           │ • Raft state     │      │   generation     │
└──────────────────┘      └──────────────────┘                           └──────────────────┘      └──────────────────┘
                                                           │
                                                           ▼
                                      ┌──────────────────────────────────────────┐
                                      │ Phase 11 & 12: Cranelift JIT & 2.0 LTS   │
                                      │ Extreme Machine Code • Enterprise Chaos  │
                                      └──────────────────────────────────────────┘
```

---

## 2. Technical Specifications by Phase

### Phase 7: Native TLS/HTTPS, WebSockets & Streaming Protocols (Next Milestone)
* **Status:** 🎯 **Next Up for Implementation**
* **Target Version:** `v1.1.0`
* **Target Crates:** `crates/net` (`trulang/tls`, `trulang/ws`, `trulang/grpc`)

#### Architectural Deliverables:
1. **Zero-Cost Native TLS/HTTPS (`trulang/tls`):**
   - Direct integration of `rustls` (pure Rust, modern TLS 1.3/1.2, memory-safe, zero OpenSSL dependency).
   - Zero-copy ALPN negotiation (`http/1.1`, `h2`) on the native OS poller loop (`kqueue`/`epoll`/`WSAPoll`).
   - Dynamic SNI certificate reloading without process restarts.
2. **Native Asynchronous WebSockets (`trulang/ws`):**
   - RFC 6455 compliant WebSocket upgrade handshake over native HTTP engine.
   - Region A per-frame arena allocation: frame headers and masked payloads recycled in O(1) time upon message dispatch.
   - Bidirectional actor binding: WebSocket connection handles automatically map to Service Actor mailboxes.
3. **Server-Sent Events (SSE):**
   - Native SSE streaming generator for event broadcasts with automatic keep-alive ping frames.
4. **HTTP/2 Wire Engine & gRPC Foundation:**
   - Multiplexed stream handling over a single TCP/TLS connection.
   - Binary Protobuf frame decoding integrated with Region A arenas.

#### Acceptance Criteria:
- [ ] Automated end-to-end HTTPS benchmark: < 1ms TLS 1.3 handshake overhead.
- [ ] 100,000 concurrent idle WebSocket connections maintained with < 20 MB total RSS.
- [ ] Full RFC 6455 compliance test suite passing (autobahn test suite subset).

---

### Phase 8: WebAssembly (WASM & WASI) and Edge Sandboxes
* **Status:** 📋 Planned
* **Target Version:** `v1.2.0`
* **Target Crates:** `crates/vm`, `crates/codegen`, `editors/vscode`

#### Architectural Deliverables:
1. **`wasm32-wasip1` Target Compilation:**
   - Compile the TruLang VM core and runtime into WebAssembly with standard WASI system calls.
   - Zero-overhead memory model: Region A and Region B arenas mapped to continuous linear WASM memory pages.
2. **Serverless Edge Runtimes:**
   - First-class bindings for Cloudflare Workers, Fastly Compute, and AWS Lambda WebAssembly runtimes.
   - Sub-millisecond cold start times with snapshot restoration.
3. **Interactive In-Browser Playground (`play.trulang.org`):**
   - Embed the full TruLang compiler and bytecode VM in the browser documentation portal.
   - Live visual dual-region memory visualizer directly inspecting interactive SVG flamegraphs in real time.

#### Acceptance Criteria:
- [ ] TruLang VM compiles to a standalone `.wasm` binary < 600 KB.
- [ ] In-browser REPL executes TruLang programs and renders live memory arenas with 0ms server latency.
- [ ] CI pipeline validates automated `wasm32-wasip1` builds and execution via `wasmtime`.

---

### Phase 9: Distributed Actor Clustering & Consensus (OTP-Grade Cluster Runtime)
* **Status:** 📋 Planned
* **Target Version:** `v1.3.0`
* **Target Crates:** `crates/cluster` (new crate: `trulang_cluster`), `crates/supervisor`

#### Architectural Deliverables:
1. **Location-Transparent Actor Addressing:**
   - Unified Actor PID addressing format: `node_id:actor_id` (e.g., `cluster://us-east-1/order_processor#42`).
   - Remote message serialization over high-efficiency binary RPC with connection pooling.
2. **Gossip-Based Membership & Failure Detection:**
   - Decentralized SWIM-style cluster membership protocol with automatic peer discovery.
   - Heartbeat monitoring with configurable failure timeouts (phi-accrual failure detector).
3. **Cluster Supervisor & Auto-Failover:**
   - Supervised actor pools that automatically redistribute worker actors across surviving nodes when a node fails.
4. **Distributed State Machine (Raft Engine):**
   - High-performance Raft consensus implementation built on TruLang persistent Region B graphs.
   - Quorum-based leader election, replicated write-ahead logs, and snapshot state compaction.

#### Acceptance Criteria:
- [ ] Network partition chaos test: Cluster successfully detects node failure in < 500ms and restores quorum.
- [ ] Actor message passing transparently works across a 5-node cluster with zero code changes in the TruLang script.
- [ ] Sustained throughput of > 250,000 remote actor messages per second over 10GbE network interfaces.

---

### Phase 10: Central Package Registry & Ecosystem Infrastructure
* **Status:** 📋 Planned
* **Target Version:** `v1.4.0`
* **Target Crates:** `crates/pkg`, `registry.trulang.org`

#### Architectural Deliverables:
1. **TruLang Central Registry API (`tpm.trulang.org`):**
   - Production registry service supporting `trulang publish`, `trulang install`, and `trulang search`.
   - Immutable package storage with content-addressable SHA-256 storage backends (S3/GCS).
2. **Cryptographic Security & Verification:**
   - Ed25519 author signing keys for published packages.
   - Automated vulnerability checking and dependency tree graph verification during `trulang install`.
3. **Automated Documentation Engine (`trulang doc --gen`):**
   - Code-doc extractor parsing TruLang doc comments (`/// ...`) into interactive static HTML portals.
   - Automated doc hosting at `docs.trulang.org/<package>/<version>`.
4. **IDE Marketplace Distribution:**
   - Official release of `trulang-1.0.0.vsix` to the Microsoft Visual Studio Code Marketplace and Open VSX Registry.

#### Acceptance Criteria:
- [ ] `trulang publish` authenticates, signs, packages, and uploads to the staging registry in < 2 seconds.
- [ ] `trulang install` downloads, verifies cryptographic signatures, and vendors packages with deterministic lockfiles.
- [ ] Extension published and discoverable on VS Code Marketplace with automatic LSP/DAP binary downloads.

---

### Phase 11: Machine-Code JIT & Cranelift Native Backend
* **Status:** 📋 Planned
* **Target Version:** `v1.5.0`
* **Target Crates:** `crates/codegen`, `crates/vm`

#### Architectural Deliverables:
1. **Cranelift Code Generation Pipeline:**
   - Translate TruLang intermediate bytecode into Cranelift IR (Intermediate Representation).
   - Generate optimized native machine code for `aarch64` (Apple Silicon, AWS Graviton) and `x86_64`.
2. **Tiered Execution Engine:**
   - Tier 1: Fast Bytecode Interpreter with zero startup latency.
   - Tier 2: Hotspot detector tracking loop iteration counters and function invocation frequencies.
   - Tier 3: Cranelift JIT compilation of hot loops into executable memory pages with seamless OSR (On-Stack Replacement).
3. **Zero-Cost Direct C-ABI FFI Inlining:**
   - Inline foreign C/Rust ABI function calls directly into compiled native assembly frames, eliminating call boundary marshaling.

#### Acceptance Criteria:
- [ ] 3x to 5x throughput improvement over bytecode interpretation in compute-heavy benchmarks (Fibonacci, Matrix multiplication, JSON serialization).
- [ ] Sub-5ms JIT compilation latency for hot function loops.

---

### Phase 12: Enterprise Operational Governance & TruLang 2.0 LTS
* **Status:** 📋 Planned
* **Target Version:** `v2.0.0`
* **Target Crates:** Workspace-wide

#### Architectural Deliverables:
1. **Long-Term Support (LTS) Contract:**
   - 3-year commitment to bug fixes, security patches, and binary backward compatibility (SemVer 2.0.0).
2. **Zero-Downtime Hot Code Reloading:**
   - Live bytecode updates in running Kubernetes pods without terminating TCP connections.
   - State migration: Actor mailboxes drained into new version actors using Region B graph re-linking.
3. **Enterprise Chaos Engineering Harness:**
   - Automated CI chaos suites injecting network latency, packet loss, OOM conditions, and supervisor worker kills.

#### Acceptance Criteria:
- [ ] Hot code reload swaps microservice handler bytecode under 10,000 req/sec load with 0 dropped requests and 0 HTTP 5xx errors.
- [ ] 99.999% uptime achieved across 72-hour continuous chaos stress test.

---

## 3. Guiding Principles

1. **Dual-Region Discipline Always:**
   All features must honor Region A for request lifetimes and Region B for long-lived state. No global locks, no unmeasured allocations.
2. **SemVer & RFC-0001 Stability:**
   All code written for TruLang 1.0 will run without modification on TruLang 2.0.
3. **Cross-Platform Verification:**
   All milestones must compile, test, and pass cleanly on macOS (Apple Silicon + Intel), Linux (x86_64), and Windows (MSVC).
