# Plan: Make Data Directories Configurable & Move to /data/compass-data/

## Metadata

- **slug**: data-dirs-configurable
- **issue**: ref #52
- **intent**: CLEAR
- **review_required**: false
- **status**: awaiting-approval

## Decisions

| # | Decision |
|---|----------|
| D1 | All data dirs → `/data/compass-data/{parquet_data,investment_data,compass_data}` |
| D2 | New `[dolt]` config section: `investment_data_dir`, `compass_data_dir` |
| D3 | CLI reads `config.toml` for defaults; `--dolt-dir`/`--output` flags override |
| D4 | Remove `DatabaseConfig` (unused), `merge` command, `merge.rs`, `data/` dir |
| D5 | Export duckdb → `/data/compass-data/compass.duckdb` |
| D6 | Fix `kb/user/config.md`: `[database] parquet_dir` → `[parquet] dir` |

## Scope In

- Add `DoltConfig` struct, wire into `AppConfig`, serde defaults
- Update CLI `compass-data` to load `config.toml` for default paths
- Update all path defaults to `/data/compass-data/...`
- Delete `DatabaseConfig`, `merge` command def, `merge.rs`, `data/` directory
- Update `kb/user/config.md`, `kb/design/architecture.md`, `kb/dev/process.md`, `AGENTS.md`
- Clone `investment_data` from chenditc/investment_data → push skwy/investment_data
- Symlink or point to `/data/compass-data/` for all three directories

## Scope Out (Must-NOT-Have)

- Do NOT move `logs/` directory
- Do NOT touch `compass-core/src/data/` provider implementations
- Do NOT change `export.rs` logic — only default value

---

## Approval Gate

Ready for approval. The plan below covers all 4 modules + docs + data operations.
After approval, will proceed to implementation.
