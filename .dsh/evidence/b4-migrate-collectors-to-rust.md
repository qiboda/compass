# B4 dual-run evidence (epic #310)

- Date: 2026-08-29 (Asia/Shanghai)
- Branch: `feat/migrate-collectors-to-rust-b4`
- Dual-run script: `crates/compass-collectors/scripts/dual_run_financial.sh`

## Results (2026 Q1)

| Collector | Report | Rust rows | Python rows | Result |
|---|---|---|---|---|
| fin_indicators | RPT_LICO_FN_CPD | 5908 | 5908 | OK, keys and values match |
| balance_sheet | RPT_F10_FINANCE_GBALANCE | 7041 | 7041 | OK, keys and values match |
| income | RPT_F10_FINANCE_GINCOME | 7175 | 7175 | OK, keys and values match |
| cash_flow | RPT_F10_FINANCE_GCASHFLOW | 7039 | 7039 | OK, keys and values match |

All runs used isolated `COMPASS_CSV_DIR` + `COMPASS_DATA_DIR` + `COMPASS_PROXY_DISABLE=1`.

## Unit tests

`cargo test -p compass-collectors`: 33 tests passing at time of evidence.
New `financial` tests cover upsert alias generation, TRIM text-column handling and
report-date period building.

## Remaining known gaps

- B4 dual-run is CSV-level only (same as B3); the final script compares full canonical values, but Dolt import/upsert round-trip is not yet part of the scripts.
- Independent RED/adversarial/requirement subagent tests remain pending (plan gate 3.5/4).
