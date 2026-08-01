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
  →  For epics: /issue-workflow creates epic + sub-issues upfront, batches by DAG
  →  implement (each sub-issue walks GATE independently)
  →  cargo nextest (tests must pass)
  →  commit with ref #<sub-N> (epic) or ref #N (single issue)
  →  ai-review (/review-work) — per sub-issue + pre-PR
  →  push master (one PR with all sub-issue commits)
  →  CI passes  →  batch close sub-issues + epic with gh issue close
```

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
`.opencode/skills/issue-workflow/SKILL.md`。要点：

- Epic + sub-issues 在规划时一次性创建（`/ulw-plan` 识别、`/issue-workflow` 批量创建）
- `.omo/plans/<epic>.md` 以 `pending | in_progress | done` 表跟踪状态
- 子任务按依赖 DAG 分批次，批次切换需**人工确认**
- 一个 epic 一个 PR，每个 sub-issue 一个 commit（`ref #<sub-N>`），regular merge
- 每个 sub-issue 独立走 GATE；合并后先关 sub-issues 再关 epic

### 当 OpenCode 发现新 Bug 时

1. 使用 `.github/ISSUE_TEMPLATE/bug_report.md` 模板创建 issue
2. 回读确认（`gh issue view <N>`）issue 已存在
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

## OpenCode 工作流

完整流程由 **`compass-workflow` skill** 强制执行（plan → gate → test-first →
per-step verify → commit → review → push）。以下是 skill 未覆盖的本地细节：

### Pre-push hook 检查（`.githooks/pre-push`）

push 前按顺序执行：

1. **CI 健康**：`master` 上的最新 CI 运行必须通过。如果失败，为失败创建 issue，修复后再 push。永远不要在 CI 破损的基础上 push。
2. **cargo fmt --check**
3. **cargo clippy -- -D warnings**
4. **cargo doc --no-deps**（必须无警告）
5. **Issue 引用**：`ref #N` 必须指向 open issues
6. **Python 检查**（若存在 `collectors/pyproject.toml`）：`uv run ruff check *.py tests/` + `uv run pytest tests/ -q`

> **覆盖率门禁**在 CI 执行（coverage job 强制 workspace + 每 crate ≥80%、Python ≥80%），太慢不适合 pre-push 本地检查。见 `kb/dev/testing.md` 覆盖率章节。

手动 pre-push checklist（与 hook 相同）：`cargo fmt --check` + `cargo clippy -- -D warnings`
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
# 1. 从目标分支切修复分支（复用 /worktree skill）
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

**加载 `/worktree` skill 获取完整流程**（创建、post-creation MANDATORY 步骤、
`/handoff`、自动启动区域、合并后清理）。

**为何不用 plugins**：评估了 `opencode-worktree` 插件（kdco/worktree via OCX），
发现存在阻塞性问题（无法幂等地重新打开、终端启动不可靠、无法重新打开 session）。
手动 worktrees + `/worktree` skill 提供了完全的控制，避免了这些问题。

## 版本控制

```sh
git add <files>              # stage only intended changes
git commit                    # uses .gitmessage template
git push origin main          # triggers CI
```

## 快速开始

```sh
cargo run                       # launch the GUI app (needs X11/Wayland)
RUST_LOG=debug cargo run        # verbose logging
```

### CLI（compass-data）

完整子命令参考见 `kb/user/cli.md`。速查：

```sh
cargo run --bin compass-data -- import                    # Dolt investment_data → Parquet（全量）
cargo run --bin compass-data -- import --since 20260725   # 增量
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
`.opencode/skills/docs/SKILL.md` § Change → kb/ Mapping Table。

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

### CI 缓存策略（rust-cache）

CI 的 `Swatinem/rust-cache@v2` 采用**仅 master save** 策略：

```yaml
- uses: Swatinem/rust-cache@v2
  with:
    save-if: ${{ github.ref == 'refs/heads/master' }}
    prefix-key: ${{ github.job }}   # 各 job 缓存独立
```

- **master**：每次 push 更新缓存（key 含 `Cargo.lock` hash，锁文件不变则命中旧缓存）
- **分支**：只 restore（GitHub cache 自动 fallback 到默认分支命中 master 条目），不写自己的缓存——避免短命分支产生孤儿缓存（7 天 LRU 淘汰前无人复用）
- **锁文件变化**（如 PR 加依赖）：key 变 → miss → 全量编译（依赖集变了，缓存无意义）
- `save-if: false` 时 **restore 仍生效**（Swatinem/rust-cache 语义），仅跳过 save

历史：`572e688` 曾用 `save-if: true`（分支自缓存提速），因短命分支孤儿缓存浪费回退为仅 master save（`d55eead`, ref #89）。

## Config

Config 位于 `~/.config/compass/config.toml`，全部字段可选，缺省回退到
`crates/compass-core/src/model.rs` 中的默认值。完整选项见 `kb/user/config.md`。

## 日志

- Stderr：始终输出。`RUST_LOG` 控制级别（`error`、`warn`、`info`、`debug`、`trace`）。
- 文件：`logs/compass.log`（每日滚动）。

## 调试技巧

### 检查东方财富 API 返回内容

```sh
# K-line API
curl "https://push2his.eastmoney.com/api/qt/stock/kline/get?secid=0.000001&klt=101&fqt=1&beg=20250101&end=20250721&lmt=10&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61"

# Symbol listing API
curl "https://push2delay.eastmoney.com/api/qt/clist/get?pn=1&pz=3&fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048&fields=f12,f14&ut=bd1d9ddb04089700cf9c27f6f7426281"
```

### 检查 Parquet 文件

```sh
ls -lh parquet_data/stock_daily.parquet
wc -l parquet_data/stock_daily.symbols.txt    # symbol count
```

### 用 DuckDB 查询 Parquet

```rust
use duckdb::Connection;
let conn = Connection::open_in_memory()?;
conn.execute_batch("SELECT * FROM read_parquet('parquet_data/stock_daily.parquet') WHERE symbol = 'SH600519' LIMIT 5")?;
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

### Dolt 数据库查询

```sh
# investment_data (read-only, third-party)
dolt --data-dir=investment_data sql -q "SELECT COUNT(*) FROM final_a_stock_eod_price"
dolt --data-dir=investment_data sql -q "SELECT * FROM final_a_stock_eod_price WHERE symbol='SZ000001' ORDER BY tradedate DESC LIMIT 5"
dolt --data-dir=investment_data sql -q "SELECT * FROM ts_a_stock_list LIMIT 5"
```

### compass_data（自定义可修改数据库）

`compass_data` 是我们自己的 Dolt 仓库，用于自定义数据 — 公司概况、
财务指标、自选股等。它与 `investment_data` 位于同级目录。

```sh
# Run `dolt sql` from the parent directory to enable cross-database queries
cd /path/to/compass
dolt sql -q "SELECT * FROM compass_data.stock_basic LIMIT 5"
dolt sql -q "SELECT * FROM compass_data.fin_indicators WHERE symbol='SH600519' ORDER BY report_date DESC"

# Cross-database JOINs
dolt sql -q "
SELECT sb.name, sb.industry_l1, ts.list_date
FROM compass_data.stock_basic sb
JOIN investment_data.ts_a_stock_list ts ON sb.ts_code = ts.ts_code
"

dolt sql -q "
SELECT sb.name, fi.report_date, fi.revenue / 1e8 AS rev_yi, fi.eps
FROM compass_data.stock_basic sb
JOIN compass_data.fin_indicators fi ON sb.symbol = fi.symbol
JOIN investment_data.final_a_stock_eod_price e ON sb.symbol = e.symbol
WHERE sb.symbol = 'SH600519'
ORDER BY e.tradedate DESC
LIMIT 3
"
```

核心表：

| 表 | 用途 | 主键 |
|---|---|---|
| `stock_basic` | 公司概况 | `symbol`（`SZ000001`）+ `ts_code`（`000001.SZ`）|
| `fin_indicators` | 每报告期财务指标 | `(symbol, report_date)` |
| `fin_balance_sheet` | 资产负债表 | `(symbol, report_date)` |
| `fin_income` | 利润表 | `(symbol, report_date)` |
| `fin_cash_flow` | 现金流量表 | `(symbol, report_date)` |

```sh
# Query financial statements
dolt sql -q "
SELECT * FROM compass_data.fin_balance_sheet
WHERE symbol='SH600519' ORDER BY report_date DESC LIMIT 3"

dolt sql -q "
SELECT * FROM compass_data.fin_income
WHERE symbol='SH600519' ORDER BY report_date DESC LIMIT 3"

dolt sql -q "
SELECT * FROM compass_data.fin_cash_flow
WHERE symbol='SH600519' ORDER BY report_date DESC LIMIT 3"

# Cross-table financial analysis
dolt sql -q "
SELECT sb.name, bs.report_date,
  bs.TOTAL_ASSETS / 1e8 AS total_assets_yi,
  inc.TOTAL_OPERATE_INCOME / 1e8 AS revenue_yi,
  cf.NETCASH_OPERATE / 1e8 AS operating_cf_yi
FROM compass_data.stock_basic sb
JOIN compass_data.fin_balance_sheet bs ON sb.symbol = bs.symbol
JOIN compass_data.fin_income inc ON bs.symbol = inc.symbol AND bs.report_date = inc.report_date
JOIN compass_data.fin_cash_flow cf ON bs.symbol = cf.symbol AND bs.report_date = cf.report_date
WHERE sb.symbol = 'SH600519'
ORDER BY bs.report_date DESC
LIMIT 3"
```

### 重置一切

> **警告：以下命令会删除所有已导入的行情数据。** 执行前请确认已完成备份（`compass-data backup`）。

```sh
rm -rf /data/compass-data/parquet_data/   # 主 Parquet 数据
rm logs/compass.log                        # 日志
```
