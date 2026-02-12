//! Callout window application - renders the callout bubble in a separate window

use ghost_ui::{Callout, CalloutStyle, TextAnimation};
use ghost_ui::CalloutApp;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use wgpu::{Device, Queue, RenderPass, TextureFormat};

use crate::config::{Anchor, Config};

/// Commands that can be sent to the callout window
#[derive(Debug, Clone)]
pub enum CalloutCommand {
    Say(String),
    Think(String),
    Scream(String),
    Hide,
}

/// Sender for callout commands - used by main app
pub type CalloutSender = Sender<CalloutCommand>;

/// Callout window app - renders the callout in a separate window
pub struct CalloutWindowApp {
    callout: Callout,
    receiver: Receiver<CalloutCommand>,
    initialized: bool,
}

impl CalloutWindowApp {
    pub fn new(config: &Config, receiver: Receiver<CalloutCommand>) -> Self {
        let callout = create_callout_from_config(config);
        Self {
            callout,
            receiver,
            initialized: false,
        }
    }

    fn process_commands(&mut self) {
        // Process all pending commands
        while let Ok(cmd) = self.receiver.try_recv() {
            match cmd {
                CalloutCommand::Say(text) => self.callout.say(text),
                CalloutCommand::Think(text) => self.callout.think(text),
                CalloutCommand::Scream(text) => self.callout.scream(text),
                CalloutCommand::Hide => self.callout.hide(),
            }
        }
    }
}

impl CalloutApp for CalloutWindowApp {
    fn init_gpu(&mut self, device: &Device, queue: &Queue, format: TextureFormat) {
        if !self.initialized {
            self.callout.init(device, queue, format);
            self.initialized = true;
            log::info!("Callout window GPU initialized");
        }
    }

    fn prepare(&mut self, device: &Device, queue: &Queue, viewport: [f32; 2], scale_factor: f32, _opacity: f32) {
        if self.callout.is_visible() {
            self.callout.prepare(device, queue, viewport, scale_factor);
        }
    }

    fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        if self.callout.is_visible() {
            self.callout.render(render_pass);
        }
    }

    fn update(&mut self, delta: f32) -> bool {
        // Process any pending commands
        let had_commands = self.receiver.try_recv().is_ok();
        if had_commands {
            // Re-process including the one we just peeked
            self.process_commands();
        }

        // Update callout animation - returns true if animation is active
        let was_visible = self.callout.is_visible();
        self.callout.update(delta);
        let is_visible = self.callout.is_visible();

        // Need redraw if: had commands, visibility changed, or animation is running
        had_commands || (was_visible != is_visible) || (is_visible && self.callout.is_animating())
    }
}

fn create_callout_from_config(config: &Config) -> Callout {
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

    // Callout position is now relative to the callout window (0,0)
    // The window itself is positioned by the offset
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

/// Create a callout command channel
pub fn create_callout_channel() -> (CalloutSender, Receiver<CalloutCommand>) {
    mpsc::channel()
}

/// Calculate callout window offset from main window based on config
pub fn calculate_callout_offset(config: &Config, skin_width: u32, skin_height: u32) -> [i32; 2] {
    let anchor = Anchor::from_str(&config.callout.anchor);
    let (anchor_x, anchor_y) = anchor.as_fraction();

    // Calculate base position from anchor
    let base_x = skin_width as f32 * anchor_x;
    let base_y = skin_height as f32 * anchor_y;

    // Apply offset
    let x = base_x + config.callout.offset[0];
    let y = base_y + config.callout.offset[1];

    [x as i32, y as i32]
}

/// Calculate callout window size based on config
pub fn calculate_callout_size(config: &Config) -> (u32, u32) {
    // Estimate height based on font size and padding
    let estimated_height = (config.callout.font_size * 3.0 + config.callout.style.padding * 2.0) as u32;
    (config.callout.max_width as u32, estimated_height.max(100))
}
