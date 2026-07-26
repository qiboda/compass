#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
WT_DIR="$REPO_ROOT/.worktrees"

# ── guard ────────────────────────────────────────────────────────────
if [ ! -d "$WT_DIR" ]; then
    echo "❌  No .worktrees/ directory found."
    exit 1
fi

mapfile -t available < <(ls -1 "$WT_DIR" 2>/dev/null)
if [ ${#available[@]} -eq 0 ]; then
    echo "❌  No worktrees found under .worktrees/"
    exit 1
fi

# ── no args: list available ──────────────────────────────────────────
if [ $# -eq 0 ]; then
    echo "Available worktrees:"
    for wt in "${available[@]}"; do
        echo "  $wt"
    done
    echo ""
    echo "Usage: open-worktrees.sh [name...]"
    echo "  e.g.  open-worktrees.sh data gui"
    exit 0
fi

# ── validate requested worktrees ─────────────────────────────────────
worktrees=()
for name in "$@"; do
    if [ -d "$WT_DIR/$name" ]; then
        worktrees+=("$name")
    else
        echo "⚠   '$name' not found — skipping"
    fi
done

if [ ${#worktrees[@]} -eq 0 ]; then
    echo "❌  No valid worktrees specified."
    exit 1
fi

echo "📦  Opening ${#worktrees[@]} worktrees: ${worktrees[*]}"
echo ""

# ── try kitty windows ────────────────────────────────────────────────
if command -v kitty &>/dev/null; then
    for wt in "${worktrees[@]}"; do
        echo "   ▶  $wt"
        kitty --directory="$WT_DIR/$wt" sh -c "opencode; exec bash" 2>/dev/null & disown
    done
    echo ""
    echo "✅  Launched in new kitty windows."
    exit 0
fi

# ── fallback: print commands ─────────────────────────────────────────
echo "Run these in separate terminals:"
echo ""
for wt in "${worktrees[@]}"; do
    echo "  opencode $WT_DIR/$wt"
done
echo ""