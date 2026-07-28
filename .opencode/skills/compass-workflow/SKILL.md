---
name: compass-workflow
description: Enforces the compass project workflow — issue-driven development, doc-sync, test-first, per-step-verify, and commit discipline. Use for any feature, bugfix, or code change in this repo.
---

# Compass Workflow

This project follows a strict workflow. You MUST enforce these rules for every code change.

---

## 🛑 TRIGGER: PRE-IMPLEMENTATION GATE (IMMEDIATE)

**The moment this skill is loaded, you are in GATE MODE.**

**Prerequisite**: grill-me (step 0) must have completed with "shared understanding
reached" before entering this gate. If calling `/grill-me` was skipped, go back
and do it first.

Before you create any todos, before you read any source files, before you write
a single line of code — you MUST verbalize the following checklist to the user.

```
🛑 PRE-IMPLEMENTATION GATE

I will now check each gate step before proceeding:

☐ STEP 0 — GRILL-ME (prerequisite)
   Shared understanding reached
   → [must confirm]

☐ STEP 1 — ISSUE
   Create gh issue from the requirement
   → [must show issue URL to user]

☐ STEP 2 — PLAN (skip only if single-file change)
   Plan agent run and approved
   → [must show plan summary]

☐ STEP 3 — TESTS (RED phase)
   → Invoke /test (qa skill) to write failing tests
   → [must show test failure output]

☐ STEP 4a — RUSTDOC
   → Invoke /rustdoc to verify #[deny(missing_docs)] compliance
   → [must show cargo doc --no-deps is warning-free]

☐ STEP 4b — DOCS (kb/)
   → Invoke /docs to identify and update kb/ files
   → [must list files]
```

**You are FORBIDDEN from using any edit/write/bash tools for implementation
until ALL four steps (1-4) above are completed and shown to the user.**

If you find yourself writing code without completing the gate, STOP IMMEDIATELY
and go back to step 0. This is a HARD BLOCK — no exceptions for feature/bugfix work.

### Exceptions (skip the gate)

The gate does NOT apply to:
- Documentation-only changes
- Lint fixes
- Typo fixes
- Test additions for existing code

> ⚠️ **Skipping the gate does NOT skip the post-implementation review.**
> The `POST-IMPLEMENTATION REVIEW` section below applies to ALL changes,
> including documentation-only. The gate and the review are separate
> processes — gate is pre-implementation, review is post-implementation.

### Gate completion signal

When all five steps are complete, announce explicitly:

```
✅ GATE COMPLETE — proceeding to implementation
```

Only then may you create todos and begin editing files.

---

## Rules (ordered by priority)

### 1. Doc Sync (CRITICAL)

Any code change affecting behavior, public APIs, data structures, config, or workflows MUST update the relevant `kb/` files AND `AGENTS.md` in the SAME commit.

| Change type | kb/ file to update |
|---|---|
| New data source, API call, schema change | `kb/design/data-providers.md` |
| Threading, pipeline, library changes | `kb/design/architecture.md` |
| Symbol format, timeframe mapping | `kb/design/symbols.md` |
| Test framework, patterns | `kb/dev/testing.md` |
| Workflow, hooks, conventions | `kb/dev/process.md` |
| Project-level conventions | `AGENTS.md` |

### 2. Requirement Flow (CRITICAL)

Before writing any feature or bugfix code:
a) verify an open GitHub issue exists (`gh issue view <N>`)
b) if none exists, create one with `gh issue create`
c) confirm with `gh issue view <N>`
d) only then implement

Skip this for: refactors, docs, lint fixes, typos.

Commit references: `ref #N` (feat/fix), no `fixes #N` / `closes #N` (auto-close unwanted).

**This applies to ALL commits — chores, docs, scripts included. No exceptions.**

### 3. Plan First (`/ulw-plan`)

**Non-negotiable for multi-step work.** Run `/ulw-plan` for: multi-step tasks (2+ modules), architecture changes, new data sources, ambiguous scope.

The plan agent produces a `.omo/plans/*.md` file with task wave ordering and verification gates. Do NOT skip this and verbally describe the plan yourself — the agent's structured output is the approved execution contract.

Skip planning only for: truly single-file fixes, test additions, doc updates.

### 4. Test First

Feature and bugfix work follows RED → GREEN → REFACTOR:
- Write failing test FIRST, watch it fail for the right reason
- Then implement
- Exploratory changes may write tests after
- Pure refactors: pin current behavior with characterization tests first

### 5. Per-Step Verify

After every code change:
- `cargo test` → all must pass
- `lsp_diagnostics` clean on changed files

### 6. Local Verify Before Commit

```sh
cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

All three must pass before `git push`.

### 7. No Type Escape

- Never `unwrap()` in production code — use `.expect(msg)` or proper error handling
- Never suppress type errors with `as` casts or `@ts-ignore`

### 8. Branching

Feature-branch workflow: most work happens on branches, merged via PR.
Trivial fixes (typo, config, one-line change) can go directly to master.

```
master  ●──●──●──●────────●  (trunk)
              \          /
pr/xxx        ●──●──●──┘   (PR branch, merge via PR)
```

### 9. Label Enforcement

When creating a GitHub issue or PR:
- Attach at least one **A-** (area) and one **C-** (category) label.
- **D-** (difficulty), **P-** (priority), and **S-** (status) are optional but recommended.

See `kb/github/labels.md` for the complete taxonomy.

`gh issue create --label "C-Bug,A-Data"` or `gh pr create --label "C-Feature,A-GUI"`.

---

## 📋 Available Skills

The compass project provides these opencode skills for specific workflow steps:

| Skill | Slash Command | Purpose | Gate Step |
|---|---|---|---|
| qa (test) | `/test` | Write failing tests (TDD/BDD), test coverage | Step 3 — TESTS |
| rustdoc | `/rustdoc` | Verify `#[deny(missing_docs)]` compliance | Step 4a — RUSTDOC |
| docs | `/docs` | Identify and update kb/ files | Step 4b — DOCS |
| reflect | `/reflect` | Write post-implementation reflection + trend analysis | Post-implementation |

When the gate checklist says `→ Invoke /<command>`, load that skill and follow
its workflow. Each skill has a `SKILL.md` file in `.opencode/skills/<name>/`.

---

## 🔍 POST-IMPLEMENTATION REVIEW (AUTOMATED)

After completing implementation, run an automated review to catch issues
before they reach the repo. The old manual checklist is replaced by this.

### Step 1: Commit

Commit the implementation first — always. Do not run review before committing.

```
git add <files>
git commit -m "feat: description

ref #N"
```

### Step 2: Run Review

Trigger `/review-work` against the current changes. The review runs 5
agents in parallel: goal verification, QA execution, code quality,
security audit, and context mining.

### Step 3: Handle Findings

For each finding reported by the review:

| Finding Type | Action |
|---|---|
| Related to current work, ≤3 files affected | Auto-fix directly |
| Unrelated to current work | Create a GitHub issue (`gh issue create`) |
| Related but >3 files affected | Create a GitHub issue |

Use the review agent's `blocking_issues` as the primary input.
In-scope = fixes within the files and modules touched by this PR/change.

### Step 4: Re-review (max 2 rounds)

After fixing issues, re-run the review to verify fixes are correct.
If the review still reports blocking issues after 2 rounds, create
issues for the remaining problems and note them in the commit message.

### Step 5: Finalize

- All in-scope issues resolved → proceed to commit (or push)
- → Invoke /reflect to write post-implementation reflection

---

## 📝 REFLECTION RECORD

After EVERY feature or bugfix implementation, invoke `/reflect` (reflect skill)
to write a post-implementation reflection and append it to `kb/dev/reflections.md`.
This replaces the old manual reflection mandate — the reflect skill handles
writing, format, and trend analysis.

See `.opencode/skills/reflect/SKILL.md` for the full reflection workflow.

---

## Commit Style

- `feat:` / `fix:` / `test:` / `refactor:` / `docs:` / `chore:`
- Atomic: one logical unit per commit
- Push immediately after commit

## Code Style

- Rust edition 2024, thiserror, async-trait, tracing
- Match existing conventions in the file you're editing
