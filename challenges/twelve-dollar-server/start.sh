#!/usr/bin/env bash
# Runs the TruLang server in the foreground. Config: SQLITE_PATH, JWT_SECRET, HOST, PORT.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
if [ -f "$DIR/bin/server" ]; then
    exec "$DIR/bin/server"
else
    exec "$DIR/../../target/release/trulang-12d-server"
fi
