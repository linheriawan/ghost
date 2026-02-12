//! Layer system for rendering overlay images with optional text

use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer,
};
use wgpu::{BindGroup, Device, MultisampleState, Queue, RenderPass, TextureFormat};

use crate::{Skin, SkinData, SkinError, SpritePipeline};

/// Text alignment options
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TextAlign {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "left" => Self::Left,
            "center" => Self::Center,
            "right" => Self::Right,
            _ => {
                log::warn!("Unknown text align '{}', defaulting to center", s);
                Self::Center
            }
        }
    }
}

/// Vertical alignment options
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextVAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

impl TextVAlign {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "top" => Self::Top,
            "center" => Self::Center,
            "bottom" => Self::Bottom,
            _ => {
                log::warn!("Unknown text valign '{}', defaulting to center", s);
                Self::Center
            }
        }
    }
}

/// Position anchor for layers
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayerAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl LayerAnchor {
    /// Parse anchor from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "top-left" | "topleft" => Self::TopLeft,
            "top-center" | "topcenter" | "top" => Self::TopCenter,
            "top-right" | "topright" => Self::TopRight,
            "center-left" | "centerleft" | "left" => Self::CenterLeft,
            "center-center" | "centercenter" | "center" => Self::Center,
            "center-right" | "centerright" | "right" => Self::CenterRight,
            "bottom-left" | "bottomleft" => Self::BottomLeft,
            "bottom-center" | "bottomcenter" | "bottom" => Self::BottomCenter,
            "bottom-right" | "bottomright" => Self::BottomRight,
            _ => {
                log::warn!("Unknown layer anchor '{}', defaulting to bottom-center", s);
                Self::BottomCenter
            }
        }
    }

    /// Get the anchor position as a fraction (0.0 to 1.0)
    pub fn as_fraction(&self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::TopCenter => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::CenterLeft => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
            Self::CenterRight => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::BottomCenter => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }
}

/// Configuration for a layer
#[derive(Debug, Clone)]
pub struct LayerConfig {
    /// Anchor point relative to parent skin
    pub anchor: LayerAnchor,
    /// Offset from anchor in pixels [x, y]
    pub offset: [f32; 2],
    /// Optional size override [width, height] in pixels (None = use image size)
    pub size: Option<[f32; 2]>,
    /// Optional text to render on the layer
    pub text: Option<String>,
    /// Text color [r, g, b, a]
    pub text_color: [f32; 4],
    /// Font size for text
    pub font_size: f32,
    /// Z-order (higher = on top)
    pub z_order: i32,
    /// Text horizontal alignment: "left", "center", "right"
    pub text_align: TextAlign,
    /// Text vertical alignment: "top", "center", "bottom"
    pub text_valign: TextVAlign,
    /// Text offset from layer origin [x, y] in pixels
    /// The origin is the top-left corner of the layer when text_align/valign are left/top
    pub text_offset: [f32; 2],
    /// Padding from layer edges [left, right, top, bottom]
    pub text_padding: [f32; 4],
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self {
            anchor: LayerAnchor::BottomCenter,
            offset: [0.0, 0.0],
            size: None,
            text: None,
            text_color: [1.0, 1.0, 1.0, 1.0],
            font_size: 16.0,
            z_order: 0,
            text_align: TextAlign::Center,
            text_valign: TextVAlign::Center,
            text_offset: [0.0, 0.0],
            text_padding: [8.0, 8.0, 8.0, 8.0], // left, right, top, bottom
        }
    }
}

/// A layer that can be rendered on top of the skin
pub struct Layer {
    /// Layer image data (loaded but not yet on GPU)
    skin_data: SkinData,
    /// GPU skin (created after GPU init)
    skin: Option<Skin>,
    /// Layer configuration
    pub config: LayerConfig,
    /// Computed position [x, y] in pixels relative to window origin
    position: [f32; 2],
    /// Prepared bind group for rendering
    bind_group: Option<BindGroup>,
}

impl Layer {
    /// Create a new layer from image data
    pub fn new(skin_data: SkinData, config: LayerConfig) -> Self {
        Self {
            skin_data,
            skin: None,
            config,
            position: [0.0, 0.0],
            bind_group: None,
        }
    }

    /// Load a layer from a file path
    pub fn from_path(path: &str, config: LayerConfig) -> Result<Self, SkinError> {
        let skin_data = SkinData::from_path(path)?;
        Ok(Self::new(skin_data, config))
    }

    /// Initialize GPU resources for this layer
    pub fn init_gpu(&mut self, device: &Device, queue: &Queue) {
        if self.skin.is_none() {
            match Skin::from_skin_data(&self.skin_data, device, queue) {
                Ok(skin) => {
                    self.skin = Some(skin);
                    log::info!("Layer GPU initialized: {}x{}", self.skin_data.width(), self.skin_data.height());
                }
                Err(e) => {
                    log::error!("Failed to create layer skin: {}", e);
                }
            }
        }
    }

    /// Calculate the layer position based on parent skin dimensions
    pub fn calculate_position(&mut self, parent_width: u32, parent_height: u32) {
        let (anchor_x, anchor_y) = self.config.anchor.as_fraction();

        // Calculate anchor point on parent
        let anchor_px = parent_width as f32 * anchor_x;
        let anchor_py = parent_height as f32 * anchor_y;

        // Center the layer on the anchor point
        let layer_width = self.skin_data.width() as f32;
        let layer_height = self.skin_data.height() as f32;

        // Position so layer is centered on anchor
        let x = anchor_px - (layer_width * anchor_x) + self.config.offset[0];
        let y = anchor_py - (layer_height * anchor_y) + self.config.offset[1];

        self.position = [x, y];
    }

    /// Get the layer's skin for rendering
    pub fn skin(&self) -> Option<&Skin> {
        self.skin.as_ref()
    }

    /// Get the computed position
    pub fn position(&self) -> [f32; 2] {
        self.position
    }

    /// Get layer dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        self.skin_data.dimensions()
    }

    /// Get the text to render (if any)
    pub fn text(&self) -> Option<&str> {
        self.config.text.as_deref()
    }

    /// Prepare the layer for rendering - creates bind group with current position
    pub fn prepare(
        &mut self,
        pipeline: &SpritePipeline,
        device: &Device,
        queue: &Queue,
        viewport: [f32; 2],
        scale_factor: f32,
    ) {
        self.prepare_with_opacity(pipeline, device, queue, viewport, scale_factor, 1.0);
    }

    /// Prepare the layer for rendering with custom opacity
    pub fn prepare_with_opacity(
        &mut self,
        pipeline: &SpritePipeline,
        device: &Device,
        queue: &Queue,
        viewport: [f32; 2],
        scale_factor: f32,
        opacity: f32,
    ) {
        let Some(skin) = &self.skin else { return };

        // Scale the position by the display scale factor
        let scaled_position = [
            self.position[0] * scale_factor,
            self.position[1] * scale_factor,
        ];

        log::debug!(
            "Layer prepare: pos={:?}, scaled_pos={:?}, viewport={:?}, scale={}, opacity={}, size={:?}",
            self.position, scaled_position, viewport, scale_factor, opacity, self.config.size
        );

        self.bind_group = Some(pipeline.create_bind_group_at_position_with_size(
            device,
            queue,
            skin,
            opacity,
            scaled_position,
            viewport,
            scale_factor,
            self.config.size,
        ));
    }

    /// Get the bind group for rendering
    pub fn bind_group(&self) -> Option<&BindGroup> {
        self.bind_group.as_ref()
    }
}

/// Renderer for layers with text support
pub struct LayerRenderer {
    // Text rendering resources
    font_system: FontSystem,
    swash_cache: SwashCache,
    text_atlas: Option<TextAtlas>,
    text_renderer: Option<GlyphonTextRenderer>,
    text_buffer: Buffer,
    initialized: bool,
}

impl LayerRenderer {
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let text_buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 20.0));

        Self {
            font_system,
            swash_cache,
            text_atlas: None,
            text_renderer: None,
            text_buffer,
            initialized: false,
        }
    }

    /// Initialize GPU resources
    pub fn init_gpu(&mut self, device: &Device, queue: &Queue, format: TextureFormat) {
        if self.initialized {
            return;
        }

        let mut text_atlas = TextAtlas::new(device, queue, format);
        let text_renderer = GlyphonTextRenderer::new(
            &mut text_atlas,
            device,
            MultisampleState::default(),
            None,
        );

        self.text_atlas = Some(text_atlas);
        self.text_renderer = Some(text_renderer);
        self.initialized = true;

        log::info!("LayerRenderer GPU initialized");
    }

    /// Clear text state at the beginning of each frame.
    /// Must be called before any `prepare_text` calls to prevent stale text from persisting.
    pub fn begin_frame(&mut self, device: &Device, queue: &Queue, viewport: [f32; 2]) {
        let Some(atlas) = &mut self.text_atlas else { return };
        let Some(renderer) = &mut self.text_renderer else { return };

        let resolution = Resolution {
            width: viewport[0] as u32,
            height: viewport[1] as u32,
        };

        // Prepare with empty text areas to clear the renderer's vertex buffer
        let _ = renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            atlas,
            resolution,
            std::iter::empty::<TextArea>(),
            &mut self.swash_cache,
        );
    }

    /// Prepare text for a layer
    ///
    /// Text positioning is relative to the layer:
    /// - text_offset [x, y] is relative to the layer's top-left corner
    /// - text_align determines horizontal alignment within the layer
    /// - text_valign determines vertical alignment within the layer
    /// - text_padding provides spacing from edges [left, right, top, bottom]
    pub fn prepare_text(
        &mut self,
        device: &Device,
        queue: &Queue,
        layer: &Layer,
        viewport: [f32; 2],
        scale_factor: f32,
    ) {
        let Some(text) = layer.text() else { return };
        let Some(atlas) = &mut self.text_atlas else { return };
        let Some(renderer) = &mut self.text_renderer else { return };

        let font_size = layer.config.font_size * scale_factor;
        let line_height = font_size * 1.2;

        // Set up text buffer
        self.text_buffer.set_metrics(
            &mut self.font_system,
            Metrics::new(font_size, line_height),
        );

        // Get layer dimensions (use configured size if available, else actual image size)
        let (img_width, img_height) = layer.dimensions();
        let (layer_width, layer_height) = layer.config.size
            .map(|s| (s[0] as u32, s[1] as u32))
            .unwrap_or((img_width, img_height));

        let layer_width_scaled = layer_width as f32 * scale_factor;
        let layer_height_scaled = layer_height as f32 * scale_factor;

        // Scale padding
        let [pad_left, pad_right, pad_top, pad_bottom] = layer.config.text_padding;
        let pad_left = pad_left * scale_factor;
        let pad_right = pad_right * scale_factor;
        let pad_top = pad_top * scale_factor;
        let pad_bottom = pad_bottom * scale_factor;

        // Available text area within padding
        let text_area_width = layer_width_scaled - pad_left - pad_right;
        let text_area_height = layer_height_scaled - pad_top - pad_bottom;

        self.text_buffer.set_size(
            &mut self.font_system,
            text_area_width.max(1.0),
            text_area_height.max(line_height),
        );

        // Set text
        let attrs = Attrs::new().family(Family::SansSerif);
        self.text_buffer.set_text(
            &mut self.font_system,
            text,
            attrs,
            Shaping::Advanced,
        );

        // Shape the text
        self.text_buffer.shape_until_scroll(&mut self.font_system);

        // Get text dimensions
        let text_width: f32 = self.text_buffer
            .layout_runs()
            .map(|run| run.line_w)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);
        let text_height = line_height; // Single line for now

        // Calculate layer position in screen coordinates
        let pos = layer.position();
        let layer_x = pos[0] * scale_factor;
        let layer_y = pos[1] * scale_factor;

        // Apply text offset (scaled)
        let offset_x = layer.config.text_offset[0] * scale_factor;
        let offset_y = layer.config.text_offset[1] * scale_factor;

        // Calculate text X position based on alignment
        let text_x = match layer.config.text_align {
            TextAlign::Left => {
                layer_x + pad_left + offset_x
            }
            TextAlign::Center => {
                layer_x + pad_left + (text_area_width - text_width) / 2.0 + offset_x
            }
            TextAlign::Right => {
                layer_x + layer_width_scaled - pad_right - text_width + offset_x
            }
        };

        // Calculate text Y position based on vertical alignment
        let text_y = match layer.config.text_valign {
            TextVAlign::Top => {
                layer_y + pad_top + offset_y
            }
            TextVAlign::Center => {
                layer_y + pad_top + (text_area_height - text_height) / 2.0 + offset_y
            }
            TextVAlign::Bottom => {
                layer_y + layer_height_scaled - pad_bottom - text_height + offset_y
            }
        };

        // Convert color to glyphon format
        let [r, g, b, a] = layer.config.text_color;
        let color = Color::rgba(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        );

        let text_area = TextArea {
            buffer: &self.text_buffer,
            left: text_x,
            top: text_y,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: viewport[0] as i32,
                bottom: viewport[1] as i32,
            },
            default_color: color,
        };

        let resolution = Resolution {
            width: viewport[0] as u32,
            height: viewport[1] as u32,
        };

        if let Err(e) = renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            atlas,
            resolution,
            [text_area],
            &mut self.swash_cache,
        ) {
            log::error!("Failed to prepare layer text: {:?}", e);
        }
    }

    /// Render text for layers
    pub fn render_text<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        let Some(renderer) = &self.text_renderer else { return };
        let Some(atlas) = &self.text_atlas else { return };

        if let Err(e) = renderer.render(atlas, render_pass) {
            log::error!("Failed to render layer text: {:?}", e);
        }
    }

    /// Trim the text atlas to free unused memory
    pub fn trim(&mut self) {
        if let Some(atlas) = &mut self.text_atlas {
            atlas.trim();
        }
    }
}

impl Default for LayerRenderer {
    fn default() -> Self {
        Self::new()
    }
}
