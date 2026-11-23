#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# demo.sh — Urban Mobility Engine — Madrid
#
# Usage:
#   ./demo.sh              → live with the interactive TUI
#   ./demo.sh --dashboard  → live CLI + HTTP server with a live dashboard
#   ./demo.sh --ticks N    → live CLI for N ticks + dashboard (default 3)
# ─────────────────────────────────────────────────────────────────────────────
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT=8080
TICKS=3

cd "$SCRIPT_DIR"

# Parse arguments
MODE="tui"
for arg in "$@"; do
    case $arg in
        --dashboard) MODE="dashboard" ;;
        --ticks)     shift; TICKS="$1" ;;
        --ticks=*)   TICKS="${arg#--ticks=}" ;;
    esac
done

# Build if the binary does not exist
if [ ! -f "target/release/tfg" ]; then
    echo "Building Rust engine..."
    cargo build --release --quiet
    echo "Build complete."
    echo ""
fi

if [ "$MODE" = "tui" ]; then
    # ── Interactive TUI mode (default) ───────────────────────────────────────
    ./target/release/tfg

else
    # ── Dashboard mode: live CLI + HTTP server ───────────────────────────────
    echo ""
    echo "Urban Mobility Engine — Madrid"
    echo "Mode: Live CLI + Dashboard"
    echo ""

    # Kill any previous process on the port
    lsof -ti tcp:$PORT | xargs kill -9 2>/dev/null || true

    # Start the HTTP server from demo/
    cd demo/
    python3 -m http.server $PORT --bind 127.0.0.1 &>/dev/null &
    SERVER_PID=$!
    cd "$SCRIPT_DIR"

    sleep 0.5
    echo "HTTP server running at http://localhost:${PORT}/dashboard.html"
    echo ""

    # Open the browser
    open "http://localhost:${PORT}/dashboard.html" 2>/dev/null || true

    echo "Running live engine (${TICKS} ticks, output -> demo/live_output.json)..."
    echo "The dashboard refreshes automatically every 30s."
    echo ""
    echo "Press Ctrl+C to stop."
    echo ""

    # Launch the live engine — writes demo/live_output.json after each tick
    trap "kill $SERVER_PID 2>/dev/null; echo ''; echo 'Server and engine stopped.'; exit 0" INT TERM
    ./target/release/tfg live --ticks "$TICKS" --output demo/

    # Keep the server alive after the ticks finish
    echo ""
    echo "Engine finished. Server is still running. Press Ctrl+C to close."
    wait $SERVER_PID
fi
