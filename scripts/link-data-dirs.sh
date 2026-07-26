#!/usr/bin/env bash
# Symlink data directories from /data/compass-data/ into the current directory.
# Use in worktrees to access data without re-downloading.
set -euo pipefail

DATA_HOME="${COMPASS_DATA_HOME:-/data/compass-data}"
TARGET="${1:-$(pwd)}"

link_dir() {
    local name="$1"
    local src="${DATA_HOME}/${name}"
    local dst="${TARGET}/${name}"

    if [ ! -e "$src" ]; then
        echo "WARN: source not found: $src (skipping)"
        return
    fi

    if [ -L "$dst" ] || [ -e "$dst" ]; then
        echo "INFO: already exists: $dst"
        return
    fi

    ln -s "$src" "$dst"
    echo "LINK: $dst -> $src"
}

echo "Linking data dirs from $DATA_HOME to $TARGET ..."

link_dir parquet_data
link_dir investment_data
link_dir compass_data

echo "Done."
