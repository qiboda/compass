//! Font registration: SourceHanSansCN (Chinese) + JetBrains Mono (numeric).
//!
//! Implemented by sub-issue #124 (S2).
//!
//! All three fonts are embedded into the binary via [`include_bytes!`] (design
//! doc `.dsh/designs/gui-upgrade.md` §3.4): no runtime path probing, so the
//! fonts work on any machine without the system fonts installed.

use std::sync::Arc;

/// Font key for Source Han Sans CN (regular weight), the primary CJK font.
///
/// First in the proportional family so all Chinese text falls back to it.
pub const FONT_SOURCE_HAN: &str = "SourceHanSansCN";

/// Font key for Source Han Sans CN (bold weight), used for headings and primary buttons.
pub const FONT_SOURCE_HAN_BOLD: &str = "SourceHanSansCN-Bold";

/// Font key for JetBrains Mono (regular weight), the monospace font for
/// numbers, symbols and codes (column alignment is a hard requirement for
/// financial UIs; egui has no OpenType tabular-nums support, so the monospace
/// family is the equivalent).
pub const FONT_JETBRAINS_MONO: &str = "JetBrainsMono";

/// Font key registered by [`egui_phosphor::add_to_fonts`] for the icon font.
pub const PHOSPHOR_FONT_KEY: &str = "phosphor";

/// Build the [`egui::FontDefinitions`] for the whole application.
///
/// Registers the three embedded fonts plus the egui-phosphor icon font.
/// Family insertion order is the glyph fallback priority: proportional is
/// led by Source Han Sans CN (regular, then bold, then the built-in defaults),
/// monospace by JetBrains Mono with Source Han Sans CN as CJK fallback.
fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        FONT_SOURCE_HAN.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/SourceHanSansCN-Regular.otf"
        ))),
    );
    fonts.font_data.insert(
        FONT_SOURCE_HAN_BOLD.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/SourceHanSansCN-Bold.otf"
        ))),
    );
    fonts.font_data.insert(
        FONT_JETBRAINS_MONO.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, FONT_SOURCE_HAN.to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(1, FONT_SOURCE_HAN_BOLD.to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, FONT_JETBRAINS_MONO.to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(1, FONT_SOURCE_HAN.to_owned());

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    fonts
}

/// Install the embedded font set on the given [`egui::Context`].
///
/// Call once during startup, before any UI is drawn. Replaces the default
/// font definitions; the egui built-in fonts remain as fallback at the tail
/// of each family.
pub fn setup_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three embedded fonts plus the egui-phosphor icon font must all be
    /// registered in `font_data`, keyed by their canonical names.
    #[test]
    fn font_definitions_register_all_embedded_fonts() {
        let defs = font_definitions();

        assert!(defs.font_data.contains_key(FONT_SOURCE_HAN));
        assert!(defs.font_data.contains_key(FONT_SOURCE_HAN_BOLD));
        assert!(defs.font_data.contains_key(FONT_JETBRAINS_MONO));
        assert!(defs.font_data.contains_key(PHOSPHOR_FONT_KEY));
    }

    /// Family insertion order defines glyph fallback priority: the primary
    /// font must be first, with the bold weight and built-in defaults behind it.
    #[test]
    fn font_definitions_place_primary_fonts_first_in_families() {
        let defs = font_definitions();

        let proportional = &defs.families[&egui::FontFamily::Proportional];
        assert_eq!(proportional[0], FONT_SOURCE_HAN);
        assert!(
            proportional.contains(&FONT_SOURCE_HAN_BOLD.to_owned()),
            "bold weight must be registered in the proportional family"
        );

        let monospace = &defs.families[&egui::FontFamily::Monospace];
        assert_eq!(monospace[0], FONT_JETBRAINS_MONO);
        assert_eq!(monospace[1], FONT_SOURCE_HAN);
    }

    /// `setup_fonts` must push the definitions onto the context so the active
    /// font atlas actually resolves the registered families.
    #[test]
    fn setup_fonts_installs_fonts_on_context() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        ctx.begin_pass(egui::RawInput::default());

        ctx.fonts(|f| {
            let defs = f.definitions();

            let proportional = &defs.families[&egui::FontFamily::Proportional];
            assert_eq!(proportional[0], FONT_SOURCE_HAN);

            let monospace = &defs.families[&egui::FontFamily::Monospace];
            assert_eq!(monospace[0], FONT_JETBRAINS_MONO);

            assert!(defs.font_data.contains_key(FONT_SOURCE_HAN));
            assert!(defs.font_data.contains_key(FONT_SOURCE_HAN_BOLD));
            assert!(defs.font_data.contains_key(FONT_JETBRAINS_MONO));
            assert!(defs.font_data.contains_key(PHOSPHOR_FONT_KEY));
        });
    }
}
