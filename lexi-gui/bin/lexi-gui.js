#!/usr/bin/env node
/**
 * CLI entrypoint for the Lexi GUI server.
 *   lexi-gui          — start the server (kills existing instance first)
 *   lexi-gui --kill   — kill the running server and exit
 */

const { execSync } = require('child_process');

const PORT = process.env.PORT || 3000;

function findPids() {
  try {
    return execSync(`lsof -ti:${PORT}`, { encoding: 'utf8' }).trim();
  } catch {
    return '';
  }
}

function killGui() {
  let pids = findPids();
  if (!pids) {
    console.log(`No Lexi GUI running on port ${PORT}.`);
    return;
  }
  console.log(`Killing Lexi GUI on port ${PORT} (PID ${pids.replace(/\n/g, ', ')})...`);
  try { execSync(`kill ${pids.replace(/\n/g, ' ')}`); } catch {}

  // Wait for clean shutdown
  for (let i = 0; i < 5; i++) {
    pids = findPids();
    if (!pids) break;
    execSync('sleep 0.5');
  }

  // Force kill if still running
  pids = findPids();
  if (pids) {
    console.log('Force killing remaining processes...');
    try { execSync(`kill -9 ${pids.replace(/\n/g, ' ')}`); } catch {}
  }
  console.log('Stopped.');
}

const arg = process.argv[2];
if (arg === '--kill' || arg === '-k') {
  killGui();
  process.exit(0);
}

// Kill any existing instance before starting
killGui();

require('../src/server/index');
