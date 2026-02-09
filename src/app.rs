//! Application state - combines UI and business logic

use ghost_callout::{ArrowPosition, Callout, CalloutStyle, ShapeRenderer, TextAnimation, TextRenderer};
use ghost_ui::{Button, GhostApp, GhostEvent, GpuResources};
use std::time::Duration;

use crate::actions;
use crate::config::{Anchor, Config};
use crate::ui;

/// Main application state
pub struct App {
    config: Config,
    buttons: Vec<Button>,
    callout: Callout,
    skin_size: (u32, u32),
    /// Skin offset within the window (for expanded windows that accommodate callouts)
    skin_offset: [f32; 2],
    // GPU renderers (initialized lazily)
    shape_renderer: Option<ShapeRenderer>,
    text_renderer: Option<TextRenderer>,
}

impl App {
    /// Create new app from configuration
    /// skin_offset is the position of the skin within the window (for expanded windows)
    pub fn new(config: Config, skin_width: u32, skin_height: u32, skin_offset: [f32; 2]) -> Self {
        let buttons = ui::create_buttons_from_config(&config.buttons);
        let callout = Self::create_callout_from_config(&config, skin_width, skin_height, skin_offset);

        Self {
            config,
            buttons,
            callout,
            skin_size: (skin_width, skin_height),
            skin_offset,
            shape_renderer: None,
            text_renderer: None,
        }
    }

    /// Create callout from configuration
    fn create_callout_from_config(
        config: &Config,
        skin_width: u32,
        skin_height: u32,
        skin_offset: [f32; 2],
    ) -> Callout {
        let anchor = Anchor::from_str(&config.callout.anchor);
        let (mut position, arrow) = Self::calculate_position(
            anchor,
            config.callout.offset,
            config.callout.max_width,
            skin_width as f32,
            skin_height as f32,
        );

        // Add skin offset to position (callout position is relative to skin, not window)
        position[0] += skin_offset[0];
        position[1] += skin_offset[1];

        // Create style with configured font size
        let style = CalloutStyle {
            background: config.callout.style.background,
            text_color: config.callout.style.text_color,
            font_size: config.callout.font_size,
            padding: config.callout.style.padding,
            border_radius: config.callout.style.border_radius,
            ..Default::default()
        };

        // Parse animation
        let animation = match config.callout.animation.as_str() {
            "instant" => TextAnimation::Instant,
            "word-by-word" | "wordbyword" => TextAnimation::WordByWord {
                wps: config.callout.animation_speed,
            },
            "stream" => TextAnimation::Stream {
                cps: config.callout.animation_speed,
            },
            _ => TextAnimation::Typewriter {
                cps: config.callout.animation_speed,
            },
        };

        let mut callout = Callout::new()
            .with_position(position[0], position[1])
            .with_arrow(arrow)
            .with_max_width(config.callout.max_width)
            .with_text_animation(animation)
            .with_style(style);

        if config.callout.duration > 0.0 {
            callout = callout.with_duration(Duration::from_secs_f32(config.callout.duration));
        }

        callout
    }

    /// Calculate callout position based on anchor
    ///
    /// The anchor specifies which point of the SKIN the callout is positioned relative to.
    /// For example:
    /// - TopRight: Callout appears near the top-right corner of the skin
    /// - CenterLeft: Callout appears to the left of the skin, vertically centered
    ///
    /// The offset is then applied from this anchor point.
    fn calculate_position(
        anchor: Anchor,
        offset: [f32; 2],
        _callout_width: f32,
        skin_width: f32,
        skin_height: f32,
    ) -> ([f32; 2], ArrowPosition) {
        let (anchor_x, anchor_y) = anchor.as_fraction();

        // Calculate base position from anchor
        let base_x = skin_width * anchor_x;
        let base_y = skin_height * anchor_y;

        // Apply offset
        let x = base_x + offset[0];
        let y = base_y + offset[1];

        // Determine arrow position based on anchor
        let arrow = match anchor {
            // Top anchors - arrow points down toward skin
            Anchor::TopLeft => ArrowPosition::Bottom(0.2),
            Anchor::TopCenter => ArrowPosition::Bottom(0.5),
            Anchor::TopRight => ArrowPosition::Bottom(0.8),

            // Center anchors - arrow points horizontally toward skin
            Anchor::CenterLeft => ArrowPosition::Right(0.5),
            Anchor::CenterCenter => ArrowPosition::Bottom(0.5),
            Anchor::CenterRight => ArrowPosition::Left(0.5),

            // Bottom anchors - arrow points up toward skin
            Anchor::BottomLeft => ArrowPosition::Top(0.2),
            Anchor::BottomCenter => ArrowPosition::Top(0.5),
            Anchor::BottomRight => ArrowPosition::Top(0.8),
        };

        ([x, y], arrow)
    }

    /// Get reference to config
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Update callout position (e.g., after window resize)
    fn update_callout_position(&mut self) {
        let anchor = Anchor::from_str(&self.config.callout.anchor);
        let (position, _arrow) = Self::calculate_position(
            anchor,
            self.config.callout.offset,
            self.config.callout.max_width,
            self.skin_size.0 as f32,
            self.skin_size.1 as f32,
        );
        // Add skin offset to position
        self.callout.set_position(
            position[0] + self.skin_offset[0],
            position[1] + self.skin_offset[1],
        );
    }
}

impl GhostApp for App {
    fn init_gpu(&mut self, gpu: GpuResources<'_>) {
        log::info!("Initializing GPU resources for callout rendering");
        self.shape_renderer = Some(ShapeRenderer::new(gpu.device, gpu.format));
        self.text_renderer = Some(TextRenderer::new(gpu.device, gpu.queue, gpu.format));

        // Initialize callout with GPU resources
        self.callout.init(gpu.device, gpu.queue, gpu.format);
    }

    fn on_event(&mut self, event: GhostEvent) {
        match event {
            GhostEvent::ButtonClicked(id) => {
                // Find which button was clicked by ID
                for btn_config in &self.config.buttons {
                    if ui::get_button_id(&btn_config.id) == id {
                        actions::on_button_click(&btn_config.id, &mut self.callout);
                        break;
                    }
                }
            }
            GhostEvent::Update(delta) => {
                self.callout.update(delta);
            }
            GhostEvent::Resized(width, height) => {
                self.skin_size = (width, height);
                self.update_callout_position();
            }
            _ => {}
        }
    }

    fn buttons(&self) -> Vec<&Button> {
        self.buttons.iter().collect()
    }

    fn buttons_mut(&mut self) -> Vec<&mut Button> {
        self.buttons.iter_mut().collect()
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, viewport: [f32; 2], scale_factor: f32) {
        // Prepare callout for rendering
        if self.callout.is_visible() {
            self.callout.prepare(device, queue, viewport, scale_factor);
        }
    }

    fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        // Render callout if visible
        if self.callout.is_visible() {
            self.callout.render(render_pass);
        }
    }
}
