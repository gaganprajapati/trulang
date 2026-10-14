#!/usr/bin/env bash
set -e

echo "=================================================="
echo "          ⚡ VLang Toolchain Installer           "
echo "=================================================="

# Check for Rust toolchain
if ! command -v cargo >/dev/null 2>&1; then
    echo "❌ Error: Rust and Cargo are required to install VLang."
    echo "Please install Rust first by running:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

echo "✓ Detected Rust toolchain: $(cargo --version)"

REPO_URL="https://github.com/gaganprajapati/vlang.git"

# If script is run inside the repository root
if [ -f "crates/cli/Cargo.toml" ]; then
    echo "📦 Building and installing VLang from local workspace..."
    cargo install --path crates/cli
else
    # Piped via curl: clone into temporary directory and build
    TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t 'vlang')
    trap 'rm -rf "$TMP_DIR"' EXIT
    echo "📦 Cloning VLang from ${REPO_URL}..."
    git clone --depth 1 "${REPO_URL}" "${TMP_DIR}"
    cd "${TMP_DIR}"
    echo "🔨 Compiling and installing vlang CLI..."
    cargo install --path crates/cli
fi

echo ""
echo "=================================================="
echo "  ✅ VLang successfully installed to ~/.cargo/bin!"
echo "=================================================="

# Check if ~/.cargo/bin is in PATH
if ! echo "$PATH" | grep -q "$HOME/.cargo/bin"; then
    echo "⚠️ Note: Make sure ~/.cargo/bin is in your PATH. Add this to your ~/.zshrc or ~/.bashrc:"
    echo '  export PATH="$HOME/.cargo/bin:$PATH"'
fi

if command -v vlang >/dev/null 2>&1; then
    echo "Installed version: $(vlang --version)"
    echo ""
    echo "Try running:"
    echo "  vlang --help"
fi
