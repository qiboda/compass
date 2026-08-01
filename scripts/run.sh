#!/bin/bash
# One-command launcher for the Compass GUI chart app.
#
# Runs `cargo run --bin compass` in the foreground: Ctrl+C to quit, logs go
# straight to the terminal. The explicit --bin is required because this is a
# virtual workspace (root Cargo.toml has no [package], so default-run is not
# available); --bin also keeps the script robust to future workspace changes.
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
    awk '/^# Usage:/ {flag=1} flag && /^#$/ {next} flag && /^$/ {exit} flag {sub(/^# ?/, ""); print}' "$0"
}

case "${1:-}" in
    -h|--help)
        show_help
        exit 0
        ;;
esac

cd "$PROJECT_ROOT"
exec cargo run --bin compass "$@"
