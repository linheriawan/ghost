//! Application state - combines UI and business logic

use std::sync::mpsc;

use ghost_ui::{
    AnimatedSkin, AnimationState, GhostApp, GhostEvent, GpuResources, Layer, LayerAnchor, LayerConfig, LayerRenderer, PersonaMeta, Skin, SkinData, SpritePipeline,
};
use wgpu::TextureFormat;

use super::callout_window::CalloutCommand;
use super::chat_window::ChatWindowCommand;
use super::log_window::LogWindowCommand;
use crate::bus::{AppBus, AppSenders};
use crate::config::Config;
use crate::skin::SkinBundle;
use crate::tray::{MenuIds, TrayCommand};
use crate::ui;
use crate::vars::GhostState;
use crate::windows::control_window::ControlWindowCommand;

/// Skin loading state for lazy loading from .persona.zip
enum SkinLoadState {
    Loading { receiver: mpsc::Receiver<AnimatedSkin> },
    Ready,
    Static,
}

#[derive(Debug, Clone)]
pub enum MainCommand {
    MicLevel(f32),
}
pub type MainSender = mpsc::Sender<MainCommand>;
pub type MainReceiver = mpsc::Receiver<MainCommand>;
pub fn create_main_channel() -> (MainSender, MainReceiver) { mpsc::channel() }

#[derive(Default)]
struct GhostData {
    mic_level: f32,
    something_else: String,
}

struct MainUiDesign {
    widgets: Vec<ui::AnyWidget>,
    layers: Vec<Layer>,
    loading_layer: Option<Layer>,
    still_skin_data: Option<SkinData>,
}

pub struct App {
    config: Config,
    bus: AppSenders,
    main_rx: Option<MainReceiver>,
    data: GhostData,
    menu_ids: MenuIds,
    should_quit: bool,

    widgets: ui::Widgets,
    layers: Vec<Layer>,
    layer_renderer: LayerRenderer,
    layer_pipeline: Option<SpritePipeline>,
    texture_format: Option<TextureFormat>,
    pub animated_skin: Option<AnimatedSkin>,
    load_state: SkinLoadState,
    still_skin: Option<Skin>,
    still_skin_data: Option<SkinData>,
    needs_gpu_reinit: bool,
    loading_layer: Option<Layer>,
    state: GhostState,
}

fn ui_design(
    config: &Config,
    state: &GhostState,
    persona_meta: Option<&PersonaMeta>,
    load_state_is_loading: bool,
) -> MainUiDesign {
    let (w, h) = state.skin_size();
    let mut widgets: Vec<ui::AnyWidget> = vec![
        ui::AnyWidget::Button(ui::make_btn("greet",  "Greet",  [10.0,  10.0], [60.0, 28.0], ghost_ui::ButtonStyle::primary(), config)),
        ui::AnyWidget::Button(ui::make_btn("think",  "Think",  [80.0,  10.0], [60.0, 28.0], ghost_ui::ButtonStyle::default(), config)),
        ui::AnyWidget::Button(ui::make_btn("scream", "Scream", [150.0, 10.0], [60.0, 28.0], ghost_ui::ButtonStyle::light(),   config)),
        ui::AnyWidget::Button(ui::make_btn("ctrl",  "Control", [100.0, 200.0],[60.0, 30.0], ghost_ui::ButtonStyle::primary(), config)),
        ui::AnyWidget::Label(ui::make_label("status_bar", "A text in the bottom", [10.0, 60.0], [160.0, 24.0], ghost_ui::LabelStyle::with_background())),
        ui::AnyWidget::Marquee(ui::make_marquee("announcement", "this is a marquee scrolling text", [10.0, 90.0], [160.0, 24.0], ghost_ui::LabelStyle::with_background(), 30.0)),
    ];
    if let Some(img) = ui::make_btn_image("win_close", "assets/icon.png", [10.0, 120.0]) {
        widgets.push(ui::AnyWidget::ButtonImage(img));
    }

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
    layers.sort_by_key(|l| l.config.z_order);

    if let Some(meta) = persona_meta {
        for layer in &mut layers {
            if let Some(ref mut text) = layer.config.text {
                if text.contains("{name}") || text.contains("{nick}") {
                    *text = text.replace("{name}", &meta.name).replace("{nick}", &meta.nick);
                }
            }
        }
    }

    let (still_skin_data, loading_layer) = if let Some(meta) = persona_meta {
        let still_data = meta.still_image.clone();
        let loading_layer = if load_state_is_loading {
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
            } else { None }
        } else { None };
        (still_data, loading_layer)
    } else { (None, None) };

    MainUiDesign { widgets, layers, loading_layer, still_skin_data }
}

impl App {
    pub fn new(
        config: Config,
        skin: SkinBundle,
        bus: &mut AppBus,
        state: GhostState,
        menu_ids: MenuIds,
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
        let design = ui_design(&config, &state, skin.persona.as_ref(), load_state_is_loading);

        Self {
            config,
            bus,
            main_rx,
            data: GhostData::default(),
            menu_ids,
            should_quit: false,

            widgets: ui::Widgets::new(design.widgets),
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
        if let Some(ref mut animated_skin) = self.animated_skin {
            animated_skin.init_gpu(gpu.device, gpu.queue);
            log::info!("Animated skin initialized with states: {:?}", animated_skin.available_states());
        }
        if let Some(ref skin_data) = self.still_skin_data {
            match Skin::from_skin_data(skin_data, gpu.device, gpu.queue) {
                Ok(skin) => {
                    self.still_skin = Some(skin);
                    log::info!("Still image skin created for loading state");
                }
                Err(e) => log::error!("Failed to create still skin: {}", e),
            }
        }
        for layer in &mut self.layers { layer.init_gpu(gpu.device, gpu.queue); }
        if let Some(ref mut layer) = self.loading_layer { layer.init_gpu(gpu.device, gpu.queue); }
        self.layer_pipeline = Some(SpritePipeline::new(gpu.device, gpu.format));
        self.texture_format = Some(gpu.format);
        self.layer_renderer.init_gpu(gpu.device, gpu.queue, gpu.format);
    }

    fn update(&mut self, delta: f32) {
        if let Some(ref rx) = self.main_rx {
            while let Ok(cmd) = rx.try_recv() {
                match cmd { MainCommand::MicLevel(level) => self.data.mic_level = level }
            }
        }
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
        if let Some(ref mut animated_skin) = self.animated_skin { animated_skin.update(delta); }

        if let Some(cmd) = crate::tray::poll_menu_event(&self.menu_ids) {
            match cmd {
                TrayCommand::OpenChat => { let _ = self.bus.chat_tx.send(ChatWindowCommand::Show); }
                TrayCommand::OpenCtrl => { let _ = self.bus.ctrl_tx.send(ControlWindowCommand::Toggle); }
                TrayCommand::OpenLog  => { let _ = self.bus.log_tx.send(LogWindowCommand::Show); }
                TrayCommand::SetState(s) => {
                    if let Some(ref mut skin) = self.animated_skin {
                        let state = AnimationState::from_str(&s);
                        if skin.has_state(state) { skin.set_state(state); }
                    }
                }
                TrayCommand::Quit => { self.should_quit = true; }
            }
        }
    }

    fn should_quit(&self) -> bool { self.should_quit }

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
            GhostEvent::Resized(width, height) => { self.state.set_main_size(width, height); }
            GhostEvent::Moved(x, y) => { self.state.set_main_pos(x, y); }
            GhostEvent::FocusChanged(focused) => { self.state.set_main_focused(focused); }
            _ => {}
        }
    }

    fn widget_list(&self) -> &[ghost_ui::AnyWidget] { &self.widgets.0 }
    fn widgets_mut(&mut self) -> Option<&mut Vec<ghost_ui::AnyWidget>> { Some(&mut self.widgets.0) }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: [f32; 2],
        scale_factor: f32,
        opacity: f32,
    ) {
        if self.needs_gpu_reinit {
            if let Some(ref mut animated_skin) = self.animated_skin {
                animated_skin.init_gpu(device, queue);
                log::info!("Animated skin GPU initialized after background load");
            }
            self.needs_gpu_reinit = false;
        }
        if let Some(pipeline) = &self.layer_pipeline {
            for layer in &mut self.layers {
                layer.prepare_with_opacity(pipeline, device, queue, viewport, scale_factor, opacity);
            }
            if let Some(ref mut layer) = self.loading_layer {
                layer.prepare_with_opacity(pipeline, device, queue, viewport, scale_factor, opacity);
            }
        }
        self.layer_renderer.begin_frame(device, queue, viewport);
        for layer in &self.layers {
            if layer.text().is_some() {
                self.layer_renderer.prepare_text(device, queue, layer, viewport, scale_factor);
            }
        }
        if let Some(ref layer) = self.loading_layer {
            if layer.text().is_some() {
                self.layer_renderer.prepare_text(device, queue, layer, viewport, scale_factor);
            }
        }
    }

    fn render_layers<'rp>(
        &mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _viewport: [f32; 2],
        render_pass: &mut wgpu::RenderPass<'rp>,
    ) where Self: 'rp {
        // Safety: Self: 'rp guarantees all GPU resources in self are valid for 'rp.
        // We re-borrow self as &'rp App so GPU methods (render_bind_group, render_text)
        // can receive references valid for the render pass lifetime.
        let this: &'rp App = unsafe { &*(self as *const App) };
        if let Some(pipeline) = &this.layer_pipeline {
            for layer in &this.layers {
                if let Some(bind_group) = layer.bind_group() {
                    pipeline.render_bind_group(render_pass, bind_group);
                }
            }
            if let Some(ref layer) = this.loading_layer {
                if let Some(bind_group) = layer.bind_group() {
                    pipeline.render_bind_group(render_pass, bind_group);
                }
            }
        }
        this.layer_renderer.render_text(render_pass);
    }

    fn hit_test(&self, x: f32, y: f32) -> bool {
        self.layers.iter().any(|l| l.contains(x, y))
    }
}
