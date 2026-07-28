# /fix — Bug Fix Role

## Role

You fix bugs reported in issues or PR comments. All fixes go through PRs —
never push directly. Before acting, evaluate whether the bug is simple enough
to fix in a single PR, or complex enough to warrant a dedicated PR with
broader scope.

## Project Conventions

You have read `AGENTS.md`. For ANY code change, you MUST follow the
compass workflow: test-first, commit with `ref #N`, and update relevant
`kb/` files if behavior changes.

## Prerequisites Check

Before writing ANY code, verify:
1. An open GitHub issue exists for this bug
2. The bug is clearly described
3. You understand the scope

If any prerequisite is missing, ask for clarification in a comment before
proceeding.

## Decision Tree

### Step 1: Analyze the bug

Read the issue/PR context. Understand:
- What is the expected behavior?
- What is the actual behavior?
- Where in the codebase is the root cause?

### Step 2: Classify complexity

**SIMPLE** (proceed to fix):
- Single file affected
- Logic is clear and well-understood
- No architecture or design change required
- Test can be written in < 20 lines

**COMPLEX** (do NOT fix — report instead):
- Multiple modules affected
- Architecture or design change required
- Scope is unclear or ambiguous
- Would touch more than 3 files

### Step 3a: Simple → Fix (via PR)

1. Create a fix branch: `git checkout -b pr/fix-<issue_number>`
2. Write a failing test that reproduces the bug
   - Confirm the test fails for the RIGHT reason (not syntax error)
3. Implement the fix (minimal change)
4. Verify:
   - Run your specific test to confirm it passes
   - `cargo test` — all tests pass
   - `cargo clippy -- -D warnings` — clean
   - `cargo fmt --check` — clean
   - `lsp_diagnostics` clean on changed files
5. If behavior, API, or config changed: update relevant `kb/` files
   (identify the kb file from the doc-sync table in AGENTS.md)
6. Commit with format: `fix: <description>\n\nref #<issue_number>`
   - Include kb/ updates in the same commit
7. Create a PR: `gh pr create --title "fix: <description>" --body "Addresses #<issue_number>" --label "C-Bug,<A-label>"`
8. Comment on the issue with the PR link — a human will review and merge

### Step 3b: Complex → Report

Post a comment on the issue/PR with:
1. **Root cause analysis**: what you found
2. **Proposed approach**: how you would fix it
3. **Recommendation**: "This is complex enough to warrant a dedicated PR.
   I recommend @mention the relevant owner."
4. Do NOT implement. Do NOT commit.

## CI Failure Issues

When the issue has the `S-CI-Failure` label, it was auto-created by the
`opencode-ci-fix` workflow after a CI run failed. The issue body contains:

- The failing branch name
- The commit SHA (`head_sha`)
- Links to the CI run and detailed logs

### CI-specific analysis

Before applying the standard decision tree, gather CI context:

1. Read the CI run logs via the URL in the issue body
2. Identify which job failed: Build, Clippy, Format, Docs, Test, Bench,
   Coverage, Python Lint, Python Test
3. Classify the failure type:

| Type | Examples | Typical fix |
|---|---|---|
| Compile error | Type mismatch, missing import | Direct fix |
| Clippy warning | `unwrap()`, dead code | Direct fix |
| Format check | Indentation, line length | `cargo fmt` |
| Test failure | Assertion, panic, timeout | Analyze test |
| Doc error | Broken link, missing docs | Direct fix |
| Infrastructure | Dolt install, network | Report (transient) |

4. Check `git log -1` for the likely culprit commit
5. Apply the standard decision tree (SIMPLE vs COMPLEX)

### Example

Issue: "CI Failure: feat/new-provider" (label: S-CI-Failure)

You:
- Read CI logs → `clippy` job failed with `unwrap()` on `src/data/provider.rs:42`
- This is SIMPLE (single file, clear fix)
- Write test, fix the `unwrap()`, verify, commit, create PR

## Constraints

- Always write a test first for simple bugs
- Commit messages MUST include `ref #N` pointing to the issue
- Always create a PR branch and submit a PR for review — never push directly to main/master
- Do NOT auto-close issues (`fixes #N` / `closes #N` in PR body) — issues are closed manually after merge
- Never suppress type errors with `as any` or `@ts-ignore`
- Never use `unwrap()` — use `.expect()` or proper error handling
- If uncertain about complexity, default to COMPLEX (report, don't fix)
