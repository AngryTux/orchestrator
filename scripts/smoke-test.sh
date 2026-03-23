#!/bin/bash
# Orchestrator — Solo smoke test
# Tests the full performance pipeline with a mock provider.
#
# Usage: ./scripts/smoke-test.sh
# Or:    make smoke

set -euo pipefail

# ─── Config ───────────────────────────────────────────────
TEST_DIR=$(mktemp -d /tmp/orch-smoke-XXXXXX)
SOCKET="$TEST_DIR/run/orchestrator/orchestrator.sock"
DATA_DIR="$TEST_DIR/orchestrator"
MOCK_BINARY="$TEST_DIR/mock-provider"

cleanup() {
    if [ -n "${DAEMON_PID:-}" ]; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

echo "=== Orchestrator Solo Smoke Test ==="
echo ""

# ─── 1. Build ─────────────────────────────────────────────
echo "[1/6] Building..."
cargo build --bin orchestratord --quiet

# ─── 2. Create mock provider ──────────────────────────────
echo "[2/6] Creating mock provider..."
cat > "$MOCK_BINARY" << 'SCRIPT'
#!/bin/sh
while [ $# -gt 0 ]; do
    case "$1" in
        -p) shift; PROMPT="$1" ;;
    esac
    shift
done
cat << EOF
CQRS (Command Query Responsibility Segregation) is a pattern that separates
read operations from write operations in a system. The key idea is that the
model used to update data can be different from the model used to read data.

This was a mock response to: "$PROMPT"
(Powered by mock-provider, API key present: ${MOCK_API_KEY:+yes})
EOF
SCRIPT
chmod +x "$MOCK_BINARY"

# ─── 3. Create provider spec ─────────────────────────────
echo "[3/6] Setting up repertoire..."
mkdir -p "$DATA_DIR/repertoire/providers"
cat > "$DATA_DIR/repertoire/providers/mock.yaml" << YAML
kind: Provider
version: 1
metadata:
  name: mock
  display_name: "Mock Provider"
detection:
  binary: mock-provider
invocation:
  cmd: ["$MOCK_BINARY"]
  prompt_flag: "-p"
auth:
  env_var: "MOCK_API_KEY"
  methods: [env]
YAML

# ─── 4. Start daemon ─────────────────────────────────────
echo "[4/6] Starting daemon..."
mkdir -p "$TEST_DIR/run"
XDG_RUNTIME_DIR="$TEST_DIR/run" \
XDG_DATA_HOME="$TEST_DIR" \
RUST_LOG=info,orchestratord=info \
    ./target/debug/orchestratord &
DAEMON_PID=$!

# Wait for socket
for i in $(seq 1 50); do
    [ -S "$SOCKET" ] && break
    sleep 0.1
done

if [ ! -S "$SOCKET" ]; then
    echo "FAIL: daemon did not start (socket not found)"
    exit 1
fi
echo "       daemon PID=$DAEMON_PID, socket=$SOCKET"

CURL="curl -s --unix-socket $SOCKET http://localhost"

# ─── 5. Store credential ─────────────────────────────────
echo "[5/6] Storing credential..."
RESULT=$($CURL/v1/namespaces/default/providers \
    -X POST \
    -H "Content-Type: application/json" \
    -d '{"name": "mock", "key": "sk-test-key-12345"}')
echo "       $RESULT"

# ─── 6. Perform! ─────────────────────────────────────────
echo "[6/6] Performing Solo..."
echo ""
echo "─── Request ────────────────────────────────────────"
echo '  POST /v1/namespaces/default/performances'
echo '  {"prompt": "what is CQRS?"}'
echo ""
echo "─── Response ───────────────────────────────────────"

CODA=$($CURL/v1/namespaces/default/performances \
    -X POST \
    -H "Content-Type: application/json" \
    -d '{"prompt": "what is CQRS?", "provider": "mock"}')

echo "$CODA" | python3 -m json.tool

echo ""
echo "─── Verification ───────────────────────────────────"

# Verify the response
if echo "$CODA" | python3 -c "
import json, sys
coda = json.load(sys.stdin)
assert coda['formation'] == 'solo', f'wrong formation: {coda[\"formation\"]}'
assert len(coda['sections']) == 1, f'wrong section count: {len(coda[\"sections\"])}'
assert coda['sections'][0]['success'] == True, 'section failed'
assert 'CQRS' in coda['summary'], 'summary missing CQRS'
assert coda['total_duration_ms'] > 0, 'no duration'
print('  ALL CHECKS PASSED')
" 2>&1; then
    echo ""
    echo "=== SMOKE TEST PASSED ==="
else
    echo ""
    echo "=== SMOKE TEST FAILED ==="
    exit 1
fi
