---
name: worktree
description: Manage git worktrees for persistent functional zone isolation. Use when creating, listing, or removing worktrees under .worktrees/. Trigger when user says "worktree", "切一个worktree", or asks to isolate a functional area.
---

# Worktree

Git worktrees provide isolated working directories for different functional
zones of the project. Each worktree is a **persistent workspace** for a
distinct area — a single worktree can host multiple features over its lifetime.
Not deleted after a feature ships.

## Convention

All worktrees live under `.worktrees/<name>/` (gitignored). They are for
**functional zone division**, not one-per-feature.

```
.worktrees/
├── custom-dolt/       # Dolt 扩展相关的一切工作
├── egui-mobius/       # egui_mobius 迁移及所有后续相关改动
└── data-pipeline/     # 数据管线优化、新增Provider
```

| Worktree path | Purpose |
|---|---|
| `.worktrees/<name>` | Persistent functional zone — hosts multiple features |
| `.worktrees/<name>` | Long-lived: created once, kept indefinitely |

## Commands

### Create

```bash
git worktree add -b feature/<name> .worktrees/<name> master
```

**Rules**:
- `<name>` = kebab-case slug matching the functional area (e.g. `custom-dolt`, `egui-mobius`)
- Based on `master` — push directly per trunk-based convention
- Never create worktrees outside `.worktrees/`

**Post-Creation (MANDATORY)** — after every `git worktree add`:

1. **Symlink local data directories** from the main repo into the worktree.
   These are gitignored and won't exist in the worktree otherwise:
   ```bash
   # From repo root — create symlinks to shared data
   ln -s "$PWD/investment_data" .worktrees/<name>/investment_data
   ln -s "$PWD/parquet_data"    .worktrees/<name>/parquet_data
   ```
   Only create symlinks for directories that actually exist in the main repo.

2. **Run `/handoff`** to save the current conversation context:
   - The handoff file goes to `.worktrees/<name>/.omo/handoff.md`
   - This captures: what was decided, what's next, relevant design context
   - Use `write` tool to create the handoff file if `/handoff` command is unavailable

3. **Tell the user** to open a new opencode session in the worktree:
   ```
   Worktree ready. Continue in a new terminal:
       cd .worktrees/<name> && opencode
   ```
   The new opencode session will automatically read `.omo/handoff.md` for context.

4. **Current session stays in master** — do NOT `cd` into the worktree in the current session.

### List

```bash
git worktree list
```

### Remove

Only when the functional zone is permanently retired:

```bash
# Remove worktree + its branch
git worktree remove .worktrees/<name> --force
git branch -D <branch-name>
```

### Clean orphans

Remove directories under `.worktrees/` that are not active git worktrees:

```bash
for d in .worktrees/*/; do
  name=$(basename "$d")
  if ! git worktree list | grep -q ".worktrees/$name"; then
    echo "orphan: $d"
    rm -rf "$d"
  fi
done
```

## Integration with compass-workflow

When the `compass-workflow` skill is also loaded:
- Each feature within a worktree still goes through the gate (issue → plan → tests → docs)
- Quality gates (`cargo test`, `cargo clippy`, `cargo fmt`) run inside the worktree
- Push directly to master per trunk-based convention

## Example: creating a Dolt extension worktree

```bash
# User: "切一个dolt扩展的worktree"
# → Fire this skill, then:
git worktree add -b feature/custom-dolt .worktrees/custom-dolt master
```

**Then** (same turn, immediately after `git worktree add` succeeds):

1. Symlink data dirs: `ln -s "$PWD/investment_data" .worktrees/custom-dolt/investment_data`
2. Run `/handoff` → writes `.worktrees/custom-dolt/.omo/handoff.md` with current context
3. Tell user: `cd .worktrees/custom-dolt && opencode`

The worktree persists — all Dolt-related features are developed here.
