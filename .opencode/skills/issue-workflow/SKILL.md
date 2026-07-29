---
name: issue-workflow
description: Manages the complete issue lifecycle — single-issue creation, epic/sub-issue decomposition, batch processing, and batch close. Use for any issue creation or management in this repo.
---

# Issue Workflow — Issue Lifecycle Agent

## Role

Manage the complete GitHub issue lifecycle for the compass project. Handle
single-issue creation (delegated from compass-workflow gate step 1), epic
decomposition with GitHub native sub-issues, batch execution tracking via
plan files, and batch close after PR merge.

## Trigger

- `/issue-workflow` slash command (user-initiated)
- compass-workflow pre-implementation gate step 1 (automated via `→ Invoke /issue-workflow`)

## Modes

This skill operates in two modes depending on context:

| Mode | Trigger | Flow |
|---|---|---|
| **Single issue** | compass-workflow gate step 1 with a single-issue requirement | Create one issue → show URL → done |
| **Epic + sub-issues** | `/ulw-plan` produces 2+ task waves with independent deliverables | Create epic → batch-create sub-issues → track in plan → batch close |

## Workflow

### Phase 0: Determine mode

1. Read the grill-me summary and `/ulw-plan` output (`.omo/plans/<name>.md`)
2. If plan has 2+ task waves with independent deliverables → **Epic mode**
3. Otherwise → **Single issue mode**

### Phase 1A: Single issue mode

1. Create issue using appropriate template:
   ```sh
   gh issue create \
     --title "<title>" \
     --body-file /tmp/issue-body.md \
     --label "A-<area>,C-<category>"
   ```
2. Verify with `gh issue view <N>`
3. Record issue number in `.omo/plans/<name>.md` if applicable
4. Return issue URL to calling workflow

### Phase 1B: Epic mode — Create epic

1. Create the **epic** (parent issue) first:
   ```sh
   gh issue create \
     --title "<epic title>" \
     --body-file /tmp/epic-body.md \
     --label "A-<area>,C-Feature"
   ```
   Epic body includes: motivation, scope overview, link to `.omo/plans/<epic>.md`.

2. Verify with `gh issue view <epic-N>`

### Phase 1B: Epic mode — Create sub-issues

1. From `.omo/plans/<epic>.md`, extract each task wave item that is an
   independent deliverable.

2. For each sub-issue, create with `--parent` flag:
   ```sh
   gh issue create \
     --title "<sub-issue title>" \
     --body-file /tmp/sub-issue-body.md \
     --label "A-<area>,C-Feature" \
     --parent <epic-N>
   ```

3. Sub-issue body template:
   ```markdown
   > **Parent**: #<epic-N>
   > **Plan**: .omo/plans/<epic-name>.md
   > **Batch**: <N>
   > **Depends on**: #<sub-X>, #<sub-Y> (or "—" if none)

   ## 描述
   <task description from plan>

   ## 验收标准
   <acceptance criteria from plan>
   ```

4. After creating all sub-issues, update `.omo/plans/<epic>.md`:
   - Fill the `Issue` column of each task row with the sub-issue number
   - Set initial status to `pending`
   - Record dependency relations

### Phase 2: Batch tracking

The `.omo/plans/<epic>.md` file is the canonical tracking document. Its `## Tasks`
section uses a Markdown table:

```markdown
## Tasks

### Batch 1
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #12 | Implement XYZ | — |
| pending | #13 | Implement ABC | #12 |

### Batch 2
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #14 | Integration tests | #12, #13 |
```

Status values: `pending` | `in_progress` | `done`

**Batch switching rule**: When the agent completes all sub-issues in the current
batch, it MUST:
1. Update all completed sub-issue statuses to `done`
2. Report batch completion to the user (list: which sub-issues done, which PRs merged)
3. Wait for user confirmation before starting the next batch
4. On confirmation, mark next batch sub-issues as `in_progress` and proceed

### Phase 3: New sub-issues during execution

If a new work item is discovered during implementation:
1. Add a task row to the plan file's appropriate batch (or a new batch)
2. Create the sub-issue:
   ```sh
   gh issue create --title "..." --body-file /tmp/new-sub.md --label "..." --parent <epic-N>
   ```
3. Fill the Issue column in the plan table
4. Re-evaluate DAG dependencies — items blocked by the new sub-issue are
   NOT moved to `in_progress` until it completes

### Phase 4: Batch close

After the PR containing all sub-issue commits is merged to `master`:

1. Close all sub-issues:
   ```sh
   gh issue close <sub-N1> <sub-N2> <sub-N3>
   ```

2. Record the PR on each sub-issue:
   ```sh
   gh issue comment <sub-N> --body "Fixed by #<PR-N>"
   ```

3. Close the epic:
   ```sh
   gh issue close <epic-N>
   ```

4. Record summary on epic:
   ```sh
   gh issue comment <epic-N> --body "All sub-issues completed:
   - #<sub-N1>: <title>
   - #<sub-N2>: <title>
   Fixed by #<PR-N>"
   ```

## Output Format

### Single issue mode
```
## Issue: #<N> — <title>
URL: https://github.com/qiboda/compass/issues/<N>
Labels: <labels>
```

### Epic mode — creation
```
## Epic: #<epic-N> — <epic-title>
URL: https://github.com/qiboda/compass/issues/<epic-N>

### Sub-issues (Batch 1)
| # | Title | Depends On |
|---|-------|------------|
| #<sub-N1> | <title> | — |
| #<sub-N2> | <title> | #<sub-N1> |

### Sub-issues (Batch 2)
| # | Title | Depends On |
|---|-------|------------|
| #<sub-N3> | <title> | #<sub-N1>, #<sub-N2> |
```

### Epic mode — batch completion
```
## Batch <N> Complete
Epic: #<epic-N>

Completed:
- #<sub-N1> <title> — merged in PR #<PR-N>
- #<sub-N2> <title> — merged in PR #<PR-N>

Pending (Batch <N+1>):
- #<sub-N3> <title> — blocked by: —

Proceed to next batch? (confirm to continue)
```

### Epic mode — final close
```
## Epic #<epic-N> Complete
All sub-issues closed. Epic closed.
PR: #<PR-N>
```

## Edge Cases

| Scenario | Behavior |
|---|---|
| Plan has 2+ waves but only one deliverable | Use single issue mode (waves ≠ sub-issues) |
| Plan has no task waves | Use single issue mode |
| Sub-issue creation fails (network/GitHub) | Retry once; if still failing, report error with the failing `gh` command |
| Issue number > 999 (GitHub auto-numbering) | Accept — GitHub numbers are auto-incrementing, no fixed range |
| User wants to add sub-issue after batch started | Phase 3 — add to plan, create issue, re-evaluate DAG |
| DAG has cycle | Report error — detect by walking Depends On chain, halt before any issue creation |
| Epic body exceeds GitHub limit | Split scope overview to plan file; epic body references plan for details |
| User says "skip batch confirmation, auto-proceed" | Respect — switch to auto mode; still report each batch completion |
| Pre-existing issue needs to become sub-issue | `gh issue edit <parent> --add-sub-issue <existing-N>` |
| Sub-issue from different repo | GitHub supports cross-repo sub-issues — use full URL: `gh issue create --parent https://github.com/owner/repo/issues/N` |

## Must NOT

- **Auto-close issues before PR merge** — close only after merge to `master`
- **Skip batch confirmation** — unless user explicitly requests auto-mode
- **Delete plan file entries** — only update status; never remove rows
- **Create sub-issues without parent** — always use `--parent` flag
- **Use `fixes #N` or `closes #N` in commits** — use `ref #N` only (manual close via `gh issue close`)
- **Modify compass-workflow skill** — issue-workflow is a peer, not a replacement
- **Create issues for non-feature/bugfix work** — docs, lint, typo fixes skip issue creation entirely

## Collaboration with compass-workflow

1. compass-workflow gate step 1 says `→ Invoke /issue-workflow to create/manage issues`
2. At gate step 1, compass-workflow calls issue-workflow to create the issue(s)
3. Issue-workflow determines single vs epic mode, creates issues, returns results
4. compass-workflow continues with gate steps 2-4b for each sub-issue independently
5. After all sub-issues complete → PR merge → issue-workflow handles batch close
6. compass-workflow handles commit, test, review, and push (unchanged)

## Reference

- `AGENTS.md` — full project workflow and gate rules
- `kb/dev/process.md` — development process documentation
- `kb/github/labels.md` — label taxonomy
- `.omo/plans/` — plan files with task tables and DAG dependencies

## Issue Body Template (Sub-Issue)

When creating a sub-issue, the body file (`/tmp/sub-issue-body.md`) follows
this template:

```markdown
> **Parent**: #<epic-N>
> **Plan**: .omo/plans/<epic-name>.md
> **Batch**: <N>
> **Depends on**: #<sub-X> (or "—" if none)

## 描述
<task description>

## 验收标准
<acceptance criteria>
```

## Plan File Task Table Format

The canonical format for `.omo/plans/<epic>.md` task tables:

```markdown
### Batch <N>
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #<N> | <one-line description> | — |
| in_progress | #<N> | <one-line description> | #<X> |
| done | #<N> | <one-line description> | #<X>, #<Y> |
```

- `Status`: one of `pending`, `in_progress`, `done`
- `Issue`: GitHub issue number (with `#` prefix), or empty if not yet created
- `Task`: one-line description from plan
- `Depends On`: comma-separated issue numbers or `—` if none

Only ONE task in a batch should be `in_progress` at a time per worktree.
Independent tasks in the same batch (no mutual dependencies) can be
`in_progress` simultaneously when parallel subagents are used.
