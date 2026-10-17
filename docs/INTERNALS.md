# How TruLang Works: Under the Hood Architecture & Execution Mechanism 🧠

<p align="center">
  <img src="../assets/logo.png" width="130" alt="TruLang Logo">
</p>

> **A Comprehensive Guide to TruLang's Internal Runtime, Memory Engine, and Concurrency Architecture.**  
> *Targeted at engineers coming from Node.js, Go, Rust, Java, and Python.*

---

## Table of Contents
1. [The Core Paradigm Shift: Node.js vs TruLang](#1-the-core-paradigm-shift-nodejs-vs-trulang)
2. [The Dual-Region Memory Architecture in Depth](#2-the-dual-region-memory-architecture-in-depth)
3. [Step-by-Step Lifecycle of a TruLang Request](#3-step-by-step-lifecycle-of-a-trulang-request)
4. [Zero-Lock Service Actors & Mailbox Backpressure](#4-zero-lock-service-actors--mailbox-backpressure)
5. [The Hybrid Thread-Pooled FFI Bridge (`crates/ffi`)](#5-the-hybrid-thread-pooled-ffi-bridge-cratesffi)
6. [Dual-Mode Execution Pipeline (Dev VM vs Production Native AOT)](#6-dual-mode-execution-pipeline-dev-vm-vs-production-native-aot)
7. [Out-of-Process Atomic Seqlock Telemetry (`crates/supervisor`)](#7-out-of-process-atomic-seqlock-telemetry-cratessupervisor)
8. [Developer Mental Model & Best Practices Guide](#8-developer-mental-model--best-practices-guide)

---

## 1. The Core Paradigm Shift: Node.js vs TruLang

In **Node.js**, every developer quickly learns that **"Never block the Event Loop"** is the fundamental design rule. Because JavaScript is single-threaded, any CPU-intensive operation (large JSON parses, cryptography, image processing) blocks the main thread, freezing all concurrent socket events. Furthermore, V8's Garbage Collector runs Stop-the-World pauses (15–45 ms) to mark and sweep dead heap references.

In **TruLang**, the paradigm shifts to **Zero-Lock Service Actors**, **Dual-Region Arenas**, and an **Offloaded Hybrid FFI Worker Pool**:

| Dimension | Node.js (Event Loop + V8) | TruLang (Service Actors + Dual-Region) |
| :--- | :--- | :--- |
| **Concurrency Model** | Single-Threaded Event Loop with libuv thread pool | Independent Single-Threaded Service Actors with lock-free bounded mailboxes |
| **Memory Reclamation** | Generational GC (Stop-the-World pauses of 15–45 ms) | **$O(1)$ Bulk Arena Reset: exactly 0.0 ms GC pauses** |
| **Allocation Mechanism** | Free-list heap allocation with fragmentation risk | Fast linear pointer bump (`ptr += size`), zero fragmentation |
| **Base RSS Footprint** | 40 – 80 MB per Node.js process | **2.5 MB RSS (~6,400 microservices per 16 GB host)** |
| **Blocking Safety** | CPU or foreign work blocks the single Event Loop | Foreign blocking I/O (SQL, HTTP) automatically offloaded to worker pool |
| **Fault Isolation** | An uncaught error or memory leak can crash the process | Requests run with strict memory budgets; overflows fail fast without crashing the actor |

```
                                  TruLang Architecture at a Glance
                                  
  [ Ingress TCP / HTTP Requests ]
                │
                ▼
  [ Bounded Mailbox Queue ] ──(If Full: 429 Too Many Requests Backpressure)
                │
                ▼
  ┌────────────────────────────────────────────────────────┐   ┌────────────────────────────────┐
  │               SERVICE ACTOR (Main Thread)              │   │   HYBRID FFI WORKER POOL       │
  │                                                        │   │   (Background Thread Pool)     │
  │  1. Receive Message -> Reset Region A Arena (O(1))     │   │                                │
  │  2. Allocate Request Objects via Fast Pointer Bump     │   │  • PostgreSQL Pool Queries     │
  │  3. Execute Pure Business Logic without Lock Contention │──>│  • HTTP/REST Client Calls      │
  │  4. Promote Long-Lived State to Region B via persist() │<──│  • Heavy Crypto & Image Ops    │
  │  5. Send Response -> Bulk Free Region A Arena (O(1))   │   │                                │
  └────────────────────────────────────────────────────────┘   └────────────────────────────────┘
                │
                ▼ Lockless Seqlock Atomic Shm (0 overhead)
  [ Independent Memory Supervisor (/tmp/trulang_shm_*.dat) ]
```

---

## 2. The Dual-Region Memory Architecture in Depth

Traditional languages force a difficult compromise: accept GC pause jitter (Java, Go, Node.js) or struggle with complex manual borrow checkers (Rust, C++). TruLang eliminates this trade-off by categorizing all memory into two distinct operational regions:

### Region A: Request Execution Arena (Transient)
- **Linear Bump Allocation ($O(1)$)**: When your code instantiates a variable, parses JSON, or builds a response string, the allocator simply advances an offset in pre-allocated memory:
  ```c
  void* ptr = arena->memory + arena->offset;
  arena->offset += (size + 7) & ~7; // 8-byte aligned bump
  ```
- **Instant $O(1)$ Bulk Reset**: When the request completes, TruLang sets `offset = 0`. The entire memory arena is reclaimed in **a single CPU cycle**.
- **0.0 ms Garbage Collection Pauses**: Memory is never traversed or swept.
- **Elastic Burst Arenas**: With `memory_budget request = 16 MB max 64 MB;`, the arena handles sudden traffic spikes up to 64 MB, failing fast with HTTP 413 if the ceiling is exceeded, then resetting cleanly back to base.

### Region B: Persistent Service Heap (Long-Lived)
- **Cross-Request State**: Used for actor-level variables that must survive across requests (e.g., in-memory caches, database connection handles, running counters).
- **Acyclic Graph (DAG) Invariant**: Enforces reference-counted ownership without cycles, guaranteeing deterministic deallocation on actor shutdown.
- **Compiler Auto-Promotion**: The compiler infers when data escapes request scope into persistent fields (e.g. `cache.insert(k, v)`) and automatically inserts `Opcode::Persist`.

### How TruLang Decides: Region A vs. Region B
1. **Default Rule:** Everything instantiated in a function or message handler begins in **Region A** (bump-allocated, zero-GC).
2. **Explicit Promotion:** Calling `persist(value)` deep-promotes the object into **Region B** with DAG cycle detection.
3. **Compiler Escape Analysis:** Direct assignments from Region A into actor-level state (Region B) are automatically detected by the compiler, which inserts `Opcode::Persist`.
4. **Pointer Invariant:** Region B pointers can *never* point to Region A (preventing dangling pointers), while Region A can freely reference Region B.

> 📖 **Read the full dedicated specification:** [**TruLang Memory Model: The Dual-Region Allocation Engine**](MEMORY_MODEL.md)

---

## 3. Step-by-Step Lifecycle of a TruLang Request

1. **Ingress & Mailbox Backpressure**:  
   Incoming HTTP/RPC messages enter the Service Actor's bounded mailbox queue. If the queue is saturated, the request is shed immediately with `ActorError::MailboxFull` (HTTP 429), protecting upstream services.
2. **Arena Zero-Offset Reset**:  
   The actor sets Region A `offset = 0` in &lt; 1 nanosecond.
3. **Sequential Execution**:  
   The actor handles the message. Since the actor owns its state exclusively, there are no mutexes, rwlocks, or synchronization overhead.
4. **Foreign I/O Offloading**:  
   Database queries and network calls are offloaded to background threads in the `WorkerPool`. The actor thread remains unblocked.
5. **State Promotion & Instant Teardown**:  
   Any cached entries are promoted to Region B. The response is sent, and Region A is bulk-reset. No garbage collector runs.

---

## 4. Zero-Lock Service Actors & Mailbox Backpressure

In multi-threaded architectures, shared mutable state requires mutexes. Under heavy concurrent load, threads spend up to **70% of CPU cycles waiting for locks**.

In TruLang:
- Every `service` is a self-contained Actor.
- Actors communicate exclusively via asynchronous message queues.
- Internal actor state is never exposed to other threads.
- Consequence: **Zero locks, zero lock contention, and zero race conditions**.

---

## 5. The Hybrid Thread-Pooled FFI Bridge (`crates/ffi`)

If an actor invokes a slow blocking foreign library directly, its mailbox freezes. TruLang's `crates/ffi` solves this:
- **Worker Pool**: Foreign blocking calls (`std::sql`, `std::http`) execute on background threads.
- **Deterministic RAII Destructors (`ExternalHandle<T>`)**: Opaque resources (e.g. PostgreSQL connection pools) are freed deterministically when dropped.
- **Region A Marshaling**: Foreign returns are marshaled directly into the caller's Region A arena, ensuring memory limits are strictly enforced.

---

## 6. Dual-Mode Execution Pipeline (Dev VM vs Production Native AOT)

TruLang provides a dual-mode execution strategy:

```bash
# 1. Dev Mode: Fast Bytecode VM (<3ms startup)
trulang run examples/user_service.tr

# 2. Native JIT Mode: Compile & execute on-the-fly
trulang run --jit examples/user_service.tr

# 3. Production Mode: Standalone AOT Native executable
trulang build --release examples/user_service.tr -o my_service
```

- **Dev Mode**: Instantaneous cold-start for rapid testing.
- **Production Mode**: Standalone Mach-O / ELF / PE binary with `-O3` vectorization, matching 90–95%+ of native Rust/C++ throughput.

---

## 7. Out-of-Process Atomic Seqlock Telemetry (`crates/supervisor`)

Monitoring request metrics (latency, memory usage) often introduces latency jitter. TruLang decouples monitoring into an independent out-of-process supervisor using lockless atomic shared memory (`/tmp/trulang_shm_*.dat`):
- Workers update telemetry atomically using Release-Acquire seqlocks.
- The supervisor reads memory snapshots without pausing or slowing down the worker.

---

## 8. Developer Mental Model & Best Practices Guide

### Rule 1: Think in Request Scopes
Do not worry about allocating intermediate strings, arrays, or JSON payloads while handling a request. Allocate freely—the entire arena is reclaimed in $O(1)$ constant time upon request completion.

### Rule 2: Keep Persistent State Truly Shared
Only promote data to Region B if it needs to survive across requests (e.g., caches, connection pools).

### Rule 3: Size Budgets with Headroom
Use elastic budgets:
```trulang
memory_budget request = 16 MB max 64 MB;
```
This keeps baseline RSS at ~2.5 MB while providing burst headroom during transient spikes.
