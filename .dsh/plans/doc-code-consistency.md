# Plan: 项目书与实现一致性全面修正（ref #336）

## 对应 issue / 背景

- Issue: **#336** `fix: 项目书与实现一致性全面修正`，标签 A-Data, C-Docs, C-Code-Quality, P-High
  https://github.com/qiboda/compass/issues/336
- Worktree: `.worktrees/fix-doc-code-consistency`，分支 `fix/doc-code-consistency`
- 用户请求：审计项目书与实现一致性 → 用户指示「全部修正」→ 单一大 PR（handoff 锁定决策）。
- 授权：按 handoff 自主推进（worktree 会话内自主完成门禁）。

## 锁定决策（含依据）

| # | 决策 | 选择 | 依据 |
|---|------|------|------|
| D1 | keepalive realtime 源 | **不实现**，文档标实际状态，PR 记录偏差 | B7 证据 `.dsh/evidence/b7-migrate-collectors-to-rust.md`：用户已明确接受 JSON-only freeproxy/keepalive、放弃 realtime 移植（B7 偏差，reflections.md:172）。依赖 pyfreeproxy（Python 库）抓第三方代理站点，Rust 无等价实现，重写属新功能开发而非一致性修正。handoff 决策 2 允许「按代码实际能力补齐/文档标注」。 |
| D2 | ts_code/裸码启发式 | **不删**，保留 + symbols.md 文档化实际状态 | 生产调用者：compass-core `infer_exchange_prefix`（main.rs:453-461 D10 config 迁移、screener_eval.rs:79、sepa/scoring.rs:945-947、GUI main.rs:682/1170/1196/1220）；collectors `to_ts_code`/`infer_exchange`（Dolt stock_basic.ts_code 列写入）。删除有数据迁移风险；#181 Task 6(d) 明确保留裸码启发式用于 config 迁移。handoff 允许「风险不可删则明确文档为实际状态并记录偏差」。 |
| D3 | missing_docs | **全部启用并补齐**（data/i18n/strategy/types/collectors 5 个 lib crate） | 「全部修正」精神；CI `clippy -- -D warnings` 强制。i18n 163 项（KEY_* 常量，用 `#[doc = "..."]` 脚本辅助）、collectors 154 项、strategy 30、types 28、data 23。compass 是 bin crate（无 lib.rs，Cargo.toml 仅 [[bin]]）不启用，AGENTS.md 措辞按实际。 |
| D3b | export 多格式 | **实现 csv + parquet-dir** | handoff 决策 2 明确「补齐实现」；历史契约：csv=单文件（`--format csv --output data.csv`）、parquet-dir=「另一个 Parquet 目录（与主库同布局）」。共享 fetch_bars 前复权语义（与 duckdb 分支一致，ref #176）。 |
| D4 | baostock 死代码 | **删除** | main.rs 无 `baostock::` 调用；调用不存在的 `scripts/fetch_adj_factor.py`；`AdjFactor`（model.rs:257）唯一使用者是 baostock.rs（duckdb.rs 用独立 `AdjFactorRecord`）；compass-data lib 无外部 crate 使用者。 |

## 交付物（批次）

### 批次 A — 代码整改（行为变更）

**A1. export 补齐 csv / parquet-dir**（`crates/compass-data/src/export.rs`、`main.rs` help）
- `csv`：单文件（`--output data.csv`），header 一行 + 每 symbol 1d bars：
  `symbol,trade_date,open,high,low,close,adjclose,volume,amount`；经 `fetch_bars`（前复权，与 duckdb 分支一致，`adjclose==close`）；先创建父目录；overwrite 语义：现有文件存在且非 overwrite → 报错/跳过（与 duckdb 分支一致的保守处理）。
- `parquet-dir`：输出目录含 `stock_daily.parquet`（symbol 列）+ `stock_daily.symbols.txt`（每行一个 symbol）+ `index_daily.parquet`/`index_basic.parquet`（若输入存在），布局与主库一致，可被 `ParquetReader::new` 重新读取。用一个 in-memory DuckDB `COPY (SELECT ...) TO ...` 或 arrow 写出；`overwrite` 时先删目标文件。
- main.rs Export help `Output format: parquet-dir, duckdb, csv` 保持并验证与实现一致（补齐后一致），`--format` 未知值仍 warn + 跳过。

**A2. collectors 读取 config.toml**（`crates/compass-collectors/src/config.rs`、`Cargo.toml`）
- 新增 `load_config()`（或等价函数）：读 `$HOME/.config/compass/config.toml`，用 serde 轻量结构解析 `[dolt]` 节（`investment_data_dir`/`compass_data_dir`）；失败 warn + 回退默认值（与 compass-data `load_config()` 同模式）；**env 变量优先**（`COMPASS_DATA_DIR`/`COMPASS_INVESTMENT_DATA_DIR` 覆盖 config.toml）。
- 需要 `toml` 依赖（workspace 已有 `toml = "0.8"`）。
- 注意：collectors 不依赖 compass-core，用独立轻量 serde 结构（避免引入重型依赖）。

**A3. 删除 baostock 死代码**
- 删除 `crates/compass-data/src/baostock.rs`；`lib.rs` 去掉 `pub mod baostock;`；`main.rs` 去掉 `mod baostock;`；`crates/compass-core/src/model.rs` 删 `AdjFactor`（先 grep 确认无其他引用，含 tests）。

**A4. missing_docs 启用并补齐**
- `crates/compass-data/src/lib.rs`、`crates/compass-i18n/src/lib.rs`、`crates/compass-strategy/src/lib.rs`、`crates/compass-types/src/lib.rs`、`crates/compass-collectors/src/lib.rs` 加 `#![warn(missing_docs)]`。
- 补齐全部 pub 项 `///` 或 `#[doc = "..."]`（KEY_* 常量批量、函数/结构体/枚举逐个）。
- 验证：`cargo doc --no-deps`（RUSTDOCFLAGS="-D warnings"）+ `cargo clippy -- -D warnings` 全绿。

**A5. CLI help 与实现同步**
- 核对各 bin help（compass-data export 格式、collectors 子命令/keepalive 帮助）与实现一致；已有 `--source realtime` 拒绝提示（main.rs:598）保持。

### 批次 B — 测试（门禁 3.5 + 4，RED→GREEN）

- **3.5 对抗性测试**（`subagent_skwy_adversarial_test`，RED）：
  - export csv/parquet-dir：空目录/坏 parquet/输出已存在（非 overwrite）/覆盖语义/前缀符号/parquet-dir 可被 ParquetReader 重读/symbols.txt 正确性。
  - collectors config.toml：无文件回退默认/坏文件 warn 回退/env 优先/相对路径。
- **4 需求测试**（`subagent_skwy_requirement_test`，RED）：
  - export csv 单文件 header + 行数 + 前复权（adjclose==close）。
  - export parquet-dir 目录布局（stock_daily.parquet + symbols.txt + index 复制）。
  - collectors config `[dolt]` 读取生效（dolt_dir/investment_data_dir 返回 config 值）。

### 批次 C — 文档同步（门禁 5b + 5c）

逐文件（按 handoff 审计清单 + 复核事实）：
- `AGENTS.md`：Knowledge base 索引补 `gui-i18n.md`；Available Skills 表补 grill-me/skwy-autonomous/skwy-autopilot/meta-tools/subagent-fleet（或改「相关工作流技能（非穷举）」）；Setup Rust 版本现为 **1.98.0**（CI stable）；API reference missing_docs 按实际（除 compass bin 外全部启用）；Testing 覆盖率命令 `cargo llvm-cov nextest`。
- `.dsh/kb/github/labels.md`：A-GUI → `crates/compass + crates/compass-ui`；A-Data 扩含 `crates/compass-collectors`；S-CI-Failure 说明改 ci-fix。
- `.dsh/kb/github/ci-fix.md` / `fix.md` / `impl.md`：删 `opencode-ci-fix`、旧 OpenCode 技能路径（`/home/skwy/.dsh/skills/...`）、Python Lint/Python Test job 描述。
- `.dsh/kb/user/index.md`：快速开始补 `import-compass --table stock_basic`。
- `.dsh/kb/user/cli.md`：6 子命令（import/check-stock-daily/import-compass/export/backup/sepa）；export 节改三格式实现后描述 + 行为；import 单次全量描述；`--since` 按各表日期列（财务 report_date、行情 trade_date、调研 survey_date）；新增 check-stock-daily 独立节；collectors 子命令补全（backfill/main-flow-backfill/index-daily-backfill/stock-basic-official/freeproxy/check-proxy-pool 等）；环境变量补全（COMPASS_DATA_DIR/COMPASS_INVESTMENT_DATA_DIR/COMPASS_AUTO_HEAL/COMPASS_NAME_EN_MAPPING 等）；keepalive 按实际（JSON 源 + realtime 未支持，B7 偏差）；concept_member 描述降级/改用实际用途；L10 图与 L146-176 export 节同步。
- `.dsh/kb/user/gui.md`：5 tab（Chart/Market/Sepa 左 + Logger 底 + Screener 右）；新增大盘（Market）章节；Parquet 路径统一绝对路径 `/data/compass-data/parquet_data`。
- `.dsh/kb/user/config.md`：`[dolt]` 节对 collectors 生效（A2 后写真实行为）。
- `.dsh/kb/dev/database.md`：表数 **17→18**，补 `backtest_result`（实测 Dolt SHOW TABLES 18 表确认）。
- `.dsh/kb/dev/testing.md`：集成测试各 crate 均可有 tests/；流水线 0~8；覆盖率 check 9 项（含 collectors 20%）；基准表补 `compass-strategy/screener_eval`。
- `.dsh/kb/dev/toolchain.md`：required status checks 9→2（实际 ci.yml 两个 job：Rust/Bench 编译）；Python 时代排查卡标注历史/退役。
- `.dsh/kb/dev/reflections.md`：dual_run 脚本条目补注（B7 后删除/仅历史参考）。
- `.dsh/kb/design/architecture.md`：crate 关系补 collectors/i18n/strategy/sepa/*；backend 通道 5；Citizen/面板 5；单文件存储；export 多格式；import --since 非增量；CLI 子命令补 check-stock-daily/sepa；覆盖率决策记录；progress 决策 Rust 实现；baostock 死代码闭环。
- `.dsh/kb/design/data-providers.md`：GUI 数据提供者区分 DuckDbProvider/ParquetReader；adjclose 措辞修正（fetch_bars 输出前复权、映射丢弃 adjclose 字段；fetch_cross_section 保留 adjclose 字段——按实际）；Dolt 导入单次全量导出；财务表 append/merge/ODKU 语义；决策记录 Python 命令改 Rust/标注 #310 取代。
- `.dsh/kb/design/symbols.md`：ts_code/裸码启发式实际状态（core 版 config 迁移兼容用途 + collectors 版 Dolt ts_code 写入；#181 保留决策）。
- `.dsh/kb/design/ui-widgets.md`：补 MarketPanel，业务组件 4→5。
- `.dsh/kb/design/gui-i18n.md`：删除 concept 主题链路（实测 index_type 仅 `official`/`industry` 两值：industry=90, official=30），改 index/industry 两节。
- `.dsh/kb/design/workflow-skills.md`：路径 `/home/skwy/.dsh/skills/` 与 `worktrees-dsh.sh`。
- `.dsh/kb/dev/process.md`：rebase 是 push 前人工流程（非 pre-push hook 行为）；keepalive 描述按实际（L501 已写 json-only，核对后保持一致）。

### 批次 D — 验证与收尾

1. 真实数据冒烟：`export --format csv` 与 `--format parquet-dir` 用 `/data/compass-data/parquet_data` 实测（输出文件/目录存在、行数 >0、ParquetReader 可重读）；collectors config.toml 读取冒烟（临时 HOME 或直接验证返回值）。
2. `cargo test` / `cargo clippy -- -D warnings` / `cargo fmt --check` / `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` 全绿。
3. Commit → review（五角度）→ fix（≤2 轮）→ rebase origin/master → reflection → 等待 push 指令（interactive）。
4. PR 创建（push 后），issue #336 收尾 comment + 关闭。

## 验证门禁（F-wave）

- **F1 合规**：全部实现 commit 引用 `ref #336`（独立成行）；无 master 实现类提交；worktree 分支内完成。
- **F2 审查**：subagent_review 五角度 P0/P1 清零。
- **F3 测试**：`cargo test` 全绿 + 覆盖率门槛（scripts/check-coverage.sh）通过（如果 llvm-cov 可直接跑）。
- **F4 scope fidelity**：handoff 审计清单逐项核对——每项 either done / documented-as-is / 记录偏差。

## 风险与偏差记录

- **偏差 1（D1）**：keepalive realtime 不实现——B7 用户已接受放弃（用户决策，非静默降级）；文档与 CLI 已明示。
- **偏差 2（D2）**：ts_code/裸码启发式不删除——生产调用者存在，#181 保留决策；文档化为实际状态。
- 若实现中遇到 handoff 未覆盖的不一致，按「补齐实现」优先；不可行的向用户提出。
