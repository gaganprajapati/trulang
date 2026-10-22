# TruLang Mission & Product Philosophy ⚡

> **Mission:** Build the world's highest-throughput, most resource-efficient backend programming language for cloud microservices by eliminating the trade-off between developer ergonomics and zero-GC performance.

---

## 1. The Core Problem We Solve

Cloud microservices run the global economy, yet developers are forced into an agonizing trade-off:

1. **The Managed GC Trap (Node.js, Go, Java, Python):**
   - Tracing Garbage Collectors periodically halt execution or consume 25-40% of CPU cycles just managing heaps.
   - Under heavy concurrency (5,000+ requests/sec), multi-gigabyte heaps suffer unpredictable **p99 latency spikes** (stop-the-world sweeps).
   - Single-threaded event loops (Node/Python) saturate individual cores and introduce memory bloat.

2. **The Systems Language Cognitive Burden (Rust, C++):**
   - While Rust provides sub-millisecond zero-GC speed, building product features involves constant wrestling with borrow-checker lifetimes (`'a`), complex async `Pin`/`Box` wrappers, and slow compile times.
   - C/C++ lacks memory safety, resulting inCVEs, use-after-free vulnerabilities, and buffer overflows.

---

## 2. The TruLang Solution: Dual-Region Memory Architecture

TruLang introduces a fundamentally different architectural model:

* **Region A (Transient Arena):** Linear bump allocator dedicated to per-request data (HTTP headers, JSON payloads, local variables). When the request finishes, the arena offset pointer is reset in $O(1)$ constant time ($0.0\text{ ms}$ GC pause time).
* **Region B (Persistent DAG):** Strict Acyclic Directed Graph arena for long-lived actor state and cross-request cache. Completely prevents circular memory leaks without requiring a background garbage collector.
* **Service Actor Concurrency:** Endpoints run inside isolated Service Actors with hardware memory budgets (e.g., `memory_budget: 64KB`). Rogue requests are shed before they can cause an Out-Of-Memory (OOM) crash on the host.

---

## 3. Guiding Product Principles

1. **Zero Unmeasured Allocations:** Every byte allocated is accounted for in either Region A or Region B.
2. **Ergonomics Without Compromise:** Write code that feels like TypeScript or Go, compile down to lean bytecode/machine code with the speed of Rust.
3. **Batteries-Included for Microservices:** Native async HTTP wire engines, package management, OpenTelemetry tracing, and database drivers built directly into the language distribution.
4. **Hardware Predictability:** An entry-level $12 cloud VPS must reliably handle 5,000+ concurrent requests without degradation.
5. **Strict Backward Compatibility:** Every release honors the SemVer 2.0 contract and the RFC-0001 Stability Promise.

---

## 4. Non-Goals

- **TruLang is NOT a general-purpose systems scripting language:** We do not optimize for shell scripting, desktop GUI applications, or OS kernels.
- **TruLang is NOT an untyped dynamic language:** All data structures and actor contracts must be statically analyzable and type-safe.
- **TruLang will NEVER adopt a global tracing garbage collector:** Zero-GC request lifetimes are our defining technical moat.
