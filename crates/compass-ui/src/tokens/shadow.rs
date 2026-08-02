//! Shadow tokens (design doc `.omo/designs/gui-upgrade.md` §4.5).

use egui::{Color32, Shadow};

/// Shadow definitions for popups and modals (design doc §4.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowTokens {
    /// Dropdown / toast popup: offset (0, 4), blur 12, black 35% (dark) / 15% (light).
    pub popup: Shadow,
    /// Modal above the backdrop: offset (0, 8), blur 24, black 50% (dark) / 25% (light).
    pub modal: Shadow,
}

impl ShadowTokens {
    /// Dark-theme shadows.
    pub const fn dark() -> Self {
        Self {
            popup: Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: Color32::from_black_alpha(89),
            },
            modal: Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(128),
            },
        }
    }

    /// Light-theme shadows.
    pub const fn light() -> Self {
        Self {
            popup: Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: Color32::from_black_alpha(38),
            },
            modal: Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(64),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dark-theme shadows asserted against the design doc §4.5 table.
    #[test]
    fn dark_shadows_match_design_spec() {
        let s = ShadowTokens::dark();
        assert_eq!(
            s.popup,
            Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: Color32::from_black_alpha(89),
            }
        );
        assert_eq!(
            s.modal,
            Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(128),
            }
        );
    }

    /// Light-theme shadows asserted against the design doc §4.5 table.
    #[test]
    fn light_shadows_match_design_spec() {
        let s = ShadowTokens::light();
        assert_eq!(
            s.popup,
            Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: Color32::from_black_alpha(38),
            }
        );
        assert_eq!(
            s.modal,
            Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(64),
            }
        );
    }

    /// The modal shadow must be heavier than the popup shadow (z-depth hierarchy).
    #[test]
    fn modal_is_heavier_than_popup() {
        assert!(ShadowTokens::dark().modal.blur > ShadowTokens::dark().popup.blur);
        assert!(ShadowTokens::dark().modal.color.a() > ShadowTokens::dark().popup.color.a());
    }
}
