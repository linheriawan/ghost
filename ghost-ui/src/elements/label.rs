//! Label widget for ghost-ui
//!
//! Non-interactive text display with optional rounded-rectangle background.

use super::{to_screen_coords, Origin};

/// Unique identifier for a label
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LabelId(pub u32);

impl LabelId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Font style for label text
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Bold,
    Italic,
}

/// Visual style for labels
#[derive(Debug, Clone, PartialEq)]
pub struct LabelStyle {
    /// Background color [r, g, b, a] (use alpha=0 for no background)
    pub background: [f32; 4],
    /// Text color [r, g, b, a]
    pub text_color: [f32; 4],
    /// Font size in pixels
    pub font_size: f32,
    /// Font style (normal, bold, italic)
    pub font_style: FontStyle,
    /// CSS-like border radius for background
    pub border_radius: f32,
    /// Padding around text [horizontal, vertical]
    pub padding: [f32; 2],
}

impl Default for LabelStyle {
    fn default() -> Self {
        Self {
            background: [0.0, 0.0, 0.0, 0.0],
            text_color: [1.0, 1.0, 1.0, 1.0],
            font_size: 14.0,
            font_style: FontStyle::Normal,
            border_radius: 0.0,
            padding: [4.0, 2.0],
        }
    }
}

impl LabelStyle {
    /// Create a style with a semi-transparent dark background
    pub fn with_background() -> Self {
        Self {
            background: [0.1, 0.1, 0.1, 0.8],
            border_radius: 4.0,
            padding: [8.0, 4.0],
            ..Default::default()
        }
    }

    /// Create a style with a colored badge background
    pub fn badge(bg_color: [f32; 4]) -> Self {
        Self {
            background: bg_color,
            border_radius: 12.0,
            padding: [10.0, 4.0],
            font_size: 12.0,
            ..Default::default()
        }
    }
}

/// A non-interactive text label widget
#[derive(Debug, Clone)]
pub struct Label {
    /// Unique identifier
    id: LabelId,
    /// Position in local coordinates
    position: [f32; 2],
    /// Size [width, height]
    size: [f32; 2],
    /// Display text
    text: String,
    /// Visual style
    style: LabelStyle,
    /// Coordinate origin
    origin: Origin,
    /// Whether label is visible
    visible: bool,
}

impl Label {
    /// Create a new label
    pub fn new(id: LabelId, text: impl Into<String>) -> Self {
        Self {
            id,
            position: [0.0, 0.0],
            size: [100.0, 24.0],
            text: text.into(),
            style: LabelStyle::default(),
            origin: Origin::BottomLeft,
            visible: true,
        }
    }

    /// Set the label position (relative to origin)
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.position = [x, y];
        self
    }

    /// Set the label size
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = [width, height];
        self
    }

    /// Set the label style
    pub fn with_style(mut self, style: LabelStyle) -> Self {
        self.style = style;
        self
    }

    /// Set the coordinate origin
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    /// Get the label ID
    pub fn id(&self) -> LabelId {
        self.id
    }

    /// Get the label text
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Set the label text
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    /// Get the style
    pub fn style(&self) -> &LabelStyle {
        &self.style
    }

    /// Get the style mutably
    pub fn style_mut(&mut self) -> &mut LabelStyle {
        &mut self.style
    }

    /// Check if visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Set visibility
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Get position in local coordinates
    pub fn position(&self) -> [f32; 2] {
        self.position
    }

    /// Set position
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.position = [x, y];
    }

    /// Get size
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    /// Set size
    pub fn set_size(&mut self, width: f32, height: f32) {
        self.size = [width, height];
    }

    /// Get the origin
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Get the screen bounds [x, y, width, height]
    pub fn screen_bounds(&self, window_height: f32) -> [f32; 4] {
        let (x, y) = to_screen_coords(
            self.position[0],
            self.position[1],
            self.size[0],
            self.size[1],
            window_height,
            self.origin,
        );
        [x, y, self.size[0], self.size[1]]
    }

    /// Check if background should be rendered (alpha > 0)
    pub fn has_background(&self) -> bool {
        self.style.background[3] > 0.0
    }
}
