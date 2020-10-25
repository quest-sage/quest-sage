use std::sync::Arc;
use wgpu::*;
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

use crate::assets::TextureAssetLoader;
use qs_common::assets::{AssetManager, AssetPath};

mod batch;
pub use batch::*;
mod texture;
pub use texture::Texture;
pub use texture::*; // want to use our texture over the wgpu texture

/// This struct represents the state of the whole application and contains all of the `winit`
/// and `wgpu` data for rendering things to the screen.
pub struct Application {
    window: Window,

    surface: Surface,
    device: Arc<Device>,
    queue: Arc<Queue>,

    /// The dimensions of the window's area we can render to.
    size: winit::dpi::PhysicalSize<u32>,

    /// Provides a way for us to recreate the swap chain when we (for example) resize the window.
    swap_chain_descriptor: SwapChainDescriptor,
    swap_chain: SwapChain,
    render_pipeline: RenderPipeline,

    /// A batch for rendering many shapes in a single draw call.
    batch: Batch,
    texture_am: AssetManager<AssetPath, Texture, TextureAssetLoader>,
}

impl Application {
    /// # Returns
    /// In order to keep the event loop (which is global to all windows) from polluting the
    /// lifetime of the application, we return them separately.
    ///
    /// # Panics
    /// Some `wgpu` types are created asynchronously, so this function is asynchronous.
    /// However, it must be called on the main thread to ensure that `winit` is happy with cross platform support.
    pub async fn new() -> (Application, EventLoop<()>) {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .with_title("Quest Sage")
            .build(&event_loop)
            .unwrap();

        // The amount of pixels we have to work with in our window.
        let size = window.inner_size();

        // These three variables essentially encapsulate various handles to the graphics card
        // and specifically the window we're working with.
        // Using BackendBit::PRIMARY we request the Vulkan + Metal + DX12 backends.
        let instance = Instance::new(BackendBit::PRIMARY);
        let surface = unsafe { instance.create_surface(&window) };
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::Default,
                compatible_surface: Some(&surface),
            })
            .await
            .unwrap();

        // Device is a connection to the graphics card. The queue allows us to
        // send commands to the device, which are executed asynchronously.
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    features: Features::empty(),
                    limits: Limits::default(),
                    shader_validation: true,
                },
                None,
            )
            .await
            .unwrap();
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        // The swap chain represents the images that will be presented to the `surface` above.
        // When we resize the window, we need to recreate the swap chain because the images
        // to be presented are now a different size.
        let swap_chain_descriptor = SwapChainDescriptor {
            usage: TextureUsage::OUTPUT_ATTACHMENT,
            format: TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: PresentMode::Immediate,
        };
        let swap_chain = device.create_swap_chain(&surface, &swap_chain_descriptor);

        // Define how we want to bind textures in our render pipeline.
        let texture_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStage::FRAGMENT,
                        ty: BindingType::SampledTexture {
                            multisampled: false,
                            dimension: TextureViewDimension::D2,
                            component_type: TextureComponentType::Uint,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStage::FRAGMENT,
                        ty: BindingType::Sampler { comparison: false },
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });

        // Now we'll define some shaders and the render pipeline.
        let vs_module = device
            .create_shader_module(include_spirv!("../compiled_assets/shaders/shader.vert.spv"));
        let fs_module = device
            .create_shader_module(include_spirv!("../compiled_assets/shaders/shader.frag.spv"));
        let render_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex_stage: ProgrammableStageDescriptor {
                module: &vs_module,
                entry_point: "main",
            },
            fragment_stage: Some(ProgrammableStageDescriptor {
                module: &fs_module,
                entry_point: "main",
            }),
            rasterization_state: Some(RasterizationStateDescriptor {
                front_face: FrontFace::Ccw,
                cull_mode: CullMode::Back,
                depth_bias: 0,
                depth_bias_slope_scale: 0.0,
                depth_bias_clamp: 0.0,
                clamp_depth: false,
            }),
            color_states: &[ColorStateDescriptor {
                format: swap_chain_descriptor.format,
                color_blend: BlendDescriptor::REPLACE,
                alpha_blend: BlendDescriptor::REPLACE,
                write_mask: ColorWrite::ALL,
            }],
            primitive_topology: PrimitiveTopology::TriangleList,
            depth_stencil_state: None,
            vertex_state: VertexStateDescriptor {
                index_format: IndexFormat::Uint16,
                vertex_buffers: &[Vertex::get_buffer_descriptor()],
            },
            sample_count: 1,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
        });

        // Let's create a batch to render many shapes in a single render pass.
        let batch = Batch::new(&device, texture_bind_group_layout);

        let texture_am = AssetManager::new(TextureAssetLoader::new(
            Arc::clone(&device),
            Arc::clone(&queue),
        ));

        (
            Application {
                window,

                surface,
                device,
                queue,

                size,

                swap_chain_descriptor,
                swap_chain,
                render_pipeline,

                batch,
                texture_am,
            },
            event_loop,
        )
    }

    /// # Arguments
    /// If `scale_factor` is `None`, then the scale factor did not change.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>, scale_factor: Option<f64>) {
        tracing::info!("Got new size: {:?} with scale {:?}", new_size, scale_factor);
        self.size = new_size;
        self.swap_chain_descriptor.width = new_size.width;
        self.swap_chain_descriptor.height = new_size.height;
        self.swap_chain = self
            .device
            .create_swap_chain(&self.surface, &self.swap_chain_descriptor);
    }

    /// Renders a single frame, submitting it to the swap chain.
    pub fn render(&mut self) {
        // Get a handle to a texture that we can render the next frame to.
        let frame = self
            .swap_chain
            .get_current_frame()
            .expect("Timeout getting texture")
            .output;

        // Clear the screen with a default colour.
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Clear Colour Encoder"),
            });
        let render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
            color_attachments: &[RenderPassColorAttachmentDescriptor {
                attachment: &frame.view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: true,
                },
            }],
            depth_stencil_attachment: None,
        });
        // Drop the render pass to tell `wgpu` to stop recording commands for this render pass.
        drop(render_pass);
        // Send the render pass into the queue to be actually rendered.
        self.queue.submit(std::iter::once(encoder.finish()));

        // Actually render stuff here.
        use itertools::iproduct;
        const AMOUNT: i64 = 10;
        const SIZE: f32 = 1.0 / AMOUNT as f32;
        let renderables = iproduct!(-AMOUNT..AMOUNT, -AMOUNT..AMOUNT)
            .map(|(x, y)| (x as f32 * SIZE, y as f32 * SIZE))
            .map(|(x, y)| {
                // `wgpu` stores texture coords with the origin in the top left, and the v axis pointing downwards.
                Renderable::Quadrilateral(
                    Vertex {
                        position: [x + SIZE * -0.4, -0.4 * SIZE + y, 0.0],
                        color: [1.0, 0.0, 0.0],
                        tex_coords: [0.0, 1.0],
                    },
                    Vertex {
                        position: [x + SIZE * 0.4, -0.4 * SIZE + y, 0.0],
                        color: [0.0, 1.0, 0.0],
                        tex_coords: [1.0, 1.0],
                    },
                    Vertex {
                        position: [x + SIZE * 0.4, 0.4 * SIZE + y, 0.0],
                        color: [0.0, 0.0, 1.0],
                        tex_coords: [1.0, 0.0],
                    },
                    Vertex {
                        position: [x + SIZE * -0.4, 0.4 * SIZE + y, 0.0],
                        color: [1.0, 0.0, 1.0],
                        tex_coords: [0.0, 0.0],
                    },
                )
            });

        self.batch.render(
            &self.device,
            &self.queue,
            &frame,
            &self.render_pipeline,
            self.texture_am
                .get(AssetPath::new(vec!["test.png".to_string()])),
            renderables,
        );
    }

    /// Executes the application.
    pub async fn run(mut self, event_loop: EventLoop<()>) {
        event_loop.run(move |event, _, control_flow| {
            match event {
                Event::WindowEvent { event, window_id } if window_id == self.window.id() => {
                    match event {
                        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,

                        WindowEvent::KeyboardInput { input, .. } => match input {
                            KeyboardInput {
                                state: ElementState::Pressed,
                                virtual_keycode: Some(VirtualKeyCode::Escape),
                                ..
                            } => *control_flow = ControlFlow::Exit,
                            _ => {}
                        },

                        WindowEvent::Resized(new_size) => self.resize(new_size, None),
                        WindowEvent::ScaleFactorChanged {
                            new_inner_size,
                            scale_factor,
                        } => self.resize(*new_inner_size, Some(scale_factor)),

                        _ => {}
                    }
                }

                Event::RedrawRequested(window_id) if window_id == self.window.id() => {
                    self.render();
                }

                Event::MainEventsCleared => {
                    // RedrawRequested will only trigger once, unless we manually
                    // request it.
                    self.window.request_redraw();
                }

                _ => {}
            }
        });
    }
}
