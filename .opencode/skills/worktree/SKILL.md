---
name: worktree
description: Manage git worktrees for PR development. Use when creating, listing, or removing worktrees under .worktrees/. Trigger when user says "worktree", "切一个worktree", or needs a PR workspace.
---

# Worktree

Git worktrees provide isolated working directories for PR development.
Each worktree is a **transient workspace** for a single PR — created
when development starts, removed after the PR is merged.

## Convention

All worktrees live under `.worktrees/<name>/` (gitignored). Branch naming: `feat/<short-description>` or `fix/<short-description>`.

```
.worktrees/
├── fix-candle-rendering/   # PR fixing candle rendering
└── add-sector-filter/      # PR adding sector filter
```

| Worktree path | Purpose |
|---|---|
| `.worktrees/<name>` | Transient PR workspace — one per PR |
| `.worktrees/<name>` | Short-lived: created for PR, removed after merge |

## Commands

### Create

```bash
git worktree add -b feat/<name> .worktrees/<name> master
```

**Rules**:
- `<name>` = kebab-case slug matching the PR (e.g. `fix-candle-rendering`, `add-sector-filter`)
- Based on `master` — PR merges back to master
- Never create worktrees outside `.worktrees/`

**Post-Creation (MANDATORY)** — after every `git worktree add`:

1. **Run `/handoff`** to save the current conversation context:
   - The handoff file goes to `.worktrees/<name>/.omo/handoff.md`
   - This captures: what was decided, what's next, relevant design context
   - Use `write` tool to create the handoff file if `/handoff` command is unavailable

2. **⚠️ 先解绑当前 opencode session（MANDATORY）** — before opening a new opencode in the worktree:
   - **Why**: opencode recognizes the worktree directory as the *same project* as master
     (same `project_id` in `~/.local/share/opencode/opencode.db` via `git_worktree`
     association). The current opencode instance (running in master) still *binds* that
     project's session, so a new `opencode` launched in the worktree fails to start.
   - **How**: release the current session binding first — e.g. exit the current opencode
     instance (or stop/quit its session) so the project is unbound, *then* launch the new
     opencode in the worktree.
   - Do NOT skip this step. Opening the worktree opencode while the master session is still
     bound will fail.

3. **Tell the user** to open a new opencode session in the worktree (only after step 2):
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

### Remove (after PR merge)

After the PR is merged, clean up:

```bash
# Remove worktree + its branch
git worktree remove .worktrees/<name> --force
git branch -D feat/<name>
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
- Each PR within a worktree goes through the gate (issue → plan → tests → docs)
- Quality gates (`cargo test`, `cargo clippy`, `cargo fmt`) run inside the worktree
- Push to the PR branch (`feat/<name>`), create PR, merge via GitHub

## Example: creating a PR worktree

```bash
# User: "切一个fix candle的worktree"
# → Fire this skill, then:
git worktree add -b feat/fix-candle-rendering .worktrees/fix-candle-rendering master
```

**Then** (same turn, immediately after `git worktree add` succeeds):

1. Run `/handoff` → writes `.worktrees/fix-candle-rendering/.omo/handoff.md` with current context
2. **解绑当前 opencode session**（见上方 Post-Creation step 2，MANDATORY）
3. Tell user: `cd .worktrees/fix-candle-rendering && opencode`

The worktree is transient — cleaned up after PR merge.
