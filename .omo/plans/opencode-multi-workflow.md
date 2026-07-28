# Plan: OpenCode GitHub Multi-Workflow Architecture

## Motivation

Current setup: single `opencode.yml` triggers on any comment containing `/oc` or `/opencode`.
Problems:
- One-size-fits-all — same permissions, same model, same prompt for all scenarios
- No distinction between "answer a question" and "fix a bug and commit"
- No PR review vs PR implementation separation
- CI failure has no automated diagnosis
- AGENTS.md is shared between local dev (full access) and GitHub bot (role-specific)

## Command System

Replace generic `/oc` with purpose-specific commands:

| Command | Scenario | Workflow File |
|---|---|---|
| `/ask` | Issue or PR comment — ask a question | `opencode-ask.yml` |
| `/fix` | Issue or PR comment — fix a reported bug | `opencode-fix.yml` |
| `/review` | PR comment — review code | `opencode-pr-review.yml` |
| `/impl` | Issue/PR comment — implement a feature | `opencode-impl.yml` |
| (auto) | CI workflow failure | `opencode-ci-fix.yml` |

## AGENTS.md Handling Strategy

### Problem

Same repo, same `AGENTS.md`, but two distinct runtime contexts:
- **Local** (opencode TUI): developer wants full compass workflow — grill-me, pre-implementation gate,
  issue-driven commits, test-first, kb-synced. The agent has full agency to implement.
- **GitHub Actions** (bot): role-specific, constrained. A reviewer should NOT implement.
  A Q&A bot should NOT commit. Each role has different permissions and behaviors.

### Decision: Common Baseline + Role Overlay

`AGENTS.md` remains the **single source of truth** for project conventions. It stays
unchanged — it's what local developers read and what the GitHub bot reads as context.

GitHub roles get **additional** role-specific instruction files under `kb/github/`
that layer ON TOP of AGENTS.md. The workflow `prompt` tells the bot how to reconcile them.

### Rationale

Why NOT alternatives:
- **Context tags in AGENTS.md** (`<!-- GITHUB-ONLY -->`): clutters the file, mixes concerns,
  makes AGENTS.md harder to maintain for humans.
- **Separate AGENTS.github.md**: duplicates project conventions, drifts out of sync.
- **Prompt-only (ignore AGENTS.md)**: loses project context — the bot won't know about
  the pre-implementation gate, kb structure, testing conventions, etc.

### Mechanism

1. **AGENTS.md** — unchanged. No context-detection section needed — the workflow `prompt` is
   the context switch.

2. **Each workflow `prompt`** is minimal — it only routes the agent to the right role file.
   Detailed instructions live in `kb/github/<role>.md`:
   ```yaml
   prompt: |
     Read AGENTS.md for project conventions, then read kb/github/<role>.md
     for your full role instructions. Follow them exactly.
   ```
   All role-specific constraints, decision trees, and output formats live in the role file,
   NOT in the workflow YAML. This keeps YAML clean and role instructions easy to maintain.

3. **Role files** (`kb/github/*.md`) contain ONLY what differs from or extends AGENTS.md:
   - Role-specific constraints (e.g., "do NOT commit")
   - Output format requirements
   - Decision trees (e.g., simple vs complex bug)
   - Permissions awareness (what GitHub token allows)

### Content Map

| File | Contains |
|---|---|---|
| `AGENTS.md` | (unchanged) Project conventions, workflow, architecture, testing |
| `kb/github/ask.md` | Full role spec: read-only, analyze, explain, guide. Output format, constraints. |
| `kb/github/fix.md` | Full role spec: decision tree (simple → fix+test+commit; complex → analyze+suggest PR), commit format, complexity criteria. |
| `kb/github/pr-review.md` | Full role spec: review checklist (correctness, conventions, Rust best practices, security, perf), output format, no-implement constraint. |
| `kb/github/impl.md` | Full role spec: follow compass workflow (issue-driven, test-first, kb-synced, commit with ref #N), implementation constraints. |
| `kb/github/ci-fix.md` | Full role spec: diagnose from CI logs, report root cause + proposed fix, no-commit constraint, output format. |

## Workflow Details

### 1. `opencode-ask.yml` — Q&A / Analysis

```
Trigger:     issue_comment OR pull_request_review_comment created containing /ask
Permissions: issues: write, pull-requests: write, contents: read
Model:       normal (cost-effective for Q&A)
Agent role:  Read-only analyst. Read codebase, answer questions, explain behavior.
             NEVER implement, NEVER commit.
Prompt:      "Read AGENTS.md for project conventions, then read kb/github/ask.md
             for your full role instructions. Follow them exactly."
```

### 2. `opencode-fix.yml` — Bug Fix

```
Trigger:     issue_comment OR pull_request_review_comment created containing /fix
Permissions: issues: write, pull-requests: write, contents: write
Model:       strong (needs correct diagnosis)
Agent role:  Bug fixer with complexity gate.
Prompt:      "Read AGENTS.md for project conventions, then read kb/github/fix.md
             for your full role instructions. Follow them exactly."
```

### 3. `opencode-pr-review.yml` — Code Review

```
Trigger:     pull_request_review_comment created containing /review
Permissions: pull-requests: write, contents: read
Model:       strong
Agent role:  Code reviewer. Read-only analysis.
Prompt:      "Read AGENTS.md for project conventions, then read kb/github/pr-review.md
             for your full role instructions. Follow them exactly."
```

### 4. `opencode-impl.yml` — Feature Implementation

```
Trigger:     issue_comment OR pull_request_review_comment created containing /impl
Permissions: contents: write, pull-requests: write, issues: write
Model:       strong
Agent role:  Full implementer following compass workflow.
Prompt:      "Read AGENTS.md for project conventions, then read kb/github/impl.md
             for your full role instructions. Follow them exactly."
```

### 5. `opencode-ci-fix.yml` — CI Failure → Issue + /fix

```
Trigger:     workflow_run: workflows=["CI"], types=[completed], conclusion=failure
Permissions: issues: write
Model:       N/A (does not use opencode action)
Mechanism:   CI fails → gh issue create with ci-failure label + CI details →
             gh issue comment "/fix" → triggers opencode-fix.yml
             The /fix role (kb/github/fix.md) handles CI-specific diagnosis.
```

## Directory Structure

```
compass/
├── AGENTS.md                          # (unchanged) Local dev conventions
├── kb/
│   └── github/                        # GitHub role-specific instructions
│       ├── ask.md                     # /ask role: read-only Q&A
│       ├── fix.md                     # /fix role: simple/complex decision tree
│       ├── pr-review.md               # /review role: review checklist
│       ├── impl.md                    # /impl role: workflow constraints
│       └── ci-fix.md                  # CI failure: diagnose only
├── .github/
│   └── workflows/
│       ├── opencode-ask.yml           # replaces opencode.yml
│       ├── opencode-fix.yml
│       ├── opencode-pr-review.yml
│       ├── opencode-impl.yml
│       ├── opencode-ci-fix.yml
│       └── ci.yml                     # (unchanged) existing CI
└── .omo/
    └── plans/
        └── opencode-multi-workflow.md # this plan
```

## Migration Path

### Step 1: Create 5 workflow YAML files

**Action**: Create `.github/workflows/opencode-{ask,fix,pr-review,impl,ci-fix}.yml`

**QA**:
| Tool | Steps | Expected Result |
|---|---|---|
| `yamllint` or `actionlint` | Run `actionlint .github/workflows/opencode-*.yml` | Zero syntax errors |
| `gh workflow view` | `gh workflow view opencode-ask.yml` | Shows valid workflow with correct trigger (`issue_comment`) and permissions |
| Manual trigger check | For each workflow: `grep -c "on:"` | Each has exactly one `on:` block matching its trigger event |
| Command filter check | For each: `grep "contains.*comment.body"` | Each matches ONLY its designated command (`/ask`, `/fix`, `/review`, `/impl`) |

### Step 2: Create `kb/github/` role files

**Action**: Create `kb/github/{ask,fix,pr-review,impl,ci-fix}.md`

**QA**:
| Tool | Steps | Expected Result |
|---|---|---|
| `ls` | `ls kb/github/` | 5 `.md` files exist |
| `wc -l` | `wc -l kb/github/*.md` | Each file ≥ 10 lines (not stubs) |
| Content review | Read each file, check against Content Map table above | Each file: (a) has role constraint stated, (b) references AGENTS.md as baseline, (c) has NO duplicate of AGENTS.md content |
| Conflict check | For each role file: `grep -i "implement\|commit\|write\|edit"` in read-only roles (`ask`, `review`, `ci-fix`) | Read-only roles contain "do NOT implement/commit" language |

### Step 3: Verify AGENTS.md compatibility

**Action**: Confirm AGENTS.md needs no changes under the Common Baseline + Role Overlay model.

**QA**:
| Tool | Steps | Expected Result |
|---|---|---|
| `grep` | `grep -i "opencode\|github action\|/oc" AGENTS.md` | No hardcoded references to old `/oc` trigger or single-workflow assumptions |
| Conflict scan | Compare each role file's "must do" vs AGENTS.md's "must do" | No direct contradiction. Where overlap exists, role file's constraint is stricter (e.g., role says "read-only", AGENTS.md says "implement test-first" — role wins, no conflict) |

### Step 4: Keep old `opencode.yml` as fallback

**Action**: Do NOT delete `opencode.yml`. Rename or leave in place with both old and new workflows coexisting.

**QA**:
| Tool | Steps | Expected Result |
|---|---|---|
| `gh workflow list` | `gh workflow list --all` | Both old `opencode` and new `opencode-*` workflows listed |
| Trigger isolation | Comment `/oc` on an issue (old trigger) | Old workflow fires (not new ones — their command filters don't match `/oc`) |
| Trigger isolation | Comment `/ask` on an issue AND a PR | New `opencode-ask` fires for both (trigger includes `issue_comment` AND `pull_request_review_comment`) |
| Trigger isolation | Comment `/fix` on an issue AND a PR | New `opencode-fix` fires for both |

### Step 5: Test each workflow in isolation

**Action**: Use a test issue/PR to trigger each command and verify correct behavior.

**QA**:
| Command | Test Method | Expected Result |
|---|---|---|
| `/ask` | Comment `/ask how does CachedProvider work?` on test issue or PR | Bot responds with read-only analysis, no commits |
| `/fix` (simple) | Comment `/fix typo in src/main.rs:42` on test issue or PR | Bot commits fix with `fix:` prefix + `ref #N` |
| `/fix` (complex) | Comment `/fix refactor entire data pipeline` on test issue or PR | Bot responds with analysis + "suggest opening a PR", no commit |
| `/review` | Comment `/review` on test PR | Bot posts review comments, no new commits |
| `/impl` | Comment `/impl add --verbose flag` on test issue | Bot implements + tests + commits with `feat:` prefix + `ref #N` |
| CI auto | Push a commit that breaks `cargo check` | `opencode-ci-fix` triggers, bot comments on the failing commit with diagnosis |

**QA for `/fix` complexity gate specifically**:
| Tool | Steps | Expected Result |
|---|---|---|
| GitHub Actions log | After triggering `/fix` on a multi-module bug, check the run log | Bot reads `kb/github/fix.md`, evaluates complexity, posts comment (no commit) |
| `git log` | After triggering `/fix` on a simple typo | `git log --oneline -1` shows `fix:` commit with `ref #N` |

### Step 6: Remove old `opencode.yml` after confirmation

**Action**: Delete `opencode.yml` after all 5 new workflows verified.

**QA**:
| Tool | Steps | Expected Result |
|---|---|---|
| `gh workflow list` | `gh workflow list` | Old `opencode` workflow NOT listed |
| Comment `/oc` on issue | Old trigger command | No workflow fires (only `opencode-ask` or `opencode-fix` handle `/ask` and `/fix`) |
| Comment `/opencode` on issue | Old trigger command | No workflow fires |
| All new commands | `/ask`, `/fix`, `/review`, `/impl` | Each fires the correct workflow |

## Open Questions

1. Should `/review` also auto-trigger on `pull_request: [opened]` (no comment needed)?
2. Should `/fix` have a `/fix-force` variant that skips complexity gate?
3. For CI-fix: auto-create an issue vs comment on the failing commit?
