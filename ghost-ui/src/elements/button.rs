//! Button widget for ghost-ui

use super::{to_screen_coords, Origin, Widget};

/// Unique identifier for a button
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ButtonId(pub u32);

impl ButtonId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Visual state of a button
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonState {
    #[default]
    Normal,
    Hover,
    Pressed,
}

/// Visual style for buttons
#[derive(Debug, Clone, PartialEq)]
pub struct ButtonStyle {
    /// Background color [r, g, b, a] for normal state
    pub background: [f32; 4],
    /// Background color when hovered
    pub background_hover: [f32; 4],
    /// Background color when pressed
    pub background_pressed: [f32; 4],
    /// Text color
    pub text_color: [f32; 4],
    /// Border radius
    pub border_radius: f32,
    /// Font size
    pub font_size: f32,
    /// Padding
    pub padding: f32,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self {
            background: [0.2, 0.2, 0.2, 0.9],
            background_hover: [0.3, 0.3, 0.3, 0.95],
            background_pressed: [0.15, 0.15, 0.15, 1.0],
            text_color: [1.0, 1.0, 1.0, 1.0],
            border_radius: 4.0,
            font_size: 14.0,
            padding: 8.0,
        }
    }
}

impl ButtonStyle {
    /// Create a light theme button style
    pub fn light() -> Self {
        Self {
            background: [0.9, 0.9, 0.9, 0.95],
            background_hover: [0.95, 0.95, 0.95, 1.0],
            background_pressed: [0.8, 0.8, 0.8, 1.0],
            text_color: [0.1, 0.1, 0.1, 1.0],
            ..Default::default()
        }
    }

    /// Create a primary (accent) button style
    pub fn primary() -> Self {
        Self {
            background: [0.2, 0.5, 0.9, 0.95],
            background_hover: [0.3, 0.6, 1.0, 1.0],
            background_pressed: [0.15, 0.4, 0.8, 1.0],
            text_color: [1.0, 1.0, 1.0, 1.0],
            ..Default::default()
        }
    }
}

/// A clickable button widget
#[derive(Debug, Clone)]
pub struct Button {
    /// Unique identifier
    id: ButtonId,
    /// Position in local coordinates (bottom-left origin by default)
    position: [f32; 2],
    /// Size [width, height]
    size: [f32; 2],
    /// Button label
    label: String,
    /// Current visual state
    state: ButtonState,
    /// Visual style
    style: ButtonStyle,
    /// Coordinate origin
    origin: Origin,
    /// Whether button is visible
    visible: bool,
}

impl Button {
    /// Create a new button
    pub fn new(id: ButtonId, label: impl Into<String>) -> Self {
        Self {
            id,
            position: [0.0, 0.0],
            size: [80.0, 32.0],
            label: label.into(),
            state: ButtonState::Normal,
            style: ButtonStyle::default(),
            origin: Origin::BottomLeft,
            visible: true,
        }
    }

    /// Set the button position (relative to origin)
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.position = [x, y];
        self
    }

    /// Set the button size
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = [width, height];
        self
    }

    /// Set the button style
    pub fn with_style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// Set the coordinate origin
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    /// Get the button ID
    pub fn id(&self) -> ButtonId {
        self.id
    }

    /// Get the button label
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Set the button label
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    /// Get the current state
    pub fn state(&self) -> ButtonState {
        self.state
    }

    /// Get the style
    pub fn style(&self) -> &ButtonStyle {
        &self.style
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

    /// Get the current background color based on state
    pub fn current_background(&self) -> [f32; 4] {
        match self.state {
            ButtonState::Normal => self.style.background,
            ButtonState::Hover => self.style.background_hover,
            ButtonState::Pressed => self.style.background_pressed,
        }
    }

    /// Check if a point (in screen coordinates) is inside the button
    fn contains_point(&self, screen_x: f32, screen_y: f32, window_height: f32) -> bool {
        if !self.visible {
            return false;
        }

        let (bx, by) = to_screen_coords(
            self.position[0],
            self.position[1],
            self.size[0],
            self.size[1],
            window_height,
            self.origin,
        );

        screen_x >= bx
            && screen_x <= bx + self.size[0]
            && screen_y >= by
            && screen_y <= by + self.size[1]
    }
}

impl Widget for Button {
    fn update_hover(&mut self, cursor_x: f32, cursor_y: f32, window_height: f32) {
        if !self.visible {
            return;
        }

        let is_inside = self.contains_point(cursor_x, cursor_y, window_height);

        self.state = match self.state {
            ButtonState::Pressed => ButtonState::Pressed, // Keep pressed until release
            _ if is_inside => ButtonState::Hover,
            _ => ButtonState::Normal,
        };
    }

    fn handle_press(&mut self, cursor_x: f32, cursor_y: f32, window_height: f32) -> bool {
        if !self.visible {
            return false;
        }

        if self.contains_point(cursor_x, cursor_y, window_height) {
            self.state = ButtonState::Pressed;
            true
        } else {
            false
        }
    }

    fn handle_release(&mut self, cursor_x: f32, cursor_y: f32, window_height: f32) -> bool {
        if !self.visible {
            return false;
        }

        let was_pressed = self.state == ButtonState::Pressed;
        let is_inside = self.contains_point(cursor_x, cursor_y, window_height);

        self.state = if is_inside {
            ButtonState::Hover
        } else {
            ButtonState::Normal
        };

        // Click completed if was pressed and released inside
        was_pressed && is_inside
    }

    fn screen_bounds(&self, window_height: f32) -> [f32; 4] {
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
}
