//! Adversarial tests for issue #230 — Button theme-aware text color tokens
//! (`on_accent` / `on_error`) and the `min_width` loading-width fix.
//!
//! These target the interface contract declared in the approved plan
//! (`.dsh/designs/button-theme-and-width-fix.md`): the `on_accent` /
//! `on_error` fields on `ColorTokens` and the `Button::min_width` builder.
//! They do not compile until the implementation lands — that compile failure
//! is the test-first RED. Once the fix is in, the same suite must compile and
//! pass (GREEN).
//!
//! The contrast helpers implement WCAG 2.1 relative luminance (sRGB
//! linearization) — the exact formula the design doc used for its 4.90:1 /
//! 4.77:1 targets.

use compass_ui::tokens::ThemeTokens;
use compass_ui::widgets::button::{Button, ButtonVariant};

// ---------------------------------------------------------------------------
// on_accent / on_error token contract
// ---------------------------------------------------------------------------

/// Both palettes must define real, opaque, non-placeholder on_* tokens. A
/// lazy implementation that aliases them to `text_primary` (the pre-#230
/// behavior) or leaves them transparent defeats the legibility fix.
#[test]
fn on_accent_and_on_error_defined_in_both_palettes() {
    for palette in [ThemeTokens::dark().color, ThemeTokens::light().color] {
        for (name, token) in [
            ("on_accent", palette.on_accent),
            ("on_error", palette.on_error),
        ] {
            assert_eq!(
                token.a(),
                255,
                "{name} must be opaque in both palettes, got {token:?}"
            );
            assert_ne!(
                token, palette.text_primary,
                "{name} must not alias text_primary (would regress #230 contrast), got {token:?}"
            );
        }
    }
}

/// The on_* tokens must clear a WCAG contrast floor on their fill. Light
/// theme hits AA (>= 4.5:1) on accent and error; dark accent also clears AA;
/// dark error is a user-accepted sub-AA tradeoff (3.48:1) but must still
/// beat the old text_primary (2.35:1) — floor at 3.0.
#[test]
fn on_tokens_meet_contrast_contract_on_fills() {
    let light = ThemeTokens::light().color;
    assert!(
        contrast_ratio(light.on_accent, light.accent) >= 4.5,
        "light on_accent on accent must be >= 4.5:1, got {:.2}:1",
        contrast_ratio(light.on_accent, light.accent)
    );
    assert!(
        contrast_ratio(light.on_error, light.error) >= 4.5,
        "light on_error on error must be >= 4.5:1, got {:.2}:1",
        contrast_ratio(light.on_error, light.error)
    );
    let dark = ThemeTokens::dark().color;
    assert!(
        contrast_ratio(dark.on_accent, dark.accent) >= 4.5,
        "dark on_accent on accent must be >= 4.5:1, got {:.2}:1",
        contrast_ratio(dark.on_accent, dark.accent)
    );
    assert!(
        contrast_ratio(dark.on_error, dark.error) > 3.0,
        "dark on_error on error must beat old text_primary (2.35:1, design 3.48:1), got {:.2}:1",
        contrast_ratio(dark.on_error, dark.error)
    );
}

// ---------------------------------------------------------------------------
// variant -> rendered text color contract
// ---------------------------------------------------------------------------

fn rendered_text_color(tokens: &ThemeTokens, variant: ButtonVariant, text: &str) -> egui::Color32 {
    let text = text.to_owned();
    let mut harness = egui_kittest::Harness::new_ui(move |ui| {
        Button::new(tokens, text.as_str()).variant(variant).show(ui);
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

/// Primary must render on_accent, Danger on_error, in BOTH themes — a fix
/// that only touches one variant or one theme fails here.
#[test]
fn primary_and_danger_render_on_tokens_both_themes() {
    for tokens in [ThemeTokens::dark(), ThemeTokens::light()] {
        let primary_color = rendered_text_color(&tokens, ButtonVariant::Primary, "Fetch");
        assert_eq!(
            primary_color, tokens.color.on_accent,
            "Primary label must render on_accent in both themes"
        );
        let danger_color = rendered_text_color(&tokens, ButtonVariant::Danger, "Delete");
        assert_eq!(
            danger_color, tokens.color.on_error,
            "Danger label must render on_error in both themes"
        );
    }
}

// ---------------------------------------------------------------------------
// min_width geometry contract
// ---------------------------------------------------------------------------

/// Render one button and return its laid-out width.
///
/// Loading buttons contain a Spinner that repaints forever, so `harness.run()`
/// would never terminate — loading frames use `step()` instead.
fn rendered_width(tokens: &ThemeTokens, text: &str, min_width: Option<f32>, loading: bool) -> f32 {
    let width = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let w = width.clone();
    let text = text.to_owned();
    let mut harness = egui_kittest::Harness::new_ui(move |ui| {
        let mut button = Button::new(tokens, text.as_str());
        if let Some(mw) = min_width {
            button = button.min_width(mw);
        }
        w.set(button.loading(loading).show(ui).rect.width());
    });
    if loading {
        harness.step();
    } else {
        harness.run();
    }
    width.get()
}

/// `.min_width()` must reach the rendered layout — a builder that stores the
/// value but never feeds `min_size` leaves「刷新」at its text width (~34px),
/// far below the 96px floor.
#[test]
fn min_width_applies_to_rendered_rect() {
    let tokens = ThemeTokens::dark();
    let width = rendered_width(&tokens, "刷新", Some(96.0), false);
    assert!(
        width >= 96.0,
        ".min_width(96.0) button rendered at {width}px — min_width not applied to layout"
    );
}

/// min_width is a FLOOR, not a clamp: text longer than the min must still
/// grow the button (a naive clamp would truncate long labels).
#[test]
fn min_width_is_floor_not_clamp() {
    let tokens = ThemeTokens::dark();
    let short = rendered_width(&tokens, "刷新", Some(96.0), false);
    let long = rendered_width(&tokens, "重新计算并刷新全部数据", Some(96.0), false);
    assert!(
        short >= 96.0,
        "min_width floor not applied: short text at {short}px"
    );
    assert!(
        long > short,
        "long text must grow past min_width (floor, not clamp): long {long}px vs short {short}px"
    );
}

/// The user-reported jump — idle「刷新」~33.7px vs loading「计算中…」~59.0px
/// (root-cause measurement) — must disappear once min_width covers the wider
/// label: both states render the same width within 1px.
#[test]
fn loading_width_stable_with_min_width() {
    let tokens = ThemeTokens::dark();
    let idle = rendered_width(&tokens, "刷新", Some(96.0), false);
    let loading_w = rendered_width(&tokens, "计算中…", Some(96.0), true);
    assert!(
        (idle - loading_w).abs() <= 1.0,
        "idle「刷新」{idle}px vs loading「计算中…」{loading_w}px must match within 1px \
         under .min_width(96.0)"
    );
}

/// min_width defaults to 0 — buttons that never call `.min_width()` keep
/// their natural text-driven width (no hidden floor, no regression for the
/// dozens of untouched call sites).
#[test]
fn default_min_width_zero_keeps_text_width() {
    let tokens = ThemeTokens::dark();
    let plain = rendered_width(&tokens, "刷新", None, false);
    let minned = rendered_width(&tokens, "刷新", Some(96.0), false);
    assert!(
        plain < minned,
        "default button must stay text-width ({plain}px) and stay narrower than the \
         96px-min button ({minned}px)"
    );
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

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
