#!/bin/bash
# Heavy integration test with real Claude CLI
set -euo pipefail

TEST_DIR=$(mktemp -d "${TMPDIR:-/tmp}/orch-heavy-XXXXXX")
SOCKET="$TEST_DIR/run/orchestrator/orchestrator.sock"
ORCH="./target/debug/orch"

cleanup() {
    if [ -n "${DAEMON_PID:-}" ]; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

echo "=== Heavy Integration Test with Claude Code ==="
echo ""

# ─── Build ────────────────────────────────────────────────
echo "[1/8] Building..."
cargo build --bin orchestratord --bin orch --quiet

# ─── Setup ────────────────────────────────────────────────
echo "[2/8] Setting up..."
mkdir -p "$TEST_DIR/run" "$TEST_DIR/data/orchestrator/repertoire/providers"

cat > "$TEST_DIR/data/orchestrator/repertoire/providers/claude.yaml" << 'YAML'
kind: Provider
version: 1
metadata:
  name: claude
  display_name: "Claude (Anthropic)"
detection:
  binary: claude
invocation:
  cmd: ["claude", "--print"]
  prompt_flag: "-p"
  extra_flags: ["--no-session-persistence"]
auth:
  env_var: "ANTHROPIC_API_KEY"
  methods: [detect, env]
YAML

# ─── Start daemon ─────────────────────────────────────────
echo "[3/8] Starting daemon..."
XDG_RUNTIME_DIR="$TEST_DIR/run" \
XDG_DATA_HOME="$TEST_DIR/data" \
RUST_LOG=error \
PATH="$HOME/.local/bin:$PATH" \
    ./target/debug/orchestratord &
DAEMON_PID=$!

for i in $(seq 1 50); do [ -S "$SOCKET" ] && break; sleep 0.1; done
if [ ! -S "$SOCKET" ]; then echo "FAIL: daemon did not start"; exit 1; fi

export XDG_RUNTIME_DIR="$TEST_DIR/run"

# ─── System checks ───────────────────────────────────────
echo "[4/8] System checks..."
HEALTH=$($ORCH health)
VERSION=$($ORCH version)
echo "       health=$HEALTH version=$VERSION"

if [ "$HEALTH" != "ok" ]; then echo "FAIL: health != ok"; exit 1; fi

$ORCH info
echo ""

# ─── Provider setup ──────────────────────────────────────
echo "[5/8] Provider setup..."
# Store a dummy credential (Claude Code uses session auth, ignores env var)
$ORCH provider add claude "session-auth-not-needed" 2>/dev/null
$ORCH provider test claude
echo ""

# ─── Solo performance ────────────────────────────────────
echo "[6/8] Solo performance (real Claude)..."
echo ""
echo "  Prompt: 'Respond in exactly one sentence: what is CQRS?'"
echo ""

START=$(date +%s%N)
$ORCH run -p claude "Respond in exactly one sentence: what is CQRS?"
END=$(date +%s%N)
ELAPSED=$(( (END - START) / 1000000 ))
echo ""
echo "  Wall time: ${ELAPSED}ms"
echo ""

# ─── Duet performance ────────────────────────────────────
echo "[7/8] Duet performance (real Claude, 2 parallel)..."
echo ""
echo "  Prompt: 'In one sentence, compare Redis vs Memcached'"
echo ""

START=$(date +%s%N)
$ORCH run -p claude -f duet "In one sentence, compare Redis vs Memcached"
END=$(date +%s%N)
ELAPSED=$(( (END - START) / 1000000 ))
echo ""
echo "  Wall time: ${ELAPSED}ms"
echo ""

# ─── Metrics + History ───────────────────────────────────
echo "[8/8] Metrics and history..."
echo ""
echo "  --- performance list ---"
$ORCH performance list
echo ""
echo "  --- metrics ---"
$ORCH metrics
echo ""

echo "=== HEAVY TEST COMPLETE ==="
