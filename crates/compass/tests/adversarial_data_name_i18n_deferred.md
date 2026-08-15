# compass adversarial tests for epic #266 sub-issue #270 (B3 GUI rendering) — DEFERRED

Status: **DEFERRED** — adversarial tests for the B3 GUI name-i18n behavior
cannot be landed as compilable RED tests under the current state. This file
is the complete hand-off: interface list + the full adversarial test matrix,
every case RED against the current implementation, drop-in ready once the
B3 helper/behavior lands and the current baseline compiles.

## Why DEFERRED (three independent blockers)

1. **Helper interface not landed (primary)**: the plan's B3c contract
   (`locale=="en" && name_en.is_some() → name_en`, else `name`) has **no
   symbol** in the tree yet — no `display_name` helper, no `i18n_name`
   module anywhere in `crates/compass/src/`. A RED test that calls the
   helper would be a **compile error, not an assertion failure**, which
   violates the RED contract (RED = assert fails on a compiling baseline).
2. **B3 behavior unimplemented**: `market.rs` core card (L177
   `name.as_str()`) and table `row_cells` (L376 `row.name`), `sepa.rs`
   `row_cells` (L456-461 industry+themes concatenation), and `screener.rs`
   industry dropdown/table column all still resolve to the Chinese name —
   so any behavioral (GUI/kittest) RED cannot even reach the assertion.
3. **Current test baseline does not compile**: `market.rs` test helper
   `sample_row` (L430 `IndexRow { ... }`) was NOT updated when B2 added the
   `name_en` field, so `cargo check --tests -p compass` fails:

```
error[E0063]: missing field `name_en` in initializer of `compass_types::IndexRow`
   --> crates/compass/src/citizens/market.rs:430:9
    |
430 |         IndexRow {
```

   (This is a B2 gap in the `market.rs` test module specifically — the `sepa.rs`
   test `sample_row` at L798 correctly carries `industry_en: None`.)

## Interface list needed to un-DEFER #270

Below is the exact contract the implementer must land before re-delegation.
Signatures are *proposals* — the plan declares the behavior, the tree defines
the symbol. Anything shown as `compass_crate::...` is a crate-private `fn` in
`crates/compass/src/` (the crate is **pure-bin**: `Cargo.toml` has only
`[[bin]] main.rs`, no `[lib]` — integration tests cannot `use compass::...`).

### 1. Display-name helper (B3c) — behavior contract
`fn display_name(locale: &str, zh: &str, en: Option<&str>) -> String`
(or `String`/`Cow`), crate-private in e.g. `src/i18n_name.rs`.
- `locale == "en"` and `en == Some(非空)` → `en`
- `locale == "en"` and `en == None` → `zh`
- `locale == "en"` and `en == Some("")` → **must fall back to `zh`** (empty
  `name_en` is an unmapped/legacy row artifact, not a displayable blank)
- `locale == "zh"` (any other non-en) → `zh`, regardless of `en`

### 2. `CORE_INDEX_WHITELIST` — become `(symbol, zh, en)` triples (B3d)
Current: `[(&str, &str); 6]` (L30-37). Change to `[(symbol, zh, en); 6]`.
Note this **breaks the existing test** `whitelist_embeds_six_core_indexes`
(L583, iterates `for (symbol, _)`) — that baseline test must be updated by
the implementer (in-source, their privilege). The adversarial M5 matrix below
joins from the new triple shape.

### 3. Market table `row_cells` locale resolution (B3d)
`row_cells(row: &IndexRow)` currently emits `DataCell::Text(row.name)` (L376).
Must resolve via `display_name(locale(), &row.name, row.name_en.as_deref())`.
Needs the current locale — `compass_i18n::locale()` (a rust_i18n macro export,
`&'static str`, available) is the source of truth; do **not** depend on
`CompassApp::language` (panel-level, not passed into `row_cells`).

### 4. Core card name resolution (B3d) — triple fallback precedence
For each whitelist entry `(symbol, zh, en)` with `row = snapshot.rows.find(symbol)`:
- `row` present, `row.name_en = Some(...)` → `en` (via display helper)
- `row` present, `row.name_en = None` → **`row.name`** (snapshot wins over zh fallback)
- `row` absent → `en` (when en locale) else `zh` from the triple
This is a distinct fallback chain from the table's (card has a hardcoded zh/en
fallback; table row falls back to `row.name`). Attack M5 exercises every arm.

### 5. `sepa.rs row_cells` locale resolution (B3e)
`row_cells(row: &SepaRow)` currently builds `industry = row.industry` then
appends `· theme` for `themes.iter().take(2)` (L456-461). Must resolve:
- industry via `display_name(locale(), &row.industry, row.industry_en.as_deref())`
- each theme via the **concept zh→en map** (D1-A); unmapped theme → Chinese

### 6. Concept zh→en map source (B3e, D1-A) — **interface gap flagged**
D1-A says "GUI 层用 index_basic.name_en 的 concept 行构建映射, 渲染 themes
按名查询". But `SepaPanel::show(ui, shared, sepa_signal, work_signal)` has **no
index_basic / concept-map input**, and `SepaData.rows[].themes` carries only
Chinese concept names. The renderer has nothing to build the map from. The
implementer must decide the injection point (e.g. an extra `&HashMap<String,
String>` param on `show`/`row_cells`, or a shared-state field), and it must be
reflected in the test seam. **This is the single most important interface
decision for the adversarial S2/S3 matrix.**

### 7. `screener.rs` industry dropdown + table column (B3f) — **interface gap flagged**
- Industry dropdown: `industries: &[String]` (zh keys, main.rs L133-138 dedup).
  en-locale display needs a zh→`industry_en` map; the **selected value round-trip
  must keep the zh key** (the `Filter::Meta(Industry(...))` engine matches zh) —
  showing en labels without corrupting the stored zh key is the core risk (SC2).
- Table industry column: `row_cells` L264 emits `DataCell::Text(row.industry)`
  from `ScreenerRow` which has **no `industry_en` field** (compass-types L209-222).
  The plan lists B3f "表格行业列" but the data source for its English text is
  undeclared (unlike IndexRow/SepaRow, ScreenerRow got no `industry_en` in B2).
  This must be resolved (either add the field + wire it, or reuse a map) before
  the SC3 adversarial tests can be written against a real target.

## Adversarial test matrix (RED against current implementation)

### market — `src/citizens/market.rs` (`mod tests`)

| # | Attack | Asserted contract (GREEN) | Keystrokes of the trap |
|---|--------|---------------------------|------------------------|
| M1 | **locale round-trip consistency** — zh→en→zh→en repeatedly on one snapshot; the rendered `name` cell flips with locale and the final state equals the last locale | every flip changes the cell; end locale is the last set | catches half-done toggles (helper reading a CACHED locale, or `row_cells` ignoring locale) |
| M2 | **`name_en = Some("")` must not render a blank** (en locale) | falls back to `zh` | an unchecked `unwrap_or`/`unwrap_or_default` on empty string yields an empty label — the classic blank-cell defect. Baseline renders `name` (zh), so true; RED now, GREEN only if helper guards `""` |
| M3 | **`name_en = Some(...)` while `locale = zh` renders `zh`** (bidirectional correctness) | zh shown; helper must not leak en into zh UI | guards an implementation that hardcodes "has en → show en" regardless of locale |
| M4 | **`name_en = None` while `locale = en` falls back to `zh`** (not blank, not panic) | zh shown | the unmapped-row + en-locale corner; baseline shows zh (true); RED only when implementer panics/blank |
| M5 | **core-card triple fallback, all arms** — (a) row present + en: shows en; (b) row present + no en: shows **row.name**; (c) row absent + en locale: shows triple-en fallback; (d) row absent + zh: shows triple-zh | each arm distinct, precedence row.name > triple-fallback | three fixture snapshots (row-with-en / row-no-en / row-absent) + two locales = 6 assertions; catches fallback precedence bugs (e.g. triple-zh overriding a present row's name) |
| M6 | **ranking table name column falls back to `row.name`** when `name_en=None` (en locale) — must NOT substitute the hardcoded core card zh fallback | `row.name` | table rows are arbitrary symbols not in the whitelist; a naive "look up core whitelist" breaks after index 6 |
| P1 | **large snapshot no-churn** — 10k `IndexRow`s, `filter_rows` + `row_cells` full pass; assert completes and cell count == 10k (O(n), no per-row allocation blow-up) | 10k rows map to exactly 10k cell vectors; `format_price` stable | guards a naive per-cell locale re-lookup that re-scans `CORE_INDEX_WHITELIST` per row (O(n·k) churn) |

### sepa — `src/citizens/sepa.rs` (`mod tests`)

| # | Attack | Asserted contract (GREEN) | Keystrokes of the trap |
|---|--------|---------------------------|------------------------|
| S1 | **industry with `industry_en` + en locale** shows en; **`None` + en** shows zh; **zh locale** always shows zh | three-way per the helper | baseline concatenates zh (`industry` L457) regardless — RED now |
| S2 | **theme concept-map hit / miss / partial** — [hit], [miss], [hit, miss] in `themes` with en locale | hit → mapped en; miss → zh; partial → mixed, each theme resolved independently | a naive `map.get(theme).unwrap_or(...)` on a missing mapping must fall back zh not panic/blank; partial-hit exercises per-item independence |
| S3 | **take(2) truncation + mapping** — `themes` with >2 entries, one of the retained two mapped, the truncated third irrelevant | only the first 2 influence the cell; mapped/zh exactly per map | guards mapping the wrong slice (applying the map before truncation vs after — both should agree, but a bug could map the dropped one) |
| S4 | **industry empty + en** — `industry=""`, `industry_en=None` → must not render a bare " · theme…" leading separator | if industry empty, no leading "·" | a `push_str(" · ")` before a separator check yields a leading-dot artifact |
| S5 | **concept row absence** — concept-map built from `index_basic.name_en` has **no row** for a theme's zh name → theme stays zh (no panic) | zh fallback on unmapped concept | the renderer must tolerate a map lookup miss when the concept table never carried the name (D1-A legacy rows) |

### screener — `src/citizens/screener.rs` (`mod tests`)

| # | Attack | Asserted contract (GREEN) | Keystrokes of the trap |
|---|--------|---------------------------|------------------------|
| SC1 | **industry dropdown en display** — en locale renders an en label for each zh industry key | every option label is the mapped en | baseline dropdown is fed zh keys (main.rs L133-138) — RED now |
| SC2 | **selected-value round-trip preserves the zh key** — user selects option "Alcohol", stored value/Filter stays `"白酒"` (the engine matches zh); re-open shows "Alcohol" selected | display label en, storage key zh, selection anchor stable across locale flips | the classic display/value mismatch: if `MultiSelect.selected` is seeded from the *en* label, reloading after a locale flip selects nothing (anchor lost) |
| SC3 | **table industry column en** — en locale + a result row whose industry is mapped shows en (data source per interface gap #7) | en shown | depends on the developer resolving gap #7; keep RED-writable once the source exists |
| SC4 | **unmapped industry both in dropdown and table stays zh** (en locale) | zh fallback, no blank, no panic | industries list contains raw zh keys; a mapping lookup miss must fall back zh everywhere |

## Ready-to-land RED test stubs (structure; bodies depend on the helper symbol)

Because the helper is unresolved, bodies are sketched — they become real
compilable RED once the implementer lands `display_name` + the row_cells
locale resolution. Placed inside each target `#[cfg(test)]` module.

```rust
// market.rs mod tests — fixture now needs name_en (also fixes the
// compile-broken sample_row, a B2 gap the implementer must backfill).
fn sample_row_en(symbol: &str, name: &str, index_type: &str, change: f64,
                 name_en: Option<&str>) -> IndexRow {
    IndexRow {
        symbol: symbol.to_string(), name: name.to_string(), index_type: index_type.to_string(),
        name_en: name_en.map(str::to_string),
        latest: 3000.0, change_pct: change, amount: 123_456_789.0,
    }
}

#[test] // M2 — blank name_en must fall back to zh
fn adversarial_270_market_empty_name_en_falls_back_zh() {
    let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
        .lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    compass_i18n::set_locale("en");
    let row = sample_row_en("SH000001", "上证指数", "official", 0.0, Some(""));
    let cells = MarketPanel::row_cells(&row);
    assert_eq!(cells[0], DataCell::Text("上证指数".to_string()),
        "M2: empty name_en must fall back to zh, not render a blank");
    compass_i18n::set_locale("zh");
}

#[test] // M3 — en present, zh locale: still zh (bidirectional)
fn adversarial_270_market_en_present_zh_locale_stays_zh() {
    let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
        .lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    compass_i18n::set_locale("zh");
    let row = sample_row_en("SH000001", "上证指数", "official", 0.0, Some("SSE Composite"));
    let cells = MarketPanel::row_cells(&row);
    assert_eq!(cells[0], DataCell::Text("上证指数".to_string()),
        "M3: zh locale must render zh even when name_en present");
}
```

## RED evidence (current baseline)

`cargo check --tests -p compass` FAILS at compile (blocker #3), so no RED
assertion output is producible yet. GREEN contract: after the implementer
lands B3c-B3f and fixes `sample_row`, every matrix case runs as an assertion
failure against any incomplete implementation, then passes once correct.

## Recommended un-DEFER path

1. Main agent lands B3 helper (`display_name`) + the three locale-resolution
   fixes (market/sepa/screener), and fixes the `market.rs sample_row` compile
   gap — producing the **first compilable interface commit**, and re-delegates
   with that commit SHA.
2. Resolve the two **interface gaps that B3 forgot to declare** before/when
   implementing: (a) the concept zh→en map injection into `SepaPanel::show`,
   (b) the `ScreenerRow.industry_en` data source for the screener table column.
3. On re-delegation, land the matrix above as in-source `#[cfg(test)]` additions
   (market/sepa/screener) OR `tests/` integration mounts via `#[path]` (pure-bin
   crate, `adversarial_245_screener_builder.rs` precedent) where the target is
   public-ish — the pure-function cases (M2/M3/M6/P1, S1-S5, SC2/SC4) are all
   expressible against `row_cells`/helper directly, so they need only the helper.
