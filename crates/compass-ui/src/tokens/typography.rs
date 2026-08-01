//! Typography tokens — font size scale (design doc `.omo/designs/gui-upgrade.md` §4.3).

/// Font size scale, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeTokens {
    /// Large numbers / window titles (20 px).
    pub display: f32,
    /// Panel titles / table headers (14 px).
    pub heading: f32,
    /// Body text (12.5 px — egui default, kept).
    pub body: f32,
    /// Auxiliary labels / tags (11 px).
    pub caption: f32,
    /// Prices / codes / times via monospace font (12 px, JetBrains Mono).
    pub mono: f32,
}

impl Default for TypeTokens {
    fn default() -> Self {
        Self {
            display: 20.0,
            heading: 14.0,
            body: 12.5,
            caption: 11.0,
            mono: 12.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every font-size field asserted against the design doc §4.3 table.
    #[test]
    fn typography_matches_design_spec() {
        let t = TypeTokens::default();
        assert_eq!(t.display, 20.0);
        assert_eq!(t.heading, 14.0);
        assert_eq!(t.body, 12.5);
        assert_eq!(t.caption, 11.0);
        assert_eq!(t.mono, 12.0);
    }

    /// Body stays at the design-spec 12.5 px; the theme layer (S3) drives the
    /// egui `TextStyle::Body` size from this token explicitly.
    #[test]
    fn body_is_design_spec_value() {
        assert_eq!(TypeTokens::default().body, 12.5);
    }
}
