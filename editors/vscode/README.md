# TruLang for Visual Studio Code

Official Visual Studio Code extension for **[TruLang](https://gagn.dev/trulang/)**, the high-performance memory-safe backend programming language with dual-region memory arenas and zero-lock service actors.

<p align="center">
  <img src="icon.png" width="128" alt="TruLang Logo">
</p>

## ✨ Features

- **🎨 Full Syntax Highlighting:** Clean, vibrant TextMate grammar for all TruLang primitives:
  - Declarations: `service`, `actor`, `fn`, `let`, `mut`, `struct`, `enum`
  - Dual-Region Memory Primitives: `persist`, `memory_budget`, `request`, `persistent`, `max`, `MB`, `KB`, `GB`
  - Concurrency & Error Handling: `spawn`, `receive`, `defer`, `try`, `catch`
  - Types: `i64`, `f64`, `bool`, `string`, `RegionA`, `RegionB`
- **🧠 Language Server Protocol (LSP) Integration:**
  - **Real-Time Diagnostics:** Inline squigglies for syntax errors and immutability violations (e.g. attempting to reassign `let` without `let mut`).
  - **Hover Information:** Rich type signatures and documentation for service actors and methods.
  - **Go to Definition:** Jump directly from call sites to function declarations.
  - **Document Formatting:** Instant AST-based formatting via `trulang fmt`.
- **⚡ Interactive Commands:**
  - `TruLang: Run Current File` (`trulang run`)
  - `TruLang: Type Check Current File` (`trulang check`)
  - `TruLang: Restart Language Server`
  - `TruLang: Open Documentation`

## 🚀 Installation & Requirements

1. **Install TruLang:**
   ```bash
   brew tap gaganprajapati/trulang
   brew install trulang
   ```
2. **Install this Extension:**
   - Install from the VS Code Marketplace or open the command palette:
   - Run `Extensions: Install from VSIX...` and select `trulang-0.1.0.vsix`.

## ⚙️ Configuration

| Setting | Default | Description |
| :--- | :--- | :--- |
| `trulang.lsp.path` | `"trulang"` | Path to the `trulang` executable in your `$PATH`. |
| `trulang.lsp.trace.server` | `"off"` | Trace communications between VS Code and the TruLang LSP. |

## 📖 Learn More

- Documentation: [https://gagn.dev/trulang/](https://gagn.dev/trulang/)
- Under the Hood Architecture: [https://gagn.dev/trulang/internals.html](https://gagn.dev/trulang/internals.html)
- GitHub Repository: [https://github.com/gaganprajapati/trulang](https://github.com/gaganprajapati/trulang)
