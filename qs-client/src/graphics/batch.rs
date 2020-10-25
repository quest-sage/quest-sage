use qs_common::assets::Asset;
use wgpu::*;
use qs_common::assets::LoadStatus;
use crate::graphics::Texture;

/// The maximum anout of vertices that may be drawn in a single batched draw call.
/// This must be smaller than the max value of a `u16` (65535) because the index
/// buffer stores the list of vertex indices as a `u16` array.
const MAX_VERTEX_COUNT: usize = 40960;
/// The maximum anout of indices that may be drawn in a single batched draw call.
const MAX_INDEX_COUNT: usize = 81920;

/// This is the internal representation of every vertex that is to be drawn. Per-vertex
/// colouring is supported, so that (for example) gradients can be easily implemented.
///
/// # Representation
/// The Vertex struct is copied directly to the GPU for each vertex. Therefore, it is explicitly
/// marked as `#[repr(C)]`. This ensures that the representation of the vertex exactly matches
/// the `VertexBufferDescriptor` returned by the `desc` function.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub tex_coords: [f32; 2],
}
/// Tell `bytemuck` that we can treat any vertex as plain old data.
unsafe impl bytemuck::Pod for Vertex {}
unsafe impl bytemuck::Zeroable for Vertex {}

impl Vertex {
    /// Tell `wgpu` exactly how a vertex is laid out in memory, so that the shaders can
    /// reference specific fields on the vertex.
    pub fn get_buffer_descriptor<'a>() -> VertexBufferDescriptor<'a> {
        VertexBufferDescriptor {
            stride: std::mem::size_of::<Vertex>() as BufferAddress,
            step_mode: InputStepMode::Vertex,
            attributes: &[
                VertexAttributeDescriptor {
                    offset: 0,
                    shader_location: 0,
                    format: VertexFormat::Float3,
                },
                VertexAttributeDescriptor {
                    offset: std::mem::size_of::<[f32; 3]>() as BufferAddress,
                    shader_location: 1,
                    format: VertexFormat::Float3,
                },
                VertexAttributeDescriptor {
                    offset: std::mem::size_of::<[f32; 6]>() as BufferAddress,
                    shader_location: 2,
                    format: VertexFormat::Float2,
                },
            ],
        }
    }
}

/// An item that can be rendered using a `Batch`.
/// To render items using a batch, call the `render` method on the batch.
pub enum Renderable {
    Empty,
    Triangle(Vertex, Vertex, Vertex),
    Quadrilateral(Vertex, Vertex, Vertex, Vertex),
}

/// The `Batch` combines multiple render calls with the same uniform parameters (textures, camera matrix, etc.)
/// into a single render pass.
pub struct Batch {
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    texture_bind_group_layout: BindGroupLayout,
}

impl Batch {
    /// Creates a new batch. Note that allocating enough room on the graphics card to store a batch is a relatively
    /// expensive operation - don't create a batch every frame or just for one object, for example.
    pub fn new(device: &Device, texture_bind_group_layout: BindGroupLayout) -> Batch {
        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("batch_vbo"),
            size: MAX_VERTEX_COUNT as BufferAddress
                * std::mem::size_of::<Vertex>() as BufferAddress,
            usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("batch_ibo"),
            size: MAX_INDEX_COUNT as BufferAddress * std::mem::size_of::<u16>() as BufferAddress,
            usage: BufferUsage::INDEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        });

        Batch {
            vertex_buffer,
            index_buffer,
            texture_bind_group_layout,
        }
    }

    /// Renders the contents of the `verts` and `inds` buffers to the screen.
    #[inline(always)]
    fn flush(
        &mut self,
        device: &Device,
        queue: &Queue,
        frame: &SwapChainTexture,
        encoder: &mut CommandEncoder,

        render_pipeline: &RenderPipeline,
        texture: &Asset<Texture>,

        verts: &mut Vec<Vertex>,
        inds: &mut Vec<u16>,
    ) {
        if inds.len() != 0 {
            if inds.len() % 2 == 1 {
                inds.push(0); // dummy value to align the slice to a size that is a multiple of 4 bytes
            }

            let mut render = |texture: &Texture| {
                // Describe how we want to send the texture to the GPU.
                let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: &self.texture_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&texture.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&texture.sampler),
                        },
                    ],
                    label: Some("texture_bind_group"),
                });
        
                // Begin recording a render pass. When we drop this struct, `wgpu` will finish recording.
                // This allows us to send this recorded list of commands to the GPU.
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                        attachment: &frame.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: None,
                });
                render_pass.set_pipeline(render_pipeline);
        
                render_pass.set_bind_group(0, &texture_bind_group, &[]);
        
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass.set_index_buffer(self.index_buffer.slice(..));

                queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&verts));
                queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&inds));

                render_pass.draw_indexed(0..inds.len() as u32, 0, 0..1);

                drop(render_pass);
                let old_encoder = std::mem::replace(encoder, device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Render Encoder"),
                    })
                );
                queue.submit(std::iter::once(old_encoder.finish()));
            };

            // TODO make a default texture for unloaded textures.
            if let Some(asset) = texture.data.upgrade() {
                if let LoadStatus::Loaded(tex) = &*asset.read().unwrap() {
                    render(tex);
                }
            }
        }

        verts.clear();
        inds.clear();
    }

    /// If there is insufficient capacity to store this amount of new vertices and indices, we will flush
    /// the batch's buffers so that they are free to be used.
    #[inline(always)]
    #[contracts::requires(new_verts < MAX_VERTEX_COUNT)]
    #[contracts::requires(new_inds < MAX_INDEX_COUNT)]
    fn ensure_capacity(
        &mut self,
        device: &Device,
        queue: &Queue,
        frame: &SwapChainTexture,
        encoder: &mut CommandEncoder,

        render_pipeline: &RenderPipeline,
        texture: &Asset<Texture>,

        verts: &mut Vec<Vertex>,
        inds: &mut Vec<u16>,

        new_verts: usize,
        new_inds: usize,
    ) {
        if verts.len() + new_verts > MAX_VERTEX_COUNT || inds.len() + new_inds > MAX_INDEX_COUNT {
            self.flush(device, queue, frame, encoder, render_pipeline, texture, verts, inds);
        }
    }

    pub fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        frame: &SwapChainTexture,

        render_pipeline: &RenderPipeline,
        texture: &Asset<Texture>,
        items: impl Iterator<Item = Renderable>,
    ) {
        // Create a command encoder that records our render information to be sent to the GPU.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("batch_render_encoder"),
        });

        // Store the vertices and indices so that we can write them to the vertex buffer and index buffer in a single function call.
        let mut verts = Vec::<Vertex>::new();
        let mut inds = Vec::<u16>::new();

        for renderable in items {
            match renderable {
                Renderable::Empty => {}
                Renderable::Triangle(v0, v1, v2) => {
                    self.ensure_capacity(device, queue, frame, &mut encoder, render_pipeline, texture, &mut verts, &mut inds, 3, 3);
                    let i0 = verts.len() as u16;
                    verts.push(v0);
                    verts.push(v1);
                    verts.push(v2);
                    inds.push(i0);
                    inds.push(i0 + 1);
                    inds.push(i0 + 2);
                }
                Renderable::Quadrilateral(v0, v1, v2, v3) => {
                    self.ensure_capacity(device, queue, frame, &mut encoder, render_pipeline, texture, &mut verts, &mut inds, 4, 6);
                    let i0 = verts.len() as u16;
                    verts.push(v0);
                    verts.push(v1);
                    verts.push(v2);
                    verts.push(v3);
                    inds.push(i0);
                    inds.push(i0 + 1);
                    inds.push(i0 + 2);
                    inds.push(i0);
                    inds.push(i0 + 2);
                    inds.push(i0 + 3);
                }
            }
        }

        self.flush(device, queue, frame, &mut encoder, render_pipeline, texture, &mut verts, &mut inds);
    }
}
