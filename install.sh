#!/usr/bin/env bash
set -euo pipefail

echo "Installing Lexi CLI..."

# Check Python version
if ! command -v python3 &>/dev/null; then
    echo "Error: python3 is required but not found." >&2
    exit 1
fi

PYTHON_VERSION=$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')
REQUIRED="3.9"
if [ "$(printf '%s\n' "$REQUIRED" "$PYTHON_VERSION" | sort -V | head -n1)" != "$REQUIRED" ]; then
    echo "Error: Python >= $REQUIRED is required (found $PYTHON_VERSION)." >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Install CLI in editable mode
cd "$SCRIPT_DIR/lexi-cli"
pip install -e . --quiet

echo "Lexi CLI installed. Run 'lexi' to start."

# The desktop app (lexi-desktop) is a native Tauri shell over the same engine.
# Grab a prebuilt installer from the GitHub Releases page, or build locally:
#   cargo install tauri-cli --version '^2'
#   cd lexi-desktop && cargo tauri build
echo "Desktop app: see lexi-desktop/README.md (prebuilt installers on GitHub Releases)."
