#!/usr/bin/env bash
# axon-ledger install script
# Usage: curl -sSf https://raw.githubusercontent.com/cklaus/axon/main/crates/axon-ledger/install.sh | sh
set -e

INSTALL_DIR="${AXON_LEDGER_INSTALL_DIR:-$HOME/.local/bin}"
REPO="https://github.com/cklaus/axon"

echo "Installing axon-ledger..."

# Check for Rust
if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust not found. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    source "$HOME/.cargo/env"
fi

# Install
cargo install --git "$REPO" axon-ledger --root "$HOME/.cargo"

echo ""
echo "axon-ledger installed to $(command -v axon-ledger 2>/dev/null || echo "$HOME/.cargo/bin/axon-ledger")"
echo ""
echo "Quick start:"
echo "  axon-ledger ingest git --repo /path/to/your/repo"
echo "  axon-ledger ingest session-dir ~/.claude/projects/<your-repo>/"
echo "  axon-ledger ingest edges"
echo "  axon-ledger why <commit-sha>"
