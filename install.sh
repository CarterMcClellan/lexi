#!/usr/bin/env bash
# Build and install the `lexi` CLI from this checkout (into ~/.cargo/bin).
# Prebuilt binaries are also attached to each GitHub Release.
set -euo pipefail

echo "Installing the Lexi CLI..."

if ! command -v cargo &>/dev/null; then
    echo "Error: cargo (Rust) is required — install it from https://rustup.rs" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cargo install --path "$SCRIPT_DIR/lexi-cli-rs" --quiet

echo "Lexi CLI installed. Run 'lexi' for the interactive shell, or 'lexi help'."
echo "If you had the old Python CLI installed, remove it: pip uninstall lexi-cli"
echo "Desktop app: see lexi-desktop/README.md (prebuilt installers on GitHub Releases)."
