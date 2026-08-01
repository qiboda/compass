//! Widget library: base atoms + composite molecules.
//!
//! Implemented by sub-issues #125 (S4 atoms), #127 (S5 migrated composites),
//! #128 (S6 new composites). Every atom takes [`crate::tokens::ThemeTokens`]
//! as its first constructor argument — zero business-crate dependencies.

pub mod badge;
pub mod button;
pub mod card;
pub mod checkbox;
pub mod divider;
pub mod dropdown;
pub mod empty_state;
pub mod icon_button;
pub mod input;
pub mod label;
pub mod price_text;
pub mod section_title;
pub mod segmented;
pub mod status_dot;
pub mod tag;
pub mod tooltip;
