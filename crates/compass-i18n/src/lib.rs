//! Compass GUI centralized i18n — zh/en locale dictionaries and rust-i18n
//! re-exports (issue #222).
//!
//! This crate owns the single `locales/` directory (zh.yml + en.yml, the
//! full key tree from `.omo/designs/gui-i18n.md` §1). It is the
//! share-in-workspace hub: `compass` and `compass-ui` both declare
//! `i18n!("../compass-i18n/locales")` pointing here, so every `t!()` call in
//! the workspace resolves against the same embedded data.
//!
//! `set_locale` is process-global (rust-i18n), so one call from `main()`
//! switches the whole GUI including the egui-charts fork's own locale data.

rust_i18n::i18n!("locales", fallback = "zh");

pub use rust_i18n::available_locales;
pub use rust_i18n::locale;
pub use rust_i18n::set_locale;
pub use rust_i18n::t;

// ---------------------------------------------------------------------------
// Key constants — compile-time-checkable keys for the business layer.
//
// Referencing `compass_i18n::KEY_APP_TITLE` instead of a bare `"app.title"`
// literal lets the compiler catch typos; the key-completeness test asserts
// every constant resolves in both locales.
// ---------------------------------------------------------------------------

pub const KEY_APP_TITLE: &str = "app.title";
pub const KEY_TAB_CHART: &str = "tab.chart";
pub const KEY_TAB_LOGGER: &str = "tab.logger";
pub const KEY_TAB_SCREENER: &str = "tab.screener";
pub const KEY_TAB_SEPA: &str = "tab.sepa";
pub const KEY_TAB_MARKET: &str = "tab.market";
pub const KEY_TOOLBAR_FETCH: &str = "toolbar.fetch";
pub const KEY_TOOLBAR_LOADING: &str = "toolbar.loading";
pub const KEY_TOOLBAR_ADJUST: &str = "toolbar.adjust";
pub const KEY_TOOLBAR_TOGGLE_SIDEBAR: &str = "toolbar.toggle_sidebar";
pub const KEY_SIDEBAR_GROUP_WATCHLIST: &str = "sidebar.group_watchlist";
pub const KEY_SIDEBAR_SEARCH_PLACEHOLDER: &str = "sidebar.search_placeholder";
pub const KEY_SIDEBAR_ADD_TOOLTIP: &str = "sidebar.add_tooltip";
pub const KEY_SIDEBAR_DELETE_TOOLTIP: &str = "sidebar.delete_tooltip";
pub const KEY_SIDEBAR_EMPTY_TITLE: &str = "sidebar.empty_title";
pub const KEY_SIDEBAR_EMPTY_DESC: &str = "sidebar.empty_desc";
pub const KEY_STATUSBAR_LOADING: &str = "statusbar.loading";
pub const KEY_STATUSBAR_SOURCE: &str = "statusbar.source";
pub const KEY_COMMON_LOADING: &str = "common.loading";
pub const KEY_COMMON_REFRESH: &str = "common.refresh";
pub const KEY_COMMON_CONFIRM: &str = "common.confirm";
pub const KEY_COMMON_CANCEL: &str = "common.cancel";
pub const KEY_COMMON_REMOVE: &str = "common.remove";
pub const KEY_COMMON_SEARCH: &str = "common.search";
pub const KEY_COMMON_NO_MATCHES: &str = "common.no_matches";
pub const KEY_COMMON_ALL: &str = "common.all";
pub const KEY_CHART_EMPTY_TITLE: &str = "chart.empty_title";
pub const KEY_CHART_EMPTY_DESC: &str = "chart.empty_desc";
pub const KEY_LOGGER_TITLE: &str = "logger.title";
pub const KEY_LOGGER_EXPORT_TOOLTIP: &str = "logger.export_tooltip";
pub const KEY_LOGGER_LOG_FETCH_FAILED: &str = "logger.log_fetch_failed";
pub const KEY_LOGGER_LOG_FETCH_COMPLETED: &str = "logger.log_fetch_completed";
pub const KEY_LOGGER_LOG_SCREENER_FAILED: &str = "logger.log_screener_failed";
pub const KEY_LOGGER_LOG_SCREENER_COMPLETED: &str = "logger.log_screener_completed";
pub const KEY_LOGGER_LOG_SEPA_FAILED: &str = "logger.log_sepa_failed";
pub const KEY_LOGGER_LOG_SEPA_COMPLETED: &str = "logger.log_sepa_completed";
pub const KEY_LOGGER_LOG_INDEX_FAILED: &str = "logger.log_index_failed";
pub const KEY_LOGGER_LOG_INDEX_COMPLETED: &str = "logger.log_index_completed";
pub const KEY_MODAL_STARTUP_TITLE: &str = "modal.startup.title";
pub const KEY_MODAL_STARTUP_BODY: &str = "modal.startup.body";
pub const KEY_MODAL_STARTUP_CONFIRM: &str = "modal.startup.confirm";
pub const KEY_MODAL_REMOVE_TITLE: &str = "modal.remove.title";
pub const KEY_MODAL_REMOVE_BODY: &str = "modal.remove.body";
pub const KEY_MODAL_REMOVE_CONFIRM: &str = "modal.remove.confirm";
pub const KEY_MODAL_REMOVE_CANCEL: &str = "modal.remove.cancel";
pub const KEY_TOAST_THEME_SWITCHED: &str = "toast.theme_switched";
pub const KEY_TOAST_LANGUAGE_SWITCHED: &str = "toast.language_switched";
pub const KEY_TOAST_FETCH_SUCCESS: &str = "toast.fetch_success";
pub const KEY_TOAST_WATCHLIST_ADDED: &str = "toast.watchlist_added";
pub const KEY_TOAST_WATCHLIST_REMOVED: &str = "toast.watchlist_removed";
pub const KEY_TOAST_LOG_EXPORTED: &str = "toast.log_exported";
pub const KEY_TOAST_LOG_EXPORT_FAILED: &str = "toast.log_export_failed";
pub const KEY_TOAST_SEPA_UPDATED: &str = "toast.sepa_updated";
pub const KEY_TOAST_INDEX_UPDATED: &str = "toast.index_updated";
pub const KEY_ERROR_DUCKDB_OPEN: &str = "error.duckdb_open";
pub const KEY_ERROR_PARQUET_OPEN: &str = "error.parquet_open";
pub const KEY_ERROR_NO_DATA: &str = "error.no_data";
pub const KEY_ERROR_SCREENER_RUN: &str = "error.screener_run";
pub const KEY_ERROR_SEPA_RUN: &str = "error.sepa_run";
pub const KEY_ERROR_INDEX_RUN: &str = "error.index_run";
pub const KEY_SCREENER_FILTER: &str = "screener.filter";
pub const KEY_SCREENER_FILTERING: &str = "screener.filtering";
pub const KEY_SCREENER_CARD_BASIC: &str = "screener.card_basic";
pub const KEY_SCREENER_CARD_TECHNICAL: &str = "screener.card_technical";
pub const KEY_SCREENER_INDUSTRY: &str = "screener.industry";
pub const KEY_SCREENER_EXCHANGE: &str = "screener.exchange";
pub const KEY_SCREENER_BOARD: &str = "screener.board";
pub const KEY_SCREENER_LIST_YEARS: &str = "screener.list_years";
pub const KEY_SCREENER_ANY: &str = "screener.any";
pub const KEY_SCREENER_YEARS_1: &str = "screener.years_1";
pub const KEY_SCREENER_YEARS_3: &str = "screener.years_3";
pub const KEY_SCREENER_YEARS_5: &str = "screener.years_5";
pub const KEY_SCREENER_MARKET_CAP: &str = "screener.market_cap";
pub const KEY_SCREENER_EXCLUDE_DELISTED: &str = "screener.exclude_delisted";
pub const KEY_SCREENER_MA: &str = "screener.ma";
pub const KEY_SCREENER_MA_ABOVE20: &str = "screener.ma_above20";
pub const KEY_SCREENER_MA_ABOVE60: &str = "screener.ma_above60";
pub const KEY_SCREENER_MA_BULLISH: &str = "screener.ma_bullish";
pub const KEY_SCREENER_BREAKOUT: &str = "screener.breakout";
pub const KEY_SCREENER_MOMENTUM: &str = "screener.momentum";
pub const KEY_SCREENER_VOLUME: &str = "screener.volume";
pub const KEY_SCREENER_N_LABEL: &str = "screener.n_label";
pub const KEY_SCREENER_MIN_PCT: &str = "screener.min_pct";
pub const KEY_SCREENER_MAX_PCT: &str = "screener.max_pct";
pub const KEY_SCREENER_TIMES: &str = "screener.times";
pub const KEY_SEPA_THERMOMETER: &str = "sepa.thermometer";
pub const KEY_SEPA_COUNT: &str = "sepa.count";
pub const KEY_SEPA_NO_DATA: &str = "sepa.no_data";
pub const KEY_SEPA_COMPUTING: &str = "sepa.computing";
pub const KEY_SEPA_COMPUTING_FULL: &str = "sepa.computing_full";
pub const KEY_SEPA_REFRESH: &str = "sepa.refresh";
pub const KEY_SEPA_EMPTY_TITLE: &str = "sepa.empty_title";
pub const KEY_SEPA_EMPTY_DESC: &str = "sepa.empty_desc";
pub const KEY_SEPA_DETAIL_HINT: &str = "sepa.detail_hint";
pub const KEY_SEPA_TOTAL_SCORE: &str = "sepa.total_score";
pub const KEY_SEPA_TABLE_RANK: &str = "sepa.table.rank";
pub const KEY_SEPA_TABLE_CODE: &str = "sepa.table.code";
pub const KEY_SEPA_TABLE_NAME: &str = "sepa.table.name";
pub const KEY_SEPA_TABLE_TOTAL: &str = "sepa.table.total";
pub const KEY_SEPA_TABLE_TREND: &str = "sepa.table.trend";
pub const KEY_SEPA_TABLE_THEME: &str = "sepa.table.theme";
pub const KEY_SEPA_TABLE_CAPITAL: &str = "sepa.table.capital";
pub const KEY_SEPA_TABLE_PATTERN: &str = "sepa.table.pattern";
pub const KEY_SEPA_TABLE_RISK: &str = "sepa.table.risk";
pub const KEY_SEPA_TABLE_INDUSTRY: &str = "sepa.table.industry";
pub const KEY_SEPA_TABLE_LATEST: &str = "sepa.table.latest";
pub const KEY_SEPA_TABLE_CHANGE: &str = "sepa.table.change";
pub const KEY_SEPA_MODULE_TREND: &str = "sepa.module.trend";
pub const KEY_SEPA_MODULE_THEME: &str = "sepa.module.theme";
pub const KEY_SEPA_MODULE_CAPITAL: &str = "sepa.module.capital";
pub const KEY_SEPA_MODULE_PATTERN: &str = "sepa.module.pattern";
pub const KEY_SEPA_MODULE_RISK: &str = "sepa.module.risk";
pub const KEY_SEPA_POSITION_FULL: &str = "sepa.position.full";
pub const KEY_SEPA_POSITION_MID: &str = "sepa.position.mid";
pub const KEY_SEPA_POSITION_LOW: &str = "sepa.position.low";
pub const KEY_SEPA_UNIT_PERCENT: &str = "sepa.unit.percent";
pub const KEY_SEPA_UNIT_COUNT: &str = "sepa.unit.count";
pub const KEY_SEPA_UNIT_TRILLION: &str = "sepa.unit.trillion";
pub const KEY_SEPA_INDICATOR_HS300_TREND: &str = "sepa.indicator.hs300_trend";
pub const KEY_SEPA_INDICATOR_ZZ1000_TREND: &str = "sepa.indicator.zz1000_trend";
pub const KEY_SEPA_INDICATOR_LIMIT_UP: &str = "sepa.indicator.limit_up";
pub const KEY_SEPA_INDICATOR_AMOUNT: &str = "sepa.indicator.amount";
pub const KEY_SEPA_INDICATOR_BREADTH: &str = "sepa.indicator.breadth";
pub const KEY_SEPA_FACTOR_MA_STRUCTURE: &str = "sepa.factor.ma_structure";
pub const KEY_SEPA_FACTOR_PRICE_POSITION: &str = "sepa.factor.price_position";
pub const KEY_SEPA_FACTOR_RELATIVE_STRENGTH: &str = "sepa.factor.relative_strength";
pub const KEY_SEPA_FACTOR_SECTOR_GAIN: &str = "sepa.factor.sector_gain";
pub const KEY_SEPA_FACTOR_SECTOR_AMOUNT: &str = "sepa.factor.sector_amount";
pub const KEY_SEPA_FACTOR_SECTOR_DIFFUSION: &str = "sepa.factor.sector_diffusion";
pub const KEY_SEPA_FACTOR_NEWS_HEAT: &str = "sepa.factor.news_heat";
pub const KEY_SEPA_FACTOR_VOLUME_PRICE: &str = "sepa.factor.volume_price";
pub const KEY_SEPA_FACTOR_CHIP_CONCENTRATION: &str = "sepa.factor.chip_concentration";
pub const KEY_SEPA_FACTOR_BIG_CAPITAL_INFLOW: &str = "sepa.factor.big_capital_inflow";
pub const KEY_SEPA_FACTOR_VCP_QUALITY: &str = "sepa.factor.vcp_quality";
pub const KEY_SEPA_FACTOR_BREAKOUT_CONFIRM: &str = "sepa.factor.breakout_confirm";
pub const KEY_SEPA_FACTOR_VOL_PENALTY: &str = "sepa.factor.vol_penalty";
pub const KEY_SEPA_FACTOR_DEEP_DRAWDOWN: &str = "sepa.factor.deep_drawdown";
pub const KEY_SEPA_FACTOR_VOLUME_STAGNATION: &str = "sepa.factor.volume_stagnation";
pub const KEY_SEPA_NOTE_DRAWDOWN: &str = "sepa.note.drawdown";
pub const KEY_SEPA_NOTE_MOMENTUM_PERCENTILE: &str = "sepa.note.momentum_percentile";
pub const KEY_SEPA_NOTE_NO_SECTOR_DATA: &str = "sepa.note.no_sector_data";
pub const KEY_SEPA_NOTE_NEWS_V1: &str = "sepa.note.news_v1";
pub const KEY_SEPA_NOTE_NEWS_DEFAULT: &str = "sepa.note.news_default";
pub const KEY_SEPA_NOTE_BIG_CAPITAL: &str = "sepa.note.big_capital";
pub const KEY_SEPA_NOTE_THERMOMETER: &str = "sepa.note.thermometer";
pub const KEY_INDEX_CARD_TITLE: &str = "index.card_title";
pub const KEY_INDEX_COUNT: &str = "index.count";
pub const KEY_INDEX_NO_DATA: &str = "index.no_data";
pub const KEY_INDEX_COMPUTING: &str = "index.computing";
pub const KEY_INDEX_REFRESH: &str = "index.refresh";
pub const KEY_INDEX_EMPTY_TITLE: &str = "index.empty_title";
pub const KEY_INDEX_EMPTY_DESC: &str = "index.empty_desc";
pub const KEY_INDEX_SEGMENT_INDUSTRY: &str = "index.segment.industry";
pub const KEY_INDEX_SEGMENT_CONCEPT: &str = "index.segment.concept";
pub const KEY_INDEX_SEGMENT_OFFICIAL: &str = "index.segment.official";
pub const KEY_INDEX_TABLE_NAME: &str = "index.table.name";
pub const KEY_INDEX_TABLE_CODE: &str = "index.table.code";
pub const KEY_INDEX_TABLE_LATEST: &str = "index.table.latest";
pub const KEY_INDEX_TABLE_CHANGE: &str = "index.table.change";
pub const KEY_INDEX_TABLE_AMOUNT: &str = "index.table.amount";
pub const KEY_WIDGETS_SEARCHABLE_DROPDOWN_NO_MATCHES: &str =
    "widgets.searchable_dropdown.no_matches";
pub const KEY_WIDGETS_DATA_TABLE_COUNT: &str = "widgets.data_table.count";
pub const KEY_WIDGETS_DATA_TABLE_EMPTY: &str = "widgets.data_table.empty";
pub const KEY_WIDGETS_MULTI_SELECT_SELECTED: &str = "widgets.multi_select.selected";
pub const KEY_WIDGETS_MULTI_SELECT_CONFIRM: &str = "widgets.multi_select.confirm";

/// All key constants, for completeness testing and future tooling.
pub const ALL_KEYS: &[&str] = &[
    KEY_APP_TITLE,
    KEY_TAB_CHART,
    KEY_TAB_LOGGER,
    KEY_TAB_SCREENER,
    KEY_TAB_SEPA,
    KEY_TAB_MARKET,
    KEY_TOOLBAR_FETCH,
    KEY_TOOLBAR_LOADING,
    KEY_TOOLBAR_ADJUST,
    KEY_TOOLBAR_TOGGLE_SIDEBAR,
    KEY_SIDEBAR_GROUP_WATCHLIST,
    KEY_SIDEBAR_SEARCH_PLACEHOLDER,
    KEY_SIDEBAR_ADD_TOOLTIP,
    KEY_SIDEBAR_DELETE_TOOLTIP,
    KEY_SIDEBAR_EMPTY_TITLE,
    KEY_SIDEBAR_EMPTY_DESC,
    KEY_STATUSBAR_LOADING,
    KEY_STATUSBAR_SOURCE,
    KEY_COMMON_LOADING,
    KEY_COMMON_REFRESH,
    KEY_COMMON_CONFIRM,
    KEY_COMMON_CANCEL,
    KEY_COMMON_REMOVE,
    KEY_COMMON_SEARCH,
    KEY_COMMON_NO_MATCHES,
    KEY_COMMON_ALL,
    KEY_CHART_EMPTY_TITLE,
    KEY_CHART_EMPTY_DESC,
    KEY_LOGGER_TITLE,
    KEY_LOGGER_EXPORT_TOOLTIP,
    KEY_LOGGER_LOG_FETCH_FAILED,
    KEY_LOGGER_LOG_FETCH_COMPLETED,
    KEY_LOGGER_LOG_SCREENER_FAILED,
    KEY_LOGGER_LOG_SCREENER_COMPLETED,
    KEY_LOGGER_LOG_SEPA_FAILED,
    KEY_LOGGER_LOG_SEPA_COMPLETED,
    KEY_LOGGER_LOG_INDEX_FAILED,
    KEY_LOGGER_LOG_INDEX_COMPLETED,
    KEY_MODAL_STARTUP_TITLE,
    KEY_MODAL_STARTUP_BODY,
    KEY_MODAL_STARTUP_CONFIRM,
    KEY_MODAL_REMOVE_TITLE,
    KEY_MODAL_REMOVE_BODY,
    KEY_MODAL_REMOVE_CONFIRM,
    KEY_MODAL_REMOVE_CANCEL,
    KEY_TOAST_THEME_SWITCHED,
    KEY_TOAST_LANGUAGE_SWITCHED,
    KEY_TOAST_FETCH_SUCCESS,
    KEY_TOAST_WATCHLIST_ADDED,
    KEY_TOAST_WATCHLIST_REMOVED,
    KEY_TOAST_LOG_EXPORTED,
    KEY_TOAST_LOG_EXPORT_FAILED,
    KEY_TOAST_SEPA_UPDATED,
    KEY_TOAST_INDEX_UPDATED,
    KEY_ERROR_DUCKDB_OPEN,
    KEY_ERROR_PARQUET_OPEN,
    KEY_ERROR_NO_DATA,
    KEY_ERROR_SCREENER_RUN,
    KEY_ERROR_SEPA_RUN,
    KEY_ERROR_INDEX_RUN,
    KEY_SCREENER_FILTER,
    KEY_SCREENER_FILTERING,
    KEY_SCREENER_CARD_BASIC,
    KEY_SCREENER_CARD_TECHNICAL,
    KEY_SCREENER_INDUSTRY,
    KEY_SCREENER_EXCHANGE,
    KEY_SCREENER_BOARD,
    KEY_SCREENER_LIST_YEARS,
    KEY_SCREENER_ANY,
    KEY_SCREENER_YEARS_1,
    KEY_SCREENER_YEARS_3,
    KEY_SCREENER_YEARS_5,
    KEY_SCREENER_MARKET_CAP,
    KEY_SCREENER_EXCLUDE_DELISTED,
    KEY_SCREENER_MA,
    KEY_SCREENER_MA_ABOVE20,
    KEY_SCREENER_MA_ABOVE60,
    KEY_SCREENER_MA_BULLISH,
    KEY_SCREENER_BREAKOUT,
    KEY_SCREENER_MOMENTUM,
    KEY_SCREENER_VOLUME,
    KEY_SCREENER_N_LABEL,
    KEY_SCREENER_MIN_PCT,
    KEY_SCREENER_MAX_PCT,
    KEY_SCREENER_TIMES,
    KEY_SEPA_THERMOMETER,
    KEY_SEPA_COUNT,
    KEY_SEPA_NO_DATA,
    KEY_SEPA_COMPUTING,
    KEY_SEPA_COMPUTING_FULL,
    KEY_SEPA_REFRESH,
    KEY_SEPA_EMPTY_TITLE,
    KEY_SEPA_EMPTY_DESC,
    KEY_SEPA_DETAIL_HINT,
    KEY_SEPA_TOTAL_SCORE,
    KEY_SEPA_TABLE_RANK,
    KEY_SEPA_TABLE_CODE,
    KEY_SEPA_TABLE_NAME,
    KEY_SEPA_TABLE_TOTAL,
    KEY_SEPA_TABLE_TREND,
    KEY_SEPA_TABLE_THEME,
    KEY_SEPA_TABLE_CAPITAL,
    KEY_SEPA_TABLE_PATTERN,
    KEY_SEPA_TABLE_RISK,
    KEY_SEPA_TABLE_INDUSTRY,
    KEY_SEPA_TABLE_LATEST,
    KEY_SEPA_TABLE_CHANGE,
    KEY_SEPA_MODULE_TREND,
    KEY_SEPA_MODULE_THEME,
    KEY_SEPA_MODULE_CAPITAL,
    KEY_SEPA_MODULE_PATTERN,
    KEY_SEPA_MODULE_RISK,
    KEY_SEPA_POSITION_FULL,
    KEY_SEPA_POSITION_MID,
    KEY_SEPA_POSITION_LOW,
    KEY_SEPA_UNIT_PERCENT,
    KEY_SEPA_UNIT_COUNT,
    KEY_SEPA_UNIT_TRILLION,
    KEY_SEPA_INDICATOR_HS300_TREND,
    KEY_SEPA_INDICATOR_ZZ1000_TREND,
    KEY_SEPA_INDICATOR_LIMIT_UP,
    KEY_SEPA_INDICATOR_AMOUNT,
    KEY_SEPA_INDICATOR_BREADTH,
    KEY_SEPA_FACTOR_MA_STRUCTURE,
    KEY_SEPA_FACTOR_PRICE_POSITION,
    KEY_SEPA_FACTOR_RELATIVE_STRENGTH,
    KEY_SEPA_FACTOR_SECTOR_GAIN,
    KEY_SEPA_FACTOR_SECTOR_AMOUNT,
    KEY_SEPA_FACTOR_SECTOR_DIFFUSION,
    KEY_SEPA_FACTOR_NEWS_HEAT,
    KEY_SEPA_FACTOR_VOLUME_PRICE,
    KEY_SEPA_FACTOR_CHIP_CONCENTRATION,
    KEY_SEPA_FACTOR_BIG_CAPITAL_INFLOW,
    KEY_SEPA_FACTOR_VCP_QUALITY,
    KEY_SEPA_FACTOR_BREAKOUT_CONFIRM,
    KEY_SEPA_FACTOR_VOL_PENALTY,
    KEY_SEPA_FACTOR_DEEP_DRAWDOWN,
    KEY_SEPA_FACTOR_VOLUME_STAGNATION,
    KEY_SEPA_NOTE_DRAWDOWN,
    KEY_SEPA_NOTE_MOMENTUM_PERCENTILE,
    KEY_SEPA_NOTE_NO_SECTOR_DATA,
    KEY_SEPA_NOTE_NEWS_V1,
    KEY_SEPA_NOTE_NEWS_DEFAULT,
    KEY_SEPA_NOTE_BIG_CAPITAL,
    KEY_SEPA_NOTE_THERMOMETER,
    KEY_INDEX_CARD_TITLE,
    KEY_INDEX_COUNT,
    KEY_INDEX_NO_DATA,
    KEY_INDEX_COMPUTING,
    KEY_INDEX_REFRESH,
    KEY_INDEX_EMPTY_TITLE,
    KEY_INDEX_EMPTY_DESC,
    KEY_INDEX_SEGMENT_INDUSTRY,
    KEY_INDEX_SEGMENT_CONCEPT,
    KEY_INDEX_SEGMENT_OFFICIAL,
    KEY_INDEX_TABLE_NAME,
    KEY_INDEX_TABLE_CODE,
    KEY_INDEX_TABLE_LATEST,
    KEY_INDEX_TABLE_CHANGE,
    KEY_INDEX_TABLE_AMOUNT,
    KEY_WIDGETS_SEARCHABLE_DROPDOWN_NO_MATCHES,
    KEY_WIDGETS_DATA_TABLE_COUNT,
    KEY_WIDGETS_DATA_TABLE_EMPTY,
    KEY_WIDGETS_MULTI_SELECT_SELECTED,
    KEY_WIDGETS_MULTI_SELECT_CONFIRM,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn locale_keys() -> std::collections::BTreeMap<String, BTreeSet<String>> {
        let path = format!("{}/locales", env!("CARGO_MANIFEST_DIR"));
        let locales =
            rust_i18n::try_load_locales(&path, |_| false, true).expect("locales dir must parse");
        locales
            .into_iter()
            .map(|(locale, translations)| {
                let keys = translations.keys().cloned().collect();
                (locale, keys)
            })
            .collect()
    }

    /// 白名单 key 前缀：允许 zh 值无 CJK 字符的技术 token / 格式串前缀。
    ///
    /// 从 `zh_values_are_chinese` 的断言 OR 链中提取，主测试与表驱动用例
    /// 共享同一份白名单。提取前 `screener.times` / `sepa.unit` /
    /// `screener.ma` / `screener.years` 分支在真实 zh.yml 数据下被更早的
    /// `cjk_count > 0` / `value.contains("%{")` 分支短路（count=0，
    /// 覆盖率缺口）——提取后表驱动用例可对每个前缀逐一真实求值。
    fn is_allowed_zh_token(key: &str) -> bool {
        key.starts_with("app.")
            || key.starts_with("screener.n_label")
            || key.starts_with("screener.min_pct")
            || key.starts_with("screener.max_pct")
            || key.starts_with("screener.times")
            || key.starts_with("sepa.position")
            || key.starts_with("sepa.unit")
            || key.starts_with("screener.ma")
            || key.starts_with("screener.years")
    }

    /// Every key constant must resolve to a real translation in BOTH locales —
    /// rust-i18n's missing-key fallback returns the key string itself, so
    /// `t!(key) != key` is the anti-false-positive check (plan Metis A7).
    #[test]
    fn all_key_constants_resolve_in_zh_and_en() {
        for &key in ALL_KEYS {
            rust_i18n::set_locale("zh");
            let zh = t!(key);
            assert_ne!(zh, key, "zh translation missing for key {key}");
            assert!(!zh.is_empty(), "zh translation empty for key {key}");

            rust_i18n::set_locale("en");
            let en = t!(key);
            assert_ne!(en, key, "en translation missing for key {key}");
            assert!(!en.is_empty(), "en translation empty for key {key}");
        }
        rust_i18n::set_locale("zh");
    }

    /// zh.yml and en.yml key sets must be exactly symmetric. Read the on-disk
    /// files via try_load_locales (t!() cannot detect asymmetry — a key
    /// missing from one locale silently falls back to the key string).
    #[test]
    fn locale_files_are_key_symmetric() {
        let keys = locale_keys();
        assert_eq!(keys.len(), 2, "expected exactly zh + en locales");
        let zh: BTreeSet<_> = keys["zh"].iter().cloned().collect();
        let en: BTreeSet<_> = keys["en"].iter().cloned().collect();
        assert_eq!(zh.len(), en.len(), "zh/en key counts differ");
        assert_eq!(zh, en, "zh and en key sets must be identical");
        assert!(!zh.is_empty(), "locale key sets must not be empty");
    }

    /// No CJK characters may appear in any EN translation value — a Chinese
    /// value in the English locale is a defect (adversarial gate).
    #[test]
    fn en_values_contain_no_cjk_characters() {
        let path = format!("{}/locales", env!("CARGO_MANIFEST_DIR"));
        let locales =
            rust_i18n::try_load_locales(&path, |_| false, true).expect("locales dir must parse");
        let en = &locales["en"];
        for (key, value) in en {
            for c in value.chars() {
                assert!(
                    !('一'..='鿿').contains(&c),
                    "en value for {key} contains CJK char {c}: {value}"
                );
            }
        }
    }

    /// zh.yml values must contain no raw ASCII text (both locales agree on
    /// the zh file being the Chinese dictionary — the zh values are all CJK
    /// except technical tokens like MA/BOLL/SEPA and format strings).
    #[test]
    fn zh_values_are_chinese() {
        let path = format!("{}/locales", env!("CARGO_MANIFEST_DIR"));
        let locales =
            rust_i18n::try_load_locales(&path, |_| false, true).expect("locales dir must parse");
        let zh = &locales["zh"];
        for (key, value) in zh {
            let cjk_count = value.chars().filter(|c| ('一'..='鿿').contains(c)).count();
            assert!(
                cjk_count > 0 || value.contains("%{") || is_allowed_zh_token(key),
                "zh value for {key} has no CJK and is not an allowed technical token: {value}"
            );
        }
    }

    /// 表驱动：白名单每个前缀分支都被真实求值（覆盖 sepa.unit /
    /// screener.ma / screener.years 等在真实 zh.yml 下被短路的分支）。
    ///
    /// 真实 zh.yml 中 `sepa.unit*` 前缀的值全部含 `%{` 占位符（被
    /// `value.contains("%{")` 短路），`screener.ma*` / `screener.years*`
    /// 前缀的值全部含 CJK（被 `cjk_count > 0` 短路）——主测试的 OR 链
    /// 永远到不了这三个分支。此处构造 value 无 CJK、无 `%{` 的
    /// (key, value) 对，唯一放行途径就是对应白名单前缀，逐一真实命中。
    #[test]
    fn zh_whitelist_prefixes_allow_cjk_free_values() {
        // (key, value)：value 不含 CJK、不含 %{ 占位符（模拟真实格式串/技术 token）
        let cases: &[(&str, &str)] = &[
            // key.starts_with("app.")
            ("app.title", "Compass — Stock Chart"),
            // key.starts_with("screener.n_label")
            ("screener.n_label", "N:"),
            // key.starts_with("screener.min_pct")
            ("screener.min_pct", "min%:"),
            // key.starts_with("screener.max_pct")
            ("screener.max_pct", "max%:"),
            // key.starts_with("screener.times")
            ("screener.times", "x3"),
            // key.starts_with("sepa.position")
            ("sepa.position.full", "80%-100%"),
            // key.starts_with("sepa.unit") —— 真实数据下被 %{ 分支短路
            ("sepa.unit.percent", "100%"),
            ("sepa.unit.count", "10"),
            ("sepa.unit.trillion", "1.2"),
            // key.starts_with("screener.ma") —— 真实数据下被 CJK 分支短路
            ("screener.ma", "MA"),
            ("screener.ma_above20", "MA20"),
            ("screener.ma_above60", "MA60"),
            ("screener.ma_bullish", "MA5>MA20>MA60"),
            // key.starts_with("screener.years") —— 真实数据下被 CJK 分支短路
            ("screener.years_1", "1Y"),
            ("screener.years_3", "3Y"),
            ("screener.years_5", "5Y"),
        ];
        for &(key, value) in cases {
            assert!(
                !value.contains("%{") && !value.chars().any(|c| ('一'..='鿿').contains(&c)),
                "测试夹具必须无 CJK 且无 %{{ 占位符: {key}={value}"
            );
            assert!(
                is_allowed_zh_token(key),
                "key {key} 应在白名单内，否则其无 CJK 值 {value} 会触发 zh 值断言"
            );
        }
    }

    /// 负面用例：非白名单 key 不得被放行——即使未来其 zh 值被改成无 CJK
    /// 的文本，也绝不能靠白名单蒙混过关（与 zh_values_are_chinese 的
    /// 防御意图一致）。
    #[test]
    fn zh_whitelist_rejects_non_whitelisted_keys() {
        // 全部为 zh.yml 真实 key，且不以任何白名单前缀开头。
        // 注意：screener.market_cap 以 "screener.ma" 开头（starts_with
        // 前缀匹配），不能用作负面 key。
        for key in [
            "tab.chart",
            "sepa.thermometer",
            "screener.filter",
            "sepa.factor.ma_structure",
            "screener.list_years",
            "common.loading",
        ] {
            assert!(
                !is_allowed_zh_token(key),
                "key {key} 不在白名单内，is_allowed_zh_token 必须返回 false"
            );
        }
    }
}
