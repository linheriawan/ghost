//! Shape rendering for callout bubbles

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, Queue, RenderPass, RenderPipeline, TextureFormat,
};

use crate::types::{ArrowPosition, CalloutStyle, CalloutType};

/// Vertex format for shape rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ShapeVertex {
    /// Position [x, y] in pixels
    pub position: [f32; 2],
    /// Color [r, g, b, a]
    pub color: [f32; 4],
}

impl ShapeVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ShapeVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// Uniforms for shape rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ShapeUniforms {
    /// Transform: [scale_x, scale_y, offset_x, offset_y]
    transform: [f32; 4],
    /// Viewport size [width, height, 0, 0]
    viewport: [f32; 4],
}

/// Represents a callout shape with its geometry
pub struct CalloutShape {
    /// Vertices for the shape
    vertices: Vec<ShapeVertex>,
    /// Indices for triangle rendering
    indices: Vec<u16>,
    /// Bounding box [x, y, width, height]
    bounds: [f32; 4],
}

impl CalloutShape {
    /// Create a new callout shape
    pub fn new(
        callout_type: CalloutType,
        width: f32,
        height: f32,
        arrow: ArrowPosition,
        style: &CalloutStyle,
    ) -> Self {
        match callout_type {
            CalloutType::Talk => Self::create_talk_shape(width, height, arrow, style),
            CalloutType::Think => Self::create_think_shape(width, height, arrow, style),
            CalloutType::Scream => Self::create_scream_shape(width, height, arrow, style),
        }
    }

    /// Create a talk bubble (rounded rectangle with tail)
    fn create_talk_shape(
        width: f32,
        height: f32,
        arrow: ArrowPosition,
        style: &CalloutStyle,
    ) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let radius = style.border_radius.min(width / 4.0).min(height / 4.0);
        let color = style.background;

        // Arrow dimensions
        let arrow_width = 20.0;
        let arrow_height = 15.0;

        // Calculate bounds including arrow
        let (bounds_x, bounds_y, bounds_w, bounds_h) = match arrow {
            ArrowPosition::Bottom(_) => (0.0, 0.0, width, height + arrow_height),
            ArrowPosition::Top(_) => (0.0, -arrow_height, width, height + arrow_height),
            ArrowPosition::Left(_) => (-arrow_height, 0.0, width + arrow_height, height),
            ArrowPosition::Right(_) => (0.0, 0.0, width + arrow_height, height),
            ArrowPosition::None => (0.0, 0.0, width, height),
        };

        // Generate rounded rectangle vertices
        let segments_per_corner = 8;
        let center_idx = vertices.len() as u16;

        // Center vertex for fan triangulation
        vertices.push(ShapeVertex {
            position: [width / 2.0, height / 2.0],
            color,
        });

        // Generate corner arcs
        let corners = [
            (radius, radius, std::f32::consts::PI, std::f32::consts::FRAC_PI_2 * 3.0), // top-left
            (width - radius, radius, std::f32::consts::FRAC_PI_2 * 3.0, std::f32::consts::TAU), // top-right
            (width - radius, height - radius, 0.0, std::f32::consts::FRAC_PI_2), // bottom-right
            (radius, height - radius, std::f32::consts::FRAC_PI_2, std::f32::consts::PI), // bottom-left
        ];

        let first_vertex_idx = vertices.len() as u16;

        for (cx, cy, start_angle, end_angle) in corners {
            for i in 0..=segments_per_corner {
                let t = i as f32 / segments_per_corner as f32;
                let angle = start_angle + t * (end_angle - start_angle);
                let x = cx + radius * angle.cos();
                let y = cy + radius * angle.sin();
                vertices.push(ShapeVertex {
                    position: [x, y],
                    color,
                });
            }
        }

        // Generate triangles (fan from center)
        let num_edge_vertices = vertices.len() as u16 - first_vertex_idx;
        for i in 0..num_edge_vertices {
            let next = (i + 1) % num_edge_vertices;
            indices.push(center_idx);
            indices.push(first_vertex_idx + i);
            indices.push(first_vertex_idx + next);
        }

        // Add arrow vertices and triangles
        if arrow.position().is_some() {
            let arrow_vertices_start = vertices.len() as u16;

            match arrow {
                ArrowPosition::Bottom(p) => {
                    let arrow_x = width * p;
                    vertices.push(ShapeVertex {
                        position: [arrow_x - arrow_width / 2.0, height],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [arrow_x, height + arrow_height],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [arrow_x + arrow_width / 2.0, height],
                        color,
                    });
                }
                ArrowPosition::Top(p) => {
                    let arrow_x = width * p;
                    vertices.push(ShapeVertex {
                        position: [arrow_x - arrow_width / 2.0, 0.0],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [arrow_x, -arrow_height],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [arrow_x + arrow_width / 2.0, 0.0],
                        color,
                    });
                }
                ArrowPosition::Left(p) => {
                    let arrow_y = height * p;
                    vertices.push(ShapeVertex {
                        position: [0.0, arrow_y - arrow_width / 2.0],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [-arrow_height, arrow_y],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [0.0, arrow_y + arrow_width / 2.0],
                        color,
                    });
                }
                ArrowPosition::Right(p) => {
                    let arrow_y = height * p;
                    vertices.push(ShapeVertex {
                        position: [width, arrow_y - arrow_width / 2.0],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [width + arrow_height, arrow_y],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [width, arrow_y + arrow_width / 2.0],
                        color,
                    });
                }
                ArrowPosition::None => {}
            }

            // Arrow triangle
            if arrow != ArrowPosition::None {
                indices.push(arrow_vertices_start);
                indices.push(arrow_vertices_start + 1);
                indices.push(arrow_vertices_start + 2);
            }
        }

        Self {
            vertices,
            indices,
            bounds: [bounds_x, bounds_y, bounds_w, bounds_h],
        }
    }

    /// Create a think bubble (cloud shape with bubble trail)
    fn create_think_shape(
        width: f32,
        height: f32,
        arrow: ArrowPosition,
        style: &CalloutStyle,
    ) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let color = style.background;

        // Create cloud-like border using overlapping circles
        let num_bumps = ((width + height) / 30.0) as usize;
        let bump_radius = 15.0;

        // Center for triangulation
        let center_idx = vertices.len() as u16;
        vertices.push(ShapeVertex {
            position: [width / 2.0, height / 2.0],
            color,
        });

        // Generate bumpy outline
        let first_vertex_idx = vertices.len() as u16;
        let total_perimeter = 2.0 * (width + height);
        let points_count = num_bumps * 8;

        for i in 0..points_count {
            let t = i as f32 / points_count as f32;
            let perimeter_pos = t * total_perimeter;

            // Calculate base position along rectangle
            let (base_x, base_y, normal_x, normal_y) = if perimeter_pos < width {
                // Top edge
                (perimeter_pos, 0.0, 0.0, -1.0)
            } else if perimeter_pos < width + height {
                // Right edge
                (width, perimeter_pos - width, 1.0, 0.0)
            } else if perimeter_pos < 2.0 * width + height {
                // Bottom edge
                (2.0 * width + height - perimeter_pos, height, 0.0, 1.0)
            } else {
                // Left edge
                (0.0, total_perimeter - perimeter_pos, -1.0, 0.0)
            };

            // Add cloud bump
            let bump_phase = (t * num_bumps as f32 * std::f32::consts::TAU).sin();
            let bump_amount = bump_radius * 0.3 * (bump_phase * 0.5 + 0.5);

            let x = base_x + normal_x * bump_amount;
            let y = base_y + normal_y * bump_amount;

            vertices.push(ShapeVertex {
                position: [x, y],
                color,
            });
        }

        // Triangulate
        let num_edge_vertices = vertices.len() as u16 - first_vertex_idx;
        for i in 0..num_edge_vertices {
            let next = (i + 1) % num_edge_vertices;
            indices.push(center_idx);
            indices.push(first_vertex_idx + i);
            indices.push(first_vertex_idx + next);
        }

        // Add thought bubbles trail
        if arrow.position().is_some() {
            let bubble_sizes = [8.0, 5.0, 3.0];
            let bubble_spacing = 12.0;

            for (i, &size) in bubble_sizes.iter().enumerate() {
                let offset = (i as f32 + 1.0) * bubble_spacing;

                let (bx, by) = match arrow {
                    ArrowPosition::Bottom(p) => (width * p, height + offset),
                    ArrowPosition::Top(p) => (width * p, -offset),
                    ArrowPosition::Left(p) => (-offset, height * p),
                    ArrowPosition::Right(p) => (width + offset, height * p),
                    ArrowPosition::None => continue,
                };

                // Add small circle for thought bubble
                let bubble_center_idx = vertices.len() as u16;
                vertices.push(ShapeVertex {
                    position: [bx, by],
                    color,
                });

                let segments = 12;
                let bubble_first = vertices.len() as u16;
                for j in 0..segments {
                    let angle = j as f32 / segments as f32 * std::f32::consts::TAU;
                    vertices.push(ShapeVertex {
                        position: [bx + size * angle.cos(), by + size * angle.sin()],
                        color,
                    });
                }

                for j in 0..segments {
                    let next = (j + 1) % segments;
                    indices.push(bubble_center_idx);
                    indices.push(bubble_first + j as u16);
                    indices.push(bubble_first + next as u16);
                }
            }
        }

        let bounds = match arrow {
            ArrowPosition::Bottom(_) => [0.0, 0.0, width, height + 40.0],
            ArrowPosition::Top(_) => [0.0, -40.0, width, height + 40.0],
            ArrowPosition::Left(_) => [-40.0, 0.0, width + 40.0, height],
            ArrowPosition::Right(_) => [0.0, 0.0, width + 40.0, height],
            ArrowPosition::None => [0.0, 0.0, width, height],
        };

        Self {
            vertices,
            indices,
            bounds,
        }
    }

    /// Create a scream bubble (spiky/jagged edges)
    fn create_scream_shape(
        width: f32,
        height: f32,
        arrow: ArrowPosition,
        style: &CalloutStyle,
    ) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let color = style.background;
        let spike_depth = 10.0;
        let spike_count = ((width + height) / 25.0) as usize;

        // Center for triangulation
        let center_idx = vertices.len() as u16;
        vertices.push(ShapeVertex {
            position: [width / 2.0, height / 2.0],
            color,
        });

        // Generate spiky outline
        let first_vertex_idx = vertices.len() as u16;
        let total_perimeter = 2.0 * (width + height);
        let points_count = spike_count * 2;

        for i in 0..points_count {
            let t = i as f32 / points_count as f32;
            let perimeter_pos = t * total_perimeter;

            // Calculate base position along rectangle
            let (base_x, base_y, normal_x, normal_y) = if perimeter_pos < width {
                // Top edge
                (perimeter_pos, 0.0, 0.0, -1.0)
            } else if perimeter_pos < width + height {
                // Right edge
                (width, perimeter_pos - width, 1.0, 0.0)
            } else if perimeter_pos < 2.0 * width + height {
                // Bottom edge
                (2.0 * width + height - perimeter_pos, height, 0.0, 1.0)
            } else {
                // Left edge
                (0.0, total_perimeter - perimeter_pos, -1.0, 0.0)
            };

            // Alternate spike direction
            let spike = if i % 2 == 0 { spike_depth } else { 0.0 };

            let x = base_x + normal_x * spike;
            let y = base_y + normal_y * spike;

            vertices.push(ShapeVertex {
                position: [x, y],
                color,
            });
        }

        // Triangulate
        let num_edge_vertices = vertices.len() as u16 - first_vertex_idx;
        for i in 0..num_edge_vertices {
            let next = (i + 1) % num_edge_vertices;
            indices.push(center_idx);
            indices.push(first_vertex_idx + i);
            indices.push(first_vertex_idx + next);
        }

        // Add arrow (larger spike for scream)
        if arrow.position().is_some() {
            let arrow_vertices_start = vertices.len() as u16;
            let arrow_width = 30.0;
            let arrow_height = 25.0;

            match arrow {
                ArrowPosition::Bottom(p) => {
                    let arrow_x = width * p;
                    vertices.push(ShapeVertex {
                        position: [arrow_x - arrow_width / 2.0, height + spike_depth],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [arrow_x, height + spike_depth + arrow_height],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [arrow_x + arrow_width / 2.0, height + spike_depth],
                        color,
                    });
                }
                ArrowPosition::Top(p) => {
                    let arrow_x = width * p;
                    vertices.push(ShapeVertex {
                        position: [arrow_x - arrow_width / 2.0, -spike_depth],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [arrow_x, -spike_depth - arrow_height],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [arrow_x + arrow_width / 2.0, -spike_depth],
                        color,
                    });
                }
                ArrowPosition::Left(p) => {
                    let arrow_y = height * p;
                    vertices.push(ShapeVertex {
                        position: [-spike_depth, arrow_y - arrow_width / 2.0],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [-spike_depth - arrow_height, arrow_y],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [-spike_depth, arrow_y + arrow_width / 2.0],
                        color,
                    });
                }
                ArrowPosition::Right(p) => {
                    let arrow_y = height * p;
                    vertices.push(ShapeVertex {
                        position: [width + spike_depth, arrow_y - arrow_width / 2.0],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [width + spike_depth + arrow_height, arrow_y],
                        color,
                    });
                    vertices.push(ShapeVertex {
                        position: [width + spike_depth, arrow_y + arrow_width / 2.0],
                        color,
                    });
                }
                ArrowPosition::None => {}
            }

            if arrow != ArrowPosition::None {
                indices.push(arrow_vertices_start);
                indices.push(arrow_vertices_start + 1);
                indices.push(arrow_vertices_start + 2);
            }
        }

        let bounds = match arrow {
            ArrowPosition::Bottom(_) => {
                [-spike_depth, -spike_depth, width + 2.0 * spike_depth, height + 35.0 + spike_depth]
            }
            ArrowPosition::Top(_) => {
                [-spike_depth, -35.0 - spike_depth, width + 2.0 * spike_depth, height + 35.0 + spike_depth]
            }
            ArrowPosition::Left(_) => {
                [-35.0 - spike_depth, -spike_depth, width + 35.0 + spike_depth, height + 2.0 * spike_depth]
            }
            ArrowPosition::Right(_) => {
                [-spike_depth, -spike_depth, width + 35.0 + spike_depth, height + 2.0 * spike_depth]
            }
            ArrowPosition::None => {
                [-spike_depth, -spike_depth, width + 2.0 * spike_depth, height + 2.0 * spike_depth]
            }
        };

        Self {
            vertices,
            indices,
            bounds,
        }
    }

    /// Get the vertices
    pub fn vertices(&self) -> &[ShapeVertex] {
        &self.vertices
    }

    /// Get the indices
    pub fn indices(&self) -> &[u16] {
        &self.indices
    }

    /// Get the bounding box [x, y, width, height]
    pub fn bounds(&self) -> [f32; 4] {
        self.bounds
    }
}

/// GPU renderer for callout shapes
pub struct ShapeRenderer {
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
    uniform_buffer: Buffer,
    current_bind_group: Option<BindGroup>,
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    index_count: u32,
}

impl ShapeRenderer {
    /// Create a new shape renderer
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shape Shader"),
            source: wgpu::ShaderSource::Wgsl(SHAPE_SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shape Bind Group Layout"),
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
            label: Some("Shape Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shape Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[ShapeVertex::desc()],
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
            label: Some("Shape Uniform Buffer"),
            contents: bytemuck::cast_slice(&[ShapeUniforms {
                transform: [1.0, 1.0, 0.0, 0.0],
                viewport: [800.0, 600.0, 0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            current_bind_group: None,
            vertex_buffer: None,
            index_buffer: None,
            index_count: 0,
        }
    }

    /// Prepare the renderer with a shape
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        shape: &CalloutShape,
        position: [f32; 2],
        viewport: [f32; 2],
    ) {
        // Update uniforms
        let uniforms = ShapeUniforms {
            transform: [1.0, 1.0, position[0], position[1]],
            viewport: [viewport[0], viewport[1], 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        // Create vertex buffer
        self.vertex_buffer = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Shape Vertex Buffer"),
            contents: bytemuck::cast_slice(shape.vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        }));

        // Create index buffer
        self.index_buffer = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Shape Index Buffer"),
            contents: bytemuck::cast_slice(shape.indices()),
            usage: wgpu::BufferUsages::INDEX,
        }));

        self.index_count = shape.indices().len() as u32;

        // Create bind group
        self.current_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shape Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.uniform_buffer.as_entire_binding(),
            }],
        }));
    }

    /// Render the prepared shape
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        if let (Some(bind_group), Some(vertex_buffer), Some(index_buffer)) =
            (&self.current_bind_group, &self.vertex_buffer, &self.index_buffer)
        {
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, bind_group, &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..self.index_count, 0, 0..1);
        }
    }
}

const SHAPE_SHADER: &str = r#"
struct Uniforms {
    transform: vec4<f32>,  // scale_x, scale_y, offset_x, offset_y
    viewport: vec4<f32>,   // width, height, 0, 0
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

    // Apply transform (scale and offset)
    let world_pos = in.position * uniforms.transform.xy + uniforms.transform.zw;

    // Convert to clip space (-1 to 1)
    let clip_x = (world_pos.x / uniforms.viewport.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / uniforms.viewport.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = in.color;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;
