//! Adversarial tests for widget deviation fixes #226 / #227 / #228 / #230.
//!
//! These target the *contract gaps* the requirement tests do not cover:
//! rendered geometry (not just private fields), non-default token values
//! (the "coincidence trap": a hardcoded literal that happens to equal the
//! default token, so plain `==` assertions cannot distinguish a token read
//! from a lucky hardcode), component identity (the dropdown popup search
//! box must be rendered by the `Input` component, not a native `TextEdit`),
//! and — for #230 — the *rendered* WCAG contrast of button labels on their
//! fill (a contract the token-level tests cannot prove until the fix lands).
//!
//! Values like 43.0 / 27.0 for `control_md` / `control_sm` are deliberately
//! non-round: they cannot be produced by a coincidental hardcode of the
//! default scale (32.0 / 24.0).

use compass_ui::tokens::ThemeTokens;
use compass_ui::widgets::badge::Badge;
use compass_ui::widgets::button::{Button, ButtonVariant};
use compass_ui::widgets::dropdown::Dropdown;
use compass_ui::widgets::icon_button::IconButton;
use egui_kittest::kittest::{NodeT, Queryable};

const ICON: &str = "\u{E20C}";

fn badge_rect(tokens: &ThemeTokens, count: usize) -> egui::Rect {
    let rect = std::rc::Rc::new(std::cell::Cell::new(egui::Rect::ZERO));
    let r = rect.clone();
    let mut harness = egui_kittest::Harness::new_ui(move |ui| {
        r.set(Badge::new(tokens, count).show(ui).rect);
    });
    harness.run();
    rect.get()
}

fn icon_rect(tokens: &ThemeTokens, small: bool) -> egui::Rect {
    let mut harness = egui_kittest::Harness::new_ui(move |ui| {
        let btn = IconButton::new(tokens, ICON);
        if small {
            btn.small().show(ui);
        } else {
            btn.show(ui);
        }
    });
    harness.run();
    harness.get_by_label(ICON).rect()
}

fn themed_tokens(control_md: f32, control_sm: f32) -> ThemeTokens {
    let mut tokens = ThemeTokens::dark();
    tokens.spacing.control_md = control_md;
    tokens.spacing.control_sm = control_sm;
    tokens
}

// ---------------------------------------------------------------------------
// Issue #226 — IconButton default size must read `control_md`, not a literal.
// ---------------------------------------------------------------------------

/// Default button must render at `control_md` even when the token differs
/// from the old hardcoded 32.0 (coincidence trap: 32.0 == default token).
#[test]
fn default_size_follows_non_default_control_md_token() {
    let tokens = themed_tokens(43.0, 24.0);
    let rect = icon_rect(&tokens, false);
    assert!(
        rect.width() >= 43.0 && rect.height() >= 43.0,
        "default IconButton must render at the control_md token (43.0), \
         got {}x{} (hardcoded 32.0 would fail)",
        rect.width(),
        rect.height()
    );
}

/// `small()` must follow `control_sm` even when it differs from the default
/// 24.0 — guards a fix that hardcodes small() to 24.0 while fixing the default.
#[test]
fn small_size_follows_non_default_control_sm_token() {
    let tokens = themed_tokens(48.0, 27.0);
    let rect = icon_rect(&tokens, true);
    assert!(
        rect.width() >= 27.0 && rect.height() >= 27.0,
        "small IconButton must render at the control_sm token (27.0), \
         got {}x{}",
        rect.width(),
        rect.height()
    );
}

/// `.size()` override must still win over the token default.
#[test]
fn explicit_size_override_wins_over_token_default() {
    let tokens = themed_tokens(43.0, 27.0);
    let rect = {
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            IconButton::new(&tokens, ICON).size(50.0).show(ui);
        });
        harness.run();
        harness.get_by_label(ICON).rect()
    };
    assert!(
        rect.width() >= 50.0 && rect.height() >= 50.0,
        "explicit .size(50.0) must win over control_md=43.0, got {}x{}",
        rect.width(),
        rect.height()
    );
}

// ---------------------------------------------------------------------------
// Issue #227 — Badge min-width 16px.
// ---------------------------------------------------------------------------

/// Every single-digit count (the narrowest text) must still render at least
/// 16px wide. count=0 and count=1 are the tightest cases; a fix that only
/// special-cases one digit can slip past a single-count test.
#[test]
fn every_single_digit_count_meets_min_width_16() {
    let tokens = ThemeTokens::dark();
    for count in 0..=9 {
        let width = badge_rect(&tokens, count).width();
        assert!(
            width >= 16.0,
            "Badge count={count} must be at least 16px wide (min-width 16px spec), got {width}"
        );
    }
}

/// min-width is a floor, not a clamp: a large count must expand the pill
/// beyond a single digit (a naive fixed-16px implementation would truncate).
#[test]
fn large_count_expands_beyond_single_digit() {
    let tokens = ThemeTokens::dark();
    let single = badge_rect(&tokens, 1).width();
    let large = badge_rect(&tokens, 123_456_789).width();
    assert!(
        large >= 16.0,
        "large-count badge must still meet min-width, got {large}"
    );
    assert!(
        large > single,
        "large-count badge ({large}) must be wider than a single-digit badge ({single}); \
         min-width 16px is a floor, not a clamp"
    );
}

/// The largest possible count must render without panicking and expand.
#[test]
fn max_usize_badge_renders_without_panic() {
    let tokens = ThemeTokens::dark();
    let width = badge_rect(&tokens, usize::MAX).width();
    assert!(
        width >= 16.0,
        "usize::MAX badge must render at least 16px wide, got {width}"
    );
}

/// Min-width holds regardless of tone (tone changes colors, not geometry).
#[test]
fn min_width_holds_for_all_tones() {
    use compass_ui::widgets::badge::BadgeTone;
    let tokens = ThemeTokens::dark();
    for tone in [BadgeTone::Neutral, BadgeTone::Accent, BadgeTone::Error] {
        let rect = std::rc::Rc::new(std::cell::Cell::new(egui::Rect::ZERO));
        let r = rect.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            r.set(Badge::new(&tokens, 0).tone(tone).show(ui).rect);
        });
        harness.run();
        let width = rect.get().width();
        assert!(
            width >= 16.0,
            "Badge tone {tone:?} count=0 must be >= 16px wide, got {width}"
        );
    }
}

// ---------------------------------------------------------------------------
// Issue #228 — Dropdown popup search box must be the Input component.
// ---------------------------------------------------------------------------

fn search_field_id(options: &[&str]) -> Option<u128> {
    let tokens = ThemeTokens::dark();
    let opts = options.to_vec();
    let first = opts[0];
    let mut harness = egui_kittest::Harness::new_ui(move |ui| {
        Dropdown::new(&tokens, opts.clone())
            .searchable(true)
            .show(ui);
    });
    harness.run();
    harness.get_by_label_contains(first).click();
    harness.run();
    harness
        .query_all_by(|n| n.role() == egui::accesskit::Role::TextInput)
        .next()
        .map(|n| u128::from(n.accesskit_node().id()))
}

/// The popup search box must be the Input component (id salt "compass_input"),
/// not the legacy native `TextEdit` with id salt "search". The accesskit node
/// id is derived from the widget id salt, so a native TextEdit left in place
/// keeps its legacy id. The pinned constant is the legacy id measured from the
/// pre-fix implementation (native TextEdit, salt "search", probe 2026-08-09).
#[test]
fn popup_search_box_is_input_component_not_legacy_textedit() {
    const LEGACY_NATIVE_TEXTEDIT_ID: u128 = 79510412964553015796712359590912589824;
    let id = search_field_id(&["1d", "1w", "1M"])
        .expect("searchable popup must contain a TextInput search field");
    assert_ne!(
        id, LEGACY_NATIVE_TEXTEDIT_ID,
        "the popup search box still uses the legacy native TextEdit (id salt \"search\"); \
         it must be rendered by the Input component (id salt \"compass_input\")"
    );
}

/// A non-searchable popup must NOT contain a search TextInput.
#[test]
fn non_searchable_popup_has_no_search_input() {
    let tokens = ThemeTokens::dark();
    let mut harness = egui_kittest::Harness::new_ui(move |ui| {
        Dropdown::new(&tokens, ["1d", "1w", "1M"]).show(ui);
    });
    harness.run();
    harness.get_by_label_contains("1d").click();
    harness.run();
    assert!(
        harness
            .query_all_by(|n| n.role() == egui::accesskit::Role::TextInput)
            .next()
            .is_none(),
        "non-searchable popup must not render a search input"
    );
}

// ---------------------------------------------------------------------------
// Issue #230 — Button theme-aware text color: rendered WCAG contrast floor.
// ---------------------------------------------------------------------------

/// Render one button and return the color of its rendered text label.
fn button_text_color(tokens: &ThemeTokens, variant: ButtonVariant) -> egui::Color32 {
    let mut harness = egui_kittest::Harness::new_ui(move |ui| {
        Button::new(tokens, "Fetch").variant(variant).show(ui);
    });
    harness.run();
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| text_shape_color(&clipped.shape))
        .next()
        .expect("button must render a text label")
}

fn text_shape_color(shape: &egui::Shape) -> Option<egui::Color32> {
    match shape {
        egui::Shape::Vec(inner) => inner.iter().find_map(text_shape_color),
        egui::Shape::Text(text) => text.galley.job.sections.first().map(|s| s.format.color),
        _ => None,
    }
}

fn linearize_channel(c: u8) -> f32 {
    let v = f32::from(c) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(c: egui::Color32) -> f32 {
    0.2126 * linearize_channel(c.r())
        + 0.7152 * linearize_channel(c.g())
        + 0.0722 * linearize_channel(c.b())
}

fn contrast_ratio(fg: egui::Color32, bg: egui::Color32) -> f32 {
    let (l_fg, l_bg) = (relative_luminance(fg), relative_luminance(bg));
    let (hi, lo) = if l_fg >= l_bg {
        (l_fg, l_bg)
    } else {
        (l_bg, l_fg)
    };
    (hi + 0.05) / (lo + 0.05)
}

/// The rendered Primary label must clear WCAG AA on the accent fill in BOTH
/// themes. Today the label is text_primary — #1B2430 on light accent (3.19:1)
/// and #D1D4DC on dark accent (3.30:1) — both below AA. This test uses only
/// the pre-fix public API, so it compiles and fails by assertion TODAY; the
/// fix (white on_accent = 4.90:1) turns it green.
#[test]
fn rendered_primary_text_meets_wcag_contrast_on_accent_both_themes() {
    for tokens in [ThemeTokens::dark(), ThemeTokens::light()] {
        let text_color = button_text_color(&tokens, ButtonVariant::Primary);
        let ratio = contrast_ratio(text_color, tokens.color.accent);
        assert!(
            ratio >= 4.5,
            "rendered Primary label {text_color:?} on accent {:?} must be >= 4.5:1 (AA), \
             got {ratio:.2}:1",
            tokens.color.accent
        );
    }
}

/// The #230 fix must only touch Primary/Danger — Default and Ghost keep
/// rendering text_primary (浅底/透明底深字). A match refactor that
/// accidentally rewires them is caught here.
#[test]
fn default_and_ghost_keep_rendering_text_primary_both_themes() {
    for tokens in [ThemeTokens::dark(), ThemeTokens::light()] {
        for variant in [ButtonVariant::Default, ButtonVariant::Ghost] {
            let color = button_text_color(&tokens, variant);
            assert_eq!(
                color, tokens.color.text_primary,
                "{variant:?} label must keep text_primary in both themes, got {color:?}"
            );
        }
    }
}
