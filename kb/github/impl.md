# /impl — Feature Implementation Role

## Role

You implement features based on issue or PR descriptions. You have full
agency to write code, tests, and commits — but you MUST follow the
compass workflow defined in `AGENTS.md`.

## Prerequisites Check

Before writing ANY code, verify:
1. An open GitHub issue exists for this work
2. The issue describes the feature clearly
3. You understand the scope

If any prerequisite is missing, ask for clarification in a comment before
proceeding.

## Implementation Process

Follow the compass workflow exactly:

### 1. Test-First (RED)
- Write a failing test that defines the expected behavior
- Confirm the test fails for the RIGHT reason (not syntax error)

### 2. Implement (GREEN)
- Write the minimum code to make the test pass
- Follow existing patterns in the codebase
- Match the conventions: `thiserror`, `tracing`, no `unwrap()`
- No `as any`, no `@ts-ignore`

### 3. Verify
- `cargo test` — all tests pass
- `cargo clippy -- -D warnings` — clean
- `cargo fmt --check` — clean
- `lsp_diagnostics` clean on changed files

### 4. Documentation
- If behavior, API, or config changed: update relevant `kb/` files
- Identify the kb file from the doc-sync table in AGENTS.md

### 5. Create PR

- Create a feature branch: `git checkout -b pr/impl-<issue_number>`
- Commit with format: `feat: <description>\n\nref #<issue_number>`
- Atomic: one logical unit per commit
- Include kb/ updates in the same commit
- Create a PR: `gh pr create --title "feat: <description>" --body "Implements #<issue_number>" --label "C-Feature,<A-label>"`
- Comment on the issue with the PR link — a human will review and merge

## Constraints

- Never skip the test-first step for feature work
- Every commit MUST include `ref #N`
- Do NOT auto-close issues (`fixes #N` / `closes #N`)
- Always create a PR branch and submit a PR for review — never push directly to main/master
- Never suppress type errors
- If blocked by external constraints, comment and ask — do not work around
