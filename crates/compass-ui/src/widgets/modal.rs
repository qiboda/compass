//! Modal dialog widget (design doc `.omo/designs/gui-upgrade.md` §5.2 / §7).
//!
//! Migrated from the binary crate (epic #119 S5) and enhanced with a closing
//! state machine (100 ms fade before `is_open` flips to false), an open
//! animation (backdrop alpha 0→1 over 120 ms linear; panel scale 0.95→1.0
//! over 150 ms cubic-out), a black 60% backdrop with the modal shadow token,
//! and a `Danger` confirm variant. The `on_confirm` `FnOnce` contract is
//! preserved: the callback fires once on confirm and is then consumed.

use std::time::Duration;

use egui::{Align2, Area, Color32, Frame, Id, LayerId, Margin, Order, Rect, Sense, Vec2};

use crate::tokens::ThemeTokens;
use crate::widgets::button::{Button, ButtonVariant};
use compass_i18n::t;

/// Backdrop fade-in duration (design §7 #3, 120 ms linear).
const BACKDROP_DURATION: Duration = Duration::from_millis(120);
/// Panel scale animation duration (design §4.6 `base`, 150 ms cubic-out).
const PANEL_DURATION: Duration = Duration::from_millis(150);
/// Close fade duration (design §4.6 `fast`, 100 ms linear).
const CLOSE_DURATION: Duration = Duration::from_millis(100);

/// Linear 0→1 progress of `started → now` within `duration`, clamped.
///
/// `started` and `now` are egui virtual-time seconds (`ctx.input(|i| i.time)`),
/// so animations advance deterministically under kittest regardless of the
/// machine's wall clock (see `kb/dev/testing.md` §时间敏感陷阱).
fn progress_since(started: f64, now: f64, duration: Duration) -> f32 {
    ((now - started) / duration.as_secs_f64().max(0.001)).clamp(0.0, 1.0) as f32
}

/// Request a repaint scaled to the backend's frame time (see [`Modal::show`]).
fn request_animation_repaint(ctx: &egui::Context) {
    let dt = ctx.input(|i| i.predicted_dt).max(0.0);
    let delay = Duration::from_secs_f32(dt * 1.5).max(Duration::from_millis(16));
    ctx.request_repaint_after(delay);
}

/// A modal dialog with open/close state, title, body text, an optional
/// confirmation callback and a closing animation.
///
/// Renders a fullscreen semi-transparent backdrop with a centered panel via
/// [`egui::Area`] when open. The panel contains the title, body text and
/// right-aligned Cancel (Ghost) / Confirm (Primary or Danger) buttons.
///
/// # Closing state machine
///
/// [`Modal::close`] starts a 100 ms fade; `is_open` stays `true` until the
/// fade completes (checked during [`Modal::show`]), so a closing modal keeps
/// rendering its fade-out frames. [`Modal::open`] resets the closing state.
///
/// # Focus trapping
///
/// [`egui::Area`] does **not** natively support focus trapping — keyboard focus
/// may escape the modal (e.g. via Tab). This is a known limitation of the
/// egui Area widget. For full focus-trapped modals, [`egui::Window`] in modal
/// mode would be required, but at the cost of platform-chrome borders.
pub struct Modal {
    tokens: ThemeTokens,
    is_open: bool,
    /// Whether the closing animation is running (public for state-machine tests).
    pub closing: bool,
    /// When the open animation started, in egui virtual-time seconds
    /// (`ctx.input(|i| i.time)`); `Some` while opening.
    pub open_started: Option<f64>,
    /// When the closing animation started, in egui virtual-time seconds
    /// (`ctx.input(|i| i.time)`); `Some` while closing.
    pub close_started: Option<f64>,
    /// Title text displayed in the modal header.
    title: String,
    /// Body text displayed in the modal content area.
    body: String,
    /// Whether the confirm button uses the Danger variant (default Primary).
    danger: bool,
    /// Confirm button label (default `t!("common.confirm")`, locale-resolved).
    confirm_text: String,
    /// Cancel button label (default `t!("common.cancel")`, locale-resolved).
    cancel_text: String,
    /// Optional callback invoked when the user clicks the confirm button.
    /// Consumed on use — set to `None` after calling.
    on_confirm: Option<Box<dyn FnOnce()>>,
}

impl std::fmt::Debug for Modal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Modal")
            .field("is_open", &self.is_open)
            .field("closing", &self.closing)
            .field("title", &self.title)
            .field("body", &self.body)
            .field("danger", &self.danger)
            .field("on_confirm", &"<callback>")
            .finish()
    }
}

impl Modal {
    /// Create a new modal in the closed state for the given theme.
    pub fn new(tokens: ThemeTokens) -> Self {
        Self {
            tokens,
            is_open: false,
            closing: false,
            open_started: None,
            close_started: None,
            title: String::new(),
            body: String::new(),
            danger: false,
            confirm_text: t!("common.confirm").into_owned(),
            cancel_text: t!("common.cancel").into_owned(),
            on_confirm: None,
        }
    }

    /// Returns `true` if the modal is currently open (visible).
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Update the theme tokens (e.g. after a theme switch) so the panel
    /// restyles without being recreated.
    pub fn set_tokens(&mut self, tokens: ThemeTokens) {
        self.tokens = tokens;
    }

    /// Open the modal and start the entry animation (resets any closing state).
    ///
    /// `now` is the current egui virtual time in seconds
    /// (`ctx.input(|i| i.time)`), stamped as the entry-animation start.
    pub fn open(&mut self, now: f64) {
        self.is_open = true;
        self.closing = false;
        self.close_started = None;
        self.open_started = Some(now);
    }

    /// Close the modal: starts the 100 ms closing animation; `is_open` flips
    /// to `false` once the fade completes during [`Self::show`].
    ///
    /// `now` is the current egui virtual time in seconds
    /// (`ctx.input(|i| i.time)`), stamped as the closing-animation start.
    pub fn close(&mut self, now: f64) {
        if self.is_open && !self.closing {
            self.closing = true;
            self.close_started = Some(now);
        }
    }

    /// Toggle the open/close state.
    ///
    /// `now` is the current egui virtual time in seconds
    /// (`ctx.input(|i| i.time)`), forwarded to [`Self::open`] / [`Self::close`].
    pub fn toggle(&mut self, now: f64) {
        if self.is_open {
            self.close(now);
        } else {
            self.open(now);
        }
    }

    /// Set the title text displayed in the modal header.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// Set the body text displayed in the modal content area.
    pub fn set_body(&mut self, body: impl Into<String>) {
        self.body = body.into();
    }

    /// Switch the confirm button to the Danger variant (default Primary).
    pub fn set_danger(&mut self, danger: bool) {
        self.danger = danger;
    }

    /// Override the confirm button label (default `t!("common.confirm")`).
    pub fn set_confirm_text(&mut self, text: impl Into<String>) {
        self.confirm_text = text.into();
    }

    /// Override the cancel button label (default `t!("common.cancel")`).
    pub fn set_cancel_text(&mut self, text: impl Into<String>) {
        self.cancel_text = text.into();
    }

    /// Set the callback invoked when the user clicks the confirm button.
    ///
    /// The callback is consumed after one invocation. Subsequent confirm
    /// clicks will simply close the modal without side effects.
    pub fn set_on_confirm(&mut self, f: impl FnOnce() + 'static) {
        self.on_confirm = Some(Box::new(f));
    }

    /// Entry animation progress 0→1 (120 ms since open).
    ///
    /// `now` is the current egui virtual time in seconds.
    pub fn entry_progress(&self, now: f64) -> f32 {
        self.open_started
            .map(|t| progress_since(t, now, BACKDROP_DURATION))
            .unwrap_or(1.0)
    }

    /// Panel scale animation progress 0→1 (150 ms since open).
    ///
    /// `now` is the current egui virtual time in seconds.
    pub fn panel_progress(&self, now: f64) -> f32 {
        self.open_started
            .map(|t| progress_since(t, now, PANEL_DURATION))
            .unwrap_or(1.0)
    }

    /// Close animation progress 0→1 (100 ms since closing started); 0 when not closing.
    ///
    /// `now` is the current egui virtual time in seconds.
    pub fn close_progress(&self, now: f64) -> f32 {
        self.close_started
            .map(|t| progress_since(t, now, CLOSE_DURATION))
            .unwrap_or(0.0)
    }

    /// Render the modal via two `egui::Area`s (backdrop + centered panel).
    ///
    /// When the modal is open, this method draws:
    /// 1. A fullscreen backdrop (black 60%, fading in over 120 ms) that
    ///    consumes clicks.
    /// 2. A centered panel (`bg_panel` + `radius.lg` + `padding.xl` + modal
    ///    shadow, scale 0.95→1.0 over 150 ms) with title, body and
    ///    right-aligned Cancel (Ghost) / Confirm (Primary|Danger) buttons.
    ///
    /// The confirm button calls the `on_confirm` callback (if set) and then
    /// starts the closing animation; Cancel / Esc do the same without the
    /// callback. When the modal is closed, this method is a no-op.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.is_open {
            return;
        }

        let tokens = self.tokens;
        let c = &tokens.color;
        let now = ctx.input(|i| i.time);
        let entry = self.entry_progress(now);
        let close = self.close_progress(now);

        // Closing state machine: once the 100 ms fade completes, drop the modal.
        if self.closing && close >= 1.0 {
            self.is_open = false;
            self.closing = false;
            self.close_started = None;
            return;
        }
        // Drive the animations while they run. The delay is scaled to the
        // backend's frame time: kittest simulates a coarse `predicted_dt`
        // (250 ms by default), which would collapse a fixed 16 ms delay into
        // an immediate repaint and spin the harness.
        if entry < 1.0 || (self.closing && close < 1.0) {
            request_animation_repaint(ctx);
        }

        let alpha = entry * (1.0 - close);

        // ctx.screen_rect() is not available in egui 0.35; use input screen_rect.
        let screen_rect = ctx
            .input(|i| i.raw.screen_rect)
            .unwrap_or(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1024.0, 768.0),
            ));

        // --- Semi-transparent backdrop ---
        // Fullscreen Area that captures clicks and paints the overlay
        // (black 60%, fading in over 120 ms).
        let backdrop_id = Id::new("modal_backdrop");
        Area::new(backdrop_id)
            .fixed_pos(screen_rect.min)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                // Consume all clicks on the backdrop so they don't pass
                // through to the UI behind the modal.
                ui.allocate_rect(screen_rect, Sense::click());
                ui.painter().rect_filled(
                    screen_rect,
                    0.0,
                    Color32::from_black_alpha((160.0 * alpha) as u8),
                );
            });

        // --- Centered modal panel ---
        // Placed in a second Area so it floats above the backdrop; the panel
        // scales in from 0.95 (150 ms cubic-out) while the backdrop fades.
        let mut should_close = false;
        let mut should_confirm = false;
        let interactive = !self.closing;
        let panel_id = Id::new("modal_panel");
        let panel_progress = self.panel_progress(now);

        Area::new(panel_id)
            .order(Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                let frame = Frame::new()
                    .fill(c.bg_panel)
                    .corner_radius(tokens.radius.lg)
                    .inner_margin(Margin::symmetric(
                        tokens.spacing.xl as i8,
                        tokens.spacing.xl as i8,
                    ))
                    .shadow(tokens.shadow.modal);
                frame.show(ui, |ui| {
                    ui.set_min_width(360.0);

                    // Title.
                    ui.label(
                        egui::RichText::new(&self.title)
                            .size(tokens.typography.heading)
                            .strong()
                            .color(c.text_primary),
                    );
                    ui.add_space(tokens.spacing.md);

                    // Body.
                    ui.label(
                        egui::RichText::new(&self.body)
                            .size(tokens.typography.body)
                            .color(c.text_secondary),
                    );
                    ui.add_space(tokens.spacing.lg);

                    // Right-aligned Cancel (Ghost) + Confirm (Primary|Danger).
                    let confirm_variant = if self.danger {
                        ButtonVariant::Danger
                    } else {
                        ButtonVariant::Primary
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Button::new(&tokens, &self.confirm_text)
                            .variant(confirm_variant)
                            .disabled(!interactive)
                            .show(ui)
                            .clicked()
                        {
                            should_confirm = true;
                            should_close = true;
                        }
                        if Button::new(&tokens, &self.cancel_text)
                            .variant(ButtonVariant::Ghost)
                            .disabled(!interactive)
                            .show(ui)
                            .clicked()
                        {
                            should_close = true;
                        }
                    });
                });
            });

        // Scale animation: panel pivot = screen center.
        let scale = 0.95 + 0.05 * emath::easing::cubic_out(panel_progress);
        if scale != 1.0 {
            let center = screen_rect.center();
            ctx.transform_layer_shapes(
                LayerId::new(Order::Foreground, panel_id),
                emath::TSTransform::new(center.to_vec2() * (1.0 - scale), scale),
            );
        }

        // Esc closes the modal (design §7 shortcuts).
        if interactive && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }

        if should_confirm && let Some(cb) = self.on_confirm.take() {
            cb();
        }
        if should_close {
            self.close(now);
        }
    }
}

impl Default for Modal {
    fn default() -> Self {
        Self::new(ThemeTokens::dark())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    // --- state (migrated) ---

    #[test]
    fn modal_starts_closed() {
        let modal = Modal::new(ThemeTokens::dark());
        assert!(!modal.is_open());
    }

    #[test]
    fn open_sets_modal_to_open() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.open(0.0);
        assert!(modal.is_open());
        assert!(!modal.closing);
    }

    #[test]
    fn close_starts_closing_state_machine() {
        let modal = Rc::new(RefCell::new(Modal::new(ThemeTokens::dark())));
        modal.borrow_mut().open(0.0);
        modal.borrow_mut().close(0.0);
        assert!(
            modal.borrow().closing,
            "close() must enter the closing animation"
        );
        assert!(modal.borrow().close_started.is_some());
        assert!(modal.borrow().is_open(), "still open while the fade runs");

        // Once the fade completes (100 ms), show() flips is_open to false:
        // 11 × 10 ms steps > 100 ms, driven by egui virtual time (ref #171).
        let mut harness = harness_for_modal(&modal);
        harness.run_steps(11);
        assert!(!modal.borrow().is_open());
        assert!(
            !modal.borrow().closing,
            "closing state must be reset after the fade"
        );
    }

    #[test]
    fn open_resets_closing_state() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.open(0.0);
        modal.close(0.0);
        modal.open(1.0);
        assert!(modal.is_open());
        assert!(!modal.closing, "re-open must cancel the closing animation");
        assert!(modal.close_started.is_none());
    }

    #[test]
    fn toggle_flips_state() {
        let mut modal = Modal::new(ThemeTokens::dark());
        // closed → open
        modal.toggle(0.0);
        assert!(modal.is_open());
        // open → closing
        modal.toggle(0.0);
        assert!(modal.closing);
        // toggle during the closing fade is a no-op (the fade continues)
        modal.toggle(0.0);
        assert!(modal.closing);
    }

    // --- builder setters (migrated) ---

    #[test]
    fn set_title_sets_field() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.set_title("Confirm Delete");
        assert_eq!(modal.title, "Confirm Delete");
    }

    #[test]
    fn set_title_accepts_string_types() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.set_title(String::from("owned"));
        assert_eq!(modal.title, "owned");
        modal.set_title("borrowed");
        assert_eq!(modal.title, "borrowed");
    }

    #[test]
    fn set_body_sets_field() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.set_body("Are you sure you want to continue?");
        assert_eq!(modal.body, "Are you sure you want to continue?");
    }

    #[test]
    fn set_on_confirm_stores_callback() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.set_on_confirm(|| {});
        assert!(modal.on_confirm.is_some());
    }

    #[test]
    fn danger_flag_stored_and_confirm_text_overridable() {
        let mut modal = Modal::new(ThemeTokens::dark());
        assert!(!modal.danger, "confirm defaults to Primary");
        modal.set_danger(true);
        assert!(modal.danger);
        modal.set_confirm_text("移除");
        modal.set_cancel_text("保留");
        assert_eq!(modal.confirm_text, "移除");
        assert_eq!(modal.cancel_text, "保留");
    }

    // --- animation progress (NEW: pure frame assertions) ---

    #[test]
    fn entry_progress_boundaries() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.open(0.0);
        let start = modal.open_started.expect("open sets the timestamp");
        assert_eq!(modal.entry_progress(start), 0.0);
        assert!((modal.entry_progress(start + 0.06) - 0.5).abs() < 0.001);
        assert_eq!(modal.entry_progress(start + 0.12), 1.0);
        assert_eq!(modal.entry_progress(start + 1.0), 1.0);
    }

    #[test]
    fn panel_progress_boundaries() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.open(0.0);
        let start = modal.open_started.expect("open sets the timestamp");
        assert_eq!(modal.panel_progress(start), 0.0);
        assert!((modal.panel_progress(start + 0.075) - 0.5).abs() < 0.001);
        assert_eq!(modal.panel_progress(start + 0.15), 1.0);
    }

    #[test]
    fn close_progress_boundaries() {
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.open(0.0);
        let start = modal.open_started.expect("open sets the timestamp");
        assert_eq!(modal.close_progress(start), 0.0);
        modal.close(0.0);
        let close_started = modal.close_started.expect("close sets the timestamp");
        assert_eq!(modal.close_progress(close_started), 0.0);
        assert!((modal.close_progress(close_started + 0.05) - 0.5).abs() < 0.001);
        assert_eq!(modal.close_progress(close_started + 0.1), 1.0);
    }

    #[test]
    fn progress_follows_injected_virtual_time() {
        // Animation progress is driven purely by the injected `now` (egui
        // virtual time), never the wall clock — the kittest determinism
        // contract (ref #171).
        let mut modal = Modal::new(ThemeTokens::dark());
        modal.open(5.0);
        assert_eq!(modal.entry_progress(5.0), 0.0);
        assert_eq!(modal.entry_progress(5.12), 1.0);
        assert_eq!(modal.panel_progress(5.0), 0.0);
        assert_eq!(modal.panel_progress(5.15), 1.0);
        modal.close(10.0);
        assert_eq!(modal.close_progress(10.0), 0.0);
        assert_eq!(modal.close_progress(10.1), 1.0);
    }

    /// Helper: create a harness for a modal, running one frame.
    fn harness_for_modal(modal: &Rc<RefCell<Modal>>) -> egui_kittest::Harness<'static> {
        let m = modal.clone();
        egui_kittest::Harness::builder()
            .with_step_dt(0.01)
            .build_ui(move |ui| {
                m.borrow_mut().show(ui.ctx());
            })
    }

    // --- kittest rendering (migrated) ---

    #[test]
    fn show_closed_is_noop() {
        let modal = Rc::new(RefCell::new(Modal::new(ThemeTokens::dark())));
        let mut harness = harness_for_modal(&modal);
        harness.run();
        assert!(!modal.borrow().is_open());
    }

    #[test]
    fn show_open_renders_buttons() {
        rust_i18n::set_locale("zh");
        let modal = Rc::new(RefCell::new(Modal::new(ThemeTokens::dark())));
        modal.borrow_mut().open(0.0);
        modal.borrow_mut().set_title("Test");
        modal.borrow_mut().set_body("Body");

        let mut harness = harness_for_modal(&modal);
        harness.run();

        assert!(modal.borrow().is_open());
        // Buttons must exist in the rendered tree (defaults resolve via the
        // active locale — zh "确认"/"取消").
        let _cancel = harness.get_by_label(&t!("common.cancel"));
        let _confirm = harness.get_by_label(&t!("common.confirm"));
    }

    #[test]
    fn cancel_closes_modal_without_calling_callback() {
        rust_i18n::set_locale("zh");
        let called = Rc::new(Cell::new(false));
        let modal = Rc::new(RefCell::new(Modal::new(ThemeTokens::dark())));
        modal.borrow_mut().open(0.0);
        modal.borrow_mut().set_on_confirm({
            let called = called.clone();
            move || {
                called.set(true);
            }
        });

        let mut harness = harness_for_modal(&modal);
        harness.run();

        harness.get_by_label(&t!("common.cancel")).click();
        harness.run();

        assert!(
            modal.borrow().closing,
            "modal must enter the closing animation"
        );
        assert!(modal.borrow().is_open(), "still open while the fade runs");
        assert!(!called.get(), "callback should NOT be called on Cancel");
        assert!(
            modal.borrow().on_confirm.is_some(),
            "callback should remain unconsumed after Cancel"
        );

        // Complete the fade → closed.
        // 11 × 10 ms steps complete the 100 ms fade (ref #171).
        harness.run_steps(11);
        assert!(
            !modal.borrow().is_open(),
            "modal should close after the fade"
        );
        assert!(!called.get());
    }

    #[test]
    fn confirm_button_calls_callback_and_closes() {
        rust_i18n::set_locale("zh");
        let called = Rc::new(Cell::new(false));
        let modal = Rc::new(RefCell::new(Modal::new(ThemeTokens::dark())));
        modal.borrow_mut().open(0.0);
        modal.borrow_mut().set_on_confirm({
            let called = called.clone();
            move || {
                called.set(true);
            }
        });

        let mut harness = harness_for_modal(&modal);
        harness.run();

        harness.get_by_label(&t!("common.confirm")).click();
        harness.run();

        assert!(called.get(), "callback should have been called");
        assert!(modal.borrow().closing);
        assert!(
            modal.borrow().on_confirm.is_none(),
            "callback should be consumed after confirm"
        );

        // 11 × 10 ms steps complete the 100 ms fade (ref #171).
        harness.run_steps(11);
        assert!(
            !modal.borrow().is_open(),
            "modal should close after the fade"
        );
    }

    #[test]
    fn confirm_button_consumes_callback_exactly_once() {
        rust_i18n::set_locale("zh");
        let call_count = Rc::new(Cell::new(0u32));
        let modal = Rc::new(RefCell::new(Modal::new(ThemeTokens::dark())));
        modal.borrow_mut().open(0.0);
        modal.borrow_mut().set_on_confirm({
            let call_count = call_count.clone();
            move || {
                call_count.set(call_count.get() + 1);
            }
        });

        let mut harness = harness_for_modal(&modal);
        harness.run();

        // First click — callback runs.
        harness.get_by_label(&t!("common.confirm")).click();
        harness.run();
        assert_eq!(call_count.get(), 1);

        // Finish the fade, re-open and click again — callback already consumed.
        // 11 × 10 ms steps complete the 100 ms fade (ref #171).
        harness.run_steps(11);
        assert!(!modal.borrow().is_open());

        modal.borrow_mut().open(1.0);
        // Re-open at a later virtual timestamp (literal, ahead of the harness
        // clock), so the entry animation stays frozen at progress 0. That does
        // not affect the click below: the scale transform only shifts painted
        // shapes, never the interaction rects. Steps just render deterministically.
        harness.run_steps(16);
        harness.get_by_label(&t!("common.confirm")).click();
        harness.run();
        assert_eq!(
            call_count.get(),
            1,
            "callback should never be called again after first consumption"
        );
    }

    // --- NEW: Esc closes (design §7 快捷键) ---

    #[test]
    fn escape_starts_closing_animation() {
        let modal = Rc::new(RefCell::new(Modal::new(ThemeTokens::dark())));
        modal.borrow_mut().open(0.0);

        let mut harness = harness_for_modal(&modal);
        harness.run();

        harness.key_press(egui::Key::Escape);
        harness.run();

        assert!(modal.borrow().closing, "Esc must close the modal");
        assert!(modal.borrow().is_open(), "still open while the fade runs");
    }

    // --- NEW: Danger confirm renders and confirms (design §5.2) ---

    #[test]
    fn danger_modal_confirms_on_click() {
        let called = Rc::new(Cell::new(false));
        let modal = Rc::new(RefCell::new(Modal::new(ThemeTokens::dark())));
        modal.borrow_mut().open(0.0);
        modal.borrow_mut().set_danger(true);
        modal.borrow_mut().set_confirm_text("移除");
        modal.borrow_mut().set_on_confirm({
            let called = called.clone();
            move || {
                called.set(true);
            }
        });

        let mut harness = harness_for_modal(&modal);
        harness.run();

        let _danger_btn = harness.get_by_label("移除");
        harness.get_by_label("移除").click();
        harness.run();

        assert!(called.get(), "danger confirm must fire the callback");
    }

    #[test]
    fn set_tokens_updates_theme_after_switch() {
        let dark = ThemeTokens::dark();
        let light = ThemeTokens::light();
        let mut modal = Modal::new(dark);
        modal.set_tokens(light);

        assert_eq!(
            modal.tokens, light,
            "after set_tokens the modal must use the light palette"
        );
        assert_ne!(
            modal.tokens, dark,
            "the modal must no longer use the dark palette"
        );
    }
}
