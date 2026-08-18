# Evidence — index_daily 真增量冒烟（issue #292）

日期：2026-08-17（本地）
分支：fix/index-daily-incremental
环境：真实 compass_data Dolt + 真实 Tencent API（EastMoney 本次连接被重置，走 Tencent 回退）

## 1. per-symbol MAX(trade_date) 查询

```text
BK881101 -> 2026-08-14
BK0475   -> None          # 新板块，应全量回填
SH000001 -> 2026-08-14
SZ399001 -> 2026-08-14
```

`max_trade_date()` 对已有 symbol 返回 ISO 日期，对无数据 symbol 返回 None（新 symbol 全量回填语义）。

## 2. Tencent 增量翻页（真实 API）

```text
last_date=2026-08-09 -> 6 rows: 2026-08-10 .. 2026-08-17
last_date=2026-08-16 -> 1 row:  2026-08-17
```

验证：增量模式只返回 `> last_date` 的行，遇到边界即停止翻页；有效空增量返回 `[]`（last_date=2026-08-17 时无新行）。

## 3. 东财增量路径

本次实际请求 EastMoney 时连接被重置（curl 56），`fetch_kline(..., last_date=...)` 正确返回 None 并进入 Tencent 回退；未观察到 `beg=0` 全量请求。单元测试已锁定 `beg=last_date+1` 的请求参数。

## 4. 自动化测试

```text
135 passed (incremental requirement/adversarial + index_daily + fast_fail + THS + Tencent fallback)
```

覆盖：已有板块增量区间、新板块全量回填、空增量 no-op、官方指数 beg=MAX+1、Tencent 升序/降序分页、畸形 payload 返回 None、THS 跨年空年/部分失败丢弃。
