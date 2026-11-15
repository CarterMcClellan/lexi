# Lexi CLI installer for Windows
$ErrorActionPreference = "Stop"

Write-Host "Installing Lexi CLI..."

# Check Python
try {
    $pythonVersion = python --version 2>&1 | Select-String -Pattern '(\d+\.\d+)' | ForEach-Object { $_.Matches[0].Value }
    if ([version]$pythonVersion -lt [version]"3.9") {
        Write-Error "Python >= 3.9 is required (found $pythonVersion)."
        exit 1
    }
} catch {
    Write-Error "Python is required but not found."
    exit 1
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Install CLI
Push-Location "$scriptDir\lexi-cli"
pip install -e . --quiet
Pop-Location

Write-Host "Lexi CLI installed. Run 'lexi' to start."

# The desktop app (lexi-desktop) is a native Tauri shell over the same engine.
# Grab a prebuilt installer from the GitHub Releases page, or build locally:
#   cargo install tauri-cli --version '^2'
#   cd lexi-desktop; cargo tauri build
Write-Host "Desktop app: see lexi-desktop\README.md (prebuilt installers on GitHub Releases)."
