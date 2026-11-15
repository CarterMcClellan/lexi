#!/usr/bin/env node

/**
 * Script to prepare CLI and Python runtime for Electron bundling
 */

const fs = require('fs');
const path = require('path');
const os = require('os');

const platform = os.platform();
const arch = os.arch();

console.log(`Preparing CLI and Python for ${platform}-${arch}...`);

const resourcesDir = path.join(__dirname, '..', 'resources');
const cliResourceDir = path.join(resourcesDir, 'cli');
const pythonResourceDir = path.join(resourcesDir, 'python');

// Ensure directories exist
if (!fs.existsSync(cliResourceDir)) {
  fs.mkdirSync(cliResourceDir, { recursive: true });
}
if (!fs.existsSync(pythonResourceDir)) {
  fs.mkdirSync(pythonResourceDir, { recursive: true });
}

/**
 * Copy CLI to resources directory
 */
function prepareCli() {
  console.log('Preparing CLI...');

  const cliSrcDir = path.join(__dirname, '..', '..', 'lexi-cli');
  const cliPyProjectPath = path.join(cliSrcDir, 'pyproject.toml');

  if (!fs.existsSync(cliPyProjectPath)) {
    console.error('CLI not found at', cliSrcDir);
    process.exit(1);
  }

  // Copy Python CLI source
  const cliSrcPath = path.join(cliSrcDir, 'src');
  const cliDestPath = path.join(cliResourceDir, 'src');

  if (fs.existsSync(cliDestPath)) {
    fs.rmSync(cliDestPath, { recursive: true });
  }

  // Copy CLI source files
  fs.cpSync(cliSrcPath, cliDestPath, { recursive: true });

  // Copy pyproject.toml
  fs.copyFileSync(
    path.join(cliSrcDir, 'pyproject.toml'),
    path.join(cliResourceDir, 'pyproject.toml')
  );

  // Create CLI launcher script
  const isWindows = platform === 'win32';
  const launcherName = isWindows ? 'lexi.bat' : 'lexi';
  const launcherPath = path.join(cliResourceDir, launcherName);

  if (isWindows) {
    fs.writeFileSync(launcherPath,
      '@echo off\n' +
      '"%~dp0..\\python\\python.exe" -m lexi_cli %*\n'
    );
  } else {
    fs.writeFileSync(launcherPath,
      '#!/bin/bash\n' +
      'exec "$(dirname "$0")/../python/bin/python" -m lexi_cli "$@"\n'
    );
    fs.chmodSync(launcherPath, '755');
  }

  console.log('CLI prepared successfully');
}

/**
 * Download and prepare embedded Python
 */
function preparePython() {
  console.log('Preparing embedded Python...');

  const readmePath = path.join(pythonResourceDir, 'README.md');

  fs.writeFileSync(readmePath, `# Python Runtime Setup

For production builds, this directory should contain an embedded Python runtime.

## Manual Setup:

### Windows:
1. Download Python embed package from https://python.org/downloads/
2. Extract to resources/python/
3. Install pip: python get-pip.py
4. Install dependencies: pip install pyyaml rich

### macOS/Linux:
1. Use pyenv or similar to create standalone Python
2. Copy to resources/python/
3. Install dependencies in embedded environment

## Automated Setup (Future):
This script will be enhanced to automatically download and prepare
the embedded Python runtime for each target platform.

Current platform: ${platform}-${arch}
`);

  console.log('Python preparation documented. Manual setup required for production builds.');
}

/**
 * Create shell integration scripts
 */
function createShellIntegration() {
  console.log('Creating shell integration...');

  const binDir = path.join(cliResourceDir, 'bin');
  if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir, { recursive: true });
  }

  const isWindows = platform === 'win32';

  if (isWindows) {
    fs.writeFileSync(path.join(binDir, 'lexi-gui.bat'),
      '@echo off\n' +
      'start "" "%LOCALAPPDATA%\\Programs\\Lexi\\Lexi.exe"\n'
    );
  } else {
    fs.writeFileSync(path.join(binDir, 'lexi-gui'),
      '#!/bin/bash\n' +
      'open -a "Lexi" 2>/dev/null || /Applications/Lexi.app/Contents/MacOS/Lexi\n'
    );
    fs.chmodSync(path.join(binDir, 'lexi-gui'), '755');
  }

  console.log('Shell integration created');
}

// Main execution
try {
  prepareCli();
  preparePython();
  createShellIntegration();

  console.log('\nPreparation complete!');
  console.log('\nNext steps:');
  console.log('1. Set up embedded Python in resources/python/ (see README)');
  console.log('2. Run: npm run build');
  console.log('3. Install generated package');
  console.log('4. Use: lexi (CLI) and lexi-gui (desktop app)');

} catch (error) {
  console.error('Preparation failed:', error.message);
  process.exit(1);
}
