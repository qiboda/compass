//! #218 requirement-acceptance tests (RED): K-line timeframe switching must
//! reload immediately.
//!
//! Contract under test:
//! 1. Segmented "1w" click syncs `shared_state.timeframe` and triggers a fetch.
//! 2. Digit shortcuts (Num1/2/3) sync the same fields.
//! 3. Startup `timeframe_index` derives from the configured `default_timeframe`.
//!
//! These tests mount under `crate::citizens` (declared `#[cfg(test)]` in
//! `mod.rs`) because the test-agent sandbox locks `src/main.rs` itself; the
//! helpers mirror the `main.rs` test module 1:1 so the main agent can move
//! the whole file into `mod tests` verbatim when committing.

use std::sync::Arc;

use compass_core::model::AppConfig;
use compass_ui::widgets::modal::Modal;
use compass_ui::widgets::searchable_dropdown::StockPicker;
use compass_ui::widgets::toast::ToastManager;
use egui_citizen::{CitizenId, Dispatcher};
use egui_dock::DockState;
use egui_kittest::kittest::Queryable;

use crate::CompassApp;
use crate::citizens::chart::ChartCitizen;
use crate::citizens::logger::LoggerPanel;
use crate::citizens::screener::ScreenerPanel;
use crate::citizens::sepa::SepaPanel;
use crate::state::SharedState;
use crate::stock_projection;
use crate::tabs::{CHART_ID, LOGGER_ID, SCREENER_ID, SEPA_ID, Tab, TabKind};
use crate::theme::CompassTheme;

pub(crate) fn build_compass_app_with_timeframe(
    egui_ctx: egui::Context,
    default_timeframe: &str,
) -> CompassApp {
    let config = AppConfig::default();
    let shared_state = Arc::new(SharedState::new("SZ000001", default_timeframe));

    let (work_signal, run_screener_signal, sepa_signal, _backend_handle) =
        crate::backend::wire_backend(config, shared_state.clone(), egui_ctx);

    let mut dispatcher = Dispatcher::new();
    let registered = crate::dispatcher::register_citizens(&mut dispatcher);

    let theme = CompassTheme::compass_dark();
    let theme_tokens = *theme.tokens();
    let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
    let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
    let screener = ScreenerPanel::new(
        CitizenId::new(SCREENER_ID),
        registered.screener,
        None,
        Box::new(|_| {}),
        &theme_tokens,
    );
    let sepa = SepaPanel::new(CitizenId::new(SEPA_ID), registered.sepa, &theme_tokens);
    let stock_picker = StockPicker::new(theme_tokens, "SZ000001", stock_projection());
    let dock_style = egui_dock::Style::default();

    let mut dock_state = DockState::new(vec![Tab::new(TabKind::Chart), Tab::new(TabKind::Sepa)]);
    if let Some(surface) = dock_state.get_surface_mut(egui_dock::SurfaceIndex::main())
        && let Some(tree) = surface.node_tree_mut()
    {
        let _ = tree.split_below(
            egui_dock::NodeIndex::root(),
            0.75,
            vec![Tab::new(TabKind::Logger)],
        );
        let _ = tree.split_below(
            egui_dock::NodeIndex::root(),
            0.5,
            vec![Tab::new(TabKind::Screener)],
        );
    }

    let startup_symbol = shared_state.symbol.get();
    let timeframe_index = crate::timeframe_index_from_value(&shared_state.timeframe.get());

    CompassApp {
        dock_state,
        dispatcher,
        chart,
        logger,
        screener,
        sepa,
        run_screener_signal,
        sepa_signal,
        screener_industries: Vec::new(),
        screener_boards: Vec::new(),
        shared_state,
        work_signal,
        stock_list: Vec::new(),
        stock_picker,
        // Mirrors the production constructor (main.rs L162): the index is
        // derived from `shared_state.timeframe` via the shared
        // `timeframe_index_from_value` helper.
        timeframe_index,
        theme,
        dock_style,
        _backend_handle,
        toast: ToastManager::new(theme_tokens),
        modal: Modal::new(theme_tokens),
        file_dialog: egui_file_dialog::FileDialog::new(),
        last_error: None,
        last_loading: false,
        last_screener_error: None,
        last_sepa_error: None,
        last_sepa_loading: false,
        last_screener_synced_symbol: startup_symbol,
        sidebar_visible: true,
        sidebar_search: String::new(),
        status_clock: String::new(),
        symbol_input_id: None,
        pending_delete: None,
        delete_confirmed: std::rc::Rc::new(std::cell::RefCell::new(false)),
        startup_modal_shown: false,
    }
}

pub(crate) fn build_compass_app(egui_ctx: egui::Context) -> CompassApp {
    build_compass_app_with_timeframe(egui_ctx, "1d")
}

pub(crate) fn build_compass_app_with_stocks(
    egui_ctx: egui::Context,
    stocks: Vec<compass_core::model::StockBasic>,
) -> CompassApp {
    let mut app = build_compass_app(egui_ctx);
    app.stock_list = stocks;
    app
}

pub(crate) fn sized_harness(app: CompassApp) -> egui_kittest::Harness<'static, CompassApp> {
    egui_kittest::Harness::builder()
        .with_size([1440.0, 900.0])
        .build_eframe(|_| app)
}

/// Segmented click must sync `shared_state.timeframe` AND trigger a fetch
/// (loading set). The fetch must fire even while a previous load is in
/// flight — loading data belongs to the old timeframe.
#[test]
fn segmented_switch_syncs_shared_state_and_triggers_fetch() {
    let mut app = build_compass_app(egui::Context::default());
    {
        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            app.render_toolbar(ui);
        });
        harness.run();
        harness.get_by_label("1w").click();
        harness.step();
    }
    assert_eq!(
        app.shared_state.timeframe.get(),
        "1w",
        "Segmented 1w click must update shared_state.timeframe"
    );
    assert!(
        app.shared_state.loading.get(),
        "switching timeframe must trigger an immediate fetch"
    );
}

/// Digit shortcut Num2 must sync the same fields as the Segmented click.
/// Asserted on synchronous state only (timeframe/index) — the loading flag
/// is skipped in the full-harness path because the wired backend thread may
/// complete the fetch within the same virtual frame.
#[test]
fn digit_key_switch_syncs_shared_state() {
    let app = build_compass_app(egui::Context::default());
    let mut harness = sized_harness(app);
    harness.run_steps(3);

    harness.key_press(egui::Key::Num2);
    harness.step();
    assert_eq!(harness.state().timeframe_index, 1);
    assert_eq!(
        harness.state().shared_state.timeframe.get(),
        "1w",
        "Num2 must update shared_state.timeframe"
    );

    harness.key_press(egui::Key::Num3);
    harness.step();
    assert_eq!(
        harness.state().shared_state.timeframe.get(),
        "1M",
        "Num3 must update shared_state.timeframe"
    );

    harness.key_press(egui::Key::Num1);
    harness.step();
    assert_eq!(
        harness.state().shared_state.timeframe.get(),
        "1d",
        "Num1 must update shared_state.timeframe"
    );
}

/// Startup: the segmented selection must match the configured
/// `default_timeframe` (e.g. "1w" → index 1), so the chart and the toolbar
/// never disagree on startup.
#[test]
fn startup_timeframe_index_matches_default_timeframe() {
    let app = build_compass_app_with_timeframe(egui::Context::default(), "1w");
    assert_eq!(
        app.timeframe_index, 1,
        "startup with default_timeframe=\"1w\" must select index 1"
    );
    assert_eq!(app.shared_state.timeframe.get(), "1w");
}
