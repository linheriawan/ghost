//! Application state - combines UI and business logic

use std::sync::mpsc;

use ghost_ui::{
    AnimatedSkin, AnimationState, Button, ButtonImage, ButtonStyle, GhostApp, GhostEvent, GpuResources, Label, Layer, LayerAnchor, LayerConfig, LayerRenderer, MarqueeLabel, PersonaMeta, Skin, SkinData, SpritePipeline, TextAlign, TextVAlign,
};
use rustfft::num_traits::ToPrimitive;
use wgpu::TextureFormat;

use super::callout_window::CalloutCommand;
use crate::bus::{AppBus, AppSenders};
use crate::config::Config;
use crate::skin::SkinBundle;
use crate::ui;
use crate::vars::GhostState;
use crate::windows::control_window::ControlWindowCommand;

#[derive(Debug, Clone)]
pub enum MainCommand {
    MicLevel(f32),
}
pub type MainSender = mpsc::Sender<MainCommand>;
pub type MainReceiver = mpsc::Receiver<MainCommand>;
pub fn create_main_channel() -> (MainSender, MainReceiver) { mpsc::channel() }

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

enum AnyWidget {
    Button(Button),
    ButtonImage(ButtonImage),
    Label(Label),
    Marquee(MarqueeLabel),
}

/// Visual output from ui_design() — the "what it looks like" for the main window
struct MainUiDesign {
    widgets: Vec<AnyWidget>,
    layers: Vec<Layer>,
    loading_layer: Option<Layer>,
    still_skin_data: Option<SkinData>,
}
#[derive(Default)]
struct GhostData {
    mic_level: f32,
    something_else: String,
}
/// Main application state
pub struct App {
    config: Config,
    bus: AppSenders,
    main_rx: Option<MainReceiver>,
    data: GhostData,

    widgets: Vec<AnyWidget>,
    layers: Vec<Layer>,
    layer_renderer: LayerRenderer,
    layer_pipeline: Option<SpritePipeline>,
    texture_format: Option<TextureFormat>,
    /// Animated skin (if using frame sequences)
    pub animated_skin: Option<AnimatedSkin>,
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
    state: &GhostState,
    persona_meta: Option<&PersonaMeta>,
    load_state_is_loading: bool,
) -> MainUiDesign {
    let (w, h) = state.skin_size();
    let mut widgets: Vec<AnyWidget> = vec![
        AnyWidget::Button(ui::make_btn("greet",  "Greet",  [10.0,  10.0], [60.0, 28.0], ButtonStyle::primary(), config)),
        AnyWidget::Button(ui::make_btn("think",  "Think",  [80.0,  10.0], [60.0, 28.0], ButtonStyle::default(), config)),
        AnyWidget::Button(ui::make_btn("scream", "Scream", [150.0, 10.0], [60.0, 28.0], ButtonStyle::light(),   config)),
        AnyWidget::Button(ui::make_btn("ctrl",  "Control", [100.0, 200.0],[60.0, 30.0], ButtonStyle::primary(), config)),
        AnyWidget::Label(ui::make_label("status_bar", "A text in the bottom", [10.0, 60.0], [160.0, 24.0], ghost_ui::LabelStyle::with_background())),
        AnyWidget::Marquee(ui::make_marquee("announcement", "this is a marquee scrolling text", [10.0, 90.0], [160.0, 24.0], ghost_ui::LabelStyle::with_background(), 30.0)),
    ];
    if let Some(img) = ui::make_btn_image("win_close", "assets/icon.png", [10.0, 120.0]) {
        widgets.push(AnyWidget::ButtonImage(img));
    }
    // Load layers from config
    let mut layers: Vec<Layer> = config.layers.iter()
        .filter_map(|cfg| ui::make_layer(cfg, state))
        .collect();

    if let Ok(mut bl) = Layer::from_path("assets/bl.png", LayerConfig::default()) {
        bl.calculate_position(w, h);
        layers.push(bl);
    }

    if let Ok(mut tr) = Layer::from_path("assets/tr.png", LayerConfig{
        anchor: LayerAnchor::TopRight,
        ..LayerConfig::default()
    }) {
        tr.calculate_position(w, h);
        layers.push(tr);
    }
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
    }

    // Extract still image data and create loading layer from persona meta
    let (still_skin_data, loading_layer) = if let Some(meta) = persona_meta {
        let still_data = meta.still_image.clone();

        let loading_layer = if load_state_is_loading {
            // Create a semi-transparent background bar centered on the character
            let (skin_width, skin_height) = state.skin_size();
            let bar_width = skin_width.min(300);
            let bar_height = 40u32;
            if let Ok(bg_data) = SkinData::solid_color(bar_width, bar_height, [0, 0, 0, 180]) {
                let layer_config = LayerConfig {
                    anchor: LayerAnchor::Center,
                    text: Some(meta.loading_text.clone()),
                    font_size: 14.0,
                    z_order: 100,
                    ..LayerConfig::default()
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

    MainUiDesign { widgets, layers, loading_layer, still_skin_data }
}

impl App {
    pub fn new(
        config: Config,
        skin: SkinBundle,
        bus: &mut AppBus,
        state: GhostState,
    ) -> Self {
        let main_rx = bus.main_rx.take();
        let bus = bus.senders();
        let load_state = if let Some(receiver) = skin.load_rx {
            SkinLoadState::Loading { receiver }
        }
        else if skin.animated.is_some() { SkinLoadState::Ready }
        else { SkinLoadState::Static };

        let load_state_is_loading = matches!(load_state, SkinLoadState::Loading { .. });

        state.set_skin_size(skin.width, skin.height);
        let design = ui_design(
            &config,
            &state,
            skin.persona.as_ref(),
            load_state_is_loading,
        );

        Self {
            config,
            bus,
            main_rx,
            data: GhostData::default(),

            widgets: design.widgets,
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
        if let Some(ref rx) = self.main_rx {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    MainCommand::MicLevel(level) => self.data.mic_level = level,
                }
            }
        }
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
                    let _ = self.bus.callout_tx.send(CalloutCommand::Say("Howdy.. have a nice day would you..".into()));
                    log::info!("Action: Greeting");
                } else if id == ui::get_button_id("think") {
                    let _ = self.bus.callout_tx.send(CalloutCommand::Think("Hmm, let me think about that...".into()));
                    log::info!("Action: Thinking");
                } else if id == ui::get_button_id("scream") {
                    let _ = self.bus.callout_tx.send(CalloutCommand::Scream("WATCH OUT!".into()));
                    log::info!("Action: Screaming");
                } else if id == ui::get_button_id("ctrl") {
                    let _ = self.bus.ctrl_tx.send(ControlWindowCommand::Toggle);
                    log::info!("Action: toggle ctrl");
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

    fn buttons(&self) -> Vec<&Button> {
        self.widgets.iter().filter_map(|w| if let AnyWidget::Button(b) = w { Some(b) } else { None }).collect()
    }
    fn buttons_mut(&mut self) -> Vec<&mut Button> {
        self.widgets.iter_mut().filter_map(|w| if let AnyWidget::Button(b) = w { Some(b) } else { None }).collect()
    }
    fn button_images(&self) -> Vec<&ButtonImage> {
        self.widgets.iter().filter_map(|w| if let AnyWidget::ButtonImage(b) = w { Some(b) } else { None }).collect()
    }
    fn button_images_mut(&mut self) -> Vec<&mut ButtonImage> {
        self.widgets.iter_mut().filter_map(|w| if let AnyWidget::ButtonImage(b) = w { Some(b) } else { None }).collect()
    }
    fn labels(&self) -> Vec<&Label> {
        self.widgets.iter().filter_map(|w| if let AnyWidget::Label(l) = w { Some(l) } else { None }).collect()
    }
    fn marquee_labels(&self) -> Vec<&MarqueeLabel> {
        self.widgets.iter().filter_map(|w| if let AnyWidget::Marquee(m) = w { Some(m) } else { None }).collect()
    }
    fn marquee_labels_mut(&mut self) -> Vec<&mut MarqueeLabel> {
        self.widgets.iter_mut().filter_map(|w| if let AnyWidget::Marquee(m) = w { Some(m) } else { None }).collect()
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

    fn hit_test(&self, x: f32, y: f32) -> bool {
        self.layers.iter().any(|l| l.contains(x, y))
    }
}
