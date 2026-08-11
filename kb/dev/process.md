# 开发流程

## Issue 驱动工作流

功能与 Bug 修复的完整开发循环：

```
User raises requirement
  →  OpenCode grills (/grill-me) to clarify scope and decisions
  →  Shared understanding reached → summarize locked-in decisions
  →  OpenCode creates GitHub issue (feature_request or bug_report template)
  →  OpenCode shows issue with gh issue view <N>
  →  /ulw-plan (if multi-step)  →  plan may identify sub-issues for epic decomposition
  →  For epics: /skwy-github-workflow creates epic + sub-issues upfront, batches by DAG
  →  implement (each sub-issue walks GATE independently)
  →  cargo nextest (tests must pass)
  →  commit with ref #<sub-N> (epic) or ref #N (single issue)
  →  ai-review (/review-work) — per sub-issue + pre-PR
  →  user confirms push → /skwy-reflect (write reflection commit) → push master (one PR with all commits)
  →  CI passes  →  batch close sub-issues + epic with gh issue close
```

> 反思 commit 在用户确认 push 后、执行 push 前提交（ref #119 教训：合并后
> 再写反思会撞上已关闭 issue 的 commit-msg hook 限制，只能摘 patch 直推）。

文档、lint 修复和错别字跳过 grill-me + issue 环节 — 直接实施。

| 工作类型 | 是否需要 Issue？ |
|---|---|
| 功能 | ✅ 需要 |
| Bug 修复 | ✅ 需要 |
| 重构 | ✅ 需要 |
| 文档更新 | ❌ 跳过 |
| Lint / 错别字 | ❌ 跳过 |

### Epic & Sub-Issue 工作流

大型需求通过 GitHub 原生子 issue 支持（`gh issue create --parent <epic-N>`）分解为
**epic**（父 issue）与 **sub-issues**（子 issue）。

核心规则：epic 创建/批次执行/关闭的完整流程见
`~/.config/opencode/skills/skwy-github-workflow/SKILL.md`。要点：

- Epic + sub-issues 在规划时一次性创建（`/ulw-plan` 识别、`/skwy-github-workflow` 批量创建）
- `.omo/plans/<epic>.md` 以 `pending | in_progress | done` 表跟踪状态
- 子任务按依赖 DAG 分批次，批次切换需**人工确认**
- 一个 epic 一个 PR，每个 sub-issue 一个 commit（`ref #<sub-N>`），regular merge
- 每个 sub-issue 独立走 GATE；合并后先关 sub-issues 再关 epic

### 当 OpenCode 发现新 Bug 时

1. 使用 `.github/ISSUE_TEMPLATE/bug_report.md` 模板创建 issue
2. 回读确认（`gh issue view <N>`）issue 已存在

**PR 内的 bug 不建独立 issue。** PR 未合并前，属于该 PR 内容范围的问题
（实现缺陷、冒烟测试发现的问题）直接在 PR 内修复，commit 引用 PR 对应的
epic/issue（`ref #<N>`），不创建新 issue；issue 收尾时在完成 comment 中
一并记录。仅当问题独立于 PR 范围、或 PR 已合并后才走上述正常 issue 流程。
3. 修复 — 带 `ref #N` 提交

### 提交 → Issue 关联

| 提交类型 | Issue 引用 |
|---|---|
| feat / fix（单个 issue）| `ref #N` |
| feat / fix（epic 子 issue）| `ref #<sub-N>` |

Issue 在验证后通过 `gh issue close N` **手动**关闭。
不要使用 `fixes #N` 或 `closes #N` — 这些会在 push 时自动关闭 issue。
Epic 工作时，先批量关闭所有子 issue，再关闭 epic。

### Commit-msg 钩子

Git 钩子（`.githooks/commit-msg`）强制执行 issue 引用：

```
Every commit must include "ref #N" — no exceptions.
feat, fix, test, refactor, docs, chore — all included.
```

钩子通过 `git config core.hooksPath .githooks` 激活（已配置）。

**OPEN 校验方式（ref #213）**：commit-msg 与 pre-push 不再逐 issue 调
`gh issue view`（无界 API 调用引发限流误报），改为**单次批量查询**：

```bash
open_set=$(unset GITHUB_TOKEN 2>/dev/null; gh issue list --repo qiboda/compass \
    --state open --json number --limit 5000 --jq '.[].number' 2>/dev/null || echo "GH_FAIL")
```

一次拉取全部 OPEN issue 号后，用 `echo "$open_set" | grep -qx "$n"` 本地查集。
**fail-closed 语义**：`gh issue list` 失败（GH_FAIL）或返回空集时拒绝 commit/push。
`--limit 5000` 覆盖仓库全部 OPEN issues（当前 ~17 个，余量充足）；若未来超限需
重新评估分页策略。行为测试见 `scripts/tests/gh-issue-list-test.sh`（fake gh 注入
PATH，精确断言命令参数与 OPEN 判定）。

## OpenCode 工作流

完整流程由 **`skwy-workflow` skill** 强制执行（plan → gate → test-first →
per-step verify → commit → review → push）。以下是 skill 未覆盖的本地细节：

### Pre-push hook 检查（`.githooks/pre-push`）

push 前按顺序执行：

0. **Rebase base 分支**：`git fetch origin <base>` → `git log HEAD..origin/<base>` 非空时 `git rebase origin/<base>`，解决冲突后再继续。分支必须基于最新 base 才能 push（避免携带过期 base 的提交）。
1. **cargo fmt --check**
2. **cargo clippy -- -D warnings**
3. **cargo doc --no-deps**（必须无警告）
4. **Issue 引用**：`ref #N` 必须**独立成行**且指向 open issues（行内 `ref #N` 视为叙述性提及，不参与校验，ref #211）
5. **Python 检查**（若存在 `collectors/pyproject.toml`）：`uv run ruff check *.py tests/` + `uv run pytest tests/ -q`

> **CI 门槛在 merge 侧，不在 push 侧（ref #172）**：master 的 branch protection
> 强制 4 个 required status checks（Rust (fmt + build + clippy + docs + nextest +
> coverage)/Bench (compile)/Python Lint/Python Test，strict=true，ref #194 合并
> 6→2 job 后同步）——
> PR 的 CI 未全绿 merge 按钮直接禁用。pre-push hook **不再检查 master CI 状态**
> （曾导致死锁：master CI 失败时，修复它的 PR 无法 push）。branch protection
> 只限制 merge，不拦 master 直推（docs/lint/typo/反思类直推照常，未启用
> enforce_admins）。

> **覆盖率门禁**在 CI 执行（coverage job 强制 Rust workspace ≥80% + per-crate 阈值——数据层 data/core 95%、其余 80%；Python ≥95%），太慢不适合 pre-push 本地检查。见 `kb/dev/testing.md` 覆盖率章节。

手动 pre-push checklist（与 hook 相同）：`git fetch origin <base>` + rebase 落后 commits + `cargo fmt --check` + `cargo clippy -- -D warnings`
+ `cargo doc --no-deps` + `ref #N` 指向 open issues，全部通过才能 push。

### 文档注释纪律

`compass-core` 中新增或修改的每个 `pub` 项 MUST 包含 `///` 文档注释。
这由 `#![warn(missing_docs)]` 强制执行 — `cargo doc --no-deps` 必须无警告。

## Git 分支

**Feature-branch 工作流。** 大部分工作在 feature 分支上进行，通过 PR 合并。
简单修复（错别字、配置、单行改动）可以直接推到 master。

```
master  ●──●──●──●────────●  (trunk)
              \          /
feat/xxx       ●──●──●──┘   (feature branch, PR, merge)
```

**合并策略**：使用常规 merge（非 squash）。保留所有提交历史 —
每个提交映射到一个 issue 引用（`ref #N`），丢失这种粒度会破坏可追溯性。

### 目标分支修复工作流

当 CI 失败在某个 **feature/PR 分支**（而非 master）时，修复不必经 master 中转——
直接从目标分支切修复分支，修复后合并回目标分支，各 PR 互不阻塞：

```sh
# 1. 从目标分支切修复分支（复用 /skwy-worktree skill）
git worktree add -b fix/<desc> .worktrees/<name> <target-branch>

# 2. 修复 + commit（ref #N）

# 3. 合并回目标分支（cherry-pick 单 commit 修复，推荐）
git checkout <target-branch> && git cherry-pick <fix-commit-sha>

# 4. 目标分支直接 push（不经 PR 到 master）——目标分支自身的 PR 负责最终合并
git push origin <target-branch>
```

**实例**：`fix/ci-fix-issue-only`（PR #88）CI 因 flaky 聚合测试失败（#75），
修复 commit `175cc80`（fix/deterministic-aggregation-test 分支）直接
cherry-pick 进目标分支 → push → PR #88 CI 变绿，无需等 master 先合并。

**何时用**：修复只影响目标分支的 CI/测试，且目标分支本身有独立 PR 进 master。
**何时不用**：修复属于 master 级 bug 且目标分支即将废弃——此时直接修 master。

### PR 合并工作流

PR 合并后、关闭关联 issue 前，在该 issue 上添加评论，注明实际变更与 PR 描述
之间的任何偏差：

- 哪些实现与 PR 描述不同
- 哪些被省略或推迟
- 哪些计划外的变更被包含

这确保 issue 作为实际交付内容的准确记录。

```
gh issue comment <N> --body "PR #M 已合并。与 PR 描述不一致之处：
- ..."
```

## Worktrees（临时 PR 工作空间）

Worktrees 位于 `.worktrees/<name>/`（gitignored）。每个 worktree 是一个
**临时 PR 工作空间**，为单个 PR 或 epic 创建，合并后清理。
分支命名：`feat/<short-description>` 或 `fix/<short-description>`。

**创建时机（强制，ref #138）**：需求经 grill-me 确认需要 worktree 时（feature/epic、
2+ 模块、将产出 `.omo/plans/*.md` 或 `.omo/designs/*.md`），**grill 共识达成后立即
创建并切换**——plan/design 等 .omo 产出文件直接在 worktree 内创建，随实现 PR 提交。
**禁止**在 master 工作区先产出 plan/design 再迁移：git worktree 是独立 checkout，
master 工作区的 untracked 文件不会出现在 worktree 中（SEPA 教训：全程在 master 规划
导致 plan/design 成 untracked、需手动迁移）。

**加载 `/skwy-worktree` skill 获取完整流程****（创建、post-creation MANDATORY 步骤、
handoff 移交、自动启动区域、`--close` 退出清理、合并后清理）。
主 session 创建 worktree 后仅需写 handoff（用途 + issue URL + 已锁定决策），
剩余工作全部由 worktree 内 agent 自主完成。

`--close <name>` 停止 cwd 指向该 worktree 的 opencode 进程、关闭其承载终端窗口，
然后移除 worktree 与分支。**当该 worktree 存在运行中的持有进程**（包括从 worktree
自身内部执行时的调用者——例如在该 worktree 的 opencode 会话里运行 `--close`），
清理会交给一个 `setsid` 脱离会话的子进程完成（`logs/open-worktrees-close.log`），
因此调用者被终止后清理仍会执行完毕（ref #104）。终端窗口关闭对每窗口终端
（kitty/xterm/konsole）可靠；对 client-server 终端（gnome-terminal）为尽力而为，
xfce4-terminal 因单实例守护进程（进程名即 xfce4-terminal）不尝试关闭，避免误关所有窗口。

**`--close` 从 worktree 内部执行（ref #205）**：脚本通过 `git rev-parse --git-common-dir`
定位主仓库根（`resolve_project_root()`），不依赖 `$0` 相对路径——从 worktree 内
以 `bash ~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh --close <name>`
调用也能正确解析主仓库。脚本已随 skwy-worktree 技能放全局（单一来源，无副本同步
问题——全局副本即最新版，无需合并 master/重新同步）。

**为何不用 plugins**：评估了 `opencode-worktree` 插件（kdco worktree 插件 via OCX），
发现存在阻塞性问题（无法幂等地重新打开、终端启动不可靠、无法重新打开 session）。
手动 worktrees + `/skwy-worktree` skill 提供了完全的控制，避免了这些问题。

## 版本控制

```sh
git add <files>              # stage only intended changes
git commit                    # uses .gitmessage template
git push origin main          # triggers CI
```

### `.omo/plans/` 必须提交（git 跟踪规则）

`.gitignore` 排除 `.omo/*` 但**例外保留 `!.omo/plans/`**——计划文件目录由 git
跟踪。每个 epic/feature 的计划文件（`.omo/plans/<name>.md`）**必须随实现
提交**（docs 类 commit），作为计划-执行-交付的权威跟踪记录。不要因 `??`
状态误判为"gitignored 工作产物"——`??` 仅表示未 add，需查 `.gitignore`
规则区分"待提交"与"被忽略"。

## 快速开始

```sh
scripts/run.sh                  # one-command launch of the GUI app (foreground)
cargo run --bin compass         # manual equivalent (needs X11/Wayland)
cargo run --bin compass-data -- <subcommand>  # data pipeline CLI
RUST_LOG=debug scripts/run.sh   # verbose logging
```

### CLI（compass-data）

完整子命令参考见 `kb/user/cli.md`。速查：

```sh
cargo run --bin compass-data -- import                    # Dolt investment_data → Parquet（全量直写）
cargo run --bin compass-data -- import --since 20260725   # ⚠️ 日期过滤直写：覆盖全文件，非追加（慎用，见 kb/dev/toolchain.md）
cargo run --bin compass-data -- import-compass --table stock_basic  # Dolt compass_data → Parquet
cargo run --bin compass-data -- export                    # Parquet → DuckDB
cargo run --bin compass-data -- backup                    # Parquet → 百度云
```

## 添加功能（手动）

如果不使用 OpenCode：

1. **探索**相关源文件（布局见 `kb/design/architecture.md`）。
2. **测试先行**：在 `#[cfg(test)] mod tests` 中编写失败的测试。
3. **实现**在源文件中。
4. **验证**：`cargo nextest run` + `lsp_diagnostics`。
5. **更新文档**如果变更影响架构、符号格式或配置。

### 知识库同步

每个影响行为、API、数据结构、配置、工作流或惯例的代码变更，必须在同一
commit 中更新相关 `kb/` 文件。如果架构概览发生变化，必须更新 AGENTS.md。

权威的「变更类型 → kb/ 文件」映射表见
`~/.config/opencode/skills/skwy-workflow/SKILL.md` 内嵌「文档同步」章节（变更 → kb/ 映射表由项目自身定义）。

### 文档惯例

**kb/design/ 文件必须使用叙事性的、面向开发者入门风格。**
新接触项目的读者应该不仅理解 _是什么_，还要理解 _为什么_。
每个设计决策必须附有其理由：它解决的问题、考虑过的替代方案、接受的权衡取舍。

**API 参考属于 `cargo doc`，而非 kb/。**
在公开类型、trait 和函数上使用 `///` 文档注释。
`kb/design/` 解释设计意图和架构；`cargo doc` 处理精确的 API 界面。
两者互补 — kb/ 讲述故事，rustdoc 提供参考。

**绝不在 kb/ 中硬编码版本号。** `Cargo.toml` 是依赖版本号的唯一可信来源。
kb/ 文档可以提及 crate 名称及其用途，但不能出现 `= "0.25"`。

**AGENTS.md 是索引，不是重复。** 它以一行摘要指向 kb/ 文件。
完整解释位于 kb/ 中，绝不在 AGENTS.md 中重复。

## TDD 工作流

功能与 Bug 修复工作遵循 TDD（测试驱动开发）：

```
DESIGN TESTS → RED → GREEN → REFACTOR
```

0. **DESIGN TESTS**：编写**测试用例文档**（测试模块内的注释块或单独的
   `#[doc]` 块），列出测试必须覆盖的所有场景：

   ```
   // Test cases:
   // 1. Normal input — returns expected result
   // 2. Empty input — returns empty/default
   // 3. Boundary values — min/max handled correctly
   // 4. Error paths — invalid input produces proper error
   // 5. Edge cases — null/missing fields, very large values, etc.
   ```

   这确保测试覆盖是全面的，防止盲点 bug。
   测试用例列表作为检查清单 — 在实现被认为完成之前，每一项必须至少有一个
   对应的 `#[test]` 或 `#[case]`。

1. **RED**：编写一个描述预期行为的失败测试。
   - 测试必须在任何实现代码存在之前失败。
   - 如果立即通过，删除或重写 — 它什么都没测试。
   - 验证测试用例文档中的每个场景都被覆盖。
2. **GREEN**：编写最小化的实现使测试通过。
3. **REFACTOR**：清理代码，同时保持测试通过。

探索性变更（新 API 集成、架构实验）可以在实现后编写测试以锁定行为。

## 运行测试与代码质量

测试运行、benchmark、Tracy profiling 见 `kb/dev/testing.md`。速查：

```sh
cargo nextest run                       # 推荐
cargo fmt --check                       # verify formatting
cargo clippy -- -D warnings             # strict lint
```

### 数据管线验证前环境预检（磁盘 + 小样本，ref #202/#136）

数据管线验证/QA 曾两次因环境问题中断，浪费验证波次——验证开始前先做两项预检：

1. **磁盘预检**：大分区（/data）易被**并发编译进程**临时填满（进程结束后
   自动释放），导致 `cargo test`/`llvm-cov` 中途失败。验证前：

   ```sh
   df -h /data        # 确认可用空间充足（如 <1G 先等编译进程结束/清理）
   ```

   ref #202 实例：/data 分区 100%（64K 可用）中断 cargo test——根因是并发
   编译占用而非真实写满，进程退出后自动释放。

2. **大库小样本 QA**：18M+ 行 `investment_data` 真实库的 CLI 手动 QA 不要
   直接跑生产数据仓库——即使 `--limit 5`，某些路径（如 symbols 全量枚举）
   仍可能超 300s。改用**临时小 Dolt 库**验证同一二进制路径：

   ```sh
   # 临时小库：dolt init + 完整 schema fixture（少量行），
   # 跑同一 CLI 命令验证逻辑路径，避免真实大库超时
   ```

   ref #136 实例：真实 import 对 18M+ 行库跑 `--limit 5` 仍超 300s——改用
   临时小 Dolt 库后同一验证秒级完成。fixture 测试覆盖不到的数据级问题
   （重复行、格式、单位口径）用小样本真实数据冒烟，不依赖全量生产库。

### CI 缓存策略（rust-cache）

CI 的 `Swatinem/rust-cache@v2` 采用**仅 master save + 分组缓存**策略：

```yaml
- uses: Swatinem/rust-cache@v2
  with:
    save-if: ${{ github.ref == 'refs/heads/master' }}
    prefix-key: ${{ github.job }}   # 每组 job 缓存独立
```

- **master**：每次 push 更新缓存（key 含 `Cargo.lock` hash，锁文件不变则命中旧缓存）
- **分支**：只 restore（GitHub cache 自动 fallback 到默认分支命中 master 条目），不写自己的缓存——避免短命分支产生孤儿缓存（7 天 LRU 淘汰前无人复用）
- **锁文件变化**（如 PR 加依赖）：key 变 → miss → 全量编译（依赖集变了，缓存无意义）
- `save-if: false` 时 **restore 仍生效**（Swatinem/rust-cache 语义），仅跳过 save

**分组缓存（ref #194）**：6 个 Rust job 合并为 1 个 + bench 独立，缓存从
6 份（~10GB）降为 2 份（~3GB）：

- `rust`：fmt + build + clippy + docs + nextest + coverage 顺序执行（同一
  target 累积，save 一次）
- `bench-check`：独立（release profile）

组内顺序执行 → 无并行覆盖；组间独立 key → 无竞争。**禁止**所有 job 共享
同一 key（#14 事故：并行覆盖缓存 → 下一 run 恢复不兼容产物全部重编译）。
保持 rust-cache 默认 `add-rust-environment-hash-key`——cargo.lock 变化时
内置 restoreKey 前缀匹配仍能复用旧缓存增量编译。

历史：`572e688` 曾用 `save-if: true`（分支自缓存提速），因短命分支孤儿缓存浪费回退为仅 master save（`d55eead`, ref #89）。

## Config

Config 位于 `~/.config/compass/config.toml`，全部字段可选，缺省回退到
`crates/compass-core/src/model.rs` 中的默认值。完整选项见 `kb/user/config.md`。

## 日志

- Stderr：始终输出。`RUST_LOG` 控制级别（`error`、`warn`、`info`、`debug`、`trace`）。
- 文件：`logs/compass.log`（每日滚动）。

## 调试技巧

### 验证 kill/pgrep 类脚本的安全纪律（ref #104 事故教训）

调试或集成验证任何含 `kill`/`pkill`/`pgrep` 的脚本时，三条纪律缺一不可：

1. **`-f` 模式自指**：`pgrep -f 'opencode'` / `pkill -f 'pattern'` 会匹配**执行该命令的 shell 自身**
   （命令文本含 pattern 字样）——先列出再核对 pid，或用 `[x]` 技巧
   （`pgrep -f '[o]pencode'`）避免自匹配。杀到自己的 bash 会话 = 会话挂死。
2. **持久 shell 的 cwd 污染**：bash 工具的持久会话 `cd` 进 fixture worktree 后，
   cwd 检查（`readlink /proc/PID/cwd`）会把会话自身卷进 kill 范围。验证一律
   通过**脚本文件**运行（脚本内 `cd` 只影响自身进程），不在持久会话直接 `cd`。
3. **子代理委托**：委托 QA/集成验证 agent 执行 kill 类验证时，明确要求
   fixture 隔离（`/tmp` 路径）、禁止触碰真实 worktree/仓库、疑似危险命令改
   为只读 Oracle 复查。agent 误杀宿主 opencode 会话 = 用户工作区被破坏。

### 检测/结束 GUI 进程的正确姿势（ref #105 QA 复发教训）

ref #104 纪律**已写但未遵守**，导致 PR 合并后的 GUI 进程检测反复误判
（PID 飘移、假阳性、长链命令超时）。具体到 compass：

```sh
# ✅ 检测：-x 精确进程名（不 -f），或 [x] 技巧
pgrep -x compass                    # 只匹配进程名 compass
pgrep -f "[t]arget/debug/compass"   # [t] 破坏自匹配

# ✅ 结束：先列出核对，再精确杀
pgrep -x compass | xargs -r kill    # -r：无匹配时不报错
# 若在 tmux 内启动：tmux kill-session -t <name> 优先

# ❌ 反例（本 session 踩坑）
pgrep -f "target/debug/compass"     # 匹配 bash 自身 → PID 飘移假阳性
pkill -f "target/debug/compass"     # 可能杀掉执行 shell → 命令超时
```

**长链命令纪律**：`pkill; sleep; build; tmux new; sleep; pgrep` 串在一起时，
bash 工具超时（120s）中断会留下半启动状态，后续检测必然误判。启动/检测
**分步执行**：启动命令确认返回后立即结束；检测用独立短命令。
窗口可见性以 `wmctrl -l` / `xdotool search` 为准，进程存在 ≠ 窗口可见。
启动 GUI 用 `tmux new-session -d`（脱离 bash 工具生命周期），
不用 `setsid ... &`（与工具超时机制冲突）。

### 检查东方财富 API 返回内容

```sh
# K-line API
curl "https://push2his.eastmoney.com/api/qt/stock/kline/get?secid=0.000001&klt=101&fqt=1&beg=20250101&end=20250721&lmt=10&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61"

# Symbol listing API
curl "https://push2delay.eastmoney.com/api/qt/clist/get?pn=1&pz=3&fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048&fields=f12,f14&ut=bd1d9ddb04089700cf9c27f6f7426281"
```

### collectors（Python 数据管线）

从东方财富 API 抓取数据到 CSV，然后导入 `compass_data` Dolt。
命令与工作流见 `kb/user/cli.md` § Python collectors 与 `kb/design/architecture.md` § collectors。

核心概念：
- **curl_cffi** 实现 TLS 伪造（东方财富反爬虫）
- **CSV 作为中介**连接 API 与 Dolt
- **`.state.json`** 文件跟踪上次抓取状态以支持增量更新
- **`--resume`** 标志用于继续中断的抓取

### 百度云备份

`compass-data backup` 将 `parquet_data/` 打包为 zip 并通过 `baidupcs`
（BaiduPCS-Go）上传到百度云：

- 目标：`/compass/` 文件夹
- 格式：带时间戳的 zip（`parquet_data-YYYYMMDD-HHMMSS.zip`）
- 独立脚本：`scripts/upload-parquet.sh [--keep-zip]`

### Dolt 数据库查询与维护

Dolt 查询示例、investment_data 同步流程（pull → push skwy → import）、
compass_data 提交推送与数据库布局见 **`kb/dev/database.md`**（ref #157）。

### 重置一切

> **警告：以下命令会删除所有已导入的行情数据。** 执行前请确认已完成备份（`compass-data backup`）。

```sh
rm -rf /data/compass-data/parquet_data/   # 主 Parquet 数据
rm logs/compass.log                        # 日志
```
