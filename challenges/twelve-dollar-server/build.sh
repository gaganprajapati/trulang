#!/usr/bin/env bash
# Builds the TruLang $12 Server binary in optimized release mode
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR/../.."
cargo build --release -p trulang-12d-server
mkdir -p "$DIR/bin"
cp "$DIR/../../target/release/trulang-12d-server" "$DIR/bin/server"
echo "✨ TruLang Challenge Server built successfully at $DIR/bin/server"
