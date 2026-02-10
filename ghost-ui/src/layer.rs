//! Layer system for rendering overlay images with optional text

use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer,
};
use wgpu::{BindGroup, Device, MultisampleState, Queue, RenderPass, TextureFormat};

use crate::{Skin, SkinData, SkinError, SpritePipeline};

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
    /// Optional text to render on the layer
    pub text: Option<String>,
    /// Text color [r, g, b, a]
    pub text_color: [f32; 4],
    /// Font size for text
    pub font_size: f32,
    /// Z-order (higher = on top)
    pub z_order: i32,
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self {
            anchor: LayerAnchor::BottomCenter,
            offset: [0.0, 0.0],
            text: None,
            text_color: [1.0, 1.0, 1.0, 1.0],
            font_size: 16.0,
            z_order: 0,
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
        let Some(skin) = &self.skin else { return };

        // Scale the position by the display scale factor
        let scaled_position = [
            self.position[0] * scale_factor,
            self.position[1] * scale_factor,
        ];

        log::debug!(
            "Layer prepare: pos={:?}, scaled_pos={:?}, viewport={:?}, scale={}",
            self.position, scaled_position, viewport, scale_factor
        );

        self.bind_group = Some(pipeline.create_bind_group_at_position(
            device,
            queue,
            skin,
            1.0,
            scaled_position,
            viewport,
            scale_factor,
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

    /// Prepare text for a layer
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

        let (layer_width, layer_height) = layer.dimensions();
        let layer_width_scaled = layer_width as f32 * scale_factor;

        self.text_buffer.set_size(
            &mut self.font_system,
            layer_width_scaled,
            line_height * 2.0,
        );

        // Set text with centered alignment
        let attrs = Attrs::new().family(Family::SansSerif);
        self.text_buffer.set_text(
            &mut self.font_system,
            text,
            attrs,
            Shaping::Advanced,
        );

        // Shape the text
        self.text_buffer.shape_until_scroll(&mut self.font_system);

        // Calculate text position (centered on layer)
        let pos = layer.position();
        let layer_center_x = pos[0] * scale_factor + layer_width_scaled / 2.0;
        let layer_center_y = pos[1] * scale_factor + (layer_height as f32 * scale_factor) / 2.0;

        // Get text width for centering
        let text_width: f32 = self.text_buffer
            .layout_runs()
            .map(|run| run.line_w)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);

        let text_x = layer_center_x - text_width / 2.0;
        let text_y = layer_center_y - line_height / 2.0;

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
