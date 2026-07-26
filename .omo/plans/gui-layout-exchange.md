# Plan: GUI Layout Rework + SH/SZ/BJ Exchange Selection — ref #43

## Locked-in Decisions

1. Layout: `TopBottomPanel::top` toolbar + `DockArea` below with Chart (75%) + Logger (25%) vertical split
2. Toolbar: `[Symbol dropdown] [Exchange dropdown] [TF:1d] [Fetch]`
3. Exchange: `全部`/SH/SZ/BJ. Selected exchange auto-prefixes symbol (`sh./sz./bj.`)
4. Symbol dropdown: loaded from `stock_basic.parquet` at startup, searchable by code+name, format `"SZ | 000001 | 平安银行"`
5. Exchange dropdown filters symbol list
6. Default: Exchange=全部, Symbol=000001
7. Remove ControlCitizen entirely
8. Toolbar state lives in `CompassApp` (local), stock list loaded once, not reactive

---

## Wave Dependency Graph

```
Wave 1 (Foundation) — 3 independent tasks
  1.1 Add Exchange enum + load_all_stock_basics() to compass-core
  1.2 Add parquet.dir to AppConfig
  1.3 Implement searchable symbol dropdown widget
       │
Wave 2 (State + Wiring + Layout) — depends on Wave 1
  2.1 Update SharedState (drop timeframe), messages, dispatcher for 2-citizen
  2.2 Update tabs.rs for 2 tabs (Chart, Logger)
  2.3 Refactor main.rs: toolbar UI, stock loading, vertical dock layout
       │
Wave 3 (Cleanup) — depends on Wave 2
  3.1 Remove ControlCitizen (delete file)
  3.2 Add/update all tests
  3.3 Update kb/ docs (gui.md, architecture.md, config.md)
```

---

## Wave 1 — Foundation

**Task 1.1** — `compass-core`: Add `Exchange` enum (`SH`, `SZ`, `BJ`) with `as_str()` to `model.rs`. Add `ParquetReader::load_all_stock_basics() -> Result<Vec<StockBasic>>` method in `parquet.rs` — single `read_parquet()` query returning all rows from `stock_basic.parquet`.

**Task 1.2** — `compass-core`: Add `ParquetConfig { dir: String }` + `[parquet]` section to `AppConfig`, default `"parquet_data"`.

**Task 1.3** — `compass GUI`: Create `widgets/searchable_dropdown.rs` with:
- `filter_stocks(stocks, query, exchange) -> Vec<&StockBasic>` — pure function, easily testable
- `show_stock_picker(ui, stock_list, filter_text, selected, exchange_filter)` — egui widget using popup/scroll area with clickable rows

---

## Wave 2 — State, Wiring, Layout

**Task 2.1** — Update `state.rs` (remove `timeframe` field, keep `symbol` for chart display), `messages.rs` (no changes needed — `symbol` already carries full code), `dispatcher.rs` (remove `RegisteredCitizens.control`, timeframe hardcoded to `"1d"` in `handle()`).

**Task 2.2** — Refactor `tabs.rs`: remove `TabKind::Control`, `CONTROL_ID`, remove `control` field from `TabViewer`.

**Task 2.3** — Major refactor of `main.rs`:
- Load `stock_list` from `ParquetReader::load_all_stock_basics()` at startup
- Add `CompassApp` fields: `stock_list`, `symbol_filter`, `selected_symbol`, `selected_exchange`, `timeframe`
- Create vertical `DockState` with `split_below(root, 0.25, [logger_tab])`
- Render `TopBottomPanel::top` toolbar with exchange ComboBox + symbol picker + Fetch button
- Remove `control.outbox` drain from frame loop
- Helper functions: `exchange_index_to_filter()`, `build_qualified_symbol()`

---

## Wave 3 — Cleanup

**Task 3.1** — Delete `citizens/control.rs`, remove from `mod.rs` and all imports.

**Task 3.2** — Tests:
- `compass-core`: Exchange `as_str()` test, `ParquetConfig` default test, `load_all_stock_basics()` test
- `compass`: `filter_stocks()` 5 tests (code match, name match, exchange filter, empty query, case), updated `SharedState` test, updated `citizens_register` test, exchange helper tests
- Remove: `control_citizen_outbox_on_fetch_click`

**Task 3.3** — Update `kb/user/gui.md` (toolbar interface), `kb/design/architecture.md` (2-citizen, layout diagram, frame loop), `kb/user/config.md` (parquet section), `kb/dev/reflections.md` (post-merge).

---

## Commit Strategy (9 atomic commits)

| # | Commit |
|---|--------|
| 1 | `feat(core): add Exchange enum and load_all_stock_basics` |
| 2 | `feat(core): add parquet.dir to AppConfig` |
| 3 | `feat(gui): add searchable stock dropdown widget` |
| 4 | `refactor(gui): update state, messages, dispatcher for 2-citizen` |
| 5 | `refactor(gui): update tabs for 2-citizen dock` |
| 6 | `refactor(gui): toolbar + stock loading + vertical dock in main` |
| 7 | `chore(gui): remove ControlCitizen` |
| 8 | `test: Exchange, parquet, filtering, toolbar` |
| 9 | `docs: update kb/ for new GUI layout` |

All ref `#43`. Push as single PR, close issue after push.

---

## Verification Gate

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test -p compass-core && cargo test -p compass
cargo build --release
cargo run  # manual smoke
```
