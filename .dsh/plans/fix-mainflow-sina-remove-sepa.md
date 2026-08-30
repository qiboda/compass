# Plan: fix/mainflow-sina-remove-sepa-empty

> Worktree: `.worktrees/fix-mainflow-sina-remove-sepa-empty`
> Issues: #338（既有，复用）/ #339（新建）/ #340（新建）
> 一个 PR，实现 commit 各 ref 一个 issue（另有 review-fix 与 docs commit，
> 共 7 个 commit：ee3edfd #339 / ec5dfcf #338 / 200319c #340 / 40e68b7 docs /
> d7e71b6 #339 review-fix / eccc7bd #340 timing-test-fix / 8125451 docs）。

## 背景

1. `capital_main_flow`（主力资金流日频表）当前依赖 EastMoney
   push2（clist 全市场快照）与 push2his（fflow daykline 历史回补），
   两套端点在本环境不可达（HTTP 000/TLS EOF），是 #338 同源故障。
2. `scripts/update-database.sh` 每日自动计算 SEPA（step5 backfill-dates、
   step6 temperature+score、step7 compute commit、step8 TOP50），经用户
   grill-me 决策：从每日管线彻底移除（CLI 保留可手动运行）。
3. 非交易日（周末）`sync()` 对日频表空增量误判失败：dragon_list 等
   `run()` 拿到 0 行后删除 CSV → `import_to_dolt` 因文件缺失返回 `Ok(0)`
   → `require_nonzero(0)` 失败，中断 `update-database.sh`。

## Tasks（单批，3 个独立交付物）

| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #339 | capital_main_flow 采集/回补切换新浪逐股拉取 | — |
| done | #340 | update-database.sh 移除每日 SEPA 自动计算（step 5-8） | — |
| done | #338 | sync 日频表空增量按日历判定（无交易日 no-op） | — |

## Issue #339 — capital_main_flow 切换新浪

### 设计

- **替换** `crates/compass-collectors/src/main_flow.rs` 全部网络路径：
  - 删除 `PUSH2_URLS` / `PUSH2_*`、`FFLOW_DAYKLINE_URL`、`fetch_page`、
    `fetch_snapshot`、`snapshot_params`、`symbol_to_secid`（EM 专用）。
  - 新增常量：`SINA_URL = "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/MoneyFlow.ssl_qsfx_lscjfb"`；
    `SINA_DAILY_NUM = 20`（窗口增量）；`SINA_BACKFILL_NUM = 1000`（历史回补）。
  - `http.rs` 新增 `SINA_MIN_INTERVAL = Duration::from_millis(100)`。
- **`run()`（每日增量）**：签名 `run() -> Result<PathBuf>`（去掉 page_size）。
  1. `last_report_date(DOLT_TABLE)`：若 `== today` 跳过（沿用）。
  2. `backfill_symbols()`（stock_basic symbol 列表，沿用）。
  3. 逐股请求 `page=1&num=20&sort=opendate&asc=0&daima=<daima>`，重试 3 次
     （每次 2s/4s 退避），全部失败/响应非数组 → `eprintln` 警告并跳过该股。
  4. 解析 `opendate` → `trade_date`；**只保留 `trade_date > last_report_date`**
     （无锚点 → 全部保留）。
  5. 0 新行 → `remove_file(csv)` + `Ok(路径)`（与 dragon::run 周末行为一致，
     import 因缺文件返回 0 → 由 #338 日历守卫判定 no-op）。
  6. 非空 → `write_csv`。
- **字段映射**（与旧 EastMoney 口径一致；rate 为百分数，与历史 f184 行同量纲，2026-08-30 用户确认 ×100）：
  `main_net_inflow = r0_net + r1_net`；`main_net_inflow_rate = (r0_net+r1_net)/(r0+r1+r2+r3)×100`（除零→0）；
  `super_large_net = r0_net`；`large_net = r1_net`；`medium_net = r2_net`；`small_net = r3_net`；
  `trade_date = opendate`；`update_date = today()`。
  `netamount`/`ratioamount`（全口径总净额/占比）不复用。
- **`backfill(start,end,symbols)`**：`page=1&num=1000` 逐股（沿用现有逐股循环与
  seen 去重、`[start,end]` 过滤、`seen.is_empty() → Err`、排序写出
  `RPT_MAIN_MONEY_FLOW_backfill.csv` 语义）。
- **`daima` 映射**：`SH600519→sh600519`、`SZ000001→sz000001`、`BJ830001→bj830001`、
  `BJ920000→bj920000`；部分北交所代码新浪无覆盖（实测 bj830001 空）→ 该股跳过。
- **`import_to_dolt`**：不变（merge + INSERT IGNORE + stock_basic 过滤，锚点
  `MAX(trade_date)`）。`SOURCE` 改为 `"Sina MoneyFlow ssl_qsfx_lscjfb"`。
- **CLI**（`main.rs`）：`main-flow` 移除 `--page-size`（调用 `main_flow::run()`）；
  `main-flow-backfill` 不变。orchestrate `sync()` 调用改 `main_flow::run()`。
- **单测**（RED→GREEN）：daima 映射、sina 行解析（字段映射/占比计算/除零）、
  num=20 窗口过滤（旧日期剔除）、非数组/空响应处理、backfill 区间过滤与
  倒序区间拒绝（沿用）、删除已废弃的 push2/fflow 相关测试。

## Issue #340 — update-database.sh 移除 SEPA 自动计算

### 设计

- `scripts/update-database.sh`：
  - 删除 step 5（`sepa backfill-dates`）、step 6（`sepa temperature` +
    `sepa score --top 50`）、step 7（compute Dolt commit）、step 8（TOP50 打印）。
  - 删除 `COMPUTE_TABLES` 变量与 `SCORE_LOG`/Step6-8 timing 块。
  - `COLLECTOR_TABLES` 追加 `data_updates`（step 2 sync 更新它、step 4 只读，
    step 3 随采集表一并提交）。
  - 头部步骤注释、尾部提示更新（不再提 SEPA 面板每日数据）。
  - `sepa` CLI（`crates/compass-data`）与 `sepa.rs` 代码**不改**。
- `scripts/tests/test-update-database.sh`：删除 step5-8 断言，新增
  `data_updates` 在 step3 提交/allowlist 断言，步骤顺序断言更新
  （last import-compass 后直接结束；失败场景不再等 sepa）。

## Issue #338 — 日频表空增量日历判定

### 设计

- `orchestrate.rs` 新增（纯函数 + 薄适配层，可单测）：
  ```rust
  /// calendar_days: trade_calendar(anchor+1, today) 的结果。
  /// 窗口内无交易日（或窗口倒置/日历不可用）→ 0 行视为成功 no-op；
  /// 窗口内有交易日却 0 行 → 失败。
  fn daily_zero_row_decision(calendar_days: Result<Vec<String>, CollectError>) -> Result<()>
  ```
  - `Ok(days)` 非空 → `require_nonzero(rows, table)`
  - `Ok(empty)` / `Err(EmptyCalendar)` / 窗口倒置 → `Ok(())`（no-op，eprintln 记录）
  - 其他 `Err`（如 MissingRepo/Dolt）→ 原样传播
- `sync()` 中 4 个日频表（capital_main_flow/index_daily/dragon_list/block_trade）
  的 `require_nonzero` 替换为 `require_daily_rows(table, rows)`：
  ```
  async fn require_daily_rows(table, rows) {
      if rows > 0 { return Ok(()); }
      let end = today; let start = next_day(last_report_date(table)?) 或 date_str(90);
      daily_zero_row_decision(crate::calendar::trade_calendar(&start, &end).await)
  }
  ```
- `backfill()` 路径：3 张表沿用 `path.exists()` 跳过（已是 no-op）；
  `capital_main_flow` 的 `require_nonzero` 保留 —— auto-heal 窗口由交易日历
  推导，0 行即数据源异常（应失败）。
- `index_daily::run()`（同步路径）当前在 daily+basic 均空时 `Err`；
  **不改**（真实全空=数据源异常需失败；周末 daily 空但 basic 通常非空 →
  由 require_daily_rows 处理）。如冒烟发现周末因 basic 也空而提前 Err，
  再按单一数据源调整（做成 issue 内的修正确认）。
- 单测：`daily_zero_row_decision` 四种分支。

## 测试计划（门禁 3.5/4）

1. **Adversarial（3.5）**：委派 `subagent_skwy_adversarial_test` 攻击
   偶发空响应/半截 JSON、除零、num 边界、多股互不覆盖日期、周末 0 行、
   日历库缺失、allowlist 幂等。
2. **Requirement（4）**：委派 `subagent_skwy_requirement_test` 按本 plan
   契约写 RED 测试（Rust 单测 + shell 断言）。
3. 实现后独立 QA 复核（requirement test 子代理）+ `just check`
   （fmt/clippy/test）+ shell 测试全量。

## 文档同步（门禁 5b）

- `.dsh/kb/design/data-providers.md`：决策记录新增 2 条（capital_main_flow
  新浪逐股切换与字段口径；日频表空增量日历判定 no-op）；数据源章节
  同步（push2/push2his → sina lscjfb）。
- `.dsh/kb/design/architecture.md`：管线步骤（update-database.sh 不再含
  sepa backfill/temperature/score；step3 含 data_updates）。
- `.dsh/kb/user/cli.md`：`main-flow` 无 `--page-size`；管线步骤描述更新
  （`sepa backfill-dates` 不再自动执行，CLI 保留）。
- `.dsh/kb/dev/database.md`：管线描述同一更新。
- `.dsh/kb/dev/toolchain.md`：如冒烟遇到新问题（sina 限流/编码）追加。

## 决策记录（门禁 5c）

- data-providers.md「## 决策记录」已有 → 追加上述 2 条。
- architecture.md 检查管线章节决策记录，必要时追加。

## 关键环境事实

- 新浪接口实测：lscjfb `page=1&num=1000` 返回 2022-07-14..2026-08-28；
  `sh600519`/`sz000001`/`bj920000` 有效；`bj830001` 空。
- `stock_basic` n=5905（SH 2461 / SZ 3105 / BJ 339）；日频 5905 股 × 100ms
  ≈ 10-15 分钟。
- `ts_trade_day_calendar` 覆盖到 2026-12-31（日历判定可行）。
- 数据核对口径：`netamount = r0_net+r1_net+r2_net+r3_net`（已实测）；
  主力净额 = `r0_net+r1_net`。
