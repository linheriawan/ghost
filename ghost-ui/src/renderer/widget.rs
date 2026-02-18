//! Widget rendering pipeline
//!
//! Replaces ButtonRenderer with a unified renderer that handles:
//! 1. Solid pipeline - SDF rounded rectangles (Button bg, Label bg)
//! 2. Image pipeline - Textured quads with brightness modifier (ButtonImage)
//! 3. Text pipeline - glyphon text (Button labels, Label text, MarqueeLabel)

use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer as TextBuffer, Color, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer, Weight,
    Style as GlyphonStyle,
};
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, MultisampleState, Queue, RenderPass,
    RenderPipeline, Sampler, TextureFormat,
};

use crate::elements::button::Button;
use crate::elements::button_image::ButtonImage;
use crate::elements::label::{FontStyle, Label};
use crate::elements::marquee_label::MarqueeLabel;
use crate::elements::Widget;

// ─── Solid Pipeline Vertex ────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct SolidVertex {
    position: [f32; 2],
    color: [f32; 4],
    rect_min: [f32; 2],
    rect_max: [f32; 2],
    corner_radius: f32,
    _padding: f32,
}

impl SolidVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x2,   // position
        1 => Float32x4,   // color
        2 => Float32x2,   // rect_min
        3 => Float32x2,   // rect_max
        4 => Float32      // corner_radius
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SolidVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

// ─── Solid Pipeline Uniforms ──────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct SolidUniforms {
    viewport: [f32; 4], // width, height, 0, 0
}

// ─── Image Pipeline Vertex ────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ImageVertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

impl ImageVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

// Unit quad for image rendering (0,0) to (1,1)
const IMAGE_VERTICES: &[ImageVertex] = &[
    ImageVertex { position: [0.0, 0.0], tex_coords: [0.0, 0.0] },
    ImageVertex { position: [1.0, 0.0], tex_coords: [1.0, 0.0] },
    ImageVertex { position: [1.0, 1.0], tex_coords: [1.0, 1.0] },
    ImageVertex { position: [0.0, 1.0], tex_coords: [0.0, 1.0] },
];

const IMAGE_INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

// ─── Image Pipeline Uniforms ──────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ImageUniforms {
    viewport: [f32; 4],     // width, height, 0, 0
    rect: [f32; 4],         // x, y, width, height (pixels)
    brightness: f32,        // 1.0 normal, 1.15 hover, 0.9 pressed
    _padding: [f32; 3],
}

// ─── Image Draw Call ──────────────────────────────────────────────────

struct ImageDrawCall {
    bind_group: BindGroup,
}

// ─── WidgetRenderer ───────────────────────────────────────────────────

/// Unified renderer for all widget types.
///
/// Render order: solid backgrounds -> image buttons -> text
pub struct WidgetRenderer {
    // --- Solid pipeline (SDF rounded rects) ---
    solid_pipeline: RenderPipeline,
    solid_uniform_buffer: Buffer,
    solid_bind_group: BindGroup,
    solid_vertex_buffer: Option<Buffer>,
    solid_index_buffer: Option<Buffer>,
    solid_index_count: u32,

    // --- Image pipeline (textured quads with brightness) ---
    image_pipeline: RenderPipeline,
    image_bind_group_layout: BindGroupLayout,
    image_sampler: Sampler,
    image_vertex_buffer: Buffer,
    image_index_buffer: Buffer,
    image_draw_calls: Vec<ImageDrawCall>,

    // --- Text pipeline (glyphon) ---
    font_system: FontSystem,
    swash_cache: SwashCache,
    text_atlas: Option<TextAtlas>,
    text_renderer: Option<GlyphonTextRenderer>,
    text_buffers: Vec<TextBuffer>,
}

impl WidgetRenderer {
    /// Create a new widget renderer
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        // ── Solid pipeline ──────────────────────────────────────
        let solid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Widget Solid Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("widget.wgsl").into()),
        });

        let solid_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Widget Solid Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let solid_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Widget Solid Pipeline Layout"),
                bind_group_layouts: &[&solid_bind_group_layout],
                push_constant_ranges: &[],
            });

        let solid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Widget Solid Pipeline"),
            layout: Some(&solid_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &solid_shader,
                entry_point: "vs_main",
                buffers: &[SolidVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &solid_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let solid_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Widget Solid Uniform Buffer"),
            contents: bytemuck::cast_slice(&[SolidUniforms {
                viewport: [800.0, 600.0, 0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let solid_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Widget Solid Bind Group"),
            layout: &solid_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: solid_uniform_buffer.as_entire_binding(),
            }],
        });

        // ── Image pipeline ──────────────────────────────────────
        let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Widget Image Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("widget_image.wgsl").into()),
        });

        let image_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Widget Image Bind Group Layout"),
                entries: &[
                    // Uniforms
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Texture
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Sampler
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let image_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Widget Image Pipeline Layout"),
                bind_group_layouts: &[&image_bind_group_layout],
                push_constant_ranges: &[],
            });

        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Widget Image Pipeline"),
            layout: Some(&image_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &image_shader,
                entry_point: "vs_main",
                buffers: &[ImageVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &image_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let image_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Widget Image Vertex Buffer"),
            contents: bytemuck::cast_slice(IMAGE_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let image_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Widget Image Index Buffer"),
            contents: bytemuck::cast_slice(IMAGE_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        // ── Text pipeline (glyphon) ─────────────────────────────
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        let mut text_atlas = TextAtlas::new(device, queue, format);
        let text_renderer = GlyphonTextRenderer::new(
            &mut text_atlas,
            device,
            MultisampleState::default(),
            None,
        );

        Self {
            solid_pipeline,
            solid_uniform_buffer,
            solid_bind_group,
            solid_vertex_buffer: None,
            solid_index_buffer: None,
            solid_index_count: 0,

            image_pipeline,
            image_bind_group_layout,
            image_sampler,
            image_vertex_buffer,
            image_index_buffer,
            image_draw_calls: Vec::new(),

            font_system,
            swash_cache,
            text_atlas: Some(text_atlas),
            text_renderer: Some(text_renderer),
            text_buffers: Vec::new(),
        }
    }

    /// Prepare all widgets for rendering.
    ///
    /// Returns a Vec of (index, text_width) pairs for marquee labels that need
    /// their text_width updated. The caller should apply these via `set_text_width()`.
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        buttons: &[&Button],
        button_images: &[&ButtonImage],
        labels: &[&Label],
        marquees: &[&MarqueeLabel],
        viewport: [f32; 2],
        scale_factor: f32,
    ) -> Vec<(usize, f32)> {
        // Update solid uniforms
        queue.write_buffer(
            &self.solid_uniform_buffer,
            0,
            bytemuck::cast_slice(&[SolidUniforms {
                viewport: [viewport[0], viewport[1], 0.0, 0.0],
            }]),
        );

        // ── 1. Generate solid rounded-rect vertices ─────────────
        self.prepare_solids(device, buttons, labels, marquees.iter().map(|m| m.label()), viewport);

        // ── 2. Generate image draw calls ────────────────────────
        self.prepare_images(device, queue, button_images, viewport);

        // ── 3. Prepare text for all text-bearing widgets ────────
        self.prepare_text(device, queue, buttons, labels, marquees, viewport, scale_factor)
    }

    /// Generate SDF rounded-rect vertices for Button and Label backgrounds
    fn prepare_solids<'a>(
        &mut self,
        device: &Device,
        buttons: &[&Button],
        labels: &[&Label],
        marquee_labels: impl Iterator<Item = &'a Label>,
        viewport: [f32; 2],
    ) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        // Buttons
        for button in buttons {
            if !button.is_visible() {
                continue;
            }
            let bounds = button.screen_bounds(viewport[1]);
            let color = button.current_background();
            let radius = button.style().border_radius;
            Self::push_rect_vertices(&mut vertices, &mut indices, bounds, color, radius);
        }

        // Labels (only those with visible backgrounds)
        for label in labels {
            if !label.is_visible() || !label.has_background() {
                continue;
            }
            let bounds = label.screen_bounds(viewport[1]);
            let color = label.style().background;
            let radius = label.style().border_radius;
            Self::push_rect_vertices(&mut vertices, &mut indices, bounds, color, radius);
        }

        // MarqueeLabel backgrounds
        for label in marquee_labels {
            if !label.is_visible() || !label.has_background() {
                continue;
            }
            let bounds = label.screen_bounds(viewport[1]);
            let color = label.style().background;
            let radius = label.style().border_radius;
            Self::push_rect_vertices(&mut vertices, &mut indices, bounds, color, radius);
        }

        if vertices.is_empty() {
            self.solid_index_count = 0;
            self.solid_vertex_buffer = None;
            self.solid_index_buffer = None;
            return;
        }

        self.solid_vertex_buffer = Some(device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Widget Solid Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            },
        ));

        self.solid_index_buffer = Some(device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Widget Solid Index Buffer"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            },
        ));

        self.solid_index_count = indices.len() as u32;
    }

    /// Helper: push 4 vertices + 6 indices for one rounded rect
    fn push_rect_vertices(
        vertices: &mut Vec<SolidVertex>,
        indices: &mut Vec<u16>,
        bounds: [f32; 4], // [x, y, w, h]
        color: [f32; 4],
        corner_radius: f32,
    ) {
        let x = bounds[0];
        let y = bounds[1];
        let w = bounds[2];
        let h = bounds[3];
        let rect_min = [x, y];
        let rect_max = [x + w, y + h];

        let base_idx = vertices.len() as u16;

        let make_vert = |px: f32, py: f32| SolidVertex {
            position: [px, py],
            color,
            rect_min,
            rect_max,
            corner_radius,
            _padding: 0.0,
        };

        vertices.push(make_vert(x, y));
        vertices.push(make_vert(x + w, y));
        vertices.push(make_vert(x + w, y + h));
        vertices.push(make_vert(x, y + h));

        indices.push(base_idx);
        indices.push(base_idx + 1);
        indices.push(base_idx + 2);
        indices.push(base_idx);
        indices.push(base_idx + 2);
        indices.push(base_idx + 3);
    }

    /// Generate image draw calls for ButtonImages
    fn prepare_images(
        &mut self,
        device: &Device,
        _queue: &Queue,
        button_images: &[&ButtonImage],
        viewport: [f32; 2],
    ) {
        self.image_draw_calls.clear();

        for btn_img in button_images {
            if !btn_img.is_visible() {
                continue;
            }
            let Some(skin) = btn_img.skin() else {
                continue;
            };

            let bounds = btn_img.screen_bounds(viewport[1]);

            let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Widget Image Uniform"),
                contents: bytemuck::cast_slice(&[ImageUniforms {
                    viewport: [viewport[0], viewport[1], 0.0, 0.0],
                    rect: bounds,
                    brightness: btn_img.brightness(),
                    _padding: [0.0; 3],
                }]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Widget Image Bind Group"),
                layout: &self.image_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(skin.texture_view()),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                    },
                ],
            });

            self.image_draw_calls.push(ImageDrawCall { bind_group });
        }
    }

    /// Prepare text for all text-bearing widgets using glyphon.
    ///
    /// Returns a Vec of (index, text_width) pairs for marquee labels.
    fn prepare_text(
        &mut self,
        device: &Device,
        queue: &Queue,
        buttons: &[&Button],
        labels: &[&Label],
        marquees: &[&MarqueeLabel],
        viewport: [f32; 2],
        _scale_factor: f32,
    ) -> Vec<(usize, f32)> {
        let Some(atlas) = &mut self.text_atlas else { return vec![] };
        let Some(renderer) = &mut self.text_renderer else { return vec![] };

        // Collect all text areas
        self.text_buffers.clear();
        let mut text_entries: Vec<TextEntry> = Vec::new();

        // Button labels
        for button in buttons {
            if !button.is_visible() || button.label().is_empty() {
                continue;
            }
            let bounds = button.screen_bounds(viewport[1]);
            let style = button.style();

            let mut buffer = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(style.font_size, style.font_size * 1.2),
            );
            buffer.set_size(
                &mut self.font_system,
                bounds[2].max(1.0),
                bounds[3].max(style.font_size * 1.2),
            );
            let attrs = Attrs::new().family(Family::SansSerif);
            buffer.set_text(&mut self.font_system, button.label(), attrs, Shaping::Advanced);
            buffer.shape_until_scroll(&mut self.font_system);

            // Center text within button bounds
            let text_width: f32 = buffer
                .layout_runs()
                .map(|run| run.line_w)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0);
            let text_height = style.font_size * 1.2;

            let left = bounds[0] + (bounds[2] - text_width) / 2.0;
            let top = bounds[1] + (bounds[3] - text_height) / 2.0;

            text_entries.push(TextEntry {
                left,
                top,
                color: style.text_color,
                bounds_left: bounds[0] as i32,
                bounds_top: bounds[1] as i32,
                bounds_right: (bounds[0] + bounds[2]) as i32,
                bounds_bottom: (bounds[1] + bounds[3]) as i32,
            });
            self.text_buffers.push(buffer);
        }

        // Label text
        for label in labels {
            if !label.is_visible() || label.text().is_empty() {
                continue;
            }
            let bounds = label.screen_bounds(viewport[1]);
            let style = label.style();
            let padding = style.padding;

            let font_attrs = match style.font_style {
                FontStyle::Bold => Attrs::new().family(Family::SansSerif).weight(Weight::BOLD),
                FontStyle::Italic => Attrs::new().family(Family::SansSerif).style(GlyphonStyle::Italic),
                FontStyle::Normal => Attrs::new().family(Family::SansSerif),
            };

            let mut buffer = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(style.font_size, style.font_size * 1.2),
            );
            let inner_w = (bounds[2] - padding[0] * 2.0).max(1.0);
            let inner_h = (bounds[3] - padding[1] * 2.0).max(style.font_size * 1.2);
            buffer.set_size(&mut self.font_system, inner_w, inner_h);
            buffer.set_text(&mut self.font_system, label.text(), font_attrs, Shaping::Advanced);
            buffer.shape_until_scroll(&mut self.font_system);

            // Position text with padding, vertically centered
            let text_height = style.font_size * 1.2;
            let left = bounds[0] + padding[0];
            let top = bounds[1] + (bounds[3] - text_height) / 2.0;

            text_entries.push(TextEntry {
                left,
                top,
                color: style.text_color,
                bounds_left: bounds[0] as i32,
                bounds_top: bounds[1] as i32,
                bounds_right: (bounds[0] + bounds[2]) as i32,
                bounds_bottom: (bounds[1] + bounds[3]) as i32,
            });
            self.text_buffers.push(buffer);
        }

        // MarqueeLabel text
        let mut marquee_text_widths: Vec<(usize, f32)> = Vec::new();
        for (idx, marquee) in marquees.iter().enumerate() {
            let label = marquee.label();
            if !label.is_visible() || label.text().is_empty() {
                continue;
            }
            let bounds = label.screen_bounds(viewport[1]);
            let style = label.style();
            let padding = style.padding;

            let font_attrs = match style.font_style {
                FontStyle::Bold => Attrs::new().family(Family::SansSerif).weight(Weight::BOLD),
                FontStyle::Italic => Attrs::new().family(Family::SansSerif).style(GlyphonStyle::Italic),
                FontStyle::Normal => Attrs::new().family(Family::SansSerif),
            };

            // For marquee, use a wider buffer so text can overflow
            let mut buffer = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(style.font_size, style.font_size * 1.2),
            );
            // Set a very wide size so we can measure the full text width
            buffer.set_size(&mut self.font_system, f32::MAX, style.font_size * 1.2);
            buffer.set_text(&mut self.font_system, label.text(), font_attrs, Shaping::Advanced);
            buffer.shape_until_scroll(&mut self.font_system);

            // Measure text width
            let text_width: f32 = buffer
                .layout_runs()
                .map(|run| run.line_w)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0);

            // Collect text width for caller to apply
            marquee_text_widths.push((idx, text_width));

            // Position text with scroll offset
            let text_height = style.font_size * 1.2;
            let left = bounds[0] + padding[0] - marquee.scroll_offset();
            let top = bounds[1] + (bounds[3] - text_height) / 2.0;

            // Clip to label bounds
            text_entries.push(TextEntry {
                left,
                top,
                color: style.text_color,
                bounds_left: bounds[0] as i32,
                bounds_top: bounds[1] as i32,
                bounds_right: (bounds[0] + bounds[2]) as i32,
                bounds_bottom: (bounds[1] + bounds[3]) as i32,
            });
            self.text_buffers.push(buffer);
        }

        if text_entries.is_empty() {
            // Prepare with empty to clear
            let resolution = Resolution {
                width: viewport[0] as u32,
                height: viewport[1] as u32,
            };
            let _ = renderer.prepare(
                device,
                queue,
                &mut self.font_system,
                atlas,
                resolution,
                std::iter::empty::<TextArea>(),
                &mut self.swash_cache,
            );
            return marquee_text_widths;
        }

        // Build text areas from entries + buffers
        let text_areas: Vec<TextArea> = text_entries
            .iter()
            .zip(self.text_buffers.iter())
            .map(|(entry, buffer)| {
                let [r, g, b, a] = entry.color;
                TextArea {
                    buffer,
                    left: entry.left,
                    top: entry.top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: entry.bounds_left,
                        top: entry.bounds_top,
                        right: entry.bounds_right,
                        bottom: entry.bounds_bottom,
                    },
                    default_color: Color::rgba(
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (b * 255.0) as u8,
                        (a * 255.0) as u8,
                    ),
                }
            })
            .collect();

        let resolution = Resolution {
            width: viewport[0] as u32,
            height: viewport[1] as u32,
        };

        if let Err(e) = renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            atlas,
            resolution,
            text_areas,
            &mut self.swash_cache,
        ) {
            log::error!("Failed to prepare widget text: {:?}", e);
        }

        marquee_text_widths
    }

    /// Render all prepared widgets
    ///
    /// Order: solid backgrounds -> image buttons -> text
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        // 1. Render solid backgrounds (SDF rounded rects)
        if self.solid_index_count > 0 {
            if let (Some(vb), Some(ib)) = (&self.solid_vertex_buffer, &self.solid_index_buffer) {
                render_pass.set_pipeline(&self.solid_pipeline);
                render_pass.set_bind_group(0, &self.solid_bind_group, &[]);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
                render_pass.draw_indexed(0..self.solid_index_count, 0, 0..1);
            }
        }

        // 2. Render image buttons
        for draw_call in &self.image_draw_calls {
            render_pass.set_pipeline(&self.image_pipeline);
            render_pass.set_bind_group(0, &draw_call.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.image_vertex_buffer.slice(..));
            render_pass.set_index_buffer(
                self.image_index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            render_pass.draw_indexed(0..6, 0, 0..1);
        }

        // 3. Render text
        if let (Some(renderer), Some(atlas)) = (&self.text_renderer, &self.text_atlas) {
            if let Err(e) = renderer.render(atlas, render_pass) {
                log::error!("Failed to render widget text: {:?}", e);
            }
        }
    }

    /// Trim the text atlas to free unused memory
    pub fn trim(&mut self) {
        if let Some(atlas) = &mut self.text_atlas {
            atlas.trim();
        }
    }
}

/// Internal helper for collecting text rendering info
struct TextEntry {
    left: f32,
    top: f32,
    color: [f32; 4],
    bounds_left: i32,
    bounds_top: i32,
    bounds_right: i32,
    bounds_bottom: i32,
}
