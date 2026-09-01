//! Requirement-acceptance contract tests for the C4 market tab (epic #255,
//! plan T6 / T7).
//!
//! The full kittest rendering tests (三 tab 渲染 / Segmented 切换 / 行点击
//! 联动 / 空态) cannot compile until `TabKind::Market` and
//! `citizens/market.rs` land — the compass crate is a pure-bin crate and
//! both symbols do not exist yet. These source-contract tests stay
//! compile-green TODAY (no reference to the missing symbols) and assert the
//! plan-declared surface, mirroring the contract-grep style already used in
//! `collectors/tests/test_index_main_cli.py`:
//! - `TabKind` gains a `Market` variant (plan T6: tabs.rs 加 Market 变体)
//! - `citizens/market.rs` exists and embeds the 6-index whitelist
//!   (SH000001/SZ399001/SZ399006/SH000300/SH000905/SH000852, plan T6)
//! - i18n keys `tab.market` + the `index.*` namespace exist symmetrically in
//!   zh.yml / en.yml (plan T6: index.* i18n zh/en 对称)
//! - the toolbar adjust Tag (前复权) is hidden for index/board symbols
//!   (plan T7) — asserted via the source guard that gates the Tag
//!
//! RED vs current code: no Market variant, no market citizen, no i18n keys,
//! and the adjust Tag renders unconditionally — all assertions below fail.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_rel(rel: &str) -> Option<String> {
    std::fs::read_to_string(crate_root().join(rel)).ok()
}

const WHITELIST: [&str; 6] = [
    "SH000001", "SZ399001", "SZ399006", "SH000300", "SH000905", "SH000852",
];

#[test]
fn tab_kind_gains_market_variant() {
    // Plan T6: tabs.rs TabKind 加 Market 变体（title "tab.market"、icon
    // TRENDING_UP、citizen_id "market"）.
    let src = read_rel("src/tabs.rs").expect("tabs.rs must exist");
    assert!(
        src.contains("Market"),
        "TabKind must gain a Market variant (plan T6)"
    );
    assert!(
        src.contains("tab.market"),
        "TabKind::Market title must be the i18n key 'tab.market'"
    );
}

#[test]
fn market_citizen_embeds_six_index_whitelist() {
    // Plan T6: 核心指数 Card 6 只白名单（SH000001/SZ399001/SZ399006/
    // SH000300/SH000905/SH000852）.
    let src =
        read_rel("src/citizens/market.rs").expect("citizens/market.rs must exist once T6 lands");
    for sym in WHITELIST {
        assert!(
            src.contains(sym),
            "market citizen must embed whitelist symbol {sym}"
        );
    }
}

#[test]
fn market_citizen_sorts_boards_by_change_desc() {
    // Plan T6: 板块 DataTable 默认涨跌幅降序 — the market citizen must sort
    // its board rows by change percent descending (not by symbol/name).
    let src =
        read_rel("src/citizens/market.rs").expect("citizens/market.rs must exist once T6 lands");
    assert!(
        src.to_lowercase().contains("sort") && src.contains("desc"),
        "board table must be sorted by change percent descending (plan T6)"
    );
}

#[test]
fn i18n_market_keys_zh_en_symmetric() {
    // Plan T6: index.* i18n 命名空间 zh/en 对称.
    let zh = read_rel("../compass-i18n/locales/zh.yml").expect("compass-i18n zh.yml must exist");
    let en = read_rel("../compass-i18n/locales/en.yml").expect("compass-i18n en.yml must exist");
    assert!(zh.contains("tab.market:"), "zh.yml must define tab.market");
    assert!(en.contains("tab.market:"), "en.yml must define tab.market");
    assert!(
        zh.lines().any(|l| l.trim_start().starts_with("index:")),
        "zh.yml must define the index.* namespace"
    );
    assert!(
        en.lines().any(|l| l.trim_start().starts_with("index:")),
        "en.yml must define the index.* namespace"
    );
}

#[test]
fn toolbar_adjust_dropdown_has_three_options_and_index_hide_guard() {
    // Plan T7 + issue #345: 前复权 Tag → three-option Dropdown (qfq/hfq/none)
    // — the toolbar must reference all three i18n option keys via a Dropdown
    // with its own `id_salt("adjust")`, and the index/board hide guard must
    // be preserved (当前标的 index_type 非空或 BK 前缀时隐藏).
    let src = read_rel("src/main.rs").expect("main.rs must exist");
    // Three i18n option keys are rendered as dropdown options.
    for key in [
        "toolbar.adjust.qfq",
        "toolbar.adjust.hfq",
        "toolbar.adjust.none",
    ] {
        assert!(
            src.contains(key),
            "toolbar adjust dropdown must reference the option key {key}"
        );
    }
    // The control is a Dropdown (not a Tag) with its own id salt "adjust".
    assert!(
        src.contains("id_salt(\"adjust\")"),
        "the adjust control must be a Dropdown with id_salt(\"adjust\")"
    );
    // The hide guard must remain (plan T7).
    assert!(
        src.contains("index_type") || src.contains("BK"),
        "the adjust Dropdown must be hidden when the symbol is an index/board \
         (plan T7: 当前标的 index_type 非空或 BK 前缀时隐藏)"
    );
}
