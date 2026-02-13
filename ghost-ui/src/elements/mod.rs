//! Elements system for ghost-ui
//!
//! Provides interactive UI elements like buttons that can be placed
//! relative to the ghost window using a bottom-left origin coordinate system.

pub mod button;
pub mod button_image;
pub mod callout;
pub mod label;
pub mod marquee_label;

pub use button::{Button, ButtonId, ButtonState, ButtonStyle};
pub use button_image::ButtonImage;
pub use label::{Label, LabelId, LabelStyle, FontStyle};
pub use marquee_label::MarqueeLabel;

/// Coordinate origin for widget positioning
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Origin {
    /// Origin at top-left corner (standard screen coordinates)
    TopLeft,
    /// Origin at bottom-left corner (common in game dev)
    #[default]
    BottomLeft,
}

/// Convert local coordinates to screen coordinates
pub fn to_screen_coords(
    x: f32,
    y: f32,
    _width: f32,
    height: f32,
    window_height: f32,
    origin: Origin,
) -> (f32, f32) {
    match origin {
        Origin::TopLeft => (x, y),
        Origin::BottomLeft => (x, window_height - y - height),
    }
}

/// Trait for renderable widgets
pub trait Widget {
    /// Update widget state based on cursor position
    fn update_hover(&mut self, cursor_x: f32, cursor_y: f32, window_height: f32);

    /// Handle mouse press, returns true if widget was clicked
    fn handle_press(&mut self, cursor_x: f32, cursor_y: f32, window_height: f32) -> bool;

    /// Handle mouse release, returns true if click was completed on widget
    fn handle_release(&mut self, cursor_x: f32, cursor_y: f32, window_height: f32) -> bool;

    /// Get the widget bounds in screen coordinates [x, y, width, height]
    fn screen_bounds(&self, window_height: f32) -> [f32; 4];
}
