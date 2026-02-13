//! Image-based button widget for ghost-ui

use super::{to_screen_coords, Origin, Widget};
use crate::elements::button::{ButtonId, ButtonState};
use crate::skin::{Skin, SkinData, SkinError};
use wgpu::{Device, Queue};

/// A clickable button that renders a PNG image instead of a colored rectangle.
/// Brightness is modified based on interaction state:
/// - Normal: 1.0
/// - Hover: 1.15 (brighter)
/// - Pressed: 0.9 (darker)
#[derive(Debug)]
pub struct ButtonImage {
    /// Unique identifier
    id: ButtonId,
    /// Position in local coordinates
    position: [f32; 2],
    /// Display size [width, height] (defaults to image dimensions)
    size: [f32; 2],
    /// Current visual state
    state: ButtonState,
    /// Coordinate origin
    origin: Origin,
    /// Whether button is visible
    visible: bool,
    /// Image data (before GPU init)
    skin_data: SkinData,
    /// GPU texture (created on init_gpu)
    skin: Option<Skin>,
}

impl ButtonImage {
    /// Create a new image button from SkinData
    pub fn new(id: ButtonId, skin_data: SkinData) -> Self {
        let size = [skin_data.width() as f32, skin_data.height() as f32];
        Self {
            id,
            position: [0.0, 0.0],
            size,
            state: ButtonState::Normal,
            origin: Origin::BottomLeft,
            visible: true,
            skin_data,
            skin: None,
        }
    }

    /// Create from a file path
    pub fn from_path(id: ButtonId, path: &str) -> Result<Self, SkinError> {
        let skin_data = SkinData::from_path(path)?;
        Ok(Self::new(id, skin_data))
    }

    /// Set the button position (relative to origin)
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.position = [x, y];
        self
    }

    /// Set the display size (overrides image dimensions)
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = [width, height];
        self
    }

    /// Set the coordinate origin
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    /// Initialize GPU resources
    pub fn init_gpu(&mut self, device: &Device, queue: &Queue) {
        if self.skin.is_some() {
            return;
        }
        match Skin::from_skin_data(&self.skin_data, device, queue) {
            Ok(skin) => {
                self.skin = Some(skin);
            }
            Err(e) => {
                log::error!("Failed to create ButtonImage skin: {}", e);
            }
        }
    }

    /// Get the button ID
    pub fn id(&self) -> ButtonId {
        self.id
    }

    /// Get the current state
    pub fn state(&self) -> ButtonState {
        self.state
    }

    /// Get the brightness modifier based on current state
    pub fn brightness(&self) -> f32 {
        match self.state {
            ButtonState::Normal => 1.0,
            ButtonState::Hover => 1.15,
            ButtonState::Pressed => 0.9,
        }
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

    /// Get the GPU skin (texture) if initialized
    pub fn skin(&self) -> Option<&Skin> {
        self.skin.as_ref()
    }

    /// Get the origin
    pub fn origin(&self) -> Origin {
        self.origin
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

impl Widget for ButtonImage {
    fn update_hover(&mut self, cursor_x: f32, cursor_y: f32, window_height: f32) {
        if !self.visible {
            return;
        }

        let is_inside = self.contains_point(cursor_x, cursor_y, window_height);

        self.state = match self.state {
            ButtonState::Pressed => ButtonState::Pressed,
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

impl std::fmt::Debug for Skin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Skin")
            .field("width", &self.width())
            .field("height", &self.height())
            .finish()
    }
}
