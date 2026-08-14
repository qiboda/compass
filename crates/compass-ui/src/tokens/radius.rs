//! Corner radius tokens (design doc `.dsh/designs/gui-upgrade.md` §4.4).

/// Corner radius scale, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadiusTokens {
    /// Inputs / buttons / rows (4 px).
    pub sm: f32,
    /// Cards / panels (6 px).
    pub md: f32,
    /// Modals / popovers (10 px).
    pub lg: f32,
    /// Tags / badges — fully round (999 px).
    pub pill: f32,
}

impl Default for RadiusTokens {
    fn default() -> Self {
        Self {
            sm: 4.0,
            md: 6.0,
            lg: 10.0,
            pill: 999.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every radius field asserted against the design doc §4.4 table.
    #[test]
    fn radius_matches_design_spec() {
        let r = RadiusTokens::default();
        assert_eq!(r.sm, 4.0);
        assert_eq!(r.md, 6.0);
        assert_eq!(r.lg, 10.0);
        assert_eq!(r.pill, 999.0);
    }

    /// Pill must be large enough to fully round any realistic widget.
    #[test]
    fn pill_fully_rounds() {
        assert!(RadiusTokens::default().pill > 100.0);
    }
}
