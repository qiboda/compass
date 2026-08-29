# B3 dual-run evidence (epic #310)

- Date: 2026-08-28
- Branch: `feat/migrate-collectors-to-rust-b3`
- Commits: `1633c4a` (implementation), `88d8cc3` (review fixes)

## Dual-run results

| Collector | Date slice | Rust rows | Python rows | Result |
|---|---|---|---|---|
| dragon_list | 2026-08-27 | 121 | 121 | OK, keys match |
| institution_survey | 2026-08-28 | 4007 | 4007 | OK, keys match |
| main_flow (snapshot) | latest full-market | 5554 | 5554 | OK, keys match |
| stock_basic (EastMoney) | one page (100 rows) | 100 | 100 | OK, keys match |

All dual-run scripts use isolated `COMPASS_CSV_DIR` and `COMPASS_DATA_DIR` (mktemp), so
they do not read or write the production Dolt/csv state.

## Unit tests

`cargo test -p compass-collectors`: 30 tests passing at time of evidence.
New B3 tests cover seat classification/merge, survey date helpers, push2 field
mapping, fflow backfill row parsing, exact Beijing trade-date derivation for a
known epoch, inverted backfill range rejection, and stock_basic symbol/field order.

## Review status

Five-angle subagent review completed on `1633c4a`; findings addressed in `88d8cc3`
(dual-run Dolt isolation, shell arg injection, backfill CLI, expect/dead-code,
test depth). Remaining known gaps:
- Dolt-level dual-run comparison is not yet part of the B3 scripts (CSV-level only).
- Independent RED/adversarial/requirement subagent tests for B3 are still pending
  (plan gate 3.5/4 remains open).
