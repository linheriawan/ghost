//! Marquee (scrolling text) label widget for ghost-ui
//!
//! Wraps a Label and adds horizontal scrolling animation.

use super::label::{Label, LabelId, LabelStyle};
use super::Origin;

/// A label with horizontally scrolling text
#[derive(Debug, Clone)]
pub struct MarqueeLabel {
    /// Inner label (provides position, size, style, text)
    label: Label,
    /// Scroll speed in pixels per second
    scroll_speed: f32,
    /// Current scroll offset in pixels
    scroll_offset: f32,
    /// Measured text width (set by WidgetRenderer after text shaping)
    text_width: f32,
    /// Gap between repeated text instances
    gap: f32,
}

impl MarqueeLabel {
    /// Create a new marquee label
    pub fn new(id: LabelId, text: impl Into<String>) -> Self {
        Self {
            label: Label::new(id, text),
            scroll_speed: 30.0,
            scroll_offset: 0.0,
            text_width: 0.0,
            gap: 40.0,
        }
    }

    /// Set the label position (relative to origin)
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.label = self.label.with_position(x, y);
        self
    }

    /// Set the label size
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.label = self.label.with_size(width, height);
        self
    }

    /// Set the label style
    pub fn with_style(mut self, style: LabelStyle) -> Self {
        self.label = self.label.with_style(style);
        self
    }

    /// Set the coordinate origin
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.label = self.label.with_origin(origin);
        self
    }

    /// Set the scroll speed (pixels per second)
    pub fn with_scroll_speed(mut self, speed: f32) -> Self {
        self.scroll_speed = speed;
        self
    }

    /// Set the gap between repeated text
    pub fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Update the scroll animation
    /// Returns true if the label needs a redraw
    pub fn update(&mut self, delta: f32) -> bool {
        if self.text_width <= 0.0 || !self.label.is_visible() {
            return false;
        }

        // Only scroll if text is wider than the label
        let label_width = self.label.size()[0];
        if self.text_width <= label_width {
            self.scroll_offset = 0.0;
            return false;
        }

        self.scroll_offset += self.scroll_speed * delta;

        // Wrap when one full cycle (text + gap) has scrolled by
        let cycle = self.text_width + self.gap;
        if self.scroll_offset >= cycle {
            self.scroll_offset -= cycle;
        }

        true
    }

    /// Get the inner label
    pub fn label(&self) -> &Label {
        &self.label
    }

    /// Get the inner label mutably
    pub fn label_mut(&mut self) -> &mut Label {
        &mut self.label
    }

    /// Get the current scroll offset
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_offset
    }

    /// Set the measured text width (called by WidgetRenderer after text shaping)
    pub fn set_text_width(&mut self, width: f32) {
        self.text_width = width;
    }

    /// Get the measured text width
    pub fn text_width(&self) -> f32 {
        self.text_width
    }

    /// Get the gap between repeated text
    pub fn gap(&self) -> f32 {
        self.gap
    }

    /// Get the scroll speed
    pub fn scroll_speed(&self) -> f32 {
        self.scroll_speed
    }

    /// Check if visible
    pub fn is_visible(&self) -> bool {
        self.label.is_visible()
    }

    /// Set visibility
    pub fn set_visible(&mut self, visible: bool) {
        self.label.set_visible(visible);
    }

    /// Set the text
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.label.set_text(text);
        // Reset scroll when text changes
        self.scroll_offset = 0.0;
        self.text_width = 0.0;
    }
}
