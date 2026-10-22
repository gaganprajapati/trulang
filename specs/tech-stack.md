# TruLang Technical Stack & Architectural Guidelines 🛠️

> **Engineering Standard:** This document defines the compiler toolchain, crate topology, architectural constraints, and coding standards for all TruLang implementations.

---

## 1. Compiler Toolchain & Host Environment

- **Host Implementation Language:** Modern, safe Rust (Stable 1.75+).
- **Build System & Package Manager:** Cargo with workspace-level dependency deduplication.
- **Cross-Platform Target Matrix:**
  - macOS (Apple Silicon `aarch64-apple-darwin` + Intel `x86_64-apple-darwin`) via `kqueue`.
  - Linux (`x86_64-unknown-linux-gnu` + future `aarch64-unknown-linux-gnu`) via `epoll`.
  - Windows (`x86_64-pc-windows-msvc`) via `WSAPoll`.
  - WebAssembly (`wasm32-unknown-unknown`, `wasm32-wasip1`) for edge runtimes and in-browser execution.

---

## 2. Workspace Crate Architecture

The TruLang engine is modularized into strictly decoupled crates:

```
crates/
├── syntax/             # Lexer (Logos), Recursive Descent Parser, AST definitions, Span tracking
├── compiler/           # AST Typechecker, Scope analysis, Constant folding, Bytecode generator
├── vm/                 # Bytecode VM, Dual-Region Allocators (Region A Bump, Region B DAG), Actor runtime
├── net/                # Native non-blocking HTTP/1.1 poller (kqueue/epoll/WSAPoll), TLS (rustls), WS
├── supervisor/         # Seqlock shared-memory atomic telemetry & health monitor (memmap2)
├── pkg/                # Tru Package Manager (tpm) resolution engine, Tru.toml parser, SHA-256 verifier
├── lsp/                # Language Server Protocol daemon (diagnostics, definitions, hover, completions)
├── dap/                # Debug Adapter Protocol daemon (breakpoints, stepping, stack inspection)
└── cli/                # The standalone developer binary (`trulang run, check, test, fmt, bench`)
```

---

## 3. Dependency Governance Rules

1. **Pure-Rust Primitives Only:**  
   Avoid external C libraries or dynamically linked dependencies (e.g. use `rustls` instead of OpenSSL). The TruLang binary must be completely self-contained and statically linked.
2. **Zero-Unsafe Workspace Policy:**  
   Any `unsafe` code must be isolated to memory allocator internals (`crates/vm/src/arena.rs`) and low-level OS poller bindings (`crates/net/src/poller.rs`). Every `unsafe` block must include an explicit `// SAFETY:` rationale.
3. **No Hidden Background Threads:**  
   Concurrency must be driven strictly through the non-blocking event loop or isolated Service Actor mailboxes.

---

## 4. Memory Model Engineering Rules

All implementations must follow strict dual-region discipline:

* **Region A (Transient Arena):**
  - Linear bump allocator for request-lifetime allocations (HTTP headers, JSON deserialization, function frames).
  - Allocation is a single pointer addition: `ptr += size`.
  - Reset is $O(1)$ constant time: `offset = 0`.
  - No individual `free()` or tracking destructors allowed in Region A.
* **Region B (Persistent DAG):**
  - Directed Acyclic Graph arena for actor state, caches, and long-lived session state.
  - References are acyclic by design: references can only point to older nodes or immutable state, preventing circular reference memory leaks.
* **Memory Budgets:**
  - Every Service Actor enforces a hard ceiling (`memory_budget: X KB/MB`).
  - Allocation attempts exceeding budget must safely fail or shed the request with HTTP 429/507 before causing an OS OOM kill.

---

## 5. Testing & Code Quality Gates

Before any PR or spec can be merged, it must satisfy:

1. **Clean Workspace Tests:** `cargo test --workspace` passes with 0 failures.
2. **Zero Compiler Warnings:** `cargo clippy --workspace -- -D warnings` passes cleanly.
3. **Deterministic Formatting:** `cargo fmt --check` has 0 diffs.
4. **Cross-Platform Verification:** Passes CI on macOS, Linux, and Windows.
