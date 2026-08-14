//! Spacing / dimension tokens (design doc `.dsh/designs/gui-upgrade.md` §4.2).

/// Spacing and control dimension scale, in points (px at 100% scale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpacingTokens {
    /// Compact in-group gap (4 px).
    pub xs: f32,
    /// In-group gap (8 px).
    pub sm: f32,
    /// Regular inter-group gap (12 px).
    pub md: f32,
    /// Panel padding (16 px).
    pub lg: f32,
    /// Section gap (24 px).
    pub xl: f32,
    /// Large section gap (32 px).
    pub xxl: f32,
    /// Small control height — Tag / small IconButton (24 px).
    pub control_sm: f32,
    /// Regular control height — Button / Input / Dropdown (32 px).
    pub control_md: f32,
    /// Large control height — Toolbar / primary button (40 px).
    pub control_lg: f32,
    /// Toolbar height (40 px).
    pub toolbar_h: f32,
    /// StatusBar height (26 px).
    pub statusbar_h: f32,
    /// Sidebar width (240 px, resizable 200–320).
    pub sidebar_w: f32,
    /// Dock tab bar height (28 px).
    pub tabbar_h: f32,
    /// Data table row height (18 px).
    pub table_row_h: f32,
}

impl Default for SpacingTokens {
    fn default() -> Self {
        Self {
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
            xl: 24.0,
            xxl: 32.0,
            control_sm: 24.0,
            control_md: 32.0,
            control_lg: 40.0,
            toolbar_h: 40.0,
            statusbar_h: 26.0,
            sidebar_w: 240.0,
            tabbar_h: 28.0,
            table_row_h: 18.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every spacing field asserted against the design doc §4.2 table.
    #[test]
    fn spacing_matches_design_spec() {
        let s = SpacingTokens::default();
        assert_eq!(s.xs, 4.0);
        assert_eq!(s.sm, 8.0);
        assert_eq!(s.md, 12.0);
        assert_eq!(s.lg, 16.0);
        assert_eq!(s.xl, 24.0);
        assert_eq!(s.xxl, 32.0);
        assert_eq!(s.control_sm, 24.0);
        assert_eq!(s.control_md, 32.0);
        assert_eq!(s.control_lg, 40.0);
        assert_eq!(s.toolbar_h, 40.0);
        assert_eq!(s.statusbar_h, 26.0);
        assert_eq!(s.sidebar_w, 240.0);
        assert_eq!(s.tabbar_h, 28.0);
        assert_eq!(s.table_row_h, 18.0);
    }

    /// The scale is strictly increasing from xs to xxl (sanity on the design ladder).
    #[test]
    fn spacing_scale_is_monotonic() {
        let s = SpacingTokens::default();
        assert!(s.xs < s.sm && s.sm < s.md && s.md < s.lg && s.lg < s.xl && s.xl < s.xxl);
    }
}
