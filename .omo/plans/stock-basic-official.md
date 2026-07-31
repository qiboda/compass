# Stock Basic Official Data Sources

**Issue**: #78 — Replace EastMoney-polluted stock_basic with official exchange data
**Worktree**: `fix-stock-basic-scope`
**Date**: 2026-07-31
**Status**: pending approval

## Context

The `stock_basic` table currently has 12,388 rows polluted with NEEQ, OTC delisted, preferred stock, and 定转 shares from the EastMoney push2 API. The fix switches to three official exchange websites (SSE JSON, SZSE XLSX, BSE JSON) producing ~5,888 clean A-share rows.

Grill-me decisions locked: scope = SH+SZ+BJ including delisted; new collector file `fetch_stock_basic_official.py`; Dolt schema adds delist_date/board/full_name/total_share and drops EastMoney-specific columns; main.py switches to full rebuild (DELETE + INSERT).

## Task Dependency Graph

| Task | Depends On | Reason |
|------|------------|--------|
| T1: Test file `test_stock_basic_official.py` | None | Tests must exist before code (TDD) |
| T2: Dolt schema ALTER TABLE | None | Independent of collector code |
| T3: `fetch_stock_basic_official.py` | T1, T2 | Tests define contract; schema must exist |
| T4: model.rs StockBasic struct | None | Independent Rust struct change |
| T5: import_compass.rs SQL | T2 | SELECT depends on new Dolt columns |
| T6: parquet.rs read queries | T4 | SELECT depends on model.rs fields |
| T7: main.py `_import_stock_basic()` | T3 | Importer must match collector output |
| T8: Integration QA | T1-T7 | End-to-end verification |
| T9: kb/ docs | T3, T4 | Reflect final state |

## Parallel Execution Graph

**Wave 1** (parallel): T1 (tests RED) + T2 (Dolt schema)
**Wave 2** (parallel): T3 (collector) + T4 (model.rs) + T5 (import_compass.rs SQL)
**Wave 3** (parallel): T6 (parquet.rs) + T7 (main.py import)
**Wave 4**: T8 (Integration QA)
**Wave 5**: T9 (kb/ docs)

**Critical Path**: T1 → T3 → T7 → T8 → T9

## Tasks

### T1: Write `collectors/tests/test_stock_basic_official.py` (TDD RED)
Failing tests covering: parse_sse_json, parse_szse_xlsx_active (inline strings, 22 cols), parse_szse_xlsx_delisted (4 cols), parse_bse_json (null() wrapper + pagination), build_record (17-col output, exchange inference), merge_exchanges (dedup by code), write_csv (column order, UTF-8-sig), row_count_target ≈ 5,888. Mock data, no network.
QA: `pytest collectors/tests/test_stock_basic_official.py -v` → ALL FAIL (RED)

### T2: Dolt schema ALTER TABLE (compass_data)
```sql
ALTER TABLE stock_basic ADD COLUMN delist_date DATE AFTER list_date;
ALTER TABLE stock_basic ADD COLUMN board VARCHAR(50) AFTER delist_date;
ALTER TABLE stock_basic ADD COLUMN full_name VARCHAR(200) AFTER board;
ALTER TABLE stock_basic ADD COLUMN total_share DOUBLE AFTER full_name;
ALTER TABLE stock_basic DROP COLUMN lead_stock;
ALTER TABLE stock_basic DROP COLUMN data_ts;
ALTER TABLE stock_basic DROP COLUMN industry_alt;
ALTER TABLE stock_basic DROP COLUMN member_count;
ALTER TABLE stock_basic DROP COLUMN market;
```
QA: `dolt sql -q "SHOW CREATE TABLE stock_basic"` shows 14 columns

### T3: Write `collectors/fetch_stock_basic_official.py`
~400-line Python collector: fetch_sse (JSON, filter STOCK_TYPE=1|8), fetch_szse (XLSX zip → sheet1.xml regex parse; active 1110 + delisted 1793_ssgs), fetch_bse (session cookie + form POST, 17 pages, null() JSON wrapper), merge (dedup by code, exchange priority), main (argparse, write stock_basic_official.csv, 17 cols). Uses requests + zipfile + re.
QA: `pytest collectors/tests/test_stock_basic_official.py -v` → ALL GREEN

### T4: Update `StockBasic` in `crates/compass-core/src/model.rs`
Add `board: Option<String>`, `full_name: Option<String>`, `total_share: Option<f64>`. Update test constructors (~6 literals).
QA: `cargo test -p compass-core` passes

### T5: Update `import_compass.rs` SQL (lines 59-76)
Expand SELECT: symbol, name, exchange, list_date, delist_date, board, full_name, total_share, industry, region. Update test DDL (line 261).
QA: `cargo test -p compass-data -- stock_basic_exports_parquet` passes

### T6: Update `parquet.rs` read queries
`load_all_stock_basics()` + `get_stock_basic_blocking()`: SELECT board/full_name/total_share/industry/area, map to StockBasic. Update test helper DDL.
QA: `cargo test -p compass-core -- parquet` → all pass

### T7: Rewrite `_import_stock_basic()` in `collectors/main.py`
Read `stock_basic_official.csv`; DELETE + INSERT with 13-col mapping; update data_updates source = "SSE/SZSE/BSE official"; remove EastMoney f-field mapping. Update sync path.
QA: `python -c "import main"` + manual import run

### T8: Integration QA
`pytest collectors/tests/ -v`, `cargo test -p compass-core`, `cargo test -p compass-data`, `cargo clippy -- -D warnings`, `cargo fmt --check`

### T9: Update kb/ docs
`kb/design/data-providers.md`, `kb/design/symbols.md`, `kb/user/cli.md`, `kb/dev/reflections.md`

## Commit Strategy

| # | Commit | Tasks |
|---|--------|-------|
| 1 | `test: add stock_basic_official collector tests (RED) ref #78` | T1 |
| 2 | `feat: alter stock_basic Dolt schema for official sources ref #78` | T2 |
| 3 | `feat: add fetch_stock_basic_official collector ref #78` | T3 |
| 4 | `feat: add board/full_name/total_share to StockBasic model ref #78` | T4 |
| 5 | `feat: update import_compass for new stock_basic columns ref #78` | T5 + T6 |
| 6 | `feat: switch main.py import to official stock_basic CSV ref #78` | T7 |
| 7 | `docs: update kb/ for official stock_basic sources ref #78` | T9 |

## TODO (Wave order)

Wave 1: T1 (tests RED), T2 (Dolt schema)
Wave 2: T3 (collector), T4 (model.rs), T5 (import_compass.rs)
Wave 3: T6 (parquet.rs), T7 (main.py)
Wave 4: T8 (Integration QA)
Wave 5: T9 (kb/ docs)
