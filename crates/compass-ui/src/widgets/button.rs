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
    fn variant_colors(&self) -> (Color32, Color32, Color32, Color32) {
        let c = &self.tokens.color;
        match self.variant {
            ButtonVariant::Default => (c.bg_panel_alt, c.bg_hover, c.bg_active, c.text_primary),
            ButtonVariant::Primary => (c.accent, c.accent_hover, c.accent_pressed, Color32::WHITE),
            ButtonVariant::Danger => (
                c.error,
                c.error.gamma_multiply(1.15),
                c.error.gamma_multiply(0.85),
                Color32::WHITE,
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
        .min_size(Vec2::new(0.0, self.height()));
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

    /// Default variant uses the panel-alt fill; primary uses the accent.
    #[test]
    fn variant_colors_follow_design() {
        let tokens = ThemeTokens::dark();
        let default = Button::new(&tokens, "x");
        assert_eq!(default.variant_colors().0, tokens.color.bg_panel_alt);

        let primary = Button::new(&tokens, "x").variant(ButtonVariant::Primary);
        assert_eq!(primary.variant_colors().0, tokens.color.accent);
        assert_eq!(primary.variant_colors().1, tokens.color.accent_hover);
        assert_eq!(primary.variant_colors().2, tokens.color.accent_pressed);

        let danger = Button::new(&tokens, "x").variant(ButtonVariant::Danger);
        assert_eq!(danger.variant_colors().0, tokens.color.error);

        let ghost = Button::new(&tokens, "x").variant(ButtonVariant::Ghost);
        assert_eq!(ghost.variant_colors().0, Color32::TRANSPARENT);
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

    /// Loading keeps the variant text color (white on Primary) — the spinner
    /// and dimmed fill signal the busy state; the label must not fade to
    /// text_disabled (user acceptance: "加载中的字体颜色不对").
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
            text_colors.contains(&Color32::WHITE),
            "loading Primary button must render white label text, got {text_colors:?}"
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
}
