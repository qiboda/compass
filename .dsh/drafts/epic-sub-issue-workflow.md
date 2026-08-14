---
slug: epic-sub-issue-workflow
status: awaiting-approval
intent: clear
review_required: false
pending-action: write .omo/plans/epic-sub-issue-workflow.md
approach: Create `issue-workflow` skill to manage all issue lifecycle (single + epic/sub-issue), then update AGENTS.md, compass-workflow, process.md, and issue templates to delegate issue management to the new skill.
---

# Draft: epic-sub-issue-workflow

> Parent issue: [#67](https://github.com/qiboda/compass/issues/67)
> Grill-me: completed 2026-07-30, 29 decisions locked

## Components (topology ledger)

| id | outcome (one line) | status | evidence path |
|----|--------------------|--------|---------------|
| issue-workflow-skill | New `.opencode/skills/issue-workflow/SKILL.md` — manages all issue lifecycle (create, split, batch, close) | active | `.opencode/skills/issue-workflow/SKILL.md` |
| agents-md-update | `AGENTS.md` — add epic/sub-issue workflow section, update GATE, issue lifecycle | active | `AGENTS.md` |
| compass-workflow-update | `.opencode/skills/compass-workflow/SKILL.md` — GATE step 1 delegates to issue-workflow; remove direct issue rules | active | `.opencode/skills/compass-workflow/SKILL.md` |
| process-md-update | `kb/dev/process.md` — add epic/sub-issue workflow, batch processing, multi-sub-issue PR | active | `kb/dev/process.md` |
| issue-templates-update | `.github/ISSUE_TEMPLATE/` — add sub-issue metadata fields (parent, plan, batch, depends on) | deferred | `.github/ISSUE_TEMPLATE/feature_request.md` |

## Open assumptions (announced defaults)

| assumption | adopted default | rationale | reversible? |
|------------|-----------------|-----------|-------------|
| GitHub sub-issue native support available | Use `gh issue create --parent` | GitHub GA since 2025-04 | No — requires GitHub org to have sub-issues enabled |
| No new labels needed for epic | Use `has:sub-issue` filter to identify epics | Grill-me Q9: no C-Epic label | Yes |
| Issue templates are markdown files | Add fields via YAML frontmatter `body` | Current templates already YAML | Yes |

## Findings (cited - path:lines)

- `AGENTS.md:87-165` — Issue-driven workflow, GATE, commit rules, issue lifecycle
- `AGENTS.md:39-83` — PRE-IMPLEMENTATION GATE 4-step checklist
- `.opencode/skills/compass-workflow/SKILL.md:1-255` — Full GATE enforcement + rules
- `kb/dev/process.md:3-19` — Current development cycle (one issue → one PR)
- `kb/dev/process.md:127-131` — Push rhythm: "Push immediately after completing each issue. Do not batch."
- `kb/dev/process.md:138-152` — Feature-branch workflow, regular merge
- `.github/ISSUE_TEMPLATE/` — Two templates: `bug_report.md`, `feature_request.md`
- Existing skills follow pattern: YAML frontmatter → Role → Trigger → Workflow → Output Format → Edge Cases → Must NOT → Collaboration

## Decisions (with rationale)

All 29 decisions from grill-me interview (2026-07-30). Key architecture decisions:

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | GitHub native sub-issue (`gh issue create --parent`) | GA since 2025-04, no manual linking needed |
| 2 | Sub-issues created upfront during `/ulw-plan` | Forces complete design thought, enables parallel batch execution |
| 3 | Parallel batch (DAG topological sort) | Independent sub-issues run in parallel; dependent ones serialize |
| 4 | Commit `ref #<sub-N>` | Preserves "one commit → one issue" granularity |
| 5 | Close sub-issues + epic after PR merge to master | Maintains "close only after push to origin/master" discipline |
| 6 | Multiple sub-issues → one PR, each sub-issue = one commit, regular merge | Avoids half-finished functionality on master; preserves commit history traceability |
| 7 | Dependencies declared in `.omo/plans/<epic>.md` Depends On column | Plan is single source of truth |
| 8 | Batch switch: manual confirmation | Gives human checkpoint; aligns with "never auto-push" discipline |
| 9 | New sub-issues during execution: allowed, update plan file | Real-world inevitably; plan stays canonical |
| 10 | Each sub-issue independently walks full GATE | Preserves test-first/doc-discipline per sub-issue |
| 11 | Epic = one worktree | Single PR output; no need for multi-worktree complexity initially |
| 12 | New skill: `issue-workflow` (`/issue-workflow`) | Covers ALL issue lifecycle; compass-workflow delegates step 1 to it |
| 13 | Issue body template fields: Parent, Plan, Batch, Depends on | Navigation + agent scheduling input |
| 14 | commit-msg hook: no changes needed | One commit = one `ref #<sub-N>` satisfies existing hook |
| 15 | Review: per-sub-issue review + epic-level pre-PR review | Two-layer: correctness (per sub-issue) + integration (pre-PR) |
| 16 | Plan status tracking: table in `.omo/plans/<epic>.md` | pending/in_progress/done; human-readable, agent-parseable, git-diffable |

## Scope IN

1. **Create** `.opencode/skills/issue-workflow/SKILL.md` — new skill covering:
   - Single issue creation (delegated from compass-workflow gate step 1)
   - Epic creation + sub-issue batch creation via `gh issue create --parent`
   - Plan file status tracking (pending/in_progress/done)
   - Batch lifecycle: create → implement → verify → close
   - Issue close: batch close sub-issues + epic after PR merge

2. **Update** `AGENTS.md`:
   - Add "Epic & Sub-Issue Workflow" section (before "Issue-Driven Commits")
   - Update PRE-IMPLEMENTATION GATE step 1: "Create gh issue" → "Invoke /issue-workflow"
   - Update Issue Lifecycle: add batch close after PR merge, epic close after all sub-issues
   - Update Available Skills table: add `issue-workflow` row
   - Update Push rhythm: "Push after PR merge" replaces "Push immediately after each issue"

3. **Update** `.opencode/skills/compass-workflow/SKILL.md`:
   - GATE step 1: replace "Create gh issue" with "Invoke /issue-workflow to create/manage issues"
   - Remove direct issue creation/management rules (Rule 2 "Requirement Flow" → delegate to issue-workflow)
   - Update Available Skills table: add issue-workflow row for gate step 1
   - Update post-implementation review: add epic-level review note
   - Update Commit Style: clarify multiple `ref #N` in one PR

4. **Update** `kb/dev/process.md`:
   - Replace "Issue-driven workflow" cycle diagram with epic-aware version
   - Add "Epic & Sub-Issue Workflow" section: epic creation, sub-issue creation, batch execution, merge, close
   - Update Push rhythm section: "Push after PR merge" (multi-sub-issue PR)
   - Update "Commit → issue linking" table: sub-issue ref pattern

5. **Update** `.github/ISSUE_TEMPLATE/feature_request.md`:
   - Add optional metadata section for when issue is a sub-issue

## Scope OUT (Must NOT have)

- ❌ Changes to `.githooks/commit-msg` or `.githooks/pre-push` (existing hooks already support this)
- ❌ Changes to `cargo test` / `cargo clippy` / `cargo fmt` / `cargo doc` flow
- ❌ New GitHub labels (Q9: no C-Epic label)
- ❌ Changes to test-first (RED → GREEN → REFACTOR) discipline
- ❌ Changes to review workflow (`/review-work` remains unchanged)
- ❌ Changes to branching strategy (feature-branch + regular merge stays)
- ❌ New dependencies or Rust code changes (pure documentation/workflow)
- ❌ Automated batch switching (Q8: manual confirmation stays)

## Open questions

None. All 29 decision points resolved via grill-me interview.

## Approval gate

**status: awaiting-approval**

### What will be built

A new `issue-workflow` skill that manages the complete issue lifecycle — from single-issue creation to epic decomposition with sub-issues and batch processing. Three existing files (AGENTS.md, compass-workflow SKILL.md, kb/dev/process.md) and one template will be updated to delegate issue management to this skill.

### Files changed

| File | Action | Scope |
|------|--------|-------|
| `.opencode/skills/issue-workflow/SKILL.md` | **CREATE** | ~200 lines — full skill definition |
| `AGENTS.md` | UPDATE | ~40 lines added/modified |
| `.opencode/skills/compass-workflow/SKILL.md` | UPDATE | ~20 lines modified |
| `kb/dev/process.md` | UPDATE | ~30 lines added/modified |
| `.github/ISSUE_TEMPLATE/feature_request.md` | UPDATE | ~10 lines added |

### Execution order

1. Create `issue-workflow/SKILL.md` (foundation — everything depends on it)
2. Update `compass-workflow/SKILL.md` (delegate gate step 1 to new skill)
3. Update `AGENTS.md` (integrate epic workflow, reference new skill)
4. Update `kb/dev/process.md` (document epic workflow for humans)
5. Update `.github/ISSUE_TEMPLATE/feature_request.md` (add sub-issue metadata fields)

### After approval

- Scaffold `.omo/plans/epic-sub-issue-workflow.md`
- Append task batches to plan
- Present summary → user can say `$start-work` to execute

---

**Approve?** Reply "ok" / "approve" / "开始" to proceed to plan creation.
