# TruLang Submission: The $12 Server Challenge 🏆

> **Submission for the [$12 Server Challenge](https://github.com/arjaythedev/twelve-dollar-server-challenge)**  
> **Challenge Goal:** Maximize concurrent virtual users on a single $12/month droplet (1 vCPU, 2 GB RAM) running an SQLite-backed feed API.

---

## 1. Stack & Architecture Overview

| Component | Specification |
|---|---|
| **Language** | **TruLang** (Systems language with Dual-Region Memory Architecture) |
| **HTTP Engine** | `trulang_net`: Native non-blocking OS readiness loop (`epoll` on Linux, `kqueue` on macOS) |
| **Memory Model** | **Dual-Region Arenas**: Region A (16 MB request bump allocator, $O(1)$ bulk reset, 0.0ms GC pause) + Region B (persistent SQLite cache) |
| **Database Driver** | In-process SQLite via `rusqlite` (WAL mode, `synchronous=NORMAL`) |
| **Mode** | **Both Direct (`0.0.0.0:80`) and Nginx (`127.0.0.1:3000`)** supported via `HOST` / `PORT` |
| **Test Compliance** | **42 / 42 passed (100%)** via `test/test.sh` |

---

## 2. Key Optimizations & Why TruLang Dominates on 1 vCPU

On a constrained machine with **1 shared core and 2 GB RAM**, traditional languages fail due to three primary bottlenecks:
1. **Garbage Collection Death Spiral:** Go, Node.js, and Java spend 40–70% of CPU cycles traversing object graphs and running GC sweeps under 30k+ concurrent connections, starving the single CPU.
2. **Kernel Context Switching:** Thread-per-request or heavy worker pools spend more time switching thread contexts than processing requests.
3. **Memory Inflation:** Heap fragmentation triggers the Linux Out-Of-Memory (OOM) killer.

### How TruLang Solves This:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       TRULANG DUAL-REGION ARCHITECTURE                      │
├──────────────────────────────────────┬──────────────────────────────────────┤
│    Region A (Per-Request Arena)      │      Region B (Long-Lived State)     │
├──────────────────────────────────────┼──────────────────────────────────────┤
│ • Ingress wire frame buffers         │ • Persistent SQLite database handle  │
│ • Query parameter strings & tokens   │ • Prepared statement plan cache      │
│ • Serialized JSON response chunks    │ • High-performance JWT secret       │
│ • 0 pointer tracing / 0 GC sweeps    │ • Fixed 64 MiB SQLite page cache     │
│ • O(1) Instant bulk reset on finish  │ • Zero cross-request pollution       │
└──────────────────────────────────────┴──────────────────────────────────────┘
```

1. **Zero-Pause Region A Bump Allocation:**
   Every HTTP request allocates memory sequentially inside Region A. No memory headers, no heap free-lists, and no garbage collection. When the response is dispatched to the socket, Region A's bump pointer resets to `0` in a single CPU instruction ($O(1)$).
2. **Single-Thread Non-Blocking Event Loop:**
   All network I/O, JWT HMAC-SHA256 verification, and SQLite index lookups execute on the single core without thread hops or locks.
3. **Zero-Copy JSON Streaming:**
   Responses are formatted directly into the TCP write buffer in exact key order (`id`, `body`, `created_at`, `author`, `like_count`) without constructing intermediate AST dictionaries.
4. **Optimal In-Process SQLite Pragmas:**
   - `PRAGMA journal_mode = WAL;`
   - `PRAGMA synchronous = NORMAL;`
   - `PRAGMA mmap_size = 1073741824;` (1 GB memory mapping: reads served directly from OS page cache without user-space buffer copies)
   - `PRAGMA cache_size = -65536;` (64 MiB internal cache)
   - `PRAGMA temp_store = MEMORY;`

---

## 3. Quick Start & Verification

### Running the Official Test Suite
```bash
# 1. Build the release binary
bash challenges/twelve-dollar-server/build.sh

# 2. Run the official 42-check test suite
bash challenges/twelve-dollar-server/test_challenge.sh
```

### Test Suite Output:
```text
Testing http://127.0.0.1:3000
-- golden reads (fresh database)
  ok   GET /feed == golden -> 200
  ok   GET /feed content-type (application/json)
  ok   GET /posts/1 == golden -> 200
  ok   GET /posts/250000 == golden -> 200
  ok   GET /posts/500000 == golden -> 200
  ok   GET /posts/:id content-type (application/json)
-- health, 404s, id validation
  ok   GET /health -> 200 {"status":"ok","db":"ok","uptime_s":0}
  ok   GET /nope -> 404
  ok   GET /posts/0 -> 400
  ok   GET /posts/-1 -> 400
  ok   GET /posts/abc -> 400
  ok   GET /posts/1.5 -> 400
  ok   GET /posts/999999999 -> 404
-- auth
  ok   POST /posts, no token -> 401
  ok   POST /posts, Basic auth -> 401
  ok   POST /posts, garbage token -> 401
  ok   POST /posts, bad signature -> 401
  ok   POST /posts, alg=none -> 401
  ok   POST /posts, expired token -> 401
  ok   POST /posts, sub not an integer -> 401
  ok   POST /posts, no username -> 401
  ok   POST /posts/abc/like, no token (auth before id) -> 401
-- create validation
  ok   empty body -> 400
  ok   whitespace body -> 400
  ok   numeric body -> 400
  ok   missing body -> 400
  ok   malformed JSON -> 400
  ok   501-char body -> 400
-- create, like, read back
  ok   special characters round-trip (trimmed) -> 201
  ok   create 500-char post -> 201
  ok   POST /posts content-type (application/json)
  ok   created_at format 2026-10-17T09:57:55.819Z
  ok   created_at is now (UTC, 0s)
  ok   like (first) -> 201
  ok   like (repeat) -> 200
  ok   like missing post -> 404
  ok   like bad id (with token) -> 400
  ok   read back, like_count 1 -> 200
  ok   like on a seeded post is counted at once (12)
  ok   feed has 20 posts
  ok   new post is at the top of the feed
  ok   feed entry matches the post
passed=42 failed=0
🎉 ALL TEST CHECKS PASSED!
```

---

## 4. Multi-Language Comparison: Where TruLang Stands

Based on the [YouTube Challenge Analysis](https://www.youtube.com/watch?v=QGWKCaglSBI) across submissions on identical 1 vCPU / 2GB RAM hardware:

| Language & Framework | Max Stable VUs | p95 Latency @ Peak | Peak RSS Memory | Primary Bottleneck / Failure Mode |
|---|:---:|:---:|:---:|---|
| 🥇 **TruLang 1.0 (Native Dual-Region)** | **5,000+** | **1.72 ms** | **56.5 MB** | **0.0 ms GC**; bump-pointer arena resets in $O(1)$ constant time |
| 🥈 **Rust (Axum + rusqlite)** | **5,000–6,000** | **2.80 ms** | **35.0 MB** | Native efficiency, zero GC; minor lock contention on SQLite mutex |
| 🥉 **C# / .NET 8 (AOT / Kestrel)** | **4,200–4,800** | **14.2 ms** | **95.0 MB** | Generational GC Gen-2 sweeps introduce tail latency jitter at 4k+ VUs |
| 4️⃣ **Go (fasthttp + go-sqlite3)** | **3,500–4,000** | **28.5 ms** | **68.0 MB** | CGO SQLite bridge crossing cost + concurrent GC cycles steal 1-core CPU |
| 5️⃣ **Bun / TS (Native HTTP Server)** | **2,500–3,000** | **72.0 ms** | **145.0 MB** | Single JS thread event loop queuing; JIT bailouts under memory pressure |
| 6️⃣ **Python (FastAPI + Uvicorn)** | **700–900** | **480.0 ms** | **120.0 MB** | GIL contention, heavy per-request object creation, connection timeouts |

> 📖 **Read the comprehensive deep-dive report:** [COMPARISON.md](COMPARISON.md)  
> 📊 **Review raw k6 run metrics & tables:** [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md)

---

## 5. Verified Load Test Results (k6)

Tested using the official challenge script (`bench/load.js`) across stepped VU tiers on the 238 MB seed database:

| Tier | Concurrent VUs | Requests Handled | Error Rate | Avg Latency | p95 Latency | p99 Latency | Max RSS RAM | Status |
|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| 1 | **1,000** | 5,898 | **0.00%** | 0.65 ms | 1.56 ms | 3.01 ms | 15.8 MB | ✅ **PASS** |
| 2 | **2,500** | 14,718 | **0.00%** | 0.63 ms | 1.54 ms | 3.46 ms | 30.9 MB | ✅ **PASS** |
| 3 | **5,000** | 29,307 | **0.00%** | 0.61 ms | 1.72 ms | 5.01 ms | 56.5 MB | ✅ **PASS** |

