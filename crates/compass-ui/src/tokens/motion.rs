//! Motion duration tokens (design doc `.omo/designs/gui-upgrade.md` §4.6).

use std::time::Duration;

/// Motion durations for UI transitions (design doc §4.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionTokens {
    /// Fast transitions — toast close / row hover (100 ms, linear).
    pub fast: Duration,
    /// Base transitions — toast enter / modal panel / state changes (150 ms, cubic-out).
    pub base: Duration,
    /// Slow large-scale transitions — panel show/hide (300 ms, cubic-in-out).
    pub slow: Duration,
}

impl Default for MotionTokens {
    fn default() -> Self {
        Self {
            fast: Duration::from_millis(100),
            base: Duration::from_millis(150),
            slow: Duration::from_millis(300),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every motion duration asserted against the design doc §4.6 table.
    #[test]
    fn motion_matches_design_spec() {
        let m = MotionTokens::default();
        assert_eq!(m.fast, Duration::from_millis(100));
        assert_eq!(m.base, Duration::from_millis(150));
        assert_eq!(m.slow, Duration::from_millis(300));
    }

    /// Durations must be ordered fast < base < slow.
    #[test]
    fn durations_are_ordered() {
        let m = MotionTokens::default();
        assert!(m.fast < m.base && m.base < m.slow);
    }
}
