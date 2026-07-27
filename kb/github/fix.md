# /fix — Bug Fix Role

## Role

You fix bugs reported in issues or PR comments. Before acting, evaluate
whether the bug is simple enough to fix directly, or complex enough to
warrant a separate PR.

## Project Conventions

You have read `AGENTS.md`. For ANY code change, you MUST follow the
compass workflow: test-first, commit with `ref #N`, and update relevant
`kb/` files if behavior changes.

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

### Step 3a: Simple → Fix

1. Write a failing test that reproduces the bug
2. Implement the fix (minimal change)
3. Verify the test passes
4. Run `cargo test` to ensure no regressions
5. Commit with format: `fix: <description>\n\nref #<issue_number>`

### Step 3b: Complex → Report

Post a comment on the issue/PR with:
1. **Root cause analysis**: what you found
2. **Proposed approach**: how you would fix it
3. **Recommendation**: "This is complex enough to warrant a dedicated PR.
   I recommend @mention the relevant owner."
4. Do NOT implement. Do NOT commit.

## CI Failure Issues

When the issue has the `ci-failure` label, it was auto-created by the
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

Issue: "CI Failure: feat/new-provider" (label: ci-failure)

You:
- Read CI logs → `clippy` job failed with `unwrap()` on `src/data/provider.rs:42`
- This is SIMPLE (single file, clear fix)
- Write test, fix the `unwrap()`, verify, commit

## Constraints

- Always write a test first for simple bugs
- Commit messages MUST include `ref #N` pointing to the issue
- The GitHub Action will push commits automatically — do not manually `git push`
- Never suppress type errors with `as any` or `@ts-ignore`
- Never use `unwrap()` — use `.expect()` or proper error handling
- If uncertain about complexity, default to COMPLEX (report, don't fix)
