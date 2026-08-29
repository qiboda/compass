# B5 dual-run evidence (epic #310)

- Date: 2026-08-29 (Asia/Shanghai)
- Branch: `feat/migrate-collectors-to-rust-b5`
- Dual-run scripts:
  - `crates/compass-collectors/scripts/dual_run_stock_basic_official.sh`
  - `crates/compass-collectors/scripts/dual_run_index_daily.sh`

## Results

| Collector | Scope | Rust | Python | Result |
|---|---|---|---|---|
| stock_basic_official | 三大交易所全量 | 5905 rows | 5905 rows | OK, all 12 canonical columns match (symbol/ts_code/code/name/list_date/delist_date/board/full_name/total_share/industry/region/update_date) |
| index_daily | official probe `1.000001` (EastMoney kline) | 8714 klines | 8714 klines | OK, kline rows exact match |
| index_daily | THS industry list (GBK page) | 90 industries | 90 industries | OK, code/name list exact match |

All live runs used isolated `COMPASS_CSV_DIR` + `COMPASS_DATA_DIR` + `COMPASS_PROXY_DISABLE=1`.

## Proxy pool tools

- `freeproxy` JSON normalization/safety/score/sort and Redis write path are covered by Rust unit tests (no live Redis server was used for this evidence).
- `check_proxy_pool` `judge`/boundary logic is covered by Rust unit tests; a live proxy_pool trial was not run because the local proxy_pool service is not part of this session's environment.
- `keepalive` JSON snapshot fallback + cycle wiring is covered by unit tests; `--source realtime` remains explicitly unsupported in Rust (documented in CLI), the only remaining Python-only dependency in B5.

## Unit tests

`cargo test -p compass-collectors --lib`: 53 tests passing at time of evidence.
`cargo clippy -p compass-collectors --all-targets`: clean.

## Remaining known gaps

- Full `index_daily` run (90 THS industries × ~20 years) is not part of this evidence; the two probes exercise the same fetch/parse paths used by the full run.
- B5 dual-run remains CSV/probe-level; Dolt import round-trip is still not exercised by the dual-run scripts.
- Independent RED/adversarial/requirement subagent tests remain pending (plan gate 3.5/4).
- Rust `freeproxy` realtime source and live Redis/proxy_pool integration are not yet verified end to end.
