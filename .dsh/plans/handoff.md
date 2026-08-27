# Handoff: 自动回补缺失数据机制（auto-heal missing data）

## 用途 / 对应 issue

- 用户请求（2026-08-27 晚）：“修复缺失。我不一定能每天更新，因此需要机制来自动处理这种缺失的情况。”
- 目标：让数据管线具备**自动检测并回补缺失数据**的能力，即使不是每天运行也能自愈。
- 本 worktree 对应 GitHub issue：**#308**（feat: 自动回补缺失数据机制，A-Data/C-Feature/D-Complex/P-High），用户已批准计划。

## 已锁定 grill-me 决策（最终契约，实现不得偏离）

1. **回补范围**：全量历史缺口——capital_main_flow 从已有最早日期（当前 2026-07-31）到最新，补所有缺失交易日。
2. **派生表也覆盖**：technical_factor / industry_factor / capital_factor / final_score / market_temperature 一并补算。
3. **触发**：集成进 `scripts/sepa_daily.sh`（后续**改名**为 `scripts/update-database.sh`），每次运行自动扫描+自动补；不另加 cron。
4. **脚本改名**：彻底改名，不保留旧名兼容入口，更新所有引用。
5. **表覆盖**：B（所有表都检查）。日频表逐日缺口检测；非日频表（fin_*/institution_survey/stock_basic/index_basic）每次运行检查 `data_updates.last_updated` + row_count（不逐交易日查，避免误报）。
6. **实现位置**：B——把自愈直接并入 `collectors/main.py sync`：sync 自动检测/回补源数据。
7. **派生补算位置**：A——sync 检测+补源数据，`update-database.sh` 在 import-compass 后调用 Rust 补算派生表。
8. **检测日历**：用 investment_data `ts_trade_day_calendar`（SSE `is_open=1`）作为交易日历。
9. **main_flow 数据源**：A 混合——日常仍用 push2 全市场快照；检测到缺口才用逐股 EastMoney `fflow/daykline` 历史 API 回补（每股票一次请求，约 6000 次）。
10. **派生补算命令**：新增 Rust 子命令 `sepa backfill-dates`（可选 `--start/--end`），脚本只需调用一次；需给 `sepa temperature` 增加 `--date`（当前只有 `sepa score --date`）。
11. **失败策略**：A 严格失败——重试后仍失败的个股/日期使整个 sync/update 报错退出，不做部分成功继续。
12. **中间缺口**：A 也自动回补（不仅尾部）——日频采集器靠回退 data_updates 锚点重跑（INSERT IGNORE 幂等），main_flow 走 fflow 历史，index_daily 按 symbol 显式补指定日期范围。
13. **stock_daily 纳入**：A——`update-database.sh` 跑完 investment_data 同步 + `cargo import` 后也对 stock_daily.parquet/Dolt 做交易日历缺口检查。
14. **investment 同步纳入**：A——`update-database.sh` 第 0 步跑 `scripts/sync-investment-data.sh` 再继续。
15. **不 export DuckDB**：继续（原约束）。

## 背景与技术事实（已查证）

- 两周缺口检查（2026-08-13~08-27，11 个 A 股交易日）：
  - stock_daily / index_daily / dragon_list / block_trade / institution_survey：11 个交易日无日期缺口（institution_survey 另有周末 08-15/16/22/23）。
  - `capital_main_flow`：缺 08-13、08-14、08-24、08-25（4 天）；原因是 `collectors/fetch_main_flow.py` 只存最新交易日的 push2 clist 快照，**不是历史 API**，没跑 sepa_daily.sh 的日子无法自动补回。
  - SEPA 派生表 technical_factor/industry_factor/capital_factor/final_score/market_temperature：只有 08-20、08-21、08-26、08-27，缺 7 天；派生表只在 sepa_daily.sh 运行时生成。
- `capital_main_flow` Dolt PK `(symbol, trade_date)`；列 symbol/trade_date/main_net_inflow/main_net_inflow_rate/super_large_net/large_net/medium_net/small_net/update_date。
- EastMoney 历史每日资金流 API（per-symbol）已验证：
  - `https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get?secid=<market>.<code>&fields1=f1,f2,f3,f7&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61,f62,f63,f64,f65&klt=101&lmt=0`
  - 实测 600519 返回 2026-07-31 起每日行，含 08-13/14 等缺失日期。
  - 字段映射：日期后第 1-5 个为 main/small/medium/large/super_large 净流入（f52-f56），随后 f57=main_net_inflow_rate；可映射到现有列 main_net_inflow, main_net_inflow_rate, super_large_net, large_net, medium_net, small_net。
  - 参考来源：HKUDS/Vibe-Trading `agent/src/tools/fund_flow_tool.py`（`_DAILY_URL` 同上）。
- `collectors/main.py` 现有子命令：`sync`（do_sync 串行 fetch+import 全部：stock_basic/fin_indicators/balance_sheet/income/cash_flow/dragon/block_trade/institution_survey/main_flow/index_daily；index_basic 为 import index_daily 副作用）、`sync-investment`。
- `scripts/sepa_daily.sh` 当前 7 步：① cargo import investment_data→Parquet；② `uv run python main.py sync`；③ Dolt commit collector tables；④ import-compass 11 表；⑤ sepa temperature + score --top 50；⑥ Dolt commit compute tables；⑦ print TOP50。
  - `COLLECTOR_TABLES=(stock_basic fin_indicators fin_balance_sheet fin_income fin_cash_flow capital_main_flow dragon_list block_trade institution_survey index_daily index_basic)`
  - `COMPUTE_TABLES=(technical_factor industry_factor capital_factor final_score market_temperature data_updates)`
- `sepa` Rust CLI 在 `crates/compass-data/src/main.rs`：`SepaCmd::Score { top, date }` 已支持 `--date`；`SepaCmd::Temperature` 无 date 参数（需扩展）。`sepa.rs` 的 run_temperature 默认取 reader.latest_trade_date()。run_score/run_temperature 都是 DELETE+append 写 Dolt，并 dolt_upsert_updates 更新 data_updates（source='compass-data sepa'）。
- `ts_trade_day_calendar` 表位于 investment_data：列 id/exchange/date/is_open（SSE 官方日历，实测有 1990-12-19 起数据）。
- 相关文档线索：
  - `.dsh/kb/user/cli.md` 行 141 定义新鲜度阈值（fin_*120天、行情7天、stock_basic不查）
  - `.dsh/kb/design/data-providers.md` 决策记录有 append 表/增量语义
  - `.dsh/kb/design/backtest.md:83-85` 明确 capital_main_flow 纯快照无历史 → 资金模块自动降级；回补需新写采集器（本工作实现）
- 当前数据终态：本地 master=0306b8a（PR #307 已合并，sepa_daily.sh 已是完整 compass_data 刷新入口）；investment_data max 2026-08-27，clean/pushed；compass_data clean/pushed（仅 untracked `_tmp_name_en`）。

## 必须走 PRE-IMPLEMENTATION GATE（worktree 会话内自主完成）

- 第一步：**读取本 handoff**（已完成）。
- 第二步：**同步原始分支**：`git fetch origin master && git rebase origin/master`（当前基准 0306b8a 若无落后可跳过但有新进展则必须同步）。
- 门禁顺序：
  1. 创建 GitHub issue（A-/C- 标签；建议 `A-Data` + `C-Feature`）。
  2. 进入 plan mode 或按项目规则输出计划（2+ 模块：Python collectors + Rust sepa CLI + shell 脚本 + docs），获得用户批准后写入 `.dsh/plans/*.md`。
  3. 委派 `subagent_skwy_adversarial_test`（第 3.5 步）与 `subagent_skwy_requirement_test`（第 4 步）写 RED 失败测试。
  4. 文档同步（第 5b 步）：至少覆盖 `.dsh/kb/user/cli.md`、`.dsh/kb/design/data-providers.md`、`.dsh/kb/design/architecture.md`（管线变更）、`.dsh/kb/dev/process.md`（如工作流变更）；并全仓 grep `sepa_daily.sh` 旧名引用逐一更新。
  5. 决策记录（第 5c 步）：检查相关 `.dsh/kb/design/*.md` 是否有 `## 决策记录` 章节，缺失则补齐。
- 实现完成后：真实数据冒烟（至少跑一次 update-database.sh，验证 Dolt↔Parquet 行数/日期、缺失日回补）、cargo test/clippy/fmt、commit→review→rebase→reflection，interactive 模式等待用户 push 指令。

## 注意事项

- 脚本改名为 `scripts/update-database.sh` 后，所有引用（AGENTS.md、`.dsh/kb/`、测试脚本 `scripts/tests/`、justfile/README 等）都要同步更新；不保留旧名。
- 任何 compass_data Dolt 写库后必须 commit+push；禁止让数据滞留工作区。
- 不 export DuckDB（保持原约束）。
- 数据异常禁止静默绕过；按 AGENTS.md 问题处理闭环执行。
