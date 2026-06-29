//! Callout window — self-contained window that renders the callout bubble.

use ghost_ui::{Callout, CalloutStyle, ExtraWindow, GhostApp, GhostEvent, GhostWindowBuilder, GpuResources, TextAnimation};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use tao::event::WindowEvent;
use tao::window::WindowId;
use wgpu::RenderPass;

use crate::bus::AppBus;
use crate::config::{Anchor, Config};

#[derive(Debug, Clone)]
pub enum CalloutCommand {
    Say(String),
    Think(String),
    Scream(String),
    Hide,
}

pub type CalloutSender = Sender<CalloutCommand>;

pub struct CalloutWindow {
    window: ghost_ui::GhostWindow,
    offset: [i32; 2],
    logic: CalloutLogic,
}

impl CalloutWindow {
    pub fn new(config: &Config, event_loop: &ghost_ui::EventLoop<()>, bus: &mut AppBus) -> Self {
        let skin_bundle = crate::skin::load(&config.skin);
        let (skin_width, skin_height) = (skin_bundle.width, skin_bundle.height);
        let offset = calculate_callout_offset(config, skin_width, skin_height);
        let (w, h) = calculate_callout_size(config);
        let window = GhostWindowBuilder::new()
            .with_size(w, h)
            .with_always_on_top(true)
            .with_draggable(false)
            .with_click_through(true)
            .with_alpha_hit_test(false)
            .with_opacity_focused(1.0)
            .with_opacity_unfocused(1.0)
            .with_title("Ghost Callout")
            .build(event_loop)
            .expect("Failed to create callout window");

        Self {
            window,
            offset,
            logic: CalloutLogic {
                callout: ui_design(config),
                receiver: bus.callout_rx.take().expect("callout_rx already consumed"),
                initialized: false,
                needs_redraw: false,
            },
        }
    }
}

impl ExtraWindow for CalloutWindow {
    fn window_id(&self) -> WindowId { self.window.window().id() }

    fn on_event(&mut self, _event: &WindowEvent) {}

    fn update(&mut self, delta: f32) {
        self.logic.update(delta);
        if self.logic.needs_redraw() { self.window.request_redraw(); }
    }

    fn render(&mut self) {
        if !self.logic.callout.is_visible() { return; }
        if !self.logic.initialized {
            self.window.init_callout_gpu(&mut self.logic);
            if !self.logic.initialized { return; }
        }
        let size = self.window.window().inner_size();
        let viewport = [size.width as f32, size.height as f32];
        self.window.prepare_callout(&mut self.logic, viewport);
        let _ = self.window.render_with_widgets_and_app(None, &mut self.logic);
    }

    fn request_redraw(&self) {
        if self.logic.callout.is_visible() { self.window.request_redraw(); }
    }

    fn is_visible(&self) -> bool { self.logic.callout.is_visible() }

    fn set_position(&self, x: i32, y: i32) { self.window.set_position(x, y); }

    fn on_primary_moved(&self, x: i32, y: i32) {
        self.window.set_position(x + self.offset[0], y + self.offset[1]);
    }

    fn bring_to_front(&self) {}
}

struct CalloutLogic {
    callout: Callout,
    receiver: Receiver<CalloutCommand>,
    initialized: bool,
    needs_redraw: bool,
}

impl GhostApp for CalloutLogic {
    fn on_event(&mut self, _: GhostEvent) {}

    fn init_gpu(&mut self, gpu: GpuResources<'_>) {
        if !self.initialized {
            self.callout.init(gpu.device, gpu.queue, gpu.format);
            self.initialized = true;
            log::info!("Callout GPU initialized");
        }
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, viewport: [f32; 2], scale_factor: f32, _opacity: f32) {
        if self.callout.is_visible() {
            self.callout.prepare(device, queue, viewport, scale_factor);
        }
    }

    fn update(&mut self, delta: f32) {
        let mut had_commands = false;
        while let Ok(cmd) = self.receiver.try_recv() {
            had_commands = true;
            match cmd {
                CalloutCommand::Say(text)    => self.callout.say(text),
                CalloutCommand::Think(text)  => self.callout.think(text),
                CalloutCommand::Scream(text) => self.callout.scream(text),
                CalloutCommand::Hide         => self.callout.hide(),
            }
        }
        let was_visible = self.callout.is_visible();
        self.callout.update(delta);
        let is_visible = self.callout.is_visible();
        self.needs_redraw = had_commands
            || (was_visible != is_visible)
            || (is_visible && self.callout.is_animating());
    }

    fn needs_redraw(&self) -> bool { self.needs_redraw }

    fn render_self<'rp>(&mut self, render_pass: &mut RenderPass<'rp>) where Self: 'rp {
        if self.callout.is_visible() {
            // Safety: Self: 'rp guarantees callout's GPU resources are valid for 'rp.
            // We extend the anonymous borrow lifetime to 'rp to satisfy Callout::render's constraint.
            let callout: &'rp Callout = unsafe { &*(&self.callout as *const Callout) };
            callout.render(render_pass);
        }
    }
}

fn ui_design(config: &Config) -> Callout {
    let style = CalloutStyle {
        background: config.callout.style.background,
        text_color: config.callout.style.text_color,
        font_size: config.callout.font_size,
        padding: config.callout.style.padding,
        border_radius: config.callout.style.border_radius,
        ..Default::default()
    };
    let animation = match config.callout.animation.as_str() {
        "instant"                       => TextAnimation::Instant,
        "word-by-word" | "wordbyword"   => TextAnimation::WordByWord { wps: config.callout.animation_speed },
        "stream"                        => TextAnimation::Stream { cps: config.callout.animation_speed },
        _                               => TextAnimation::Typewriter { cps: config.callout.animation_speed },
    };
    let mut callout = Callout::new()
        .with_position(0.0, 0.0)
        .with_max_width(config.callout.max_width)
        .with_text_animation(animation)
        .with_style(style);
    if config.callout.duration > 0.0 {
        callout = callout.with_duration(Duration::from_secs_f32(config.callout.duration));
    }
    callout
}

pub fn create_callout_channel() -> (CalloutSender, Receiver<CalloutCommand>) {
    mpsc::channel()
}

pub fn calculate_callout_offset(config: &Config, skin_width: u32, skin_height: u32) -> [i32; 2] {
    let anchor = Anchor::from_str(&config.callout.anchor);
    let (anchor_x, anchor_y) = anchor.as_fraction();
    let callout_size = calculate_callout_size(config);
    let mut x = skin_width as f32 * anchor_x;
    let mut y = skin_height as f32 * anchor_y;
    if anchor_x > 0.5 { x -= callout_size.0 as f32; }
    if anchor_y > 0.5 { y -= callout_size.1 as f32; }
    x += config.callout.offset[0];
    y += config.callout.offset[1];
    [x as i32, y as i32]
}

pub fn calculate_callout_size(config: &Config) -> (u32, u32) {
    let estimated_height = (config.callout.font_size * 3.0 + config.callout.style.padding * 2.0) as u32;
    (config.callout.max_width as u32, estimated_height.max(100))
}
