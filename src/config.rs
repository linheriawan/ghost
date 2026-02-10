//! Configuration loading from ui.toml

use serde::Deserialize;
use std::path::Path;

/// Root configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub skin: SkinConfig,
    pub callout: CalloutConfig,
    #[serde(default)]
    pub buttons: Vec<ButtonConfig>,
    #[serde(default)]
    pub layers: Vec<LayerConfig>,
}

/// Skin/background configuration
#[derive(Debug, Clone, Deserialize)]
pub struct SkinConfig {
    /// Path to the skin image or animation directory
    pub path: String,
    /// Whether this is an animated skin (directory of frames)
    #[serde(default)]
    pub animated: bool,
    /// Frames per second for animated skins
    #[serde(default = "default_skin_fps")]
    pub fps: f32,
}

fn default_skin_fps() -> f32 {
    24.0
}

/// Callout configuration
#[derive(Debug, Clone, Deserialize)]
pub struct CalloutConfig {
    /// Anchor point: "top-left", "top-center", "top-right", etc.
    pub anchor: String,
    /// Offset from anchor [x, y]
    pub offset: [f32; 2],
    /// Maximum width
    pub max_width: f32,
    /// Font size
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Animation type
    #[serde(default = "default_animation")]
    pub animation: String,
    /// Animation speed (cps or wps)
    #[serde(default = "default_animation_speed")]
    pub animation_speed: f32,
    /// Duration in seconds (0 = permanent)
    #[serde(default = "default_duration")]
    pub duration: f32,
    /// Style options
    #[serde(default)]
    pub style: CalloutStyleConfig,
}

fn default_font_size() -> f32 {
    16.0
}
fn default_animation() -> String {
    "typewriter".to_string()
}
fn default_animation_speed() -> f32 {
    30.0
}
fn default_duration() -> f32 {
    5.0
}

/// Callout style configuration
#[derive(Debug, Clone, Deserialize)]
pub struct CalloutStyleConfig {
    #[serde(default = "default_background")]
    pub background: [f32; 4],
    #[serde(default = "default_text_color")]
    pub text_color: [f32; 4],
    #[serde(default = "default_padding")]
    pub padding: f32,
    #[serde(default = "default_border_radius")]
    pub border_radius: f32,
}

impl Default for CalloutStyleConfig {
    fn default() -> Self {
        Self {
            background: default_background(),
            text_color: default_text_color(),
            padding: default_padding(),
            border_radius: default_border_radius(),
        }
    }
}

fn default_background() -> [f32; 4] {
    [1.0, 1.0, 1.0, 0.95]
}
fn default_text_color() -> [f32; 4] {
    [0.1, 0.1, 0.1, 1.0]
}
fn default_padding() -> f32 {
    14.0
}
fn default_border_radius() -> f32 {
    10.0
}

/// Button configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ButtonConfig {
    pub id: String,
    pub label: String,
    pub position: [f32; 2],
    #[serde(default = "default_button_size")]
    pub size: [f32; 2],
    #[serde(default = "default_button_style")]
    pub style: String,
}

/// Layer configuration for overlay images
#[derive(Debug, Clone, Deserialize)]
pub struct LayerConfig {
    /// Path to the layer image
    pub path: String,
    /// Anchor point: "top-left", "bottom-center", etc.
    #[serde(default = "default_layer_anchor")]
    pub anchor: String,
    /// Offset from anchor [x, y] in pixels
    #[serde(default)]
    pub offset: [f32; 2],
    /// Optional text to display on the layer
    pub text: Option<String>,
    /// Text color [r, g, b, a]
    #[serde(default = "default_layer_text_color")]
    pub text_color: [f32; 4],
    /// Font size for text
    #[serde(default = "default_layer_font_size")]
    pub font_size: f32,
    /// Z-order (higher = rendered on top)
    #[serde(default)]
    pub z_order: i32,
}

fn default_layer_anchor() -> String {
    "bottom-center".to_string()
}

fn default_layer_text_color() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_layer_font_size() -> f32 {
    16.0
}

fn default_button_size() -> [f32; 2] {
    [60.0, 28.0]
}
fn default_button_style() -> String {
    "default".to_string()
}

/// Anchor position enum
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    CenterCenter,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Anchor {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "top-left" | "topleft" => Self::TopLeft,
            "top-center" | "topcenter" | "top" => Self::TopCenter,
            "top-right" | "topright" => Self::TopRight,
            "center-left" | "centerleft" | "left" => Self::CenterLeft,
            "center-center" | "centercenter" | "center" => Self::CenterCenter,
            "center-right" | "centerright" | "right" => Self::CenterRight,
            "bottom-left" | "bottomleft" => Self::BottomLeft,
            "bottom-center" | "bottomcenter" | "bottom" => Self::BottomCenter,
            "bottom-right" | "bottomright" => Self::BottomRight,
            _ => {
                log::warn!("Unknown anchor '{}', defaulting to top-right", s);
                Self::TopRight
            }
        }
    }

    /// Get the anchor position as a fraction of skin size (0.0 to 1.0)
    pub fn as_fraction(&self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::TopCenter => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::CenterLeft => (0.0, 0.5),
            Self::CenterCenter => (0.5, 0.5),
            Self::CenterRight => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::BottomCenter => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }
}

/// Window layout information including skin offset
#[derive(Debug, Clone)]
pub struct WindowLayout {
    /// Total window width
    pub window_width: u32,
    /// Total window height
    pub window_height: u32,
    /// Skin offset within window [x, y]
    pub skin_offset: [f32; 2],
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| ConfigError::Io(e.to_string()))?;

        toml::from_str(&content)
            .map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Load from default path (ui.toml in current directory)
    pub fn load_default() -> Result<Self, ConfigError> {
        Self::load("ui.toml")
    }

    /// Calculate the window layout to accommodate both skin and callout
    /// Returns the window size and skin offset within the window
    pub fn calculate_window_layout(&self, skin_width: u32, skin_height: u32) -> WindowLayout {
        let anchor = Anchor::from_str(&self.callout.anchor);
        let (anchor_x, anchor_y) = anchor.as_fraction();

        // Calculate callout position relative to skin
        let callout_x = skin_width as f32 * anchor_x + self.callout.offset[0];
        let callout_y = skin_height as f32 * anchor_y + self.callout.offset[1];

        // Estimate callout size (width is known, height is estimated)
        let callout_width = self.callout.max_width;
        let estimated_callout_height = 200.0; // Conservative estimate

        // Calculate bounding box of callout
        let callout_left = callout_x;
        let callout_right = callout_x + callout_width;
        let callout_top = callout_y;
        let callout_bottom = callout_y + estimated_callout_height;

        // Calculate how much we need to expand in each direction
        let expand_left = (-callout_left).max(0.0);
        let expand_right = (callout_right - skin_width as f32).max(0.0);
        let expand_top = (-callout_top).max(0.0);
        let expand_bottom = (callout_bottom - skin_height as f32).max(0.0);

        // Calculate final window size
        let window_width = (skin_width as f32 + expand_left + expand_right).ceil() as u32;
        let window_height = (skin_height as f32 + expand_top + expand_bottom).ceil() as u32;

        // Skin offset is where the skin should be rendered within the window
        let skin_offset = [expand_left, expand_top];

        log::info!(
            "Window layout: {}x{}, skin offset: {:?}, expand: L={} R={} T={} B={}",
            window_width, window_height, skin_offset,
            expand_left, expand_right, expand_top, expand_bottom
        );

        WindowLayout {
            window_width,
            window_height,
            skin_offset,
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Parse(e) => write!(f, "Parse error: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {}
