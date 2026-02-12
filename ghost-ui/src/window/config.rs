//! Window configuration types

/// Maximum texture size supported by most GPUs.
/// We use a conservative limit to ensure compatibility.
pub(super) const MAX_TEXTURE_SIZE: u32 = 2048;

/// Default alpha threshold for hit testing (0-255).
/// Pixels with alpha <= this value are considered transparent.
pub(super) const DEFAULT_ALPHA_THRESHOLD: u8 = 10;

/// Clamp dimensions to fit within max texture size while maintaining aspect ratio.
pub(super) fn clamp_to_max_size(width: u32, height: u32, max_size: u32) -> (u32, u32) {
    if width <= max_size && height <= max_size {
        return (width, height);
    }

    let aspect_ratio = width as f32 / height as f32;

    if width > height {
        // Width is the limiting factor
        let new_width = max_size;
        let new_height = (new_width as f32 / aspect_ratio).round() as u32;
        (new_width, new_height.min(max_size))
    } else {
        // Height is the limiting factor
        let new_height = max_size;
        let new_width = (new_height as f32 * aspect_ratio).round() as u32;
        (new_width.min(max_size), new_height)
    }
}

/// Configuration for creating a ghost window.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    /// Width of the window in pixels.
    pub width: u32,
    /// Height of the window in pixels.
    pub height: u32,
    /// Whether the window should always stay on top.
    pub always_on_top: bool,
    /// Whether mouse clicks should pass through the window entirely.
    pub click_through: bool,
    /// Whether the window can be dragged.
    pub draggable: bool,
    /// Window title (not visible for borderless windows).
    pub title: String,
    /// Opacity when window is focused (0.0 to 1.0).
    pub opacity_focused: f32,
    /// Opacity when window is unfocused (0.0 to 1.0).
    pub opacity_unfocused: f32,
    /// Whether to maintain aspect ratio when resizing.
    pub maintain_aspect_ratio: bool,
    /// Whether to use alpha-based hit testing (clicks on transparent areas pass through).
    pub alpha_hit_test: bool,
    /// Alpha threshold for hit testing (0-255). Pixels with alpha <= this are transparent.
    pub alpha_threshold: u8,
    /// Whether to change opacity based on focus state.
    pub focus_opacity_enabled: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 200,
            height: 200,
            always_on_top: true,
            click_through: false,
            draggable: true,
            title: "Ghost".to_string(),
            opacity_focused: 1.0,
            opacity_unfocused: 0.5,
            maintain_aspect_ratio: true,
            alpha_hit_test: true,
            alpha_threshold: DEFAULT_ALPHA_THRESHOLD,
            focus_opacity_enabled: true,
        }
    }
}

/// Configuration for a linked callout window
pub struct CalloutWindowConfig {
    /// Offset from main window position [x, y]
    pub offset: [i32; 2],
    /// Size of the callout window
    pub size: (u32, u32),
}
