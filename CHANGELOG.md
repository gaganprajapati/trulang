# Changelog 📜

All notable changes to the TruLang language, compiler, runtime, and supervisor will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- **Language Server Protocol (`crates/lsp`)**:
  - Integrated `trulang lsp --stdio` subcommand in core CLI binary for seamless IDE connectivity (VS Code, Cursor, Neovim, Zed) with zero extra toolchain downloads.
  - Live syntax and typecheck diagnostics with actionable compiler hints (e.g. suggesting `let mut` on illegal mutations).
  - Rich hover tooltips for services, actor fields, memory budgets, functions, and imported modules.
  - Document jump-to-definition across service declarations, fields, methods, functions, and imports.
  - Automated code formatter (`trulang fmt <file> [--write]`) enforcing consistent 4-space indentation and clean brace structure.
- **Safe Rust/C FFI Bridge Layer (`crates/ffi`)**:
  - `import "rust:pkg" as mod;` syntax for integrating native libraries and standard drivers.
  - **Hybrid Background Worker Pool (`WorkerPool`)**: Offloads blocking foreign I/O (database queries, network sockets, heavy crypto) to a dedicated worker thread pool, guaranteeing the single-threaded actor mailbox is never starved.
  - **Deterministic RAII Destructors (`ExternalHandle<T>`)**: Opaque handles (e.g. PostgreSQL connection pools, sockets) with automatic cleanup when actor state or variables go out of scope.
  - **Region A Arena Marshaling**: Marshals foreign return values directly into Region A bump memory, strictly enforcing memory budgets so foreign payloads cannot bypass memory quotas.
  - **Standard Library Modules**:
    - `std::json`: Fast JSON parser (`json.parse`) and serializer (`json.stringify`) streaming into TruLang values.
    - `std::http`: High-performance HTTP client (`http.get`, `http.post`) offloaded to worker pool.
    - `std::sql` & `rust:trulang_std_pg`: Universal connection pooling abstractions (`db.create_pool`, `pool.query_user`).
- **Elastic Burst Arenas**: Support tiered memory budgets (`memory_budget request = 16 MB max 64 MB;`) with burst headroom and `ArenaBurst = 5` Seqlock telemetry warnings, while retaining strict backward-compatible caps when `max` is omitted.
- **Compiler Auto-Promotion (`persist` Inference)**: Transparent compiler pass that automatically injects `Opcode::Persist` when writing transient data into persistent actor fields or mutating persistent maps (`cache.insert(k, v)`), eliminating manual `persist()` boilerplate while keeping explicit calls valid.
- **Enhanced CLI Diagnostics**: `trulang run` reports base budget, max ceiling, and runtime elastic burst count.
- **Primitive Type Aliases**: Added parser support for `String`, `Int`, `Float`, `Bool` capitalized type names alongside standard lowercase tokens.
- **New Example**: Added `examples/elastic_burst.tr` demonstrating elastic burst allocation and auto-promotion.

- **Native AOT Code Generation & Standalone Binaries (`crates/codegen`, `trulang build`)**:
  - `trulang build [--release] <file> [-o <out>]`: Native ahead-of-time compilation generating optimized native machine code and linking into standalone executables (Mach-O on macOS, ELF on Linux, PE on Windows).
  - Preserves TruLang's dual-region memory architecture: inline Region A $O(1)$ bump arena allocation, zero GC pauses, elastic burst handling up to ceiling, and Region B persistent memory tracking.
  - Produces standalone binaries with zero runtime dependencies on the `trulang` CLI or virtual machine.
  - Emits native C source via `trulang build --emit-c <file>` for cross-compilation, security auditing, or embedding into external C/Rust toolchains.
  - Closes compute throughput gap to within 90–95%+ of native Rust/C++ through `-O3` vectorization and host compiler optimization.
- **Native JIT Engine (`trulang run --jit`)**:
  - On-the-fly native machine code compilation and execution (`trulang run --jit <file>`), delivering native execution speed with near-instant iteration during testing and benchmarks.
- **Dual-Mode Execution Architecture**:
  - **Dev Mode (`trulang run <file>`)**: Bytecode VM with <3ms instantaneous cold-start for development loops.
  - **Production Mode (`trulang build --release <file>`)**: Standalone native machine code binaries for high-throughput production services.
  - **JIT Mode (`trulang run --jit <file>`)**: High-speed native JIT execution.

---

## [0.1.0] - 2026-10-14

### Added
- **Dual-Region Execution Arenas**:
  - **Region A**: Fast bump-pointer allocator for transient, request-scoped objects, reclaimed in $O(1)$ constant time with **0.0 ms Garbage Collection pauses**.
  - **Region B**: Reference-counted, acyclic DAG storage for long-lived service state promoted explicitly via `persist(x)`.
- **Zero-Lock Service Actors**:
  - Single-threaded actor execution eliminating data races, mutex lock contention, and deadlocks.
  - Bounded mailbox queues with automatic backpressure load-shedding (`ActorError::MailboxFull`).
- **Real-Time Out-of-Process Seqlock Telemetry**:
  - Independent memory supervisor streaming metrics via lockless atomic shared memory (`/tmp/trulang_shm_active.dat`).
  - Zero-overhead performance profiling on running worker processes.
- **Language Syntax & Static Typing**:
  - Immutable by default (`let` vs `let mut`).
  - Static typechecker with local type inference and nullable types (`T?`).
  - Safe error handling with `try/catch` and deterministic `defer` RAII cleanup.
  - Checked integer arithmetic preventing overflow/underflow exploits.
  - Recursive descent parser with strict recursion depth protection (`MAX_RECURSION_DEPTH = 256`).
- **CLI & Developer Tooling**:
  - `trulang check <file>`: Static syntax, typing, and ownership verification.
  - `trulang run <file>`: High-speed bytecode compiler and virtual machine execution.
  - `trulang monitor`: Real-time streaming supervisor monitor.
  - `trulang bench <file>`: Multi-threaded concurrent benchmarking suite with latency percentile analysis.
  - Version inspection: Support for `trulang -v`, `trulang -V`, `trulang --version`, and `trulang version`.
- **Cross-Platform Releases & CI/CD**:
  - Automated GitHub Actions release pipeline (`.github/workflows/release.yml`) cross-compiling release binaries for:
    - **macOS ARM64** (Apple Silicon M1/M2/M3/M4) -> `trulang-darwin-arm64.tar.gz`
    - **macOS x86_64** (Intel) -> `trulang-darwin-x86_64.tar.gz`
    - **Linux x86_64** (Ubuntu / Debian / CentOS / Alpine) -> `trulang-linux-x86_64.tar.gz`
    - **Windows x64** (MSVC) -> `trulang-windows-x64.zip`
  - Automated 1-line installation script (`scripts/install.sh`) detecting OS/arch and fetching prebuilt standalone binaries directly from GitHub Releases.
- **Cross-Platform Shared Memory (`memmap2`)**:
  - Replaced Unix-specific `libc::mmap` with `memmap2`, enabling full native Windows memory mapping alongside Linux and macOS.
- **Interactive Documentation Portal (`docs/index.html`)**:
  - Complete language specification with interactive sample loader.
  - In-browser bytecode VM playground.
  - Interactive multi-language benchmark visualizer comparing TruLang against Rust, Go, Node.js, Java, and Python.
