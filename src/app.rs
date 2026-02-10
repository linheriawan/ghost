//! Application state - combines UI and business logic

use ghost_ui::{Button, GhostApp, GhostEvent, GpuResources, Layer, LayerAnchor, LayerConfig, LayerRenderer, SpritePipeline};
use wgpu::TextureFormat;

use crate::callout_app::{CalloutCommand, CalloutSender};
use crate::config::Config;
use crate::ui;

/// Main application state
pub struct App {
    config: Config,
    buttons: Vec<Button>,
    callout_sender: CalloutSender,
    skin_size: (u32, u32),
    layers: Vec<Layer>,
    layer_renderer: LayerRenderer,
    layer_pipeline: Option<SpritePipeline>,
    texture_format: Option<TextureFormat>,
}

impl App {
    /// Create new app from configuration
    pub fn new(
        config: Config,
        skin_width: u32,
        skin_height: u32,
        callout_sender: CalloutSender,
    ) -> Self {
        let buttons = ui::create_buttons_from_config(&config.buttons);

        // Load layers from config
        let mut layers = Vec::new();
        for layer_config in &config.layers {
            let ghost_config = LayerConfig {
                anchor: LayerAnchor::from_str(&layer_config.anchor),
                offset: layer_config.offset,
                text: layer_config.text.clone(),
                text_color: layer_config.text_color,
                font_size: layer_config.font_size,
                z_order: layer_config.z_order,
            };

            match Layer::from_path(&layer_config.path, ghost_config) {
                Ok(mut layer) => {
                    layer.calculate_position(skin_width, skin_height);
                    log::info!("Loaded layer: {} at position {:?}", layer_config.path, layer.position());
                    layers.push(layer);
                }
                Err(e) => {
                    log::error!("Failed to load layer '{}': {}", layer_config.path, e);
                }
            }
        }

        // Sort layers by z_order
        layers.sort_by_key(|l| l.config.z_order);

        Self {
            config,
            buttons,
            callout_sender,
            skin_size: (skin_width, skin_height),
            layers,
            layer_renderer: LayerRenderer::new(),
            layer_pipeline: None,
            texture_format: None,
        }
    }

    /// Send a callout command
    fn send_callout(&self, cmd: CalloutCommand) {
        if let Err(e) = self.callout_sender.send(cmd) {
            log::error!("Failed to send callout command: {}", e);
        }
    }
}

impl GhostApp for App {
    fn init_gpu(&mut self, gpu: GpuResources<'_>) {
        log::info!("Main window GPU initialized");

        // Initialize layer GPU resources
        for layer in &mut self.layers {
            layer.init_gpu(gpu.device, gpu.queue);
        }

        // Create sprite pipeline for layers
        self.layer_pipeline = Some(SpritePipeline::new(gpu.device, gpu.format));
        self.texture_format = Some(gpu.format);

        // Initialize layer text renderer
        self.layer_renderer.init_gpu(gpu.device, gpu.queue, gpu.format);
    }

    fn on_event(&mut self, event: GhostEvent) {
        match event {
            GhostEvent::ButtonClicked(id) => {
                // Find which button was clicked by ID
                for btn_config in &self.config.buttons {
                    if ui::get_button_id(&btn_config.id) == id {
                        // Send command to callout window based on button ID
                        match btn_config.id.as_str() {
                            "greet" => {
                                self.send_callout(CalloutCommand::Say("Hi, how are you today?".to_string()));
                                log::info!("Action: Greeting");
                            }
                            "think" => {
                                self.send_callout(CalloutCommand::Think("Hmm, let me think about that...".to_string()));
                                log::info!("Action: Thinking");
                            }
                            "scream" => {
                                self.send_callout(CalloutCommand::Scream("WATCH OUT!".to_string()));
                                log::info!("Action: Screaming");
                            }
                            _ => {
                                self.send_callout(CalloutCommand::Say(format!("Button '{}' clicked!", btn_config.id)));
                                log::info!("Action: Unknown button '{}'", btn_config.id);
                            }
                        }
                        break;
                    }
                }
            }
            GhostEvent::Resized(width, height) => {
                self.skin_size = (width, height);
                // Recalculate layer positions
                for layer in &mut self.layers {
                    layer.calculate_position(width, height);
                }
            }
            GhostEvent::Moved(_x, _y) => {
                // Main window moved - callout window position is updated by the event loop
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
        // Prepare layer bind groups
        if let Some(pipeline) = &self.layer_pipeline {
            for layer in &mut self.layers {
                layer.prepare(pipeline, device, queue, viewport, scale_factor);
            }
        }

        // Prepare layer text rendering
        for layer in &self.layers {
            if layer.text().is_some() {
                self.layer_renderer.prepare_text(device, queue, layer, viewport, scale_factor);
            }
        }
    }

    fn render_layers<'a>(
        &'a mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _viewport: [f32; 2],
        render_pass: &mut wgpu::RenderPass<'a>,
    ) {
        // Render layer images
        if let Some(pipeline) = &self.layer_pipeline {
            for layer in &self.layers {
                if let Some(bind_group) = layer.bind_group() {
                    pipeline.render_bind_group(render_pass, bind_group);
                }
            }
        }

        // Render layer text
        self.layer_renderer.render_text(render_pass);
    }
}
