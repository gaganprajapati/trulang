# Contributing to TruLang ⚡

Thank you for your interest in contributing to TruLang! We welcome contributions from developers of all skill levels—whether you are reporting an issue, improving documentation, fixing a bug, or building a new language feature.

---

## 🧭 Code of Conduct

We are committed to providing a friendly, safe, and welcoming environment for all contributors. Please be respectful, constructive, and open to feedback.

---

## 🏗️ Architecture & Codebase Overview

TruLang is implemented in modern, idiomatic Rust as a modular Cargo workspace:

```
trulang/
├── crates/
│   ├── syntax/         # Lexer (Logos), recursive descent parser, AST, and recursion limits
│   ├── compiler/       # AST static analysis, borrow checker, typechecker, and bytecode generator
│   ├── vm/             # Stack-based Bytecode VM, dual-region memory arenas, and Actor runtime
│   ├── supervisor/     # Atomic 64-bit Seqlock shared memory telemetry protocol (memmap2)
│   └── cli/            # Standalone CLI binary (trulang check, run, monitor, bench)
├── examples/           # TruLang source programs (.tr) demonstrating language features
├── benchmarks/         # Multi-language comparison scripts (Rust, Go, Node.js, Python, Java)
├── docs/               # Interactive documentation, language specification, and playground
└── tests/              # Multi-threaded load, leakage, and backpressure integration tests
```

### Module Responsibilities:
1. **`crates/syntax`**:
   - Tokenizes input with `logos`.
   - Parses AST via recursive descent with an explicit recursion depth limit (`MAX_RECURSION_DEPTH = 256`) to prevent stack overflow exploits.
2. **`crates/compiler`**:
   - Statically checks variable bindings (`let` vs `let mut`), immutability across scopes, and type inference.
   - Emits compact bytecode instructions (`Opcode`) for the virtual machine.
3. **`crates/vm`**:
   - Executes bytecode on a high-speed stack VM.
   - Manages **Region A** (bump-pointer arena for request-scoped transient memory).
   - Manages **Region B** (reference-counted DAG storage for persistent actor state).
   - Enforces single-threaded actor isolation and bounded mailbox backpressure.
4. **`crates/supervisor`**:
   - Cross-platform shared memory (`memmap2`) publishing real-time telemetry via lock-free atomic Seqlocks.
   - Supported natively across Linux, macOS (Apple Silicon & Intel), and Windows MSVC.

---

## 🛠️ Setting Up Your Development Environment

### Prerequisites:
- **Rust toolchain 1.75+** (`rustc` and `cargo`):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Git**

### Clone and Build:
```bash
git clone https://github.com/gaganprajapati/trulang.git
cd trulang

# Check workspace compiles with zero errors
cargo check --all-targets

# Build debug binary
cargo build

# Run all unit and integration test suites
cargo test
```

---

## 🧪 Testing Guidelines (Strict Quality Standards)

TruLang adheres to strict Test-Driven Development (TDD). Every new feature, language construct, or bug fix **must include automated tests**.

### 1. Running the Full Test Suite
```bash
cargo test
```
All 25 unit and integration tests across syntax, compiler, VM, and supervisor must pass.

### 2. Running Load & Zero-Leakage Stress Tests
To ensure the dual-region memory model and Seqlock telemetry do not leak memory under high concurrency:
```bash
cargo test --test load_and_leak_test -- --nocapture
```
This runs 100,000 sustained iterations verifying that Region A arena resets achieve 0 bytes drift.

### 3. Testing the CLI Locally
```bash
# Check syntax of an example
cargo run --bin trulang -- check examples/user_service.tr

# Execute on Bytecode VM
cargo run --bin trulang -- run examples/user_service.tr

# Run concurrent benchmarking
cargo run --bin trulang -- bench examples/user_service.tr --requests 50000 --concurrency 8
```

---

## 📐 Clean Code & Engineering Standards

To keep the codebase maintainable and robust, contributors must follow these clean-code rules:

1. **Zero Panics in Library Crates**:
   - Never call `.unwrap()` or `.expect()` in `crates/syntax`, `crates/compiler`, or `crates/vm` on user-controlled inputs.
   - Always return typed errors (`Result<T, SyntaxError>`, `Result<T, CompileError>`, `Result<T, VmError>`).
2. **Explicit Separation of Concerns (SRP)**:
   - AST nodes belong in `crates/syntax`.
   - Semantic analysis and bytecode generation belong in `crates/compiler`.
   - Runtime execution, memory allocation, and actors belong in `crates/vm`.
3. **Cross-Platform Compatibility**:
   - Do not use raw POSIX syscalls (`libc::mmap`, `libc::getuid`, `std::os::fd`) without cross-platform abstraction (`memmap2` and `#[cfg(unix)]` / `#[cfg(windows)]`).
   - All code must build cleanly on Linux, macOS, and Windows.
4. **Formatting & Clippy**:
   - Format all code with `rustfmt`:
     ```bash
     cargo fmt --all -- --check
     ```
   - Ensure zero clippy warnings:
     ```bash
     cargo clippy --all-targets -- -D warnings
     ```

---

## 🔀 Submitting a Pull Request (PR)

1. **Fork the repository** on GitHub.
2. **Create a descriptive feature branch**:
   ```bash
   git checkout -b feature/auto-promotion-inference
   # or
   git checkout -b fix/windows-shm-permissions
   ```
3. **Write clean, tested commits**:
   - Follow conventional commit messages:
     - `feat(compiler): add auto-promotion inference for actor cache`
     - `fix(vm): prevent overflow on large slice allocations`
     - `docs: update installation guide with new release flags`
4. **Push to your fork and submit a PR** against the `main` branch.
5. **CI Checklist**: Ensure all GitHub Actions matrix builds pass (Linux, macOS ARM64, macOS x86_64, Windows MSVC).

---

## 💬 Community & Questions

- **Issues**: Use [GitHub Issues](https://github.com/gaganprajapati/trulang/issues) to report bugs or submit feature proposals.
- **Discussions**: Start a thread in GitHub Discussions for architectural ideas or questions.
