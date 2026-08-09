//! #221 requirement-acceptance tests (RED): the SEPA ranking table must
//! render its body rows inside the real docked window.
//!
//! Repro path (plan §7): `build_compass_app_with_stocks` + `sized_harness`
//! 1440x900, 50 `sepa_data` rows injected via the pub `shared_state`, SEPA
//! tab activated programmatically (`DockState::set_active_tab` — egui_dock
//! tab buttons carry no accesskit label, see kb/dev/testing.md), then the
//! table body row labels are asserted.
//!
//! Mounted under `crate::citizens` like `ui_fixes_218.rs` (sandbox locks
//! `src/main.rs`); the main agent can move this into `mod tests` when
//! committing.

use compass_i18n::t;
use compass_types::{MarketThermometer, SepaData, SepaDetails, SepaFactor, SepaIndicator, SepaRow};
use egui_kittest::kittest::Queryable;

use super::ui_fixes_218::{LANG_LOCK, build_compass_app_with_stocks, sized_harness};
use crate::CompassApp;
use crate::tabs::{Tab, TabKind};

/// Key-resolution test helper (plan T4): resolves a key through the shared
/// compass-i18n dictionary.
fn tr(key: &str) -> String {
    t!(key).to_string()
}

fn sample_row(rank: usize, symbol: &str, name: &str) -> SepaRow {
    SepaRow {
        symbol: symbol.to_string(),
        name: name.to_string(),
        rank,
        total_score: 80.0 - rank as f64,
        trend: 20.0,
        theme: 18.0,
        capital: 15.0,
        pattern: 15.0,
        risk: 0.0,
        industry: "白酒".to_string(),
        themes: vec!["茅指数".to_string()],
        latest_price: 1500.0,
        change_pct: 2.5,
        details: SepaDetails {
            trend: vec![SepaFactor {
                label_key: "sepa.factor.vcp_quality",
                score: 9.2,
                max: 10.0,
                note_key: None,
                note_args: None,
            }],
            theme: vec![],
            capital: vec![],
            pattern: vec![],
            risk: vec![SepaFactor {
                label_key: "sepa.factor.volume_stagnation",
                score: 0.0,
                max: 2.0,
                note_key: None,
                note_args: None,
            }],
        },
    }
}

fn thermometer() -> MarketThermometer {
    MarketThermometer {
        score: 72.0,
        position_key: "sepa.position.full",
        position_pct: 90.0,
        indicators: vec![SepaIndicator {
            label_key: "sepa.indicator.hs300_trend",
            value: 62.4,
            unit_key: "sepa.unit.percent",
            delta_pct: Some(2.0),
            heat: 0.8,
        }],
    }
}

fn sepa_data_50() -> SepaData {
    let rows = (1..=50)
        .map(|i| sample_row(i, &format!("SH6000{i:02}"), &format!("测试股票{i}")))
        .collect();
    SepaData {
        rows,
        thermometer: thermometer(),
        date: "2026-08-02".to_string(),
    }
}

/// The docked SEPA panel must render the ranking table body: the first row's
/// stock-code label (rank 1 sorts first) and the row-count label must both
/// be present in the accesskit tree of the 1440x900 window.
#[test]
fn sepa_ranking_table_renders_body_rows_in_dock() {
    let _guard = LANG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = build_compass_app_with_stocks(egui::Context::default(), Vec::new());
    app.shared_state.sepa_data.set(Some(sepa_data_50()));
    app.shared_state.sepa_loading.set(false);
    activate_sepa_tab(&mut app);
    let mut harness = sized_harness(app);
    harness.run_steps(3);

    let _ = harness.get_by_label("SH600001");
    let count = harness
        .query_all_by_label_contains(&t!("widgets.data_table.count", count = 50))
        .count();
    assert!(
        count >= 1,
        "the 50-row count label must render in the docked SEPA panel"
    );
}

/// The ranking table must lay out header ABOVE body rows, not beside them.
///
/// `results_area` renders the table inside `ui::horizontal` (next to the
/// detail panel); egui_extras' TableBuilder assumes a vertical stacking
/// context for its header scope and body ScrollArea, so in a horizontal
/// layout the body rows are placed to the RIGHT of the header instead of
/// below it. This is the real-GUI regression reported in #221 ("列表在表头
/// 右边") — earlier tests only asserted label existence, never position.
#[test]
fn sepa_table_header_is_above_body_rows() {
    let _guard = LANG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = build_compass_app_with_stocks(egui::Context::default(), Vec::new());
    app.shared_state.sepa_data.set(Some(sepa_data_50()));
    app.shared_state.sepa_loading.set(false);
    activate_sepa_tab(&mut app);
    let mut harness = sized_harness(app);
    harness.run_steps(3);

    // "排名" also appears inside the detail-panel hint ("点击排名行查看
    // 评分详情"), so disambiguate by x: the table header sits left of the
    // detail panel, while the hint is in the right-side detail panel.
    let rank_header = tr("sepa.table.rank");
    let header = harness
        .query_all_by_label_contains(&rank_header)
        .min_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .expect("header '排名' cell must exist");
    let first_row = harness.get_by_label("SH600001");
    let header = header.rect();
    let first_row = first_row.rect();
    assert!(
        header.min.x < first_row.min.x,
        "table header must sit left of the first body row (body BELOW header), got header x={:.1} row x={:.1}",
        header.min.x,
        first_row.min.x,
    );

    // Header must be above the first body row (body BELOW header).
    assert!(
        header.max.y <= first_row.min.y,
        "table header must stack ABOVE the body rows, but got header bottom={:.1} row top={:.1} (x header={:.1} row={:.1})",
        header.max.y,
        first_row.min.y,
        header.min.x,
        first_row.min.x,
    );
}

/// The docked SEPA panel must render all 50 injected rows (not truncate to
/// the visible viewport): the last row's code must exist too.
#[test]
fn sepa_ranking_table_renders_all_fifty_rows_in_dock() {
    let _guard = LANG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = build_compass_app_with_stocks(egui::Context::default(), Vec::new());
    app.shared_state.sepa_data.set(Some(sepa_data_50()));
    app.shared_state.sepa_loading.set(false);
    activate_sepa_tab(&mut app);
    let mut harness = sized_harness(app);
    harness.run_steps(3);

    let _ = harness.get_by_label("SH600050");
}

/// The dock tree splits below the root (Chart+Sepa share the top leaf), so
/// the root node is a split, not a leaf: locate the SEPA tab's leaf via
/// `find_tab` before activating it.
fn activate_sepa_tab(app: &mut CompassApp) {
    let path = app
        .dock_state
        .find_tab(&Tab::new(TabKind::Sepa))
        .expect("SEPA tab exists in the dock tree");
    app.dock_state
        .set_active_tab(path)
        .expect("activate SEPA tab");
}

/// Sanity guard: the injected fixture itself is deterministic (50 rows,
/// rank-1 code first, rank-50 last) so failures in the dock tests above
/// cannot be blamed on the fixture.
#[test]
fn sepa_dock_test_data_is_deterministic() {
    let _guard = LANG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let data = sepa_data_50();
    assert_eq!(data.rows.len(), 50);
    assert_eq!(data.rows[0].symbol, "SH600001");
    assert_eq!(data.rows[49].symbol, "SH600050");
}
