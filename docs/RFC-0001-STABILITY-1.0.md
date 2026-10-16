# RFC 0001: TruLang 1.0 GA Stability & Semantic Versioning Guarantee

- **Status:** Ratified & Active (1.0 GA)
- **Author:** TruLang Core Architecture Team
- **Created:** October 2026
- **Category:** Standards Track / Core Specification

---

## 1. Executive Summary & Vision

TruLang 1.0 marks the General Availability (GA) milestone of a production-ready, memory-safe backend programming language engineered specifically for high-throughput, latency-critical cloud microservices. TruLang eliminates garbage collection pauses ($0.0\text{ ms}$ GC) through its innovative **Dual-Region Memory Architecture**, while enforcing acyclic safety for long-lived state and microsecond-level async I/O.

This document establishes the **TruLang 1.0 Stability Promise**: a binding contract guaranteeing that all valid TruLang 1.0 source code, package manifests, and compiled bytecode will compile and run without modification on all future **TruLang 1.x** releases.

```
┌────────────────────────────────────────────────────────────────────────┐
│                   TRULANG 1.0 STABILITY PILLARS                        │
├────────────────────────────────────────────────────────────────────────┤
│ 1. Syntax & Grammar Stability      │ Lexer, parser & AST compatibility │
│ 2. Dual-Region Memory Contract     │ Region A bump + Region B DAG      │
│ 3. Actor Concurrency & Mailboxes   │ Backpressure & lockless telemetry │
│ 4. Standard Library Wire Protocols │ HTTP/1.1, PostgreSQL, Redis, OTel │
│ 5. Tooling Protocol Guarantees     │ LSP (stdio) & DAP (stdio)         │
│ 6. Cryptographic Package Integrity │ Tru.toml & Tru.lock SHA-256       │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Semantic Versioning Commitment (SemVer 2.0.0)

TruLang strictly adheres to [Semantic Versioning 2.0.0](https://semver.org/). Version numbers are structured as `MAJOR.MINOR.PATCH`:

1. **MAJOR Version (1.0 -> 2.0):**
   - Incrementing the MAJOR version indicates potential breaking API or language changes.
   - TruLang commits to maintaining the 1.x branch with security patches and bug fixes for a minimum of **36 months** following the release of 1.0 GA.

2. **MINOR Version (1.0 -> 1.1 -> 1.x):**
   - Added functionality, performance optimizations, and standard library extensions that are **strictly backward-compatible**.
   - No valid 1.0 code will break on any 1.x release.
   - If an API or construct is superseded, it will be marked `@deprecated` but will remain fully functional throughout the entire 1.x lifecycle.

3. **PATCH Version (1.0.0 -> 1.0.1):**
   - Strictly bug fixes, security hardening, and internal runtime optimizations.
   - Zero syntax, stdlib, or wire protocol modifications.

---

## 3. Language & Grammar Specification Guarantees

### 3.1 Lexical & Syntactic Constructs
The following language constructs are frozen and guaranteed for the 1.x lifecycle:

- **Functions & Control Flow:**
  - `fn name(arg: type, ...) -> ret_type { ... }`
  - `let`, `let mut`, `if / else`, `while`, `return`
  - Inline block closures and nested scopes
- **Dual-Region Memory Operations:**
  - Implicit Region A bump allocation with $O(1)$ bulk arena reclamation upon handler return
  - `persist(expr)` promoting values into Region B persistent ARC memory
  - Cyclic DAG verification: all promoted graphs must remain acyclic Directed Acyclic Graphs (DAGs)
- **Actor Declarations & Mailboxes:**
  - `actor ServiceName { ... }`
  - Explicit bounded mailbox configurations: `#[mailbox(capacity = N, backpressure = "reject")]`
  - Automatic supervision and lockless Seqlock telemetry export
- **Built-in Test Runner Declarations:**
  - `test "description" { ... }`
  - Assertions: `assert(cond, [msg])`, `assert_eq(left, right, [msg])`
- **Object & Map Literals:**
  - Key-value object literals: `{ key: val, ... }`
  - Array literals: `[item, ...]`

### 3.2 Type System Guarantees
- Built-in primitives: `i64`, `f64`, `bool`, `string`, `null`
- Dynamic structures: `array`, `map`, `object`, `handle`
- Nominal type inference and static return type checking
- Safe handling of arithmetic overflow: division by zero and integer overflows trigger explicit recoverable `ArithmeticOverflow` or `DivisionByZero` runtime errors rather than undefined behavior.

---

## 4. Dual-Region Memory Model Contract

```
Incoming Request Wire Frame
         │
         ▼
┌─────────────────────────────────┐
│ Region A: Transient Bump Arena  │ ──► Request Response Serialized
│  • Wire frame buffers           │ ──► Entire arena wiped in O(1)!
│  • Parsed JSON / DB DataRows    │
│  • Local stack registers        │
└─────────────────────────────────┘
         │
         │ persist() promotion
         ▼
┌─────────────────────────────────┐
│ Region B: Persistent State DAG  │ ──► Survives per-request arena wipes
│  • Connection pools (DB/Redis)  │ ──► Atomic RefCount (ARC) acyclic DAG
│  • Actor state & caches         │
└─────────────────────────────────┘
```

1. **Region A (Execution Arena):**
   - Per-request or per-worker memory allocation uses contiguous bump memory.
   - Allocation time complexity is strictly $O(1)$.
   - Deallocation time complexity is strictly $O(1)$ by resetting the bump pointer to offset 0.
   - Elastic memory budgets are enforced: base budget with headroom ceiling. Exceeding the ceiling triggers recoverable `MemoryExceeded` errors.

2. **Region B (Persistent State DAG):**
   - Managed via Atomic Reference Counting (`Arc<T>`).
   - Cycles are strictly forbidden and detected at promotion time (`CyclicPersistentData` error).
   - Long-lived connection pools, caches, and actor states remain active across millions of Region A request resets.

---

## 5. Standard Library & Wire Protocol Contracts

TruLang standard library modules exported through `import "trulang/<module>" as alias;` are guaranteed backward-compatible:

| Module Specifier | Canonical Type | Stability Contract |
| :--- | :--- | :--- |
| `trulang/http` | `http::Request`, `http::Response` | HTTP/1.1 wire parser, JSON response encoding, Keep-Alive connection pooling |
| `trulang/postgres` | `pg::create_pool`, `PgPool` | PostgreSQL Frontend/Backend Protocol v3.0, MD5 auth, DataRow parsing |
| `trulang/redis` | `redis::create_pool`, `RedisPool` | Redis RESP2/RESP3 binary protocol client, pipelining, string/hash operations |
| `trulang/trace` | `trace::create_span`, `TraceSpan` | W3C `traceparent` context extraction, span hierarchy, response header injection |
| `trulang/json` | `json.parse`, `json.stringify` | Zero-copy JSON serialization into Region A bump memory |

---

## 6. Developer Tooling & IDE Protocol Guarantees

TruLang 1.0 provides first-class, protocol-compliant tooling built into the unified CLI:

1. **Language Server Protocol (LSP):**
   - Invoked via `trulang lsp [--stdio]`.
   - Adheres to the official Language Server Protocol specification.
   - Diagnostics, go-to-definition, hover documentation, and source code formatting (`trulang fmt`).

2. **Debug Adapter Protocol (DAP):**
   - Invoked via `trulang dap [--stdio]`.
   - Adheres to the official Debug Adapter Protocol specification for Visual Studio Code.
   - Breakpoints in `.tr` files, stepping (`next`, `stepIn`, `stepOut`), stack traces, and dual-region memory inspection (`Region A` vs `Region B` scopes).

3. **Visual Memory Profiler:**
   - Invoked via `trulang profile <file> [--format svg|json|text]`.
   - Emits self-contained interactive SVG flamegraphs, structured JSON metrics, and folded stack text.
   - Accurately identifies `persist()` promotion paths and elastic budget burst events.

---

## 7. Package Manager & Cryptographic Integrity (`tpm`)

1. **Package Manifest (`Tru.toml`):**
   - Stable schema defining `[package]` (`name`, `version`, `authors`, `edition`, `license`) and `[dependencies]`.
2. **Cryptographic Lockfile (`Tru.lock`):**
   - Cryptographic SHA-256 verification of every vendored source file.
   - Bit-for-bit reproducible compilation across development, CI/CD, and production Kubernetes clusters.

---

## 8. Deprecation & Evolution Policy

To allow TruLang to continue evolving while protecting user code:
1. No feature or API will be removed in a minor release (`1.x`).
2. Deprecated features will emit clear compiler warnings detailing the recommended modern alternative.
3. Automated code migration tools (`trulang fix`) will be provided before any breaking changes are introduced in future major releases (`2.0`).

---

*Adopted by the TruLang Project Community and ratified for TruLang 1.0 General Availability.*
