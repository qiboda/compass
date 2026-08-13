//! Adversarial tests: BK prefix search/normalization in SearchableDropdown
//! (epic #255 plan T4 / C3-GUI, plan T7 toolbar merge).
//!
//! Plan contract under attack:
//! - `normalize_query` / `strip_exchange_prefix` recognize the `bk` prefix so
//!   the pure code "0475" matches BK0475 (plan T4: "0475" 匹配 BK0475)
//! - the toolbar search "000001" surfaces BOTH SZ000001 (平安银行) and
//!   SH000001 (上证指数) when the merged list contains both (plan T7)
//! - `format_display` must not duplicate the prefix ("BK | 0475 | 半导体",
//!   never "BK | BK0475 | 半导体") — plan T4 Must NOT do
//!
//! Why `tests/`: the sandbox denies writes to `src/**`; the private
//! `normalize_query` / `strip_exchange_prefix` / `format_display` are covered
//! through the public `filter_stocks` entry point.
//!
//! RED vs current code: `strip_exchange_prefix` does not know `bk`, so
//! query "0475" cannot match BK0475 (q_code stays "0475" but the symbol's
//! bare form "BK0475" does not start with it) and the display text would
//! render "BK | BK0475 | 半导体".

use compass_ui::widgets::searchable_dropdown::{StockProjection, filter_stocks};

#[derive(Clone)]
struct TestStock {
    symbol: String,
    name: String,
    exchange: Option<String>,
}

impl TestStock {
    fn new(symbol: &str, name: &str, exchange: &str) -> Self {
        Self {
            symbol: symbol.into(),
            name: name.into(),
            exchange: Some(exchange.into()),
        }
    }
}

fn projection() -> StockProjection<TestStock> {
    StockProjection::new(
        |s: &TestStock| &s.symbol,
        |s: &TestStock| &s.name,
        |s: &TestStock| s.exchange.as_deref(),
    )
}

/// Merged toolbar list (stock + index_basic, plan T7): the same bare code
/// 000001 exists on both exchanges and a board row carries a BK prefix.
fn merged_list() -> Vec<TestStock> {
    vec![
        TestStock::new("SZ000001", "平安银行", "SZ"),
        TestStock::new("SH000001", "上证指数", "SH"),
        TestStock::new("BK0475", "半导体", "BK"),
        TestStock::new("SH600519", "贵州茅台", "SH"),
    ]
}

#[test]
fn filter_stocks_bk_bare_code_match() {
    // RED: query "0475" must find BK0475 via the bk-prefixed bare code.
    // Currently strip_exchange_prefix("bk0475") leaves "bk0475", which does
    // not start with "0475" → empty result.
    let stocks = merged_list();
    let result = filter_stocks(&stocks, "0475", None, &projection());
    let symbols: Vec<&str> = result.iter().map(|s| s.symbol.as_str()).collect();
    assert!(
        symbols.contains(&"BK0475"),
        "query '0475' must match BK0475; got {symbols:?}"
    );
}

#[test]
fn filter_stocks_bk_prefixed_case_variants() {
    // Guard: prefixed spellings (upper/lower) already match today via the
    // lowercase symbol.starts_with path — must keep working after the bk
    // branch lands.
    let stocks = merged_list();
    for query in ["BK0475", "bk0475"] {
        let result = filter_stocks(&stocks, query, None, &projection());
        let symbols: Vec<&str> = result.iter().map(|s| s.symbol.as_str()).collect();
        assert!(
            symbols.contains(&"BK0475"),
            "query {query:?} must match BK0475; got {symbols:?}"
        );
    }
}

#[test]
fn filter_stocks_bk_exchange_filter() {
    // Guard: exchange=Some("BK") must narrow to board rows only.
    let stocks = merged_list();
    let result = filter_stocks(&stocks, "", Some("BK"), &projection());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].symbol, "BK0475");
}

#[test]
fn filter_stocks_bare_code_000001_matches_both_exchanges() {
    // Plan T7 acceptance: typing "000001" surfaces SZ000001 (平安银行) AND
    // SH000001 (上证指数) as two rows.
    let stocks = merged_list();
    let result = filter_stocks(&stocks, "000001", None, &projection());
    let symbols: Vec<&str> = result.iter().map(|s| s.symbol.as_str()).collect();
    assert!(
        symbols.contains(&"SZ000001"),
        "query '000001' must surface the stock; got {symbols:?}"
    );
    assert!(
        symbols.contains(&"SH000001"),
        "query '000001' must surface the index; got {symbols:?}"
    );
}

#[test]
fn filter_stocks_bk_empty_query_returns_all() {
    let stocks = merged_list();
    let result = filter_stocks(&stocks, "", None, &projection());
    assert_eq!(result.len(), 4, "empty query matches every merged row");
}

#[test]
fn filter_stocks_large_list_bk_match_no_regression() {
    // Performance/correctness guard: a ~6500-row merged list (plan T7 D11
    // O(n) refilter) must still surface BK0475 for "0475" — proving the BK
    // branch does not break filtering at scale.
    let mut stocks: Vec<TestStock> = (0..6500)
        .map(|i| TestStock::new(&format!("SZ{:06}", 100000 + i), "stub", "SZ"))
        .collect();
    stocks.push(TestStock::new("BK0475", "半导体", "BK"));

    let result = filter_stocks(&stocks, "0475", None, &projection());
    let symbols: Vec<&str> = result.iter().map(|s| s.symbol.as_str()).collect();
    assert!(
        symbols.contains(&"BK0475"),
        "6500-row list must still match BK0475 for '0475'"
    );
}
