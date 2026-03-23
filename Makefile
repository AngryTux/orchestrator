# Orchestrator — Makefile
# Secure LLM workspace manager and orchestrator
#
# Usage:
#   make              Build debug binaries
#   make test         Run all tests
#   make check        Lint + audit + test (CI-ready)
#   make run          Start daemon in foreground
#   make curl-health  Quick smoke test against running daemon
#   make install      Install binaries + systemd units
#   make setup-tools  Install dev tools (clippy, tarpaulin, audit)

# ─── Config ───────────────────────────────────────────────
# Detect cargo: mise shim → rustup → PATH fallback
CARGO       := $(or $(shell which cargo 2>/dev/null),$(wildcard $(HOME)/.cargo/bin/cargo),cargo)
SOCKET      := $(XDG_RUNTIME_DIR)/orchestrator/orchestrator.sock
CURL        := curl -s --unix-socket $(SOCKET) http://localhost
INSTALL_DIR := $(HOME)/.cargo/bin
SYSTEMD_DIR := $(HOME)/.config/systemd/user

# ─── Build ────────────────────────────────────────────────
.PHONY: build build-release

build:                          ## Build debug binaries
	$(CARGO) build

build-release:                  ## Build optimized release binaries
	$(CARGO) build --release

# ─── Test ─────────────────────────────────────────────────
.PHONY: test test-verbose coverage

test:                           ## Run all tests
	$(CARGO) test

test-verbose:                   ## Run all tests with output
	$(CARGO) test -- --nocapture

coverage:                       ## Run tests with coverage report
	$(CARGO) tarpaulin --skip-clean --out stdout

# ─── Quality ──────────────────────────────────────────────
.PHONY: lint audit fmt fmt-check check

lint:                           ## Run clippy linter
	$(CARGO) clippy --all-targets -- -D warnings

audit:                          ## Check dependencies for vulnerabilities
	$(CARGO) audit

fmt:                            ## Format code
	$(CARGO) fmt

fmt-check:                      ## Check formatting (CI)
	$(CARGO) fmt -- --check

check: fmt-check lint audit test  ## Full CI check: fmt + lint + audit + test

# ─── Run ──────────────────────────────────────────────────
.PHONY: run run-debug stop

run:                            ## Start daemon in foreground
	RUST_LOG=info $(CARGO) run --bin orchestratord

run-debug:                      ## Start daemon with debug logging
	RUST_LOG=debug $(CARGO) run --bin orchestratord

stop:                           ## Stop running daemon (SIGTERM)
	@pkill orchestratord 2>/dev/null && echo "orchestratord stopped" || echo "not running"

# ─── Smoke Tests (against running daemon) ─────────────────
.PHONY: curl-health curl-version curl-info curl-all

curl-health:                    ## GET /v1/system/health
	$(CURL)/v1/system/health | python3 -m json.tool

curl-version:                   ## GET /v1/system/version
	$(CURL)/v1/system/version | python3 -m json.tool

curl-info:                      ## GET /v1/system/info
	$(CURL)/v1/system/info | python3 -m json.tool

curl-all: curl-health curl-version curl-info  ## All smoke tests

smoke:                          ## Full Solo smoke test (mock provider, end-to-end)
	./scripts/smoke-test.sh

# ─── Install ──────────────────────────────────────────────
.PHONY: install install-systemd uninstall

install: build-release          ## Install binaries to ~/.cargo/bin
	install -Dm755 target/release/orchestratord $(INSTALL_DIR)/orchestratord
	install -Dm755 target/release/orch $(INSTALL_DIR)/orch
	@echo "installed to $(INSTALL_DIR)"

install-systemd: install        ## Install binaries + systemd user units
	mkdir -p $(SYSTEMD_DIR)
	cp contrib/systemd/orchestratord.socket $(SYSTEMD_DIR)/
	cp contrib/systemd/orchestratord.service $(SYSTEMD_DIR)/
	systemctl --user daemon-reload
	systemctl --user enable orchestratord.socket
	systemctl --user start orchestratord.socket
	@echo "systemd socket activated — daemon starts on first connection"

uninstall:                      ## Remove binaries + systemd units
	systemctl --user stop orchestratord.socket orchestratord.service 2>/dev/null || true
	systemctl --user disable orchestratord.socket 2>/dev/null || true
	rm -f $(SYSTEMD_DIR)/orchestratord.socket $(SYSTEMD_DIR)/orchestratord.service
	systemctl --user daemon-reload
	rm -f $(INSTALL_DIR)/orchestratord $(INSTALL_DIR)/orch
	rm -f $(SOCKET)
	@echo "uninstalled"

# ─── Dev Tools ────────────────────────────────────────────
.PHONY: setup-tools clean

setup-tools:                    ## Install dev tools (tarpaulin, audit)
	$(CARGO) install cargo-tarpaulin cargo-audit

clean:                          ## Remove build artifacts
	$(CARGO) clean

# ─── Help ─────────────────────────────────────────────────
.PHONY: help

help:                           ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := build
