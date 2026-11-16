# Build and install the `lexi` CLI from this checkout (into ~\.cargo\bin).
# Prebuilt binaries are also attached to each GitHub Release.
$ErrorActionPreference = "Stop"

Write-Host "Installing the Lexi CLI..."

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo (Rust) is required - install it from https://rustup.rs"
    exit 1
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
cargo install --path "$scriptDir\lexi-cli-rs" --quiet

Write-Host "Lexi CLI installed. Run 'lexi' for the interactive shell, or 'lexi help'."
Write-Host "If you had the old Python CLI installed, remove it: pip uninstall lexi-cli"

# The desktop app (lexi-desktop) is a native Tauri shell over the same engine.
# Grab a prebuilt installer from the GitHub Releases page, or build locally:
#   cargo install tauri-cli --version '^2'
#   cd lexi-desktop; cargo tauri build
Write-Host "Desktop app: see lexi-desktop\README.md (prebuilt installers on GitHub Releases)."
