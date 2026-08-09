//! Unified button atom: variants, sizes, leading icon, loading and disabled
//! states (design doc §5.1 `Button`).
//!
//! Hover / press transitions are driven by egui's widget-state visuals: the
//! atom pushes a scoped style whose `widgets.{inactive,hovered,active}` fills
//! are the variant colors, so egui applies the state fill itself.

use crate::tokens::ThemeTokens;
use egui::{Color32, CornerRadius, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

/// Visual variant of a [`Button`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Neutral default button (panel-alt fill + border).
    Default,
    /// Accent-filled primary action.
    Primary,
    /// Error-filled destructive action.
    Danger,
    /// Transparent fill with a border.
    Ghost,
}

/// Size of a [`Button`] (controls the height via the spacing tokens).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    /// Small (24 px).
    Sm,
    /// Regular (32 px).
    Md,
    /// Large (40 px).
    Lg,
}

/// Unified button with compass variants, sizes, optional leading icon,
/// loading and disabled states.
pub struct Button<'a> {
    tokens: &'a ThemeTokens,
    text: String,
    variant: ButtonVariant,
    size: ButtonSize,
    icon: Option<&'a str>,
    loading: bool,
    disabled: bool,
    min_width: f32,
}

impl<'a> Button<'a> {
    /// Create a button for the given theme with a text label.
    pub fn new(tokens: &'a ThemeTokens, text: impl Into<String>) -> Self {
        Self {
            tokens,
            text: text.into(),
            variant: ButtonVariant::Default,
            size: ButtonSize::Md,
            icon: None,
            loading: false,
            disabled: false,
            min_width: 0.0,
        }
    }

    /// Set the visual variant (defaults to [`ButtonVariant::Default`]).
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the size (defaults to [`ButtonSize::Md`]).
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Set a minimum width in points. The button renders at least this wide,
    /// keeping the label steady when text changes between states (e.g. the
    /// SEPA refresh button "刷新" → "计算中…", issue #230). Text wider than
    /// the minimum still grows the button (min is a floor, not a clamp).
    pub fn min_width(mut self, min_width: f32) -> Self {
        self.min_width = min_width;
        self
    }

    /// Set a leading phosphor icon character.
    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Show an embedded spinner and ignore clicks (the button is dimmed).
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Disable the button (no clicks, disabled colors).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The height of this button from the spacing tokens.
    pub fn height(&self) -> f32 {
        match self.size {
            ButtonSize::Sm => self.tokens.spacing.control_sm,
            ButtonSize::Md => self.tokens.spacing.control_md,
            ButtonSize::Lg => self.tokens.spacing.control_lg,
        }
    }

    /// (variant, base fill, hover fill, pressed fill, text color).
    ///
    /// Default/Ghost use the theme's `text_primary` (light/transparent fills);
    /// Primary/Danger use the dedicated `on_accent`/`on_error` contrast tokens
    /// so labels stay legible on solid accent/error fills in both themes
    /// (light text_primary is dark — 3.19:1 on accent, below WCAG AA; white
    /// on_accent is 4.90:1, issue #230).
    fn variant_colors(&self) -> (Color32, Color32, Color32, Color32) {
        let c = &self.tokens.color;
        match self.variant {
            ButtonVariant::Default => (c.bg_panel_alt, c.bg_hover, c.bg_active, c.text_primary),
            ButtonVariant::Primary => (c.accent, c.accent_hover, c.accent_pressed, c.on_accent),
            ButtonVariant::Danger => (
                c.error,
                c.error.gamma_multiply(1.15),
                c.error.gamma_multiply(0.85),
                c.on_error,
            ),
            ButtonVariant::Ghost => (
                Color32::TRANSPARENT,
                c.bg_hover,
                c.bg_active,
                c.text_primary,
            ),
        }
    }

    /// Show the button and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        let tokens = self.tokens;
        let (fill, hover_fill, pressed_fill, text_color) = self.variant_colors();
        let disabled = self.disabled || self.loading;

        // Scoped widget-state style so egui picks variant fills per hover/press.
        let previous_style = ui.style().clone();
        let mut style = (*previous_style).clone();
        let w = &mut style.visuals.widgets;
        for state in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
        ] {
            state.bg_stroke = Stroke::new(
                1.0,
                match self.variant {
                    ButtonVariant::Primary | ButtonVariant::Danger => Color32::TRANSPARENT,
                    ButtonVariant::Default | ButtonVariant::Ghost => tokens.color.border,
                },
            );
            state.corner_radius = CornerRadius::from(tokens.radius.sm);
        }
        w.inactive.weak_bg_fill = fill;
        w.hovered.weak_bg_fill = hover_fill;
        w.active.weak_bg_fill = pressed_fill;
        w.noninteractive.weak_bg_fill = fill;

        // Loading keeps the variant's text color (e.g. white on Primary) —
        // the spinner + dimmed fill already signal the busy state; only a
        // true `disabled` button dims its label to text_disabled.
        let label_color = if self.disabled {
            tokens.color.text_disabled
        } else {
            text_color
        };
        let mut label = String::new();
        if let Some(icon) = self.icon {
            label.push_str(icon);
            label.push(' ');
        }
        label.push_str(&self.text);

        let mut button = egui::Button::new(
            RichText::new(label)
                .color(label_color)
                .size(tokens.typography.body),
        )
        .corner_radius(tokens.radius.sm)
        .min_size(Vec2::new(self.min_width, self.height()));
        if disabled {
            button = button.sense(Sense::hover());
        }

        ui.set_style(style);
        let response = ui.add(button);
        ui.set_style(previous_style);

        // Loading overlay: dim the button and draw a spinner at its right edge.
        if self.loading {
            let radius = CornerRadius::from(tokens.radius.sm);
            ui.painter()
                .rect_filled(response.rect, radius, Color32::from_black_alpha(102));
            let spinner_rect = Rect::from_center_size(
                Pos2::new(response.rect.right() - 14.0, response.rect.center().y),
                Vec2::splat(14.0),
            );
            ui.put(
                spinner_rect,
                egui::Spinner::new().size(14.0).color(label_color),
            );
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Variants follow the design: fills per variant; labels use
    /// on_accent/on_error on solid accent/error fills and text_primary on
    /// light/transparent fills (theme-switch aware, issue #230).
    #[test]
    fn variant_colors_follow_design() {
        let tokens = ThemeTokens::dark();
        let default = Button::new(&tokens, "x");
        assert_eq!(default.variant_colors().0, tokens.color.bg_panel_alt);

        let primary = Button::new(&tokens, "x").variant(ButtonVariant::Primary);
        assert_eq!(primary.variant_colors().0, tokens.color.accent);
        assert_eq!(primary.variant_colors().1, tokens.color.accent_hover);
        assert_eq!(primary.variant_colors().2, tokens.color.accent_pressed);
        assert_eq!(primary.variant_colors().3, tokens.color.on_accent);

        let danger = Button::new(&tokens, "x").variant(ButtonVariant::Danger);
        assert_eq!(danger.variant_colors().0, tokens.color.error);
        assert_eq!(danger.variant_colors().3, tokens.color.on_error);

        let ghost = Button::new(&tokens, "x").variant(ButtonVariant::Ghost);
        assert_eq!(ghost.variant_colors().0, Color32::TRANSPARENT);
        assert_eq!(ghost.variant_colors().3, tokens.color.text_primary);
    }

    /// Sizes map to the control spacing tokens.
    #[test]
    fn sizes_map_to_control_tokens() {
        let tokens = ThemeTokens::dark();
        assert_eq!(
            Button::new(&tokens, "x").size(ButtonSize::Sm).height(),
            tokens.spacing.control_sm
        );
        assert_eq!(
            Button::new(&tokens, "x").size(ButtonSize::Md).height(),
            tokens.spacing.control_md
        );
        assert_eq!(
            Button::new(&tokens, "x").size(ButtonSize::Lg).height(),
            tokens.spacing.control_lg
        );
    }

    /// Clicking a primary button fires once per click.
    #[test]
    fn primary_button_click_fires() {
        let tokens = ThemeTokens::dark();
        let clicked = Rc::new(Cell::new(0u32));
        let c = clicked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            if Button::new(&tokens, "Go")
                .variant(ButtonVariant::Primary)
                .show(ui)
                .clicked()
            {
                c.set(c.get() + 1);
            }
        });
        harness.run();
        harness.get_by_label("Go").click();
        harness.run();
        assert_eq!(clicked.get(), 1);
    }

    /// A loading button ignores clicks (no spinner-embedded label to query,
    /// but the original label stays present and non-interactive).
    #[test]
    fn loading_button_ignores_clicks() {
        let tokens = ThemeTokens::dark();
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            if Button::new(&tokens, "Fetch")
                .variant(ButtonVariant::Primary)
                .loading(true)
                .show(ui)
                .clicked()
            {
                c.set(true);
            }
        });
        harness.step();
        harness.get_by_label("Fetch").click();
        harness.step();
        assert!(!clicked.get(), "loading button must not fire clicks");
    }

    /// A disabled button ignores clicks.
    #[test]
    fn disabled_button_ignores_clicks() {
        let tokens = ThemeTokens::dark();
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            if Button::new(&tokens, "No").disabled(true).show(ui).clicked() {
                c.set(true);
            }
        });
        harness.run();
        harness.get_by_label("No").click();
        harness.run();
        assert!(!clicked.get(), "disabled button must not fire clicks");
    }

    /// Loading keeps the variant's text color (on_accent for Primary) — the
    /// spinner and dimmed fill already signal the busy state; the label must
    /// not fade to text_disabled (user acceptance: "加载中的字体颜色不对").
    #[test]
    fn loading_button_keeps_variant_text_color() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Button::new(&tokens, "Fetch")
                .variant(ButtonVariant::Primary)
                .loading(true)
                .show(ui);
        });
        harness.step();

        let text_colors: Vec<Color32> = harness
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| text_shape_color(&clipped.shape))
            .collect();
        assert!(
            text_colors.contains(&tokens.color.on_accent),
            "loading Primary button must render the on_accent label, got {text_colors:?}"
        );
        assert!(
            !text_colors.contains(&tokens.color.text_disabled),
            "loading must not dim the label to text_disabled, got {text_colors:?}"
        );
    }

    /// A truly disabled button dims its label to text_disabled.
    #[test]
    fn disabled_button_dims_label() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Button::new(&tokens, "No").disabled(true).show(ui);
        });
        harness.step();

        let text_colors: Vec<Color32> = harness
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| text_shape_color(&clipped.shape))
            .collect();
        assert!(
            text_colors.contains(&tokens.color.text_disabled),
            "disabled button must dim the label to text_disabled, got {text_colors:?}"
        );
    }

    /// Recursively collect the color of every text shape.
    fn text_shape_color(shape: &egui::Shape) -> Option<Color32> {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().find_map(text_shape_color),
            egui::Shape::Text(text) => text.galley.job.sections.first().map(|s| s.format.color),
            _ => None,
        }
    }

    /// The leading icon is part of the rendered label.
    #[test]
    fn icon_is_rendered_in_label() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Button::new(&tokens, "Fetch").icon("\u{E20C}").show(ui);
        });
        harness.run();
        let _ = harness.get_by_label_contains("Fetch");
    }

    /// Issue #230: Primary text follows the theme `on_accent` token (Material
    /// on-* semantics) in both palettes — light text_primary (#1B2430) on
    /// accent (#2962FF) is only 3.19:1, below WCAG AA 4.5:1; white is 4.90:1.
    #[test]
    fn primary_variant_text_follows_on_accent_token() {
        for tokens in [ThemeTokens::dark(), ThemeTokens::light()] {
            let primary = Button::new(&tokens, "x").variant(ButtonVariant::Primary);
            assert_eq!(
                primary.variant_colors().3,
                tokens.color.on_accent,
                "Primary label must be the theme on_accent token (got text_primary)"
            );
        }
    }

    /// Issue #230: Danger text follows the theme `on_error` token in both
    /// palettes — light text_primary on error (#D93025) is 3.28:1, below AA.
    #[test]
    fn danger_variant_text_follows_on_error_token() {
        for tokens in [ThemeTokens::dark(), ThemeTokens::light()] {
            let danger = Button::new(&tokens, "x").variant(ButtonVariant::Danger);
            assert_eq!(
                danger.variant_colors().3,
                tokens.color.on_error,
                "Danger label must be the theme on_error token (got text_primary)"
            );
        }
    }

    /// Issue #230: Default/Ghost keep `text_primary` (light/transparent fills,
    /// where dark-on-light text is already readable) — no regression.
    #[test]
    fn default_ghost_variants_keep_text_primary() {
        for tokens in [ThemeTokens::dark(), ThemeTokens::light()] {
            let default = Button::new(&tokens, "x");
            assert_eq!(default.variant_colors().3, tokens.color.text_primary);
            let ghost = Button::new(&tokens, "x").variant(ButtonVariant::Ghost);
            assert_eq!(ghost.variant_colors().3, tokens.color.text_primary);
        }
    }

    /// Issue #230: a loading Primary button still renders the variant text
    /// color (now on_accent) — only a true disabled button dims to
    /// text_disabled. The label must never fade while loading.
    #[test]
    fn loading_button_keeps_on_accent_variant_text() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Button::new(&tokens, "Fetch")
                .variant(ButtonVariant::Primary)
                .loading(true)
                .show(ui);
        });
        harness.step();

        let text_colors: Vec<Color32> = harness
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| text_shape_color(&clipped.shape))
            .collect();
        assert!(
            text_colors.contains(&tokens.color.on_accent),
            "loading Primary button must render the on_accent label, got {text_colors:?}"
        );
        assert!(
            !text_colors.contains(&tokens.color.text_disabled),
            "loading must not dim the label to text_disabled, got {text_colors:?}"
        );
    }

    /// Issue #230: `.min_width(96)` renders the button at least 96px wide —
    /// the SEPA refresh button must not shrink below the two-state width.
    #[test]
    fn min_width_sets_floor_on_rendered_width() {
        let tokens = ThemeTokens::dark();
        let rect = Rc::new(Cell::new(egui::Rect::ZERO));
        let r = rect.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            r.set(Button::new(&tokens, "刷新").min_width(96.0).show(ui).rect);
        });
        harness.run();
        assert!(
            rect.get().width() >= 96.0,
            "button with min_width(96) must render >= 96px wide, got {}",
            rect.get().width()
        );
    }

    /// Issue #230: SEPA refresh button — idle「刷新」and loading「计算中…」
    /// render the same width under min_width(96) (±1px tolerance). Loading
    /// uses `harness.step()` so the spinner's repaint requests do not make
    /// `run()` spin forever.
    #[test]
    fn sepa_refresh_idle_loading_width_stable_with_min_width() {
        let tokens = ThemeTokens::dark();
        let idle = Rc::new(Cell::new(0.0f32));
        let loading = Rc::new(Cell::new(0.0f32));

        let idle_w = idle.clone();
        let mut idle_harness = egui_kittest::Harness::new_ui(move |ui| {
            idle_w.set(
                Button::new(&tokens, "刷新")
                    .variant(ButtonVariant::Primary)
                    .min_width(96.0)
                    .show(ui)
                    .rect
                    .width(),
            );
        });
        idle_harness.run();

        let loading_w = loading.clone();
        let mut loading_harness = egui_kittest::Harness::new_ui(move |ui| {
            loading_w.set(
                Button::new(&tokens, "计算中…")
                    .variant(ButtonVariant::Primary)
                    .min_width(96.0)
                    .loading(true)
                    .show(ui)
                    .rect
                    .width(),
            );
        });
        loading_harness.step();

        assert!(
            (idle.get() - loading.get()).abs() <= 1.0,
            "idle/loading widths must match under min_width(96): idle={}, loading={}",
            idle.get(),
            loading.get()
        );
    }

    /// Issue #230: the default min_width=0 keeps the text-driven width —
    /// a short label renders narrower than a long one (no regression).
    #[test]
    fn default_min_width_zero_keeps_text_driven_width() {
        let tokens = ThemeTokens::dark();
        let short = Rc::new(Cell::new(0.0f32));
        let long = Rc::new(Cell::new(0.0f32));

        let s = short.clone();
        let mut short_harness = egui_kittest::Harness::new_ui(move |ui| {
            s.set(Button::new(&tokens, "A").show(ui).rect.width());
        });
        short_harness.run();

        let l = long.clone();
        let mut long_harness = egui_kittest::Harness::new_ui(move |ui| {
            l.set(Button::new(&tokens, "计算中…").show(ui).rect.width());
        });
        long_harness.run();

        assert!(
            short.get() < long.get(),
            "without min_width the width must follow the text: short={}, long={}",
            short.get(),
            long.get()
        );
    }

    /// Issue #230: min_width is a floor, not a clamp — a label wider than
    /// min_width must still expand the button beyond it.
    #[test]
    fn text_wider_than_min_width_grows_naturally() {
        let tokens = ThemeTokens::dark();
        let rect = Rc::new(Cell::new(egui::Rect::ZERO));
        let r = rect.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            r.set(
                Button::new(&tokens, "这是一个非常长的按钮文本用于验证自然增长")
                    .min_width(96.0)
                    .show(ui)
                    .rect,
            );
        });
        harness.run();
        assert!(
            rect.get().width() > 96.0,
            "long text must grow beyond min_width(96), got {}",
            rect.get().width()
        );
    }
}
