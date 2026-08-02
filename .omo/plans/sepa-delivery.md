# sepa-delivery - Work Plan（东方SEPA · 交付层）

> 执行计划 3/3 — 覆盖 Batch 4+5（epic #139 子 issue #150-#152）
> 依赖：plan 2（sepa-engine）todo 8+10（契约类型 + run_sepa）+ plan 1（sepa-collectors）todo 6（读取原语）。
> 配套：`.omo/plans/sepa-collectors.md`（数据就绪）、`.omo/plans/sepa-engine.md`（引擎）、`.omo/plans/sepa.md`（生命周期跟踪）、`.omo/designs/sepa-gui.md`（GUI 设计，已确认+审查修订）

## TL;DR (For humans)

**What you'll get:** SEPA 系统的"出口"——① `compass-data sepa score/temperature` 命令行（算分 + 打印 TOP50 表格 + 写回 Dolt 备份）；② `sepa_daily.sh` 每日一键脚本（更新行情 → 采集 → 导入 → 算分 → 写回 → 打印，全程幂等可重跑）；③ GUI 新增「东方SEPA」标签页（TOP50 排名表 + 五模块分数色阶 + 点击行查看评分详情 + 市场温度计条 + 点击联动图表）。交付后每天一条命令即得选股名单。

**Why this approach:** CLI 写回 Dolt 用锁定两段式（DELETE + `dolt table import -a`）保证幂等可重跑；GUI 完全照抄现有 screener 面板的 citizen→Signal→AsyncDispatcher 通道模式（第三条通道），零新机制；脚本照抄 sync-investment-data.sh 骨架；数据流保持"GUI 只读 Parquet"架构（温度计/评分在进程内 run_sepa 计算，不依赖 CLI）。

**What it will NOT do:** 不做自动定时（脚本手动执行）；不做历史批量回算；GUI 不自动触发计算（纯手动刷新）；不改 dock_style；不改 screener 面板现有行为；不新增 UI 依赖。

**Effort:** Large
**Risk:** Medium - GUI 接线点较多（wire_backend 3-tuple 波及 6 处测试解构）；双 tab leaf 视觉需 kittest 断言；脚本端到端依赖前两层就绪
**Decisions to sanity-check:** 写回两段式（DELETE + import -a，不用 REPLACE INTO）、脚本两段 Dolt commit（③ 采集表 + ⑥ 计算表）、GUI TOP N 纯本地截断（不回写 shared_state）

Your next move: 批准后在 worktree 内按 Wave 4→5 执行；每子 issue 一个 commit（ref #N）。

---

> TL;DR (machine): Large effort, 3 todos (CLI+write-back → daily script → GUI panel), wire_backend 3-tuple touches 6 test sites, two-stage Dolt commit in script, GUI per sepa-gui.md with kittest assertions.

## Scope
### Must have
- `compass-data sepa` 嵌套子命令（`SepaCmd::{Score{--top,--date}, Temperature}`）+ main.rs 注册 + dispatch
- `src/sepa.rs`：run_sepa 调用 → TOP50 表格打印 → 写回 Dolt 5 计算表（两段式）+ data_updates 登记
- compass-data/Cargo.toml 新增 `compass-strategy = { path = "../compass-strategy" }`
- `scripts/sepa_daily.sh`：7 步幂等流水线（import → 采集 → Dolt commit 采集表 → import-compass → sepa 计算 → Dolt commit 计算表 → 打印 TOP50）
- GUI：TabKind::Sepa 标签页 + SepaPanel（温度计条/工具条/12 列表格/右侧详情/状态）+ DataCell::Score/Rank + score_color + 第三条通道 + 图表联动 + egui_kittest 测试
- 文档：kb/design/ui.md（SEPA 面板权威版）、kb/user/cli.md（sepa 命令）、kb/dev/testing.md（如新测试模式）

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不用 REPLACE INTO（锁定两段式 DELETE + import -a）；不做 --overwrite 之外破坏性操作
- GUI 不自动触发计算；TOP N 截断只作用本地副本（不回写 shared_state）；不改 dock_style；不改 screener 面板现有行为（仅提取 dispatch_symbol_fetch 共享函数）
- 不新增 UI/外部依赖；不加 serde 到 SEPA 类型；不做 cron 定时
- 脚本 `dolt add` 限定表（勿 `add .` 卷入 collectors 未提交变更）；任一步失败非零退出

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（先写失败测试）— tokio::test + temp Dolt fixture（import_compass.rs:229 setup_dolt 模式）+ egui_kittest（compass crate 既有无头 harness）+ 脚本自测（scripts/tests/ 先例）
- Evidence: `.omo/evidence/sepa-delivery/task-<N>-sepa-delivery.<ext>`
- 质量门：`cargo test -p compass-data -p compass-ui -p compass` + clippy + fmt + doc --no-deps + llvm-cov ≥80%；`bash -n scripts/sepa_daily.sh`
- **前置风险登记**：master 基线 flaky（#138）——执行前先修或豁免

## Execution strategy
### Parallel execution waves
- Wave 4: todo 11 → 12 链式（脚本调 CLI）
- Wave 5: todo 13（GUI；可与 todo 12 并行——依赖 plan 2 而非 CLI）

### Dependency matrix（本 plan 内部 + 跨 plan）
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 11 (CLI+写回) | plan2 todo 10（run_sepa） | 12 | — |
| 12 (sepa_daily.sh) | 11 | — | 13 |
| 13 (GUI) | plan2 todo 8+10、plan1 todo 6 | — | 12 |

跨 plan 依赖：todo 13 **不依赖 11**（设计文档确认 backend handler 进程内调用 compass_strategy::run_sepa）。

## Todos

- [ ] 11. cli: compass-data sepa 子命令 + 写回 Dolt — issue #150
  What to do / Must NOT do:
  **A. CLI 注册**（crates/compass-data/src/main.rs）：
  - `Command` 枚举（L24-109）新增变体：
    ```rust
    Sepa { #[command(subcommand)] cmd: SepaCmd }
    ```
  - 新枚举（同文件）：
    ```rust
    #[derive(Subcommand)]
    enum SepaCmd {
        /// 计算当日评分并输出 TOP N 表格，写回 Dolt
        Score {
            /// 输出条数上限（默认 50）
            #[arg(long, default_value_t = 50)]
            top: usize,
            /// 指定日期（默认最新交易日；YYYY-MM-DD）
            #[arg(long)]
            date: Option<String>,
        },
        /// 计算市场温度计，写回 Dolt
        Temperature,
    }
    ```
  - `run()` dispatch（L165-232）加 `Command::Sepa { cmd } => match cmd { ... }` 分支；错误包装 `"Sepa {cmd} failed: {e}"`（参照 L187/L205 惯例）
  **B. 新模块 src/sepa.rs**（main.rs 顶部加 `mod sepa;`）：
  - `pub fn run_score(top: usize, date: Option<NaiveDate>, reader: &ParquetReader) -> Result<(), Box<dyn Error>>`：
    1. 构造 `SepaQuery { top_n: top }`，now = date.unwrap_or(最近交易日/今天)
    2. 调 `compass_strategy::sepa::run_sepa(&query, reader, now)` → SepaData
    3. 终端打印 TOP50 表格：列 = rank / 代码 / 名称 / 总分 / 趋势 / 题材 / 资金 / 形态 / 风险（`{:.1}` 一位小数，mono 对齐；参照现有 CLI 输出风格）
    4. 写回 Dolt（见 C）
  - `pub fn run_temperature(reader: &ParquetReader) -> Result<(), Box<dyn Error>>`：调温度计函数（run_sepa 返回的 SepaData.thermometer 或独立入口）→ 打印 score/position → 写回 market_temperature（见 C）
  - 日志：`tracing::info!` 记录 matched/returned/elapsed（照抄 run_screener 日志模式 lib.rs:87-94）
  **C. 写回 Dolt（锁定两段式，不用 REPLACE INTO）**：
  - dolt_dir = config.dolt.compass_data_dir（main.rs 现有回退逻辑）
  - 对 5 张计算表（technical_factor / industry_factor / capital_factor / final_score / market_temperature）：
    1. `dolt sql -q "DELETE FROM <table> WHERE trade_date = '<date>'"`（先清当日——幂等重跑核心）
    2. CSV 落 temp（schema 对齐 Dolt DDL：symbol 带前缀 + trade_date + 各分数列 + update_date）
    3. `dolt table import -a --continue <table> <csv>`（append；当日行已被 DELETE，冲突消除）
  - data_updates 5 列 upsert（source = `'compass-data sepa'`，last_report_date = 当日）
  - 表不存在时 `dolt sql -q "CREATE TABLE IF NOT EXISTS ..."`（**列级 DDL 由执行者按 SepaData 字段自定义并留档到 kb/design/data-providers.md 决策记录**——epic 决策 13 仅有通用约定，无列级定义，审查修订）：technical_factor(symbol, trade_date, ma60, ma120, ma250, atr20, rs_score, vcp_score, update_date)、industry_factor(concept_code, concept_name, trade_date, return20, return60, concept_amount, heat_score, news_score, update_date)、capital_factor(symbol, trade_date, volume_ratio_score, chip_score, main_flow_score, institution_score, update_date)、final_score(symbol, trade_date, trend_score, theme_score, money_score, pattern_score, risk_score, total_score, rank, update_date)、market_temperature(trade_date, score, hs300_trend, zz1000_trend, limit_up_count, total_amount, breadth, position_suggestion, update_date)
  - 执行方式：`std::process::Command::new("dolt").arg("--data-dir").arg(dolt_dir)...`（复用 import_dolt.rs:19-55 的 subprocess 封装风格；如需可提取公共 helper）
  **D. Cargo.toml**：dependencies 新增 `compass-strategy = { path = "../compass-strategy" }`（审查确认：当前仅依赖 compass-core，需补）
  **E. 测试**（main.rs 既有测试模式 L355-465/471-613 + sepa.rs 内嵌）：
  - CLI 解析：`Cli::try_parse_from(["compass-data", "sepa", "score", "--top", "30"])` 断言字段；`sepa temperature` 解析
  - dispatch：temp Dolt + temp parquet fixture → run_score → 断言 Dolt 5 表当日行数、打印输出含 TOP 行
  - 幂等：同日期重跑 → 行数不增
  - 失败：run_sepa 错误（如 reader 缺数据）→ Err 非零退出
  Must NOT: 不用 REPLACE INTO（需 SQL 转义且与 dolt_table_import 封装不一致）；不改既有 4 子命令；不做 --overwrite 之外破坏性操作；写回失败不静默（返回 Err）。
  Parallelization: Wave 4 | Blocked by: plan2 todo 10 | Blocks: 12
  References: `crates/compass-data/src/main.rs:24-109`（Command 枚举）、`crates/compass-data/src/main.rs:161-234`（run dispatch）、`crates/compass-data/src/main.rs:355-465,471-613`（CLI/dispatch 测试模式）、`crates/compass-data/src/import_dolt.rs:19-55`（dolt subprocess 封装风格）、`crates/compass-data/src/import_compass.rs:229-256`（setup_dolt 测试模式）、epic #139 body 决策 13/15/21（表结构/CLI 位置/TOP50）
  Acceptance criteria (agent-executable):
  - `cargo test -p compass-data` 全绿（含 sepa CLI 解析 + 写回幂等测试）
  - `cargo run --bin compass-data -- sepa score --top 10` 在测试 fixture 上端到端可跑（temp Dolt 断言行数）
  - 覆盖率 ≥80%（compass-data crate）
  QA scenarios:
  - happy: temp Dolt + fixture parquet → run_score → Dolt 5 表当日行存在 + stdout 表格含 rank 1-10
  - 幂等: 同日期重跑 → 行数不增（DELETE + append 语义验证）
  - failure: reader 指向空 parquet → run_sepa Err → 非零退出 + 错误信息含 "Sepa score failed"
  - Evidence: `.omo/evidence/sepa-delivery/task-11-sepa-delivery.txt`
  Commit: Y | feat(cli): add compass-data sepa subcommand

- [ ] 12. script: sepa_daily.sh 幂等每日脚本 — issue #151
  What to do / Must NOT do:
  照抄 `scripts/sync-investment-data.sh` 骨架（set -euo pipefail + 头部注释 + PROJECT_ROOT + preflight + red/green/info 彩色输出）新建 `scripts/sepa_daily.sh`，7 步流水线：
  1. **行情更新**：`cargo run --bin compass-data -- import`（investment_data → Parquet，增量）
  2. **数据采集**：`cd collectors && uv run python main.py fetch main_flow dragon block_trade institution_survey concept_member`（5 新数据源；已有数据自动跳过——data_updates 增量）
  3. **Dolt commit 采集表**：`cd /data/compass-data/compass_data && dolt status` 检测变更 → `dolt add capital_main_flow dragon_list block_trade institution_survey concept_member`（**限定表，勿 add .**）→ `dolt commit -m "feat: sepa collectors data ref #139"` → `dolt push origin main`；无变更则跳过
  4. **导入 Parquet**：`cargo run --bin compass-data -- import-compass --table capital_main_flow` 等 4 张（增量）+ `--table concept_member --overwrite`（全量覆盖）
  5. **计算**：`cargo run --bin compass-data -- sepa temperature` + `sepa score --top 50`
  6. **Dolt commit 计算表**：`dolt add technical_factor industry_factor capital_factor final_score market_temperature data_updates` → `dolt commit -m "feat: sepa scores ref #139"` → `dolt push origin main`（**必须第二次 commit——否则计算表变更滞留工作区不落 remote**，违背 epic 决策 2/9；审查修订）
  7. **打印 TOP50**：`cargo run --bin compass-data -- sepa score --top 50`（或复用第 5 步输出——实现时避免重复计算，第 5 步已打印则第 7 步仅提示）
  - preflight：`command -v dolt uv cargo`；`dolt creds ls`；`.dolt` 存在性
  - 每步失败：`red "step N failed: ..."` + `exit 1`（不静默）
  - 幂等：任一步已完成的日期自动跳过（第 1/2 步靠 --since/last_report_date，第 5 步靠 DELETE+append）
  - 脚本自测：新建 `scripts/tests/test-sepa-daily.sh`（参照 pre-push-ref-regex-test.sh 先例）：`bash -n` + 用临时目录/假命令验证流程分支（如 mock cargo/uv/dolt 断言调用顺序）
  Must NOT: 不做 cron 定时；不静默失败；`dolt add` 不限定表 → 流程违规；不重复计算（第 5/7 步避免两次 run_sepa）。
  Parallelization: Wave 4 | Blocked by: 11 | Blocks: —
  References: `scripts/sync-investment-data.sh:1-124`（骨架：preflight/彩色输出/dolt 调用）、`scripts/tests/pre-push-ref-regex-test.sh`（自测先例）、AGENTS.md（compass_data Dolt 仓库 commit/push 规范）、epic #139 body 决策 16/3
  Acceptance criteria (agent-executable):
  - `bash -n scripts/sepa_daily.sh` 通过
  - `bash scripts/tests/test-sepa-daily.sh` 通过（自测：流程分支断言）
  - 真实环境端到端跑通一次 → 输出 TOP50 + Dolt 采集表/计算表当日行 + data_updates 更新（客观验证：`dolt sql -q "SELECT COUNT(*) FROM final_score WHERE trade_date=..."`）
  QA scenarios:
  - happy: 完整链路输出 TOP50
  - 幂等: 立即重跑 → Dolt 行数不变（采集表增量跳过 + 计算表 DELETE+append）
  - failure: 中断第 1 步（import 失败）→ 非零退出 + 红色错误信息可定位
  - Evidence: `.omo/evidence/sepa-delivery/task-12-sepa-delivery.txt`
  Commit: Y | feat(scripts): add sepa_daily.sh idempotent daily pipeline

- [ ] 13. gui: SEPA 评分面板 — issue #152
  What to do / Must NOT do:
  按 `.omo/designs/sepa-gui.md` 全套实现（设计已确认 + 审查修订）：
  **A. compass-ui 组件扩展**：
  - `DataCell`（data_table.rs:31-44）新增 2 变体：
    ```rust
    /// 色阶评分值（数值排序按 value；inverted=true 时 norm=1-|v|/max，风险列用）
    Score { value: f32, max: f32, inverted: bool },
    /// 排名（1-3 名 warning 强调）
    Rank(usize),
    ```
  - `score_color(tokens: &ThemeTokens, norm: f32) -> Color32` 纯函数（放 compass-ui widgets 或 tokens 模块）：norm ≥0.8 → success；0.5-0.8 → lerp(warning, success)；0.25-0.5 → lerp(error, warning)；<0.25 → error
  - Score 渲染格式 `{:.1}`；compare_cells 数值排序分支（Text/Price/Count 既有分支后加）
  - （可选）`DataTable::set_selected(Option<usize>)` 行高亮（selection_bg）——详情面板联动；不做也成立
  **B. 消息/状态/后端接线**（照抄 screener 通道模式）：
  - messages.rs：`RunSepaRequest {}`（无参）+ `RunSepaResponse { data: SepaData, error: Option<String> }`
  - state.rs：`sepa_data: Dynamic<Option<SepaData>>` + `sepa_loading: Dynamic<bool>` + `sepa_error: Dynamic<Option<String>>`（**单 Option 字段**避免半更新，区别于 screener 分字段）
  - backend.rs：第三条 `AsyncDispatcher<RunSepaRequest, RunSepaResponse>` 通道（照抄 backend.rs:114-168）；handler 进程内调 `compass_strategy::sepa::run_sepa`；**wire_backend 返回当前已是 3-tuple（`(work_signal, screener_signal, BackendHandle)`，backend.rs:170-177）——实际变更为 3-tuple → 4-tuple（新增 sepa_signal），同步更新 main.rs:73 / backend.rs 测试 4 处（:282,:327,:364,:473）/ main.rs 测试 1 处（:1044）解构（审查修订：原"2-tuple→3-tuple"描述陈旧）**
  - 主 crate 已有 compass-strategy 依赖（验证 Cargo.toml；无则补）
  **C. 面板**（crates/compass/src/citizens/sepa.rs → SepaPanel，镜像 ScreenerPanel 结构）：
  - tabs.rs TabKind::Sepa（标题「东方SEPA」+ egui-phosphor 图标）；**叠入 Chart leaf 双 tab**（DockState::new(vec![Tab::new(TabKind::Chart), Tab::new(TabKind::Sepa)])——审查已验证 dock_style 无需修改）；dispatcher.rs 注册 citizen
  - SepaPanel::show(ui, shared_state, sepa_signal, work_signal)：
    1. 温度计顶条（Card：THERMOMETER icon + 市场温度 score + 仓位建议 Tag + 5 指标 chip，chip tint = score_color(heat)、delta 箭头 A 股红涨绿跌）
    2. 工具条：计数标签「共 N 行 · 日期」+ `Segmented ["TOP 50","TOP 30"]` + 刷新按钮（Primary + ARROW_CLOCKWISE；loading 时禁用 + spinner）
    3. 12 列表格：排名(Rank)/代码(Text)/名称(Text)/总分(Score)/趋势(Score max30)/题材(Score max25)/资金(Score max20)/形态(Score max20)/风险(Score inverted)/行业(Text, 数据可用时拼 `行业 · 题材1 · 题材2`)/最新价(Price)/涨跌幅(Price)；默认排序 rank 升序
    4. 右侧详情面板（~300px）：点击行刷新（名称+排名 Tag + 总分大字 + 五模块行[标签+分数+ProgressBar] + 子项 SepaFactor 列表[label+score/max+note] + 题材 Tag 区）；无选中行显示占位
    5. 状态：loading spinner / error colored_label + toast / 空态 EmptyState「暂无 SEPA 评分数据 / 点击刷新计算全市场 TOP50」
    6. **TOP N 纯 GUI 截断**：`let mut rows = data.rows.clone(); rows.truncate(top_n)`——**只作用本地副本，绝不回写 shared_state**（切回 50 数据不丢）
    7. 行点击：`dispatch_row_fetch` 联动图表（提取共享函数 `dispatch_symbol_fetch(state, work_signal, symbol)` 到 dispatcher.rs，screener 薄封装；SEPA 直调）
    8. 刷新：`sepa_loading.set(true)` → RunSepaRequest → 完成写回 state；成功 toast「SEPA 评分已更新 · N 只」（last_sepa_loading true→false 转换，照抄 main.rs:918-924 模式）；失败 toast（main.rs:536-542 模式）
    9. 主题切换：`set_tokens` 刷新面板 + table（screener.rs:461-467 模式）
  **D. 测试**（egui_kittest，禁目测）：
  - 面板渲染：空态 → 注入 SepaData → 表格行数/温度计条渲染断言
  - 交互：点击刷新 → loading → 注入响应 → 表格填充；点击行 → 详情更新 + 图表联动信号发出（dispatcher 层断言）
  - **双 tab leaf 视觉断言**：dock 渲染 Chart+Sepa 双 tab → 断言激活 tab 与未激活 tab 样式形状差异（参照 dock_style.rs:176 形状测试先例——审查 M4 要求）
  - score_color：色阶端点/中间值单元测试
  Must NOT: 不改 dock_style；不自动触发计算（纯手动刷新，design §5.4 已确认）；不改 screener 面板现有行为；TOP N 截断不回写 shared_state；不加新 UI 依赖。
  Parallelization: Wave 5 | Blocked by: plan2 todo 8+10、plan1 todo 6 | Blocks: —
  References: `.omo/designs/sepa-gui.md`（全文：布局/交互/契约/决策记录，含审查修订的 Score{inverted}/Rank/risk∈[−3.75,0]）、`crates/compass/src/citizens/screener.rs:246-262`（刷新按钮）、`crates/compass/src/citizens/screener.rs:270-291`（结果区三段式）、`crates/compass/src/citizens/screener.rs:461-487`（dispatch_row_fetch）、`crates/compass/src/backend.rs:114-168`（通道模式）、`crates/compass/src/state.rs:11-34`（SharedState）、`crates/compass/src/tabs.rs:50-54`（TabKind）、`crates/compass-ui/src/widgets/data_table.rs:31-44`（DataCell）、`crates/compass/src/main.rs:536-542,918-924`（toast 模式）
  Acceptance criteria (agent-executable):
  - `cargo test -p compass-ui -p compass` 全绿（含 kittest 渲染/交互/双 tab 断言 + score_color 单测）
  - `cargo clippy` 干净；`cargo fmt --check` 通过
  - GUI 冒烟：真实数据刷新出 TOP50（kittest 快照断言 + 后端日志断言，禁目测）
  QA scenarios:
  - happy: kittest 点击刷新 → 注入响应 → 表格 50 行 + 温度计条 + 详情面板联动
  - failure: 后端 error → error 状态 + toast 推送（None→Some 转换）
  - boundary: 空数据空态渲染；TOP30 切换行数即时变化且切回 50 数据不丢（本地副本验证）
  - Evidence: `.omo/evidence/sepa-delivery/task-13-sepa-delivery.txt`
  Commit: Y | feat(gui): SEPA scoring panel

## Final verification wave（本 plan，epic 总收尾）
> 并行运行，全部 APPROVE 后进入 PR/push。Surface results 并等用户确认。
- [ ] F1. 合规审计: 逐 todo 核对（两段式写回/concept_member 全量/题材公式分母 90/风险 −扣分×0.05/温度计常量/契约无 serde/wire_backend 3-tuple 全解构/TOP N 不回写/GUI 无自动触发）；**kb/ 文档同步核对**（data-providers/ui/cli/testing/config 五处）；sepa-gui.md 归档提交
- [ ] F2. 质量门: `cargo test` 全 workspace + clippy + fmt + doc --no-deps + llvm-cov 每 crate ≥80%；Python pytest --cov-fail-under=80 + ruff
- [ ] F3. 真实端到端 QA: sepa_daily.sh 真实运行一次 → TOP50 + Dolt 双段 commit 落 remote（`dolt log` 验证两次 feat commit）+ 温度计分数合理（客观数值验证）；GUI 冒烟 kittest 快照 + 后端日志断言
- [ ] F4. 范围保真: Scope OUT 无一泄漏（卖出系统/北向/LLM/回测/官方板块指数/历史回算/自动定时/REPLACE INTO）

## Commit strategy（本 plan，epic 收尾）
- todo 11（`ref #150`）、todo 12（`ref #151`）、todo 13（`ref #152`）各 1 commit；每 commit 后 /review-work（≤2 轮修复）
- push 前 rebase origin/master；push 前 /reflect 写反思 commit（ref #119 教训）
- **push 成功后 epic 收尾（强制）**：逐子 issue（#140-#152）追加完成 comment（实现摘要+验收状态+commit 列表）→ 关闭 13 个子 issue → 关闭 epic #139 + 总结 comment（issue-workflow 阶段 4；comments.md"永远追加"）
- follow-up #153（LLM 新闻）/ #154（回测）保持 OPEN，epic 关闭 comment 中注明依赖就绪
- Dolt 数据 commit 与代码 commit 分离（Dolt 由 sepa_daily.sh ③⑥ 两次 commit；代码 git ref #N）

## Success criteria（epic 完成）
- 13 个实现 todo 全部完成 + 各 plan F1-F4 APPROVE
- 真实端到端：sepa_daily.sh 一次运行 → TOP50 表格 + 温度计 + Dolt 采集表/计算表当日行 + 双段 commit 落 remote
- GUI 面板展示评分排名/详情/温度计，点击联动图表（kittest 验证）
- 覆盖率 Rust 每 crate ≥80% / Python ≥80%；CI 全绿
- kb/ 文档同步完成；epic #139 收尾（13 子 issue 关闭 + 总结 comment）
