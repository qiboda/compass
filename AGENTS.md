# AGENTS.md — compass

A-share 股票图表桌面应用（egui）。数据管线以本地 Dolt `investment_data` 为**主数据源**
（18M+ 行，6000+ 标的）。GUI 只读本地 Parquet 文件（DuckDB 查询），**无在线回退**。
Python collectors 抓取数据写入 Dolt（财务数据来自 EastMoney；stock_basic 来自三大交易所官网）。

**项目书** = 本项目所有规则与知识文件的统称，包括 `AGENTS.md` 和 `kb/` 目录下所有文件。

**默认对话语言：中文。** 所有回答、解释、讨论默认使用中文，代码注释和提交信息按惯例使用英文。

---

## 品质准则

精益求精，追求完美。每一行代码、每一次提交、每一个决策，都应以最高标准衡量。容不得将就、凑合、差不多。

- 代码不行就重构，不要留着凑合
- 设计不对就推翻，不要叠加补丁
- 流程有漏洞就堵，不要绕过去

---

## ⚡ GRILL-ME FIRST (ALWAYS)

**每次用户消息都必须先加载 `/grill-me` 再回应。** 无任何例外。

grill-me 访谈必须达到 "shared understanding reached" 才能进行任何其他操作——
包括读文件、分类请求、创建 todos、写代码。

**Grill-me 完成后 → 任何 feature 或 bugfix 工作必须进入下面的 PRE-IMPLEMENTATION GATE。
Grill-me 是第 0 步；gate 是第 1-4 步。不要因为 grill-me 已达成共识就跳过 gate。**

---

## 🛑 PRE-IMPLEMENTATION GATE (任何代码变更前必读)

**本 gate 适用于所有代码变更。** 唯一例外：
- 纯文档变更（typo、格式、补充说明）
- Cargo fmt / clippy 修复（CI 已覆盖）
- 注释或字符串中的 trivial typo

**除此之外的一切 —— feature、bugfix、重构、新命令、CI 变更、hooks、脚本、
依赖更新 —— 必须走 gate。**

动手改任何文件之前，向用户逐条 verbalize 以下步骤并确认完成：

| Step | 动作 | 所需证据 |
|---|---|---|
| **1. Issue** | 调用 `/issue-workflow` 创建/管理 issue | 向用户展示 issue URL |
| **2. Plan** | 涉及 2+ 模块时运行 `/ulw-plan` agent 直到批准 | `.omo/plans/*.md` 文件创建 + 用户批准 |
| **3. Tests** | 调用 `/test`（qa skill）写失败测试 | 测试失败输出 |
| **4a. Rustdoc** | 调用 `/rustdoc` 验证 `#![warn(missing_docs)]` 合规 | `cargo doc --no-deps` 无警告 |
| **4b. Docs** | 调用 `/docs` 确定哪些 `kb/` 文件需更新 | 向用户列出文件清单 |
| **4c. 决策记录** | 检查相关 `kb/design/` 文件是否含 `## 决策记录` 章节 | 缺失则补齐后再继续 |

**任何一步未完成即 STOP。不实现。不创建 todos。不改文件。**

### SELF-CHECK（强制 —— 每次代码编辑前问自己这 4 个问题）

1. **"这项工作有 GitHub issue 吗？"** — 没有就 NOW 创建。
2. **"我的 commit message 包含 `ref #N` 吗？"** — 没有就加。
3. **"我先写了失败测试吗？"** — 没有就先写再实现。
4. **"我更新了相关 kb/ 文件吗？"** — 没有就确定文件并更新。

这 4 个问题不是可选的。它们是最低标准。跳过任何一个就是违反工作流。

**Test-first 不可妥协**：任何 bugfix 或 feature 变更必须从能复现问题的失败测试开始
（RED），再做让它通过的修复（GREEN）。适用于 Python（`collectors/tests/`）、
Rust（`#[cfg(test)]`）以及本仓库所有语言。先写修复再写失败测试是反模式 ——
见 `kb/dev/reflections.md` 历史摩擦记录章节（test-first 教训）。

### HARD BLOCK

本 gate 不可妥协。加载 `compass-workflow` skill 时会再次提醒此 gate。
如果发现自己没完成这些步骤就在写代码，即违反工作流——立即停止，
`git stash` 或 revert，回到第 0 步。

**流程违规本身就是 bug。** 跳过 gate 的工作无论代码质量如何都是不完整的。
在 reflections 中记录违规。

### 实现后：Reflection Record

每次 feature/bugfix 完成后，调用 `/reflect`（reflect skill）写事后反思，
追加到 `kb/dev/reflections.md`。

这是强制要求 —— 与实现一起提交或紧随其后。

---

## Workflow (MANDATORY)

所有 **feature** 和 **bugfix** 工作 MUST 加载 `compass-workflow` skill。
它强制执行：issue 驱动开发、doc-sync、test-first、分步验证、commit 纪律。

**加载 skill 后**：立即按上面的 PRE-IMPLEMENTATION GATE 检查清单走一遍，一步不跳。

### Available Skills

| Skill | Slash Command | 用途 |
|---|---|---|
| `compass-workflow` | `/compass-workflow` | 强制执行 issue 驱动开发、doc-sync、test-first、分步验证、commit 纪律 |
| `issue-workflow` | `/issue-workflow` | 创建和管理 issues（单 issue + epic/sub-issue 分解与批量关闭） |
| `worktree` | `/worktree` | 管理 PR 开发的 git worktrees（创建/删除/启动区域） |
| `qa` (test) | `/test` | 编写单元/集成测试（TDD/BDD）、测试覆盖 |
| `rustdoc` | `/rustdoc` | 验证 `#![warn(missing_docs)]` 合规 |
| `docs` | `/docs` | 根据代码变更识别并更新 `kb/` 文件 |
| `reflect` | `/reflect` | 写事后反思（含 User corrections + 趋势分析） |
| `product` | `/product` | Sprint 候选分析（只读，milestone 提议） |

所有 skill 位于 `.opencode/skills/<name>/SKILL.md`。OpenCode 从文件系统
自动发现 skill —— 无需注册。

### Epic & Sub-Issue Workflow

跨多模块的大型需求分解为 **epic**（父 issue）+ **sub-issues**（子 issue）
（GitHub 原生 sub-issue）。关键规则：一个 epic = 一个 PR（每个 sub-issue 一个
commit，`ref #<sub-N>`）、一个 worktree、按依赖 DAG 分批处理（手动切换批次）、
合并后批量关闭。计划文件（`.omo/plans/<epic>.md`）跟踪状态。

完整子 issue 生命周期见 `.opencode/skills/issue-workflow/SKILL.md`。

### Issue-Driven Commits

**每个 commit 必须引用 GitHub issue。** 无例外 —— 包括 chores、docs、scripts。
pre-push hook 拒绝没有 `ref #N` 的 commit。

epic 工作的每个 commit 引用其子 issue（`ref #<sub-N>`）。

```
feat: add thing

ref #26
```

### Commit → Review (MANDATORY)

每次 commit 后必须 review。无例外。

1. **Commit**: stage 变更、写含 `ref #N` 的描述性消息、commit。
2. **Review**: 对已提交变更运行 `/review-work`（5 个并行 agent：goal、quality、security、QA、context）。
3. **Fix**: review 发现问题就修复并重新 commit（最多 2 轮）。

Docs、lint 修复、typo、trivial chores 可跳过。

### Commit & Push

Commit 和 push 是**两个独立操作**。不要用 `&&` 串联。

**Commit**: 直接执行，不需要向用户申请确认。提交是 agent 的职责，按流程 commit 后自动 review。

**HARD BLOCK: Never auto-push.** 等用户明确说 "push" / "推送" 才 push。
**Follow the user's exact words.** "commit" 只表示 commit；"push" 只表示 push。

完整 push gate 清单见 `kb/dev/process.md`。

### Issue Lifecycle

**HARD BLOCK: 只在 push 后关闭 issue。** issue 只有在修复到达
`origin/master` 后才算 "done"。commit 后不要关闭 —— 等 push 成功。

**Epic close**: PR 合并到 master 后，先关闭所有 sub-issues，再关闭 epic。
在 epic 上记录总结 comment 列出所有完成的 sub-issues。

完整 issue lifecycle 见 `kb/dev/process.md`，Bevy 风格 A-/C-/D-/P-/S- 标签
分类见 `kb/github/labels.md`。最低要求：一个 A- 和一个 C- 标签。

### Scope Discipline

**绝不静默改变已计划的方案。** 如果外部约束（库 bug、API 不兼容、缺 crate）
阻塞了已确认的实现方案，不要通过改变 feature 设计来绕过。
向用户提出该问题并请求决策。

grill-me 决策和已批准的 plan 构成契约。任何偏离 —— 即使是务实的 workaround ——
都需要用户先批准。

---

## Sprint 规划

使用 GitHub Milestones 进行每周 sprint 管理（周一规划 / 周日回顾，周末为核心开发窗口）。
`product` skill 每周一扫描代码库和 open issues，提出 3-5 个候选需求；`/product brainstorm`
可随时手动触发。Sprint 节奏由 `compass-workflow` skill 的 Sprint Rhythm 规则强制执行。

## 摩擦记录（并入反思）

任何「AI 行为偏差被用户纠正」的场合（grill-me 分歧、执行方向偏离、意图误解、约束遗漏等），
在写事后反思时记录到 `reflections.md` 条目的 **User corrections** 小节
（自动检测：用户纠正时提示是否记录；随 `/reflect` 一并写入）。
`friction.md` 机制已移除（2026-08-01）——历史摩擦条目见 `reflections.md` 末尾
"历史摩擦记录"章节。

## 决策记录

所有 `kb/design/` 下的设计文档 MUST 包含 `## 决策记录` 章节，自包含地记录
关键设计决策的 **what + why + why-not**。

- **格式**: 表格 `| 决策 | 选项 | 选择 | 理由 | 排除原因 |`
- **保障**: `compass-workflow` PRE-IMPLEMENTATION GATE Step 4c 检查是否存在
- **自包含**: 决策记录不依赖外部引用（如 friction.md），所有理由直接写在设计文档内

---

## Worktrees

PR 开发使用 git worktrees，位于 `.worktrees/<name>/`（gitignored），每个 worktree 对应
一个 PR/epic，合并后清理。创建后执行 `/handoff` 并运行 `scripts/open-worktrees.sh` 自动
启动工作树区域（探测默认终端 + setsid 脱离进程组，无需手动解绑当前 session）。
**加载 `worktree` skill 获取完整流程**（含 post-creation MANDATORY 步骤与清理）。

## Knowledge base

详细文档在 `kb/` 下，按四部分组织。**AGENTS.md 是索引，不是重复** — 细节只在 kb/ 中，绝不在此复制。

| 文件 | 内容 |
|---|---|
| `kb/design/architecture.md` | 系统总览、crate 关系、线程模型、数据管线、存储策略、库选型 |
| `kb/design/data-providers.md` | Provider trait 体系、DuckDbProvider/ParquetReader、错误处理、DDL |
| `kb/design/symbols.md` | A 股市场分段、符号约定、交换所推断、timeframe 映射 |
| `kb/dev/testing.md` | rstest + tokio::test 模式、内存 DuckDB、Dolt 测试库、benchmark/Tracy |
| `kb/dev/process.md` | 开发流程、命令、配置、调试、Dolt 操作、重置 |
| `kb/dev/reflections.md` | 事后反思 — 做了什么、哪里出错、教训 + 历史摩擦记录（User corrections） |
| `kb/user/index.md` | 用户总览 — Compass 是什么、快速开始、前置条件 |
| `kb/user/gui.md` | 图表应用 — 界面、控件、数据流、股票代码 |
| `kb/user/cli.md` | 数据管线 — import/import-compass/export/backup、工作流、排障 |
| `kb/user/config.md` | 配置参考 — 全部选项、默认值、示例 |
| `kb/github/labels.md` | Issue/PR 标签分类 — Bevy 风格 C/A/D/P/S 前缀 |
| `kb/github/comments.md` | 评论规范 — 永远追加，绝不修改 |
| `kb/github/ask.md` | GitHub bot 角色 — /ask 只读问答（工作流按路径加载，勿改） |
| `kb/github/fix.md` | GitHub bot 角色 — /fix 修 bug（工作流按路径加载，勿改） |
| `kb/github/impl.md` | GitHub bot 角色 — /impl 实现功能（工作流按路径加载，勿改） |
| `kb/github/pr-review.md` | GitHub bot 角色 — /review 代码审查（工作流按路径加载，勿改） |
| `kb/github/ci-fix.md` | GitHub bot 角色 — CI 失败诊断（工作流按路径加载，勿改） |

## Setup

- **Rust edition 2024** — 需要 Rust ≥1.85。当前：1.96。
- **GUI app** — 需要显示服务器（X11/Wayland）。`cargo run` 打开窗口。
- 日志写入 `logs/compass.log`（每日轮转）。
- 配置在 `~/.config/compass/config.toml`（缺省回退默认值）。见 `kb/user/config.md`。

## Commands

```sh
cargo build
cargo run                    # GUI 图表窗口
cargo run --bin compass-data -- <subcommand>  # 数据管线 CLI
cargo test                   # 单元 + 集成测试
cargo fmt
cargo clippy
RUST_LOG=debug cargo run     # 详细日志
```

### compass-data CLI 速查

```sh
cargo run --bin compass-data -- import                    # Dolt investment_data → Parquet（全量）
cargo run --bin compass-data -- import --since 20260725   # 增量
cargo run --bin compass-data -- import-compass --table stock_basic  # Dolt compass_data → Parquet
cargo run --bin compass-data -- export                    # Parquet → DuckDB
cargo run --bin compass-data -- backup                    # Parquet → 百度云
```

`import-compass`/`export` 默认 merge/skip，`--overwrite` 覆盖；`import` 总是全量直写。
完整选项见 `kb/user/cli.md`。

## Architecture & Data providers

- **架构**: `kb/design/architecture.md` — 线程模型、数据管线、schema、源码布局、库选型
- **数据提供者**: `kb/design/data-providers.md` — DuckDB、Dolt、ParquetReader、DataError
- **符号约定**: `kb/design/symbols.md` — 市场分段、交换所推断、timeframe 映射

**Priority**: Dolt `investment_data` (local) 是主数据源。GUI 数据访问全部本地 — 无在线回退。

### compass_data Dolt 仓库 — 每次数据变更后 commit & push

`/data/compass-data/compass_data` 是 Dolt 仓库（remote:
`doltremoteapi.dolthub.com/skwy/compass_data`）。**每次数据修改**（import、re-import、
schema 变更、data_updates 更新）都必须提交并推送到 remote：

```sh
cd /data/compass-data/compass_data
dolt add <table>...        # or `dolt add .`
dolt commit -m "feat: ..." # describe the data change
dolt push origin main
```

完整 Dolt 操作指南（含跨库查询示例）见 `kb/dev/process.md#dolt-database-queries`。

## Parquet schema & Config

- **Parquet 主数据库结构** 与 **DuckDB DDL**: `kb/design/data-providers.md`（Schema 章节）
- **配置参考**: `kb/user/config.md`（全部选项 + 默认值 + 示例）

## Testing

见 `kb/dev/testing.md` — rstest + tokio::test 模式、内存 DuckDB、Dolt 测试库、benchmark、Tracy 分析。

**覆盖率门槛（CI 强制，低于阈值 CI 失败）**：Rust workspace 总 + 每 crate（compass-core / compass-data / compass）各自行覆盖率 ≥80%（`cargo llvm-cov --json` + `scripts/check-coverage.sh` 校验）；Python collectors `--cov=.` 全量计入 ≥80%（`--cov-fail-under=80`）。GUI 用 egui_kittest 无头集成测试，Python 用 stub AsyncSession 模拟网络。详见 `kb/dev/testing.md` 覆盖率章节。

## API reference

类型级 API 参考见 `cargo doc --open`（`#![warn(missing_docs)]` 强制所有 pub 项带 `///` 注释）。
egui-charts 用法示例见 `kb/user/gui.md` 与 `cargo doc`。
