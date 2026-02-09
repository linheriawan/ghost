//! Button rendering pipeline

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, Queue, RenderPass, RenderPipeline, TextureFormat,
};

use crate::widget::{Button, Widget};

/// Vertex for button rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ButtonVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl ButtonVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ButtonVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// Uniforms for button rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ButtonUniforms {
    viewport: [f32; 4], // width, height, 0, 0
}

/// Renderer for button widgets
pub struct ButtonRenderer {
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
    uniform_buffer: Buffer,
    bind_group: BindGroup,
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    index_count: u32,
}

impl ButtonRenderer {
    /// Create a new button renderer
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Button Shader"),
            source: wgpu::ShaderSource::Wgsl(BUTTON_SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Button Bind Group Layout"),
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Button Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Button Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[ButtonVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Button Uniform Buffer"),
            contents: bytemuck::cast_slice(&[ButtonUniforms {
                viewport: [800.0, 600.0, 0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Button Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            bind_group,
            vertex_buffer: None,
            index_buffer: None,
            index_count: 0,
        }
    }

    /// Prepare buttons for rendering
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        buttons: &[&Button],
        viewport: [f32; 2],
    ) {
        // Update uniforms
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[ButtonUniforms {
                viewport: [viewport[0], viewport[1], 0.0, 0.0],
            }]),
        );

        // Generate vertices for all visible buttons
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for button in buttons {
            if !button.is_visible() {
                continue;
            }

            let bounds = button.screen_bounds(viewport[1]);
            let color = button.current_background();
            let base_idx = vertices.len() as u16;

            // Simple rectangle (no rounded corners for now)
            let x = bounds[0];
            let y = bounds[1];
            let w = bounds[2];
            let h = bounds[3];

            vertices.push(ButtonVertex {
                position: [x, y],
                color,
            });
            vertices.push(ButtonVertex {
                position: [x + w, y],
                color,
            });
            vertices.push(ButtonVertex {
                position: [x + w, y + h],
                color,
            });
            vertices.push(ButtonVertex {
                position: [x, y + h],
                color,
            });

            indices.push(base_idx);
            indices.push(base_idx + 1);
            indices.push(base_idx + 2);
            indices.push(base_idx);
            indices.push(base_idx + 2);
            indices.push(base_idx + 3);
        }

        if vertices.is_empty() {
            self.index_count = 0;
            return;
        }

        self.vertex_buffer = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Button Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }));

        self.index_buffer = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Button Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        }));

        self.index_count = indices.len() as u32;
    }

    /// Render the prepared buttons
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }

        if let (Some(vertex_buffer), Some(index_buffer)) =
            (&self.vertex_buffer, &self.index_buffer)
        {
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..self.index_count, 0, 0..1);
        }
    }
}

const BUTTON_SHADER: &str = r#"
struct Uniforms {
    viewport: vec4<f32>, // width, height, 0, 0
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Convert to clip space (-1 to 1)
    let clip_x = (in.position.x / uniforms.viewport.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (in.position.y / uniforms.viewport.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = in.color;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;
