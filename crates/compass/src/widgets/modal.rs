//! Modal dialog widget.
//!
//! A blocking overlay that captures user input before allowing further
//! interaction with the main UI. Used for confirmations, settings panels,
//! and other focused interactions.

use egui::{Align2, Area, Color32, Frame, Id, Order, Rect, Sense, Vec2};

/// A modal dialog with open/close state, title, body text, and an optional
/// confirmation callback.
///
/// Renders a fullscreen semi-transparent overlay with a centered Frame panel
/// via [`egui::Area`] when open. The panel contains the title, body text, and
/// OK / Cancel buttons.
///
/// # Focus trapping
///
/// [`egui::Area`] does **not** natively support focus trapping — keyboard focus
/// may escape the modal (e.g. via Tab). This is a known limitation of the
/// egui Area widget. For full focus-trapped modals, [`egui::Window`] in modal
/// mode would be required, but at the cost of platform-chrome borders.
pub struct Modal {
    /// Whether the modal overlay is currently visible.
    is_open: bool,
    /// Title text displayed in the modal header.
    title: String,
    /// Body text displayed in the modal content area.
    body: String,
    /// Optional callback invoked when the user clicks the OK button.
    /// Consumed on use — set to `None` after calling.
    on_confirm: Option<Box<dyn FnOnce()>>,
}

impl std::fmt::Debug for Modal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Modal")
            .field("is_open", &self.is_open)
            .field("title", &self.title)
            .field("body", &self.body)
            .field("on_confirm", &"<callback>")
            .finish()
    }
}

impl Modal {
    /// Create a new modal in the closed state with empty title and body.
    pub fn new() -> Self {
        Self {
            is_open: false,
            title: String::new(),
            body: String::new(),
            on_confirm: None,
        }
    }

    /// Returns `true` if the modal is currently open (visible).
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Open the modal (make the overlay visible).
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Close the modal (hide the overlay).
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Toggle the open/close state.
    ///
    /// If open, close; if closed, open.
    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
    }

    /// Set the title text displayed in the modal header.
    #[allow(dead_code)]
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// Set the body text displayed in the modal content area.
    #[allow(dead_code)]
    pub fn set_body(&mut self, body: impl Into<String>) {
        self.body = body.into();
    }

    /// Set the callback invoked when the user clicks the OK button.
    ///
    /// The callback is consumed after one invocation. Subsequent OK clicks
    /// will simply close the modal without side effects.
    #[allow(dead_code)]
    pub fn set_on_confirm(&mut self, f: impl FnOnce() + 'static) {
        self.on_confirm = Some(Box::new(f));
    }

    /// Render the modal via `egui::Area`.
    ///
    /// When the modal is open, this method draws:
    /// 1. A fullscreen semi-transparent backdrop that consumes clicks.
    /// 2. A centered `Frame::window` panel with the title, body text, and
    ///    OK / Cancel buttons.
    ///
    /// The OK button calls the `on_confirm` callback (if set) and then closes
    /// the modal. The Cancel button simply closes the modal.
    ///
    /// When the modal is closed, this method is a no-op.
    #[allow(dead_code)]
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.is_open {
            return;
        }

        // ctx.screen_rect() is not available in egui 0.35; use input screen_rect.
        let screen_rect = ctx
            .input(|i| i.raw.screen_rect)
            .unwrap_or(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1024.0, 768.0),
            ));

        // --- Semi-transparent backdrop ---
        // Uses a fullscreen Area to capture clicks and paint the overlay.
        Area::new(Id::new("modal_backdrop"))
            .fixed_pos(screen_rect.min)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                // Consume all clicks on the backdrop so they don't pass through
                // to the UI behind the modal.
                ui.allocate_rect(screen_rect, Sense::click());

                ui.painter().rect_filled(
                    screen_rect,
                    0.0,
                    Color32::from_black_alpha(160),
                );
            });

        // --- Centered modal panel ---
        // Placed in a second Area so it floats above the backdrop.
        let mut should_close = false;
        let mut should_confirm = false;

        Area::new(Id::new("modal_panel"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                Frame::window(ui.style()).show(ui, |ui| {
                    ui.set_min_width(320.0);

                    // Title
                    ui.heading(&self.title);
                    ui.add_space(8.0);

                    // Body
                    ui.label(&self.body);
                    ui.add_space(16.0);

                    // Buttons
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.button("Cancel").clicked() {
                                should_close = true;
                            }
                            if ui.button("  OK  ").clicked() {
                                should_confirm = true;
                                should_close = true;
                            }
                        },
                    );
                });
            });

        if should_confirm {
            if let Some(cb) = self.on_confirm.take() {
                cb();
            }
        }
        if should_close {
            self.is_open = false;
        }
    }
}

impl Default for Modal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_starts_closed() {
        let modal = Modal::new();
        assert!(!modal.is_open());
    }

    #[test]
    fn open_sets_modal_to_open() {
        let mut modal = Modal::new();
        modal.open();
        assert!(modal.is_open());
    }

    #[test]
    fn close_sets_modal_to_closed() {
        let mut modal = Modal::new();
        modal.open(); // open first
        modal.close();
        assert!(!modal.is_open());
    }

    #[test]
    fn toggle_flips_state() {
        let mut modal = Modal::new();

        // closed → open
        modal.toggle();
        assert!(modal.is_open());

        // open → closed
        modal.toggle();
        assert!(!modal.is_open());

        // closed → open again
        modal.toggle();
        assert!(modal.is_open());
    }
}
