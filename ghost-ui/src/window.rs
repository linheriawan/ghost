//! Ghost window creation and event handling

use std::path::Path;

use tao::{
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};
use thiserror::Error;

use crate::platform::configure_window;
use crate::renderer::{Renderer, RendererError};
use crate::skin::SkinData;
use crate::Skin;

/// Maximum texture size supported by most GPUs.
/// We use a conservative limit to ensure compatibility.
const MAX_TEXTURE_SIZE: u32 = 2048;

/// Default alpha threshold for hit testing (0-255).
/// Pixels with alpha <= this value are considered transparent.
const DEFAULT_ALPHA_THRESHOLD: u8 = 10;

#[derive(Error, Debug)]
pub enum WindowError {
    #[error("Failed to create window: {0}")]
    WindowCreationFailed(#[from] tao::error::OsError),
    #[error("Renderer error: {0}")]
    RendererError(#[from] RendererError),
    #[error("Skin error: {0}")]
    SkinError(#[from] crate::SkinError),
}

/// Configuration for creating a ghost window.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    /// Width of the window in pixels.
    pub width: u32,
    /// Height of the window in pixels.
    pub height: u32,
    /// Whether the window should always stay on top.
    pub always_on_top: bool,
    /// Whether mouse clicks should pass through the window entirely.
    pub click_through: bool,
    /// Whether the window can be dragged.
    pub draggable: bool,
    /// Window title (not visible for borderless windows).
    pub title: String,
    /// Opacity when window is focused (0.0 to 1.0).
    pub opacity_focused: f32,
    /// Opacity when window is unfocused (0.0 to 1.0).
    pub opacity_unfocused: f32,
    /// Whether to maintain aspect ratio when resizing.
    pub maintain_aspect_ratio: bool,
    /// Whether to use alpha-based hit testing (clicks on transparent areas pass through).
    pub alpha_hit_test: bool,
    /// Alpha threshold for hit testing (0-255). Pixels with alpha <= this are transparent.
    pub alpha_threshold: u8,
    /// Whether to change opacity based on focus state.
    pub focus_opacity_enabled: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 200,
            height: 200,
            always_on_top: true,
            click_through: false,
            draggable: true,
            title: "Ghost".to_string(),
            opacity_focused: 1.0,
            opacity_unfocused: 0.5,
            maintain_aspect_ratio: true,
            alpha_hit_test: true,
            alpha_threshold: DEFAULT_ALPHA_THRESHOLD,
            focus_opacity_enabled: true,
        }
    }
}

/// Clamp dimensions to fit within max texture size while maintaining aspect ratio.
fn clamp_to_max_size(width: u32, height: u32, max_size: u32) -> (u32, u32) {
    if width <= max_size && height <= max_size {
        return (width, height);
    }

    let aspect_ratio = width as f32 / height as f32;

    if width > height {
        // Width is the limiting factor
        let new_width = max_size;
        let new_height = (new_width as f32 / aspect_ratio).round() as u32;
        (new_width, new_height.min(max_size))
    } else {
        // Height is the limiting factor
        let new_height = max_size;
        let new_width = (new_height as f32 * aspect_ratio).round() as u32;
        (new_width.min(max_size), new_height)
    }
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

    /// Set the window position.
    pub fn set_position(&self, x: i32, y: i32) {
        self.data
            .window
            .set_outer_position(tao::dpi::LogicalPosition::new(x, y));
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

    /// Get a reference to the underlying tao window.
    pub fn window(&self) -> &Window {
        &self.data.window
    }

    /// Request a redraw of the window.
    pub fn request_redraw(&self) {
        self.data.window.request_redraw();
    }

    /// Render the current frame.
    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        if let Some(ref mut renderer) = self.renderer {
            renderer.render(self.data.skin.as_ref(), self.data.current_opacity)
        } else {
            Ok(())
        }
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

/// Builder for creating GhostWindow with a fluent API.
pub struct GhostWindowBuilder {
    config: WindowConfig,
    skin_bytes: Option<Vec<u8>>,
}

impl GhostWindowBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: WindowConfig::default(),
            skin_bytes: None,
        }
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

        Ok(window)
    }
}

impl Default for GhostWindowBuilder {
    fn default() -> Self {
        Self::new()
    }
}
