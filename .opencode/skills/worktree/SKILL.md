---
name: worktree
description: Manage git worktrees for isolated feature development. Use when creating, listing, or removing worktrees under .worktrees/. Trigger when user says "worktree", "切一个worktree", or asks to isolate work on a branch.
---

# Worktree

Git worktrees provide isolated working directories for parallel or experimental
development without `git stash` / branch-switching overhead.

## Convention

All worktrees live under `.worktrees/<name>/` (gitignored).

```
.worktrees/
├── compass-mobius/    # → branch: feature/compass-mobius
├── egui-upgrade/      # → branch: feature/egui-upgrade
└── fix-auth/          # → branch: fix/auth
```

| Worktree path | Branch name | Rule |
|---|---|---|
| `.worktrees/<name>` | `feature/<name>` | Default for feature work |
| `.worktrees/<name>` | `fix/<name>` | For bugfix isolation |
| `.worktrees/<name>` | `<name>` | When branch name already includes prefix |

## Commands

### Create

```bash
# Feature branch (most common)
git worktree add -b feature/<name> .worktrees/<name> <base-ref>

# With explicit branch prefix
git worktree add -b <full-branch> .worktrees/<dir-name> <base-ref>
```

**Rules**:
- `<name>` = kebab-case slug matching the work's purpose (e.g. `egui-mobius`, `fix-download`)
- `<base-ref>` defaults to `master` (HEAD)
- Directory name = sanitized branch name (drop the `feature/` or `fix/` prefix, keep slashes as hyphens)
- Never create worktrees outside `.worktrees/`

**Post-Creation (MANDATORY)** — after every `git worktree add`:

1. **Run `/handoff`** to save the current conversation context:
   - The handoff file goes to `.worktrees/<name>/.omo/handoff.md`
   - This captures: what was decided, what's next, relevant design context
   - Use `write` tool to create the handoff file if `/handoff` command is unavailable

2. **Tell the user** to open a new opencode session in the worktree:
   ```
   Worktree ready. Continue in a new terminal:
       cd .worktrees/<name> && opencode
   ```
   The new opencode session will automatically read `.omo/handoff.md` for context.

3. **Current session stays in master** — do NOT `cd` into the worktree in the current session.

### List

```bash
git worktree list
```

### Remove

```bash
# Remove worktree + its branch
git worktree remove .worktrees/<name> --force
git branch -D <branch-name>

# Safer: check branch first, then remove
git worktree list | grep <name>
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

## When to use

| Use worktree | Don't use worktree |
|---|---|
| Risky / experimental changes | Trivial fixes |
| Multi-day features | Single-commit changes |
| Library migration / API upgrade | Documentation updates |
| Parallel features without stash | Typo / lint fixes |

## Integration with compass-workflow

When the `compass-workflow` skill is also loaded:
- Worktrees replace the branch-switching step — you work in the isolated directory
- The pre-implementation gate still applies (issue → plan → tests → docs)
- Quality gates (`cargo test`, `cargo clippy`, `cargo fmt`) run inside the worktree

## Example: creating a library migration worktree

```bash
# User: "切一个使用egui_mobius的worktree"
# → Fire this skill, then:
git worktree add -b feature/egui-mobius .worktrees/egui-mobius master
```

**Then** (same turn, immediately after `git worktree add` succeeds):

1. Run `/handoff` → writes `.worktrees/egui-mobius/.omo/handoff.md` with current context
2. Tell user: `cd .worktrees/egui-mobius && opencode`

**When done**, merge back and clean up:
```bash
git worktree remove .worktrees/egui-mobius --force
git branch -D feature/egui-mobius
```
