# pr-workflow — Work Plan

## TL;DR (For humans)

**What you'll get**: Compass 开发工作流从主干开发（trunk-based，直接 push master）切换到 PR 工作流。每个功能/修复在独立分支上开发，通过 PR 提交，CI 通过后由人工合并（squash merge），合并后自动关闭关联 Issue。

**Why this approach**: 5 个独立文档/配置文件各自更新，互不依赖，可并行执行。CI 已支持 PR 触发，无需修改。

**What it will NOT do**: 不修改 CI 配置、不改变代码结构、不影響 Beads 任务追踪、不改变测试或 lint 规则。

**Effort**: ~30 分钟，纯文档和 hook 脚本修改。

**Decisions**: squash merge | 人工合并 | `Closes #N` 自动关闭 Issue | 目标分支 `master` | 分支命名 `feat/xxx` / `fix/xxx`

---

## Scope

### IN
- `AGENTS.md`：更新工作流步骤、Session Completion、Agent Context Profiles
- `kb/dev/process.md`：重写 Git 分支章节、PR 节奏、版本控制流程、修复 master/main 不一致
- `.opencode/skills/compass-workflow/SKILL.md`：规则 8、Pre-implementation Gate、Post-audit
- `.githooks/pre-push`：去掉 master CI 检查，保留 fmt/clippy/doc 质量门
- `.github/PULL_REQUEST_TEMPLATE.md`：新建 PR 模板

### OUT
- CI 配置（已支持 PR）
- 代码实现文件
- Beads 配置
- 测试文件

### Must-NOT-Have
- 不自动合并 PR（人工操作）
- 不改变 commit-msg hook（`ref #N` 规则保留）
- 不引入 PR 审批人数要求（solo 项目）

---

## Verification strategy

**测试策略**: 纯文档/配置变更，无代码变更，使用 lint + 手动验证。

- 文档一致性：`AGENTS.md` 与 `kb/dev/process.md` 与 SKILL.md 三者不矛盾
- Hook 语法：`bash -n .githooks/pre-push` 通过
- PR 模板：Markdown 语法正确，所有 checklist 项可勾选
- 分支名一致性：所有文件引用 `master`（无残留 `main`）

---

## Execution strategy

5 个组件独立，按逻辑顺序分 2 波执行：

**Wave 1**（核心流程）：`kb/dev/process.md` + `compass-workflow SKILL.md` — 定义新工作流
**Wave 2**（周边配置）：`AGENTS.md` + `.githooks/pre-push` + `PULL_REQUEST_TEMPLATE.md` — 对齐新流程

每波内可并行。每个 todo 完成后独立提交。

---

## Todos

### Wave 1 — Core workflow docs

- [x] 1. Rewrite `kb/dev/process.md` git branching, PR, and commit discipline sections
  **References**: `kb/dev/process.md` lines 7–16 (workflow diagram), lines 28–42 (bug discovery + commit linking), lines 89–103 (before pushing), lines 109–137 (push rhythm, commit discipline, git branching, version control)
  **Changes**:
  - Lines 7–16: Replace issue-driven workflow diagram with full PR flow:
    ```
    User raises requirement
      →  OpenCode grills (/grill-me) to clarify scope and decisions
      →  Shared understanding reached → summarize locked-in decisions
      →  OpenCode creates GitHub issue (feature_request or bug_report template)
      →  OpenCode shows issue with gh issue view <N>
      →  git checkout -b feat/desc              # create feature branch
      →  /ulw-plan (if multi-step) → implement
      →  cargo nextest + clippy + fmt → commit with ref #N → push branch
      →  gh pr create --body "Closes #N"        # create PR
      →  CI passes → manual squash merge → issue auto-closes via Closes #N
      →  git checkout master && git pull && git branch -d feat/desc  # cleanup
    ```
  - Line 32: `commit with `fixes #N`` → `commit with `ref #N``
  - Lines 36–42: Replace commit-msg/issue-close section with PR-based version:
    ```markdown
    ### Commit → issue linking

    | Commit type | Issue reference |
    |---|---|
    | feat / fix | `ref #N` in commit body |

    `ref #N` goes in the commit body (enforced by commit-msg hook).
    `Closes #N` goes in the PR description body — GitHub auto-closes when the PR is merged.

    Never put `fixes #N` or `closes #N` in a commit message — that would
    auto-close the issue when the commit merges to master, bypassing PR review.
    ```
  - Lines 89–103 ("Before pushing"): Update to reflect PR workflow:
    - Line 93: Remove "latest CI run on master must be passing" (CI runs on PR branch, managed by GitHub Actions)
    - Line 95: Replace "Never push on top of a broken CI" with "Never merge a PR with failing CI — check CI status at the PR page or with `gh pr checks <branch>`"
    - Lines 101–103: The "Manual pre-push checklist" content refers to pre-push hook quality gates that remain (fmt, clippy, doc). Keep this section but update the lead-in text from "All four must pass before `git push`" to "All quality gates must pass before `git push`" (line 105).
  - Lines 109–112: Replace "Push rhythm" with "PR rhythm":
    ```markdown
    ### PR rhythm

    Create a PR immediately after completing each issue. Do not batch.
    Keep PRs small and focused on a single issue.
    ```
  - Lines 113–118: Replace "Commit discipline" section:
    ```markdown
    ### Commit discipline

    - Each commit = one logical unit. Never mix bugfix + feature + refactor.
    - Conventional commits: `feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`.
    - All feat/fix commits must include `ref #N` in the commit body.
    - Never use `fixes #N` or `closes #N` in commit messages — those go in PR body.
    - Template: `git config commit.template .gitmessage` is already set.
    ```
  - Lines 120–129: Replace trunk-based section with feature-branch + PR workflow:
    ```markdown
    ## Git branching

    **Feature-branch + PR.** Each feature/fix gets its own branch off `master`.

    ```
    master  ──●────────●────────●──  (PR squash merge)
              \        /        /
      feat/xxx  ●──●──●        /
      fix/yyy         ●──●──●─
    ```

    - Branch naming: `feat/<short-description>` for features, `fix/<short-description>` for fixes
    - Push branch → create PR → CI passes → manual squash merge → branch deleted
    - Never push directly to `master`
    ```
  - Lines 131–137: Replace "Version control" with PR workflow (includes remote cleanup):
    ```markdown
    ## Version control

    ```sh
    git checkout -b feat/short-desc          # create feature branch
    git add <files>                           # stage only intended changes
    git commit                                 # uses .gitmessage template
    git push -u origin feat/short-desc        # push branch (not master), set upstream
    gh pr create --base master --title "..." --body "Closes #N"  # create PR
    # Wait for CI to pass, then manually squash-merge via GitHub UI
    git checkout master && git pull            # sync local master
    git branch -d feat/short-desc             # delete local branch
    git push origin --delete feat/short-desc  # clean up remote branch
    ```
  - Any remaining occurrence of `main` as target branch → fix to `master`
  **Acceptance criteria**:
  - Section "Git branching" describes feature-branch + PR workflow with ASCII diagram
  - Section "PR rhythm" exists and replaces push rhythm
  - "Version control" shows branch → PR → merge → cleanup commands
  - Workflow diagram (lines 7–16) includes push branch, PR creation, and auto-close steps
  - No occurrences of "trunk-based" or "push master" remain in process.md
  - No occurrences of `fixes #N` or `closes #N` in commit guidance (lines 32, 117 both fixed)
  - All references to target branch use `master` (no `main`)
  - "Before pushing" section updated to PR context (no master CI check)
  - Commit discipline section no longer says `fixes #N`/`closes #N` for commits
  **QA**:
  - Happy: Read `kb/dev/process.md`, verify new workflow is described, old trunk-based text is gone
  - Failure: `grep -i "trunk.based\|push.*master\|push.*main\|fixes #N\|closes #N" kb/dev/process.md` returns no matches (except in quoted PR body examples)
  - Cross-check: `grep "ref #N" kb/dev/process.md` returns matches in commit discipline and bug-fix sections
  **Commit**: `docs: switch to PR-based workflow in kb/dev/process.md`

- [x] 2. Update `.opencode/skills/compass-workflow/SKILL.md` PR rules and gates
  **References**: `.opencode/skills/compass-workflow/SKILL.md` lines 20–39 (Gate step checkboxes), lines 130–131 (Rule 8), lines 137–149 (Post-audit)
  **Changes**:
  - Pre-implementation Gate (insert AFTER line 39, after step 4 checkbox): Add step 5:
    ```
    ☐ STEP 5 — BRANCH
       Create feature branch from master (git checkout -b feat/desc or fix/desc)
       → [must show branch name]
    ```
  - Rule 8 (line 131): Replace "Trunk-based: push directly to `master`. No feature branches." with:
    ```markdown
    ### 8. Branching + PR

    Feature-branch + PR workflow:
    - Branch naming: `feat/<desc>` for features, `fix/<desc>` for fixes
    - Push to branch (never master), create PR via `gh pr create`
    - CI must pass before merge; merge is manual (squash)
    - `Closes #N` in PR description for auto-close on merge
    - Delete branch after merge
    ```
  - Post-implementation Audit (lines 137–149): Add PR checklist items after existing items (note: post-audit runs after implementation, before push/merge — only include pre-merge checks):
    ```
    ☐ Is the branch pushed to remote? (git push origin <branch>)
    ☐ Is a PR created? (gh pr create --base master)
    ☐ Does the PR body include Closes #N?
    ```
    Do NOT add "branch deleted after merge" here — that belongs in Session Completion cleanup, not the post-audit (merge hasn't happened yet).
    Also update line 142: "gate steps (0-4)" → "gate steps (1-5)" to match the new 5-step gate numbering in SKILL.md.
  **Acceptance criteria**:
  - Rule 8 describes feature-branch + PR workflow with branch naming convention
  - Pre-implementation Gate step 5 (branch creation) appears after step 4 in the checkbox list
  - Post-audit includes PR-specific checklist items
  - No reference to trunk-based or direct push to master remains
  **QA**:
  - Happy: Read SKILL.md, verify gate checklist has step 5, rule 8 describes PRs
  - Failure: `grep "trunk.based\|push.*master" .opencode/skills/compass-workflow/SKILL.md` returns no matches
  - Verify step 5 inserted at correct position (after step 4 checkbox, before FORBIDDEN text)
  **Commit**: `docs: update compass-workflow skill for PR-based workflow`

### Wave 2 — Supporting config

- [x] 3. Update `AGENTS.md` workflow sections for PR workflow
  **References**: `AGENTS.md` lines 20–41 (Pre-implementation Gate), lines 59–64 (Workflow section), lines 251–257 (Agent Context Profiles), lines 259–282 (Session Completion)
  **Changes**:
  - Pre-implementation Gate (line 28): Add branch creation step AFTER issue creation (step 1.5, not step 0) — the issue must exist first so you know what to name the branch. Insert between step 1 and step 2 rows:
    ```
    | **1. Issue** | Verify `gh issue view <N>` exists, or create one | Issue URL shown to user |
    | **1b. Branch** | Create feature branch: `git checkout -b feat/desc` or `fix/desc` | Branch name shown |
    | **2. Plan** | If 2+ modules involved: run `/ulw-plan` agent until approval | `.omo/plans/*.md` file created + user approved |
    ```
    (Renumbering: old steps 2–4 become 2–4, branch is step 1b/1.5 between issue and plan.)
    This ordering matches `kb/dev/process.md` workflow: grill → issue → show issue → create branch → plan/implement.
  - Session Completion (lines 259–282): Replace the "Handle git/sync by active profile" section with PR workflow steps, while PRESERVING Beads `bd dolt push` for issue sync:
    ```markdown
    4. **Push branch and create PR**:
       ```bash
       git push origin <branch>
       gh pr create --base master --title "..." --body "Closes #N"
       ```
    5. **Wait for CI**: Ensure all checks pass before manual merge.
    6. **Sync Beads** (if using Dolt sync for issues):
       ```bash
       bd dolt push
       ```
    7. **Clean up after merge**:
       ```bash
       git checkout master && git pull
       git branch -d <branch>
       git push origin --delete <branch>   # clean up remote branch too
       ```
    8. **Hand off** - Summarize changes, validation, issue status, PR link, and suggested next commands.
    ```
  - Agent Context Profiles (line 255): Replace conservative profile text with exact replacement:
    ```markdown
    - **Conservative (default)**: Use `bd` for task tracking. Do not run `git push` to master,
      Dolt remote sync, or merge PRs unless explicitly asked. Allowed without explicit ask:
      create feature branches, push feature branches, create PRs via `gh pr create`.
      At handoff, report changed files, validation, PR link, and suggested next commands.
    ```
    Note: the old text said "Do not run git commits" — this restriction is lifted because
    PR workflow requires committing to feature branches. This is an intentional policy change.
  - Beads Session Completion (lines 263–276): Update to match — same replacement as above.
  - Note to implementer: AGENTS.md has TWO Beads blocks (`BEGIN BEADS INTEGRATION` at line 229 and `BEGIN BEADS CODEX SETUP` at line 285). Only the first block (lines 229–283) contains Session Completion and Agent Context Profiles. The second block (lines 285–307) has no workflow instructions — leave it unchanged.
  **Acceptance criteria**:
  - Pre-implementation Gate has branch creation AFTER issue creation (step 1b), matching process.md ordering
  - Session Completion describes push branch → create PR → wait CI → Beads sync → merge → clean up remote branch → handoff
  - Conservative profile allows feature branch push + `gh pr create`, forbids push to master
  - `bd dolt push` preserved in Session Completion (not silently removed)
  - Agent profile text is exact replacement (not a vague directive)
  - Remote branch cleanup (`git push origin --delete`) included in Version Control and Session Completion
  **QA**:
  - Happy: Read AGENTS.md, trace full cycle: issue → branch → commit → push → PR → CI → merge → cleanup → handoff
  - Failure: `grep "push.*master\|push.*main" AGENTS.md` returns no matches outside of the conservative profile's "Do not push to master" text
  - Profile check: `grep -A5 "Conservative (default)" AGENTS.md` shows updated text allowing branch pushes
  - Beads check: `grep "bd dolt push" AGENTS.md` returns at least 1 match (preserved in Session Completion)
  **Commit**: `docs: update AGENTS.md for PR-based workflow`

- [x] 4. Adapt `.githooks/pre-push` for PR workflow (remove master CI check, add master/main push guard, fix stdin)
  **References**: `.githooks/pre-push` lines 1–11 (header), lines 15–28 (CI status check on master), lines 30–113 (quality gates + issue ref checks)
  **Changes**:
  - Remove the CI status check block (lines 15–28): with PRs, CI runs on the PR branch, not triggered by push to master
  - Add master/main push guard AND issue reference validation in a SINGLE merged stdin loop — critical: pre-push hook receives stdin as a pipe; two separate `while read` loops would race and the second would get zero input. The guard check and issue-ref validation MUST share one loop:
    ```bash
    # --- Validate refs: guard master/main + check issue references ---
    while read -r local_ref local_oid remote_ref remote_oid; do
        # Skip branch deletion
        if [ "$local_oid" = "0000000000000000000000000000000000000000" ]; then
            continue
        fi

        # Guard: reject direct pushes to master or main
        if [ "$remote_ref" = "refs/heads/master" ] || [ "$remote_ref" = "refs/heads/main" ]; then
            echo "ERROR: Direct pushes to $remote_ref are disabled."
            echo "  Use feature-branch + PR workflow instead:"
            echo "    git checkout -b feat/desc"
            echo "    git push origin feat/desc"
            echo "    gh pr create --base master"
            echo ""
            has_error=1
            continue
        fi

        # Determine commit range for issue-ref check
        if [ "$remote_oid" = "0000000000000000000000000000000000000000" ]; then
            range="$local_oid"
        else
            range="${remote_oid}..${local_oid}"
        fi

        # Extract issue references from all commits in the range
        issues=$(git log "$range" --format="%B" 2>/dev/null \
            | grep -ioE 'ref[[:space:]]+#[0-9]+' \
            | grep -oE '#[0-9]+' \
            | sort -u \
            | tr -d '#' \
            || true)

        if [ -z "$issues" ]; then
            continue
        fi

        for n in $issues; do
            state=$(unset GITHUB_TOKEN 2>/dev/null; gh issue view "$n" --repo qiboda/compass --json state --jq '.state' 2>/dev/null || echo "MISSING")

            if [ "$state" != "OPEN" ]; then
                echo ""
                echo "ERROR: push rejected — issue #$n is $state (must be OPEN)."
                echo ""
                if [ "$state" = "MISSING" ]; then
                    echo "  Issue #$n does not exist. Create it with 'gh issue create'."
                else
                    echo "  Issue #$n is $state. Reopen it with 'gh issue reopen $n'."
                fi
                echo ""
                has_error=1
            fi
        done
    done
    # Note: Closes #N is expected in the PR body, not validated by this hook.
    ```
  - Keep all quality gates (they don't read stdin): cargo fmt (lines 32–38), cargo clippy (lines 41–48), cargo doc (lines 51–58)
  - Update header comment (lines 1–10) to reflect PR workflow — remove "0. Latest CI on master must be passing", add "0. Guard against direct pushes to master/main"
  **Acceptance criteria**:
  - No CI status check for master branch in pre-push hook
  - Direct pushes to BOTH `refs/heads/master` AND `refs/heads/main` are rejected
  - Guard check and issue reference validation share a single stdin loop (no race)
  - fmt, clippy, doc checks remain active
  - `bash -n .githooks/pre-push` passes syntax check
  **QA**:
  - Happy: Run `bash -n .githooks/pre-push`, verify exit code 0
  - Failure: `grep "gh run list.*master" .githooks/pre-push` returns no matches
  - Guard test: `grep "refs/heads/master.*refs/heads/main" .githooks/pre-push` returns both refs in the guard
  - Stdin race test: verify only ONE `while read -r local_ref` loop exists (grep count = 1)
  **Commit**: `chore: adapt pre-push hook for PR workflow (remove master CI check, add master/main push guard, fix stdin race)`

- [x] 5. Create `.github/PULL_REQUEST_TEMPLATE.md`
  **References**: None (new file)
  **Content**:
    ```markdown
    ## Summary

    <!-- Brief description of what this PR does -->

    ## Checklist

    - [ ] Tests pass (`cargo test`)
    - [ ] Clippy clean (`cargo clippy -- -D warnings`)
    - [ ] Formatting clean (`cargo fmt --check`)
    - [ ] Docs warning-free (`cargo doc --no-deps`)
    - [ ] `kb/` docs updated if behavior/API/data/conventions changed
    - [ ] Reflection appended to `kb/dev/reflections.md`
    - [ ] PR targets `master` branch

    ## Related Issue

    Closes #N

    ## Verification

    <!-- How did you test this? What evidence (screenshots, logs, test output)? -->
    ```
  **Acceptance criteria**:
  - File exists at `.github/PULL_REQUEST_TEMPLATE.md`
  - Contains Summary, Checklist (7 items minimum), Related Issue, Verification sections
  - `Closes #N` placeholder present
  **QA**:
  - Happy: Open file, verify all 4 sections present and checklist items are checkable `[ ]`
  - Failure: File missing or missing `Closes #` placeholder
  **Commit**: `docs: add PR template`

---

## Final verification wave

- [x] F1. Plan compliance audit — verify all 5 todos executed, all acceptance criteria met
- [x] F2. Cross-document consistency — `AGENTS.md`, `kb/dev/process.md`, SKILL.md all describe the same PR workflow without contradiction
  **Evidence**: `grep -rn "master\|main\|trunk" AGENTS.md kb/dev/process.md .opencode/skills/compass-workflow/SKILL.md` — all target branch references are `master`, no trunk-based mentions
- [x] F3. Workflow walkthrough — simulate a full cycle mentally: `git checkout -b feat/test` → commit with `ref #1` → push branch → `gh pr create` (body: `Closes #1`) → CI passes → manual squash merge → issue auto-closes → `git branch -d feat/test`
  **Evidence**: Every step has a corresponding command or instruction in the docs
- [x] F4. Scope fidelity — verify no unintended changes: CI unchanged, commit-msg hook unchanged, Beads config unchanged

---

## Commit strategy

5 commits total, one per todo, pushed to a single PR branch:

```
feat/pr-workflow (branch)
  ├── docs: switch to PR-based workflow in kb/dev/process.md
  ├── docs: update compass-workflow skill for PR-based workflow
  ├── docs: update AGENTS.md for PR-based workflow
  ├── chore: adapt pre-push hook for PR workflow (remove master CI check)
  └── docs: add PR template
```

Commit message format: `docs:` or `chore:` (no `ref #N` required — workflow change is self-referential).

---

## Success criteria

1. 开发者能通过阅读 `kb/dev/process.md` 独立完成一次完整的 feature-branch → PR → merge 流程
2. `compass-workflow` SKILL 加载后，agent 在当前会话中按新工作流执行
3. Pre-push hook 不再因 master CI 状态阻挡分支 push
4. PR 模板在创建新 PR 时自动填充
5. 所有文档中目标分支统一为 `master`
