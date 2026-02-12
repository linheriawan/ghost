//! Application state - combines UI and business logic

use std::sync::mpsc;

use ghost_ui::{
    AnimatedSkin, AnimationState, Button, GhostApp, GhostEvent, GpuResources, Layer, LayerAnchor,
    LayerConfig, LayerRenderer, PersonaMeta, Skin, SkinData, SpritePipeline, TextAlign, TextVAlign,
};
use wgpu::TextureFormat;

use super::callout_window::{CalloutCommand, CalloutSender};
use super::chat_window::{ChatSender, ChatWindowCommand};
use crate::config::Config;
use crate::tray::{self, MenuIds, TrayCommand};
use crate::ui;

/// Skin loading state for lazy loading from .persona.zip
enum SkinLoadState {
    /// Background thread is loading animation frames
    Loading {
        receiver: mpsc::Receiver<AnimatedSkin>,
    },
    /// Animation is loaded and ready
    Ready,
    /// Not using animated skin (static image)
    Static,
}

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
    /// Animated skin (if using frame sequences)
    animated_skin: Option<AnimatedSkin>,
    /// Tray menu IDs for event handling
    menu_ids: Option<MenuIds>,
    /// Flag to signal quit
    should_quit: bool,
    /// Chat window sender (to send commands to chat window)
    chat_sender: ChatSender,
    /// Lazy loading state
    load_state: SkinLoadState,
    /// Still image skin displayed during loading
    still_skin: Option<Skin>,
    /// Still image data (kept until GPU init creates the Skin)
    still_skin_data: Option<SkinData>,
    /// Flag for deferred GPU init after background load completes
    needs_gpu_reinit: bool,
    /// Loading indicator overlay
    loading_layer: Option<Layer>,
}

impl App {
    /// Create new app from configuration
    pub fn new(
        config: Config,
        skin_width: u32,
        skin_height: u32,
        callout_sender: CalloutSender,
        animated_skin: Option<AnimatedSkin>,
        chat_sender: ChatSender,
        persona_meta: Option<PersonaMeta>,
        skin_load_receiver: Option<mpsc::Receiver<AnimatedSkin>>,
    ) -> Self {
        let buttons = ui::create_buttons_from_config(&config.buttons);

        // Load layers from config
        let mut layers = Vec::new();
        for layer_config in &config.layers {
            let ghost_config = LayerConfig {
                anchor: LayerAnchor::from_str(&layer_config.anchor),
                offset: layer_config.offset,
                size: layer_config.size,
                text: layer_config.text.clone(),
                text_color: layer_config.text_color,
                font_size: layer_config.font_size,
                z_order: layer_config.z_order,
                text_align: TextAlign::from_str(&layer_config.text_align),
                text_valign: TextVAlign::from_str(&layer_config.text_valign),
                text_offset: layer_config.text_offset,
                text_padding: layer_config.text_padding,
            };

            match Layer::from_path(&layer_config.path, ghost_config) {
                Ok(mut layer) => {
                    layer.calculate_position(skin_width, skin_height);
                    log::info!(
                        "Loaded layer: {} at position {:?}",
                        layer_config.path,
                        layer.position()
                    );
                    layers.push(layer);
                }
                Err(e) => {
                    log::error!("Failed to load layer '{}': {}", layer_config.path, e);
                }
            }
        }

        // Sort layers by z_order
        layers.sort_by_key(|l| l.config.z_order);

        // Determine load state
        let load_state = if let Some(receiver) = skin_load_receiver {
            SkinLoadState::Loading { receiver }
        } else if animated_skin.is_some() {
            SkinLoadState::Ready
        } else {
            SkinLoadState::Static
        };

        // Substitute persona placeholders in layer text ({name}, {nick})
        if let Some(ref meta) = persona_meta {
            for layer in &mut layers {
                if let Some(ref mut text) = layer.config.text {
                    if text.contains("{name}") || text.contains("{nick}") {
                        *text = text.replace("{name}", &meta.name).replace("{nick}", &meta.nick);
                    }
                }
            }
        }

        // Extract still image data and create loading layer from persona meta
        let (still_skin_data, loading_layer) = if let Some(ref meta) = persona_meta {
            let still_data = meta.still_image.clone();

            let loading_layer = if matches!(load_state, SkinLoadState::Loading { .. }) {
                // Create a semi-transparent background bar centered on the character
                let bar_width = skin_width.min(300);
                let bar_height = 40u32;
                if let Ok(bg_data) = SkinData::solid_color(bar_width, bar_height, [0, 0, 0, 180]) {
                    let layer_config = LayerConfig {
                        anchor: LayerAnchor::Center,
                        offset: [0.0, 0.0],
                        size: None,
                        text: Some(meta.loading_text.clone()),
                        text_color: [1.0, 1.0, 1.0, 1.0],
                        font_size: 14.0,
                        z_order: 100,
                        text_align: TextAlign::Center,
                        text_valign: TextVAlign::Center,
                        text_offset: [0.0, 0.0],
                        text_padding: [8.0, 8.0, 8.0, 8.0],
                    };
                    let mut layer = Layer::new(bg_data, layer_config);
                    layer.calculate_position(skin_width, skin_height);
                    Some(layer)
                } else {
                    None
                }
            } else {
                None
            };

            (still_data, loading_layer)
        } else {
            (None, None)
        };

        Self {
            config,
            buttons,
            callout_sender,
            skin_size: (skin_width, skin_height),
            layers,
            layer_renderer: LayerRenderer::new(),
            layer_pipeline: None,
            texture_format: None,
            animated_skin,
            menu_ids: None,
            should_quit: false,
            chat_sender,
            load_state,
            still_skin: None,
            still_skin_data,
            needs_gpu_reinit: false,
            loading_layer,
        }
    }

    /// Send a callout command
    fn send_callout(&self, cmd: CalloutCommand) {
        if let Err(e) = self.callout_sender.send(cmd) {
            log::error!("Failed to send callout command: {}", e);
        }
    }

    /// Set tray menu IDs for event handling
    pub fn set_menu_ids(&mut self, menu_ids: MenuIds) {
        self.menu_ids = Some(menu_ids);
    }

    /// Set animation state by name
    pub fn set_animation_state(&mut self, state_name: &str) {
        if let Some(ref mut animated_skin) = self.animated_skin {
            let state = AnimationState::from_str(state_name);
            if animated_skin.has_state(state) {
                animated_skin.set_state(state);
                log::info!("Animation state changed to: {:?}", state);
            } else {
                log::warn!("Animation state not available: {}", state_name);
            }
        }
    }

    /// Open the chat window
    fn open_chat_window(&self) {
        if let Err(e) = self.chat_sender.send(ChatWindowCommand::Show) {
            log::error!("Failed to send chat window show command: {}", e);
        } else {
            log::info!("Chat window show command sent");
        }
    }

    /// Poll and handle tray menu events
    fn poll_tray_events(&mut self) {
        let Some(ref menu_ids) = self.menu_ids else {
            return;
        };

        if let Some(cmd) = tray::poll_menu_event(menu_ids) {
            match cmd {
                TrayCommand::OpenChat => {
                    self.open_chat_window();
                }
                TrayCommand::SetState(state) => {
                    self.set_animation_state(&state);
                }
                TrayCommand::Quit => {
                    log::info!("Quit requested from tray");
                    self.should_quit = true;
                }
            }
        }
    }
}

impl GhostApp for App {
    fn init_gpu(&mut self, gpu: GpuResources<'_>) {
        log::info!("Main window GPU initialized");

        // Initialize animated skin GPU resources (if already loaded)
        if let Some(ref mut animated_skin) = self.animated_skin {
            animated_skin.init_gpu(gpu.device, gpu.queue);
            log::info!(
                "Animated skin initialized with states: {:?}",
                animated_skin.available_states()
            );
        }

        // Create still skin from persona meta (for loading state display)
        if let Some(ref skin_data) = self.still_skin_data {
            match Skin::from_skin_data(skin_data, gpu.device, gpu.queue) {
                Ok(skin) => {
                    self.still_skin = Some(skin);
                    log::info!("Still image skin created for loading state");
                }
                Err(e) => log::error!("Failed to create still skin: {}", e),
            }
        }

        // Initialize layer GPU resources
        for layer in &mut self.layers {
            layer.init_gpu(gpu.device, gpu.queue);
        }

        // Initialize loading layer GPU resources
        if let Some(ref mut layer) = self.loading_layer {
            layer.init_gpu(gpu.device, gpu.queue);
        }

        // Create sprite pipeline for layers
        self.layer_pipeline = Some(SpritePipeline::new(gpu.device, gpu.format));
        self.texture_format = Some(gpu.format);

        // Initialize layer text renderer
        self.layer_renderer.init_gpu(gpu.device, gpu.queue, gpu.format);
    }

    fn update(&mut self, delta: f32) {
        // Poll tray menu events
        self.poll_tray_events();

        // Check for completed background load
        if let SkinLoadState::Loading { ref receiver } = self.load_state {
            if let Ok(loaded_skin) = receiver.try_recv() {
                log::info!("Background skin loading complete — transitioning to animation");
                self.animated_skin = Some(loaded_skin);
                self.load_state = SkinLoadState::Ready;
                self.needs_gpu_reinit = true;
                self.loading_layer = None;
                self.still_skin = None;
                self.still_skin_data = None;
            }
        }

        // Update animated skin
        if let Some(ref mut animated_skin) = self.animated_skin {
            animated_skin.update(delta);
        }
    }

    fn current_skin(&self) -> Option<&Skin> {
        match &self.load_state {
            SkinLoadState::Loading { .. } => self.still_skin.as_ref(),
            SkinLoadState::Ready => self.animated_skin.as_ref().and_then(|a| a.current_skin()),
            SkinLoadState::Static => None,
        }
    }

    fn target_fps(&self) -> f32 {
        if self.animated_skin.is_some() || matches!(self.load_state, SkinLoadState::Loading { .. })
        {
            self.config.skin.fps
        } else {
            30.0
        }
    }

    fn should_quit(&self) -> bool {
        self.should_quit
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
                                self.send_callout(CalloutCommand::Say(
                                    "Hi, how are you today?".to_string(),
                                ));
                                log::info!("Action: Greeting");
                            }
                            "think" => {
                                self.send_callout(CalloutCommand::Think(
                                    "Hmm, let me think about that...".to_string(),
                                ));
                                log::info!("Action: Thinking");
                            }
                            "scream" => {
                                self.send_callout(CalloutCommand::Scream(
                                    "WATCH OUT!".to_string(),
                                ));
                                log::info!("Action: Screaming");
                            }
                            _ => {
                                self.send_callout(CalloutCommand::Say(format!(
                                    "Button '{}' clicked!",
                                    btn_config.id
                                )));
                                log::info!("Action: Unknown button '{}'", btn_config.id);
                            }
                        }
                        break;
                    }
                }
            }
            GhostEvent::Resized(_width, _height) => {
                // Note: Don't update skin_size on resize. The skin dimensions are fixed,
                // and layers should always be positioned relative to the original skin size.
                // The resize event may give different values on HiDPI displays.
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

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: [f32; 2],
        scale_factor: f32,
        opacity: f32,
    ) {
        // Handle deferred GPU init after background load completes
        if self.needs_gpu_reinit {
            if let Some(ref mut animated_skin) = self.animated_skin {
                animated_skin.init_gpu(device, queue);
                log::info!("Animated skin GPU initialized after background load");
            }
            self.needs_gpu_reinit = false;
        }

        // Prepare layer bind groups with window opacity
        if let Some(pipeline) = &self.layer_pipeline {
            for layer in &mut self.layers {
                layer.prepare_with_opacity(pipeline, device, queue, viewport, scale_factor, opacity);
            }
            // Prepare loading layer
            if let Some(ref mut layer) = self.loading_layer {
                layer.prepare_with_opacity(pipeline, device, queue, viewport, scale_factor, opacity);
            }
        }

        // Clear stale text state before preparing new text
        self.layer_renderer.begin_frame(device, queue, viewport);

        // Prepare layer text rendering
        for layer in &self.layers {
            if layer.text().is_some() {
                self.layer_renderer
                    .prepare_text(device, queue, layer, viewport, scale_factor);
            }
        }
        // Prepare loading layer text
        if let Some(ref layer) = self.loading_layer {
            if layer.text().is_some() {
                self.layer_renderer
                    .prepare_text(device, queue, layer, viewport, scale_factor);
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
            // Render loading layer
            if let Some(ref layer) = self.loading_layer {
                if let Some(bind_group) = layer.bind_group() {
                    pipeline.render_bind_group(render_pass, bind_group);
                }
            }
        }

        // Render layer text
        self.layer_renderer.render_text(render_pass);
    }
}
