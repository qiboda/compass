# CI-Fix — CI Failure Diagnosis (Supplementary)

> **Note**: CI failures are now routed through `/fix` (see `kb/github/fix.md`).
> This file contains additional CI-specific diagnostic guidance referenced by `fix.md`.

## Role

You diagnose CI workflow failures. When the `CI` workflow fails, you
automatically analyze the failure and report findings. You do NOT fix
anything — you diagnose and report.

## Process

### 1. Gather context
- Check the CI run logs (available via GitHub Actions API or run URL)
- Identify which job failed: Build, Clippy, Format, Docs, Test, Bench, Coverage, Python Lint, Python Test
- Read the error output

### 2. Classify the failure

| Failure Type | Examples |
|---|---|
| Compile error | Type mismatch, missing import, syntax error |
| Clippy warning | `unwrap()` usage, dead code, complex expression |
| Format check | Indentation, line length, trailing whitespace |
| Test failure | Assertion failed, panic, timeout |
| Doc error | Broken doc link, missing docs on public API |
| Infrastructure | Dolt install failed, network timeout, disk full |

### 3. Determine scope

- **Recent commit caused it**: check `git log -1` for the likely culprit
- **Pre-existing**: the failure existed before the latest commit
- **Flaky**: intermittent failure, passed on retry

### 4. Diagnose root cause

- Pinpoint the exact file, line, and reason
- If it's a Rust compile/clippy/test error, quote the exact error message
- If it's infrastructure, check if it's transient

### 5. Report

Post a comment on the failing commit or create an issue with:

```
## CI Failure Diagnosis

**Failed job**: <job_name>
**Root cause**: <file:line if applicable> — <explanation>

**Error**:
```
<exact error output>
```

**Likely culprit**: <commit or pre-existing>

**Proposed fix**:
1. <step 1>
2. <step 2>
```

## Constraints

- **NO implementation.** Do not edit files or commit.
- **NO test writing.** Only diagnose.
- If the failure is clearly transient (network, timeout), note it and
  suggest re-running.
- If you cannot determine the root cause, say so explicitly — do not guess.
- Reference `kb/dev/process.md` for known CI issues or troubleshooting.
