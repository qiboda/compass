# Plan: index_daily 真增量同步

Issue: https://github.com/qiboda/compass/issues/292
Worktree: fix/index-daily-incremental

## 背景

`collectors/fetch_index_daily.py` 的 `run()` 只有全局 `last_report_date == 今天`
时短路；否则每个 THS 行业从今年回退到 2007 全量拉取，官方指数也始终 `beg=0`
全量拉取，再靠 `INSERT IGNORE` 去重。实测 90 行业约 1 小时、旧年份大量
404/502/504。

## 目标

真正的按 symbol 增量：只拉 `MAX(trade_date)` 之后的数据区间；新 symbol 全量回填；
空增量（周末/停牌）不算失败。

## 接口变更

1. `fetch_index_daily.py` 新增/修改：
   - `max_trade_date(dolt_table: str, symbol: str) -> str | None`
     —— 查询 Dolt `index_daily` 中该 symbol 的最大 `trade_date`（ISO 字符串；
     表/库不存在返回 None）。
   - `fetch_ths_kline(...)` 不变；`run()` 中行业循环按
     `max_trade_date(DOLT_TABLE, symbol)` 计算起始年份：
     - `None`（新板块）→ 当前年 → 2007 全量回填，并保留现有 None/空年语义；
     - 有日期 → 仅拉 `max(trade_date 年份) → 当前年`，过滤掉 `<= max` 的旧行；
       空增量（无新行但存在有效响应）按成功计，不触发 fast-fail。
   - `fetch_kline(..., last_date: str | None = None)`：
     - `last_date is None` → `beg=0`（保持全量）；
     - 否则 `beg=(last_date+1).strftime('%Y%m%d')`。
   - `_fetch_tencent_kline(..., last_date: str | None = None)`：
     - `None` → 现有全量翻页；
     - 有日期 → 从最新页开始，保留 `> last_date` 的行，一旦在当前页遇到
       `<= last_date` 的行即停止翻页；有效响应但无新行返回 `[]`（不是 None）。
   - `run()` 官方指数循环：
     - 先取该 symbol 的 `max_trade_date`；
     - 传 `last_date` 给 EastMoney；如 EastMoney 失败/空则走 Tencent 增量；
     - Tencent 有效响应无新行 → 成功 no-op（不 bump fast-fail）；
       Tencent 请求失败 → 仍按失败计。
2. 保持 `last_report_date == 今天` 的全局短路。

## 测试策略（RED → GREEN）

- 需求测试（subagent_skwy_requirement_test）覆盖：
  1. 已有 THS 板块二次运行只请求 `MAX 年份→今年`，不请求更早年份；
  2. 新 THS 板块仍全量回填（2007 起）；
  3. 已有 THS 板块无新行（周末/停牌）→ `run()` 不失败、不 bump fast-fail；
  4. 官方指数 EastMoney `beg` 变为上次日期+1（抓 params 断言）；
  5. 官方指数 Tencent 增量遇到 `<= last_date` 即停止翻页、保留新行；
  6. 全局 `last_report_date == 今天` 仍零请求。
- 对抗性测试（subagent_skwy_adversarial_test）补充边界：
  - 跨年 `MAX` 边界（12-31 → 次年 1 月）；
  - `MAX` 为今天、`MAX` 为未来日期（API 脏数据）→ 不请求全量；
  - 空增量时 fetch 全部失败 vs 有效空响应区分；
  - 新板块首年 404（None）后旧年成功仍全量回填；
  - Dolt 无 `index_daily` 表 / 无 `data_updates` 表时安全降级为全量。

## 文档同步（Step 5b）

- `.dsh/kb/design/data-providers.md`：更新第 185 行附近“K线 beg=0 全量拉取
  +INSERT IGNORE”的描述为“按 symbol MAX(trade_date) 增量，新 symbol 全量回填”。

## 实施步骤

1. 完成 RED 测试（需求 + 对抗）。
2. 实现 `max_trade_date`、`fetch_kline`/`_fetch_tencent_kline` 增量参数、
   `run()` 行业/官方循环改造。
3. 全量 Python 测试（pytest collectors/tests）通过。
4. 真实数据冒烟：对单/少量板块与官方指数跑增量 fetch，确认请求区间与无新行
   no-op。
5. 文档更新 + 决策记录检查。
6. commit 引用 `ref #292`，PR 审查、rebase、反思、push、关闭 issue。
