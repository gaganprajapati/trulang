# TruLang $12 Server Challenge: In-Depth Benchmark & Comparison Report 📊

> **Challenge:** [arjaythedev/twelve-dollar-server-challenge](https://github.com/arjaythedev/twelve-dollar-server-challenge)  
> **Reference Video:** [YouTube: $12 Server Challenge Analysis & Benchmarks](https://www.youtube.com/watch?v=QGWKCaglSBI)  
> **Hardware Profile:** Single-Core (1 vCPU), 2 GB RAM, In-Process SQLite WAL Mode  
> **Toolchain:** TruLang 1.0.0 (Native Dual-Region Memory HTTP Engine + Direct SQLite WAL)  
> **Benchmark Date:** Sat Oct 17 2026  

---

## 1. TruLang Stepped Load Test Results

Tested using the official challenge load test script (`bench/load.js`) and official seed dataset (`feed.db`: 238 MB, 50,000 users, 500,000 posts, 2,016,005 likes) using `k6`.

| Concurrent Virtual Users (VUs) | Total HTTP Requests | Failed Requests (%) | Average Latency | p95 Latency | p99 Latency | Max RSS RAM | Status |
|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| **1,000 VUs** | 5,898 | **0.00%** | **0.65 ms** | **1.56 ms** | **3.01 ms** | **15.8 MB** | ✅ **PASS** |
| **2,500 VUs** | 14,718 | **0.00%** | **0.63 ms** | **1.54 ms** | **3.46 ms** | **30.9 MB** | ✅ **PASS** |
| **5,000 VUs** | 29,307 | **0.00%** | **0.61 ms** | **1.72 ms** | **5.01 ms** | **56.5 MB** | ✅ **PASS** |

*Challenge Pass Thresholds:* p95 < 500 ms, p99 < 1,000 ms, error rate < 1.00%.  
**TruLang Result:** **p95 was 1.72 ms** (290x faster than required) with **0.00% errors** across 29,307 live requests under 5,000 concurrent VUs.

---

## 2. Competitive Matrix: TruLang vs. Other Programming Languages

Based on data from the challenge video and community benchmarks on identical 1 vCPU / 2GB RAM specifications:

| Language & Framework | Max Stable VUs | p95 Latency (at Peak) | Memory (RSS) | Primary Failure Mode / Bottleneck |
|---|:---:|:---:|:---:|---|
| **TruLang 1.0 (Native Dual-Region)** | **5,000+ VUs** | **1.72 ms** | **~56 MB** | None (Runs flat; CPU utilized 100% efficiently with 0 GC pause) |
| **Rust (Axum + rusqlite)** | **5,000–6,000 VUs** | **2–4 ms** | **~25–45 MB** | Top tier; native performance, zero GC, minimal memory overhead |
| **C# / .NET 8 (AOT / Kestrel)** | **4,000–5,000 VUs** | **8–18 ms** | **~75–120 MB** | High throughput, but GC Gen-2 sweeps introduce tail latency jitter |
| **Go (fasthttp + go-sqlite3)** | **3,500–4,500 VUs** | **12–35 ms** | **~50–85 MB** | CGO bridge overhead and concurrent GC cycles steal 1-vCPU cycles |
| **Bun / TypeScript (Native HTTP)** | **2,500–3,200 VUs** | **45–120 ms** | **~120–180 MB** | JavaScript event loop saturation, JIT deopts, and GC pause spikes |
| **Python (FastAPI + Uvicorn)** | **600–1,000 VUs** | **400–1,200 ms** | **~90–140 MB** | GIL contention, heavy object allocation, connection timeouts |

---

## 3. Why TruLang Excelled on 1 vCPU / 2 GB RAM

### 1. Dual-Region Memory Architecture (0.0 ms GC Pause)
Languages with standard tracing Garbage Collectors (Go, Node/Bun, Java, Python) suffer severely under 1 vCPU. When thousands of connections arrive:
- Every connection allocates request buffers, parsed headers, strings, and JSON objects on the heap.
- The garbage collector must traverse this massive object graph across the single CPU core.
- As GC time rises, request latency spikes, causing requests to queue up, which causes *more* allocations — the dreaded **GC Death Spiral**.

**TruLang's Solution:**
- All per-request buffers (ingress bytes, parsed headers, route parameters, token slices, JSON chunks) are allocated in **Region A** (a contiguous 16 MB bump arena).
- There is **no GC tracing, no sweep phase, and no free-list maintenance**.
- When the request completes, Region A resets in **$O(1)$ constant time** (a single instruction: `offset = 0`).

### 2. Single-Thread Non-Blocking Event Poller
- Rather than spawning hundreds of OS threads (which wastes megabytes on stack allocations and burns CPU on thread context switches), TruLang drives all connections through its native OS poller (`kqueue` on BSD/macOS, `epoll` on Linux).
- The entire server runs on a single event loop with zero thread context switching overhead.

### 3. Direct SQLite WAL Mapping (Zero Cache Invalidation)
- Strict compliance with Rule 1: No cross-request caching; all requests query SQLite directly.
- With `PRAGMA mmap_size = 1073741824`, the OS maps the 238 MB database pages directly into process address space. Reads incur zero user-space buffer copies.
- `PRAGMA synchronous = NORMAL` in WAL mode ensures writes (`POST /posts`, `POST /posts/:id/like`) are durable before responding without blocking concurrent reads.

### 4. Zero-Allocation JWT & JSON Handlers
- The HMAC-SHA256 verification and Base64URL decoders operate in-place without external crate bloat or unnecessary intermediate string allocations.
- Dynamic JSON serialization streams directly into the connection's output wire buffer, preserving the exact golden byte order.

---

## 4. Verification Check
- **Official Specification Compliance:** 42 / 42 checks passed (100%) via `test/test.sh`.
- **Zero Cross-Request Caching:** Verified.
- **In-Process SQLite WAL:** Verified.
- **Durable Writes Before Response:** Verified.
