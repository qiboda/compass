# Handoff: 项目书与实现一致性全面修正（单一大 PR）

## 用途 / 对应 issue

- 用户请求：**“review 项目书，和项目实现是否一致，是否需要更新等等” → 审计后用户指示“全部修正。”**
- 目标：把 AGENTS.md / `.dsh/kb/`（项目书）与当前仓库实现之间的所有不一致全部修复。
- 用户已确认方向：**文档 + 全部代码整改**（不只是改文档）；按 **单一大 PR** 组织（不做 epic 分批）。
- 对应 GitHub issue：**尚未创建**，worktree 会话按门禁第 2 步创建一个单 issue（建议标题：`fix: 项目书与实现一致性全面修正`；标签 `A-Data` + `C-Refactor` 或 `C-Docs`）。

## 已锁定 grill-me 决策（最终契约）

1. **全部修正**：审计出的所有不一致都处理，文档与实现最终一致。
2. **代码方向**：审计中“实现与文档不一致”的能力按**补齐实现**处理：
   - `compass-data export` 实现 csv / parquet-dir 输出（不仅仅 DuckDB）；
   - `compass-collectors keepalive` 实现 realtime 源（或按代码实际能力补齐/文档标注时以“实现优先”为准，禁止只改文档绕过）；
   - `compass-collectors` 支持读取 `~/.config/compass/config.toml`（与 `compass-data` 一致）；
   - 全部公开 crate 启用 missing_docs 并补齐 `///` 文档（或按实现完善后文档与实现一致）；
   - 删除/清理 `baostock.rs` 死代码（调用不存在 Python 脚本）；
   - 按审计建议处理 `ts_code` / 裸码启发式遗留（优先删除；若因 Dolt schema/数据迁移风险不可删，则明确文档为实际状态并在 PR 中记录偏差）。
3. **单一大 PR**：一个 issue、一个 worktree、一个 PR；可以多个原子 commit，每 commit 带独立行 `ref #N`。
4. **不 export DuckDB 约束**：与本修复无关，保持现状。
5. **禁止静默绕过**：遇到“改文档还是改代码”的两难时，默认按“补齐实现”做；确实不可实现/风险过大时向用户提出，不得悄悄改成只改文档。

## 审计发现清单（必须逐项处理）

### A. AGENTS.md / 流程 / GitHub 元数据

- `AGENTS.md` Knowledge base 索引补 `.dsh/kb/design/gui-i18n.md`。
- `AGENTS.md` Available Skills 表补 `grill-me`、`skwy-autonomous`、`skwy-autopilot`、`meta-tools`、`subagent-fleet`，或改为“相关工作流技能（非穷举）”。
- `AGENTS.md` Setup Rust 版本改为实际/不写死（当前 1.98.0 + CI stable）。
- `AGENTS.md` API reference 的 `missing_docs` 表述按实现措辞（全部启用后则为全 crate；若未全启则收窄）。
- `AGENTS.md` Testing 覆盖率命令改为 `cargo llvm-cov nextest`。
- `.dsh/kb/dev/process.md`：明确 rebase 是 push 前人工流程，不是 pre-push hook 行为。
- `.dsh/kb/github/labels.md`：`A-GUI` 改为 crates/compass + crates/compass-ui；`A-Data` 扩为含 crates/compass-collectors 或新增 A-Collectors；`S-CI-Failure` 改 ci-fix。
- `.dsh/kb/github/ci-fix.md`、`fix.md`、`impl.md`：删除 `opencode-ci-fix`、旧 OpenCode 技能路径、Python Lint/Python Test job 描述；改为 `ci-fix`、`/home/skwy/.dsh/skills/...`。

### B. 用户文档

- `.dsh/kb/user/index.md`：快速开始补 `import-compass --table stock_basic`。
- `.dsh/kb/user/cli.md`：
  - `compass-data` 顶层命令改为 6 个（import/check-stock-daily/import-compass/export/backup/sepa）。
  - `export` 输出格式按实现补齐后写全（实现 csv/parquet-dir 后；若选择仅 DuckDB 则必须同步所有文档/help）。
  - `import` 描述改为“单次全量 SQL 导出单文件”，删除逐股 6000+ 查询旧描述。
  - `--since` 说明按各表日期列（财务 report_date、行情 trade_date、调研 survey_date）。
  - 新增 `check-stock-daily` 独立小节（--dolt-dir/--parquet-dir）。
  - 补 `compass-collectors` 子命令：`backfill`、`main-flow-backfill`、`index-daily-backfill`、`stock-basic-official`、`freeproxy`、`check-proxy-pool` 等。
  - 补环境变量：`COMPASS_DATA_DIR`、`COMPASS_INVESTMENT_DATA_DIR`、`COMPASS_AUTO_HEAL`、`COMPASS_NAME_EN_MAPPING` 等。
  - keepalive 按实现后写法（realtime 已实现则写双源；若最终未实现则写仅 json + realtime 未支持）。
  - 删除/降级 `concept_member` 所有活跃描述。
- `.dsh/kb/user/gui.md`：
  - 布局改为 Chart/Market/Sepa/Logger/Screener 五个标签页。
  - 新增「大盘（Market）」面板章节。
  - Parquet 路径统一为绝对路径 `/data/compass-data/parquet_data`。
- `.dsh/kb/user/config.md`：明确 `[dolt]` 与 collector 配置关系；实现 collectors 读 config.toml 后文档写真实行为。

### C. 开发文档

- `.dsh/kb/dev/database.md`：compass_data 表数 17→18，补 `backtest_result`。
- `.dsh/kb/dev/testing.md`：
  - “集成测试仅 compass-core”改为各 crate 均可有 tests/。
  - “7 步流水线”改为 0~8（含 1b/4b 等）。
  - 覆盖率 check 数量 8→9（含 compass-collectors 20%）。
  - 基准表补 `compass-strategy/screener_eval`.
- `.dsh/kb/dev/toolchain.md`：required status checks 9→2（Rust/Bench）；Python 时代排查卡统一标注历史/退役或归档。
- `.dsh/kb/dev/reflections.md`：dual_run 脚本条目补注“B7 后删除/仅历史参考”。

### D. 设计文档

- `.dsh/kb/design/architecture.md`：
  - crate 关系补 `compass-collectors`、`compass-i18n`、`compass-strategy/sepa/*`。
  - backend 通道改 5 条；Citizen/面板改 5 个；SharedState 示例更新。
  - 单文件存储表述修正；export 仅 DuckDB 或补齐多格式后同步；`import --since` 非增量。
  - CLI 子命令补 check-stock-daily/sepa；覆盖率决策记录更新；progress 决策更新为 Rust 实现。
  - Python 退役声明与 baostock 死代码闭环。
- `.dsh/kb/design/data-providers.md`：
  - GUI 数据提供者区分 DuckDbProvider/ParquetReader。
  - adjclose 实际保留并前复权；修改“丢弃”错误描述。
  - Dolt 导入为单次全量导出。
  - 财务表 append/merge/ODKU 语义。
  - 决策记录中 Python 命令改为 Rust 或标注被 #310 取代。
- `.dsh/kb/design/symbols.md`：ts_code/裸码启发式按代码处理（删除旧函数或文档改为真实状态）。
- `.dsh/kb/design/ui-widgets.md`：补 MarketPanel，业务组件 4→5。
- `.dsh/kb/design/gui-i18n.md`：删除 concept 主题链路，改为 index/industry 两节。
- `.dsh/kb/design/workflow-skills.md`：路径改为 `/home/skwy/.dsh/skills/` 与 `worktrees-dsh.sh`。

### E. 代码整改

- `crates/compass-data/src/baostock.rs`：删除（Python 脚本已不存在）或明确死代码标注并移除 pub mod。
- `crates/compass-data/src/export.rs` + `main.rs`：实现 csv/parquet-dir（或按用户选定方向同步 CLI help/文档）。
- `crates/compass-collectors/src/config.rs` / `orchestrate.rs`：读取 `~/.config/compass/config.toml`，使 `[dolt]` 等配置对 collectors 生效；保留环境变量覆盖。
- `crates/compass-collectors/src/keepalive.rs` + `main.rs`：实现 realtime 源或按审计明确能力边界并同步文档。
- `crates/*/src/lib.rs`：启用 `#![warn(missing_docs)]` 并补齐所有 pub 项文档（如过大，至少保证 compile lint 通过；否则按实际收窄文档）。
- `crates/compass-core/src/data/symbol.rs`、`crates/compass-collectors/src/stock_basic*.rs`：处理 ts_code/启发式遗留（删除或文档化）。
- CLI help 文案/usage 与实现同步（export 格式、子命令、环境变量等）。

## 当前事实

- master = `c4df0aa`（含 Python→Rust 迁移及同步计时 PR #335）。
- 无 `collectors/` 跟踪文件；项目书仍有多处 Python 残留引用（见清单）。
- 工作区已有旧 worktree（`.worktrees/issue-112/121/122`）不影响本 worktree。
- 审计仅只读；本 handoff 之前未修改仓库。

## 必须走 PRE-IMPLEMENTATION GATE（worktree 会话内自主完成）

1. 读取本 handoff；`git fetch origin master && git rebase origin/master`。
2. 创建单 issue（A- + C- 标签），展示 URL。
3. 编写计划（2+ 模块，尤其代码整改），获用户批准后写 `.dsh/plans/*.md`。
4. 委派 `subagent_skwy_adversarial_test`（3.5）与 `subagent_skwy_requirement_test`（4）写 RED。
5. 文档同步（5b）与决策记录（5c）。
6. 实现后：真实数据冒烟（export 多格式、collectors config、realtime 等按功能验证）、`cargo test`/clippy/fmt、commit→review→rebase→reflection，interactive 模式等待 push 指令。

## 注意事项

- 单一大 PR 但务必保持原子 commit，每 commit 引用同一个 open issue。
- 任何行为变化（export 多格式、config.toml、realtime、ts_code 删除）必须有测试/证据，不能只改文档。
- 如某项“补齐实现”实际不可行或风险过大，向用户提出并等待决策；不得静默降级为改文档。
- 不 export DuckDB 约束不变。
