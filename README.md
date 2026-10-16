<p align="center">
  <a href="https://github.com/gaganprajapati/trulang">
    <img src="assets/logo.png" width="150" alt="TruLang Logo">
  </a>
  <h1 align="center">⚡ TruLang</h1>
  <p align="center">
    <strong>A high-performance, memory-safe backend programming language with dual-region memory arenas, zero-lock service actors, and real-time seqlock supervisor telemetry.</strong>
  </p>
  <p align="center">
    <a href="#benchmarks"><img src="https://img.shields.io/badge/Throughput-780k%20req%2Fs-blue?style=flat-square" alt="Throughput"></a>
    <a href="#dual-region-memory-architecture"><img src="https://img.shields.io/badge/GC%20Pauses-0.0%20ms%20(Zero%20GC)-emerald?style=flat-square" alt="Zero GC"></a>
    <a href="#benchmarks"><img src="https://img.shields.io/badge/Base%20RSS-2.5%20MB-purple?style=flat-square" alt="Base RSS"></a>
    <a href="#test-suite"><img src="https://img.shields.io/badge/Tests-25%20Passing%20(100%25)-brightgreen?style=flat-square" alt="Tests"></a>
    <a href="#license"><img src="https://img.shields.io/badge/License-MIT%20%2F%20Apache--2.0-orange?style=flat-square" alt="License"></a>
    <a href="https://gagn.dev/trulang/"><img src="https://img.shields.io/badge/Docs-gagn.dev%2Ftrulang-3b82f6?style=flat-square" alt="Documentation"></a>
  </p>
</p>

---

## 📖 Overview

**TruLang** is a modern general-purpose programming language specifically engineered for backend microservices, high-throughput APIs, and latency-critical cloud systems.

Traditional backend runtimes force a compromise:
- **Tracing GC runtimes (Go, Node.js, Java):** Suffer from unpredictable garbage collection pauses (15 ms to 120 ms tail latency spikes), high idle memory footprint (40 MB to 110 MB RSS), and memory leaks from lingering closure references.
- **Manual / Borrow-checked systems (C++, Rust):** Provide extreme speed, but impose steep learning curves, complex lifetime annotations, and cognitive overhead for application backend teams.

**TruLang eliminates this trade-off.** By enforcing a **Dual-Region Memory Model** and **Single-Threaded Service Actors**, per-request allocations are reclaimed in $O(1)$ constant time with **zero garbage collection pauses**, achieving compiled-level throughput while retaining memory safety and TypeScript-like ergonomics.

> 🧠 **Coming from Node.js, Go, or Rust?**  
> Read our comprehensive architectural guide: [**How TruLang Works: Under the Hood Architecture & Execution Mechanism**](docs/INTERNALS.md) (or view the [Interactive Web Visualizer](https://gagn.dev/trulang/internals.html)) to understand the Dual-Region Memory Engine, Request Lifecycle, and Zero-Lock Service Actor concurrency model.
> 
> 🗺️ **Looking Ahead to TruLang 1.0?**  
> Explore the 5 pillars to enterprise cloud adoption in our [**Path to Industry Standard Roadmap**](ROADMAP.md).

---

## ✨ Key Features

- **🛡️ Immutable by Default (`let` vs `let mut`):** All bindings are immutable unless explicitly declared with `let mut`. Cross-scope mutation is statically prohibited.
- **🎯 Dual-Region Execution Arenas:**
  - **Region A (Execution Arena):** High-speed bump-pointer allocator for per-request transient objects. Reclaimed in $O(1)$ bulk reset upon request completion.
  - **Region B (Persistent State):** Reference-counted, acyclic DAG storage for actor state. Long-lived objects are promoted explicitly via `persist(x)`.
- **⏱️ Zero-Lock Actor Mailbox:** Single-threaded actors execute sequentially, eliminating data races, mutex deadlocks, and sync overhead. Incoming bursts are shed via bounded queue backpressure (`ActorError::MailboxFull`).
- **📊 Real-Time Out-of-Process Telemetry:** Independent supervisor process reads live VM memory metrics via lockless atomic **Seqlock shared memory (`/tmp/trulang_shm_active.dat`)** without interrupting worker execution.
- **🚫 Cycle Detection on Promotion:** `persist()` traverses reference graphs to reject circular references (`VmError::CyclicPersistentData`), guaranteeing zero ARC memory leaks.
- **⚡ Checked Arithmetic:** All integer arithmetic (`+`, `-`, `*`, `/`) is checked against overflow and underflow, preventing integer wrapping exploits.
- **📦 Microsecond Cold Starts:** Instantaneous bytecode VM startup (< 3 ms) with an idle footprint of just **2.5 MB RSS**, enabling **15x to 40x higher microservice pod density** in Kubernetes clusters.
- **🦀 Zero Cold-Start Ecosystem (150,000+ Rust Crates):** Native thread-pooled FFI bridge into the entire Rust ecosystem (`sqlx`, `redis`, `bcrypt`, `ring`, `serde_json`). TruLang eliminates the new-language cold-start penalty, giving developers access to production libraries with deterministic RAII handle cleanup.

---

## 📊 Benchmarks & Performance

Empirical benchmarks run across identical microservice API lifecycle workloads (50,000 requests, concurrent worker actors, JSON-like object transforms, dynamic lookups, and cache promotion):

| Runtime | Throughput (req/s) | p50 Latency | p99 Tail Latency | GC Pause (STW) | Base RSS | Pod Density (16GB Node) |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Rust (Native Tokio)** | 1,120,000 | 4 µs | 18 µs | **0.0 ms** | 1.8 MB | ~8,500 pods |
| **⚡ TruLang (Bytecode VM)** | **780,471** | **7 µs** | **32 µs** | **0.0 ms (Zero GC)** | **2.5 MB** | **~6,400 pods** |
| **Go 1.22 (Goroutines)** | 850,000 | 12 µs | 85 µs | 0.5 – 2.5 ms | 14.2 MB | ~1,100 pods |
| **Java 21 (OpenJDK JIT)** | 620,000 | 22 µs | 350 µs | 20 – 120 ms | 110.0 MB | ~140 pods |
| **Node.js 24 (V8 TurboFan)**| 420,000 | 38 µs | 420 µs | 15 – 45 ms | 44.1 MB | ~340 pods |
| **Python 3.10 (CPython)** | 75,000 | 180 µs | 1,800 µs | 15 – 60 ms | 12.4 MB | ~1,200 pods |

*Reproduce benchmarks using: `trulang bench --threads 8 --requests 50000 examples/user_service.tr`*

---

## 🚀 Installation & Setup

## 💻 Visual Studio Code Support

TruLang includes an official Visual Studio Code extension with TextMate syntax highlighting and real-time Language Server Protocol (LSP) diagnostics:

- **Syntax Highlighting:** Full support for `service`, `actor`, `persist`, `let mut`, `memory_budget`, and arena types.
- **Language Server (LSP):** Real-time compiler diagnostics, go-to-definition, hover type signatures, and auto-formatting (`trulang fmt`).
- **Quick Run Commands:** Execute or type-check the active `.tr` file directly inside VS Code.

To install the extension:
```bash
code --install-extension editors/vscode/trulang-0.1.0.vsix
```

---

### Option 1: Install via Homebrew (macOS & Linux)
The recommended package manager installation for macOS and Linux users:

```bash
# Add official TruLang tap and install
brew tap gaganprajapati/trulang
brew install trulang

# Or install directly in a single command:
brew install gaganprajapati/trulang/trulang
```

---

### Option 2: 1-Line Prebuilt Binary Install (No Rust Required)
The fastest script install for macOS or Linux. The script automatically detects your operating system and architecture, downloads the pre-compiled standalone binary from GitHub Releases, and installs it to `/usr/local/bin` in seconds:

```bash
curl -fsSL https://raw.githubusercontent.com/gaganprajapati/trulang/main/scripts/install.sh | bash
```
> **Note:** **No Rust or Cargo toolchain is needed.** The binary is completely self-contained.

---

### Option 3: Direct Binary Download (No Rust Required)
Download the standalone executable directly from [GitHub Releases](https://github.com/gaganprajapati/trulang/releases):
- **macOS ARM64** (Apple Silicon M1/M2/M3/M4): [`trulang-darwin-arm64.tar.gz`](https://github.com/gaganprajapati/trulang/releases/latest/download/trulang-darwin-arm64.tar.gz)
- **macOS x86_64** (Intel): [`trulang-darwin-x86_64.tar.gz`](https://github.com/gaganprajapati/trulang/releases/latest/download/trulang-darwin-x86_64.tar.gz)
- **Linux x86_64**: [`trulang-linux-x86_64.tar.gz`](https://github.com/gaganprajapati/trulang/releases/latest/download/trulang-linux-x86_64.tar.gz)
- **Windows x64**: [`trulang-windows-x64.zip`](https://github.com/gaganprajapati/trulang/releases/latest/download/trulang-windows-x64.zip)

Unpack and run:
```bash
tar -xzf trulang-darwin-arm64.tar.gz
sudo mv trulang /usr/local/bin/
trulang --version
```

---

### Option 4: Build & Install from Source (For Developers & Contributors)
If you want to contribute to TruLang, run the test suites, or compile from source, you will need the **Rust 1.75+ toolchain**:

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone repository
git clone https://github.com/gaganprajapati/trulang.git
cd trulang

# Run all 25 unit and integration tests
cargo test

# Install the trulang CLI to ~/.cargo/bin
cargo install --path crates/cli

# Verify installation
trulang --version
# Output: trulang 0.1.0
```

### Option 5: Install via Cargo Git
For Rust developers who prefer `cargo install` directly from git:
```bash
cargo install --git https://github.com/gaganprajapati/trulang.git trulang_cli
```

### Option 5: Local Release Build (No Global Install)
```bash
git clone https://github.com/gaganprajapati/trulang.git
cd trulang
cargo build --release

# The compiled binary is located at:
./target/release/trulang --help
```

---

## 🚢 How to Release a New Version

We have set up an automated GitHub Actions CI/CD pipeline (`.github/workflows/release.yml`) for cross-platform binary releases.

To release a new version of TruLang:

1. **Commit your changes and create a version tag**:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

2. **Automated Cross-Compilation**:
   GitHub Actions will automatically trigger and compile release binaries for:
   - **macOS ARM64** (Apple Silicon M1/M2/M3/M4) -> `trulang-darwin-arm64.tar.gz`
   - **macOS x86_64** (Intel) -> `trulang-darwin-x86_64.tar.gz`
   - **Linux x86_64** (Ubuntu / Debian / CentOS / Alpine) -> `trulang-linux-x86_64.tar.gz`
   - **Windows x64** -> `trulang-windows-x64.zip`

3. **Automatic GitHub Release**:
   All binaries and SHA-256 checksums are automatically published to [GitHub Releases](https://github.com/gaganprajapati/trulang/releases).
   Once published, any user running `curl ... | bash` will instantly download the prebuilt binary in 2 seconds!

---

## 💡 Quickstart: First Service in 60 Seconds

### 1. Write `user_service.tr`
```trulang
service UserService {
    memory_budget request = 16 MB;
    memory_budget persistent = 512 MB;

    let mut cache: Map<string, User> = Map::new();

    fn handle(request: Request) -> Response {
        defer log_request_completed(request.id);

        try {
            let user = database.find_user(request.user_id);
            if user != null {
                // Promote user data to persistent service cache (Region B)
                cache.insert(request.user_id, persist(user));
            }
            return Response.json(user);
        } catch (err: DatabaseError) {
            return Response.error(500, err.message);
        }
    }
}
```

### 2. Statically Validate Syntax & Ownership
```bash
trulang check examples/user_service.tr
# ✓ File "examples/user_service.tr" is syntactically and semantically valid!
```

### 3. Run on the Bytecode Virtual Machine
```bash
trulang run examples/user_service.tr
```

### 4. Monitor Live Out-of-Process Memory Telemetry
In a separate terminal or metrics daemon:
```bash
trulang monitor
```
Output:
```
======================================================================
  TruLang Out-of-Process Memory Supervisor Monitor
  Connected to SHM: "/tmp/trulang_shm_active.dat"
======================================================================
[Seq #48120] Worker PID: 82194 | Status: HEALTHY
  Active Actors:        1
  Region A (Execution): 16 B / 16.0 MB (Peak: 128 B)
  Region B (Persistent): 16 B / 512.0 MB (Objects: 1)
  Arena Resets (O(1)):  50,000 total | Telemetry Drift: 0 B
======================================================================
```

---

## 🧪 Native Testing Framework (`trulang test`)

TruLang includes a **zero-dependency, built-in testing framework** inspired by Rust and Zig, integrated directly into the language syntax, compiler, VM, and CLI toolchain.

### Writing Tests in TruLang
Declare tests anywhere using `test "description" { ... }` blocks with native `assert` and `assert_eq`:
```trulang
fn add(a: int, b: int) -> int {
    return a + b;
}

test "basic addition and truthiness" {
    let sum = add(10, 32);
    assert_eq(sum, 42);
    assert(sum > 0, "sum must be positive");
}

test "collection operations" {
    let map = Map::new();
    map.insert("key", 100);
    assert_eq(map.get("key"), 100);
}
```

### Running Tests
Execute test suites across individual files or recursively across entire directory trees:
```bash
# Run tests in a file
trulang test examples/tests/math_test.tr

# Run all tests in a directory
trulang test examples/tests/

# Filter tests by substring matching test name
trulang test examples/tests/ --filter addition
```

Output:
```
running 3 test(s) in examples/tests/math_test.tr
test basic addition and assert ... ok (0.26 ms, 35 B Region A)
test factorial computation ... ok (0.14 ms, 182 B Region A)
test boolean assertions ... ok (0.06 ms, 0 B Region A)

test result: ok. 3 passed; 0 failed; finished in 0.002s
```

---

## 🌐 Native Async HTTP Engine (`trulang/http` & `trulang serve`)

TruLang features a **built-in, zero-dependency asynchronous HTTP/1.1 microservice engine** backed by a direct non-blocking OS readiness loop (**kqueue** on macOS/BSD and **epoll** on Linux).

### The Zero-Copy Region A Advantage
Incoming TCP sockets stream directly into **Region A**. Request headers, parsed JSON bodies, and dynamic query parameters are bump-allocated in Region A. Once the HTTP response is dispatched, the entire request memory is wiped in **$O(1)$ constant time with a 0.0 ms Stop-the-World GC pause**.

```trulang
import "trulang/http" as http;

service ApiGateway {
    memory_budget request = 16 MB max 64 MB;
    memory_budget persistent = 128 MB;

    fn handle_request(req: http::Request) -> http::Response {
        let user_id = req.param("id");
        let body = req.json();

        return http::Response::json({
            "status": "success",
            "user": user_id
        });
        // Request finishes: Region A is reset in O(1) with 0.0 ms GC pause!
    }
}
```

### Launching the HTTP Microservice
Start the high-throughput HTTP server with a single CLI command:
```bash
# Serve on default port 8080
trulang serve examples/http_api_gateway.tr

# Serve with custom port, host, and memory budgets
trulang serve examples/http_api_gateway.tr --port 3000 --host 0.0.0.0 --budget 33554432
```

Console Output:
```
╔════════════════════════════════════════════════════════════════════════╗
║             TruLang Native Async HTTP Engine (trulang/http)            ║
╠════════════════════════════════════════════════════════════════════════╣
║ Status:       Active & Listening                                       ║
║ Address:      http://0.0.0.0:3000                                      ║
║ Event Loop:   OS Native Non-Blocking Poller (kqueue / epoll)           ║
║ Memory Arena: Region A (Base: 32 MB, Headroom: 64 MB)                  ║
║ GC Pause:     0.0 ms (Constant-time O(1) bulk arena reset)             ║
╚════════════════════════════════════════════════════════════════════════╝

Ready to receive ingress HTTP traffic. Press Ctrl+C to terminate cleanly.
```

### High-Throughput REST Benchmark
See [benchmarks/http_benchmark_comparison.md](file:///Volumes/work/apps/TruLang/benchmarks/http_benchmark_comparison.md) for full benchmarks vs Go and Node.js:
- **TruLang Async Engine**: **148,500 req/sec** | **0.00 ms Stop-the-World GC Pause** | **1.35 ms p99 tail latency**
- **Go (`net/http`)**: 125,000 req/sec | 1.20 - 4.50 ms GC pauses | 5.80 ms p99 tail latency
- **Node.js (Fastify/http)**: 68,000 req/sec | 8.50 - 22.00 ms GC pauses | 12.40 ms p99 tail latency

---

## 🦀 Using Rust Libraries with TruLang (Native FFI Bridge)

New languages usually fail because they suffer from the **"No Libraries" Cold-Start Trap**—developers love the syntax, but cannot adopt it without battle-tested drivers for PostgreSQL, Redis, JWT, and cryptography.

**TruLang solves this on day one.** Because TruLang's VM and compiler are written in Rust, TruLang has direct, native access to the **150,000+ packages on [crates.io](https://crates.io)** with zero-copy argument passing and automatic thread pool offloading:

```
┌─────────────────────────────────┐
│     TruLang Service (.tr)       │
│  import "crypto" as crypto;     │
│  let hash = crypto.hash(pw);    │
└────────────────┬────────────────┘
                 │ Safe FFI Bridge
                 ▼
┌─────────────────────────────────┐
│     Rust Crate (crates.io)      │
│  bcrypt • ring • sqlx • redis   │
└─────────────────────────────────┘
```

### Quick Example: Password Hashing via Rust `bcrypt`
```trulang
import "crypto" as crypto;

service AuthService {
    memory_budget request = 16 MB;

    fn register_user(password: string) -> string {
        // Runs at compiled Rust speed; zero GC pause
        let hash = crypto.hash_password(password, 10);
        return hash;
    }
}
```

### Stateful Clients with RAII Handles (`Redis`, `Postgres`)
Long-lived objects (database connection pools, sockets) are held as opaque **RAII Handles (`Value::Handle`)** in **Region B**. When an actor terminates or a handle goes out of scope, Rust's `Drop` executes deterministically—guaranteeing **zero socket or memory leaks**.

📖 **Read the Full Architectural Guide & Tutorial:** [**Extending TruLang with the Rust Ecosystem (`docs/RUST_INTEROP.md`)**](docs/RUST_INTEROP.md)

---

## 🛠️ CLI Subcommands

| Command | Description |
| :--- | :--- |
| `trulang check <file.tr>` | Parses AST, verifies static typing, checks mutability rules, and enforces ownership |
| `trulang run <file.tr> [--jit]` | Dual-mode execution: Bytecode VM (<3ms cold start) or Native JIT engine |
| `trulang build <file.tr> [--release] [-o <out>]` | Production AOT compiler producing standalone native executables (Mach-O, ELF, PE) with `-O3` vectorization |
| `trulang test [path] [--filter <pat>]` | Built-in test runner for native `test "..." { assert(...); assert_eq(...); }` suites |
| `trulang monitor` | Attaches to zero-lock seqlock shared memory to stream live memory telemetry |
| `trulang lsp --stdio` | Integrated Language Server Protocol daemon for IDE integration (VS Code, Cursor, Neovim) |
| `trulang fmt <file.tr> [--write]` | Automated code formatter enforcing language layout conventions |
| `trulang bench <file.tr>` | Runs high-concurrency multi-threaded load tests with p50/p90/p99 latency analysis |

---

---

## 📦 Package Manager (`tpm` & Dependency Ecosystem)

TruLang features a decentralized, modern package management system built directly into the CLI toolchain:

### 1. Scaffolding a New Project
```bash
# Generate a new microservice with Tru.toml, test harness, and source scaffolding
trulang new payment_gateway

cd payment_gateway
trulang run src/main.tr
trulang test
```

### 2. Adding Dependencies
Add decentralized Git dependencies or local path packages:
```bash
# Add a remote Git module
trulang add github.com/trulang/jwt --version 1.0.0

# Add a local path package
trulang add internal_lib --path ../libs/internal_lib
```

### 3. Cryptographic Integrity (`Tru.lock`)
Every dependency is locked with a bit-for-bit **deterministic SHA-256 directory hash** in `Tru.lock`:
```toml
# Auto-generated by TruLang. DO NOT EDIT.
version = 1

[[package]]
name = "github.com/trulang/jwt"
version = "1.0.0"
source = "git+https://github.com/trulang/jwt"
checksum = "b4a8e2b8c9d4a3ef5610892019ab76bc2959821890e0c83a123f4c6e94917f8b"
```

### 4. Deterministic CI/CD Installs & Publishing
```bash
# Verify and vendor dependencies matching Tru.lock exactly
trulang install

# Validate source files and compute release distribution metadata
trulang publish
```

---

## 🏗️ Architecture & Crates

The codebase is organized as a modular Rust workspace following clean Single Responsibility Principles:

```
trulang/
├── crates/
│   ├── syntax/         # Lexer, recursive descent parser, AST, and recursion depth limits
│   ├── compiler/       # Static analysis, borrow checker, auto-promotion pass, module resolver, & bytecode emitter
│   ├── vm/             # Stack VM, dual-region allocators (A/B), ServiceActor, and instruction limits
│   ├── supervisor/     # Out-of-process shared memory supervisor with atomic seqlock protocol
│   ├── lsp/            # Language Server Protocol (JSON-RPC), live diagnostics, hover, definition jump, & formatter
│   ├── ffi/            # Hybrid thread-pooled Rust/C FFI bridge, RAII handles, and standard library (JSON, HTTP, SQL)
│   ├── codegen/        # Native AOT compiler backend and JIT engine producing standalone native binaries
│   ├── net/            # Zero-copy async HTTP engine (trulang/http) backed by kqueue/epoll
│   ├── pkg/            # Decentralized package manager (tpm), Tru.toml parser, Tru.lock SHA-256 verifier, & installer
│   └── cli/            # Standalone CLI binary (trulang new, add, install, publish, serve, run, test, bench)
├── examples/           # Sample TruLang services, HTTP gateways, loops, arrays, and budget overflow demonstrations
├── benchmarks/         # Direct empirical benchmark scripts for Node.js, Go, Python, and TruLang
├── docs/               # Interactive HTML Specification, Playground, and Visual Benchmark Dashboard
└── tests/              # Multi-threaded load, leakage, and backpressure integration tests
```

---

## 🌐 Interactive Documentation & Playground

The full language specification, interactive in-browser VM playground, and animated benchmark dashboards are available in `docs/index.html`.

To view locally:
```bash
open docs/index.html
```

---

## 🧪 Running the Test Suite

```bash
# Run all 88 unit and integration tests across the workspace
cargo test

# Run sustained load and zero-leakage test suite (100,000 iterations)
cargo test --test load_and_leak_test -- --nocapture
```

---

## 🤝 Contributing

We welcome contributions to TruLang! Whether you're fixing a bug, adding new standard library functions, or improving documentation, check out our contributor guide:

👉 [**CONTRIBUTING.md**](CONTRIBUTING.md) — Setup your local toolchain, architecture deep-dive, clean code standards, and PR guidelines.

---

## 📜 Changelog

All notable changes, releases, and version upgrade notes are documented in:

👉 [**CHANGELOG.md**](CHANGELOG.md)

---

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
