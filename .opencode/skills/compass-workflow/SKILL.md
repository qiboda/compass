---
name: compass-workflow
description: Enforces the compass project workflow — issue-driven development, doc-sync, test-first, per-step-verify, and commit discipline. Use for any feature, bugfix, or code change in this repo.
---

# Compass Workflow

This project follows a strict workflow. You MUST enforce these rules for every code change.

---

## 🛑 TRIGGER: PRE-IMPLEMENTATION GATE (IMMEDIATE)

**The moment this skill is loaded, you are in GATE MODE.**

Before you create any todos, before you read any source files, before you write
a single line of code — you MUST verbalize the following checklist to the user.

```
🛑 PRE-IMPLEMENTATION GATE

I will now check each gate step before proceeding:

☐ STEP 1 — ISSUE
   Create gh issue from the requirement
   → [must show issue URL to user]

☐ STEP 2 — PLAN (skip only if single-file change)
   Plan agent run and approved
   → [must show plan summary]

☐ STEP 3 — TESTS (RED phase)
   Write failing test FIRST, confirm it fails for the right reason
   → [must show test failure output]

☐ STEP 4 — DOCS
   Identify which kb/ files need updating:
   → [must list files]
```

**You are FORBIDDEN from using any edit/write/bash tools for implementation
until ALL four steps (1-4) above are completed and shown to the user.**

If you find yourself writing code without completing the gate, STOP IMMEDIATELY
and go back to step 1. This is a HARD BLOCK — no exceptions for feature/bugfix work.

### Exceptions (skip the gate)

The gate does NOT apply to:
- Documentation-only changes
- Lint fixes
- Typo fixes
- Test additions for existing code

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

### 3. Plan First

Use the plan agent for: multi-step tasks (2+ modules), architecture changes, new data sources, ambiguous scope.

Skip planning for: single-file fixes, tests, doc updates.

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

Trunk-based: push directly to `master`. No feature branches.

---

## 🔄 POST-IMPLEMENTATION SELF-AUDIT

After completing implementation, review your own work against this checklist:

```
🔍 POST-IMPLEMENTATION AUDIT

☐ Were all gate steps (0-4) completed before code was written?
☐ Does every changed kb/ file reflect the actual changes?
☐ Do all tests pass? (cargo test)
☐ Is cargo clippy clean?
☐ Is cargo fmt --check clean?
☐ Does the commit include ref #N?
☐ Are kb/ updates in the same commit as code changes?
```

If any box is unchecked, fix it before pushing.

---

## 📝 REFLECTION RECORD (MANDATORY)

After EVERY feature or bugfix implementation, you MUST write a brief reflection
and append it to `kb/dev/reflections.md`. This is NOT optional.

### Format

```markdown
## [date] — [issue ref] [brief title]

**What was done**: [1-2 sentences summarizing the change]

**What went wrong** (if any): [process failures, missed steps, surprises]

**Lessons learned**: [what to do differently next time]
```

### Purpose

Reflections compound. They prevent the same mistakes from recurring. If you
skipped the gate or violated a rule, that MUST appear in the reflection.

The reflection MUST be committed in the same commit as the implementation,
or as a follow-up commit immediately after.

---

## Commit Style

- `feat:` / `fix:` / `test:` / `refactor:` / `docs:` / `chore:`
- Atomic: one logical unit per commit
- Push immediately after commit

## Code Style

- Rust edition 2024, thiserror, async-trait, tracing
- Match existing conventions in the file you're editing
