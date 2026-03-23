#!/usr/bin/env bash
set -e

# ─── Config ──────────────────────────────────────────────
REPO="AngryTux/orchestrator"
REPO_URL="https://github.com/$REPO"
VERSION="0.1.0"
INSTALL_DIR="$HOME/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/orchestrator"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/orchestrator"
SYSTEMD_DIR="$HOME/.config/systemd/user"

# ─── Output helpers ──────────────────────────────────────
info()    { echo "→ $1"; }
ok()      { echo "✅ $1"; }
warn()    { echo "⚠️  $1"; }
fail()    { echo "❌ $1" >&2; exit 1; }

# Spinner — runs a command with an animated progress indicator
# Usage: spin "message" command arg1 arg2 ...
spin() {
    local msg="$1"; shift
    local frames='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
    local pid

    "$@" >/dev/null 2>&1 &
    pid=$!

    while kill -0 "$pid" 2>/dev/null; do
        for (( i=0; i<${#frames}; i++ )); do
            printf "\r  ${frames:$i:1} %s" "$msg" >&2
            sleep 0.08
        done
    done

    wait "$pid"
    local exit_code=$?
    printf "\r%-60s\r" "" >&2

    if [ $exit_code -eq 0 ]; then
        ok "$msg"
    else
        fail "$msg"
    fi

    return $exit_code
}

# ─── Banner ──────────────────────────────────────────────
cat << "BANNER"

   ╔═══════════════════════════════════╗
   ║        O R C H E S T R A T O R   ║
   ║   Secure LLM Workspace Manager   ║
   ╚═══════════════════════════════════╝

BANNER
echo "   Install Script — v$VERSION"
echo ""

# ─── Detect system ───────────────────────────────────────
detect_system() {
    OS=$(uname -s)
    ARCH=$(uname -m)
    KERNEL_FULL=$(uname -r)
    KERNEL=$(echo "$KERNEL_FULL" | cut -d. -f1-2)
    KERNEL_MAJOR=$(echo "$KERNEL" | cut -d. -f1)
    KERNEL_MINOR=$(echo "$KERNEL" | cut -d. -f2)

    [ "$OS" = "Linux" ] || fail "Linux required (got $OS)"

    case "$ARCH" in
        x86_64|aarch64) ;;
        *) fail "x86_64 or aarch64 required (got $ARCH)" ;;
    esac

    if [ "$KERNEL_MAJOR" -lt 6 ] || { [ "$KERNEL_MAJOR" -eq 6 ] && [ "$KERNEL_MINOR" -lt 12 ]; }; then
        warn "Kernel $KERNEL detected. Recommended >= 6.12 for full Landlock support."
        warn "Orchestrator will run with reduced isolation."
    fi

    ok "Linux $ARCH, kernel $KERNEL"
}

# ─── Check dependencies ─────────────────────────────────
check_deps() {
    local missing=""

    if ! command -v git >/dev/null 2>&1; then
        missing="$missing git"
    fi

    if ! command -v cargo >/dev/null 2>&1; then
        missing="$missing cargo"
    fi

    if [ -n "$missing" ]; then
        fail "Missing dependencies:$missing. Install cargo via https://rustup.rs"
    fi

    ok "Dependencies found (git, cargo)"
}

# ─── Check previous installation ────────────────────────
check_previous_install() {
    if [ -f "$INSTALL_DIR/orchestratord" ]; then
        warn "Existing installation found in $INSTALL_DIR"
        warn "Will be upgraded."
    fi
}

# ─── Download source ─────────────────────────────────────
download_source() {
    BUILD_DIR=$(mktemp -d)
    trap 'rm -rf "$BUILD_DIR"' EXIT

    spin "Downloading source..." git clone --depth 1 "$REPO_URL.git" "$BUILD_DIR/orchestrator"
    [ -d "$BUILD_DIR/orchestrator" ] || fail "Download failed"
}

# ─── Build ───────────────────────────────────────────────
build_release() {
    cd "$BUILD_DIR/orchestrator"
    spin "Building release binaries (this may take a few minutes)..." cargo build --release

    [ -f target/release/orchestratord ] || fail "orchestratord binary not found"
    [ -f target/release/orch ] || fail "orch binary not found"
}

# ─── Install binaries ────────────────────────────────────
install_binaries() {
    info "Installing binaries to $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR"
    install -m 755 target/release/orchestratord "$INSTALL_DIR/orchestratord"
    install -m 755 target/release/orch "$INSTALL_DIR/orch"
    ok "orchestratord and orch installed"
}

# ─── Create directories ─────────────────────────────────
create_dirs() {
    info "Creating data directories..."
    mkdir -p "$DATA_DIR/repertoire/providers"
    mkdir -p "$DATA_DIR/namespaces"
    mkdir -p "$DATA_DIR/db"
    mkdir -p "$CONFIG_DIR"
    ok "Directories created"
}

# ─── Install repertoire specs ────────────────────────────
install_repertoire() {
    info "Installing provider specs..."
    if [ -d "$BUILD_DIR/orchestrator/repertoire/providers" ]; then
        cp -n "$BUILD_DIR/orchestrator/repertoire/providers/"*.yaml \
            "$DATA_DIR/repertoire/providers/" 2>/dev/null || true
    fi
    ok "Repertoire installed"
}

# ─── Install systemd units ──────────────────────────────
install_systemd() {
    if ! command -v systemctl >/dev/null 2>&1; then
        warn "systemd not found. Start daemon manually: orchestratord"
        return
    fi

    info "Installing systemd user units..."
    mkdir -p "$SYSTEMD_DIR"

    cat > "$SYSTEMD_DIR/orchestratord.socket" << 'UNIT'
[Unit]
Description=Orchestrator daemon socket

[Socket]
ListenStream=%t/orchestrator/orchestrator.sock
SocketMode=0600
DirectoryMode=0700

[Install]
WantedBy=sockets.target
UNIT

    cat > "$SYSTEMD_DIR/orchestratord.service" << UNIT
[Unit]
Description=Orchestrator daemon
After=network.target
Requires=orchestratord.socket

[Service]
Type=notify
ExecStart=$INSTALL_DIR/orchestratord
NotifyAccess=main
WatchdogSec=30
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=$DATA_DIR
PrivateTmp=yes
ProtectKernelTunables=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes

[Install]
WantedBy=default.target
UNIT

    systemctl --user daemon-reload
    systemctl --user enable orchestratord.socket >/dev/null 2>&1
    systemctl --user start orchestratord.socket >/dev/null 2>&1
    ok "systemd socket activated"
}

# ─── Verify installation ────────────────────────────────
verify_install() {
    local all_ok=true

    if [ -f "$INSTALL_DIR/orchestratord" ]; then
        ok "orchestratord binary"
    else
        fail "orchestratord not found"
        all_ok=false
    fi

    if [ -f "$INSTALL_DIR/orch" ]; then
        ok "orch binary"
    else
        fail "orch not found"
        all_ok=false
    fi

    if [ -d "$DATA_DIR" ]; then
        ok "data directory"
    else
        warn "data directory missing"
        all_ok=false
    fi

    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        warn "$INSTALL_DIR is not in PATH"
        warn "Add to your shell profile: export PATH=\"$INSTALL_DIR:\$PATH\""
    fi

    return 0
}

# ─── Run ─────────────────────────────────────────────────
echo "=== Step 1: System detection ==="
detect_system
echo ""

echo "=== Step 2: Dependencies ==="
check_deps
check_previous_install
echo ""

echo "=== Step 3: Download ==="
download_source
echo ""

echo "=== Step 4: Build ==="
build_release
echo ""

echo "=== Step 5: Install ==="
install_binaries
create_dirs
install_repertoire
echo ""

echo "=== Step 6: systemd ==="
install_systemd
echo ""

echo "=== Step 7: Verify ==="
verify_install
echo ""

cat << EOF
╔══════════════════════════════════════════╗
║   Orchestrator installed (v$VERSION)        ║
╚══════════════════════════════════════════╝

  Get started:

    orch health                 # check daemon
    orch info                   # system capabilities
    orch provider add claude YOUR_API_KEY
    orch run "what is CQRS?"

  Using Claude Code (no API key needed):

    orch provider add claude dummy
    orch run "what is CQRS?"

EOF
