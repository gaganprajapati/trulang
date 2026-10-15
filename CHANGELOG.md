# Changelog 📜

All notable changes to the VLang language, compiler, runtime, and supervisor will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- **Elastic Burst Arenas**: Support tiered memory budgets (`memory_budget request = 16 MB max 64 MB;`) with burst headroom and `ArenaBurst = 5` Seqlock telemetry warnings, while retaining strict backward-compatible caps when `max` is omitted.
- **Compiler Auto-Promotion (`persist` Inference)**: Transparent compiler pass that automatically injects `Opcode::Persist` when writing transient data into persistent actor fields or mutating persistent maps (`cache.insert(k, v)`), eliminating manual `persist()` boilerplate while keeping explicit calls valid.
- **Enhanced CLI Diagnostics**: `vlang run` reports base budget, max ceiling, and runtime elastic burst count.
- **Primitive Type Aliases**: Added parser support for `String`, `Int`, `Float`, `Bool` capitalized type names alongside standard lowercase tokens.
- **New Example**: Added `examples/elastic_burst.vl` demonstrating elastic burst allocation and auto-promotion.

### Planned (Phase 2: Tooling & Ecosystem)
- **Language Server Protocol (`vlang-lsp`)**: Real-time syntax and type diagnostics, hover definitions, jump-to-definition, and auto-complete for modern IDEs (VS Code, Cursor, Neovim).
- **Rust/C FFI Bridge**: Zero-cost FFI module bridge (`import "rust:pkg" as mod`) to leverage mature database and messaging drivers.

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
  - Independent memory supervisor streaming metrics via lockless atomic shared memory (`/tmp/vlang_shm_active.dat`).
  - Zero-overhead performance profiling on running worker processes.
- **Language Syntax & Static Typing**:
  - Immutable by default (`let` vs `let mut`).
  - Static typechecker with local type inference and nullable types (`T?`).
  - Safe error handling with `try/catch` and deterministic `defer` RAII cleanup.
  - Checked integer arithmetic preventing overflow/underflow exploits.
  - Recursive descent parser with strict recursion depth protection (`MAX_RECURSION_DEPTH = 256`).
- **CLI & Developer Tooling**:
  - `vlang check <file>`: Static syntax, typing, and ownership verification.
  - `vlang run <file>`: High-speed bytecode compiler and virtual machine execution.
  - `vlang monitor`: Real-time streaming supervisor monitor.
  - `vlang bench <file>`: Multi-threaded concurrent benchmarking suite with latency percentile analysis.
  - Version inspection: Support for `vlang -v`, `vlang -V`, `vlang --version`, and `vlang version`.
- **Cross-Platform Releases & CI/CD**:
  - Automated GitHub Actions release pipeline (`.github/workflows/release.yml`) cross-compiling release binaries for:
    - **macOS ARM64** (Apple Silicon M1/M2/M3/M4) -> `vlang-darwin-arm64.tar.gz`
    - **macOS x86_64** (Intel) -> `vlang-darwin-x86_64.tar.gz`
    - **Linux x86_64** (Ubuntu / Debian / CentOS / Alpine) -> `vlang-linux-x86_64.tar.gz`
    - **Windows x64** (MSVC) -> `vlang-windows-x64.zip`
  - Automated 1-line installation script (`scripts/install.sh`) detecting OS/arch and fetching prebuilt standalone binaries directly from GitHub Releases.
- **Cross-Platform Shared Memory (`memmap2`)**:
  - Replaced Unix-specific `libc::mmap` with `memmap2`, enabling full native Windows memory mapping alongside Linux and macOS.
- **Interactive Documentation Portal (`docs/index.html`)**:
  - Complete language specification with interactive sample loader.
  - In-browser bytecode VM playground.
  - Interactive multi-language benchmark visualizer comparing VLang against Rust, Go, Node.js, Java, and Python.
