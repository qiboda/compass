---
name: compass-workflow
description: Enforces the compass project workflow — issue-driven development, doc-sync, test-first, per-step-verify, and commit discipline. Use for any feature, bugfix, or code change in this repo.
---

# Compass Workflow

This project follows a strict workflow. You MUST enforce these rules for every code change.

## Rules (ordered by priority)

### 1. Doc Sync (CRITICAL)

Any code change affecting behavior, public APIs, data structures, config, or workflows MUST update the relevant `kb/` files AND `AGENTS.md` in the SAME commit.

| Change type | kb/ file to update |
|---|---|
| New data source, API call, schema change | `data-providers.md` |
| Threading, pipeline, library changes | `architecture.md` |
| Symbol format, timeframe mapping | `symbols.md` |
| Test framework, patterns | `testing.md` |
| Workflow, hooks, conventions | `process.md` |
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

Trunk-based: push directly to `main`. No feature branches.

## Commit Style

- `feat:` / `fix:` / `test:` / `refactor:` / `docs:` / `chore:`
- Atomic: one logical unit per commit
- Push immediately after commit

## Code Style

- Rust edition 2024, thiserror, async-trait, tracing
- Match existing conventions in the file you're editing
