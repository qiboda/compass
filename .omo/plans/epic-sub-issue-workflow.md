# epic-sub-issue-workflow - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** A new issue lifecycle skill that supports breaking large requirements into GitHub sub-issues, processing them in dependency-ordered batches, and merging them as a single PR. Existing workflow rules (test-first, review, commit discipline) remain unchanged.

**Why this approach:** Build a dedicated `issue-workflow` skill that handles all issue lifecycle (single + epic/sub-issue), then delegate to it from compass-workflow. This keeps concerns separated and allows independent evolution of issue management.

**What it will NOT do:** No changes to commit hooks, test discipline, branching strategy, or code quality gates. No new labels. No automated batch switching.

**Effort:** Quick — 5 documentation files, zero code changes
**Risk:** Low — purely additive workflow documentation; existing rules unchanged
**Decisions to sanity-check:** Sub-issue → one PR (risk of delayed feedback on master), manual batch switching (throughput vs control tradeoff)

Your next move: Approve, then run `$start-work` to execute. Full execution detail follows below.

---

> TL;DR (machine): Quick effort, Low risk — create issue-workflow skill + update 4 docs to support epic/sub-issue batch processing

## Scope
### Must have
- New `.opencode/skills/issue-workflow/SKILL.md` skill file
- `AGENTS.md` updated with epic/sub-issue workflow section
- `.opencode/skills/compass-workflow/SKILL.md` updated: GATE step 1 delegatess to issue-workflow
- `kb/dev/process.md` updated with epic workflow documentation
- `.github/ISSUE_TEMPLATE/feature_request.md` updated with sub-issue metadata fields

### Must NOT have (guardrails, anti-slop, scope boundaries)
- No changes to `.githooks/` (commit-msg, pre-push)
- No new GitHub labels
- No Rust code changes
- No test/cargo/build step changes
- No changes to review workflow (`/review-work`)
- No new dependencies

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: none (pure documentation change — verification is structural consistency check)
- Evidence: manual diff review of each file against this plan

## Execution strategy
### Parallel execution waves
- Wave 1: T1 (issue-workflow SKILL.md) — foundation, blocks all others
- Wave 2: T2 (compass-workflow), T3 (AGENTS.md), T4 (process.md), T5 (templates) — all parallel, reference T1

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | - | 2,3,4,5 | - |
| 2 | 1 | - | 3,4,5 |
| 3 | 1 | - | 2,4,5 |
| 4 | 1 | - | 2,3,5 |
| 5 | 1 | - | 2,3,4 |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

- [ ] 1. Create issue-workflow skill (`.opencode/skills/issue-workflow/SKILL.md`)
  What to do: Write a complete skill file following the existing skill format (YAML frontmatter → Role → Trigger → Workflow → Output Format → Edge Cases → Must NOT → Collaboration). Covers single-issue creation, epic/sub-issue batch creation via `gh issue create --parent`, plan file status tracking, batch lifecycle management, and batch close after PR merge.
  Must NOT do: Modify any existing file. Do not include test/cargo/code-related sections — this skill manages GitHub issues only.
  Parallelization: Wave 1 | Blocked by: - | Blocks: 2,3,4,5
  References: `.opencode/skills/qa/SKILL.md` (format template), `AGENTS.md:87-165` (current issue rules to absorb), grill-me Q1-Q29 decisions
  Acceptance criteria: File exists at `.opencode/skills/issue-workflow/SKILL.md`. Contains all required sections per existing skill format. Covers: single issue creation, epic creation, sub-issue batch creation, batch status tracking, batch close. Matches all 29 grill-me decisions.
  QA scenarios: Read file, verify all section headers present, verify `gh issue create --parent` syntax used, verify batch close `gh issue close` logic, verify collaboration section references compass-workflow.
  Commit: Y | feat(skills): add issue-workflow skill for epic/sub-issue lifecycle management

- [ ] 2. Update compass-workflow SKILL.md to delegate issue management
  What to do: Modify GATE step 1 checklist from "Create gh issue" to "Invoke /issue-workflow". Update Available Skills table to add issue-workflow row. Update Rule 2 (Requirement Flow) to reference issue-workflow instead of inline issue commands. Update Commit Style note about multi-ref PR.
  Must NOT do: Change any other gate steps (2-4b). Do not change test/review/rustdoc/docs rules. Do not change post-implementation review.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: -
  References: `.opencode/skills/compass-workflow/SKILL.md` (current), `.opencode/skills/issue-workflow/SKILL.md` (new skill to reference)
  Acceptance criteria: GATE step 1 reads "Invoke /issue-workflow to create/manage issues". Available Skills table has issue-workflow row. Rule 2 delegates to issue-workflow. No other rules changed.
  QA scenarios: grep for "gh issue create" in file — should NOT appear (delegated to issue-workflow). grep for "/issue-workflow" — should appear.
  Commit: Y | refactor(skills): delegate issue management from compass-workflow to issue-workflow

- [ ] 3. Update AGENTS.md with epic/sub-issue workflow
  What to do: Add "Epic & Sub-Issue Workflow" section before "Issue-Driven Commits". Update PRE-IMPLEMENTATION GATE step 1 to delegate to issue-workflow. Update Issue Lifecycle to include batch close + epic close. Add issue-workflow to Available Skills table. Update Push rhythm section.
  Must NOT do: Change grill-me priority rule. Do not change gate steps 2-4b. Do not change commit style.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: -
  References: `AGENTS.md:87-165` (sections to update), grill-me decisions Q1-Q29
  Acceptance criteria: "Epic & Sub-Issue Workflow" section exists. GATE step 1 references issue-workflow. Issue Lifecycle includes batch close flow. Available Skills table has issue-workflow.
  QA scenarios: Read file, verify all required sections present, verify consistent reference to `/issue-workflow`.
  Commit: Y | docs(workflow): add epic/sub-issue workflow to AGENTS.md

- [ ] 4. Update kb/dev/process.md with epic workflow documentation
  What to do: Add "Epic & Sub-Issue Workflow" section after existing Issue-driven workflow. Document: epic creation flow, sub-issue creation, batch execution, one-PR-multiple-commits pattern, batch close. Update Push rhythm section (was "push after each issue", now "push after PR merge"). Update "Commit → issue linking" table.
  Must NOT do: Change testing/debugging/config sections. Do not change worktree documentation.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: -
  References: `kb/dev/process.md:3-45` (workflow sections to update), `kb/dev/process.md:127-131` (push rhythm), grill-me decisions
  Acceptance criteria: Epic workflow section documents: create epic → plan sub-issues → batch implement → one PR → merge → batch close. Push rhythm updated. Commit linking table covers sub-issue ref pattern.
  QA scenarios: Read file, verify new section exists, verify push rhythm text updated, verify no contradictions with existing sections.
  Commit: Y | docs(process): document epic/sub-issue workflow

- [ ] 5. Update issue template with sub-issue metadata fields
  What to do: Add optional metadata section to `.github/ISSUE_TEMPLATE/feature_request.md` for when the issue is a sub-issue. Include fields: Parent (#N), Plan (file path), Batch (number), Depends on (#N list). Keep existing template structure intact.
  Must NOT do: Change existing template fields. Do not make metadata mandatory — it's only populated when the issue IS a sub-issue.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: -
  References: `.github/ISSUE_TEMPLATE/feature_request.md` (current), grill-me Q23
  Acceptance criteria: Template has optional "Sub-Issue Metadata" section with Parent, Plan, Batch, Depends on fields. Existing Problem/Proposed/Alternatives/Context sections unchanged.
  QA scenarios: Read file, verify new section uses HTML comments for optionality, verify original template structure preserved.
  Commit: Y | chore(templates): add sub-issue metadata fields to feature request template

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Consistency audit — check all 5 files agree on: skill name (`issue-workflow`), slash command (`/issue-workflow`), batch close flow, `ref #<sub-N>` pattern
- [ ] F2. Grill-me decision coverage — verify all 29 locked decisions are reflected in at least one file
- [ ] F3. No regression — grep for "gh issue create" in compass-workflow SKILL.md (should NOT appear), verify AGENTS.md gate step 1 changed

## Commit strategy
- T1: `feat(skills): add issue-workflow skill for epic/sub-issue lifecycle management` — `ref #67`
- T2: `refactor(skills): delegate issue management from compass-workflow to issue-workflow` — `ref #67`
- T3: `docs(workflow): add epic/sub-issue workflow to AGENTS.md` — `ref #67`
- T4: `docs(process): document epic/sub-issue workflow` — `ref #67`
- T5: `chore(templates): add sub-issue metadata fields to feature request template` — `ref #67`
- All 5 commits in one PR, regular merge.

## Success criteria
- [ ] All 5 files changed and consistent
- [ ] `issue-workflow` skill loadable via `/issue-workflow` slash command
- [ ] compass-workflow GATE step 1 delegates to issue-workflow
- [ ] AGENTS.md documents full epic/sub-issue lifecycle
- [ ] kb/dev/process.md has human-readable epic workflow docs
- [ ] Issue template supports sub-issue metadata
