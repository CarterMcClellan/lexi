#!/usr/bin/env bash
# Launch the Lexi GUI server
#   launch-gui.sh          — start the GUI (kills existing instance first)
#   launch-gui.sh --kill   — kill the running GUI and exit
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

PORT="${PORT:-3000}"

kill_gui() {
    local pids
    pids=$(lsof -ti:"$PORT" 2>/dev/null || true)
    if [ -n "$pids" ]; then
        echo "Killing Lexi GUI on port $PORT (PID $pids)..."
        echo "$pids" | xargs kill 2>/dev/null || true
        # Wait briefly for clean shutdown
        for _ in 1 2 3 4 5; do
            pids=$(lsof -ti:"$PORT" 2>/dev/null || true)
            [ -z "$pids" ] && break
            sleep 0.5
        done
        # Force kill if still running
        pids=$(lsof -ti:"$PORT" 2>/dev/null || true)
        if [ -n "$pids" ]; then
            echo "Force killing remaining processes..."
            echo "$pids" | xargs kill -9 2>/dev/null || true
        fi
        echo "Stopped."
    else
        echo "No Lexi GUI running on port $PORT."
    fi
}

if [ "${1:-}" = "--kill" ] || [ "${1:-}" = "-k" ]; then
    kill_gui
    exit 0
fi

# Kill any existing instance before starting
kill_gui

if [ ! -d node_modules ]; then
    echo "Installing dependencies..."
    npm install
fi

echo "Starting Lexi GUI on http://localhost:$PORT"
node src/server/index.js
