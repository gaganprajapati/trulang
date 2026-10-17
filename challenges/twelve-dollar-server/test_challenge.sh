#!/usr/bin/env bash
# Runs the official 42-check test suite against the TruLang Challenge Server
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
CHALLENGE_REPO="/tmp/twelve-dollar-server-challenge"
PORT=3000
HOST="127.0.0.1"
JWT_SECRET="twelve-dollar-challenge"
TMP_DIR="$(mktemp -d)"
DB_PATH="$TMP_DIR/feed.db"

if [ ! -f "$CHALLENGE_REPO/seed/feed.db" ]; then
    echo "Seed database not found in $CHALLENGE_REPO. Generating..."
    bash "$CHALLENGE_REPO/seed/make-seed.sh"
fi

cp "$CHALLENGE_REPO/seed/feed.db" "$DB_PATH"
echo "Prepared fresh database copy at $DB_PATH"

export SQLITE_PATH="$DB_PATH"
export JWT_SECRET="$JWT_SECRET"
export HOST="$HOST"
export PORT="$PORT"

echo "Starting TruLang Challenge Server on http://$HOST:$PORT..."
"$DIR/start.sh" > "$TMP_DIR/server.log" 2>&1 &
SERVER_PID=$!

cleanup() {
    echo "Cleaning up server PID $SERVER_PID..."
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "Waiting for server to become healthy..."
for i in $(seq 30); do
    if curl -sf "http://$HOST:$PORT/health" >/dev/null 2>&1; then
        echo "Server is healthy!"
        break
    fi
    sleep 0.2
done

echo "Running official test suite from $CHALLENGE_REPO/test/test.sh..."
bash "$CHALLENGE_REPO/test/test.sh" "http://$HOST:$PORT"
echo "🎉 ALL TEST CHECKS PASSED!"
