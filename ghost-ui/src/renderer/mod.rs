//! wgpu-based renderer for ghost windows

mod button;
mod sprite;

pub use button::ButtonRenderer;
pub use sprite::SpritePipeline;

use tao::window::Window;
use thiserror::Error;
use wgpu::{Device, Queue, Surface, SurfaceConfiguration, TextureFormat};

use crate::Skin;

#[derive(Error, Debug)]
pub enum RendererError {
    #[error("Failed to create wgpu adapter")]
    AdapterCreationFailed,
    #[error("Failed to request wgpu device: {0}")]
    DeviceRequestFailed(#[from] wgpu::RequestDeviceError),
    #[error("Failed to create surface: {0}")]
    SurfaceCreationFailed(#[from] wgpu::CreateSurfaceError),
    #[error("Surface configuration failed")]
    SurfaceConfigFailed,
}

pub struct Renderer<'window> {
    device: Device,
    queue: Queue,
    surface: Surface<'window>,
    config: SurfaceConfiguration,
    sprite_pipeline: SpritePipeline,
}

impl<'window> Renderer<'window> {
    /// Create a new renderer for the given tao window.
    pub fn new(window: &'window Window, width: u32, height: u32) -> Result<Self, RendererError> {
        use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};

        // Create wgpu instance
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // Get raw handles from tao window (which uses raw-window-handle 0.5)
        // and convert them to the format wgpu expects (raw-window-handle 0.6)
        let raw_window_handle = window.raw_window_handle();
        let raw_display_handle = window.raw_display_handle();

        // Convert 0.5 handles to 0.6 format
        let window_handle = convert_window_handle(raw_window_handle);
        let display_handle = convert_display_handle(raw_display_handle);

        // Create surface using the converted handles
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: display_handle,
                raw_window_handle: window_handle,
            })
        }?;

        // Request adapter
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or(RendererError::AdapterCreationFailed)?;

        // Request device
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("ghost-ui device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            },
            None,
        ))?;

        // Configure surface
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        // Find best alpha mode for transparency
        let alpha_mode = if surface_caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if surface_caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            surface_caps.alpha_modes[0]
        };

        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create sprite pipeline
        let sprite_pipeline = SpritePipeline::new(&device, surface_format);

        Ok(Self {
            device,
            queue,
            surface,
            config,
            sprite_pipeline,
        })
    }

    /// Get a reference to the device.
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Get a reference to the queue.
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// Resize the renderer surface.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Render a skin to the window.
    pub fn render(&mut self, skin: Option<&Skin>, opacity: f32) -> Result<(), wgpu::SurfaceError> {
        self.render_with_extra(skin, opacity, [0.0, 0.0], |_| {})
    }

    /// Render a skin to the window with additional content.
    pub fn render_with_extra<F>(
        &mut self,
        skin: Option<&Skin>,
        opacity: f32,
        skin_offset: [f32; 2],
        extra: F,
    ) -> Result<(), wgpu::SurfaceError>
    where
        F: FnOnce(&mut wgpu::RenderPass<'_>),
    {
        let viewport_size = [self.config.width as f32, self.config.height as f32];

        // Prepare sprite pipeline if we have a skin
        if let Some(skin) = skin {
            self.sprite_pipeline.prepare(
                &self.device,
                &self.queue,
                skin,
                opacity,
                skin_offset,
                viewport_size,
            );
        }

        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if skin.is_some() {
                self.sprite_pipeline.render(&mut render_pass);
            }

            // Render additional content (buttons, widgets, etc.)
            extra(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Render a skin with button widgets.
    pub fn render_with_buttons(
        &mut self,
        skin: Option<&Skin>,
        opacity: f32,
        skin_offset: [f32; 2],
        button_renderer: Option<&ButtonRenderer>,
    ) -> Result<(), wgpu::SurfaceError> {
        let viewport_size = [self.config.width as f32, self.config.height as f32];

        // Prepare sprite pipeline if we have a skin
        if let Some(skin) = skin {
            self.sprite_pipeline.prepare(
                &self.device,
                &self.queue,
                skin,
                opacity,
                skin_offset,
                viewport_size,
            );
        }

        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if skin.is_some() {
                self.sprite_pipeline.render(&mut render_pass);
            }

            // Render buttons
            if let Some(btn_renderer) = button_renderer {
                btn_renderer.render(&mut render_pass);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Get the surface texture format.
    pub fn format(&self) -> TextureFormat {
        self.config.format
    }

    /// Render a skin with button widgets and app's custom rendering.
    pub fn render_with_buttons_and_app<'a, A: crate::GhostApp>(
        &'a mut self,
        skin: Option<&Skin>,
        opacity: f32,
        skin_offset: [f32; 2],
        button_renderer: Option<&'a ButtonRenderer>,
        app: &'a mut A,
    ) -> Result<(), wgpu::SurfaceError> {
        let viewport_size = [self.config.width as f32, self.config.height as f32];

        // Prepare sprite pipeline if we have a skin
        if let Some(skin) = skin {
            self.sprite_pipeline.prepare(
                &self.device,
                &self.queue,
                skin,
                opacity,
                skin_offset,
                viewport_size,
            );
        }

        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if skin.is_some() {
                self.sprite_pipeline.render(&mut render_pass);
            }

            // Render layers and text overlays (after skin, before buttons)
            app.render_layers(&self.device, &self.queue, viewport_size, &mut render_pass);

            // Render buttons
            if let Some(btn_renderer) = button_renderer {
                btn_renderer.render(&mut render_pass);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Render a transparent window with callout app content (no skin).
    pub fn render_callout<'a, C: crate::CalloutApp>(
        &'a mut self,
        app: &'a C,
    ) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Callout Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Callout Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            app.render(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

// Helper functions to convert raw-window-handle 0.5 types to 0.6 types
// This is needed because tao uses 0.5 and wgpu 0.19 uses 0.6

fn convert_window_handle(
    handle: raw_window_handle::RawWindowHandle,
) -> wgpu::rwh::RawWindowHandle {
    use raw_window_handle::RawWindowHandle as Rwh05;
    use wgpu::rwh::RawWindowHandle as Rwh06;

    match handle {
        #[cfg(target_os = "macos")]
        Rwh05::AppKit(h) => {
            let new_handle = wgpu::rwh::AppKitWindowHandle::new(
                std::ptr::NonNull::new(h.ns_view as *mut _).unwrap(),
            );
            Rwh06::AppKit(new_handle)
        }
        #[cfg(target_os = "windows")]
        Rwh05::Win32(h) => {
            use std::num::NonZeroIsize;
            let mut new_handle = wgpu::rwh::Win32WindowHandle::new(
                NonZeroIsize::new(h.hwnd as isize).unwrap(),
            );
            new_handle.hinstance = NonZeroIsize::new(h.hinstance as isize);
            Rwh06::Win32(new_handle)
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        Rwh05::Xlib(h) => {
            let mut new_handle = wgpu::rwh::XlibWindowHandle::new(h.window);
            new_handle.visual_id = h.visual_id;
            Rwh06::Xlib(new_handle)
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        Rwh05::Xcb(h) => {
            use std::num::NonZeroU32;
            let mut new_handle = wgpu::rwh::XcbWindowHandle::new(
                NonZeroU32::new(h.window).unwrap(),
            );
            new_handle.visual_id = NonZeroU32::new(h.visual_id);
            Rwh06::Xcb(new_handle)
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        Rwh05::Wayland(h) => {
            let new_handle = wgpu::rwh::WaylandWindowHandle::new(
                std::ptr::NonNull::new(h.surface as *mut _).unwrap(),
            );
            Rwh06::Wayland(new_handle)
        }
        _ => panic!("Unsupported window handle type"),
    }
}

fn convert_display_handle(
    handle: raw_window_handle::RawDisplayHandle,
) -> wgpu::rwh::RawDisplayHandle {
    use raw_window_handle::RawDisplayHandle as Rdh05;
    use wgpu::rwh::RawDisplayHandle as Rdh06;

    match handle {
        #[cfg(target_os = "macos")]
        Rdh05::AppKit(_) => Rdh06::AppKit(wgpu::rwh::AppKitDisplayHandle::new()),
        #[cfg(target_os = "windows")]
        Rdh05::Windows(_) => Rdh06::Windows(wgpu::rwh::WindowsDisplayHandle::new()),
        #[cfg(all(unix, not(target_os = "macos")))]
        Rdh05::Xlib(h) => {
            let new_handle = if h.display.is_null() {
                wgpu::rwh::XlibDisplayHandle::new(None, h.screen)
            } else {
                wgpu::rwh::XlibDisplayHandle::new(
                    Some(std::ptr::NonNull::new(h.display as *mut _).unwrap()),
                    h.screen,
                )
            };
            Rdh06::Xlib(new_handle)
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        Rdh05::Xcb(h) => {
            let new_handle = if h.connection.is_null() {
                wgpu::rwh::XcbDisplayHandle::new(None, h.screen)
            } else {
                wgpu::rwh::XcbDisplayHandle::new(
                    Some(std::ptr::NonNull::new(h.connection as *mut _).unwrap()),
                    h.screen,
                )
            };
            Rdh06::Xcb(new_handle)
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        Rdh05::Wayland(h) => {
            let new_handle = wgpu::rwh::WaylandDisplayHandle::new(
                std::ptr::NonNull::new(h.display as *mut _).unwrap(),
            );
            Rdh06::Wayland(new_handle)
        }
        _ => panic!("Unsupported display handle type"),
    }
}
