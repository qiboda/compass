#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
WT_DIR="$REPO_ROOT/.worktrees"

# ── guard ────────────────────────────────────────────────────────────
if [ ! -d "$WT_DIR" ]; then
    echo "❌  No .worktrees/ directory found."
    exit 1
fi

mapfile -t worktrees < <(ls -1 "$WT_DIR" 2>/dev/null)
if [ ${#worktrees[@]} -eq 0 ]; then
    echo "❌  No worktrees found under .worktrees/"
    exit 1
fi

echo "📦  Found ${#worktrees[@]} worktrees: ${worktrees[*]}"
echo ""

# ── try kitty windows ────────────────────────────────────────────────
if command -v kitty &>/dev/null; then
    for wt in "${worktrees[@]}"; do
        echo "   ▶  $wt"
        kitty --directory="$WT_DIR/$wt" opencode 2>/dev/null & disown
    done
    echo ""
    echo "✅  All worktrees launched in new kitty windows."
    exit 0
fi

# ── fallback: print commands ─────────────────────────────────────────
echo "Run these in separate terminals:"
echo ""
for wt in "${worktrees[@]}"; do
    echo "  opencode $WT_DIR/$wt"
done
echo ""