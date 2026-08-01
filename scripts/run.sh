#!/bin/bash
# One-command launcher for the Compass GUI chart app.
#
# Runs `cargo run --bin compass` in the foreground: Ctrl+C to quit, logs go
# straight to the terminal. A plain `cargo run` also works now that the
# workspace declares `default-run = "compass"` (ref #117), but the explicit
# --bin keeps this script robust regardless of workspace metadata.
#
# Usage:
#   scripts/run.sh               # launch the GUI (foreground)
#   scripts/run.sh --release     # build and run in release mode
#   scripts/run.sh -h | --help   # print this help
#
# Extra arguments are forwarded to `cargo run` (e.g. `scripts/run.sh --release`).

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

show_help() {
    sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
}

case "${1:-}" in
    -h|--help)
        show_help
        exit 0
        ;;
esac

cd "$PROJECT_ROOT"
exec cargo run --bin compass "$@"
