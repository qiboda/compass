//! Toast notification widget (design doc `.omo/designs/gui-upgrade.md` §5.2 / §7).
//!
//! Migrated from the binary crate (epic #119 S5) and enhanced with token-based
//! level colors, an entry animation (right slide +16 px → 0, alpha 0 → 1,
//! 150 ms cubic-out) and a closing state machine (alpha → 0 + height → 0 over
//! 100 ms linear before removal). Cards are 280 px wide, anchored 16 px from
//! the top-right corner, with a 3 px level bar on the left, an optional close
//! button and a 3 px lifetime progress bar at the bottom.

use std::time::{Duration, Instant};

use crate::tokens::{ColorTokens, ThemeTokens};

/// Entry animation duration (design §4.6 `base`).
const ENTRY_DURATION: Duration = Duration::from_millis(150);
/// Close animation duration (design §4.6 `fast`).
const CLOSE_DURATION: Duration = Duration::from_millis(100);

/// Linear 0→1 progress of `started → now` within `duration`, clamped.
fn progress_since(started: Instant, now: Instant, duration: Duration) -> f32 {
    (now.saturating_duration_since(started).as_secs_f32() / duration.as_secs_f32().max(0.001))
        .clamp(0.0, 1.0)
}

/// Severity level of a toast notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

impl ToastLevel {
    /// Token color for this level (`info` / `success` / `warning` / `error`).
    pub fn color(&self, tokens: &ColorTokens) -> egui::Color32 {
        match self {
            ToastLevel::Info => tokens.info,
            ToastLevel::Success => tokens.success,
            ToastLevel::Warning => tokens.warning,
            ToastLevel::Error => tokens.error,
        }
    }

    /// Phosphor icon glyph for this level.
    pub fn icon(&self) -> &'static str {
        match self {
            ToastLevel::Info => egui_phosphor::regular::INFO,
            ToastLevel::Success => egui_phosphor::regular::CHECK_CIRCLE,
            ToastLevel::Warning => egui_phosphor::regular::WARNING,
            ToastLevel::Error => egui_phosphor::regular::X_CIRCLE,
        }
    }
}

/// A single toast notification with level, message, creation time, auto-dismiss
/// duration and animation state.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Unique id within its manager (stable animation / area identity).
    pub id: u64,
    /// Severity level.
    pub level: ToastLevel,
    /// Display message text.
    pub message: String,
    /// When this toast was created (entry animation + expiry base).
    pub created_at: Instant,
    /// How long this toast stays visible before auto-dismiss.
    pub duration: Duration,
    /// Whether the closing animation is running.
    pub closing: bool,
    /// When the closing animation started (`Some` while closing).
    pub close_started: Option<Instant>,
    /// Rendered card height from the last frame (close-collapse reference).
    pub height: f32,
}

impl Toast {
    /// Create a new toast; duration is auto-selected by level
    /// (Info/Success/Warning: 3 s, Error: 8 s).
    fn new(id: u64, level: ToastLevel, message: String) -> Self {
        let duration = match level {
            ToastLevel::Info | ToastLevel::Success | ToastLevel::Warning => Duration::from_secs(3),
            ToastLevel::Error => Duration::from_secs(8),
        };
        Self {
            id,
            level,
            message,
            created_at: Instant::now(),
            duration,
            closing: false,
            close_started: None,
            height: 0.0,
        }
    }

    /// Entry animation progress 0→1 (150 ms since creation).
    pub fn entry_progress(&self, now: Instant) -> f32 {
        progress_since(self.created_at, now, ENTRY_DURATION)
    }

    /// Close animation progress 0→1 (100 ms since closing started); 0 when not closing.
    pub fn close_progress(&self, now: Instant) -> f32 {
        self.close_started
            .map(|t| progress_since(t, now, CLOSE_DURATION))
            .unwrap_or(0.0)
    }

    /// Begin the closing animation (idempotent).
    fn close(&mut self, now: Instant) {
        if !self.closing {
            self.closing = true;
            self.close_started = Some(now);
        }
    }

    /// Returns true if this toast has exceeded its display duration.
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }
}

/// Manages a FIFO queue of toast notifications with auto-expiry, entry/close
/// animations and top-right stacking.
///
/// Toasts are pushed in (newest last), popped from the front (oldest first).
/// The queue is capped at 10 items — pushing beyond the cap evicts the oldest.
/// Expired toasts play the closing animation before removal.
#[derive(Debug, Clone)]
pub struct ToastManager {
    tokens: ThemeTokens,
    toasts: Vec<Toast>,
    next_id: u64,
}

impl ToastManager {
    /// Create an empty toast manager for the given theme.
    pub fn new(tokens: ThemeTokens) -> Self {
        Self {
            tokens,
            toasts: Vec::new(),
            next_id: 0,
        }
    }

    /// Update the theme tokens after a theme switch so new toasts use the
    /// new palette.
    pub fn set_tokens(&mut self, tokens: ThemeTokens) {
        self.tokens = tokens;
    }

    /// Number of pending toasts.
    pub fn len(&self) -> usize {
        self.toasts.len()
    }

    /// Returns true if no toasts are pending.
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// Push a new toast onto the end of the queue.
    ///
    /// Automatically assigns duration based on level. If the queue exceeds the
    /// maximum capacity (10), the oldest toast is evicted.
    pub fn push(&mut self, level: ToastLevel, message: impl Into<String>) {
        let toast = Toast::new(self.next_id, level, message.into());
        self.next_id += 1;
        self.toasts.push(toast);
        if self.toasts.len() > 10 {
            self.toasts.remove(0);
        }
    }

    /// Remove and return the oldest toast (FIFO). Returns `None` if empty.
    pub fn pop(&mut self) -> Option<Toast> {
        if self.toasts.is_empty() {
            None
        } else {
            Some(self.toasts.remove(0))
        }
    }

    /// Render the toast stack (top-right, 16 px anchor).
    ///
    /// Expired toasts enter the closing animation; closing toasts fade and
    /// collapse (alpha → 0, height → 0, 100 ms) before removal.
    pub fn render(&mut self, ctx: &egui::Context) {
        let tokens = self.tokens;
        let now = Instant::now();

        // Expiry transitions the toast into the closing animation instead of
        // removing it instantly, so the fade-out plays.
        for toast in &mut self.toasts {
            if toast.is_expired() {
                toast.close(now);
            }
        }
        if self.toasts.is_empty() {
            return;
        }

        // Stack the cards top-right; each toast lives in its own Area so the
        // entry slide (+16 px) can be applied per card. Y positions reuse the
        // previous frame's measured heights (1-frame lag is imperceptible in a
        // 100 ms close animation).
        let mut y = 16.0;
        let mut remove_at: Vec<usize> = Vec::new();
        for (i, toast) in self.toasts.iter_mut().enumerate() {
            let close = toast.close_progress(now);
            if toast.closing && close >= 1.0 {
                remove_at.push(i);
                continue;
            }

            let entry_raw = toast.entry_progress(now);
            let entry = emath::easing::cubic_out(entry_raw);
            let alpha = entry * (1.0 - close);
            let x_off = (1.0 - entry_raw) * 16.0;
            let vis_height = if toast.closing {
                Some(toast.height * (1.0 - close))
            } else {
                None
            };

            let mut height = toast.height;
            let area_id = egui::Id::new(("compass_toast_area", toast.id));
            egui::Area::new(area_id)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0 - x_off, y))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    height = render_toast(ui, toast, &tokens, alpha, vis_height);
                });
            toast.height = height;
            y += toast.height + 8.0;

            // Drive the animations while they run. The delay is scaled to the
            // backend's frame time: kittest simulates a coarse `predicted_dt`
            // (250 ms by default), which would collapse a fixed 16 ms delay
            // into an immediate repaint and spin the harness.
            if entry_raw < 1.0 || (toast.closing && close < 1.0) {
                request_animation_repaint(ctx);
            }
        }
        for i in remove_at.into_iter().rev() {
            self.toasts.remove(i);
        }
    }
}

/// Request a repaint scaled to the backend's frame time (see [`ToastManager::render`]).
fn request_animation_repaint(ctx: &egui::Context) {
    let dt = ctx.input(|i| i.predicted_dt).max(0.0);
    let delay = Duration::from_secs_f32(dt * 1.5).max(Duration::from_millis(16));
    ctx.request_repaint_after(delay);
}

/// Render a single toast card; returns the measured (unclipped) card height.
fn render_toast(
    ui: &mut egui::Ui,
    toast: &mut Toast,
    tokens: &ThemeTokens,
    alpha: f32,
    vis_height: Option<f32>,
) -> f32 {
    let c = &tokens.color;
    let level_color = toast.level.color(c);

    ui.set_opacity(alpha);
    if let Some(h) = vis_height {
        // Collapse the card visually (clip to the shrinking height) while
        // closing; the layout rect above stays full-size so text never
        // re-wraps mid-animation.
        let clip = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(280.0, h));
        ui.set_clip_rect(clip);
    }

    let frame = egui::Frame::new()
        .fill(c.bg_panel)
        .stroke(egui::Stroke::new(1.0, c.border))
        .corner_radius(tokens.radius.md)
        .shadow(tokens.shadow.popup)
        .inner_margin(egui::Margin::symmetric(8, 6));
    let inner = frame.show(ui, |ui| {
        ui.set_min_width(280.0);
        ui.horizontal(|ui| {
            // 3 px level-color bar on the left edge.
            let bar =
                egui::Rect::from_min_size(ui.cursor().min, egui::vec2(3.0, ui.available_height()));
            ui.painter().rect_filled(bar, 0.0, level_color);
            ui.add_space(8.0);

            ui.label(
                egui::RichText::new(toast.level.icon())
                    .color(level_color)
                    .size(tokens.typography.body),
            );
            ui.add_space(4.0);
            ui.label(&toast.message);

            // Close button at the right edge.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close_btn = egui::Button::new(
                    egui::RichText::new(egui_phosphor::regular::X)
                        .color(c.text_secondary)
                        .size(tokens.typography.caption),
                )
                .frame(false)
                .min_size(egui::vec2(18.0, 18.0));
                if ui.add(close_btn).clicked() {
                    toast.close(Instant::now());
                }
            });
        });

        // 3 px lifetime progress bar at the bottom.
        let elapsed = toast.created_at.elapsed();
        let remaining = toast.duration.saturating_sub(elapsed);
        let fraction = remaining.as_secs_f32() / toast.duration.as_secs_f32().max(0.001);
        let bar_width = ui.available_width() * fraction.clamp(0.0, 1.0);
        let bar_rect =
            egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(bar_width, 3.0));
        ui.painter().rect_filled(bar_rect, 0.0, level_color);
        ui.add_space(4.0);
    });

    inner.response.rect.height()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- manager basics (migrated) ---

    #[test]
    fn new_manager_is_empty() {
        let manager = ToastManager::new(ThemeTokens::dark());
        assert_eq!(manager.len(), 0);
        assert!(manager.is_empty());
    }

    #[test]
    fn push_increases_count() {
        let mut manager = ToastManager::new(ThemeTokens::dark());
        manager.push(ToastLevel::Info, "hello");
        assert_eq!(manager.len(), 1);
        assert!(!manager.is_empty());
    }

    #[test]
    fn pop_returns_fifo_order() {
        let mut manager = ToastManager::new(ThemeTokens::dark());
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
        let mut manager = ToastManager::new(ThemeTokens::dark());
        assert!(manager.pop().is_none());
    }

    #[test]
    fn toast_level_ordering_correct() {
        assert!(ToastLevel::Error > ToastLevel::Warning);
        assert!(ToastLevel::Warning > ToastLevel::Success);
        assert!(ToastLevel::Success > ToastLevel::Info);
    }

    #[test]
    fn test_push_cap_at_10_evicts_oldest() {
        let mut manager = ToastManager::new(ThemeTokens::dark());
        for i in 0..12 {
            manager.push(ToastLevel::Info, format!("toast-{i}"));
        }
        assert_eq!(manager.len(), 10);

        // First 2 should be evicted (oldest); pop returns toast-2 first.
        let first = manager.pop().expect("should have toast-2");
        assert_eq!(first.message, "toast-2");
    }

    #[test]
    fn test_push_error_has_longer_duration() {
        let mut manager = ToastManager::new(ThemeTokens::dark());
        manager.push(ToastLevel::Error, "err");
        manager.push(ToastLevel::Info, "info");

        let err_toast = manager.pop().expect("error toast");
        let info_toast = manager.pop().expect("info toast");

        assert_eq!(err_toast.duration, Duration::from_secs(8));
        assert_eq!(info_toast.duration, Duration::from_secs(3));
    }

    #[test]
    fn test_is_expired_fresh_toast_not_expired() {
        let toast = Toast::new(0, ToastLevel::Info, "fresh".into());
        assert!(!toast.is_expired());
    }

    #[test]
    fn test_is_expired_expired_toast() {
        let expired = Toast {
            id: 0,
            level: ToastLevel::Info,
            message: "expired".into(),
            created_at: Instant::now() - Duration::from_secs(10),
            duration: Duration::from_secs(3),
            closing: false,
            close_started: None,
            height: 0.0,
        };
        assert!(expired.is_expired());
    }

    #[test]
    fn test_is_expired_exactly_at_boundary() {
        let exact = Toast {
            id: 0,
            level: ToastLevel::Success,
            message: "boundary".into(),
            created_at: Instant::now() - Duration::from_secs(3),
            duration: Duration::from_secs(3),
            closing: false,
            close_started: None,
            height: 0.0,
        };
        // >= duration, so this should be expired.
        assert!(exact.is_expired());
    }

    // --- level tokens (NEW: 4 colors read ColorTokens) ---

    #[test]
    fn level_colors_follow_color_tokens() {
        let tokens = ThemeTokens::dark();
        let c = &tokens.color;
        assert_eq!(ToastLevel::Info.color(c), c.info);
        assert_eq!(ToastLevel::Success.color(c), c.success);
        assert_eq!(ToastLevel::Warning.color(c), c.warning);
        assert_eq!(ToastLevel::Error.color(c), c.error);
    }

    #[test]
    fn level_icons_are_distinct() {
        let icons = [
            ToastLevel::Info.icon(),
            ToastLevel::Success.icon(),
            ToastLevel::Warning.icon(),
            ToastLevel::Error.icon(),
        ];
        for (i, a) in icons.iter().enumerate() {
            for b in &icons[i + 1..] {
                assert_ne!(a, b, "levels must have distinct icons");
            }
        }
    }

    // --- animation progress (NEW: pure frame assertions) ---

    #[test]
    fn entry_progress_boundaries() {
        let toast = Toast::new(0, ToastLevel::Info, "x".into());
        let start = toast.created_at;
        assert_eq!(toast.entry_progress(start), 0.0);
        assert!((toast.entry_progress(start + Duration::from_millis(75)) - 0.5).abs() < 0.001);
        assert_eq!(
            toast.entry_progress(start + Duration::from_millis(150)),
            1.0
        );
        assert_eq!(toast.entry_progress(start + Duration::from_secs(1)), 1.0);
    }

    #[test]
    fn close_progress_boundaries() {
        let mut toast = Toast::new(0, ToastLevel::Info, "x".into());
        let start = toast.created_at;
        assert_eq!(toast.close_progress(start), 0.0, "not closing → 0");
        toast.close(start);
        let close_started = toast.close_started.expect("close sets the timestamp");
        assert_eq!(toast.close_progress(close_started), 0.0);
        assert!(
            (toast.close_progress(close_started + Duration::from_millis(50)) - 0.5).abs() < 0.001
        );
        assert_eq!(
            toast.close_progress(close_started + Duration::from_millis(100)),
            1.0
        );
        assert_eq!(
            toast.close_progress(close_started + Duration::from_secs(1)),
            1.0
        );
    }

    // --- closing state machine (NEW) ---

    #[test]
    fn close_is_idempotent() {
        let now = Instant::now();
        let mut toast = Toast::new(0, ToastLevel::Info, "x".into());
        toast.close(now);
        let started = toast.close_started;
        toast.close(now + Duration::from_millis(50));
        assert_eq!(
            toast.close_started, started,
            "second close must not restart"
        );
        assert!(toast.closing);
    }

    fn harness_for_toasts(
        manager: &std::rc::Rc<std::cell::RefCell<ToastManager>>,
    ) -> egui_kittest::Harness<'static> {
        let m = manager.clone();
        egui_kittest::Harness::new_ui(move |ui| {
            m.borrow_mut().render(ui.ctx());
        })
    }

    #[test]
    fn test_render_empty_no_panic() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let manager = Rc::new(RefCell::new(ToastManager::new(ThemeTokens::dark())));
        let mut harness = harness_for_toasts(&manager);
        harness.run();
    }

    #[test]
    fn test_render_with_toasts_no_panic() {
        use egui_kittest::kittest::Queryable;
        use std::cell::RefCell;
        use std::rc::Rc;
        let manager = Rc::new(RefCell::new(ToastManager::new(ThemeTokens::dark())));
        manager.borrow_mut().push(ToastLevel::Info, "info toast");
        manager
            .borrow_mut()
            .push(ToastLevel::Success, "success toast");
        manager
            .borrow_mut()
            .push(ToastLevel::Warning, "warning toast");
        manager.borrow_mut().push(ToastLevel::Error, "error toast");

        let mut harness = harness_for_toasts(&manager);
        harness.run();
        // Verify all message labels render.
        let _info = harness.get_by_label("info toast");
        let _success = harness.get_by_label("success toast");
        let _warning = harness.get_by_label("warning toast");
        let _error = harness.get_by_label("error toast");
    }

    #[test]
    fn test_render_expired_toast_closes_then_is_removed() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let manager = Rc::new(RefCell::new(ToastManager::new(ThemeTokens::dark())));

        // Inject an expired toast plus a fresh one.
        {
            let expired = Toast {
                id: 0,
                level: ToastLevel::Info,
                message: "expired-toast".into(),
                created_at: Instant::now() - Duration::from_secs(10),
                duration: Duration::from_secs(3),
                closing: false,
                close_started: None,
                height: 0.0,
            };
            manager.borrow_mut().toasts.push(expired);
        }
        manager
            .borrow_mut()
            .push(ToastLevel::Success, "fresh-toast");

        let mut harness = harness_for_toasts(&manager);
        harness.run();

        // Expired toast must have entered the closing animation, not be
        // removed instantly.
        assert_eq!(
            manager.borrow().len(),
            2,
            "expired toast is closing, not removed"
        );
        assert!(manager.borrow().toasts[0].closing);
        assert!(manager.borrow().toasts[0].close_started.is_some());

        // Once the closing animation completes, the toast is removed.
        let now = Instant::now();
        manager.borrow_mut().toasts[0].close_started = Some(now - Duration::from_millis(200));
        harness.run();
        let remaining = manager.borrow().len();
        assert_eq!(
            remaining, 1,
            "closing toast should be removed after the animation"
        );
    }

    #[test]
    fn test_close_button_starts_closing_animation() {
        use egui_kittest::kittest::Queryable;
        use std::cell::RefCell;
        use std::rc::Rc;
        let manager = Rc::new(RefCell::new(ToastManager::new(ThemeTokens::dark())));
        manager.borrow_mut().push(ToastLevel::Info, "dismiss me");

        let mut harness = harness_for_toasts(&manager);
        harness.run();

        harness.get_by_label(egui_phosphor::regular::X).click();
        harness.run();

        assert!(
            manager.borrow().toasts[0].closing,
            "clicking the close button must start the closing animation"
        );
        assert!(
            manager.borrow().toasts[0].close_started.is_some(),
            "close timestamp must be recorded"
        );

        // Mid-animation the toast is still rendered.
        assert_eq!(manager.borrow().len(), 1);

        // After the close animation completes the toast is removed.
        let now = Instant::now();
        manager.borrow_mut().toasts[0].close_started = Some(now - Duration::from_millis(200));
        harness.run();
        assert!(manager.borrow().is_empty());
    }

    #[test]
    fn set_tokens_updates_theme_after_switch() {
        let dark = ThemeTokens::dark();
        let light = ThemeTokens::light();
        let mut manager = ToastManager::new(dark);
        manager.set_tokens(light);

        assert_eq!(
            ToastLevel::Success.color(&manager.tokens.color),
            ToastLevel::Success.color(&light.color),
            "after set_tokens the manager must use the light palette"
        );
        assert_ne!(
            ToastLevel::Success.color(&manager.tokens.color),
            ToastLevel::Success.color(&dark.color),
            "the manager must no longer use the dark palette"
        );
    }
}
