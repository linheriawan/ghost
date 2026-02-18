//! Chat window using egui with tao/wgpu integration
//!
//! This creates an egui-based chat window that integrates with the existing
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

use crate::config::ChatConfig;
use crate::vars::GhostState;

/// Visual theme for the chat window — all "what it looks like" in one place.
pub struct ChatTheme {
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

/// Define "what the chat window looks like" — all visual constants in one place.
///
/// For now values are code defaults. Having them in ChatTheme makes it easy
/// to add toml fields later.
fn ui_design(_config: &ChatConfig) -> ChatTheme {
    ChatTheme {
        bg_color: egui::Color32::from_rgb(24, 24, 32),
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

/// Message in the chat
#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: String, // "user" or "assistant"
    pub content: String,
}

/// Commands to control the chat window
#[derive(Debug)]
pub enum ChatWindowCommand {
    Show,
    Hide,
    Toggle,
    AddMessage(ChatMessage),
}

/// Channel for sending commands to the chat window
pub type ChatSender = Sender<ChatWindowCommand>;
pub type ChatReceiver = Receiver<ChatWindowCommand>;

/// Create a channel for chat window communication
pub fn create_chat_channel() -> (ChatSender, ChatReceiver) {
    channel()
}

/// Chat window state and rendering
pub struct ChatWindow {
    window: Window,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_renderer: egui_wgpu::Renderer,
    messages: Vec<ChatMessage>,
    input_text: String,
    receiver: ChatReceiver,
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
    theme: ChatTheme,
}

impl ChatWindow {
    /// Create a new chat window (starts hidden)
    pub fn new(
        event_loop: &EventLoop<()>,
        receiver: ChatReceiver,
        on_send: Option<Sender<String>>,
        size: [u32; 2],
        assistant_name: Option<String>,
        state: GhostState,
        chat_config: &ChatConfig,
    ) -> Self {
        // Create the window (hidden initially, no decorations for precise positioning)
        let window = WindowBuilder::new()
            .with_inner_size(LogicalSize::new(size[0], size[1]))
            .with_min_inner_size(LogicalSize::new(300, 400))
            .with_title("Ghost Chat")
            .with_visible(false)
            .with_decorations(true)
            .build(event_loop)
            .expect("Failed to create chat window");

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
                label: Some("Chat Window Device"),
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

        let theme = ui_design(chat_config);

        Self {
            window,
            surface,
            device,
            queue,
            config,
            egui_ctx,
            egui_renderer,
            messages: vec![ChatMessage {
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
        }
    }

    /// Get the window ID for event routing
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Check if the window is visible
    pub fn is_visible(&self) -> bool {
        self.visible
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
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    /// Add a message to the chat
    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
        self.needs_repaint = true;
    }

    /// Set the window position (in physical pixels)
    pub fn set_position(&self, x: i32, y: i32) {
        self.window
            .set_outer_position(tao::dpi::PhysicalPosition::new(x, y));
    }

    /// Bring window to front (without stealing focus)
    pub fn bring_to_front(&self) {
        if self.visible {
            #[cfg(target_os = "macos")]
            {
                use tao::platform::macos::WindowExtMacOS;
                // Get the NSWindow and call orderFront to bring to front without stealing focus
                let ns_window = self.window.ns_window();
                unsafe {
                    use objc::{msg_send, sel, sel_impl};
                    let _: () = msg_send![ns_window as cocoa::base::id, orderFront: cocoa::base::nil];
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                // On other platforms, set focus
                self.window.set_focus();
            }
        }
    }

    /// Process incoming commands and update snap/visibility state
    pub fn update_state(&mut self, _delta: f32) {
        // 1. Process channel commands
        while let Ok(cmd) = self.receiver.try_recv() {
            match cmd {
                ChatWindowCommand::Show => self.show(),
                ChatWindowCommand::Hide => self.hide(),
                ChatWindowCommand::Toggle => self.toggle(),
                ChatWindowCommand::AddMessage(msg) => self.add_message(msg),
            }
        }

        // 2. Update visibility in GhostState
        self.state.set_chat_visible(self.visible);

        // 3. Handle re-snap when becoming visible
        if self.visible && !self.was_visible {
            let (mx, my) = self.state.main_pos();
            let snap_cfg = self.state.snap_config();
            self.state.set_chat_snapped(true);
            let snap_x = mx + snap_cfg.scaled_extra_offset[0];
            let snap_y = my + snap_cfg.scaled_extra_offset[1];
            self.set_position(snap_x, snap_y);
            self.state.set_chat_last_set_pos(snap_x, snap_y);
        }
        self.was_visible = self.visible;

        // 4. Follow main window position if snapped
        if self.visible && self.state.chat_snapped() {
            let (mx, my) = self.state.main_pos();
            let snap_cfg = self.state.snap_config();
            let new_x = mx + snap_cfg.scaled_extra_offset[0];
            let new_y = my + snap_cfg.scaled_extra_offset[1];
            let (last_x, last_y) = self.state.chat_last_set_pos();
            if new_x != last_x || new_y != last_y {
                self.set_position(new_x, new_y);
                self.state.set_chat_last_set_pos(new_x, new_y);
            }
        }
    }

    /// Handle window events (with snap/unsnap detection)
    pub fn on_event(&mut self, event: &WindowEvent) {
        // Track resize to suppress false unsnap from OS-generated Moved events
        if let WindowEvent::Resized(_) = event {
            self.state.set_chat_just_resized(true);
        }

        // Detect user-initiated drag: unsnap or re-snap by proximity
        if let WindowEvent::Moved(position) = event {
            if self.state.chat_just_resized() {
                // Resize caused this Moved event — update tracked position, don't unsnap
                self.state.set_chat_just_resized(false);
                if self.state.chat_snapped() {
                    self.state.set_chat_last_set_pos(position.x, position.y);
                }
            } else {
                let (mx, my) = self.state.main_pos();
                let snap_cfg = self.state.snap_config();
                let snap_x = mx + snap_cfg.scaled_extra_offset[0];
                let snap_y = my + snap_cfg.scaled_extra_offset[1];

                if self.state.chat_snapped() {
                    let (last_x, last_y) = self.state.chat_last_set_pos();
                    let unsnap_tolerance = 10;
                    if (position.x - last_x).abs() > unsnap_tolerance
                        || (position.y - last_y).abs() > unsnap_tolerance
                    {
                        self.state.set_chat_snapped(false);
                        log::info!("Chat window unsnapped");
                    }
                } else {
                    let snap_tolerance = 30;
                    if (position.x - snap_x).abs() <= snap_tolerance
                        && (position.y - snap_y).abs() <= snap_tolerance
                    {
                        self.state.set_chat_snapped(true);
                        self.state.set_chat_last_set_pos(snap_x, snap_y);
                        self.set_position(snap_x, snap_y);
                        log::info!("Chat window re-snapped");
                    }
                }
            }
        }

        self.handle_event_inner(event);
    }

    /// Handle window events (inner, for egui integration)
    fn handle_event_inner(&mut self, event: &WindowEvent) {
        // Convert tao event to egui input
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

                // Convert tao KeyCode to egui key
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

                self.egui_ctx.input_mut(|i| {
                    // Send key event
                    if let Some(key) = egui_key {
                        i.events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: event.repeat,
                            modifiers: i.modifiers,
                        });
                    }

                    // Send text event for printable characters (only on press)
                    if pressed {
                        if let Some(text) = event.text {
                            // Don't send text for control characters
                            if !text.chars().all(|c| c.is_control()) {
                                i.events.push(egui::Event::Text(text.to_string()));
                            }
                        }
                    }
                });
                self.needs_repaint = true;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.egui_ctx.input_mut(|i| {
                    i.modifiers.alt = modifiers.alt_key();
                    i.modifiers.ctrl = modifiers.control_key();
                    i.modifiers.shift = modifiers.shift_key();
                    i.modifiers.mac_cmd = modifiers.super_key();
                    i.modifiers.command = if cfg!(target_os = "macos") {
                        modifiers.super_key()
                    } else {
                        modifiers.control_key()
                    };
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                // Convert physical pixels to logical pixels
                let scale_factor = self.window.scale_factor() as f32;
                let pos = egui::pos2(
                    position.x as f32 / scale_factor,
                    position.y as f32 / scale_factor,
                );
                self.egui_ctx.input_mut(|i| {
                    i.events.push(egui::Event::PointerMoved(pos));
                });
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
                self.egui_ctx.input_mut(|i| {
                    i.events.push(egui::Event::PointerButton {
                        pos: i.pointer.latest_pos().unwrap_or_default(),
                        button: egui_button,
                        pressed,
                        modifiers: i.modifiers,
                    });
                });
                self.needs_repaint = true;
            }
            WindowEvent::Focused(focused) => {
                self.egui_ctx.input_mut(|i| {
                    i.focused = *focused;
                });
                self.needs_repaint = true;
            }
            _ => {}
        }
    }

    /// Request a redraw
    pub fn request_redraw(&self) {
        if self.visible {
            self.window.request_redraw();
        }
    }

    /// Check if repaint is needed
    pub fn needs_repaint(&self) -> bool {
        self.needs_repaint && self.visible
    }

    /// Render the chat window
    pub fn render(&mut self) {
        if !self.visible {
            return;
        }

        self.needs_repaint = false;

        let output = self.surface.get_current_texture();
        let output = match output {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                log::error!("Chat window: Out of memory");
                return;
            }
            Err(e) => {
                log::error!("Chat window surface error: {:?}", e);
                return;
            }
        };

        let view = output
            .texture
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
            ..Default::default()
        };

        // Clone data needed for UI
        let messages = self.messages.clone();
        let mut input_text = std::mem::take(&mut self.input_text);
        let on_send = self.on_send.clone();
        let assistant_name = self.assistant_name.clone();

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
        let mut new_messages: Vec<ChatMessage> = Vec::new();

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            // Dark theme style overrides
            let mut style = (*ctx.style()).clone();
            style.visuals.widgets.inactive.bg_fill = widget_inactive_bg;
            style.visuals.widgets.hovered.bg_fill = widget_hovered_bg;
            style.visuals.widgets.active.bg_fill = widget_active_bg;
            ctx.set_style(style);

            // Bottom panel: input area
            egui::TopBottomPanel::bottom("input_panel")
                .resizable(false)
                .min_height(56.0)
                .frame(egui::Frame::none()
                    .fill(input_panel_bg)
                    .inner_margin(input_panel_margin))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let available = ui.available_width();

                        // Styled text input
                        let text_edit = egui::TextEdit::singleline(&mut input_text)
                            .hint_text(&input_hint)
                            .desired_width(available - 60.0)
                            .text_color(input_text_color)
                            .frame(true);

                        let response = ui.add(text_edit);

                        // Send on Enter key
                        let enter_pressed =
                            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        // Accent send button
                        let send_btn = egui::Button::new(
                            egui::RichText::new(">").color(send_btn_text_color).strong().size(16.0)
                        )
                        .fill(send_btn_color)
                        .rounding(send_btn_rounding)
                        .min_size(send_btn_size);

                        let send_clicked = ui.add(send_btn).clicked();

                        if (enter_pressed || send_clicked) && !input_text.trim().is_empty() {
                            let user_msg = input_text.trim().to_string();

                            new_messages.push(ChatMessage {
                                role: "user".to_string(),
                                content: user_msg.clone(),
                            });

                            if let Some(ref sender) = on_send {
                                let _ = sender.send(user_msg.clone());
                            }

                            new_messages.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: format!(
                                    "You said: \"{}\" (AI integration coming soon!)",
                                    user_msg
                                ),
                            });

                            input_text.clear();
                        }
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
                        });
                });
        });

        // Update state with new messages and input
        self.messages.extend(new_messages);
        self.input_text = input_text;

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
            label: Some("Chat Encoder"),
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
                label: Some("Chat Render Pass"),
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

/// Implement ExtraWindow trait for integration with ghost-ui event loop
impl ExtraWindow for ChatWindow {
    fn window_id(&self) -> WindowId {
        self.window.id()
    }

    fn on_event(&mut self, event: &WindowEvent) {
        ChatWindow::on_event(self, event);
    }

    fn update(&mut self, delta: f32) {
        self.update_state(delta);
    }

    fn render(&mut self) {
        ChatWindow::render(self);
    }

    fn request_redraw(&self) {
        ChatWindow::request_redraw(self);
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn set_position(&self, x: i32, y: i32) {
        ChatWindow::set_position(self, x, y);
    }

    fn bring_to_front(&self) {
        ChatWindow::bring_to_front(self);
    }
}
