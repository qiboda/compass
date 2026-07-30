//! Toast notification widget.
//!
//! Non-modal notification popups that auto-dismiss after a configurable
//! duration. Used for transient status messages (fetch complete, errors) in
//! the chart application.

use std::time::{Duration, Instant};

/// Severity level of a toast notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub enum ToastLevel {
    /// Informational message — no user action required.
    Info,
    /// Operation completed successfully.
    Success,
    /// Non-critical warning — user may want to investigate.
    Warning,
    /// Critical error — user attention required.
    Error,
}

/// A single toast notification with level, message, creation time, and auto-dismiss duration.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Severity level.
    pub level: ToastLevel,
    /// Display message text.
    pub message: String,
    /// When this toast was created (for expiry calculation).
    pub created_at: Instant,
    /// How long this toast stays visible before auto-dismiss.
    pub duration: Duration,
}

impl Toast {
    /// Create a new toast with the given level and message.
    /// Duration is auto-selected based on level:
    /// - Info / Success / Warning: 3 seconds
    /// - Error: 8 seconds
    fn new(level: ToastLevel, message: String) -> Self {
        let duration = match level {
            ToastLevel::Info | ToastLevel::Success | ToastLevel::Warning => {
                Duration::from_secs(3)
            }
            ToastLevel::Error => Duration::from_secs(8),
        };
        Self {
            level,
            message,
            created_at: Instant::now(),
            duration,
        }
    }

    /// Returns true if this toast has exceeded its display duration and should be removed.
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }
}

/// Manages a FIFO queue of toast notifications with auto-expiry.
///
/// Toasts are pushed in (newest last), popped from the front (oldest first).
/// The queue is capped at 10 items — pushing beyond the cap evicts the oldest.
/// The GUI layer is responsible for rendering and timing.
#[derive(Debug, Clone, Default)]
pub struct ToastManager {
    toasts: Vec<Toast>,
}

impl ToastManager {
    /// Create an empty toast manager.
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
        }
    }

    /// Number of pending toasts.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.toasts.len()
    }

    /// Returns true if no toasts are pending.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// Push a new toast onto the end of the queue.
    ///
    /// Automatically assigns duration based on level. If the queue exceeds
    /// the maximum capacity (10), the oldest toast is evicted.
    pub fn push(&mut self, level: ToastLevel, message: impl Into<String>) {
        let toast = Toast::new(level, message.into());
        self.toasts.push(toast);
        // Cap at max 10 — evict oldest if exceeded
        if self.toasts.len() > 10 {
            self.toasts.remove(0);
        }
    }

    /// Remove and return the oldest toast (FIFO). Returns `None` if empty.
    #[allow(dead_code)]
    pub fn pop(&mut self) -> Option<Toast> {
        if self.toasts.is_empty() {
            None
        } else {
            Some(self.toasts.remove(0))
        }
    }

    /// Remove expired toasts and render remaining ones in a top-right stack.
    ///
    /// Each toast is rendered as a colored card with a Phosphor icon, the
    /// message text, and a thin horizontal progress bar that shrinks as the
    /// toast approaches expiry.
    pub fn render(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| !t.is_expired());

        if self.toasts.is_empty() {
            return;
        }

        egui::Area::new(egui::Id::new("toast_area"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for toast in &self.toasts {
                    let (icon, color) = match toast.level {
                        ToastLevel::Info => (
                            egui_phosphor::regular::INFO,
                            egui::Color32::from_rgb(59, 130, 246),
                        ),
                        ToastLevel::Success => (
                            egui_phosphor::regular::CHECK_CIRCLE,
                            egui::Color32::from_rgb(34, 197, 94),
                        ),
                        ToastLevel::Warning => (
                            egui_phosphor::regular::WARNING,
                            egui::Color32::from_rgb(234, 179, 8),
                        ),
                        ToastLevel::Error => (
                            egui_phosphor::regular::X_CIRCLE,
                            egui::Color32::from_rgb(239, 68, 68),
                        ),
                    };

                    let elapsed = toast.created_at.elapsed();
                    let remaining = toast.duration.saturating_sub(elapsed);
                    let fraction = remaining.as_secs_f32()
                        / toast.duration.as_secs_f32().max(0.001);

                    egui::Frame::new()
                        .fill(color.linear_multiply(0.15))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_min_width(240.0);
                            ui.horizontal(|ui| {
                                ui.colored_label(color, icon.to_string());
                                ui.add_space(6.0);
                                ui.label(&toast.message);
                            });
                            // Thin progress bar showing remaining lifetime
                            let bar_width = ui.available_width()
                                * fraction.clamp(0.0, 1.0);
                            let bar_rect = egui::Rect::from_min_size(
                                ui.next_widget_position(),
                                egui::vec2(bar_width, 3.0),
                            );
                            ui.painter().rect_filled(bar_rect, 0.0, color);
                            ui.add_space(4.0);
                        });
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_is_empty() {
        let manager = ToastManager::new();
        assert_eq!(manager.len(), 0);
        assert!(manager.is_empty());
    }

    #[test]
    fn push_increases_count() {
        let mut manager = ToastManager::new();
        manager.push(ToastLevel::Info, "hello");
        assert_eq!(manager.len(), 1);
        assert!(!manager.is_empty());
    }

    #[test]
    fn pop_returns_fifo_order() {
        let mut manager = ToastManager::new();
        manager.push(ToastLevel::Info, "first");
        manager.push(ToastLevel::Error, "second");

        let first = manager.pop().expect("should have first toast");
        assert_eq!(first.message, "first");
        assert_eq!(first.level, ToastLevel::Info);
        assert_eq!(manager.len(), 1);

        let second = manager.pop().expect("should have second toast");
        assert_eq!(second.message, "second");
        assert_eq!(second.level, ToastLevel::Error);
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn pop_empty_returns_none() {
        let mut manager = ToastManager::new();
        assert!(manager.pop().is_none());
    }

    #[test]
    fn toast_level_ordering_correct() {
        // Error > Warning > Info (Success is between Warning and Info)
        assert!(ToastLevel::Error > ToastLevel::Warning);
        assert!(ToastLevel::Warning > ToastLevel::Success);
        assert!(ToastLevel::Success > ToastLevel::Info);
    }
}
