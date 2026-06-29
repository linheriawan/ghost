//! Generic ghost_ui follower window — wraps GhostWindow + GhostApp and
//! implements ExtraWindow so apps don't need per-window bridge structs.

use tao::event::{ElementState, MouseButton, WindowEvent};
use tao::window::{Window, WindowId};

use super::{ExtraWindow, GhostApp, GhostEvent, GhostWindow};
use crate::elements::Widget;
use crate::renderer::WidgetRenderer;

/// Wraps a GhostWindow + GhostApp pair and implements ExtraWindow.
///
/// Use this instead of writing a custom ExtraWindow wrapper for every
/// ghost_ui-based secondary window.  The app drives show/hide by returning
/// `Some(bool)` from `GhostApp::take_visibility()` inside its `update()`.
pub struct GhostFollower<A: GhostApp> {
    pub window: GhostWindow,
    pub app: A,
    widget_renderer: Option<WidgetRenderer>,
    gpu_initialized: bool,
    visible: bool,
}

impl<A: GhostApp> GhostFollower<A> {
    pub fn new(window: GhostWindow, app: A) -> Self {
        Self { window, app, widget_renderer: None, gpu_initialized: false, visible: false }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.window.set_visible(true);
        self.window.set_focus();
        self.window.request_redraw();
        self.app.on_visible_changed(true);
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.window.set_visible(false);
        self.app.on_visible_changed(false);
    }

    pub fn toggle(&mut self) {
        if self.visible { self.hide() } else { self.show() }
    }
}

impl<A: GhostApp> ExtraWindow for GhostFollower<A> {
    fn window_id(&self) -> WindowId { self.window.window().id() }

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

            WindowEvent::CursorEntered { .. } => {
                self.window.apply_hit_test(true);
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.window.handle_cursor_moved(*position);
                let (cx, cy) = (position.x as f32, position.y as f32);
                for btn in self.app.buttons_mut() { btn.update_hover(cx, cy, window_height); }
                for img in self.app.button_images_mut() { img.update_hover(cx, cy, window_height); }
                self.window.request_redraw();
            }

            WindowEvent::CursorLeft { .. } => {
                self.window.handle_cursor_left();
                for btn in self.app.buttons_mut() { btn.update_hover(-1.0, -1.0, window_height); }
                for img in self.app.button_images_mut() { img.update_hover(-1.0, -1.0, window_height); }
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left, ..
            } => {
                if let Some(pos) = self.window.cursor_position() {
                    let (cx, cy) = (pos.x as f32, pos.y as f32);
                    let pressed =
                        self.app.buttons_mut().iter_mut().any(|b| b.handle_press(cx, cy, window_height))
                        || self.app.button_images_mut().iter_mut().any(|b| b.handle_press(cx, cy, window_height));
                    if !pressed && self.window.should_handle_click() && self.window.is_draggable() {
                        self.window.drag();
                    }
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left, ..
            } => {
                if let Some(pos) = self.window.cursor_position() {
                    let (cx, cy) = (pos.x as f32, pos.y as f32);
                    let mut clicked: Vec<_> = self.app.buttons_mut().iter_mut()
                        .filter_map(|b| b.handle_release(cx, cy, window_height).then(|| b.id()))
                        .collect();
                    clicked.extend(self.app.button_images_mut().iter_mut()
                        .filter_map(|b| b.handle_release(cx, cy, window_height).then(|| b.id())));
                    for id in clicked { self.app.on_event(GhostEvent::ButtonClicked(id)); }
                }
            }

            _ => {}
        }
    }

    fn update(&mut self, delta: f32) {
        self.app.update(delta);
        if let Some(v) = self.app.take_visibility() {
            if v { self.show(); } else { self.hide(); }
        }
    }

    fn render(&mut self) {
        if !self.visible { return; }
        if !self.gpu_initialized {
            if let Some(wr) = self.window.init_app_gpu(&mut self.app) {
                self.widget_renderer = Some(wr);
                self.gpu_initialized = true;
            } else { return; }
        }
        let size = self.window.window().inner_size();
        let viewport = [size.width as f32, size.height as f32];
        if let Some(ref mut wr) = self.widget_renderer {
            self.window.widget_prepare(wr, &mut self.app, viewport);
        }
        self.window.prepare_app(&mut self.app, viewport);
        let _ = self.window.render_with_widgets_and_app(self.widget_renderer.as_ref(), &mut self.app);
    }

    fn request_redraw(&self) {
        if self.visible { self.window.request_redraw(); }
    }

    fn is_visible(&self) -> bool { self.visible }

    fn set_position(&self, x: i32, y: i32) { self.window.set_position(x, y); }

    fn bring_to_front(&self) {
        if self.visible { bring_to_front(self.window.window()); }
    }
}

#[allow(unused_variables)]
fn bring_to_front(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::WindowExtMacOS;
        let ns_window = window.ns_window();
        unsafe {
            use objc::{msg_send, sel, sel_impl};
            let _: () = msg_send![ns_window as cocoa::base::id, orderFront: cocoa::base::nil];
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window.set_focus();
}
