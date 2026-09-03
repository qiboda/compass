# Plan: fix/mainflow-null-rows-filter

> Worktree: `.worktrees/fix-mainflow-null-rows-filter`
> Issue: [#348](https://github.com/qiboda/compass/issues/348)（单 issue 单批）
> 基础: master `1ea7dbe`（worktree 会话启动时 HEAD == origin/master，已同步）
> 分支: `fix/mainflow-null-rows-filter`

## 背景

2026-09-02 更新数据库后抽查 2026-08-19 ~ 2026-09-02，`capital_main_flow` 有
3,042 行全 NULL 空行（`main_net_inflow` 及全部 flow 列均为 NULL；全表 134,755
行，占 2.26%）。2,997 行为退市股在退市日期之后仍被写入，45 行为上市中但当日
无数据（停牌）。

### 根因（含本 worktree 会话的调查修正）

主 session handoff 记录的根因是 `backfill_symbols()` 未按 list/delist 日期过滤。
本会话进一步实验确认了**完整机理**（记录用于 issue 收尾 comment 与决策记录）：

1. **NULL 行产生于 #339（Sina 切换，commit `ee3edfd` 2026-08-30 合入）之前**：
   NULL 行 `update_date ∈ {08-02, 08-17..08-22, 08-26..08-29}`，全部早于 8-30，
   即 EastMoney push2 时代遗留（全市场截面含全部 stock_basic，退市/停牌股
   f62 缺失 → 空值写入）。Dolt 与 Parquet 均含这些空行（非一致性 bug）。
2. **Sina 时代不会再产生 NULL 行**（实测，curl 验证）：
   - 退市股（SH600001，2009-12-29 退市）→ Sina 返回**历史页**（2010-10-25~
     11-01，数值全 0.0000）→ 日期不在窗口内被 `filter_daily_window` /
     `extract_backfill_window` 过滤 → 不写入。
   - 停牌股（SH600984 8-11~8-24 停牌）→ Sina 返回**当日占位行**（opendate=当天、
     r0_net=0.0000 等全 0）→ 解析为 `main_net_inflow="0"`（`parse_sina_row` 0.0 →
     fmt "0"），**非 NULL**；仅当 backfill 区间覆盖停牌期或每日补跑窗口
     `trade_date > last_report_date` 时才会以 0 值行写入（见「已知边界 2」）。
3. **Sina 时代仍存在的问题**（本 PR 修的是这里）：
   - `backfill_symbols()` 全量 5908 只（含 354 只退市股）→ 退市股被无意义请求
     （每只 100ms 节流 + 网络往返）；在 #342 严格失败模式下，任一退市股接口
     异常（Sina 对部分退市股可能返回异常/超时）→ `BackfillSymbolFailed` →
     auto-heal / update-database.sh 整体失败。
   - 显式 `--symbols` 场景（CLI 手动回补）同样不过滤：用户指定退市股时请求
     无意义且空转。
   - 无防御性导入守卫：任何路径（历史上 EastMoney、未来代码变更）都可能在
     `capital_main_flow` 留下 NULL 行。

## 锁定决策（grill 已确认，contract）

1. 代码修 + 清理现有脏数据（Dolt/Parquet 删除 NULL 空行）。
2. 每日采集（run）与历史回补（backfill）共用 active-symbol 过滤。
3. 显式 `--symbols` 同样按活跃区间过滤。
4. 过滤条件：`list_date` 为空或 `CAST(list_date AS DATE) <= end`；`delist_date`
   为空或 `delist_date >= start`（即目标区间内至少一天处于上市状态）。
5. 防御性导入守卫：禁止 `main_net_inflow` NULL 行写入 `capital_main_flow`。

## 设计

代码变更集中在 `crates/compass-collectors/src/main_flow.rs`（+ 文档）。

### 1. active-symbol 查询（替代 `backfill_symbols()`）

- 新增纯函数 `active_symbols_sql(start: &str, end: &str) -> String`：实现拼为
  **单行**（Rust `\` 续行符，实际执行 SQL 无换行/回车；需求测试锁定单行形态）：
  ```sql
  SELECT symbol FROM stock_basic WHERE (list_date IS NULL OR list_date = '' OR CAST(list_date AS DATE) <= '<end>') AND (delist_date IS NULL OR delist_date >= '<start>') ORDER BY symbol
  ```
  - 空串分支（`list_date = ''`）为 Dolt 2.3.1 实测必需：`CAST('' AS DATE)` 按
    MySQL 语义得 NULL（`'' IS NULL`=FALSE、CAST 后 NULL → 比较为 NULL →
    WHERE 排除），不加显式空串分支会丢掉 `list_date=''` 的上市股（本机
    stock_basic 无空串行，但契约按测试 14 锁定保留；经评审 HIGH-1 裁定
    「SQL 补空串分支，测试保持」）。
  - 与 index_daily 同款 `dolt_sql_csv` 拼接风格；`start`/`end` 的 SQL 注入防御：
    调用方契约（`backfill` 内 `NaiveDate::parse_from_str` 先行校验 ISO 格式；
    `run()` 的 `today()` 由 chrono 生成），helper 文档注明。
  - 已验证（只读查询）：全量 5908 vs active（2026-08-19..09-02）5554，
    差 354 = 全部退市股；被排除样例均为退市股。
- 新增纯函数 `parse_symbol_csv(out: &str) -> Vec<String>`：从 `backfill_symbols()`
  现有的 CSV 解析（trim/lines/skip(1)/filter empty）抽出，可单测；过滤规格含
  字面 `"NULL"` 文本行（`!= "NULL"`）——dolt CSV 输出中真 NULL 为空字段、
  字符串 "NULL" 为字面文本（实测区分），两者都不应成为 symbol（评审 MED-3
  裁定：设计规格与测试计划 L135 对齐）。
- 新增 `async fn active_symbols(start: &str, end: &str) -> Result<Vec<String>>`：
  `.dolt` 存在 → `dolt_sql_csv(active_symbols_sql(...))` + `parse_symbol_csv`；
  无 `.dolt`（测试/开发环境）→ `vec!["SH600519"]`（保留现有 fallback 语义）。
  结果为空 → `Err(InvalidInput("stock_basic contains no symbols"))`
  （延续现有保护）。
- **删除** `backfill_symbols()`（无其他调用方：grep 确认仅 `run()`/`backfill()` 使用）。

### 2. run()（每日采集）

`let symbols = active_symbols(&today, &today).await?;` 替换原
`backfill_symbols().await?`（main_flow.rs:324）。语义：只采集"今天处于上市
状态"的股票（`delist_date >= today` 或 NULL；`list_date <= today` 或 NULL），
退市股不再请求。

### 3. backfill(start, end, symbols)（历史回补）

```rust
let active = active_symbols(start, end).await?;                 // 已过滤
let symbol_list = match symbols {
    Some(s) => filter_active_symbols(s, &active_set),           // 显式列表 ∩ 活跃
    None => active,
};
```

新增纯函数
`filter_active_symbols(symbols: &[String], active: &HashSet<String>) -> Vec<String>`：
按输入顺序保留在 active 集中的 symbol（不去重，保持现有语义）。

**过滤后为空的语义**（门禁决策点，见开放问题）：
- `Some(s)` 全部被过滤（如用户对退市股手动 backfill）→
  `Err(InvalidInput("backfill: all {n} requested symbols are outside the active window in {start}..{end}"))`
  ——如实报错（区别于现有误导性 "no symbols to fetch"），CLI 用户可知原因。
- `None` 路径理论上非空（5554 只）；若 Dolt 数据异常导致空 → 同名 Err 保护。

（备选 no-op 语义会误导 CLI 用户"已成功回补"，不推荐；交由用户确认。）

### 4. import_to_dolt 导入守卫

`insert_sql` 追加 `AND main_net_inflow IS NOT NULL`（现有
`WHERE symbol IN (SELECT symbol FROM stock_basic)` 之后）：
```sql
INSERT IGNORE INTO capital_main_flow (symbol, trade_date, ...)
SELECT symbol, trade_date, ... FROM _tmp_mf
WHERE symbol IN (SELECT symbol FROM stock_basic)
  AND main_net_inflow IS NOT NULL
```
CSV 空字符串经 `dolt table import` 入 Dolt 后即为 NULL（现有 3,042 行脏数据
正是此形态），守卫阻断任何路径的 NULL 行写入。

### 5. orchestrate.rs

不变：`backfill(start,end,None)` / auto-heal 调用路径自动受益（None → 过滤后
活跃列表），`require_nonzero` 语义保留。

## 测试计划（RED → GREEN）

### 对抗性测试（subagent_skwy_adversarial_test，门禁 3.5）

攻击点：
- `active_symbols_sql`：未上市（list_date > end）排除；退市（delist < start）
  排除；**区间内退市（delist ∈ [start,end]）保留**（至少一天上市）；`list_date`
  NULL/空串 保留；`delist_date` NULL 保留；单日窗口 start==end。
- `filter_active_symbols`：混合列表只留 active；全 not-active → 空；输入顺序
  保持；重复 symbol 保持；active 含额外 symbol 不影响。
- `parse_symbol_csv`：header 行跳过；`NULL` 文本行；空白行；无 dolt header 边界。
- 过滤后为空 → Err 消息**不含** "no symbols to fetch"（防误导性回归）且含
  start/end 与请求数。
- import 守卫 SQL 文本：含 `main_net_inflow IS NOT NULL`，且与 stock_basic
  子查询并存（and 语义）。
- 不变量：`active_symbols_sql` 生成的 SQL 中 start/end 不出现引号注入面
  （非法日期在 `backfill` 入口已被 `parse_from_str` 拒绝 —— 现有测试
  `backfill_malformed_start_date_rejected_before_network` 覆盖）。

### 需求测试（subagent_skwy_requirement_test，门禁 4）

- `active_symbols_sql` 生成正确的 WHERE（决策 4 语义逐条断言）；
- run() 调用点使用 `active_symbols(today, today)`（编译期/行为断言）；
- backfill None → 使用 active_symbols 过滤结果；Some → filter_active_symbols；
- 守卫 SQL 存在；空列表 Err 消息语义；
- 既有测试全绿回归（`backfill_rejects_inverted_*`、`backfill_empty_symbols_errors_before_network`
  等）。

### 实现后独立验证

- `cargo test -p compass-collectors`、`cargo clippy -p compass-collectors`、
  `cargo fmt --check`。
- 委派 `subagent_skwy_requirement_test` 独立 QA 复核（验证者与实现者分离）。
- 真实数据定向冒烟：
  - `main-flow-backfill --start 2026-08-19 --end 2026-08-28 --symbols SH600001`
    （退市股）→ 应报 "outside the active window" 类错误（不入网、无 CSV）。
  - `main-flow-backfill --start 2026-08-19 --end 2026-08-28 --symbols SH600519`
    （正常股）→ 正常产出 CSV（有行）。
  - 不跑全量 update-database.sh。

## 数据清理（实现验证后执行）

1. `dolt sql -q "DELETE FROM capital_main_flow WHERE main_net_inflow IS NULL"`
   （当前 3,042 行；删除前记录 COUNT 快照）。
2. `dolt commit` + `dolt push origin main`（compass_data 仓库，必须）。
3. 无 `--since` 全量 `cargo run --bin compass-data -- import-compass
   --table capital_main_flow` 重建 Parquet（import 自带行数/一致性校验）。
4. 校验：Dolt `COUNT(*) WHERE main_net_inflow IS NULL` == 0；
   Dolt 总行数 == Parquet 行数；不 export DuckDB（锁定决策）。

## 文档同步（门禁 5b）

| 文件 | 变更 |
|---|---|
| `.dsh/kb/design/data-providers.md` | #339 决策行（约 597 行）补充：采集目标按 `stock_basic` list/delist 活跃区间过滤（issue #348）；「限制」段同步；`## 决策记录` 表新增 #348 行（what/why/why-not，含根因修正与数据源口径边界） |
| `.dsh/kb/user/cli.md` | `main-flow-backfill`（约 270-271 行）补一句：symbols/全量均按活跃区间过滤，退市股不请求；自动回补不再产生空行 |

决策记录检查（门禁 5c）：`data-providers.md` 已有 `## 决策记录` 章节 ✅
（本 PR 在其新增一行，不另建文档）。

## 实施顺序与 commit 计划

| # | 步骤 | commit |
|---|---|---|
| 1 | 对抗性测试 RED（子代理产出，主 agent 审查后提交） | `test: adversarial ...` `ref #348` |
| 2 | 需求测试 RED（同上） | `test: requirement ...` `ref #348` |
| 3 | 实现 GREEN（设计 1-4 落码） | `fix: filter active symbols ...` `ref #348` |
| 4 | 本地验证（test/clippy/fmt）+ 独立 QA 复核 | — |
| 5 | 真实数据冒烟 + 数据清理（Dolt 操作，独立于 git commit） | — |
| 6 | 文档同步（data-providers.md / cli.md，含决策记录） | `docs: sync kb for #348` `ref #348` |
| 7 | review（subagent_review，必要时修复 commit） | `ref #348` |
| 8 | 反思（skwy-reflect，用户确认 push 后） | `docs: #348 reflection` `ref #348` |
| 9 | rebase origin/master → push → PR → issue #348 收尾（comment + close） | — |

每个 commit 独立行 `ref #348`；Dolt 写库后立即 commit/push；push 前 rebase。

## 已知边界（记录，不处理 —— 除非用户要求）

1. **stock_basic delist_date 与 Sina 历史数据口径不一致**：SH600001 在
   stock_basic 中 delist 2009-12-29，但 Sina 仍返回 2010-10-25~11-01 数据。
   按锁定决策 4 过滤后，此类股票的"退市后"Sina 数据无法再通过 backfill 补齐
   （保守：宁可缺数据、不要失真数据；数据源口径差异另按需追查）。
2. **停牌占位行**：Sina 对停牌日返回全 0 占位行（r0_net=0.0000），当前解析为
   `main_net_inflow="0"`（非 NULL）并可能经 backfill 写入 0 值行。issue #348
   锁定范围仅 NULL 行；0 值占位行是否一并排除属**范围扩张**，见开放问题 Q2。
3. **run() 每日锚点 (today, today)**：delist_date == today（退市当日）的股票
   仍会被采集一次，无害（窗口数据在退市前；Sina 无退市后新数据可返回）。
   last_report_date 严重滞后（多日未跑）且期间有股票退市时，其退市前最后
   交易日数据可能漏采（边缘；每日管线正常情况下不发生）。
4. **run() 调用点无行为级测试锁定**（QA minor 1，记录缺口）：范围声明
   「run() 使用 active_symbols(&today, &today)」仅由编译期验证
   （main_flow.rs:324 调用 + #339 遗留签名测试），无断言锁定 run() 内部
   接线；提取可测辅助函数需重构 run() 结构，超出 #348 范围，如需锁定另开
   issue 跟进。

## 开放问题（用户 grill 确认，已决）

- **Q1 过滤后为空语义**：显式 `--symbols` 被活跃过滤清空时 —
  **(a) Err 明确消息（已确认）**：`backfill: all N requested symbols are outside
  the active window in {start}..{end}`，且不含旧 "no symbols to fetch" 文案。
- **Q2 停牌占位 0 值行**：是否本 PR 一并处理 — **仅记录边界（已确认）**
  （范围外；Sina 全 0 占位行解析为 "0" 值写入而非 NULL，守卫不拦；#339 锁定
  的字段映射口径不变）。
