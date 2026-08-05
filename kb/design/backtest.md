# SEPA 历史批量回测（backtest）

回测系统评估 SEPA 评分策略的历史绩效：对回测窗口内**每个交易日**重新运行
`run_sepa` 评分引擎（点内计算，窗口起点前推 550 交易日取截面，天然不偷看
未来），模拟"每日收盘后按当日评分取 TOP-N 等权持仓、持有 N 个交易日后换仓"
的组合管理策略，输出绩效指标并与市值前 300 等权代理基准对比。

实现分布：引擎纯函数与编排在 `crates/compass-strategy/src/sepa/backtest.rs`，
CLI 子命令与 Dolt 写回在 `crates/compass-data/src/backtest.rs`（`sepa backtest`）。

## 架构

```
run_sepa(SepaQuery{top_n}, reader, date)  ──逐日──►  ranked rows (per date)
                                                          │
ParquetReader.fetch_cross_section(start-1, end) ──► daily_returns (adjclose)
                                                          │
                                                          ▼
                                          simulate_portfolio (NAV 序列 + 换仓日索引)
                                          compute_benchmark_returns (市值前300等权)
                                                          │
                                                          ▼
                                          compute_metrics (绩效指标)
                                                          │
                                                          ▼
                          equity_csv / write_back_result (Dolt backtest_result)
```

- **引擎逐日重算**：不重构 `run_sepa`——按回测日历逐日调用，评分引擎本身已
  日期参数化（`now` 参数），每日本质独立。
- **日历**：从 `fetch_cross_section(start − 1, end)` 取 distinct trade_date
  升序。`start − 1` 是初始建仓日（其 NAV 不输出），输出窗口为 `[start..end]`。
- **组合模拟**（纯函数）：每个评分日收盘按当日评分取 TOP_N 等权建仓，持有
  `hold_days` 个交易日后换仓。第 t 日组合收益 = 当前持仓各成分 adjclose 日
  收益率等权平均（跳过非 finite 成分；全缺则该日收益为 0）。换仓日先计当日
  收益（旧持仓）再扣成本（新买 1×cost + 卖出 1×cost，即乘 `1−2×cost`）；初始
  建仓日只扣买入 1×cost。窗口尾部不足 hold_days 时维持持仓到窗口结束，不再
  扣卖出成本。无评分日保持 NAV 不变、不换仓。
- **收益口径**：前复权价（adjclose）日收益率——复权价跨除权日不断裂，避免
  分红送股造成的伪收益。
- **基准代理**：每个交易日按市值（`total_share × 当日 close`，两者均须
  finite 且 >0）降序取前 300 等权，日收益为各成分 adjclose 日收益率均值。
  无指数价格序列（`ts_index_weight` 只有成分权重），故用市值前 300 等权代理，
  与温度计口径一致。
- **绩效指标**（纯函数）：累计收益、年化收益（252 交易日）、胜率/盈亏比
  （**按换仓周期计**——每周期收益 = 周期末 NAV / 周期首 NAV − 1，>0 记胜）、
  最大回撤、换仓次数；基准侧：累计收益；对比：超额收益、年化超额。
- **成本模型**：统一单边 0.1%（参数化），买入与卖出各收一次。

## CLI

```
compass-data sepa backtest [--start YYYY-MM-DD] [--end YYYY-MM-DD]
                           [--top N] [--days N] [--cost F] [--csv PATH]
```

默认 `--start 2025-01-01`、`--end` 最新交易日、`--top 50`、`--days 5`、
`--cost 0.001`。stdout 打印摘要指标表；`--csv` 写权益曲线 CSV；Dolt
`backtest_result` 表存每日净值曲线。用法见 `kb/user/cli.md`。

## Dolt 写回

`backtest_result` 表（PK trade_date），按窗口 range DELETE + `dolt table
import -a` 追加（幂等可重跑），`data_updates` 同步登记：

```sql
CREATE TABLE IF NOT EXISTS backtest_result (
  trade_date   DATE NOT NULL,
  strategy_nav DOUBLE,
  benchmark_nav DOUBLE,
  update_date  DATE,
  PRIMARY KEY (trade_date)
)
```

## 已知限制

- **概念成员/ST 状态为当前快照**：历史回测中，某股票在历史上是否属于某概念、
  是否 ST，用的是当前时点的成分/状态 → 轻微前瞻（look-ahead）偏差。回测窗口
  2025 年起、偏差可控，报告中标注。
- **主力资金流无历史**：`capital_main_flow` 为纯快照无历史 → 资金模块自动
  降级（量价配合 + 筹码集中，大资金流入归 0）。历史主力资金流采集列为后续
  issue（A2）。
- **基准成员资格 = 当日收盘市值**：日 t 的基准成员由日 t 收盘市值排名决定并
  吃掉日 t 收益——这是基准自身的轻微 look-ahead，多数指数代理同此惯例，属
  可接受口径而非策略问题（见决策记录）。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 回测数据路径 | 重构引擎支持批量 / 逐日调 run_sepa 重放 | 逐日调 `run_sepa(&SepaQuery{top_n}, reader, now)` | 引擎已日期参数化，点内计算天然防偷看；零重构风险 | 重构引擎引入回归面，且评分逻辑与回放职责混淆 |
| 资金因子处理 | 回补历史主力资金流 / 引擎降级 | 引擎自动降级（量价配合+筹码集中，大资金流入归 0） | 资金流表为空时引擎已验证不崩；回补需新写采集器，列后续 issue | 回补工作量大且超出本 PR 范围（A2 后续项） |
| 回测窗口 | 全历史 / 2025-01-01 至今 | 2025-01-01 至今（参数化可调） | 概念成员快照偏差 × 样本量平衡 | 更早窗口放大快照偏差，且计算量线性增长 |
| 策略模拟 | 每日全仓换手 / 持有 N 日换仓 | 每日收盘按当日评分取 TOP50 等权，持有 N=5 交易日换仓（N 参数化） | 换仓频率与实际操作节奏一致，减少交易成本敏感性 | 每日全仓换手成本极高且不现实 |
| 胜率/盈亏比口径 | 按单只股票计交易 / 按换仓周期计 | 每个换仓周期计一笔交易 | 组合层面每周期一次决策，统计口径简单清晰 | 按个股统计需逐持仓归因，依赖每日明细（未持久化） |
| 收益价格口径 | 原始价 close / 前复权 adjclose | adjclose 日收益率 | 复权价跨除权日不断裂，避免分红送股伪收益 | close 在除权日产生跳空，扭曲收益统计 |
| 年化方式 | 365 自然日 / 252 交易日 | 252 交易日 | A 股交易日惯例，与温度计口径一致 | 自然日年化低估实际年化率 |
| 基准 | 沪深300 指数价格 / 市值前300等权代理 | 市值前300等权（total_share × close，与温度计一致） | ts_index_weight 只有成分权重无指数价格序列；零新依赖 | 接入指数价格需新数据源；前300等权与温度计口径对齐便于对照 |
| 基准成员资格 | 期初固定 / 当日收盘市值逐日更新 | 当日收盘市值排名（日 t 成员吃日 t 收益） | 多数指数代理同此惯例；简单无需成员快照表 | 期初固定需历史成员数据（不可得）；注意此为基准自身轻微 look-ahead，非策略问题 |
| 交易成本 | 零成本 / 统一单边 0.1% | 统一 0.1% 单边（买入/卖出各收，参数化） | 反映实际摩擦成本，参数可调 | 零成本高估策略收益，掩盖换仓频率影响 |
| backtest_result 表结构 | 存每日持仓明细 / 只存每日净值 | PK(trade_date) + strategy_nav/benchmark_nav/update_date | 净值曲线足够支撑绩效复盘与画图；明细体积大且可重算 | 明细持久化无查询需求，徒增表体积 |
| 写回方式 | 按窗口整体 REPLACE / range DELETE + append | range DELETE `WHERE trade_date BETWEEN start AND end` + `dolt table import -a` | 幂等重跑（同窗口重跑行数不增）；与 sepa.rs 两段式模式一致 | REPLACE 需整表转义；range 精确限定窗口避免误删窗口外历史 |
| CLI 参数 | 配置文件 / 命令行 flag | 全部走 clap flag（start/end/top/days/cost/csv） | 回测是一次性分析命令，参数即用即传；配置文件适合长期运行任务 | config 键为重复运行场景设计，回测频率低不值得加键 |
| SEPA 类型序列化 | 给 compass-types 加 serde 派生 / 手写 CSV 提取 | 手写提取字段（equity_csv 等） | 加 serde 违反既有"不加 serde"契约（lib.rs 文档化），且波及 GUI 依赖 | serde 派生改动公共类型面，影响面超出回测需求 |
| 已知 PIT 偏差 | 构建历史快照 fixture / 接受限制并标注 | 接受概念成员/ST 当前快照偏差，报告中标注 | 构建 PIT 快照需历史版本数据（不可得或成本极高）；窗口 2025 起偏差可控 | PIT fixture 是重型工程，收益与成本不匹配 |
