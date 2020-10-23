use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

mod batch;
pub use batch::*;

/// This struct represents the state of the whole application and contains all of the `winit`
/// and `wgpu` data for rendering things to the screen.
pub struct Application {
    window: Window,

    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,

    /// The dimensions of the window's area we can render to.
    size: winit::dpi::PhysicalSize<u32>,

    /// Provides a way for us to recreate the swap chain when we (for example) resize the window.
    swap_chain_descriptor: wgpu::SwapChainDescriptor,
    swap_chain: wgpu::SwapChain,
    render_pipeline: wgpu::RenderPipeline,

    /// A batch for rendering many shapes in a single draw call.
    batch: Batch,
    /// A debug texture for testing.
    texture_view: wgpu::TextureView,
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
        let instance = wgpu::Instance::new(wgpu::BackendBit::PRIMARY);
        let surface = unsafe { instance.create_surface(&window) };
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::Default,
                compatible_surface: Some(&surface),
            })
            .await
            .unwrap();

        // Device is a connection to the graphics card. The queue allows us to
        // send commands to the device, which are executed asynchronously.
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                    shader_validation: true,
                },
                None,
            )
            .await
            .unwrap();

        // The swap chain represents the images that will be presented to the `surface` above.
        // When we resize the window, we need to recreate the swap chain because the images
        // to be presented are now a different size.
        let swap_chain_descriptor = wgpu::SwapChainDescriptor {
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Immediate,
        };
        let swap_chain = device.create_swap_chain(&surface, &swap_chain_descriptor);

        // Define how we want to bind textures in our render pipeline.
        let texture_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStage::FRAGMENT,
                    ty: wgpu::BindingType::SampledTexture {
                        multisampled: false,
                        dimension: wgpu::TextureViewDimension::D2,
                        component_type: wgpu::TextureComponentType::Uint,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStage::FRAGMENT,
                    ty: wgpu::BindingType::Sampler { comparison: false },
                    count: None,
                },
            ],
            label: Some("texture_bind_group_layout"),
        });

        // Now we'll define some shaders and the render pipeline.
        let vs_module =
            device.create_shader_module(wgpu::include_spirv!("../compiled_assets/shaders/shader.vert.spv"));
        let fs_module =
            device.create_shader_module(wgpu::include_spirv!("../compiled_assets/shaders/shader.frag.spv"));
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&texture_bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex_stage: wgpu::ProgrammableStageDescriptor {
                module: &vs_module,
                entry_point: "main",
            },
            fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                module: &fs_module,
                entry_point: "main",
            }),
            rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: wgpu::CullMode::Back,
                depth_bias: 0,
                depth_bias_slope_scale: 0.0,
                depth_bias_clamp: 0.0,
                clamp_depth: false,
            }),
            color_states: &[wgpu::ColorStateDescriptor {
                format: swap_chain_descriptor.format,
                color_blend: wgpu::BlendDescriptor::REPLACE,
                alpha_blend: wgpu::BlendDescriptor::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
            }],
            primitive_topology: wgpu::PrimitiveTopology::TriangleList,
            depth_stencil_state: None,
            vertex_state: wgpu::VertexStateDescriptor {
                index_format: wgpu::IndexFormat::Uint16,
                vertex_buffers: &[Vertex::get_buffer_descriptor()],
            },
            sample_count: 1,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
        });

        // Let's create a batch to render many shapes in a single render pass.
        let batch = Batch::new(&device, texture_bind_group_layout);

        // Now, let's initialise a texture for testing purposes.
        let texture_image = image::load_from_memory(include_bytes!("../compiled_assets/test.png"))
            .expect("Could not load test.png");
        let texture_rgba = texture_image.to_rgba();
        use image::GenericImageView;
        let texture_dimensions = texture_image.dimensions();
        let texture_size = wgpu::Extent3d {
            width: texture_dimensions.0,
            height: texture_dimensions.1,
            depth: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            // All textures are stored as 3D, we represent our 2D texture
            // by setting depth to 1.
            size: texture_size,
            mip_level_count: 1, // We'll talk about this a little later
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            // SAMPLED tells wgpu that we want to use this texture in shaders
            // COPY_DST means that we want to copy data to this texture
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
            label: Some("diffuse_texture"),
        });

        // Send the RGBA data to our graphics card.
        queue.write_texture(
            // Tells `wgpu` where to copy the pixel data
            wgpu::TextureCopyView {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            // The actual pixel data
            &texture_rgba,
            // The layout of the texture
            wgpu::TextureDataLayout {
                offset: 0,
                bytes_per_row: 4 * texture_dimensions.0,
                rows_per_image: texture_dimensions.1,
            },
            texture_size,
        );

        // Define some metadata associated with the texture to let us use it in a rnder pass.
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        (Application {
            window,

            surface,
            device,
            queue,

            size,

            swap_chain_descriptor,
            swap_chain,
            render_pipeline,

            batch,
            texture_view,
        }, event_loop)
    }

    /// # Arguments
    /// If `scale_factor` is `None`, then the scale factor did not change.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>, scale_factor: Option<f64>) {
        tracing::info!("Got new size: {:?} with scale {:?}", new_size, scale_factor);
        self.size = new_size;
        self.swap_chain_descriptor.width = new_size.width;
        self.swap_chain_descriptor.height = new_size.height;
        self.swap_chain = self.device.create_swap_chain(&self.surface, &self.swap_chain_descriptor);
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
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Clear Colour Encoder")
        });
        let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
                Renderable::Quadrilateral(
                    Vertex {
                        position: [x + SIZE * -0.4, -0.4 * SIZE + y, 0.0],
                        color: [1.0, 0.0, 0.0],
                        tex_coords: [0.0, 0.0],
                    },
                    Vertex {
                        position: [x + SIZE * 0.4, -0.4 * SIZE + y, 0.0],
                        color: [0.0, 1.0, 0.0],
                        tex_coords: [1.0, 0.0],
                    },
                    Vertex {
                        position: [x + SIZE * 0.4, 0.4 * SIZE + y, 0.0],
                        color: [0.0, 0.0, 1.0],
                        tex_coords: [1.0, 1.0],
                    },
                    Vertex {
                        position: [x + SIZE * -0.4, 0.4 * SIZE + y, 0.0],
                        color: [1.0, 0.0, 1.0],
                        tex_coords: [0.0, 1.0],
                    },
                )
            });

        self.batch.render(&self.device, &self.queue, &frame, &self.render_pipeline, &self.texture_view, renderables);
    }

    /// Executes the application.
    pub async fn run(mut self, event_loop: EventLoop<()>) {
        event_loop.run(move |event, _, control_flow| {
            match event {
                Event::WindowEvent { event, window_id } if window_id == self.window.id() => match event {
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
                    WindowEvent::ScaleFactorChanged { new_inner_size, scale_factor } => {
                        self.resize(*new_inner_size, Some(scale_factor))
                    }

                    _ => {}
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