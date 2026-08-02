//! Tooltip atom: unified hover tooltip wrapper with configurable delay
//! (design doc §5.1 `Tooltip`).

use crate::tokens::ThemeTokens;
use egui::{Response, Ui, WidgetText};

/// Wraps `on_hover_text` / `on_hover_ui` with the compass tooltip delay
/// (0.4 s by default). Visual styling (panel-alt fill, small radius, caption
/// font) is applied by the theme layer.
pub struct Tooltip<'a> {
    tokens: &'a ThemeTokens,
    delay: f32,
}

impl<'a> Tooltip<'a> {
    /// Create a tooltip wrapper for the given theme.
    pub fn new(tokens: &'a ThemeTokens) -> Self {
        Self { tokens, delay: 0.4 }
    }

    /// Set the hover delay in seconds (default 0.4).
    pub fn delay(mut self, delay: f32) -> Self {
        self.delay = delay;
        self
    }

    /// The configured delay in seconds.
    pub fn delay_secs(&self) -> f32 {
        self.delay
    }

    /// Attach a plain-text tooltip to the response.
    pub fn text(self, response: Response, text: impl Into<WidgetText>) -> Response {
        let delay = self.delay;
        let ctx = response.ctx.clone();
        let previous = ctx.global_style().interaction.tooltip_delay;
        ctx.global_style_mut(|style| style.interaction.tooltip_delay = delay);
        let out = response.on_hover_text(text);
        ctx.global_style_mut(|style| style.interaction.tooltip_delay = previous);
        out
    }

    /// Attach a rich custom tooltip UI to the response.
    pub fn show_ui(self, response: Response, add_contents: impl FnOnce(&mut Ui)) -> Response {
        let delay = self.delay;
        let ctx = response.ctx.clone();
        let previous = ctx.global_style().interaction.tooltip_delay;
        ctx.global_style_mut(|style| style.interaction.tooltip_delay = delay);
        let out = response.on_hover_ui(add_contents);
        ctx.global_style_mut(|style| style.interaction.tooltip_delay = previous);
        out
    }

    /// The theme tokens held by this wrapper (styling hooks for future use).
    pub fn tokens(&self) -> &'a ThemeTokens {
        self.tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;

    /// The default delay follows the design spec (0.4 s).
    #[test]
    fn default_delay_is_design_spec() {
        let tokens = ThemeTokens::dark();
        assert_eq!(Tooltip::new(&tokens).delay_secs(), 0.4);
    }

    /// Delay is configurable.
    #[test]
    fn delay_is_configurable() {
        let tokens = ThemeTokens::dark();
        assert_eq!(Tooltip::new(&tokens).delay(0.8).delay_secs(), 0.8);
    }

    /// Hovering a widget with a tooltip renders without panic.
    #[test]
    fn tooltip_renders() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let button = ui.button("Hover me");
            Tooltip::new(&tokens).text(button, "Helpful hint");
        });
        harness.run();
    }
}
