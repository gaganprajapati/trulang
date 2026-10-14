#!/usr/bin/env bash
set -e

echo "=================================================="
echo "          ⚡ VLang Toolchain Installer           "
echo "=================================================="

GITHUB_REPO="gaganprajapati/vlang"
INSTALL_DIR="/usr/local/bin"

# Determine user install dir if /usr/local/bin is not writable
if [ ! -w "$INSTALL_DIR" ]; then
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
fi

# Detect Operating System and Architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
    linux)   TARGET_OS="linux" ;;
    darwin)  TARGET_OS="darwin" ;;
    *)       TARGET_OS="$OS" ;;
esac

case "$ARCH" in
    x86_64|amd64)   TARGET_ARCH="x86_64" ;;
    arm64|aarch64)  TARGET_ARCH="arm64" ;;
    *)              TARGET_ARCH="$ARCH" ;;
esac

ARTIFACT_NAME="vlang-${TARGET_OS}-${TARGET_ARCH}.tar.gz"
DOWNLOAD_URL="https://github.com/${GITHUB_REPO}/releases/latest/download/${ARTIFACT_NAME}"

echo "Detected Platform: ${TARGET_OS}-${TARGET_ARCH}"

install_from_source() {
    echo ""
    echo "📦 Falling back to building from source via Cargo..."

    if ! command -v cargo >/dev/null 2>&1; then
        echo "❌ Error: Rust and Cargo are required to compile VLang from source."
        echo "Please install Rust first by running:"
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi

    echo "✓ Detected Rust toolchain: $(cargo --version)"

    if [ -f "crates/cli/Cargo.toml" ]; then
        echo "🔨 Building and installing from local workspace..."
        cargo install --path crates/cli
    else
        TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t 'vlang')
        trap 'rm -rf "$TMP_DIR"' EXIT
        echo "📦 Cloning VLang from https://github.com/${GITHUB_REPO}.git..."
        git clone --depth 1 "https://github.com/${GITHUB_REPO}.git" "${TMP_DIR}"
        cd "${TMP_DIR}"
        echo "🔨 Compiling vlang release binary..."
        cargo install --path crates/cli
    fi
}

# Try pre-compiled binary download first (Instant 2-second install, no Rust required)
TMP_DOWNLOAD_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t 'vlang-dl')
trap 'rm -rf "$TMP_DOWNLOAD_DIR"' EXIT

echo "🔍 Checking for prebuilt release binary at ${DOWNLOAD_URL}..."

HTTP_STATUS=$(curl -s -L -o "${TMP_DOWNLOAD_DIR}/${ARTIFACT_NAME}" -w "%{http_code}" "$DOWNLOAD_URL" || echo "000")

if [ "$HTTP_STATUS" = "200" ] && [ -s "${TMP_DOWNLOAD_DIR}/${ARTIFACT_NAME}" ]; then
    echo "✓ Downloaded prebuilt binary archive (${ARTIFACT_NAME})."
    echo "📦 Extracting and installing to ${INSTALL_DIR}/vlang..."
    
    tar -xzf "${TMP_DOWNLOAD_DIR}/${ARTIFACT_NAME}" -C "${TMP_DOWNLOAD_DIR}"
    
    if [ -w "$INSTALL_DIR" ]; then
        mv "${TMP_DOWNLOAD_DIR}/vlang" "${INSTALL_DIR}/vlang"
        chmod +x "${INSTALL_DIR}/vlang"
    else
        echo "🔐 Sudo access required to install to ${INSTALL_DIR}:"
        sudo mv "${TMP_DOWNLOAD_DIR}/vlang" "${INSTALL_DIR}/vlang"
        sudo chmod +x "${INSTALL_DIR}/vlang"
    fi
    
    echo ""
    echo "=================================================="
    echo "  ✅ VLang successfully installed to ${INSTALL_DIR}/vlang!"
    echo "=================================================="
else
    echo "ℹ️ No pre-compiled binary found on GitHub Releases for ${TARGET_OS}-${TARGET_ARCH}."
    install_from_source
fi

# Ensure install dir is in PATH
if ! echo "$PATH" | grep -q "$INSTALL_DIR" && ! echo "$PATH" | grep -q "$HOME/.cargo/bin"; then
    echo "⚠️ Note: Make sure ${INSTALL_DIR} is in your PATH. Add this to your ~/.zshrc or ~/.bashrc:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
fi

echo ""
if command -v vlang >/dev/null 2>&1; then
    echo "Installed version: $(vlang --version)"
    echo ""
    echo "Try running:"
    echo "  vlang --help"
fi
