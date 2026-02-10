//! Sprite rendering pipeline for PNG textures

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, Queue, RenderPass, RenderPipeline, Sampler,
    TextureFormat,
};

use crate::Skin;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

// Full-screen quad vertices
const VERTICES: &[Vertex] = &[
    Vertex {
        position: [-1.0, -1.0],
        tex_coords: [0.0, 1.0],
    },
    Vertex {
        position: [1.0, -1.0],
        tex_coords: [1.0, 1.0],
    },
    Vertex {
        position: [1.0, 1.0],
        tex_coords: [1.0, 0.0],
    },
    Vertex {
        position: [-1.0, 1.0],
        tex_coords: [0.0, 0.0],
    },
];

const INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Uniforms {
    opacity: f32,
    _padding: f32,
    /// Skin offset in NDC (normalized device coordinates)
    offset: [f32; 2],
    /// Skin size as fraction of viewport (0.0 to 1.0)
    size: [f32; 2],
    _padding2: [f32; 2],
}

pub struct SpritePipeline {
    pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    uniform_buffer: Buffer,
    bind_group_layout: BindGroupLayout,
    sampler: Sampler,
    current_bind_group: Option<BindGroup>,
}

impl SpritePipeline {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        // Create shader module
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sprite Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Sprite Bind Group Layout"),
            entries: &[
                // Texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
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
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Uniforms (used in both vertex and fragment shaders)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sprite Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sprite Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
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
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });

        // Create vertex buffer
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create index buffer
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Create uniform buffer
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[Uniforms {
                opacity: 1.0,
                _padding: 0.0,
                offset: [0.0, 0.0],
                size: [1.0, 1.0],
                _padding2: [0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            bind_group_layout,
            sampler,
            current_bind_group: None,
        }
    }

    /// Prepare the pipeline for rendering with a specific skin.
    ///
    /// * `skin_offset` - Offset of skin within viewport [x, y] in pixels
    /// * `viewport_size` - Size of the viewport [width, height] in pixels
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        skin: &Skin,
        opacity: f32,
        skin_offset: [f32; 2],
        _viewport_size: [f32; 2],
    ) {
        // When skin_offset is [0,0], render full-screen (skin fills viewport)
        // This handles DPI scaling correctly since the window is sized to the skin
        let (size_x, size_y, offset_x, offset_y) = if skin_offset[0] == 0.0 && skin_offset[1] == 0.0 {
            // Full-screen: size=1.0, offset=0.0 (same as original behavior)
            (1.0, 1.0, 0.0, 0.0)
        } else {
            // Offset rendering: calculate NDC coordinates
            // Note: This path needs the viewport to be in the same coordinate space as skin_offset
            let size_x = skin.width() as f32 / _viewport_size[0];
            let size_y = skin.height() as f32 / _viewport_size[1];
            let offset_x = (skin_offset[0] / _viewport_size[0]) * 2.0 - 1.0 + size_x;
            let offset_y = 1.0 - (skin_offset[1] / _viewport_size[1]) * 2.0 - size_y;
            (size_x, size_y, offset_x, offset_y)
        };

        // Update uniforms
        let uniforms = Uniforms {
            opacity,
            _padding: 0.0,
            offset: [offset_x, offset_y],
            size: [size_x, size_y],
            _padding2: [0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        // Create bind group for this skin
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Skin Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(skin.texture_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
            ],
        });

        self.current_bind_group = Some(bind_group);
    }

    /// Render the prepared skin.
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        if let Some(bind_group) = &self.current_bind_group {
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..6, 0, 0..1);
        }
    }

    /// Prepare to render a sprite at a specific pixel position within the viewport.
    ///
    /// * `position` - Position [x, y] in pixels from top-left of viewport
    /// * `viewport_size` - Size of the viewport [width, height] in pixels
    pub fn prepare_at_position(
        &mut self,
        device: &Device,
        queue: &Queue,
        skin: &Skin,
        opacity: f32,
        position: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        self.current_bind_group = Some(self.create_bind_group_at_position(
            device, queue, skin, opacity, position, viewport_size, 1.0,
        ));
    }

    /// Create a bind group for rendering a sprite at a specific position.
    /// This is useful for layers that need their own bind groups.
    pub fn create_bind_group_at_position(
        &self,
        device: &Device,
        _queue: &Queue,
        skin: &Skin,
        opacity: f32,
        position: [f32; 2],
        viewport_size: [f32; 2],
        scale_factor: f32,
    ) -> BindGroup {
        // Scale the skin dimensions for physical pixels
        let skin_width = skin.width() as f32 * scale_factor;
        let skin_height = skin.height() as f32 * scale_factor;

        // Calculate sprite size as fraction of viewport
        let size_x = skin_width / viewport_size[0];
        let size_y = skin_height / viewport_size[1];

        // Convert pixel position to NDC
        // NDC goes from -1 (left/bottom) to +1 (right/top)
        // Pixel coordinates go from 0 (top-left) with Y increasing downward

        // Calculate the center of the sprite in NDC
        let center_x = (position[0] + skin_width / 2.0) / viewport_size[0] * 2.0 - 1.0;
        let center_y = 1.0 - (position[1] + skin_height / 2.0) / viewport_size[1] * 2.0;

        // Create a uniform buffer for this specific layer
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Layer Uniform Buffer"),
            contents: bytemuck::cast_slice(&[Uniforms {
                opacity,
                _padding: 0.0,
                offset: [center_x, center_y],
                size: [size_x, size_y],
                _padding2: [0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create bind group for this skin
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Layer Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(skin.texture_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        })
    }

    /// Render a bind group (for layers)
    pub fn render_bind_group<'a>(&'a self, render_pass: &mut RenderPass<'a>, bind_group: &'a BindGroup) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..6, 0, 0..1);
    }
}
