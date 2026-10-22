# TruLang Master Roadmap Hub 🗺️

> **The Living Status & Development Roadmap for TruLang**  
> **Current Stable Release:** **TruLang 1.0.0 GA** (Production Ready)  
> **Active Milestone:** **Phase 7: Native TLS/HTTPS & WebSockets** (`v1.1.0`)  
> **Stability Promise:** [RFC-0001: TruLang 1.0 Stability Contract](docs/RFC-0001-STABILITY-1.0.md)

---

## 1. Milestone Tracking Matrix

| Phase | Title | Focus Area | Status | Target Release | Detailed Spec |
|:---:|---|---|:---:|:---:|:---:|
| **1** | **Native Microservice Engine** | Async HTTP (`trulang/http`), Region A reset, Routing | **✅ Completed** | `v1.0.0` | [01-ROADMAP-1.0-GA.md](roadmaps/completed/01-ROADMAP-1.0-GA.md#phase-1-native-microservice-runtime-and-zero-overhead-http-engine) |
| **2** | **Secure Package Manager** | `Tru.toml`, `tpm`, SHA-256 verification, vendoring | **✅ Completed** | `v1.0.0` | [01-ROADMAP-1.0-GA.md](roadmaps/completed/01-ROADMAP-1.0-GA.md#phase-2-tru-package-manager-tpm-and-dependency-ecosystem) |
| **3** | **Native Database Drivers** | Binary wire protocols (Redis RESP3 & Postgres v3.0) | **✅ Completed** | `v1.0.0` | [01-ROADMAP-1.0-GA.md](roadmaps/completed/01-ROADMAP-1.0-GA.md#phase-3-high-performance-native-database-wire-drivers) |
| **4** | **Developer Tooling & LSP** | Language Server Protocol, `trulang fmt`, `trulang test` | **✅ Completed** | `v1.0.0` | [01-ROADMAP-1.0-GA.md](roadmaps/completed/01-ROADMAP-1.0-GA.md#phase-4-production-tooling-developer-experience--ide-ecosystem) |
| **5** | **Cloud & K8s Hardening** | Prometheus `/metrics`, OTel tracing, graceful drain, Helm | **✅ Completed** | `v1.0.0` | [01-ROADMAP-1.0-GA.md](roadmaps/completed/01-ROADMAP-1.0-GA.md#phase-5-cloud-native-production-hardening-docker--kubernetes-orchestration) |
| **6** | **DAP Debugger & TruLang 1.0** | Debug Adapter Protocol, visual flamegraph profiler, GA | **✅ Completed** | `v1.0.0` | [01-ROADMAP-1.0-GA.md](roadmaps/completed/01-ROADMAP-1.0-GA.md#phase-6-interactive-debugger-dap-visual-memory-profiler--trulang-10-ga) |
| **7** | **Native TLS & WebSockets** | `rustls` HTTPS, WebSockets RFC 6455, SSE, HTTP/2 | **🎯 Next Up** | `v1.1.0` | [02-ROADMAP-2.0-SCALE.md](roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md#phase-7-native-tlshttps-websockets--streaming-protocols-next-milestone) |
| **8** | **WASM & Edge Runtime** | `wasm32-wasip1`, Cloudflare/Fastly Edge, In-Browser VM | **📋 Planned** | `v1.2.0` | [02-ROADMAP-2.0-SCALE.md](roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md#phase-8-webassembly-wasm--wasi-and-edge-sandboxes) |
| **9** | **Distributed Clustering** | Location-transparent actors, SWIM gossip, Raft consensus | **📋 Planned** | `v1.3.0` | [02-ROADMAP-2.0-SCALE.md](roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md#phase-9-distributed-actor-clustering--consensus-otp-grade-cluster-runtime) |
| **10** | **Central Package Registry** | `tpm.trulang.org`, VS Code Marketplace, `trulang doc` | **📋 Planned** | `v1.4.0` | [02-ROADMAP-2.0-SCALE.md](roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md#phase-10-central-package-registry--ecosystem-infrastructure) |
| **11** | **Cranelift Machine-Code JIT** | Native AOT/JIT, Hotspot OSR, Direct C-ABI FFI inlining | **📋 Planned** | `v1.5.0` | [02-ROADMAP-2.0-SCALE.md](roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md#phase-11-machine-code-jit--cranelift-native-backend) |
| **12** | **Enterprise Governance 2.0** | Live hot-code reload, 3-year LTS contract, chaos testing | **📋 Planned** | `v2.0.0` | [02-ROADMAP-2.0-SCALE.md](roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md#phase-12-enterprise-operational-governance--trulang-20-lts) |

---

## 2. Roadmap Document Organization

To ensure complete clarity on what is already delivered versus what lies ahead, roadmap documentation is structured as follows:

```
roadmaps/
├── README.md                           <-- Master Status Dashboard (this file)
├── GROWTH-AND-ADOPTION.md              <-- Go-To-Market, Hacker News Launch, & Community Strategy
├── completed/
│   └── 01-ROADMAP-1.0-GA.md            <-- Full specifications & verification of Phases 1 to 6
└── upcoming/
    └── 02-ROADMAP-2.0-SCALE.md         <-- Full technical specifications for Phases 7 to 12
```

- **[GROWTH-AND-ADOPTION.md](GROWTH-AND-ADOPTION.md):** The comprehensive Go-To-Market strategy, detailing the Hacker News "Show HN" playbook, technical deep-dive articles, $12 Server Challenge distribution, and contributor flywheel.

- **[01-ROADMAP-1.0-GA.md](roadmaps/completed/01-ROADMAP-1.0-GA.md):** The complete historical record of the 6 foundational phases that brought TruLang from prototype to 1.0 General Availability. Includes test verification logs (106/106 passing tests across 12 workspace crates) and architectural breakdown.
- **[02-ROADMAP-2.0-SCALE.md](roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md):** The forward-looking technical blueprint for TruLang 2.0 ("The Scale, Edge & Distributed Cloud Era"), detailing architectural requirements, crate targets, and acceptance criteria.

---

## 3. What Is Completed (TruLang 1.0 GA)

1. **Dual-Region Memory Engine:** Zero-pause garbage collection using per-request bump arenas (Region A) with constant-time $O(1)$ bulk resets and reference-counted graph storage (Region B).
2. **Native Async HTTP Engine (`trulang/http`):** Multi-platform OS readiness event loop (`kqueue` on macOS/BSD, `epoll` on Linux, `WSAPoll` on Windows).
3. **Secure Package Manager (`tpm`):** Deterministic resolution, SHA-256 integrity verification, and local vendoring via `Tru.toml` and `Tru.lock`.
4. **Native Database Wire Protocols:** Hand-crafted binary protocol engines for Redis (`RESP2`/`RESP3`) and PostgreSQL (v3.0) with pooling.
5. **Developer Tooling:** Built-in Language Server Protocol (`trulang lsp`), AST code formatter (`trulang fmt`), and native test runner (`trulang test`).
6. **Cloud & Kubernetes Hardening:** Prometheus `/metrics`, OpenTelemetry distributed tracing context propagation, graceful shutdown draining, and Helm charts.
7. **Interactive Debugger & Profiler:** Full Debug Adapter Protocol (`crates/dap`) daemon for VS Code interactive debugging and dual-region SVG flamegraph profiler (`trulang profile`).
8. **RFC-0001 Stability Promise:** Official SemVer 2.0.0 backward compatibility guarantee.

---

## 4. What Lies Ahead (TruLang 2.0 Horizons)

The immediate focus is **Phase 7: Native TLS/HTTPS & WebSockets (`v1.1.0`)**:
- Native `rustls` integration directly in the event loop for zero-overhead HTTPS without requiring an Nginx/Envoy reverse proxy.
- Bidirectional WebSockets with Region A per-frame memory recycling.
- Server-Sent Events (SSE) streaming generators.

For complete specifications on all upcoming phases (WASM, Actor Clustering, Central Registry, Cranelift JIT, and Enterprise 2.0 LTS), refer to [**02-ROADMAP-2.0-SCALE.md**](roadmaps/upcoming/02-ROADMAP-2.0-SCALE.md).
