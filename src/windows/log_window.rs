//! Log window using egui with tao/wgpu integration
//!
//! This creates an egui-based Log window that integrates with the existing
//! tao event loop instead of spawning a separate thread.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

use egui_wgpu::ScreenDescriptor;
use ghost_ui::ExtraWindow;
use tao::dpi::LogicalSize;
use tao::event::WindowEvent;
use tao::event_loop::EventLoop;
use tao::window::{Window, WindowBuilder, WindowId};
use wgpu::{Device, Queue, Surface, SurfaceConfiguration};

use crate::bus::AppBus;
use crate::config::LogConfig;
use crate::vars::GhostState;

/// Visual theme for the Log window — all "what it looks like" in one place.
pub struct LogTheme {
    // Panel colors
    pub bg_color: egui::Color32,
    pub input_panel_bg: egui::Color32,
    // Widget style overrides
    pub widget_inactive_bg: egui::Color32,
    pub widget_hovered_bg: egui::Color32,
    pub widget_active_bg: egui::Color32,
    // Message bubbles
    pub user_bubble_bg: egui::Color32,
    pub assistant_bubble_bg: egui::Color32,
    pub bubble_text_color: egui::Color32,
    pub bubble_rounding: f32,
    pub bubble_padding: egui::Margin,
    pub bubble_max_width_ratio: f32,
    // Role label
    pub role_label_color: egui::Color32,
    pub role_label_size: f32,
    // Message spacing
    pub message_spacing: f32,
    // Input area
    pub input_text_color: egui::Color32,
    pub input_hint: String,
    pub input_panel_margin: egui::Margin,
    // Send button
    pub send_btn_color: egui::Color32,
    pub send_btn_text_color: egui::Color32,
    pub send_btn_rounding: f32,
    pub send_btn_size: egui::Vec2,
}

/// Define "what the Log window looks like" — all visual constants in one place.
///
/// For now values are code defaults. Having them in LogTheme makes it easy
/// to add toml fields later.
fn ui_design(_config: &LogConfig) -> LogTheme {
    LogTheme {
        bg_color: egui::Color32::from_rgb(255, 24, 32),
        input_panel_bg: egui::Color32::from_rgb(30, 30, 46),

        widget_inactive_bg: egui::Color32::from_rgb(55, 55, 70),
        widget_hovered_bg: egui::Color32::from_rgb(65, 65, 80),
        widget_active_bg: egui::Color32::from_rgb(59, 130, 246),

        user_bubble_bg: egui::Color32::from_rgb(59, 130, 246),
        assistant_bubble_bg: egui::Color32::from_rgb(55, 65, 81),
        bubble_text_color: egui::Color32::WHITE,
        bubble_rounding: 12.0,
        bubble_padding: egui::Margin::symmetric(12.0, 8.0),
        bubble_max_width_ratio: 0.8,

        role_label_color: egui::Color32::from_rgb(140, 140, 160),
        role_label_size: 11.0,
        message_spacing: 12.0,

        input_text_color: egui::Color32::WHITE,
        input_hint: "Type a message...".to_string(),
        input_panel_margin: egui::Margin::symmetric(12.0, 10.0),

        send_btn_color: egui::Color32::from_rgb(59, 130, 246),
        send_btn_text_color: egui::Color32::WHITE,
        send_btn_rounding: 8.0,
        send_btn_size: egui::vec2(40.0, 32.0),
    }
}

/// Message in the Log
#[derive(Clone, Debug)]
pub struct LogMessage {
    pub role: String, // "user" or "assistant"
    pub content: String,
}

/// Commands to control the Log window
#[derive(Debug)]
pub enum LogWindowCommand {
    Show,
    Hide,
    Toggle,
    AddMessage(LogMessage),
}

/// Channel for sending commands to the Log window
pub type LogSender = Sender<LogWindowCommand>;
pub type LogReceiver = Receiver<LogWindowCommand>;

/// Create a channel for Log window communication
pub fn create_Log_channel() -> (LogSender, LogReceiver) {
    channel()
}

/// Log window state and rendering
pub struct LogWindow {
    window: Window,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_renderer: egui_wgpu::Renderer,
    messages: Vec<LogMessage>,
    input_text: String,
    receiver: LogReceiver,
    on_send: Option<Sender<String>>,
    visible: bool,
    needs_repaint: bool,
    start_time: Instant,
    assistant_name: String,
    /// Shared state for cross-window coordination
    state: GhostState,
    /// Track previous visibility for re-snap on show
    was_visible: bool,
    /// Visual theme (produced by ui_design)
    theme: LogTheme,
    /// Accumulated egui events between frames (drained into RawInput each render).
    pending_events: Vec<egui::Event>,
    /// Current modifier key state.
    modifiers: egui::Modifiers,
    /// Whether the window currently has focus.
    focused: bool,
    /// Whether voice mode is active (mic → STT → LLM → TTS).
    voice_mode: bool,
    /// Voice status label (stripped of the `##....` level bar portion).
    voice_status: String,
    /// Audio input level 0.0–1.0, derived from the `#` count in VoiceStatus.
    audio_level: f32,
}

impl LogWindow {
    /// Create a new Log window (starts hidden)
    pub fn new(
        event_loop: &EventLoop<()>,
        bus: &mut AppBus,
        size: [u32; 2],
        assistant_name: Option<String>,
        state: GhostState,
        Log_config: &LogConfig,
    ) -> Self {
        let receiver = bus.log_rx.take().expect("log_rx already consumed");
        let on_send: Option<Sender<String>> = None;
        // Create the window (hidden initially, no decorations for precise positioning)
        let window = WindowBuilder::new()
            .with_inner_size(LogicalSize::new(size[0], size[1]))
            .with_min_inner_size(LogicalSize::new(300, 400))
            .with_title("Ghost Log")
            .with_visible(false)
            .with_decorations(true)
            .build(event_loop)
            .expect("Failed to create Log window");

        // Create wgpu instance and surface
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // SAFETY: The window lives as long as the surface
        let surface = unsafe {
            let window_ptr = &window as *const Window;
            instance
                .create_surface(&*window_ptr)
                .expect("Failed to create surface")
        };

        // Request adapter
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("Failed to find suitable adapter");

        // Create device and queue
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Log Window Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ))
        .expect("Failed to create device");

        // Configure surface
        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create egui context
        let egui_ctx = egui::Context::default();

        // Create egui-wgpu renderer
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1);

        let assistant_name = assistant_name.unwrap_or_else(|| "Assistant".to_string());

        let theme = ui_design(Log_config);

        Self {
            window,
            surface,
            device,
            queue,
            config,
            egui_ctx,
            egui_renderer,
            messages: vec![LogMessage {
                role: "assistant".to_string(),
                content: "Hello! How can I help you today?".to_string(),
            }],
            input_text: String::new(),
            receiver,
            on_send,
            visible: false,
            needs_repaint: true,
            start_time: Instant::now(),
            assistant_name,
            state,
            was_visible: false,
            theme,
            pending_events: Vec::new(),
            modifiers: egui::Modifiers::NONE,
            focused: false,
            voice_mode: false,
            voice_status: String::new(),
            audio_level: 0.0,
        }
    }

    /// Show the window
    pub fn show(&mut self) {
        self.visible = true;
        self.window.set_visible(true);
        self.window.set_focus();
        self.needs_repaint = true;
    }

    /// Hide the window
    pub fn hide(&mut self) {
        self.visible = false;
        self.window.set_visible(false);
    }

    /// Toggle window visibility
    pub fn toggle(&mut self) {
        if self.visible { self.hide(); } 
        else { self.show(); }
    }

    /// Add a message to the Log
    pub fn add_message(&mut self, message: LogMessage) {
        self.messages.push(message);
        self.needs_repaint = true;
    }

    fn update_state(&mut self, _delta: f32) {
        // 1. Process channel commands
        while let Ok(cmd) = self.receiver.try_recv() {
            match cmd {
                LogWindowCommand::Show => self.show(),
                LogWindowCommand::Hide => self.hide(),
                LogWindowCommand::Toggle => self.toggle(),
                LogWindowCommand::AddMessage(msg) => self.add_message(msg),
            }
        }

        // 1b. Poll brain responses

        // 2. Update visibility in GhostState
        self.state.set_log_visible(self.visible);

        // 3. Handle re-snap when becoming visible
        if self.visible && !self.was_visible {
            let (mx, my) = self.state.main_pos();
            let snap_cfg = self.state.snap_config();
            self.state.set_log_snapped(true);
            let snap_x = mx + snap_cfg.scaled_extra_offset[0];
            let snap_y = my + snap_cfg.scaled_extra_offset[1];
            self.set_position(snap_x, snap_y);
            self.state.set_log_last_set_pos(snap_x, snap_y);
        }
        self.was_visible = self.visible;

        // 4. Follow main window position if snapped
        if self.visible && self.state.log_snapped() {
            let (mx, my) = self.state.main_pos();
            let snap_cfg = self.state.snap_config();
            let new_x = mx + snap_cfg.scaled_extra_offset[0];
            let new_y = my + snap_cfg.scaled_extra_offset[1];
            let (last_x, last_y) = self.state.log_last_set_pos();
            if new_x != last_x || new_y != last_y {
                self.set_position(new_x, new_y);
                self.state.set_log_last_set_pos(new_x, new_y);
            }
        }
    }

    /// Handle window events (with snap/unsnap detection)
    fn on_window_event(&mut self, event: &WindowEvent) {
        // Track resize to suppress false unsnap from OS-generated Moved events
        if let WindowEvent::Resized(_) = event {
            self.state.set_log_just_resized(true);
        }

        // Detect user-initiated drag: unsnap or re-snap by proximity
        if let WindowEvent::Moved(position) = event {
            if self.state.log_just_resized() {
                // Resize caused this Moved event — update tracked position, don't unsnap
                self.state.set_log_just_resized(false);
                if self.state.log_snapped() {
                    self.state.set_log_last_set_pos(position.x, position.y);
                }
            } else {
                let (mx, my) = self.state.main_pos();
                let snap_cfg = self.state.snap_config();
                let snap_x = mx + snap_cfg.scaled_extra_offset[0];
                let snap_y = my + snap_cfg.scaled_extra_offset[1];

                if self.state.log_snapped() {
                    let (last_x, last_y) = self.state.log_last_set_pos();
                    let unsnap_tolerance = 10;
                    if (position.x - last_x).abs() > unsnap_tolerance
                        || (position.y - last_y).abs() > unsnap_tolerance
                    {
                        self.state.set_log_snapped(false);
                        log::info!("Log window unsnapped");
                    }
                } else {
                    let snap_tolerance = 30;
                    if (position.x - snap_x).abs() <= snap_tolerance
                        && (position.y - snap_y).abs() <= snap_tolerance
                    {
                        self.state.set_log_snapped(true);
                        self.state.set_log_last_set_pos(snap_x, snap_y);
                        self.set_position(snap_x, snap_y);
                        log::info!("Log window re-snapped");
                    }
                }
            }
        }

        self.handle_event_inner(event);
    }

    /// Handle window events (inner, for egui integration).
    ///
    /// Events are accumulated in `self.pending_events` and drained into
    /// `RawInput` at the start of each `render()` call.  This avoids the
    /// problem where `ctx.run(raw_input, ...)` discards events that were
    /// pushed via `ctx.input_mut()` between frames.
    fn handle_event_inner(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    self.config.width = size.width;
                    self.config.height = size.height;
                    self.surface.configure(&self.device, &self.config);
                    self.needs_repaint = true;
                }
            }
            WindowEvent::CloseRequested => {
                self.hide();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == tao::event::ElementState::Pressed;

                use tao::keyboard::KeyCode;
                let egui_key = match event.physical_key {
                    KeyCode::Escape => Some(egui::Key::Escape),
                    KeyCode::Tab => Some(egui::Key::Tab),
                    KeyCode::Backspace => Some(egui::Key::Backspace),
                    KeyCode::Enter | KeyCode::NumpadEnter => Some(egui::Key::Enter),
                    KeyCode::Space => Some(egui::Key::Space),
                    KeyCode::Delete => Some(egui::Key::Delete),
                    KeyCode::ArrowDown => Some(egui::Key::ArrowDown),
                    KeyCode::ArrowLeft => Some(egui::Key::ArrowLeft),
                    KeyCode::ArrowRight => Some(egui::Key::ArrowRight),
                    KeyCode::ArrowUp => Some(egui::Key::ArrowUp),
                    KeyCode::Home => Some(egui::Key::Home),
                    KeyCode::End => Some(egui::Key::End),
                    KeyCode::PageUp => Some(egui::Key::PageUp),
                    KeyCode::PageDown => Some(egui::Key::PageDown),
                    KeyCode::KeyA => Some(egui::Key::A),
                    KeyCode::KeyC => Some(egui::Key::C),
                    KeyCode::KeyV => Some(egui::Key::V),
                    KeyCode::KeyX => Some(egui::Key::X),
                    KeyCode::KeyZ => Some(egui::Key::Z),
                    _ => None,
                };

                if let Some(key) = egui_key {
                    self.pending_events.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: event.repeat,
                        modifiers: self.modifiers,
                    });
                }

                // Text event for printable characters (only on press)
                if pressed {
                    if let Some(text) = event.text {
                        if !text.chars().all(|c| c.is_control()) {
                            self.pending_events
                                .push(egui::Event::Text(text.to_string()));
                        }
                    }
                }
                self.needs_repaint = true;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers.alt = modifiers.alt_key();
                self.modifiers.ctrl = modifiers.control_key();
                self.modifiers.shift = modifiers.shift_key();
                self.modifiers.mac_cmd = modifiers.super_key();
                self.modifiers.command = if cfg!(target_os = "macos") {
                    modifiers.super_key()
                } else {
                    modifiers.control_key()
                };
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale_factor = self.window.scale_factor() as f32;
                let pos = egui::pos2(
                    position.x as f32 / scale_factor,
                    position.y as f32 / scale_factor,
                );
                self.pending_events.push(egui::Event::PointerMoved(pos));
                self.needs_repaint = true;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = *state == tao::event::ElementState::Pressed;
                let egui_button = match button {
                    tao::event::MouseButton::Left => egui::PointerButton::Primary,
                    tao::event::MouseButton::Right => egui::PointerButton::Secondary,
                    tao::event::MouseButton::Middle => egui::PointerButton::Middle,
                    _ => return,
                };
                // Use the most recent pointer position we know about
                let pos = self
                    .pending_events
                    .iter()
                    .rev()
                    .find_map(|e| {
                        if let egui::Event::PointerMoved(p) = e {
                            Some(*p)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                self.pending_events.push(egui::Event::PointerButton {
                    pos,
                    button: egui_button,
                    pressed,
                    modifiers: self.modifiers,
                });
                self.needs_repaint = true;
            }
            WindowEvent::Focused(focused) => {
                self.focused = *focused;
                self.needs_repaint = true;
            }
            _ => {}
        }
    }

    /// Check if repaint is needed
    pub fn needs_repaint(&self) -> bool { self.needs_repaint && self.visible }

    fn do_render(&mut self) {
        if !self.visible { return; }

        self.needs_repaint = false;

        let output = self.surface.get_current_texture();
        let output = match output {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                log::error!("Log window: Out of memory");
                return;
            }
            Err(e) => {
                log::error!("Log window surface error: {:?}", e);
                return;
            }
        };

        let view = output.texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Begin egui frame with time info for cursor blinking
        // IMPORTANT: screen_rect must be in LOGICAL pixels (physical / scale_factor)
        let scale_factor = self.window.scale_factor() as f32;
        let logical_width = self.config.width as f32 / scale_factor;
        let logical_height = self.config.height as f32 / scale_factor;

        // Set pixels_per_point on the context
        self.egui_ctx.set_pixels_per_point(scale_factor);

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(logical_width, logical_height),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            predicted_dt: 1.0 / 60.0,
            events: std::mem::take(&mut self.pending_events),
            modifiers: self.modifiers,
            focused: self.focused,
            ..Default::default()
        };

        // Clone data needed for UI
        let messages = self.messages.clone();
        let mut input_text = std::mem::take(&mut self.input_text);
        let on_send = self.on_send.clone();
        let assistant_name = self.assistant_name.clone();
        let voice_mode = self.voice_mode;
        let voice_status = self.voice_status.clone();
        let audio_level = self.audio_level;

        // Extract theme values for use inside the closure
        let theme = &self.theme;
        let widget_inactive_bg = theme.widget_inactive_bg;
        let widget_hovered_bg = theme.widget_hovered_bg;
        let widget_active_bg = theme.widget_active_bg;
        let input_panel_bg = theme.input_panel_bg;
        let input_panel_margin = theme.input_panel_margin;
        let input_hint = theme.input_hint.clone();
        let input_text_color = theme.input_text_color;
        let send_btn_color = theme.send_btn_color;
        let send_btn_text_color = theme.send_btn_text_color;
        let send_btn_rounding = theme.send_btn_rounding;
        let send_btn_size = theme.send_btn_size;
        let bg_color = theme.bg_color;
        let user_bubble_bg = theme.user_bubble_bg;
        let assistant_bubble_bg = theme.assistant_bubble_bg;
        let bubble_text_color = theme.bubble_text_color;
        let bubble_rounding = theme.bubble_rounding;
        let bubble_padding = theme.bubble_padding;
        let bubble_max_width_ratio = theme.bubble_max_width_ratio;
        let role_label_color = theme.role_label_color;
        let role_label_size = theme.role_label_size;
        let message_spacing = theme.message_spacing;

        // New messages to add after the frame
        let mut new_messages: Vec<LogMessage> = Vec::new();
        // Voice mode toggle requested this frame
        let mut new_voice_mode: Option<bool> = None;

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            // Dark theme style overrides
            let mut style = (*ctx.style()).clone();
            style.visuals.widgets.inactive.bg_fill = widget_inactive_bg;
            style.visuals.widgets.hovered.bg_fill = widget_hovered_bg;
            style.visuals.widgets.active.bg_fill = widget_active_bg;
            ctx.set_style(style);

            // Bottom panel: input row + status bar
            egui::TopBottomPanel::bottom("input_panel")
                .resizable(false)
                .frame(egui::Frame::none()
                    .fill(input_panel_bg)
                    .inner_margin(input_panel_margin))
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        // ── Row 1: input text with embedded send button (always visible) ──
                        {
                            let input_frame = egui::Frame::none()
                                .fill(widget_inactive_bg)
                                .rounding(egui::Rounding::same(8.0))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 70, 90)))
                                .inner_margin(egui::Margin::symmetric(8.0, 4.0));

                            let mut send_action = false;
                            input_frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let btn_w = 28.0;
                                    let gap = 6.0;
                                    let te_width = ui.available_width() - btn_w - gap;

                                    // Hint changes based on mode so the user knows what's happening
                                    let hint = if voice_mode {
                                        "Listening for voice..."
                                    } else {
                                        &input_hint
                                    };

                                    let te = egui::TextEdit::singleline(&mut input_text)
                                        .frame(false)
                                        .hint_text(hint)
                                        .desired_width(te_width)
                                        .text_color(input_text_color);

                                    let te_resp = ui.add(te);
                                    let enter = te_resp.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter));

                                    ui.add_space(gap);
                                    let send_btn = egui::Button::new(
                                        egui::RichText::new("►")
                                            .color(send_btn_text_color)
                                            .size(13.0),
                                    )
                                    .fill(send_btn_color)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(btn_w, 22.0));

                                    send_action = (ui.add(send_btn).clicked() || enter)
                                        && !input_text.trim().is_empty();
                                });
                            });

                            if send_action {
                                let user_msg = input_text.trim().to_string();
                                new_messages.push(LogMessage {
                                    role: "user".to_string(),
                                    content: user_msg.clone(),
                                });
                                if let Some(ref sender) = on_send {
                                    let _ = sender.send(user_msg.clone());
                                }
                                input_text.clear();
                            }
                        }

                        ui.add_space(5.0);

                        // ── Row 2: status | Mode dropdown | Mic level ──────────
                        ui.horizontal(|ui| {
                            let dim = egui::Color32::from_rgb(110, 110, 130);

                            // Status text (left) — colour-coded by pipeline phase
                            let status_str = if voice_status.is_empty() { "Ready" } else { &voice_status };
                            let status_color = match voice_status.as_str() {
                                s if s.starts_with("Hearing") => egui::Color32::from_rgb(60, 210, 110),
                                s if s.starts_with("Transcrib") || s.starts_with("Correct") => egui::Color32::from_rgb(250, 190, 50),
                                s if s.starts_with("Thinking") => egui::Color32::from_rgb(120, 170, 255),
                                s if s.contains("error") || s.contains("Error") => egui::Color32::from_rgb(220, 80, 80),
                                _ => dim,
                            };
                            ui.label(
                                egui::RichText::new(status_str)
                                    .color(status_color)
                                    .size(11.0)
                                    .italics(),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Mic level bar (rightmost, only in voice mode)
                                if voice_mode {
                                    let bar_color = if audio_level > 0.3 {
                                        egui::Color32::from_rgb(60, 200, 100)
                                    } else {
                                        egui::Color32::from_rgb(60, 120, 70)
                                    };
                                    ui.add(
                                        egui::ProgressBar::new(audio_level)
                                            .desired_width(70.0)
                                            .fill(bar_color),
                                    );
                                    ui.label(egui::RichText::new("Mic").color(dim).size(11.0));
                                    ui.separator();
                                }

                                // Mode dropdown
                                let mode_label = if voice_mode { "Voice" } else { "Text" };
                                egui::ComboBox::from_id_source("mode_combo")
                                    .selected_text(egui::RichText::new(mode_label).size(11.0))
                                    .width(60.0)
                                    .show_ui(ui, |ui| {
                                        let text_sel = ui.selectable_label(!voice_mode, "Text");
                                        let voice_sel = ui.selectable_label(voice_mode, "Voice");
                                        if text_sel.clicked() && voice_mode {
                                            new_voice_mode = Some(false);
                                        }
                                        if voice_sel.clicked() && !voice_mode {
                                            new_voice_mode = Some(true);
                                        }
                                    });
                                ui.label(egui::RichText::new("Mode").color(dim).size(11.0));
                            });
                        });
                    });
                });

            // Central panel: messages area
            egui::CentralPanel::default()
                .frame(egui::Frame::none()
                    .fill(bg_color)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0)))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let panel_width = ui.available_width();

                            for msg in &messages {
                                let is_user = msg.role == "user";
                                let role_label = if is_user {
                                    "You"
                                } else {
                                    &assistant_name
                                };

                                let msg_bg = if is_user {
                                    user_bubble_bg
                                } else {
                                    assistant_bubble_bg
                                };

                                let max_bubble_width = panel_width * bubble_max_width_ratio;

                                // Layout: right-aligned for user, left-aligned for assistant
                                if is_user {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                        ui.allocate_ui(egui::vec2(max_bubble_width, 0.0), |ui| {
                                            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                                // Role label
                                                ui.label(
                                                    egui::RichText::new(role_label)
                                                        .color(role_label_color)
                                                        .size(role_label_size)
                                                );
                                                // Bubble
                                                egui::Frame::none()
                                                    .fill(msg_bg)
                                                    .rounding(bubble_rounding)
                                                    .inner_margin(bubble_padding)
                                                    .show(ui, |ui| {
                                                        ui.set_max_width(max_bubble_width - 24.0);
                                                        ui.label(
                                                            egui::RichText::new(&msg.content)
                                                                .color(bubble_text_color)
                                                        );
                                                    });
                                            });
                                        });
                                    });
                                } else {
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                                        ui.allocate_ui(egui::vec2(max_bubble_width, 0.0), |ui| {
                                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                                // Role label
                                                ui.label(
                                                    egui::RichText::new(role_label)
                                                        .color(role_label_color)
                                                        .size(role_label_size)
                                                );
                                                // Bubble
                                                egui::Frame::none()
                                                    .fill(msg_bg)
                                                    .rounding(bubble_rounding)
                                                    .inner_margin(bubble_padding)
                                                    .show(ui, |ui| {
                                                        ui.set_max_width(max_bubble_width - 24.0);
                                                        ui.label(
                                                            egui::RichText::new(&msg.content)
                                                                .color(bubble_text_color)
                                                        );
                                                    });
                                            });
                                        });
                                    });
                                }
                                ui.add_space(message_spacing);
                            }

                            // Show "thinking..." indicator when waiting for brain
                            
                        });
                });
        });

        // Update state with new messages and input
        
        self.input_text = input_text;
        if let Some(vm) = new_voice_mode {
            self.voice_mode = vm;
            self.voice_status = if vm { "Listening".to_string() } else { String::new() };
            self.audio_level = 0.0;
        }

        // Handle repaint requests - check if there are pending animations
        if !full_output.shapes.is_empty() {
            self.needs_repaint = true;
        }

        // Process egui output
        let clipped_primitives = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

        // Update textures
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, delta);
        }

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Log Encoder"),
        });

        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &clipped_primitives,
            &screen_descriptor,
        );

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Log Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.094,
                            g: 0.094,
                            b: 0.125,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.egui_renderer.render(&mut render_pass, &clipped_primitives, &screen_descriptor);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Free textures
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
    }

}


impl ExtraWindow for LogWindow {
    fn window_id(&self) -> WindowId { self.window.id() }
    fn is_visible(&self) -> bool { self.visible }
    fn on_event(&mut self, event: &WindowEvent) { self.on_window_event(event); }
    fn update(&mut self, delta: f32) { self.update_state(delta); }
    fn render(&mut self) { self.do_render(); }
    fn request_redraw(&self) { if self.visible { self.window.request_redraw(); } }
    fn set_position(&self, x: i32, y: i32) { self.window.set_outer_position(tao::dpi::PhysicalPosition::new(x, y)); }
    fn bring_to_front(&self) {
        if self.visible {
            #[cfg(target_os = "macos")]
            {
                use tao::platform::macos::WindowExtMacOS;
                let ns_window = self.window.ns_window();
                unsafe {
                    use objc::{msg_send, sel, sel_impl};
                    let _: () = msg_send![ns_window as cocoa::base::id, orderFront: cocoa::base::nil];
                }
            }
            #[cfg(not(target_os = "macos"))]
            self.window.set_focus();
        }
    }
}
