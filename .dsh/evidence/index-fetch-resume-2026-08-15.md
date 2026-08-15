# 指数数据采集续采记录（2026-08-15）

## 背景

首次真实采集（epic #255/#260 实现后从未跑过真实采集）触发东财 push2his 反爬，
IP 被封禁（HTTP 000，全部镜像 91/79/100-130 均封锁），采集 3.5 小时后中断。
**官方指数段全部失败**（30 个官方指数连 index_basic 条目都没有），
行业板块 496 个、概念板块 459 个行情缺失。

## 当前已入库范围（Dolt compass_data，commit sphqaah893h9336kj4o6lisa6hu307j4）

| 表 | 覆盖 | 说明 |
|---|---|---|
| index_basic | 1000 行（概念 504 + 行业 496） | 无官方指数条目（官方段全部 FAILED，未写入 basic） |
| index_daily | 2759 行，45 个概念板块（BK1656-BK1753 区间） | 每标的 13-105 天，日期范围 2026-03-16 ~ 2026-08-14 |
| parquet | index_daily.parquet / index_basic.parquet 已导出 | GUI 大盘 tab 可读 45 概念板块 |

## 缺失清单（下次续采目标）

- **官方指数 30 个**：`fetch_index_daily.py::OFFICIAL_INDICES` 白名单（SH000001 上证指数等 30 个）——index_basic 与 index_daily 均需补
- **行业板块 496 个**：清单见 `index_missing_industry_2026-08-15.txt`
- **概念板块 459 个**：清单见 `index_missing_concept_2026-08-15.txt`

合计 985 个标的待补。

## 续采方式

封禁解除后重跑全量采集即可——采集器是幂等增量设计：

```bash
cd /data/codes/compass/collectors
uv run python main.py fetch index_daily    # 重新拉全部 1000 板块 + 30 官方
uv run python main.py import index_daily  # INSERT IGNORE merge，按 PK(symbol, trade_date) 去重
```

- 已入库的 45 板块数据会被保留（同 PK 行 IGNORE，新行追加），无需清表
- 官方指数首次写入时会自动补 index_basic 条目（run() 在非增量全量运行时重建 basic）
- **限流已全局调大**：本次 Throttle 0.5-0.8s/请求 × 1000+ 标的触发封禁。issue #277 已将
  `common.py::EM_MIN_INTERVAL` 及 `fetch_fin_indicators.py` / `fetch_stock_basic.py` 的
  局部限流常量全部调至 **2.0s**；续采无需临时改，仍建议分批运行（官方 30 个先拉，板块分 2-3 批）
- **快速失败保护（issue #277）**：`fetch_index_daily.py::run()` 现维护连续失败计数器，连续 5 个标的失败
  （请求失败或 empty）即终止并保留已抓 CSV，避免封禁后再次空转数小时
- **封禁恢复检测**：`curl -s -o /dev/null -w "%{http_code}" "https://push2his.eastmoney.com/api/qt/stock/kline/get?secid=1.000001&klt=101&fqt=0&beg=0&end=20500000&lmt=10&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61"` → 200 即恢复

## 完成定义

续采后验证（Dolt）：

```sql
SELECT COUNT(*), COUNT(DISTINCT symbol) FROM index_daily;          -- 期望 ≥ 30 万行、1030 标的
SELECT index_type, COUNT(*) FROM index_basic GROUP BY index_type;  -- 期望 concept 504 / industry 496 / official 30
```

然后 `import-compass --table index_daily` + `--table index_basic` 重新导出 parquet，GUI 大盘 tab 全量可用。
