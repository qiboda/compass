# sepa-engine - Work Plan（东方SEPA · 评分引擎层）

> 执行计划 2/3 — 覆盖 Batch 3（epic #139 子 issue #147-#149）
> 依赖：plan 1（sepa-collectors）todo 6（读取原语）+ todo 7（表就绪）。产出被 plan 3（sepa-delivery）消费。
> 配套：`.omo/plans/sepa-collectors.md`（数据就绪）、`.omo/plans/sepa-delivery.md`（交付）、`.omo/plans/sepa.md`（生命周期跟踪）、`.omo/designs/sepa-gui.md`（GUI 设计，契约类型定义处）

## TL;DR (For humans)

**What you'll get:** SEPA 系统的"大脑"——① 定义全部共享数据结构（评分行/详情/温度计等 7 个类型，GUI 与 CLI 都靠它）；② 技术指标库（均线/ATR/相对强度/VCP 形态/回撤，纯函数可独立测试）；③ 板块日线本地聚合 + 市场温度计（市场热度评分 + 仓位建议）；④ 五模块评分引擎（趋势 30% + 题材 25% + 资金 20% + 形态 20% + 风险 −5%）+ 股票池过滤 + 全市场 TOP50 排名入口。评分公式全部锁定（审查修订版），实现者零决策。

**Why this approach:** 引擎是纯函数层（compass-strategy mod sepa），与 IO 分离、可完全离线测试；契约类型先落地（否则 GUI/CLI 无法编译）；题材强度用本地成分股等权聚合（不依赖东财板块指数）；评分公式的每处细节（分母 90、风险贡献 −扣分×0.05、温度计阈值常量）都是审查后锁定的，直接照写。

**What it will NOT do:** 不做卖出信号；不调任何在线接口（温度计/板块聚合全部本地计算）；不写 Dolt/Parquet（只读输入 + 纯函数输出，写回属 plan 3）；不做历史批量回算（只算最新交易日）；不加 serde 到契约类型。

**Effort:** Large
**Risk:** Medium - 指标/评分的边界条件多（窗口不足/除零/NaN），TDD 必须覆盖；VCP 形态识别对噪声序列需有区分度
**Decisions to sanity-check:** 题材公式分母恒 90（有/无 news 满分均 25）、风险贡献 = −扣分合计×0.05 ∈ [−3.75,0]、SEPA_WINDOW_DAYS=550、温度计四项阈值（涨停 80 家 / 成交额 1.2 万亿）

Your next move: 批准后在 worktree 内按 Wave 3 执行；每子 issue 一个 commit（ref #N）。

---

> TL;DR (machine): Large effort, 3 todos chain (contract types → indicators+thermometer → scoring), pure-function engine in compass-strategy mod sepa, all formulas locked post-review, zero online calls, SEPA_WINDOW_DAYS=550.

## Scope
### Must have
- compass-types 契约类型 7 个：SepaQuery / SepaFactor / SepaDetails / SepaRow / SepaIndicator / MarketThermometer / SepaData（derive Debug/Clone/PartialEq，不加 serde）
- compass-strategy `mod sepa` 指标库：ma / atr20 / momentum_return / volume_ratio / rs_score / vcp_score / drawdown_from_high（纯函数，`&[&CrossSectionBar]` 切片风格，窗口不足返回 None/0 不崩溃）
- concept_daily 本地聚合（成分股等权：当日涨跌幅等权平均 + 成交额合计 + 上涨家数占比）
- 市场温度计（5 项截面代理，公式锁定，阈值写入模块常量）
- 五模块评分引擎（趋势 30 / 题材 25 / 资金 20 / 形态 20 / 风险 −5，公式全锁定）
- 股票池过滤（ST/退市、次新<60 交易日、20 日均额<3000 万、停牌 5 日、北交所剔除）
- 入口 `pub fn run_sepa(query: &SepaQuery, reader: &ParquetReader, now: NaiveDate) -> Result<SepaData, ScreenerError>`（TOP50 官方排序 + rank）
- 测试：rstest 指标边界 + 三场景温度计 fixture + 评分排序 fixture + 过滤规则逐一 + 覆盖率 ≥80%
- 文档：kb/dev/testing.md（如新增测试模式）

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不做卖出信号（风险模块不含卖出规则）；不调东财板块接口（concept_daily 本地聚合）
- 不写 Dolt/Parquet/任何持久化（写回属 plan 3 todo 11）；不做历史批量回算（只算最新交易日）
- 不加 serde 到 SEPA 类型；不改现有 run_screener 行为（mod sepa 是新增，不动 lib.rs 既有函数）
- 不新增外部 crate 依赖（纯 f64 运算，现有依赖已够）
- **评分公式禁止自行发挥**：题材分母恒 90（禁止 ÷80 归一化）、风险 = −扣分×0.05（禁止 "100−扣分×−0.05"）、温度计阈值用模块常量（禁止自定）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（先写失败测试再实现）— rstest + tokio::test + tempdir DuckDB COPY parquet fixture（tests/screener.rs 既有模式）
- Evidence: `.omo/evidence/sepa-engine/task-<N>-sepa-engine.<ext>`
- 质量门：`cargo test -p compass-types -p compass-strategy` + `cargo clippy` + `cargo fmt --check` + `cargo doc --no-deps`（#![warn(missing_docs)]，新 pub 项必须带 /// 注释）+ llvm-cov ≥80%
- **前置风险登记**：master 基线 `run_screener_emits_completion_log` flaky（open issue #138）——执行前先修 #138 或登记豁免，否则 `cargo test -p compass-strategy` 全量跑会被卡

## Execution strategy
### Parallel execution waves
- Wave 3: todo 8 → 9 → 10 链式（9 依赖 8 的契约类型；10 依赖 8+9）

### Dependency matrix（本 plan 内部 + 跨 plan）
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 8 (契约类型+指标库) | plan1 todo 6（读取原语） | 9, 10, plan3 todo 13 | — |
| 9 (聚合+温度计) | plan1 todo 6/7 + 本 plan todo 8（MarketThermometer 契约） | 10, plan3 todo 13 | — |
| 10 (评分+过滤+run_sepa) | 本 plan todo 8+9 | plan3 todo 11/13 | — |

跨 plan 依赖：plan 3 todo 11（CLI）依赖 todo 10；plan 3 todo 13（GUI）依赖 todo 8+10（进程内 run_sepa）。

## Todos

- [ ] 8. engine: 契约类型 + SEPA 指标库 — issue #147
  What to do / Must NOT do:
  **第一步：契约类型**（crates/compass-types/src/lib.rs 新增，GUI/CLI/引擎共用，derive `Debug, Clone, PartialEq`——**不加 serde**）：
  ```rust
  pub struct SepaQuery { pub top_n: usize }           // 后端截断上限（默认 50）

  pub struct SepaFactor {                              // 评分子项，GUI 零解析按原样渲染
      pub label: String, pub score: f64, pub max: f64, pub note: Option<String>
  }

  pub struct SepaDetails {                             // 五模块子项明细
      pub trend: Vec<SepaFactor>, pub theme: Vec<SepaFactor>,
      pub capital: Vec<SepaFactor>, pub pattern: Vec<SepaFactor>,
      pub risk: Vec<SepaFactor>,
  }

  pub struct SepaRow {
      pub symbol: String, pub name: String, pub rank: usize,
      pub total_score: f64,                            // 0..100
      pub trend: f64, pub theme: f64, pub capital: f64, pub pattern: f64,
      pub risk: f64,                                   // -3.75..0（扣分贡献，审查修订）
      pub industry: String, pub themes: Vec<String>,   // 题材可能为空
      pub latest_price: f64, pub change_pct: f64,      // 当日涨跌幅 %
      pub details: SepaDetails,
  }

  pub struct SepaIndicator {                           // 温度计指标 chip（GUI 通用渲染）
      pub label: String, pub value_text: String,
      pub delta_pct: Option<f64>,                      // 较昨日，A 股红涨绿跌
      pub heat: f64,                                   // 0..1 色阶 tint
  }

  pub struct MarketThermometer {
      pub score: f64, pub position: String,            // "80%-100%" 等
      pub position_pct: f64,                           // 0..100
      pub indicators: Vec<SepaIndicator>,              // 5 项
  }

  pub struct SepaData { pub rows: Vec<SepaRow>, pub thermometer: MarketThermometer, pub date: String }
  ```
  契约位置与 GUI 设计对齐：`.omo/designs/sepa-gui.md:247-276`。
  **第二步：指标库**（compass-strategy 新增 `mod sepa`；lib.rs 加 `pub mod sepa;`，现有 run_screener 不动）：
  全部纯函数，签名风格照抄 `lib.rs:225-229` 的 `fn ma(series: &[&CrossSectionBar], n: usize) -> f64`：
  - `ma(series, n)`：最后 n 根 adjclose 简单平均；`series.len() < n` → 返回 None（用 `Option<f64>`）或沿用现 ma 返回 f64 + 调用方守卫——**统一为 Option 风格更安全**，见下
  - `atr20(series)`：TR = max(high-low, |high-prev_close|, |low-prev_close|)，20 根平均；不足 21 根 → None
  - `momentum_return(series, days)`：`(latest - base) / base * 100.0`，base = 第 days+1 根 adjclose；不足 → None
  - `volume_ratio(series, days)`：近 days 日均量 / 前 days 日均量（量比）；不足 2×days → None
  - `rs_score(series, peers_momentum)`：个股 60 日×70% + 20 日×30% 加权动量 vs 板块内分位排名（0-1）；板块成分 <5 只 → 回落全市场排名；输入 peers_momentum 由调用方（todo 10）传入，本函数只做分位计算
  - `vcp_score(series)`：120 根内识别最多 3 个"波峰-回撤"周期（波峰 = 局部高点，回撤 = 波峰到后续谷底跌幅），按回撤收敛程度（≈20%→10%→5% 递减）给 0-1 分 + ATR20 收缩（当前 < 60 日前）加分 + 整理期缩量加分；不足 120 根 → 按实际根数降级评分，<30 根 → None；**对噪声序列（无收敛回撤）应得低分（区分度测试）**
  - `drawdown_from_high(series, days)`：距 days 日高点回撤百分比（正数表示回撤）；不足 → None
  - 窗口不足统一约定：返回 `None`/0 分，不 panic、不产生 NaN
  - 模块组织：`mod sepa` 内含 `indicators`（以上纯函数）与后续 todo 9/10 的 `aggregation`/`temperature`/`scoring` 子模块（按规模可单文件，但指标函数集中一个文件便于测试）
  Must NOT: 不改 lib.rs 既有函数（run_screener/ma/matches_* 等）；指标全部纯函数可独立测试（无 IO、无 reader 依赖）；不加 serde。
  Parallelization: Wave 3 | Blocked by: plan1 todo 6 | Blocks: 9, 10, plan3 todo 13
  References: `crates/compass-strategy/src/lib.rs:225-294`（指标风格）、`crates/compass-strategy/tests/screener.rs:14-129`（fixture 模式：TestBar/daily_series/rising_series）、`.omo/designs/sepa-gui.md:247-276`（契约类型）、epic #139 body 决策 24（RS 双窗口）、`crates/compass-core/src/model.rs:137-148`（CrossSectionBar，含 todo 6 扩展后的 9 字段）
  Acceptance criteria (agent-executable):
  - `cargo test -p compass-types -p compass-strategy` 全绿（契约类型编译 + 各指标 rstest）
  - `cargo doc --no-deps` 无 missing_docs 警告（新 pub 项全带 ///）
  - 覆盖率 ≥80%（compass-strategy crate 门槛，llvm-cov）
  QA scenarios:
  - happy: 各指标 rstest——rising_series（40 根 10→20 线性）ma/momentum 正值、下降序列反向、平台期零值
  - boundary: 空序列 / 不足 N 根 / base==0.0 / 除零 → None 不 panic；NaN 输入不传播
  - VCP 区分度: 构造典型 20%→10%→5% 收缩形态 → 高分（≥0.7）；构造无收敛噪声序列 → 低分（<0.3）；断言两者分差
  - Evidence: `.omo/evidence/sepa-engine/task-8-sepa-engine.txt`
  Commit: Y | feat(types): SEPA contract types; feat(strategy): SEPA indicator library

- [ ] 9. engine: concept_daily 本地聚合 + 市场温度计 — issue #148
  What to do / Must NOT do:
  **A. concept_daily 聚合**（mod sepa 内）：
  - 输入：`fetch_concept_member()`（plan1 todo 6 原语）+ `fetch_cross_section(range_start, now)`（含 amount）
  - 按 concept_code 分组：当日涨跌幅 = 成分股当日涨跌幅等权平均；成交额合计 = Σ amount；上涨家数占比 = 涨幅>0 家数 / 总数
  - symbol 归一化：collector 表带前缀（SH600519）、stock_daily 裸代码（600519）——用 `crates/compass-core/src/data/symbol.rs` 的 `parse_explicit_prefix` / 等价 helper 归一
  - 输出：板块聚合映射 `HashMap<concept_code, ConceptDaily { pct_change, amount, up_ratio, member_count }>`（内部结构，供 todo 10 题材模块消费）
  - 窗口：与引擎主窗口一致（SEPA_WINDOW_DAYS=550，todo 10 定义常量，此处引用）
  **B. 市场温度计**（公式锁定，阈值写入模块常量）：
  - `TEMP_LIMIT_UP_FULL: usize = 80`（涨停满分阈值，源自 epic 决策 2 牛市"涨停>80 家"）
  - `TEMP_AMOUNT_FULL_TRILLION: f64 = 1.2`（成交额满分阈值万亿，源自 epic 决策 2）
  - ① 沪深300代理 = 市值前 300 等权均线趋势分 = (>MA250 占比) × 30
  - ② 中证1000代理 = 市值 801-1800 等权均线趋势分 = (>MA250 占比) × 30（市值 = total_share × close，StockBasic.total_share）
  - ③ 涨停数分 = min(涨停数/80, 1) × 15（涨停 = 当日涨幅 ≥ 9.8% 计数，统一口径不按板区分）
  - ④ 成交额分 = min(amount 万亿/1.2, 1) × 15（全市场 amount 总和）
  - ⑤ 赚钱效应分 = (上涨家数占比) × 10
  - 总分 = ①+②+③+④+⑤ ∈ [0,100]
  - 仓位映射：≥80 → "80%-100%"；60-80 → "40%-70%"；<60 → "0%-20%"（position_pct = 区间中值或下界，取中值：90/55/10）
  - 输出 MarketThermometer（score/position/position_pct/indicators 5 项，每项 SepaIndicator{label, value_text, delta_pct, heat}——heat = 该项贡献分/该项满分）
  - 涨停数/上涨家数/成交额计算基于 `fetch_cross_section` 最新一根 bar（range_start = now − 550 天）
  Must NOT: 不调东财板块接口；市值分层用 total_share×close（不新增数据源）；温度计阈值用常量、禁止实现期自定。
  Parallelization: Wave 3 | Blocked by: plan1 todo 6/7 + 本 plan todo 8 | Blocks: 10, plan3 todo 13
  References: epic #139 body 决策 2/14/21（温度计代理算法与仓位映射）、`crates/compass-core/src/model.rs`（StockBasic.total_share）、`.omo/designs/sepa-gui.md:265-271`（SepaIndicator/MarketThermometer 契约）、`crates/compass-core/src/data/symbol.rs`（parse_explicit_prefix）
  Acceptance criteria (agent-executable):
  - `cargo test -p compass-strategy` 全绿（温度计三场景 + 聚合测试）
  - 三场景 fixture：牛市（前 300 与 801-1800 全 >MA250 + 涨停 ≥80 + 上涨占比高）→ score ≥80 → "80%-100%"；熊市（全 <MA250 + 涨停少）→ <60 → "0%-20%"；结构行情（指数分低但某板块强）→ 60-80 → "40%-70%"
  - 聚合测试：板块涨幅 = 成分股等权平均（fixture 2 成分 3%+5% → 4.0%）
  - 覆盖率 ≥80%
  QA scenarios:
  - happy: 牛市 fixture → 断言 score、position、position_pct、indicators.len()==5
  - boundary: 空市场（0 股票）/ 单股市场 → 不崩溃（0 分或按常量兜底）；涨停数 0 → ③=0
  - 归一化: 成分 symbol 带前缀 vs 裸代码混合 fixture → 聚合正确
  - Evidence: `.omo/evidence/sepa-engine/task-9-sepa-engine.txt`
  Commit: Y | feat(strategy): concept daily aggregation and market thermometer

- [ ] 10. engine: 五模块评分引擎 + 过滤 + run_sepa 入口 — issue #149
  What to do / Must NOT do:
  实现评分引擎（模块内百分制 → 按权重折算 30/25/20/20/−5，**公式全锁定，禁止自行发挥**）：
  **趋势 30%**（模块内 100 分 → ×0.30）：
  - 均线结构 45 分制：close>MA250 +18；MA60>MA120 +9；MA120>MA250 +9；MA250 向上（MA250 今日 > 5 日前） +9（等比缩放自 epic 决策 1 的 +10/+5/+5/+5）
  - 价格位置 20 分制：距一年高点回撤 <10% → 20；10-20% → 16；20-30% → 10；>50% → 0（等比缩放自 10/8/5/0）
  - RS 35 分制：板块内动量分位前 10% → 35，其余按分位线性递减（0-35）
  **题材 25%**（**锁定公式：题材分 = min((行业涨幅 30 + 行业成交额 30 + 行业扩散 20 + news_score)/90 × 25, 25)**——分母恒 90，news 缺失时 news_score 记默认 10，**满分恒 25，min cap 显式**（有 news=20 时 100/90×25=27.8 → cap 25）：
  - 分母恒 90，news 缺失时 news_score 记默认 10，**满分恒 25**（禁止 ÷80 归一化——会致有新闻满分 22.5 < 无新闻满分 25，排序颠倒）
  - 行业涨幅 30：板块 20/60 日动量加权（20 日×70% + 60 日×30%），归一化 0-30（全市场板块排名分位）
  - 行业成交额 30：板块当日成交额占全市场比例，0-30（分位）
  - 行业扩散 20：上涨占比×50% + 领涨带动×50%（前 5 涨幅股中 ≥2 只涨 >5% → 领涨满分；上涨占比 = 成分涨幅>0 比例）
  - news_score：默认 10/20（v1 无新闻数据，字段预留）
  - 数据源：todo 9 的 concept_daily 聚合映射
  **资金 20%**（模块内 100 分 → ×0.20）：
  - 量价配合 40：近 20 日上涨日均量 vs 下跌日均量（比值 ≥1.5 → 满分，线性 0-40）
  - 筹码集中 30：近 60 日涨幅 20-40% 后横盘 20 天 + 缩量（量比 <0.7）符合度 0-30
  - 大资金流入 30：主力净流入 20（近 5 日累计 main_net_inflow 分位）+ 龙虎榜机构 10（有机构净买入则得）+ 调研辅助 5（近期有机构调研则得）+ **大宗交易 ±5（block_trade 近 5 日 premium_rate 折价 >2% 加分、溢价 >2% 减分，审查补充落点）**；**min(30, 合计) 封顶**（含大宗调整后封顶）
  - 数据源：fetch_capital_main_flow / fetch_dragon_list / fetch_institution_survey / fetch_block_trade（plan1 todo 6 原语）
  **形态 20%**（模块内 100 分 → ×0.20）：
  - VCP 质量分 15：todo 8 vcp_score × 15
  - 突破确认分 5：close 创 120 日平台新高 + 量比 ≥1.5 → 5；距平台高点 <3% → 3；否则 0；**温度计联动**：温度计 ≥60 → 满分，40-60 → 半分，<40 → 0
  **风险 −5%**（**锁定公式：风险贡献 = −(扣分合计) × 0.05 ∈ [−3.75, 0]**）：
  - ATR>5%（ATR20/close） −20；120 日回撤 >30% −30；20 日涨幅 >30% 且量比 >3（放量滞涨） −25；扣分合计上限 75
  - 禁止 "100−扣分 再 ×(−0.05)"（无风险股 −5、全扣股 −1.25，方向颠倒）
  - 与 GUI 契约对齐：risk ∈ [−3.75, 0]，GUI norm = 1−|risk|/3.75（design 已同步）
  **过滤**（硬过滤，不进评分）：name 含 "ST"/"退" 剔除；上市 <60 交易日（list_date，60 交易日 ≈ 90 日历日）剔除；近 20 日均成交额 <3000 万剔除；近 5 个交易日无行情（停牌）剔除；北交所（exchange BJ）剔除
  **入口**：
  ```rust
  pub const SEPA_WINDOW_DAYS: i64 = 550;  // mod sepa 顶部，参照 lib.rs READ_WINDOW_DAYS:41
  pub fn run_sepa(query: &SepaQuery, reader: &ParquetReader,
                  now: NaiveDate) -> Result<SepaData, ScreenerError>
  ```
  - 流程照抄 run_screener（lib.rs:44-97）：fetch_cross_section(now−550, now) + load_all_stock_basics + 5 新表原语 → **先算市场温度计（todo 9，突破确认分消费其分数，必须在逐股评分循环前完成）** → 分组 → 逐标的过滤+评分（窗口不足跳过不崩）→ 总分排序（total_cmp NaN-safe）→ 截断 query.top_n → 组装 SepaRow（rank 官方顺序；**themes 来自 fetch_concept_member 按 symbol 分组取 concept_name**）+ SepaDetails（每模块子项 Vec<SepaFactor>）+ MarketThermometer（todo 9，温度计对象在评分循环前已算，此处复用）
  - symbol 前缀归一化：parse_explicit_prefix
  Must NOT: 不做卖出信号；风险模块不含卖出规则；不改 lib.rs run_screener；不写持久化。
  Parallelization: Wave 3 | Blocked by: 本 plan todo 8+9 | Blocks: plan3 todo 11/13
  References: epic #139 body 决策 1/10/11/12/17/18/19/22/23/25（评分细则全集）、`crates/compass-strategy/src/lib.rs:44-97`（run_screener 流程范本：分组/Ok(None) 短路/排序/日志）、`crates/compass-strategy/tests/screener.rs:131-143`（stock_000001 fixture）
  Acceptance criteria (agent-executable):
  - `cargo test -p compass-strategy` 全绿；覆盖率 ≥80%
  - 评分排序测试：fixture 市场（强趋势+热门题材股 vs 垃圾股）→ 断言总分排序（强者前）
  - 题材公式边界：有 news（如 15）→ 满分 25 可达；无 news（默认 10）→ 同样满分 25 可达（同模块其余项满分时）；断言两者不因 news 缺失而超过 25
  - 风险方向：无风险股 risk=0；全扣股 risk=−3.75（不出现 −5）
  - 过滤规则逐一测试（ST/次新/流动性/停牌/北交所各一用例）
  - 空结果：全部过滤后返回空 rows 不崩溃
  QA scenarios:
  - happy: fixture 评分排序断言（强者总分 > 弱者）
  - boundary: 窗口不足股票跳过（不 panic）；除零/NaN 不传播；空市场空结果
  - 公式锁定验证: 题材满分恒 25（有/无 news 两场景）；风险最差 −3.75（不越界）
  - Evidence: `.omo/evidence/sepa-engine/task-10-sepa-engine.txt`
  Commit: Y | feat(strategy): SEPA five-module scoring engine

## Final verification wave（本 plan）
> 并行运行，全部 APPROVE 后进入 plan 3。Surface results 并等用户确认。
- [ ] F1. 公式合规审计: 逐公式核对锁定值——题材分母 90（无 ÷80）、风险 −扣分×0.05（无 "100−扣分×−0.05"）、risk 范围 [−3.75,0]、温度计常量（80/1.2）、SEPA_WINDOW_DAYS=550、模块权重 30/25/20/20/−5；契约类型 7 个与 design 文档一致（无 serde）
- [ ] F2. 质量门: `cargo test -p compass-types -p compass-strategy` + clippy + fmt + doc --no-deps + llvm-cov ≥80%
- [ ] F3. 真实数据冒烟: 真实 Parquet 上跑 run_sepa（测试或小 bin）→ 输出 TOP 若干行、分数在合理区间（0-100）、无 panic；温度计分数合理（对照当日市场直觉性检查：涨停数/成交额与真实市场量级一致，客观验证数值区间而非目测）
- [ ] F4. 范围保真: 无卖出信号、无在线调用、无持久化写入、无 serde、无 run_screener 改动

## Commit strategy（本 plan）
- todo 8（`feat(types): ...` + `feat(strategy): ...` 合 1 commit，`ref #147`）、todo 9（`ref #148`）、todo 10（`ref #149`）
- 每 commit 后 /review-work（5 agent）；修复重 commit（≤2 轮）
- 全部 3 commit 在 .worktrees/sepa/ 同一 PR；push 前 rebase origin/master

## Success criteria（本 plan）
- 契约类型 7 个编译通过（GUI/CLI 可依赖）
- 指标库 + 温度计 + 评分引擎全部 TDD 全绿，公式与锁定值一致
- run_sepa 真实数据可跑出 TOP50 排序
- 覆盖率 ≥80%；F1-F4 APPROVE → 解锁 plan 3（sepa-delivery）
