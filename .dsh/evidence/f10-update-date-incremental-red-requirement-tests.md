# RED Evidence — issue #299 F10 UPDATE_DATE incremental requirement tests

- Date: 2026-08-20
- Test file: `collectors/tests/test_f10_incremental_requirement.py`
- Run: `cd collectors && uv run pytest tests/test_f10_incremental_requirement.py -q`
- Result: **25 failed, 6 passed** in ~14.5s

## RED failure summary (25 failures)

### A. run(incremental=True) — UPDATE_DATE 单次增量 + state.json（12 失败 = 4 tests × 3 tables）
- `test_incremental_uses_update_date_single_fetch_and_writes_csv[balance_sheet|income|cash_flow]`
  - `TypeError: run() got an unexpected keyword argument 'incremental'` (line 94)
- `test_no_anchor_falls_back_to_2020_01_01[...]`
  - `TypeError: run() got an unexpected keyword argument 'incremental'` (line 141)
- `test_writes_state_json_with_required_keys[...]`
  - `TypeError: run() got an unexpected keyword argument 'incremental'` (line 184)
- `test_no_report_date_enumeration_in_incremental[...]`
  - `TypeError: run() got an unexpected keyword argument 'incremental'` (line 223)

### B. import_to_dolt() merge 语义（6 失败 = 2 tests × 3 tables）
- `test_incremental_csv_keeps_historical_rows[...]`
  - `AssertionError: assert '2' == '3'` — replace 语义清空历史行（增量 CSV 只含修订/新行），merge 应保留 3 行
- `test_import_replace_table_called_with_merge_true[...]`
  - `AssertionError: F10 import must use merge (ODKU) semantics` (captured merge is False)

### C. main.py（7 失败）
- `test_dispatch_fetch_passes_incremental_to_run[balance_sheet|income|cash_flow]`
  - `TypeError: dispatch_fetch() got an unexpected keyword argument 'incremental'`
- `test_fetch_cli_supports_incremental_flag[balance_sheet|income|cash_flow]`
  - `SystemExit: 2` — `main.py: error: unrecognized arguments: --incremental`
- `test_do_sync_calls_three_f10_tables_with_incremental_true`
  - `AssertionError: do_sync must call the three F10 run() with incremental=True` (assert 0 >= 3)

## Passed (6) — existing-behavior contract guards, must stay green after implementation
- `TestImportMergeSemantics::test_first_run_creates_table_and_imports[...]` (3)
- `TestImportMergeSemantics::test_data_updates_updated_with_row_count_and_last_report_date[...]` (3)

## Scenarios covered (per plan contract)
1. run(incremental=True): UPDATE_DATE 单次增量（fetch_by_update_date），不枚举 REPORT_DATE
2. 无 anchor → 固定 "2020-01-01" 走 UPDATE_DATE 路径
3. total_records>0 写 state.json（last_report_date / last_update_date / total_rows / last_run）
4. import merge: 首建表成功 / 历史行保留 / 同 PK 修订覆盖 / data_updates 更新
5. import_replace_table 以 merge=True + ODKU (insert_sql 含 ON DUPLICATE KEY UPDATE) 调用
6. main: dispatch_fetch 透传 incremental、fetch CLI --incremental、do_sync 三表 run(incremental=True)

## Scenario manufacturability
All scenarios manufactured from public interfaces via monkeypatch:
- `fetch_balance_sheet/income/cash_flow.fetch_by_update_date` (raising=False — name lands after impl)
- `module.update_date_anchor` (raising=False)
- `module.AsyncSession` via `make_stub_session` fixture
- `module.import_replace_table` (capture merge kwarg)
- Dolt tempdir (`dolt --data-dir <tmp> init`, COMPASS_DATA_DIR, dolt_sql_csv helper)
- main.py module run/import_to_dolt/entrypoint monkeypatch + dolt_sql no-op
- `monkeypatch.setattr(..., raising=False)` used so attributes absent in RED become
  valid AttributeError/TypeError RED now and land cleanly at GREEN without rewrites.
