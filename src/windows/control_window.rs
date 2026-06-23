//! Control window — transparent skin window using GhostWindow + GhostApp

use std::sync::mpsc::{channel, Receiver, Sender};

use ghost_ui::{
    skin, ExtraWindow, GhostApp, GhostEvent, GhostWindowBuilder, Widget, WidgetRenderer,
};
use tao::event::{ElementState, MouseButton, WindowEvent};
use tao::event_loop::EventLoop;
use tao::window::WindowId;

use super::callout_window::{CalloutCommand, CalloutSender};
use crate::bus::AppBus;

/// Commands to show or hide the control window
#[derive(Debug)]
pub enum ControlWindowCommand {
    Show,
    Hide,
    Toggle,
}

pub type ControlSender = Sender<ControlWindowCommand>;
pub type ControlReceiver = Receiver<ControlWindowCommand>;

pub fn create_control_channel() -> (ControlSender, ControlReceiver) { channel() }

/// Minimal GhostApp for the control window.
/// The skin (quad_arrow.png) is loaded onto the GhostWindow itself.
pub struct ControlApp { callout_tx: CalloutSender, }

impl ControlApp {
    pub fn new(callout_tx: CalloutSender) -> Self { Self { callout_tx } }

    fn send(&self, cmd: CalloutCommand) {
        if let Err(e) = self.callout_tx.send(cmd) {
            log::error!("Control: failed to send callout: {}", e);
        }
    }
}

impl GhostApp for ControlApp {
    fn on_event(&mut self, event: GhostEvent) {
        // Extend with buttons later; nothing to handle yet.
        let _ = event;
    }
}

/// Wraps GhostWindow + ControlApp to implement ExtraWindow.
pub struct ControlWindow {
    window: ghost_ui::GhostWindow,
    app: ControlApp,
    widget_renderer: Option<WidgetRenderer>,
    gpu_initialized: bool,
    receiver: ControlReceiver,
    visible: bool,
}

impl ControlWindow {
    pub fn new(event_loop: &EventLoop<()>, bus: &mut AppBus) -> Self {
        let receiver = bus.ctrl_rx.take().expect("ctrl_rx already consumed");
        let callout_tx = bus.callout_tx.clone();
        let skin_data = skin("assets/quad_arrow.png").unwrap_or_else(|e| {
            log::error!("Failed to load control skin: {}", e);
            panic!("Could not load assets/quad_arrow.png");
        });
        let window = GhostWindowBuilder::new()
            .with_size(skin_data.width(), skin_data.height())
            .with_always_on_top(true)
            .with_draggable(true)
            .with_click_through(false)
            .with_alpha_hit_test(true)
            .with_opacity_focused(1.0)
            .with_opacity_unfocused(0.7)
            .with_title("Ghost Controller")
            .with_skin_data(&skin_data)
            .build(event_loop)
            .expect("Failed to create controller window");

        Self {
            window,
            app: ControlApp::new(callout_tx),
            widget_renderer: None,
            gpu_initialized: false,
            receiver,
            visible: false,
        }
    }

    fn show(&mut self) {
        self.visible = true;
        self.window.set_visible(true);
        self.window.set_focus();
        self.window.request_redraw();
    }

    fn hide(&mut self) {
        self.visible = false;
        self.window.set_visible(false);
    }

    fn toggle(&mut self) {
        if self.visible { self.hide() } else { self.show() }
    }
}

impl ExtraWindow for ControlWindow {
    fn window_id(&self) -> WindowId {
        self.window.window().id()
    }

    fn on_event(&mut self, event: &WindowEvent) {
        let window_height = self.window.window().inner_size().height as f32;

        match event {
            WindowEvent::CloseRequested => self.hide(),

            WindowEvent::Resized(size) => {
                self.window.handle_resize(size.width, size.height);
                self.app.on_event(GhostEvent::Resized(size.width, size.height));
            }

            WindowEvent::Focused(focused) => {
                self.window.handle_focus(*focused);
                self.app.on_event(GhostEvent::FocusChanged(*focused));
            }

            WindowEvent::Moved(pos) => {
                self.app.on_event(GhostEvent::Moved(pos.x, pos.y));
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.window.handle_cursor_moved(*position);
                for btn in self.app.buttons_mut() {
                    btn.update_hover(position.x as f32, position.y as f32, window_height);
                }
            }

            WindowEvent::CursorLeft { .. } => {
                self.window.handle_cursor_left();
                for btn in self.app.buttons_mut() {
                    btn.update_hover(-1.0, -1.0, window_height);
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(pos) = self.window.cursor_position() {
                    let cx = pos.x as f32;
                    let cy = pos.y as f32;
                    let mut pressed = false;
                    for btn in self.app.buttons_mut() {
                        if btn.handle_press(cx, cy, window_height) {
                            pressed = true;
                            break;
                        }
                    }
                    if !pressed
                        && self.window.should_handle_click()
                        && self.window.is_draggable()
                    {
                        self.window.drag();
                    }
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(pos) = self.window.cursor_position() {
                    let cx = pos.x as f32;
                    let cy = pos.y as f32;
                    let clicked: Vec<_> = self
                        .app
                        .buttons_mut()
                        .iter_mut()
                        .filter_map(|btn| {
                            if btn.handle_release(cx, cy, window_height) {
                                Some(btn.id())
                            } else {
                                None
                            }
                        })
                        .collect();
                    for id in clicked {
                        self.app.on_event(GhostEvent::ButtonClicked(id));
                    }
                }
            }

            _ => {}
        }
    }

    fn update(&mut self, delta: f32) {
        while let Ok(cmd) = self.receiver.try_recv() {
            match cmd {
                ControlWindowCommand::Show => self.show(),
                ControlWindowCommand::Hide => self.hide(),
                ControlWindowCommand::Toggle => self.toggle(),
            }
        }
        if self.visible { self.app.update(delta); }
    }

    fn render(&mut self) {
        if !self.visible { return; }

        // Lazy GPU init on first render
        if !self.gpu_initialized {
            if let Some(wr) = self.window.init_app_gpu(&mut self.app) {
                self.widget_renderer = Some(wr);
                self.gpu_initialized = true;
            } 
            else { return; }
        }

        let size = self.window.window().inner_size();
        let viewport = [size.width as f32, size.height as f32];
        self.window.prepare_app(&mut self.app, viewport);
        let _ = self.window.render_with_widgets_and_app(self.widget_renderer.as_ref(), &mut self.app);
    }

    fn request_redraw(&self) {
        if self.visible { self.window.request_redraw(); }
    }

    fn is_visible(&self) -> bool { self.visible }

    fn set_position(&self, x: i32, y: i32) { self.window.set_position(x, y); }

    fn bring_to_front(&self) {
        if self.visible {
            #[cfg(target_os = "macos")]
            {
                use tao::platform::macos::WindowExtMacOS;
                let ns_window = self.window.window().ns_window();
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
