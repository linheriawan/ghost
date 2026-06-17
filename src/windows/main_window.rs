//! Application state - combines UI and business logic

use std::sync::mpsc;

use ghost_ui::{
    AnimatedSkin, AnimationState, Button, GhostApp, GhostEvent, GpuResources, Layer, LayerAnchor,
    LayerConfig, LayerRenderer, PersonaMeta, Skin, SkinData, SpritePipeline, TextAlign, TextVAlign,
    ButtonStyle,
};
use wgpu::TextureFormat;

use super::callout_window::{CalloutCommand, CalloutSender};
use crate::config::Config;
use crate::skin::SkinBundle;
use crate::ui;
use crate::vars::GhostState;
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

/// Visual output from ui_design() — the "what it looks like" for the main window
struct MainUiDesign {
    buttons: Vec<Button>,
    layers: Vec<Layer>,
    loading_layer: Option<Layer>,
    still_skin_data: Option<SkinData>,
}

/// Main application state
pub struct App {
    config: Config,
    button_list: Vec<Button>,
    callout_sender: CalloutSender,
    skin_size: (u32, u32),
    layers: Vec<Layer>,
    layer_renderer: LayerRenderer,
    layer_pipeline: Option<SpritePipeline>,
    texture_format: Option<TextureFormat>,
    /// Animated skin (if using frame sequences)
    animated_skin: Option<AnimatedSkin>,
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
    /// Shared state for cross-window coordination
    state: GhostState,
}

/// Define "what the main window looks like" — all visual setup in one place.
///
/// Receives config values as input and returns the visual components.
/// This makes visual modifications easier: only look at this one function.
fn ui_design(
    config: &Config,
    skin_width: u32,
    skin_height: u32,
    persona_meta: Option<&PersonaMeta>,
    load_state_is_loading: bool,
) -> MainUiDesign {
    // Buttons are defined in code; ui.toml can override position/size/style per id.
    let buttons = vec![
        ui::make_btn("greet",  "Greet",  [10.0,  10.0], [60.0, 28.0], ButtonStyle::primary(), config),
        ui::make_btn("think",  "Think",  [80.0,  10.0], [60.0, 28.0], ButtonStyle::default(), config),
        ui::make_btn("scream", "Scream", [150.0, 10.0], [60.0, 28.0], ButtonStyle::light(),   config),
        ui::make_btn("hello",  "Hello",  [100.0, 200.0],[60.0, 30.0], ButtonStyle::primary(), config),
    ];

    // Load layers from config
    let mut layers: Vec<Layer> = config.layers.iter()
        .filter_map(|cfg| ui::make_layer(cfg, skin_width, skin_height))
        .collect();

    // Sort layers by z_order
    layers.sort_by_key(|l| l.config.z_order);

    // Substitute persona placeholders in layer text ({name}, {nick})
    if let Some(meta) = persona_meta {
        for layer in &mut layers {
            if let Some(ref mut text) = layer.config.text {
                if text.contains("{name}") || text.contains("{nick}") {
                    *text = text.replace("{name}", &meta.name).replace("{nick}", &meta.nick);
                }
            }
        }
    } else {
        let name_path = config.skin.path.clone();
        let name_part = name_path.split('.').next().unwrap().split('/').last().unwrap();
        for layer in &mut layers {
            if let Some(ref mut text) = layer.config.text {
                if text.contains("{name}") || text.contains("{nick}") {
                    *text = text.replace("{name}", name_part).replace("{nick}", name_part);
                }
            }
        }
    }

    // Extract still image data and create loading layer from persona meta
    let (still_skin_data, loading_layer) = if let Some(meta) = persona_meta {
        let still_data = meta.still_image.clone();

        let loading_layer = if load_state_is_loading {
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
    
    MainUiDesign {
        buttons,
        layers,
        loading_layer,
        still_skin_data,
    }
}

impl App {
    pub fn new(
        config: Config,
        skin: SkinBundle,
        callout_sender: CalloutSender,
        state: GhostState,
    ) -> Self {
        let load_state = if let Some(receiver) = skin.load_rx {
            SkinLoadState::Loading { receiver }
        } else if skin.animated.is_some() {
            SkinLoadState::Ready
        } else {
            SkinLoadState::Static
        };

        let load_state_is_loading = matches!(load_state, SkinLoadState::Loading { .. });

        let design = ui_design(
            &config,
            skin.width,
            skin.height,
            skin.persona.as_ref(),
            load_state_is_loading,
        );

        Self {
            config,
            button_list: design.buttons,
            callout_sender,
            skin_size: (skin.width, skin.height),
            layers: design.layers,
            layer_renderer: LayerRenderer::new(),
            layer_pipeline: None,
            texture_format: None,
            animated_skin: skin.animated,
            load_state,
            still_skin: None,
            still_skin_data: design.still_skin_data,
            needs_gpu_reinit: false,
            loading_layer: design.loading_layer,
            state,
        }
    }

    /// Send a callout command
    fn send_callout(&self, cmd: CalloutCommand) {
        if let Err(e) = self.callout_sender.send(cmd) {
            log::error!("Failed to send callout command: {}", e);
        }
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
        { self.config.skin.fps } 
        else { 30.0 }
    }

    fn on_event(&mut self, event: GhostEvent) {
        match event {
            GhostEvent::ButtonClicked(id) => {
                if id == ui::get_button_id("greet") {
                    self.send_callout(CalloutCommand::Say("Hi, how are you today?".to_string()));
                    log::info!("Action: Greeting");
                } else if id == ui::get_button_id("think") {
                    self.send_callout(CalloutCommand::Think("Hmm, let me think about that...".to_string()));
                    log::info!("Action: Thinking");
                } else if id == ui::get_button_id("scream") {
                    self.send_callout(CalloutCommand::Scream("WATCH OUT!".to_string()));
                    log::info!("Action: Screaming");
                } else if id == ui::get_button_id("hello") {
                    self.send_callout(CalloutCommand::Say("Hello world!".to_string()));
                    log::info!("Action: Hello");
                }
            }
            GhostEvent::Resized(width, height) => {
                // Note: Don't update skin_size on resize. The skin dimensions are fixed,
                // and layers should always be positioned relative to the original skin size.
                // The resize event may give different values on HiDPI displays.
                self.state.set_main_size(width, height);
            }
            GhostEvent::Moved(x, y) => {
                self.state.set_main_pos(x, y);
            }
            GhostEvent::FocusChanged(focused) => {
                self.state.set_main_focused(focused);
            }
            _ => {}
        }
    }

    fn buttons(&self) -> Vec<&Button> { self.button_list.iter().collect() }

    fn buttons_mut(&mut self) -> Vec<&mut Button> { self.button_list.iter_mut().collect() }

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
