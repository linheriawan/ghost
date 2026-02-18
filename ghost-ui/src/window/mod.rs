//! Ghost window creation and event handling

mod config;
mod platform;

pub use config::{WindowConfig, CalloutWindowConfig};
use config::{clamp_to_max_size, MAX_TEXTURE_SIZE};

use std::path::Path;

use crate::elements::Widget;

use tao::{
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};
use thiserror::Error;

use platform::configure_window;
use crate::renderer::{Renderer, RendererError};
use crate::skin::SkinData;
use crate::Skin;

#[derive(Error, Debug)]
pub enum WindowError {
    #[error("Failed to create window: {0}")]
    WindowCreationFailed(#[from] tao::error::OsError),
    #[error("Renderer error: {0}")]
    RendererError(#[from] RendererError),
    #[error("Skin error: {0}")]
    SkinError(#[from] crate::SkinError),
}

/// Internal struct to hold window data with proper ownership.
struct WindowData {
    window: Window,
    skin: Option<Skin>,
    config: WindowConfig,
    aspect_ratio: f32,
    last_size: (u32, u32),
    /// Current cursor position in window coordinates
    cursor_position: Option<PhysicalPosition<f64>>,
    /// Original skin dimensions (before any scaling)
    original_skin_size: Option<(u32, u32)>,
    /// Whether the window is currently focused
    is_focused: bool,
    /// Current opacity (computed from focus state)
    current_opacity: f32,
    /// Skin offset within the window [x, y] in pixels
    skin_offset: [f32; 2],
}

/// A transparent, shaped window for ghost UI elements.
pub struct GhostWindow {
    data: Box<WindowData>,
    renderer: Option<Renderer<'static>>,
}

impl GhostWindow {
    /// Create a new ghost window with the given configuration.
    pub fn new(event_loop: &EventLoop<()>, config: WindowConfig) -> Result<Self, WindowError> {
        // Clamp size to GPU limits
        let (clamped_width, clamped_height) =
            clamp_to_max_size(config.width, config.height, MAX_TEXTURE_SIZE);

        if clamped_width != config.width || clamped_height != config.height {
            log::warn!(
                "Window size {}x{} exceeds GPU limit, clamped to {}x{}",
                config.width,
                config.height,
                clamped_width,
                clamped_height
            );
        }

        let aspect_ratio = clamped_width as f32 / clamped_height as f32;
        let initial_opacity = if config.focus_opacity_enabled {
            config.opacity_unfocused // Start unfocused
        } else {
            config.opacity_focused
        };

        let window = WindowBuilder::new()
            .with_inner_size(LogicalSize::new(clamped_width, clamped_height))
            .with_transparent(true)
            .with_decorations(false)
            .with_always_on_top(config.always_on_top)
            .with_title(&config.title)
            .build(event_loop)?;

        // Apply platform-specific configuration
        configure_window(&window, config.click_through);

        let window_size = window.inner_size();

        // Store window data in a box
        let data = Box::new(WindowData {
            window,
            skin: None,
            config,
            aspect_ratio,
            last_size: (window_size.width, window_size.height),
            cursor_position: None,
            original_skin_size: None,
            is_focused: false,
            current_opacity: initial_opacity,
            skin_offset: [0.0, 0.0],
        });

        // Create renderer with a reference to the boxed window
        // SAFETY: The window lives in the Box which won't move. We transmute the lifetime
        // to 'static because the Box lives as long as GhostWindow.
        let renderer = unsafe {
            let window_ref: &'static Window = std::mem::transmute(&data.window);
            Renderer::new(window_ref, window_size.width, window_size.height)?
        };

        Ok(Self {
            data,
            renderer: Some(renderer),
        })
    }

    /// Set the skin for the window.
    pub fn set_skin(&mut self, skin: Skin) {
        self.data.original_skin_size = Some((skin.width(), skin.height()));
        self.data.skin = Some(skin);
    }

    /// Load and set a skin from PNG bytes.
    pub fn load_skin_from_bytes(&mut self, bytes: &[u8]) -> Result<(), crate::SkinError> {
        if let Some(ref renderer) = self.renderer {
            let skin = Skin::from_png_bytes(bytes, renderer.device(), renderer.queue())?;
            self.data.original_skin_size = Some((skin.width(), skin.height()));
            self.data.skin = Some(skin);
        }
        Ok(())
    }

    /// Load and set a skin from a file path (for runtime skin switching).
    pub fn load_skin_from_path(&mut self, path: impl AsRef<Path>) -> Result<(), crate::SkinError> {
        let bytes = std::fs::read(path)?;
        self.load_skin_from_bytes(&bytes)
    }

    /// Load and set a skin from SkinData (for runtime skin switching).
    pub fn load_skin_from_data(&mut self, data: &SkinData) -> Result<(), crate::SkinError> {
        self.load_skin_from_bytes(data.bytes())
    }

    /// Set the window position (in physical pixels).
    pub fn set_position(&self, x: i32, y: i32) {
        self.data
            .window
            .set_outer_position(tao::dpi::PhysicalPosition::new(x, y));
    }

    /// Set the opacity directly (bypasses focus-based opacity).
    pub fn set_opacity(&mut self, opacity: f32) {
        self.data.current_opacity = opacity.clamp(0.0, 1.0);
    }

    /// Set the focused opacity.
    pub fn set_opacity_focused(&mut self, opacity: f32) {
        self.data.config.opacity_focused = opacity.clamp(0.0, 1.0);
        if self.data.is_focused && self.data.config.focus_opacity_enabled {
            self.data.current_opacity = self.data.config.opacity_focused;
        }
    }

    /// Set the unfocused opacity.
    pub fn set_opacity_unfocused(&mut self, opacity: f32) {
        self.data.config.opacity_unfocused = opacity.clamp(0.0, 1.0);
        if !self.data.is_focused && self.data.config.focus_opacity_enabled {
            self.data.current_opacity = self.data.config.opacity_unfocused;
        }
    }

    /// Enable or disable focus-based opacity changes.
    pub fn set_focus_opacity_enabled(&mut self, enabled: bool) {
        self.data.config.focus_opacity_enabled = enabled;
        if enabled {
            self.update_opacity_for_focus();
        }
    }

    /// Get the current opacity value.
    pub fn opacity(&self) -> f32 {
        self.data.current_opacity
    }

    /// Get a reference to the underlying tao window.
    pub fn window(&self) -> &Window {
        &self.data.window
    }

    /// Request a redraw of the window.
    pub fn request_redraw(&self) {
        self.data.window.request_redraw();
    }

    /// Set the skin offset within the window.
    pub fn set_skin_offset(&mut self, offset: [f32; 2]) {
        self.data.skin_offset = offset;
    }

    /// Get the skin offset within the window.
    pub fn skin_offset(&self) -> [f32; 2] {
        self.data.skin_offset
    }

    /// Render the current frame.
    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        if let Some(ref mut renderer) = self.renderer {
            renderer.render(self.data.skin.as_ref(), self.data.current_opacity)
        } else {
            Ok(())
        }
    }

    /// Render with additional content (for widgets, etc.)
    pub fn render_with_extra<F>(&mut self, extra: F) -> Result<(), wgpu::SurfaceError>
    where
        F: FnOnce(&mut wgpu::RenderPass<'_>),
    {
        if let Some(ref mut renderer) = self.renderer {
            renderer.render_with_extra(
                self.data.skin.as_ref(),
                self.data.current_opacity,
                self.data.skin_offset,
                extra,
            )
        } else {
            Ok(())
        }
    }

    /// Render with button widgets
    pub fn render_with_buttons(
        &mut self,
        button_renderer: Option<&crate::renderer::ButtonRenderer>,
    ) -> Result<(), wgpu::SurfaceError> {
        if let Some(ref mut renderer) = self.renderer {
            renderer.render_with_buttons(
                self.data.skin.as_ref(),
                self.data.current_opacity,
                self.data.skin_offset,
                button_renderer,
            )
        } else {
            Ok(())
        }
    }

    /// Render with button widgets and app's custom rendering
    pub fn render_with_buttons_and_app<A: GhostApp>(
        &mut self,
        button_renderer: Option<&crate::renderer::ButtonRenderer>,
        app: &mut A,
    ) -> Result<(), wgpu::SurfaceError> {
        if let Some(ref mut renderer) = self.renderer {
            // Get app's skin first (if any) to avoid borrow conflicts
            // We use a raw pointer to work around the borrow checker
            let app_skin_ptr = app.current_skin().map(|s| s as *const crate::Skin);
            let skin = match app_skin_ptr {
                Some(ptr) => Some(unsafe { &*ptr }),
                None => self.data.skin.as_ref(),
            };
            renderer.render_with_buttons_and_app(
                skin,
                self.data.current_opacity,
                self.data.skin_offset,
                button_renderer,
                app,
            )
        } else {
            Ok(())
        }
    }

    /// Render with widget renderer and app's custom rendering
    pub fn render_with_widgets_and_app<A: GhostApp>(
        &mut self,
        widget_renderer: Option<&crate::renderer::WidgetRenderer>,
        app: &mut A,
    ) -> Result<(), wgpu::SurfaceError> {
        if let Some(ref mut renderer) = self.renderer {
            let app_skin_ptr = app.current_skin().map(|s| s as *const crate::Skin);
            let skin = match app_skin_ptr {
                Some(ptr) => Some(unsafe { &*ptr }),
                None => self.data.skin.as_ref(),
            };
            renderer.render_with_widgets_and_app(
                skin,
                self.data.current_opacity,
                self.data.skin_offset,
                widget_renderer,
                app,
            )
        } else {
            Ok(())
        }
    }

    /// Render a callout window (transparent, no skin)
    pub fn render_callout<C: CalloutApp>(&mut self, app: &C) -> Result<(), wgpu::SurfaceError> {
        if let Some(ref mut renderer) = self.renderer {
            renderer.render_callout(app)
        } else {
            Ok(())
        }
    }

    /// Get the current cursor position (in screen coordinates)
    pub fn cursor_position(&self) -> Option<PhysicalPosition<f64>> {
        self.data.cursor_position
    }

    /// Handle focus change.
    pub fn handle_focus(&mut self, focused: bool) {
        self.data.is_focused = focused;
        if self.data.config.focus_opacity_enabled {
            self.update_opacity_for_focus();
        }

        // When unfocused, disable click-through so user can click to focus
        // When focused, re-enable alpha-based click-through
        if self.data.config.alpha_hit_test && !self.data.config.click_through {
            if !focused {
                // Unfocused: always accept clicks so user can focus the window
                self.update_click_through(false);
            } else {
                // Focused: re-evaluate based on current cursor position
                let is_transparent = !self.hit_test_at_cursor();
                self.update_click_through(is_transparent);
            }
        }
    }

    /// Update opacity based on current focus state.
    fn update_opacity_for_focus(&mut self) {
        self.data.current_opacity = if self.data.is_focused {
            self.data.config.opacity_focused
        } else {
            self.data.config.opacity_unfocused
        };
    }

    /// Handle cursor movement.
    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.data.cursor_position = Some(position);

        // Only apply alpha-based click-through when focused
        // When unfocused, we want all clicks to reach the window so it can be focused
        if self.data.config.alpha_hit_test && !self.data.config.click_through && self.data.is_focused {
            let is_transparent = !self.hit_test_at_cursor();
            self.update_click_through(is_transparent);
        }
    }

    /// Handle cursor leaving the window.
    pub fn handle_cursor_left(&mut self) {
        self.data.cursor_position = None;
        // Re-enable click handling when cursor leaves (only matters when focused)
        if self.data.config.alpha_hit_test && !self.data.config.click_through && self.data.is_focused {
            self.update_click_through(false);
        }
    }

    /// Update platform click-through state.
    #[allow(unused_variables)]
    fn update_click_through(&self, transparent: bool) {
        #[cfg(target_os = "macos")]
        {
            #[allow(unused_imports)]
            use tao::platform::macos::WindowExtMacOS;
            let _ = self.data.window.set_ignore_cursor_events(transparent);
        }

        // Windows and Linux don't have easy per-pixel click-through,
        // so we handle it in the event loop instead
    }

    /// Test if the cursor is over a non-transparent pixel.
    fn hit_test_at_cursor(&self) -> bool {
        let Some(cursor_pos) = self.data.cursor_position else {
            return false;
        };

        let Some(ref skin) = self.data.skin else {
            return true; // No skin = solid window
        };

        let Some((orig_w, orig_h)) = self.data.original_skin_size else {
            return true;
        };

        // Get current window size
        let (win_w, win_h) = self.data.last_size;
        if win_w == 0 || win_h == 0 {
            return false;
        }

        // Scale cursor position to skin coordinates
        let scale_x = orig_w as f64 / win_w as f64;
        let scale_y = orig_h as f64 / win_h as f64;

        let skin_x = (cursor_pos.x * scale_x) as f32;
        let skin_y = (cursor_pos.y * scale_y) as f32;

        skin.hit_test(skin_x, skin_y, self.data.config.alpha_threshold)
    }

    /// Check if a click at the current cursor position should be handled.
    pub fn should_handle_click(&self) -> bool {
        if self.data.config.click_through {
            return false;
        }

        if !self.data.config.alpha_hit_test {
            return true;
        }

        self.hit_test_at_cursor()
    }

    /// Handle a window resize event, maintaining aspect ratio if configured.
    pub fn handle_resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        // Clamp to max texture size
        let (clamped_width, clamped_height) = clamp_to_max_size(width, height, MAX_TEXTURE_SIZE);

        let (final_width, final_height) = if self.data.config.maintain_aspect_ratio {
            // Determine which dimension changed more
            let (last_w, last_h) = self.data.last_size;
            let width_delta = (clamped_width as i32 - last_w as i32).abs();
            let height_delta = (clamped_height as i32 - last_h as i32).abs();

            if width_delta > height_delta {
                // Width changed more, adjust height to match aspect ratio
                let new_height = (clamped_width as f32 / self.data.aspect_ratio).round() as u32;
                (clamped_width, new_height.max(1))
            } else {
                // Height changed more, adjust width to match aspect ratio
                let new_width = (clamped_height as f32 * self.data.aspect_ratio).round() as u32;
                (new_width.max(1), clamped_height)
            }
        } else {
            (clamped_width, clamped_height)
        };

        // Clamp again after aspect ratio adjustment
        let (final_width, final_height) =
            clamp_to_max_size(final_width, final_height, MAX_TEXTURE_SIZE);

        // Update window size if aspect ratio changed it
        if self.data.config.maintain_aspect_ratio
            && (final_width != clamped_width || final_height != clamped_height)
        {
            self.data
                .window
                .set_inner_size(PhysicalSize::new(final_width, final_height));
        }

        self.data.last_size = (final_width, final_height);

        if let Some(ref mut renderer) = self.renderer {
            renderer.resize(final_width, final_height);
        }
    }

    /// Check if the window is draggable.
    pub fn is_draggable(&self) -> bool {
        self.data.config.draggable
    }

    /// Check if the window is focused.
    pub fn is_focused(&self) -> bool {
        self.data.is_focused
    }

    /// Start dragging the window.
    pub fn drag(&self) {
        let _ = self.data.window.drag_window();
    }

    /// Get the window's outer position (screen coordinates).
    pub fn outer_position(&self) -> Option<(i32, i32)> {
        self.data.window.outer_position().ok().map(|p| (p.x, p.y))
    }

    /// Get the current aspect ratio.
    pub fn aspect_ratio(&self) -> f32 {
        self.data.aspect_ratio
    }

    /// Set whether to maintain aspect ratio during resize.
    pub fn set_maintain_aspect_ratio(&mut self, maintain: bool) {
        self.data.config.maintain_aspect_ratio = maintain;
    }

    /// Set the alpha threshold for hit testing.
    pub fn set_alpha_threshold(&mut self, threshold: u8) {
        self.data.config.alpha_threshold = threshold;
    }
}

/// Events that can be emitted by the ghost window
#[derive(Debug, Clone)]
pub enum GhostEvent {
    /// A button was clicked
    ButtonClicked(crate::elements::ButtonId),
    /// Window was focused or unfocused
    FocusChanged(bool),
    /// Window was resized
    Resized(u32, u32),
    /// Window was moved (x, y in screen coordinates)
    Moved(i32, i32),
    /// Frame update (for animations)
    Update(f32), // delta time in seconds
}

/// GPU resources for app initialization
pub struct GpuResources<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub format: wgpu::TextureFormat,
}

/// Application trait for handling ghost window events
pub trait GhostApp {
    /// Called once when GPU resources are available (for initializing renderers)
    fn init_gpu(&mut self, _gpu: GpuResources<'_>) {}

    /// Called each frame to update state (e.g., animations)
    /// delta is the time since the last frame in seconds
    fn update(&mut self, _delta: f32) {}

    /// Return true if the app wants to quit
    fn should_quit(&self) -> bool {
        false
    }

    /// Called when an event occurs
    fn on_event(&mut self, event: GhostEvent);

    /// Called before rendering, return buttons to render
    fn buttons(&self) -> Vec<&crate::elements::Button> {
        Vec::new()
    }

    /// Called to update button states (for hover effects, etc.)
    fn buttons_mut(&mut self) -> Vec<&mut crate::elements::Button> {
        Vec::new()
    }

    /// Return image buttons to render
    fn button_images(&self) -> Vec<&crate::elements::ButtonImage> {
        Vec::new()
    }

    /// Return mutable image buttons (for hover/press state updates)
    fn button_images_mut(&mut self) -> Vec<&mut crate::elements::ButtonImage> {
        Vec::new()
    }

    /// Return labels to render
    fn labels(&self) -> Vec<&crate::elements::Label> {
        Vec::new()
    }

    /// Return marquee labels to render
    fn marquee_labels(&self) -> Vec<&crate::elements::MarqueeLabel> {
        Vec::new()
    }

    /// Return mutable marquee labels (for scroll animation updates)
    fn marquee_labels_mut(&mut self) -> Vec<&mut crate::elements::MarqueeLabel> {
        Vec::new()
    }

    /// Return the current skin to render (for animated skins)
    /// If None, the window's static skin will be used
    fn current_skin(&self) -> Option<&crate::Skin> {
        None
    }

    /// Return true if the app needs continuous frame updates (for animations)
    /// When true, the event loop will use Poll instead of Wait
    fn needs_continuous_update(&self) -> bool {
        self.current_skin().is_some()
    }

    /// Return the target frames per second for animations (default: 30)
    fn target_fps(&self) -> f32 {
        30.0
    }

    /// Called before rendering to prepare GPU resources (callouts, etc.)
    /// scale_factor is the display's DPI scale (1.0 for standard, 2.0 for Retina)
    /// opacity is the current window opacity (0.0 to 1.0)
    fn prepare(&mut self, _device: &wgpu::Device, _queue: &wgpu::Queue, _viewport: [f32; 2], _scale_factor: f32, _opacity: f32) {}

    /// Called during rendering to render layers and text overlays
    /// This is called after the main skin is rendered but before buttons
    fn render_layers<'a>(
        &'a mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _viewport: [f32; 2],
        _render_pass: &mut wgpu::RenderPass<'a>,
    ) {}
}

/// Run the ghost window event loop with custom event handling.
///
/// This takes ownership of the GhostWindow and runs until the window is closed.
pub fn run_with_app<A: GhostApp + 'static>(
    mut ghost_window: GhostWindow,
    event_loop: EventLoop<()>,
    mut app: A,
) {
    use std::time::Instant;

    let mut last_frame = Instant::now();
    let mut widget_renderer: Option<crate::renderer::WidgetRenderer> = None;
    let mut gpu_initialized = false;

    event_loop.run(move |event, _, control_flow| {
        // Always use Poll for continuous rendering
        *control_flow = ControlFlow::Poll;

        let window_size = ghost_window.window().inner_size();
        let window_height = window_size.height as f32;

        match event {
            Event::WindowEvent {
                event: WindowEvent::Focused(focused),
                ..
            } => {
                ghost_window.handle_focus(focused);
                app.on_event(GhostEvent::FocusChanged(focused));
                ghost_window.request_redraw();
            }

            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                ghost_window.handle_cursor_moved(position);

                // Update button hover states
                let cursor_x = position.x as f32;
                let cursor_y = position.y as f32;
                for button in app.buttons_mut() {
                    button.update_hover(cursor_x, cursor_y, window_height);
                }
                for button_image in app.button_images_mut() {
                    button_image.update_hover(cursor_x, cursor_y, window_height);
                }

                ghost_window.request_redraw();
            }

            Event::WindowEvent {
                event: WindowEvent::CursorLeft { .. },
                ..
            } => {
                ghost_window.handle_cursor_left();

                // Reset button states
                for button in app.buttons_mut() {
                    button.update_hover(-1.0, -1.0, window_height);
                }
                for button_image in app.button_images_mut() {
                    button_image.update_hover(-1.0, -1.0, window_height);
                }
            }

            Event::WindowEvent {
                event: WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                },
                ..
            } => {
                if let Some(cursor_pos) = ghost_window.cursor_position() {
                    let cursor_x = cursor_pos.x as f32;
                    let cursor_y = cursor_pos.y as f32;

                    // Check if any button was pressed
                    let mut button_pressed = false;
                    for button in app.buttons_mut() {
                        if button.handle_press(cursor_x, cursor_y, window_height) {
                            button_pressed = true;
                            break;
                        }
                    }
                    if !button_pressed {
                        for button_image in app.button_images_mut() {
                            if button_image.handle_press(cursor_x, cursor_y, window_height) {
                                button_pressed = true;
                                break;
                            }
                        }
                    }

                    if !button_pressed && ghost_window.should_handle_click() && ghost_window.is_draggable() {
                        ghost_window.drag();
                    }

                    ghost_window.request_redraw();
                }
            }

            Event::WindowEvent {
                event: WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    ..
                },
                ..
            } => {
                if let Some(cursor_pos) = ghost_window.cursor_position() {
                    let cursor_x = cursor_pos.x as f32;
                    let cursor_y = cursor_pos.y as f32;

                    // Check if any button was released (clicked) - collect IDs first
                    let mut clicked_ids: Vec<_> = app
                        .buttons_mut()
                        .iter_mut()
                        .filter_map(|button| {
                            if button.handle_release(cursor_x, cursor_y, window_height) {
                                Some(button.id())
                            } else {
                                None
                            }
                        })
                        .collect();

                    // Also check image buttons
                    let img_clicked_ids: Vec<_> = app
                        .button_images_mut()
                        .iter_mut()
                        .filter_map(|button| {
                            if button.handle_release(cursor_x, cursor_y, window_height) {
                                Some(button.id())
                            } else {
                                None
                            }
                        })
                        .collect();
                    clicked_ids.extend(img_clicked_ids);

                    // Then emit events
                    for id in clicked_ids {
                        app.on_event(GhostEvent::ButtonClicked(id));
                    }

                    ghost_window.request_redraw();
                }
            }

            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                ghost_window.handle_resize(size.width, size.height);
                app.on_event(GhostEvent::Resized(size.width, size.height));
            }

            Event::WindowEvent {
                event: WindowEvent::Moved(position),
                ..
            } => {
                app.on_event(GhostEvent::Moved(position.x, position.y));
            }

            Event::MainEventsCleared => {
                let now = Instant::now();
                let delta = now.duration_since(last_frame).as_secs_f32();
                last_frame = now;

                app.update(delta);
                app.on_event(GhostEvent::Update(delta));

                // Update marquee label animations
                for marquee in app.marquee_labels_mut() {
                    marquee.update(delta);
                }

                // Check if app wants to quit
                if app.should_quit() {
                    *control_flow = ControlFlow::Exit;
                    return;
                }

                ghost_window.request_redraw();
            }

            Event::RedrawRequested(_) => {
                // Initialize GPU resources if needed
                if !gpu_initialized {
                    if let Some(ref renderer) = ghost_window.renderer {
                        // Initialize widget renderer
                        widget_renderer = Some(crate::renderer::WidgetRenderer::new(
                            renderer.device(),
                            renderer.queue(),
                            renderer.format(),
                        ));

                        // Initialize image button GPU resources
                        for btn_img in app.button_images_mut() {
                            btn_img.init_gpu(renderer.device(), renderer.queue());
                        }

                        // Let app initialize its GPU resources
                        app.init_gpu(GpuResources {
                            device: renderer.device(),
                            queue: renderer.queue(),
                            format: renderer.format(),
                        });

                        gpu_initialized = true;
                    }
                }

                // Prepare widgets for rendering
                let viewport = [window_size.width as f32, window_size.height as f32];
                let mut marquee_widths = Vec::new();
                if let (Some(ref mut wid_renderer), Some(ref renderer)) =
                    (&mut widget_renderer, &ghost_window.renderer)
                {
                    let scale_factor = ghost_window.window().scale_factor() as f32;
                    let buttons: Vec<&crate::elements::Button> = app.buttons();
                    let button_images: Vec<&crate::elements::ButtonImage> = app.button_images();
                    let labels: Vec<&crate::elements::Label> = app.labels();
                    let marquees: Vec<&crate::elements::MarqueeLabel> = app.marquee_labels();
                    marquee_widths = wid_renderer.prepare(
                        renderer.device(),
                        renderer.queue(),
                        &buttons,
                        &button_images,
                        &labels,
                        &marquees,
                        viewport,
                        scale_factor,
                    );
                }
                // Apply measured text widths to marquee labels
                if !marquee_widths.is_empty() {
                    let mut marquees_mut = app.marquee_labels_mut();
                    for (idx, width) in marquee_widths {
                        if let Some(m) = marquees_mut.get_mut(idx) {
                            m.set_text_width(width);
                        }
                    }
                }

                // Let app prepare its rendering (callouts, etc.)
                if let Some(ref renderer) = ghost_window.renderer {
                    let scale_factor = ghost_window.window().scale_factor() as f32;
                    let opacity = ghost_window.opacity();
                    app.prepare(renderer.device(), renderer.queue(), viewport, scale_factor, opacity);
                }

                // Render with widgets and app's extra rendering
                let render_result = ghost_window.render_with_widgets_and_app(
                    widget_renderer.as_ref(),
                    &mut app,
                );
                if let Err(e) = render_result {
                    log::error!("Render error: {:?}", e);
                    match e {
                        wgpu::SurfaceError::Lost => {
                            let size = ghost_window.window().inner_size();
                            ghost_window.handle_resize(size.width, size.height);
                        }
                        wgpu::SurfaceError::OutOfMemory => {
                            *control_flow = ControlFlow::Exit;
                        }
                        _ => {}
                    }
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }

            _ => (),
        }
    });
}

/// Run the ghost window event loop.
///
/// This takes ownership of the GhostWindow and runs until the window is closed.
pub fn run(mut ghost_window: GhostWindow, event_loop: EventLoop<()>) {
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::Focused(focused),
                ..
            } => {
                ghost_window.handle_focus(focused);
                ghost_window.request_redraw();
            }

            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                ghost_window.handle_cursor_moved(position);
            }

            Event::WindowEvent {
                event: WindowEvent::CursorLeft { .. },
                ..
            } => {
                ghost_window.handle_cursor_left();
            }

            Event::WindowEvent {
                event: WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                },
                ..
            } => {
                if ghost_window.should_handle_click() && ghost_window.is_draggable() {
                    ghost_window.drag();
                }
            }

            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                ghost_window.handle_resize(size.width, size.height);
            }

            Event::MainEventsCleared => {
                ghost_window.request_redraw();
            }

            Event::RedrawRequested(_) => {
                if let Err(e) = ghost_window.render() {
                    log::error!("Render error: {:?}", e);
                    match e {
                        wgpu::SurfaceError::Lost => {
                            let size = ghost_window.window().inner_size();
                            ghost_window.handle_resize(size.width, size.height);
                        }
                        wgpu::SurfaceError::OutOfMemory => {
                            *control_flow = ControlFlow::Exit;
                        }
                        _ => {}
                    }
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }

            _ => (),
        }
    });
}

/// Application trait for callout rendering
pub trait CalloutApp {
    /// Called once when GPU resources are available
    fn init_gpu(&mut self, _device: &wgpu::Device, _queue: &wgpu::Queue, _format: wgpu::TextureFormat) {}

    /// Called before rendering to prepare GPU resources
    fn prepare(&mut self, _device: &wgpu::Device, _queue: &wgpu::Queue, _viewport: [f32; 2], _scale_factor: f32, _opacity: f32) {}

    /// Called during rendering
    fn render<'a>(&'a self, _render_pass: &mut wgpu::RenderPass<'a>) {}

    /// Called on update (for animations). Returns true if redraw is needed.
    fn update(&mut self, _delta: f32) -> bool { false }
}

/// Run the ghost window with a linked callout window.
///
/// The callout window follows the main window, positioned at the given offset.
pub fn run_with_app_and_callout<A, C>(
    mut main_window: GhostWindow,
    mut callout_window: GhostWindow,
    callout_offset: [i32; 2],
    event_loop: EventLoop<()>,
    mut app: A,
    mut callout_app: C,
) where
    A: GhostApp + 'static,
    C: CalloutApp + 'static,
{
    use std::time::Instant;

    let main_window_id = main_window.window().id();
    let callout_window_id = callout_window.window().id();

    let mut last_frame = Instant::now();
    let mut widget_renderer: Option<crate::renderer::WidgetRenderer> = None;
    let mut main_gpu_initialized = false;
    let mut callout_gpu_initialized = false;

    // Get scale factor for converting logical to physical offsets
    let scale_factor = main_window.window().scale_factor();
    let scaled_callout_offset = [
        (callout_offset[0] as f64 * scale_factor) as i32,
        (callout_offset[1] as f64 * scale_factor) as i32,
    ];

    // Position callout window initially
    if let Some((x, y)) = main_window.outer_position() {
        callout_window.set_position(x + scaled_callout_offset[0], y + scaled_callout_offset[1]);
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent { window_id, event, .. } if window_id == main_window_id => {
                let window_size = main_window.window().inner_size();
                let window_height = window_size.height as f32;

                match event {
                    WindowEvent::Focused(focused) => {
                        main_window.handle_focus(focused);
                        app.on_event(GhostEvent::FocusChanged(focused));
                        main_window.request_redraw();
                    }

                    WindowEvent::CursorMoved { position, .. } => {
                        main_window.handle_cursor_moved(position);
                        let cursor_x = position.x as f32;
                        let cursor_y = position.y as f32;
                        for button in app.buttons_mut() {
                            button.update_hover(cursor_x, cursor_y, window_height);
                        }
                        for button_image in app.button_images_mut() {
                            button_image.update_hover(cursor_x, cursor_y, window_height);
                        }
                        main_window.request_redraw();
                    }

                    WindowEvent::CursorLeft { .. } => {
                        main_window.handle_cursor_left();
                        for button in app.buttons_mut() {
                            button.update_hover(-1.0, -1.0, window_height);
                        }
                        for button_image in app.button_images_mut() {
                            button_image.update_hover(-1.0, -1.0, window_height);
                        }
                    }

                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Left,
                        ..
                    } => {
                        if let Some(cursor_pos) = main_window.cursor_position() {
                            let cursor_x = cursor_pos.x as f32;
                            let cursor_y = cursor_pos.y as f32;
                            let mut button_pressed = false;
                            for button in app.buttons_mut() {
                                if button.handle_press(cursor_x, cursor_y, window_height) {
                                    button_pressed = true;
                                    break;
                                }
                            }
                            if !button_pressed {
                                for button_image in app.button_images_mut() {
                                    if button_image.handle_press(cursor_x, cursor_y, window_height) {
                                        button_pressed = true;
                                        break;
                                    }
                                }
                            }
                            if !button_pressed && main_window.should_handle_click() && main_window.is_draggable() {
                                main_window.drag();
                            }
                            main_window.request_redraw();
                        }
                    }

                    WindowEvent::MouseInput {
                        state: ElementState::Released,
                        button: MouseButton::Left,
                        ..
                    } => {
                        if let Some(cursor_pos) = main_window.cursor_position() {
                            let cursor_x = cursor_pos.x as f32;
                            let cursor_y = cursor_pos.y as f32;
                            let mut clicked_ids: Vec<_> = app
                                .buttons_mut()
                                .iter_mut()
                                .filter_map(|button| {
                                    if button.handle_release(cursor_x, cursor_y, window_height) {
                                        Some(button.id())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            let img_clicked_ids: Vec<_> = app
                                .button_images_mut()
                                .iter_mut()
                                .filter_map(|button| {
                                    if button.handle_release(cursor_x, cursor_y, window_height) {
                                        Some(button.id())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            clicked_ids.extend(img_clicked_ids);
                            for id in clicked_ids {
                                app.on_event(GhostEvent::ButtonClicked(id));
                            }
                            main_window.request_redraw();
                        }
                    }

                    WindowEvent::Resized(size) => {
                        main_window.handle_resize(size.width, size.height);
                        app.on_event(GhostEvent::Resized(size.width, size.height));
                    }

                    WindowEvent::Moved(position) => {
                        // Update callout window position to follow main window
                        callout_window.set_position(
                            position.x + scaled_callout_offset[0],
                            position.y + scaled_callout_offset[1],
                        );
                        app.on_event(GhostEvent::Moved(position.x, position.y));
                    }

                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }

                    _ => {}
                }
            }

            Event::WindowEvent { window_id, event, .. } if window_id == callout_window_id => {
                match event {
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }
                    _ => {}
                }
            }

            Event::MainEventsCleared => {
                let now = Instant::now();
                let delta = now.duration_since(last_frame).as_secs_f32();
                last_frame = now;

                app.update(delta);
                app.on_event(GhostEvent::Update(delta));
                callout_app.update(delta);

                // Update marquee labels
                for marquee in app.marquee_labels_mut() {
                    marquee.update(delta);
                }

                // Check if app wants to quit
                if app.should_quit() {
                    *control_flow = ControlFlow::Exit;
                    return;
                }

                main_window.request_redraw();
                callout_window.request_redraw();
            }

            Event::RedrawRequested(window_id) if window_id == main_window_id => {
                let window_size = main_window.window().inner_size();

                // Initialize GPU resources if needed
                if !main_gpu_initialized {
                    if let Some(ref renderer) = main_window.renderer {
                        widget_renderer = Some(crate::renderer::WidgetRenderer::new(
                            renderer.device(),
                            renderer.queue(),
                            renderer.format(),
                        ));
                        for btn_img in app.button_images_mut() {
                            btn_img.init_gpu(renderer.device(), renderer.queue());
                        }
                        app.init_gpu(GpuResources {
                            device: renderer.device(),
                            queue: renderer.queue(),
                            format: renderer.format(),
                        });
                        main_gpu_initialized = true;
                    }
                }

                // Prepare and render main window
                let viewport = [window_size.width as f32, window_size.height as f32];
                let mut marquee_widths = Vec::new();
                if let (Some(ref mut wid_renderer), Some(ref renderer)) =
                    (&mut widget_renderer, &main_window.renderer)
                {
                    let scale_factor = main_window.window().scale_factor() as f32;
                    let buttons: Vec<&crate::elements::Button> = app.buttons();
                    let button_images: Vec<&crate::elements::ButtonImage> = app.button_images();
                    let labels: Vec<&crate::elements::Label> = app.labels();
                    let marquees: Vec<&crate::elements::MarqueeLabel> = app.marquee_labels();
                    marquee_widths = wid_renderer.prepare(
                        renderer.device(),
                        renderer.queue(),
                        &buttons,
                        &button_images,
                        &labels,
                        &marquees,
                        viewport,
                        scale_factor,
                    );
                }
                if !marquee_widths.is_empty() {
                    let mut marquees_mut = app.marquee_labels_mut();
                    for (idx, width) in marquee_widths {
                        if let Some(m) = marquees_mut.get_mut(idx) {
                            m.set_text_width(width);
                        }
                    }
                }

                if let Some(ref renderer) = main_window.renderer {
                    let scale_factor = main_window.window().scale_factor() as f32;
                    let opacity = main_window.opacity();
                    app.prepare(renderer.device(), renderer.queue(), viewport, scale_factor, opacity);
                }

                let _ = main_window.render_with_widgets_and_app(widget_renderer.as_ref(), &mut app);
            }

            Event::RedrawRequested(window_id) if window_id == callout_window_id => {
                let window_size = callout_window.window().inner_size();

                // Initialize GPU resources if needed
                if !callout_gpu_initialized {
                    if let Some(ref renderer) = callout_window.renderer {
                        callout_app.init_gpu(
                            renderer.device(),
                            renderer.queue(),
                            renderer.format(),
                        );
                        callout_gpu_initialized = true;
                    }
                }

                // Prepare callout (always full opacity)
                if let Some(ref renderer) = callout_window.renderer {
                    let viewport = [window_size.width as f32, window_size.height as f32];
                    let scale_factor = callout_window.window().scale_factor() as f32;
                    callout_app.prepare(renderer.device(), renderer.queue(), viewport, scale_factor, 1.0);
                }

                // Render callout window
                let _ = callout_window.render_callout(&callout_app);
            }

            _ => (),
        }
    });
}

/// Trait for extra windows that can be managed by the event loop
pub trait ExtraWindow {
    /// Get the window ID for event routing
    fn window_id(&self) -> tao::window::WindowId;
    /// Handle a window event
    fn on_event(&mut self, event: &WindowEvent);
    /// Process commands/updates (called each frame)
    fn update(&mut self, delta: f32);
    /// Render the window
    fn render(&mut self);
    /// Request a redraw
    fn request_redraw(&self);
    /// Check if visible
    fn is_visible(&self) -> bool;
    /// Set window position (for following main window)
    fn set_position(&self, x: i32, y: i32);
    /// Bring window to front (when main window is focused)
    fn bring_to_front(&self);
}

/// Run the ghost window with a linked callout window and optional extra window (like chat).
///
/// The callout window follows the main window, positioned at the given offset.
/// The extra window manages its own snap/follow logic via GhostState.
pub fn run_with_app_callout_and_extra<A, C, E>(
    mut main_window: GhostWindow,
    mut callout_window: GhostWindow,
    callout_offset: [i32; 2],
    event_loop: EventLoop<()>,
    mut app: A,
    mut callout_app: C,
    mut extra_window: Option<E>,
) where
    A: GhostApp + 'static,
    C: CalloutApp + 'static,
    E: ExtraWindow + 'static,
{
    use std::time::Instant;

    let main_window_id = main_window.window().id();
    let callout_window_id = callout_window.window().id();
    let extra_window_id = extra_window.as_ref().map(|e| e.window_id());

    let mut last_frame = Instant::now();
    let mut widget_renderer: Option<crate::renderer::WidgetRenderer> = None;
    let mut main_gpu_initialized = false;
    let mut callout_gpu_initialized = false;

    // Get scale factor for converting logical to physical offsets
    let scale_factor = main_window.window().scale_factor();
    let scaled_callout_offset = [
        (callout_offset[0] as f64 * scale_factor) as i32,
        (callout_offset[1] as f64 * scale_factor) as i32,
    ];

    log::debug!(
        "Scale factor: {}, callout_offset: {:?} -> {:?}",
        scale_factor, callout_offset, scaled_callout_offset
    );

    // Position callout window initially
    if let Some((x, y)) = main_window.outer_position() {
        callout_window.set_position(x + scaled_callout_offset[0], y + scaled_callout_offset[1]);
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent { window_id, event, .. } if window_id == main_window_id => {
                let window_size = main_window.window().inner_size();
                let window_height = window_size.height as f32;

                match event {
                    WindowEvent::Focused(focused) => {
                        main_window.handle_focus(focused);
                        app.on_event(GhostEvent::FocusChanged(focused));
                        main_window.request_redraw();

                        // Bring extra window to front when main window is focused
                        if focused {
                            if let Some(ref extra) = extra_window {
                                extra.bring_to_front();
                            }
                        }
                    }

                    WindowEvent::CursorMoved { position, .. } => {
                        main_window.handle_cursor_moved(position);
                        let cursor_x = position.x as f32;
                        let cursor_y = position.y as f32;
                        for button in app.buttons_mut() {
                            button.update_hover(cursor_x, cursor_y, window_height);
                        }
                        for button_image in app.button_images_mut() {
                            button_image.update_hover(cursor_x, cursor_y, window_height);
                        }
                        main_window.request_redraw();
                    }

                    WindowEvent::CursorLeft { .. } => {
                        main_window.handle_cursor_left();
                        for button in app.buttons_mut() {
                            button.update_hover(-1.0, -1.0, window_height);
                        }
                        for button_image in app.button_images_mut() {
                            button_image.update_hover(-1.0, -1.0, window_height);
                        }
                    }

                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Left,
                        ..
                    } => {
                        if let Some(cursor_pos) = main_window.cursor_position() {
                            let cursor_x = cursor_pos.x as f32;
                            let cursor_y = cursor_pos.y as f32;
                            let mut button_pressed = false;
                            for button in app.buttons_mut() {
                                if button.handle_press(cursor_x, cursor_y, window_height) {
                                    button_pressed = true;
                                    break;
                                }
                            }
                            if !button_pressed {
                                for button_image in app.button_images_mut() {
                                    if button_image.handle_press(cursor_x, cursor_y, window_height) {
                                        button_pressed = true;
                                        break;
                                    }
                                }
                            }
                            if !button_pressed && main_window.should_handle_click() && main_window.is_draggable() {
                                main_window.drag();
                            }
                            main_window.request_redraw();
                        }
                    }

                    WindowEvent::MouseInput {
                        state: ElementState::Released,
                        button: MouseButton::Left,
                        ..
                    } => {
                        if let Some(cursor_pos) = main_window.cursor_position() {
                            let cursor_x = cursor_pos.x as f32;
                            let cursor_y = cursor_pos.y as f32;
                            let mut clicked_ids: Vec<_> = app
                                .buttons_mut()
                                .iter_mut()
                                .filter_map(|button| {
                                    if button.handle_release(cursor_x, cursor_y, window_height) {
                                        Some(button.id())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            let img_clicked_ids: Vec<_> = app
                                .button_images_mut()
                                .iter_mut()
                                .filter_map(|button| {
                                    if button.handle_release(cursor_x, cursor_y, window_height) {
                                        Some(button.id())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            clicked_ids.extend(img_clicked_ids);
                            for id in clicked_ids {
                                app.on_event(GhostEvent::ButtonClicked(id));
                            }
                            main_window.request_redraw();
                        }
                    }

                    WindowEvent::Resized(size) => {
                        main_window.handle_resize(size.width, size.height);
                        app.on_event(GhostEvent::Resized(size.width, size.height));
                    }

                    WindowEvent::Moved(position) => {
                        // Callout always follows main window
                        callout_window.set_position(
                            position.x + scaled_callout_offset[0],
                            position.y + scaled_callout_offset[1],
                        );
                        app.on_event(GhostEvent::Moved(position.x, position.y));
                    }

                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }

                    _ => {}
                }
            }

            Event::WindowEvent { window_id, event, .. } if window_id == callout_window_id => {
                match event {
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }
                    _ => {}
                }
            }

            Event::WindowEvent { window_id, event, .. } if Some(window_id) == extra_window_id => {
                if let Some(ref mut extra) = extra_window {
                    extra.on_event(&event);
                    extra.request_redraw();
                }
            }

            Event::MainEventsCleared => {
                let now = Instant::now();
                let delta = now.duration_since(last_frame).as_secs_f32();

                let target_fps = app.current_skin()
                    .map(|_| 24.0)
                    .unwrap_or(10.0);
                let min_frame_time = 1.0 / target_fps;

                last_frame = now;

                app.update(delta);
                app.on_event(GhostEvent::Update(delta));
                let callout_changed = callout_app.update(delta);

                // Update marquee label animations
                for marquee in app.marquee_labels_mut() {
                    marquee.update(delta);
                }

                // Update extra window
                if let Some(ref mut extra) = extra_window {
                    extra.update(delta);
                }

                if app.should_quit() {
                    *control_flow = ControlFlow::Exit;
                    return;
                }

                if app.current_skin().is_some() {
                    main_window.request_redraw();
                }

                if callout_changed {
                    callout_window.request_redraw();
                }

                if let Some(ref extra) = extra_window {
                    if extra.is_visible() {
                        extra.request_redraw();
                    }
                }

                *control_flow = ControlFlow::WaitUntil(
                    now + std::time::Duration::from_secs_f32(min_frame_time)
                );
            }

            Event::RedrawRequested(window_id) if window_id == main_window_id => {
                let window_size = main_window.window().inner_size();

                if !main_gpu_initialized {
                    if let Some(ref renderer) = main_window.renderer {
                        widget_renderer = Some(crate::renderer::WidgetRenderer::new(
                            renderer.device(),
                            renderer.queue(),
                            renderer.format(),
                        ));
                        for btn_img in app.button_images_mut() {
                            btn_img.init_gpu(renderer.device(), renderer.queue());
                        }
                        app.init_gpu(GpuResources {
                            device: renderer.device(),
                            queue: renderer.queue(),
                            format: renderer.format(),
                        });
                        main_gpu_initialized = true;
                    }
                }

                let viewport = [window_size.width as f32, window_size.height as f32];
                let mut marquee_widths = Vec::new();
                if let (Some(ref mut wid_renderer), Some(ref renderer)) =
                    (&mut widget_renderer, &main_window.renderer)
                {
                    let scale_factor = main_window.window().scale_factor() as f32;
                    let buttons: Vec<&crate::elements::Button> = app.buttons();
                    let button_images: Vec<&crate::elements::ButtonImage> = app.button_images();
                    let labels: Vec<&crate::elements::Label> = app.labels();
                    let marquees: Vec<&crate::elements::MarqueeLabel> = app.marquee_labels();
                    marquee_widths = wid_renderer.prepare(
                        renderer.device(),
                        renderer.queue(),
                        &buttons,
                        &button_images,
                        &labels,
                        &marquees,
                        viewport,
                        scale_factor,
                    );
                }
                if !marquee_widths.is_empty() {
                    let mut marquees_mut = app.marquee_labels_mut();
                    for (idx, width) in marquee_widths {
                        if let Some(m) = marquees_mut.get_mut(idx) {
                            m.set_text_width(width);
                        }
                    }
                }

                if let Some(ref renderer) = main_window.renderer {
                    let scale_factor = main_window.window().scale_factor() as f32;
                    let opacity = main_window.opacity();
                    app.prepare(renderer.device(), renderer.queue(), viewport, scale_factor, opacity);
                }

                let _ = main_window.render_with_widgets_and_app(widget_renderer.as_ref(), &mut app);
            }

            Event::RedrawRequested(window_id) if window_id == callout_window_id => {
                let window_size = callout_window.window().inner_size();

                if !callout_gpu_initialized {
                    if let Some(ref renderer) = callout_window.renderer {
                        callout_app.init_gpu(
                            renderer.device(),
                            renderer.queue(),
                            renderer.format(),
                        );
                        callout_gpu_initialized = true;
                    }
                }

                if let Some(ref renderer) = callout_window.renderer {
                    let viewport = [window_size.width as f32, window_size.height as f32];
                    let scale_factor = callout_window.window().scale_factor() as f32;
                    callout_app.prepare(renderer.device(), renderer.queue(), viewport, scale_factor, 1.0);
                }

                let _ = callout_window.render_callout(&callout_app);
            }

            Event::RedrawRequested(window_id) if Some(window_id) == extra_window_id => {
                if let Some(ref mut extra) = extra_window {
                    extra.render();
                }
            }

            _ => (),
        }
    });
}

/// Builder for creating GhostWindow with a fluent API.
pub struct GhostWindowBuilder {
    config: WindowConfig,
    skin_bytes: Option<Vec<u8>>,
    skin_offset: [f32; 2],
}

impl GhostWindowBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: WindowConfig::default(),
            skin_bytes: None,
            skin_offset: [0.0, 0.0],
        }
    }

    /// Set the skin offset within the window.
    /// This is used when the window is larger than the skin to accommodate callouts.
    pub fn with_skin_offset(mut self, offset: [f32; 2]) -> Self {
        self.skin_offset = offset;
        self
    }

    /// Set the window size. Will be automatically clamped to GPU limits.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.config.width = width;
        self.config.height = height;
        self
    }

    /// Set whether the window should always be on top.
    pub fn with_always_on_top(mut self, always_on_top: bool) -> Self {
        self.config.always_on_top = always_on_top;
        self
    }

    /// Set whether clicks should pass through the window entirely.
    pub fn with_click_through(mut self, click_through: bool) -> Self {
        self.config.click_through = click_through;
        self
    }

    /// Set whether the window is draggable.
    pub fn with_draggable(mut self, draggable: bool) -> Self {
        self.config.draggable = draggable;
        self
    }

    /// Set the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.config.title = title.into();
        self
    }

    /// Set the opacity when focused (default: 1.0).
    pub fn with_opacity_focused(mut self, opacity: f32) -> Self {
        self.config.opacity_focused = opacity;
        self
    }

    /// Set the opacity when unfocused (default: 0.5).
    pub fn with_opacity_unfocused(mut self, opacity: f32) -> Self {
        self.config.opacity_unfocused = opacity;
        self
    }

    /// Set both focused and unfocused opacity to the same value.
    /// This effectively disables focus-based opacity changes.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.config.opacity_focused = opacity;
        self.config.opacity_unfocused = opacity;
        self.config.focus_opacity_enabled = false;
        self
    }

    /// Enable or disable focus-based opacity changes (default: true).
    pub fn with_focus_opacity(mut self, enabled: bool) -> Self {
        self.config.focus_opacity_enabled = enabled;
        self
    }

    /// Set whether to maintain aspect ratio during resize (default: true).
    pub fn with_maintain_aspect_ratio(mut self, maintain: bool) -> Self {
        self.config.maintain_aspect_ratio = maintain;
        self
    }

    /// Set whether to use alpha-based hit testing (default: true).
    /// When enabled, clicks on transparent pixels pass through to windows below.
    pub fn with_alpha_hit_test(mut self, enabled: bool) -> Self {
        self.config.alpha_hit_test = enabled;
        self
    }

    /// Set the alpha threshold for hit testing (default: 10).
    /// Pixels with alpha <= this value are considered transparent.
    pub fn with_alpha_threshold(mut self, threshold: u8) -> Self {
        self.config.alpha_threshold = threshold;
        self
    }

    /// Set the skin from PNG bytes.
    pub fn with_skin_bytes(mut self, bytes: &[u8]) -> Self {
        self.skin_bytes = Some(bytes.to_vec());
        self
    }

    /// Set the skin from SkinData.
    pub fn with_skin_data(mut self, data: &SkinData) -> Self {
        self.skin_bytes = Some(data.bytes().to_vec());
        self
    }

    /// Build the GhostWindow.
    pub fn build(self, event_loop: &EventLoop<()>) -> Result<GhostWindow, WindowError> {
        let mut window = GhostWindow::new(event_loop, self.config)?;

        if let Some(bytes) = self.skin_bytes {
            window.load_skin_from_bytes(&bytes)?;
        }

        // Set skin offset
        window.set_skin_offset(self.skin_offset);

        Ok(window)
    }
}

impl Default for GhostWindowBuilder {
    fn default() -> Self {
        Self::new()
    }
}
