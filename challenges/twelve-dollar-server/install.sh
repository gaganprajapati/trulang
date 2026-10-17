#!/usr/bin/env bash
# Installs toolchain on clean Ubuntu 24.04 (Challenge Rule 10)
set -euo pipefail
if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
