# sepa-backtest - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 一个可运行的量化回测工具：用 SEPA 评分系统逐日重算 2025 年以来全市场的历史评分，模拟"每天收盘后买入评分最高的 50 只股票、等权持有 5 个交易日后换仓"的策略，输出胜率、盈亏比、最大回撤、年化收益等绩效指标，并和"市值最大的 300 只股票等权组合"作为的基准对比，结果保存为 CSV 和数据库表。

**Why this approach:** SEPA 评分引擎本身已支持按任意历史日期计算（点内计算，不偷看未来），所以不需要重构引擎——只需逐日调用它重放历史；绩效统计和基准代理写成纯函数，方便测试和复用。

**What it will NOT do:** 不回补历史主力资金流数据（资金模块自动降级，列为后续任务）；不做任何界面改动；不引入新依赖库；不改动现有评分/温度计的写回逻辑。

**Effort:** Medium
**Risk:** Medium - 历史概念成员/股本为当前快照造成轻微前瞻偏差（报告中标注，窗口 2025 起偏差可控）；逐日全市场重算约分钟级

**Decisions to sanity-check:** ① 胜率按"每个换仓周期"计一笔交易（非按单只股票）；② 收益率用前复权价（adjclose）计算；③ 回测参数全部走命令行参数而非配置文件；④ 基准用市值前 300 等权代理（因无指数价格序列）。

Your next move: approve the plan, then run `$start-work sepa-backtest --worktree /data/codes/compass/.worktrees/sepa-backtest`. Full execution detail follows below.

---

> TL;DR (machine): Medium effort, Medium risk - 6 todos: backtest engine pure functions + run_backtest orchestration + CLI subcommand + Dolt write-back + docs; TDD, 95% coverage floor on compass-data.

## Scope
### Must have
- **回测引擎**（compass-strategy `src/sepa/backtest.rs`）：
  - `BacktestParams`（start/end/top_n/hold_days/cost）+ `BacktestResult`（每日权益曲线 + 摘要指标）数据类型
  - 组合模拟纯函数：逐日按评分取 TOP_N 等权持仓、持有 N 交易日换仓、0.1% 单边成本（参数化）
  - 绩效统计纯函数：累计收益、年化收益（252 交易日）、胜率、盈亏比、最大回撤、换仓次数
  - 基准代理纯函数：市值前300等权（`total_share × close` 口径，对齐温度计 temperature.rs:124-137）
  - `run_backtest` 编排函数：逐日调 `run_sepa(&SepaQuery{top_n}, reader, now)` 批量回算 + 组合模拟 + 指标 + 基准
- **CLI 子命令**（compass-data）：`compass-data sepa backtest --start YYYY-MM-DD --end YYYY-MM-DD --top N --days N --cost F --csv PATH`
- **Dolt 写回**：`backtest_result` 表（每日权益曲线：trade_date/strategy_nav/benchmark_nav/update_date，PK trade_date），range DELETE + `dolt table import -a` + `data_updates` 登记
- **CSV 输出**：权益曲线 CSV（含基准两列）
- **测试**：引擎纯函数（组合模拟/绩效统计/基准代理）+ run_backtest 集成（tempdir parquet 多日期）+ CLI 解析 + Dolt 端到端（tempdir dolt）
- **文档**：kb/user/cli.md 加 backtest 子命令、kb/design/backtest.md 新建（含 `## 决策记录`）、AGENTS.md kb 索引、data-providers.md 决策记录补 backtest_result 行

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不给 compass-types SEPA 类型加 serde 派生（违反文档化契约 lib.rs:216-220）——CSV 手写提取字段
- 不引入任何新依赖（无 csv crate、无统计库）
- 不实现历史主力资金流采集（后续 issue A2）
- 不实现 GUI 变更
- 不接指数价格序列（ts_index_weight 无价格 → 市值前300等权代理）
- 不实现卖出/止损止盈信号系统
- 不扩展 sepa_daily.sh 每日调度
- 不引入 PIT 快照 fixture 解决偏差（接受已知限制，报告中标注）
- 不重构 run_sepa / 引擎内部（逐日循环即可）
- 不改动现有 final_score/market_temperature 写回行为（只新增 backtest_result 路径）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD** — 每 todo 先写失败测试（RED）再实现（GREEN）；实现+测试=同一 todo
- Framework: cargo test（rstest `#[case]`、`#[cfg(test)]`）、Dolt tempdir（dolt init 无 commit + ENV_MUTEX 串行化）
- CLI 覆盖门槛：compass-data **95% 行覆盖**（check-coverage.sh 强制）；compass-strategy 80%
- Evidence: `.omo/evidence/` 目录收集测试输出（cargo test 输出重定向）
- 测试模式参照：compass-strategy/tests/sepa.rs Fixture::build（DuckDB → COPY TO parquet tempdir）、compass-data/src/sepa.rs setup_dolt/dolt_count

## Execution strategy
### Parallel execution waves
- **Wave 1**（引擎，compass-strategy，**串行执行 Todo 1 → Todo 2**——二者写同一文件 `sepa/backtest.rs`，不并行，避免同文件冲突）：Todo 1（组合模拟）、Todo 2（绩效统计+基准代理）
- **Wave 2**（编排，compass-strategy）：Todo 3（run_backtest 编排，依赖 1+2）
- **Wave 3**（写回，compass-data）：Todo 4（Dolt 写回，依赖 3）
- **Wave 4**（CLI + 文档）：Todo 5（CLI 注册，**依赖 3+4**——run_backtest_cli 调用 write_back_result）、Todo 6（文档，依赖全部）

> 注：waves 是依赖分层示意，实际执行由同一 executor **串行按依赖序**推进（Todo 1→2→3→4→5→6），每 todo 完成后立即 commit + review，与 Commit strategy 一致。

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 组合模拟纯函数 | — | 3 | 无（与 2 同文件，串行） |
| 2 绩效统计+基准代理纯函数 | — | 3 | 无（与 1 同文件，串行） |
| 3 run_backtest 编排 | 1, 2 | 4, 5 | — |
| 4 Dolt 写回 | 3 | 5 | — |
| 5 CLI 注册 | 3, 4 | 6 | — |
| 6 文档 | 1-5 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [x] 1. 组合模拟纯函数（compass-strategy `src/sepa/backtest.rs`）
  What to do / Must NOT do: 新建 `crates/compass-strategy/src/sepa/backtest.rs`。定义 `BacktestParams { start: NaiveDate, end: Option<NaiveDate>, top_n: usize, hold_days: usize, cost: f64 }`（`Default`: start=2025-01-01, end=None, top_n=50, hold_days=5, cost=0.001；**end 为 Option，None 由 run_backtest 用 latest_trade_date 解析——见 Todo 3，全计划统一此类型**）与 `EquityPoint { trade_date: NaiveDate, strategy_nav: f64, benchmark_nav: f64 }`。核心纯函数：`pub fn simulate_portfolio(ranked_daily: &[(NaiveDate, Vec<&SepaRow>)], daily_returns: &HashMap<String, HashMap<NaiveDate, f64>>, params: &BacktestParams) -> (Vec<f64>, Vec<usize>)`——返回 (每日 NAV 序列, **换仓日在序列中的索引列表**，供 Todo 2 胜率周期边界使用）。**收益时序约定（必须严格实现，避免 look-ahead）**：(1) 首日 NAV=1.0，首日组合收益=0；(2) **第 t 日组合收益 = 当前持仓（最近一次换仓日收盘选出，初始为首个评分日收盘选出）各成分 adjclose 日收益率的等权平均**，跳过当日无 finite 收益的成分（停牌不产生 NaN），若全部缺失则收益=0；**非换仓日持仓严格不变（hold_days 决定换仓时点，绝不逐日重选）**；(3) 换仓日在第 t 日**先计当日收益（旧持仓）再扣成本**：新买 1×cost、卖出旧仓 1×cost，即换仓日 NAV 乘 (1−2×cost)；初始建仓日（首个评分日）只扣买入 1×cost；(4) 每个评分日按当日评分取 TOP_N 等权建仓，持有 hold_days 个交易日后换仓；(5) **尾部截断**：窗口末尾不足 hold_days 时维持持仓到窗口结束，计入最后一个（可能截断的）周期，不再扣卖出成本；(6) 无评分日（当日无候选）保持 NAV 不变、不换仓。Must NOT: 不引入 serde、不碰 compass-types 类型（保持无 serde 契约）、不读文件（纯函数，数据由调用方注入）、不用 f64 无穷/NaN 传播（finite 校验）、不并行执行 Todo 2（同文件，串行）。在 `mod.rs` 注册 `pub mod backtest;`。
  Parallelization: Wave 1（串行，勿与 Todo 2 并行——同文件） | Blocked by: — | Blocks: 3
  References (executor has NO interview context - be exhaustive): crates/compass-strategy/src/sepa/mod.rs（模块注册）；crates/compass-strategy/src/sepa/scoring.rs:81-85（run_sepa 签名）、:312-316（top_n 逻辑）；crates/compass-types/src/lib.rs:259-290（SepaRow 字段：symbol/total_score/latest_price）；crates/compass-strategy/src/lib.rs:261-296（momentum_return/change_over 收益辅助可参考风格）；crates/compass-strategy/tests/sepa.rs:120-289（fixture 模式）、:298-336（bars/stock 辅助）
  Acceptance criteria (agent-executable): `cargo test -p compass-strategy backtest::` 通过；手算用例①：2 只股票 A/B、3 个评分日 d1<d2<d3、hold_days=2、cost=0，d1 建仓 TOP2（等权）、d2 持有、d3 换仓（重选 TOP2），NAV 序列精确匹配手算值；用例②：cost=0.001 时断言换仓日 NAV 扣减比例 = (1−0.002)（买入+卖出各 0.001），初始建仓日扣 0.001；用例③：hold_days=2 但窗口仅 3 日（尾部截断）→ d3 换仓后维持至结束，无额外卖出成本；用例④：某评分日 rows 为空 → NAV 保持前值不 panic；空输入返回 (vec![], vec![])。断言返回的换仓索引与 hand-calc 的换仓日一一对应
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass-strategy backtest_simulate` 输出 PASS；failure: 成分日收益含 NaN/非有限 → 该成分跳过、组合收益不为 NaN；全空输入返回空不 panic；Evidence .omo/evidence/task-1-sepa-backtest.txt
  Commit: Y | feat(strategy): add backtest portfolio simulation pure functions
- [x] 2. 绩效统计 + 市值前300基准代理纯函数（compass-strategy `src/sepa/backtest.rs`）
  What to do / Must NOT do: 同文件追加（**串行，勿与 Todo 1 并行**）。绩效统计 `pub fn compute_metrics(nav: &[f64], dates: &[NaiveDate], rebalance_indices: &[usize], benchmark_nav: &[f64]) -> BacktestMetrics`：`BacktestMetrics { cumulative_return: f64, annualized_return: f64, win_rate: f64, profit_loss_ratio: f64, max_drawdown: f64, rebalance_count: usize, benchmark_cumulative_return: f64, excess_return: f64, annualized_excess: f64 }`——累计=(末/首)-1（空或单点返回 0）；年化=(1+累计)^(252/交易日数)-1，交易日数=dates.len()-1，为 0 时返回 0 不 panic；**胜率/盈亏比按换仓周期计**：周期边界由 `rebalance_indices` 给出（含首尾——首周期从索引 0 到第一个 rebalance，中间周期 rebalance 间，末周期含截断尾部），每周期收益=周期末 NAV/周期首 NAV−1（含该周期边界成本），>0 记胜；盈亏比=平均盈利/平均亏损，**亏损为 0 时返回 0（文档化）**；最大回撤=遍历 NAV 峰值到谷值最大跌幅；benchmark_cumulative_return 同累计算法；excess_return = strategy 累计 − benchmark 累计；annualized_excess = strategy 年化 − benchmark 年化（按各自 dates 长度年化）。基准代理 `pub fn compute_benchmark_returns(bars_by_symbol: &HashMap<String, Vec<&CrossSectionBar>>, basics_by_symbol: &HashMap<String, &StockBasic>, dates: &[NaiveDate]) -> HashMap<NaiveDate, f64>`——每个交易日：市值=total_share×当日 close（**同时过滤 total_share 与 close 均须 finite 且 >0**，对齐 temperature.rs 校验），按市值降序取前 300（不足按实际），等权日收益率=各成分 adjclose 日收益率均值（跳过非 finite，全缺则该日无收益键）；无成分时该日返回 0 收益。**基准成员资格按当日收盘市值排名（多数指数代理同此惯例）——属基准自身的轻微 look-ahead，非策略问题；此口径须写入 kb/design/backtest.md 决策记录（Task 6）**。Must NOT: 不引入统计库、不修改 temperature.rs、不并行执行 Todo 1。
  Parallelization: Wave 1（串行，勿与 Todo 1 并行——同文件） | Blocked by: — | Blocks: 3
  References (executor has NO interview context - be exhaustive): crates/compass-strategy/src/sepa/temperature.rs:124-152（市值排序/前300口径：`share * s.close`、`ranked[..ranked.len().min(300)]`）、:57-60（函数签名风格）、:71-77（finite 校验模式）；crates/compass-core/src/model.rs:105-129（StockBasic.total_share）、:138-158（CrossSectionBar 字段）；crates/compass-strategy/src/sepa/indicators.rs:195-212（drawdown_from_high 参考——组合回撤需自写遍历）
  Acceptance criteria (agent-executable): `cargo test -p compass-strategy backtest_metrics` 与 `backtest_benchmark` 通过；手算 NAV [1.0, 1.1, 0.99, 1.2] 断言累计 0.2、最大回撤 0.1；3 周期 [赢,输,赢]（rebalance_indices=[1,2]）断言胜率 2/3、盈亏比正确；全赢时盈亏比 0 不 panic；excess：strategy 累计 0.2、benchmark 累计 0.1 → excess_return 0.1；基准：3 只股票市值 [1e9,2e9,3e9] 取前2等权、日收益 [0.1,0.2] 断言 0.15；close=0 或 NaN 的股票不进排名；空 dates/空 bars 返回空不 panic
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass-strategy backtest_metrics` PASS；failure: 全 NaN NAV 返回 0 不 panic；单只股票（不足300）按实际数量等权；无亏损周期盈亏比=0；Evidence .omo/evidence/task-2-sepa-backtest.txt
  Commit: Y | feat(strategy): add backtest metrics and benchmark proxy functions
- [x] 3. run_backtest 编排 + CSV 序列化（compass-strategy `src/sepa/backtest.rs`）
  What to do / Must NOT do: `pub fn run_backtest(params: &BacktestParams, reader: &ParquetReader) -> Result<BacktestResult, ScreenerError>`（**不再接收 SepaQuery——内部用 `SepaQuery { top_n: params.top_n }` 构造，单一 top_n 来源，杜绝双参数矛盾**）：`BacktestResult { points: Vec<EquityPoint>, metrics: BacktestMetrics }`。流程：(1) **end 解析 + start 校验（判据与 end 解耦）**：`let end = params.end.unwrap_or(reader.latest_trade_date()?.ok_or_else(...)?)`——None 时用最新交易日；独立校验 `start > reader.latest_trade_date()?` 时返回 `ScreenerError`（**判据 = start 晚于数据最新日，与 end 是否显式给定无关**；start>end 亦返回 Err）；(2) 交易日历=从 `reader.fetch_cross_section(start - 1日, end)` 取 distinct trade_date 升序（**日历含 start−1 首日，points 只输出 [start..end]**）；(3) **逐日 `run_sepa(&SepaQuery{top_n: params.top_n}, reader, date)` 对日历全部日期（含 start−1）调用**，收集 `(date, data.rows)`——**start−1 为初始建仓日**（收盘选股、扣 1×cost、当日收益 0，其 NAV 不输出）；**run_sepa 返回 Ok(空 rows) 属正常降级**（parquet 缺失时 fetch 返回空向量），空 rows 日期保留在日历中、组合模拟按 Todo 1 约定保持 NAV；(4) 从同一窗口 bars 构建 daily_returns（adjclose 日收益率，symbol→date→return，日历首日 start−1 收益=0）；(5) 调 simulate_portfolio（**首输出日 start 的 strategy_nav = (1−cost)×(1+r[start])，r[start]=start−1→start 收益**）+ compute_benchmark_returns + compute_metrics；(6) 组装 points（**基准 NAV 自 start−1 起从 1.0 复合，首输出日 benchmark_nav = 1.0×(1+r_bench[start])**）。**CSV 序列化**：`pub fn equity_csv(points: &[EquityPoint]) -> String` 手写（header: trade_date,strategy_nav,benchmark_nav；日期 %Y-%m-%d、数值 fmt_double 风格最多6位小数无指数）。Must NOT: 不写文件（CSV 字符串返回，由 CLI 层写盘）；不修改 scoring.rs；不缓存 run_sepa 内部数据；**不将 run_sepa 的空结果误判为错误**（仅实际子进程/查询失败传播 ScreenerError）。
  Parallelization: Wave 2 | Blocked by: 1, 2 | Blocks: 4, 5
  References (executor has NO interview context - be exhaustive): crates/compass-strategy/src/sepa/scoring.rs:81-94（run_sepa 调用与 fetch_cross_section 参数）、:312-316（top_n 逻辑）；crates/compass-core/src/data/parquet.rs:439-463（fetch_cross_section 签名/SQL）、:297（latest_trade_date——返回 Option<NaiveDate>）、:444-446（缺失文件返回空向量非错误）；crates/compass-data/src/sepa.rs:208-214（fmt_double 风格参考——compass-strategy 侧需自写等价小函数，不得依赖 compass-data）；crates/compass-strategy/tests/sepa.rs:120-289（集成 fixture：多日期 bars 生成 bars() 辅助 :298，INSERT 中 adjclose=close 便于手算）
  Acceptance criteria (agent-executable): `cargo test -p compass-strategy run_backtest` 通过；集成测试：fixture 造 300 日平坦前置窗口 + 10 个目标交易日、3 只股票（closes 精心构造使 run_sepa 评分 TOP 稳定——参考 tests/sepa.rs bars()/stock() 辅助 :298/:319），断言 points.len()==10、**points[0].strategy_nav 显式断言等于 (1−cost)×(1+r[start]) 的数值**（r[start] 由 fixture 手算）、points[0].benchmark_nav == 1.0×(1+r_bench[start])、metrics 数值与手算一致、equity_csv 输出 header+10 行且数值格式正确；end=None 时用 latest_trade_date；**start>end 断言返回 Err（ScreenerError）**；**start 晚于最新数据（即使 end 显式）断言返回 Err**；**全部 parquet 缺失（空 fixture）断言返回 Ok(空 points) 不 panic——测试须显式传 end（end=None 时 latest_trade_date→None→Err 为另一文档化路径）**
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass-strategy run_backtest` PASS；failure: start>end → Err；空数据（显式 end）→ Ok(空) 不 panic；start 晚于最新数据 → Err；Evidence .omo/evidence/task-3-sepa-backtest.txt
  Commit: Y | feat(strategy): add run_backtest orchestration and equity CSV
- [x] 4. Dolt 写回 backtest_result（compass-data `src/backtest.rs`）
  What to do / Must NOT do: 新建 `crates/compass-data/src/backtest.rs`（`mod backtest;` 注册于 main.rs:3 附近）。实现 `pub fn write_back_result(dolt_dir: &Path, points: &[EquityPoint], start: NaiveDate, end: NaiveDate) -> Result<(), Box<dyn Error>>`：DDL 常量 `BACKTEST_SCHEMA`=`CREATE TABLE IF NOT EXISTS backtest_result (trade_date DATE NOT NULL, strategy_nav DOUBLE, benchmark_nav DOUBLE, update_date DATE, PRIMARY KEY (trade_date))`；range DELETE `DELETE FROM backtest_result WHERE trade_date >= '{start}' AND trade_date <= '{end}'`；手写 CSV（csv_field/fmt_double 风格，**CSV 含 update_date 列=运行日墙钟**，对齐现有 write_back 的 today 处理）+ temp 暂存 + `dolt table import -a` + `dolt_upsert_updates(dolt_dir, "backtest_result", today, end, points.len())`（**today=运行日墙钟、report_date=end、row_count=points.len()**）。复用 sepa.rs 的 `dolt_sql`/`dolt_import`/`dolt_upsert_updates`/`csv_field`/`fmt_double`——当前是**私有 fn**（sepa.rs:411/427/447/203/208），**改为 `pub(crate)`（最小改动）**。测试（**本 todo 包含写回测试，勿推迟**）：`setup_dolt` 模式（sepa.rs:473-500），断言——(a) 写回后 `SELECT COUNT(*) FROM backtest_result` 总数 == points.len()（**注意：dolt_count 按单 trade_date 计数，这里必须用全表 count 辅助，或断言 `dolt_count(..., end) == 1`（range 边界行）而非 points.len()**）；(b) 幂等重跑：再次写回后总数不变；(c) data_updates 有 backtest_result 行且 last_report_date=end；(d) CSV 列序与 DDL 对齐（写回行可被 dolt 查询读出 strategy_nav/benchmark_nav 数值）。
  Parallelization: Wave 3 | Blocked by: 3 | Blocks: 5
  References (executor has NO interview context - be exhaustive): crates/compass-data/src/sepa.rs:30-77（DDL 常量模式）、:231-408（write_back 两段式）、:252-257（单日 DELETE——需改为 range）、:378-406（temp 暂存/import）、:411-461（dolt_sql/dolt_import/dolt_upsert_updates 私有 fn）、:203-214（csv_field/fmt_double）、:473-500（setup_dolt/dolt_count 测试辅助——注意 dolt_count 按单 trade_date 计数）；crates/compass-data/src/main.rs:3（mod 注册）、:304（ENV_MUTEX）；crates/compass-data/src/import_dolt.rs:19-37（run_dolt_sql_csv——全表 count 可复用）；crates/compass-strategy/src/sepa/backtest.rs（Todo 1-3 的 EquityPoint 类型）
  Acceptance criteria (agent-executable): `cargo test -p compass-data backtest_write` 通过；(a) 写回后全表 count == points.len()；(b) 重复写回幂等（总数不变）；(c) data_updates 有 backtest_result 行；(d) dolt 查询可读出与 CSV 一致的数值
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass-data backtest_write` PASS；failure: 空 points（0 行）跳过 import 不报错（对齐 write_back L391-394 行为）；dolt 缺失时错误传播；Evidence .omo/evidence/task-4-sepa-backtest.txt
  Commit: Y | feat(data): write backtest_result to Dolt with range delete
- [x] 5. CLI 子命令注册（compass-data `src/main.rs` + `src/backtest.rs`）
  What to do / Must NOT do: main.rs `SepaCmd` enum 加 `Backtest { start: Option<String>, end: Option<String>, top: Option<usize>, days: Option<usize>, cost: Option<f64>, csv: Option<PathBuf> }`（clap `#[arg(long)]`，start 默认 2025-01-01、end 默认最新、top 默认 50、days 默认 5、cost 默认 0.001）；`name()` 加 `SepaCmd::Backtest { .. } => "backtest"`；`run()` 分发加 arm：解析日期 `%Y-%m-%d`（复用现有模式 main.rs:271-276）、调 `backtest::run_backtest_cli(top, start, end, days, cost, csv, &reader, &dolt_dir)`，错误包装 `"Sepa {cmd_name} failed: {e}"`。在 backtest.rs 实现 `pub fn run_backtest_cli(...)`：构造 `BacktestParams`（start 默认 2025-01-01、end Option、top 默认 50、days 默认 5、cost 默认 0.001；**top 直接作为引擎 top_n——回测需要真实 TOP_N 持仓，不使用全市场计算+打印截断的 P0-1 约定**）、调 `run_backtest`、写 CSV 文件（若 --csv 给路径，用 `std::fs::write`）、stdout 打印摘要指标表（累计/年化/胜率/盈亏比/最大回撤/换仓次数/基准累计/超额/年化超额）、调 `write_back_result`。Must NOT: 不改 run_score/run_temperature 行为；不加 config 键；**测试必须覆盖 run_backtest_cli 主体**（compass-data 95% 行覆盖门槛——parse-only 测试不够）：端到端测试用 tempdir parquet fixture（tests/sepa.rs Fixture 模式）+ tempdir dolt（setup_dolt 模式），断言 CSV 文件内容、stdout 摘要、Dolt 行数。
  Parallelization: Wave 4 | Blocked by: 3, 4 | Blocks: 6
  References (executor has NO interview context - be exhaustive): crates/compass-data/src/main.rs:9（clap derive）、:120-132（SepaCmd 现有变体）、:134-142（name()）、:265-287（run() 分发）、:271-276（日期解析模式）、:525-546（CLI parse 测试模式）、:549-561、:564-572；crates/compass-data/src/sepa.rs:83-112（run_score 结构参照）、:96（SepaQuery{top_n: usize::MAX} 约定——回测不遵循此约定）、:117-134（run_temperature）；crates/compass-data/src/backtest.rs（Todo 4 的 write_back_result）；crates/compass-strategy/src/sepa/backtest.rs（Todo 3 的 run_backtest/BacktestParams）；crates/compass-strategy/src/sepa/scoring.rs:44（DEFAULT_TOP_N=50）；crates/compass-strategy/tests/sepa.rs:120-289（端到端 fixture 模式）；crates/compass-data/src/sepa.rs:473-500（setup_dolt/dolt_count）；kb/dev/testing.md:249-272（95% 覆盖门槛）
  Acceptance criteria (agent-executable): `cargo test -p compass-data cli_sepa_backtest` 通过；新增测试：`--start 2025-01-01 --days 5 --cost 0.001` 解析正确、默认值生效（BacktestParams 默认 2025-01-01/50/5/0.001）、非法日期报错；`cargo run --bin compass-data -- sepa backtest --help` 显示全部参数；**端到端测试**：tempdir fixture + tempdir dolt 下 run_backtest_cli 全链路（CSV 文件存在且含 header+行、stdout 含"累计"等摘要字段、dolt 有行）——此测试覆盖 CLI 主体，保护 95% 门槛
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass-data cli_sepa_backtest` PASS；failure: `--start not-a-date` 返回 `invalid --start` 错误；csv 路径不可写时错误传播；Evidence .omo/evidence/task-5-sepa-backtest.txt
  Commit: Y | feat(data): add sepa backtest CLI subcommand
- [x] 6. 文档同步（kb/）
  What to do / Must NOT do: (1) kb/user/cli.md 在 sepa 章节（L227-244）之后、`---`（L246）之前插入 backtest 子命令小节：用法示例 + 选项表（--start/--end/--top/--days/--cost/--csv）+ 输出说明（stdout 摘要 + CSV + Dolt backtest_result）；(2) 新建 `kb/design/backtest.md`：回测架构（逐日重算、组合模拟、基准代理口径、成本模型、指标定义）+ **`## 决策记录`** 表格（决策=每换仓周期计交易、adjclose 收益、252 年化、range DELETE、backtest_result 表结构、CLI flags 非 config、不给 SEPA 加 serde、已知 PIT 偏差、**基准成员资格=当日收盘市值（日 t 成员吃日 t 收益，文档化 look-ahead，Todo 2 强制）**）——格式对齐 data-providers.md:323 `| 决策 | 选项 | 选择 | 理由 | 排除原因 |`；(3) AGENTS.md kb 表格加 backtest.md 行；(4) data-providers.md 决策记录表（L336-348 区域）追加 backtest_result 表 DDL 决策行。Must NOT: 不编造未实现功能；不删除现有文档内容。
  Parallelization: Wave 4 | Blocked by: 1-5 | Blocks: —
  References (executor has NO interview context - be exhaustive): kb/user/cli.md:227-244（sepa 章节）、:246（--- 分隔）、:248（排障章节）；kb/design/data-providers.md:321-348（决策记录表结构 + sepa 行）；kb/design/ui.md:195（决策记录存在性对照）；AGENTS.md kb 表格（Knowledge base 章节）；kb/dev/testing.md:249-272（覆盖率门槛——文档需注明 backtest 新代码的覆盖要求）
  Acceptance criteria (agent-executable): 文档文件存在且含 `## 决策记录` 章节（grep -c "## 决策记录" kb/design/backtest.md 输出 1）；kb/user/cli.md 含 `sepa backtest` 字样；markdown 链接/格式检查（grep 无孤立反引号）；无 TODO/FIXME 残留
  QA scenarios (name the exact tool + invocation): happy: `grep -c "## 决策记录" kb/design/backtest.md` == 1；failure: 文档中提到但不存在的 CLI 参数（交叉核对 main.rs 实际参数）；Evidence .omo/evidence/task-6-sepa-backtest.txt
  Commit: Y | docs: document sepa backtest CLI and design decisions

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA
- [ ] F4. Scope fidelity

## Commit strategy
- 每个实现 todo 一个 commit，message 含 `ref #154`（issue 驱动，pre-push hook 校验）
- 顺序：1→2→3→4→5→6（**串行，每 todo 完成后立即 commit + `/review-work`，最多 2 轮修复**；Todo 1/2 同文件不可并行，Todo 5 依赖 Todo 4 的 write_back_result）
- 文档 commit（6）与实现同批
- push 前：rebase master（git fetch origin master + rebase）、用户确认后 `/reflect` 写反思 commit、再 push
- 禁止 `fixes #N`/`closes #N`（手动 gh issue close）

## Success criteria
- `cargo test` 全量通过（新增引擎/CLI/写回测试）
- `cargo clippy --all-targets` 无警告；`cargo fmt --check` 通过
- `cargo doc --no-deps` 无 missing_docs 警告（`#![warn(missing_docs)]` 合规，所有 pub 项带 `///`）
- coverage：compass-data ≥95%（backtest.rs/CLI 全链路端到端测试覆盖，含 run_backtest_cli 主体）、compass-strategy ≥80%
- `cargo run --bin compass-data -- sepa backtest --start 2026-07-01` 短窗口冒烟跑通（stdout 摘要 + CSV 文件 + Dolt backtest_result 行）；全窗口 2025-01-01 起用 `--release` 跑（预计分钟级，kb/design/backtest.md 记录预期耗时）
- kb/ 文档同步完成，kb/design/backtest.md 含决策记录
