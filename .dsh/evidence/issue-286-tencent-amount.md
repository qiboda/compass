# Issue #286 — Tencent newfqkline 官方指数成交额恢复（证据）

日期：2026-08-16
分支：`fix/286-tencent-amount`
Commit：`155b98d` + review fixes（后续 commit）

## 变更

- `collectors/fetch_index_daily.py`：腾讯回退从 `fqkline/get` 切换为 `newfqkline/get`，
  解析 day 行 index 8 成交额（万元 → 元 ×10000）；缺失/畸形/非有限/负值/溢出降级为 0。
- 测试：`test_tencent_fallback_requirement.py` / `test_tencent_fallback_adversarial.py`
  由 RED 转 GREEN。
- 文档：`.dsh/kb/design/data-providers.md` 更新腾讯回退描述并新增决策记录。

## 真实数据冒烟

### Dolt（compass_data）

```sql
SELECT index_type, COUNT(*) total,
       SUM(CASE WHEN amount=0 THEN 1 ELSE 0 END) zero,
       SUM(CASE WHEN amount IS NULL THEN 1 ELSE 0 END) nullcnt
FROM index_daily GROUP BY index_type;
```

| index_type | total | zero | nullcnt |
|---|---|---|---|
| industry | 367780 | 0 | 0 |
| official | 160254 | 0 | 0 |

最新官方指数（`trade_date = MAX(trade_date)`，2026-08-14）：

| symbol | close | volume | amount |
|---|---|---|---|
| SH000001 | 3927.18 | 499,525,600 | 990,371,905,536 |
| SZ399001 | 14354.31 | 642,557,312 | 1,152,471,269,376 |
| SZ399006 | 3626.30 | 193,507,808 | 553,388,474,368 |
| SH000300 | 4665.88 | 178,430,688 | 549,769,576,448 |
| SH000905 | 7990.33 | 183,327,920 | 408,994,316,288 |
| SH000852 | 7769.82 | 250,317,824 | 482,267,627,520 |

### Parquet（index_daily.parquet）

DuckDB 查询结果与 Dolt 一致：

- official：160,254 行，0 零值，0 NULL。
- 最新 SH000001 amount ≈ 9.90371905536e+11 元。

### CSV（index_daily.csv）

- industry：367,780 行全部非零。
- official：160,254 行全部非零。
- 零值/缺失：0。

## 验证命令

```sh
cd /data/compass-data/compass_data && dolt sql -q "SELECT ..."
cd /data/codes/compass && cargo run --bin compass-data -- import-compass --table index_daily
python3 - <<'PY'  # DuckDB read_parquet 检查
PY
cd /data/codes/compass/collectors && .venv/bin/python -m pytest tests/ --no-cov -q  # 614 passed
```
