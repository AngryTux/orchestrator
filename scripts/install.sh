#!/bin/sh
# Orchestrator installer
# Usage: curl -fsSL https://raw.githubusercontent.com/AngryTux/orchestrator/main/scripts/install.sh | sh
set -e

REPO="AngryTux/orchestrator"
REPO_URL="https://github.com/$REPO"
INSTALL_DIR="$HOME/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/orchestrator"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/orchestrator"
SYSTEMD_DIR="$HOME/.config/systemd/user"

# ─── Colors ───────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { printf "${CYAN}→${NC} %s\n" "$1"; }
ok()    { printf "${GREEN}✓${NC} %s\n" "$1"; }
warn()  { printf "${YELLOW}!${NC} %s\n" "$1"; }
fail()  { printf "${RED}✗${NC} %s\n" "$1"; exit 1; }

# ─── Detect ──────────────────────────────────────────────
info "Detecting system..."

OS=$(uname -s)
ARCH=$(uname -m)
KERNEL=$(uname -r | cut -d. -f1-2)
KERNEL_MAJOR=$(echo "$KERNEL" | cut -d. -f1)
KERNEL_MINOR=$(echo "$KERNEL" | cut -d. -f2)

[ "$OS" = "Linux" ] || fail "Linux required (got $OS)"
[ "$ARCH" = "x86_64" ] || [ "$ARCH" = "aarch64" ] || fail "x86_64 or aarch64 required (got $ARCH)"

# Kernel >= 6.12 recommended (Landlock ABI v6)
if [ "$KERNEL_MAJOR" -lt 6 ] || { [ "$KERNEL_MAJOR" -eq 6 ] && [ "$KERNEL_MINOR" -lt 12 ]; }; then
    warn "Kernel $KERNEL detected. Recommended >= 6.12 for full Landlock support."
    warn "Orchestrator will run with reduced isolation."
fi

ok "Linux $ARCH, kernel $KERNEL"

# ─── Dependencies ────────────────────────────────────────
info "Checking dependencies..."

command -v git >/dev/null 2>&1 || fail "git is required"
command -v cargo >/dev/null 2>&1 || fail "cargo is required (install via https://rustup.rs)"
command -v systemctl >/dev/null 2>&1 || warn "systemd not found — socket activation unavailable"

ok "git, cargo found"

# ─── Check existing installation ─────────────────────────
if [ -f "$INSTALL_DIR/orchestratord" ]; then
    CURRENT=$("$INSTALL_DIR/orchestratord" --version 2>/dev/null || echo "unknown")
    warn "Existing installation found ($CURRENT). Will upgrade."
fi

# ─── Clone or update ─────────────────────────────────────
BUILD_DIR=$(mktemp -d)
trap 'rm -rf "$BUILD_DIR"' EXIT

info "Downloading source..."
git clone --depth 1 --progress "$REPO_URL.git" "$BUILD_DIR/orchestrator" 2>&1 | \
    while IFS= read -r line; do
        case "$line" in
            *Receiving*|*Resolving*|*Counting*)
                printf "\r  ${CYAN}%s${NC}" "$line"
                ;;
        esac
    done
printf "\r%-60s\n" ""
[ -d "$BUILD_DIR/orchestrator" ] || fail "Download failed"
ok "Source downloaded"

# ─── Build ───────────────────────────────────────────────
info "Building (release mode). This may take a few minutes..."
cd "$BUILD_DIR/orchestrator"

# Show progress spinner during compilation
cargo build --release 2>&1 | while IFS= read -r line; do
    # Count compiled crates
    case "$line" in
        *Compiling*)
            CRATE=$(echo "$line" | sed 's/.*Compiling \([^ ]*\).*/\1/')
            printf "\r${CYAN}  compiling${NC} %-30s" "$CRATE"
            ;;
        *Finished*)
            printf "\r%-50s\n" ""
            ;;
    esac
done

# Verify build succeeded
[ -f target/release/orchestratord ] || fail "Build failed"
[ -f target/release/orch ] || fail "Build failed"
ok "Build complete"

# ─── Install binaries ────────────────────────────────────
info "Installing binaries to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
install -m 755 target/release/orchestratord "$INSTALL_DIR/orchestratord"
install -m 755 target/release/orch "$INSTALL_DIR/orch"
ok "orchestratord and orch installed"

# ─── Create directories ─────────────────────────────────
info "Creating directories..."
mkdir -p "$DATA_DIR/repertoire/providers"
mkdir -p "$DATA_DIR/namespaces"
mkdir -p "$DATA_DIR/db"
mkdir -p "$CONFIG_DIR"
ok "XDG directories created"

# ─── Install repertoire ─────────────────────────────────
info "Installing repertoire..."
if [ -d "$BUILD_DIR/orchestrator/repertoire/providers" ]; then
    cp -n "$BUILD_DIR/orchestrator/repertoire/providers/"*.yaml "$DATA_DIR/repertoire/providers/" 2>/dev/null || true
fi
ok "Provider specs installed"

# ─── systemd units ───────────────────────────────────────
if command -v systemctl >/dev/null 2>&1; then
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
PrivateTmp=yes
ProtectKernelTunables=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes

[Install]
WantedBy=default.target
UNIT

    systemctl --user daemon-reload
    systemctl --user enable orchestratord.socket 2>/dev/null
    systemctl --user start orchestratord.socket 2>/dev/null
    ok "systemd socket activated"
else
    warn "systemd not available — start daemon manually: orchestratord"
fi

# ─── Verify ──────────────────────────────────────────────
info "Verifying installation..."

if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    warn "$INSTALL_DIR is not in PATH. Add to your shell profile:"
    warn "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

VERSION=$("$INSTALL_DIR/orch" version 2>/dev/null || echo "unknown")

echo ""
printf "${GREEN}════════════════════════════════════════${NC}\n"
printf "${GREEN}  Orchestrator installed (v%s)${NC}\n" "$VERSION"
printf "${GREEN}════════════════════════════════════════${NC}\n"
echo ""
echo "  Get started:"
echo ""
echo "    orch health              # check daemon"
echo "    orch info                # system capabilities"
echo "    orch provider add claude YOUR_API_KEY"
echo "    orch run \"what is CQRS?\""
echo ""
echo "  Or use Claude Code (no API key needed):"
echo ""
echo "    orch provider add claude dummy"
echo "    orch run \"what is CQRS?\""
echo ""
