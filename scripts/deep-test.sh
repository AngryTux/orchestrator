#!/bin/bash
# Deep integration test — validates isolation, security, and edge cases
# with real Claude CLI.
set -euo pipefail

TEST_DIR=$(mktemp -d "${TMPDIR:-/tmp}/orch-deep-XXXXXX")
SOCKET="$TEST_DIR/run/orchestrator/orchestrator.sock"
ORCH="./target/debug/orch"
PASS=0
FAIL=0

cleanup() {
    if [ -n "${DAEMON_PID:-}" ]; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

check() {
    local name="$1"
    local condition="$2"
    if eval "$condition"; then
        echo "  PASS  $name"
        PASS=$((PASS + 1))
    else
        echo "  FAIL  $name"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== Deep Integration Test ==="
echo ""

# ─── Setup ────────────────────────────────────────────────
cargo build --bin orchestratord --bin orch --quiet

mkdir -p "$TEST_DIR/run" "$TEST_DIR/data/orchestrator/repertoire/providers"

cat > "$TEST_DIR/data/orchestrator/repertoire/providers/claude.yaml" << 'YAML'
kind: Provider
version: 1
metadata:
  name: claude
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

# Mock provider (for isolation tests — no network needed)
MOCK="$TEST_DIR/mock-provider"
cat > "$MOCK" << 'SCRIPT'
#!/bin/sh
while [ $# -gt 0 ]; do case "$1" in -p) shift; echo "mock: $1"; exit 0;; esac; shift; done
SCRIPT
chmod +x "$MOCK"

cat > "$TEST_DIR/data/orchestrator/repertoire/providers/mock.yaml" << YAML
kind: Provider
version: 1
metadata:
  name: mock
detection:
  binary: mock
invocation:
  cmd: ["$MOCK"]
  prompt_flag: "-p"
auth:
  env_var: "MOCK_KEY"
  methods: [env]
YAML

XDG_RUNTIME_DIR="$TEST_DIR/run" \
XDG_DATA_HOME="$TEST_DIR/data" \
RUST_LOG=error \
PATH="$HOME/.local/bin:$PATH" \
    ./target/debug/orchestratord &
DAEMON_PID=$!

for i in $(seq 1 50); do [ -S "$SOCKET" ] && break; sleep 0.1; done
export XDG_RUNTIME_DIR="$TEST_DIR/run"

# Store credentials
$ORCH provider add claude "dummy" 2>/dev/null
$ORCH provider add mock "test-key" 2>/dev/null

echo "─── 1. Credential Isolation ─────────────────────"
echo ""

# Credentials scoped per namespace
$ORCH provider add mock "ns-default-key" -n default 2>/dev/null
$ORCH provider add mock "ns-secure-key" -n secure 2>/dev/null

DEFAULT_PROVIDERS=$($ORCH provider list -n default)
SECURE_PROVIDERS=$($ORCH provider list -n secure)

check "default ns has providers" '[ -n "$DEFAULT_PROVIDERS" ]'
check "secure ns has providers" '[ -n "$SECURE_PROVIDERS" ]'

# Remove from one namespace — other unaffected
$ORCH provider rm mock -n secure 2>/dev/null
SECURE_AFTER=$($ORCH provider list -n secure)
DEFAULT_AFTER=$($ORCH provider list -n default)

check "secure ns empty after rm" 'echo "$SECURE_AFTER" | grep -q "No providers"'
check "default ns still has mock after secure rm" 'echo "$DEFAULT_AFTER" | grep -q "mock"'

echo ""
echo "─── 2. Clean Environment (mock provider) ────────"
echo ""

# Mock provider runs with env auth — should get clean env
ENV_MOCK="$TEST_DIR/env-mock"
cat > "$ENV_MOCK" << 'SCRIPT'
#!/bin/sh
env | sort
SCRIPT
chmod +x "$ENV_MOCK"

cat > "$TEST_DIR/data/orchestrator/repertoire/providers/envtest.yaml" << YAML
kind: Provider
version: 1
metadata:
  name: envtest
detection:
  binary: envtest
invocation:
  cmd: ["$ENV_MOCK"]
  prompt_flag: "-p"
auth:
  env_var: "ENVTEST_KEY"
  methods: [env]
YAML

# Restart daemon to pick up new provider
kill "$DAEMON_PID" 2>/dev/null; wait "$DAEMON_PID" 2>/dev/null
XDG_RUNTIME_DIR="$TEST_DIR/run" \
XDG_DATA_HOME="$TEST_DIR/data" \
RUST_LOG=error \
PATH="$HOME/.local/bin:$PATH" \
    ./target/debug/orchestratord &
DAEMON_PID=$!
for i in $(seq 1 50); do [ -S "$SOCKET" ] && break; sleep 0.1; done

$ORCH provider add envtest "secret-key-xyz" 2>/dev/null

ENV_OUTPUT=$($ORCH run -p envtest "test" 2>/dev/null || true)

check "env contains ENVTEST_KEY" 'echo "$ENV_OUTPUT" | grep -q "ENVTEST_KEY=secret-key-xyz"'
check "env does NOT contain HOME" '! echo "$ENV_OUTPUT" | grep -q "^HOME="'
check "env does NOT contain USER" '! echo "$ENV_OUTPUT" | grep -q "^USER="'
check "env does NOT contain SHELL" '! echo "$ENV_OUTPUT" | grep -q "^SHELL="'
check "env does NOT contain XDG_RUNTIME_DIR" '! echo "$ENV_OUTPUT" | grep -q "^XDG_RUNTIME_DIR="'

ENV_COUNT=$(echo "$ENV_OUTPUT" | grep -c "=" || true)
# sh auto-sets PWD, SHLVL, _ — so clean env has credential + ~3 shell vars
check "env has minimal vars (<=6)" '[ "$ENV_COUNT" -le 6 ]'

echo ""
echo "─── 3. Detect Auth (Claude) preserves env ───────"
echo ""

# Claude with detect auth should have HOME, PATH
CLAUDE_HEALTH=$($ORCH run -p claude "respond with exactly: alive" 2>/dev/null || echo "FAILED")

check "claude responds (detect auth works)" 'echo "$CLAUDE_HEALTH" | grep -iq "alive"'

echo ""
echo "─── 4. Namespace Isolation ───────────────────────"
echo ""

$ORCH namespace create test-ns-1 2>/dev/null
$ORCH namespace create test-ns-2 2>/dev/null

NS_LIST=$($ORCH namespace list)
check "namespace list includes test-ns-1" 'echo "$NS_LIST" | grep -q "test-ns-1"'
check "namespace list includes test-ns-2" 'echo "$NS_LIST" | grep -q "test-ns-2"'

# Performance in one namespace not visible in another
$ORCH provider add mock "key" -n test-ns-1 2>/dev/null
$ORCH run -p mock -n test-ns-1 "namespace test" 2>/dev/null

PERFS_NS1=$($ORCH performance list -n test-ns-1)
PERFS_NS2=$($ORCH performance list -n test-ns-2)

check "performance visible in test-ns-1" 'echo "$PERFS_NS1" | grep -q "namespace test"'
check "performance NOT visible in test-ns-2" 'echo "$PERFS_NS2" | grep -q "No performances"'

# Cleanup
$ORCH namespace rm test-ns-1 2>/dev/null
$ORCH namespace rm test-ns-2 2>/dev/null
NS_AFTER=$($ORCH namespace list)
check "namespaces cleaned up" '! echo "$NS_AFTER" | grep -q "test-ns-1"'

echo ""
echo "─── 5. Performance History & Metrics ────────────"
echo ""

# Run multiple performances
$ORCH run -p mock "perf one" 2>/dev/null
$ORCH run -p mock "perf two" 2>/dev/null
$ORCH run -p mock "perf three" 2>/dev/null

PERF_LIST=$($ORCH performance list)
METRICS=$($ORCH metrics)

PERF_COUNT=$(echo "$PERF_LIST" | grep -c "^perf-" || true)
check "3+ performances recorded" '[ "$PERF_COUNT" -ge 3 ]'

TOTAL_PERFS=$(echo "$METRICS" | grep "Performances:" | awk '{print $2}')
check "metrics shows 4+ total (including earlier)" '[ "$TOTAL_PERFS" -ge 4 ]'

echo ""
echo "─── 6. Duet Parallel Execution ──────────────────"
echo ""

START=$(date +%s%N)
$ORCH run -p mock -f duet "parallel test" 2>/dev/null
END=$(date +%s%N)
DUET_MS=$(( (END - START) / 1000000 ))

check "duet completes in <2s (parallel)" '[ "$DUET_MS" -lt 2000 ]'

DUET_PERFS=$($ORCH performance list)
check "duet recorded as duet formation" 'echo "$DUET_PERFS" | grep -q "duet"'

echo ""
echo "─── 7. Error Handling ───────────────────────────"
echo ""

# Nonexistent provider
ERR=$($ORCH run -p nonexistent "test" 2>&1 || true)
check "nonexistent provider returns error" 'echo "$ERR" | grep -qi "error\|400"'

# Invalid namespace (path traversal)
ERR=$($ORCH provider list -n "../../etc" 2>&1 || true)
check "path traversal rejected" 'echo "$ERR" | grep -qi "error\|400"'

echo ""
echo "─── 8. Provider Test Endpoint ───────────────────"
echo ""

TEST_RESULT=$($ORCH provider test claude)
check "provider test: credential valid" 'echo "$TEST_RESULT" | grep -q "valid"'
check "provider test: binary found" 'echo "$TEST_RESULT" | grep -q "found"'

echo ""
echo "==========================================="
echo "  Results: $PASS passed, $FAIL failed"
echo "==========================================="

[ "$FAIL" -eq 0 ] || exit 1
