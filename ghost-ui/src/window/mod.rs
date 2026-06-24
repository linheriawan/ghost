//! Ghost window creation and event handling

mod config;
mod platform;

pub use config::{WindowConfig, CalloutWindowConfig};
use config::{clamp_to_max_size, MAX_TEXTURE_SIZE};

use std::path::Path;

use crate::elements::Widget;

use tao::{
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::WindowEvent,
    event_loop::EventLoop,
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
        // Re-evaluate click-through based on current cursor position regardless of focus.
        // Transparent areas stay pass-through even when unfocused; only opaque areas
        // can receive the click that refocuses the window.
        if self.data.config.alpha_hit_test && !self.data.config.click_through {
            let is_transparent = !self.hit_test_at_cursor();
            self.update_click_through(is_transparent);
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
        // Note: click-through is updated here for skin-only hit testing.
        // If the app has layers on top of transparent skin areas, call
        // apply_hit_test(hit) after this to include them.
        if self.data.config.alpha_hit_test && !self.data.config.click_through {
            let is_transparent = !self.hit_test_at_cursor();
            self.update_click_through(is_transparent);
        }
    }

    /// Override the click-through decision after handle_cursor_moved.
    /// Pass true if the cursor is over any opaque content (skin or layers).
    pub fn apply_hit_test(&self, hit: bool) {
        if self.data.config.alpha_hit_test && !self.data.config.click_through {
            self.update_click_through(!hit);
        }
    }

    /// Handle cursor leaving the window.
    pub fn handle_cursor_left(&mut self) {
        self.data.cursor_position = None;
        if self.data.config.alpha_hit_test && !self.data.config.click_through {
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

    /// Test if a point (in window physical pixels) is over a non-transparent skin pixel.
    pub fn hit_test_skin(&self, x: f32, y: f32) -> bool {
        let Some(ref skin) = self.data.skin else { return true };
        let Some((orig_w, orig_h)) = self.data.original_skin_size else { return true };
        let (win_w, win_h) = self.data.last_size;
        if win_w == 0 || win_h == 0 { return false; }
        let scale_x = orig_w as f64 / win_w as f64;
        let scale_y = orig_h as f64 / win_h as f64;
        skin.hit_test((x as f64 * scale_x) as f32, (y as f64 * scale_y) as f32, self.data.config.alpha_threshold)
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

    /// Initialize a GhostApp's GPU resources and return a WidgetRenderer.
    /// Returns None if the renderer is not yet ready (call again on next redraw).
    pub fn init_app_gpu<A: GhostApp>(&mut self, app: &mut A) -> Option<crate::renderer::WidgetRenderer> {
        if let Some(ref renderer) = self.renderer {
            for btn_img in app.button_images_mut() {
                btn_img.init_gpu(renderer.device(), renderer.queue());
            }
            app.init_gpu(GpuResources {
                device: renderer.device(),
                queue: renderer.queue(),
                format: renderer.format(),
            });
            Some(crate::renderer::WidgetRenderer::new(renderer.device(), renderer.queue(), renderer.format()))
        } else {
            None
        }
    }

    /// Prepare widgets (buttons, labels, marquees) and update marquee layout widths.
    /// Call before prepare_app and render_with_widgets_and_app.
    pub fn widget_prepare<A: GhostApp>(
        &self,
        widget_renderer: &mut crate::renderer::WidgetRenderer,
        app: &mut A,
        viewport: [f32; 2],
    ) {
        if let Some(ref renderer) = self.renderer {
            let sf = self.data.window.scale_factor() as f32;
            let marquee_widths = {
                let buttons = app.buttons();
                let button_images = app.button_images();
                let labels = app.labels();
                let marquees = app.marquee_labels();
                widget_renderer.prepare(
                    renderer.device(), renderer.queue(),
                    &buttons, &button_images, &labels, &marquees,
                    viewport, sf,
                )
            };
            if !marquee_widths.is_empty() {
                let mut marquees_mut = app.marquee_labels_mut();
                for (idx, width) in marquee_widths {
                    if let Some(m) = marquees_mut.get_mut(idx) {
                        m.set_text_width(width);
                    }
                }
            }
        }
    }

    /// Prepare a GhostApp for the current frame (call before render_with_widgets_and_app).
    pub fn prepare_app<A: GhostApp>(&self, app: &mut A, viewport: [f32; 2]) {
        if let Some(ref renderer) = self.renderer {
            let sf = self.data.window.scale_factor() as f32;
            let opacity = self.data.current_opacity;
            app.prepare(renderer.device(), renderer.queue(), viewport, sf, opacity);
        }
    }

    /// Initialize a CalloutApp's GPU resources.
    pub fn init_callout_gpu<C: CalloutApp>(&mut self, app: &mut C) {
        if let Some(ref renderer) = self.renderer {
            app.init_gpu(renderer.device(), renderer.queue(), renderer.format());
        }
    }

    /// Prepare a CalloutApp for the current frame (always full opacity).
    pub fn prepare_callout<C: CalloutApp>(&self, app: &mut C, viewport: [f32; 2]) {
        if let Some(ref renderer) = self.renderer {
            let sf = self.data.window.scale_factor() as f32;
            app.prepare(renderer.device(), renderer.queue(), viewport, sf, 1.0);
        }
    }

    /// Show or hide the underlying window.
    pub fn set_visible(&self, visible: bool) {
        self.data.window.set_visible(visible);
    }

    /// Set keyboard focus to this window.
    pub fn set_focus(&self) {
        self.data.window.set_focus();
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

    /// Return true if the given window-space point (physical pixels) is over
    /// any app-owned opaque content (layers, UI elements) that isn't part of
    /// the skin. The runner calls this after the skin hit test so layers are
    /// included in click-through decisions.
    fn hit_test(&self, _x: f32, _y: f32) -> bool { false }

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
    /// Called when the primary window moves; override to follow it.
    fn on_primary_moved(&self, _x: i32, _y: i32) {}
    /// Bring window to front (when main window is focused)
    fn bring_to_front(&self);
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
