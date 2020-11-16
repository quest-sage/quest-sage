use std::sync::Arc;
use std::time::Instant;
use stretch::{
    geometry::{Point, Size},
    number::Number,
    style::{Dimension, Style},
};
use wgpu::*;
use winit::{
    dpi::PhysicalPosition,
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

use crate::{
    assets::{FontAssetLoader, TextureAssetLoader},
    ui::*,
};
use qs_common::profile::InterpolatedStopwatch;
use qs_common::{
    assets::{AssetManager, AssetPath},
    profile::ProfileSegmentGuard,
};

mod batch;
pub use batch::*;
mod texture;
// want to use our texture struct over the wgpu texture
pub use texture::Texture;
pub use texture::*;
mod camera;
pub use camera::*;
mod text;
pub use text::*;
mod multi_batch;
pub use multi_batch::*;

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

    last_frame_time: Instant,
    fps_counter: InterpolatedStopwatch,

    texture_am: AssetManager<AssetPath, Texture, TextureAssetLoader>,
    _font_am: AssetManager<AssetPath, rusttype::Font<'static>, FontAssetLoader>,
    camera: Camera,
    ui_camera: Camera,
    multi_batch: MultiBatch,

    mouse_position: PhysicalPosition<f64>,

    test_font_family: Arc<FontFamily>,
    /// A test widget.
    test_text: RichText,
    ui: UI,
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
        let scale_factor = window.scale_factor();

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
        let texture_bind_group_layout_desc = &BindGroupLayoutDescriptor {
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
        };
        // Define how we want to bind uniforms.
        let uniform_bind_group_layout_desc = wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStage::VERTEX,
                ty: wgpu::BindingType::UniformBuffer {
                    dynamic: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("uniform_bind_group_layout"),
        };

        let camera = Camera::new(CameraData::Orthographic {
            eye: cgmath::Point2::new(0.0, 0.0),
            view_height: 2.0,
            aspect_ratio: 1.0,
        });
        let ui_camera = Camera::new(CameraData::Orthographic {
            eye: cgmath::Point2::new(0.0, 0.0),
            view_height: 800.0,
            aspect_ratio: 1.0,
        });

        // Let's create a batch to render many shapes in a single render pass.
        let batch = Batch::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            include_spirv!("shader.vert.spv"),
            include_spirv!("shader.frag.spv"),
            device.create_bind_group_layout(&texture_bind_group_layout_desc),
            device.create_bind_group_layout(&uniform_bind_group_layout_desc),
            swap_chain_descriptor.format,
        );

        let mut texture_am = AssetManager::new(TextureAssetLoader::new(
            Arc::clone(&device),
            Arc::clone(&queue),
        ));

        let mut font_am = AssetManager::new(FontAssetLoader::default());

        let text_renderer = TextRenderer::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            device.create_bind_group_layout(&texture_bind_group_layout_desc),
            device.create_bind_group_layout(&uniform_bind_group_layout_desc),
            swap_chain_descriptor.format,
            scale_factor as f32,
        );

        let multi_batch = MultiBatch::new(batch, text_renderer);

        let mut test_text = RichText::new(Default::default());
        let test_font_family = Arc::new(FontFamily::new(vec![FontFace::new(
            "Noto Sans".to_string(),
            font_am.get(AssetPath::new(vec!["NotoSans-Regular.ttf".to_string()])),
            Some(font_am.get(AssetPath::new(vec!["NotoSans-Bold.ttf".to_string()]))),
            Some(font_am.get(AssetPath::new(vec!["NotoSans-Italic.ttf".to_string()]))),
            Some(font_am.get(AssetPath::new(vec!["NotoSans-BoldItalic.ttf".to_string()]))),
        )]));
        let _ = test_text.set_text(Arc::clone(&test_font_family))
        .h1(|b| b
            .write("Header thing ")
            .italic(|b| b
                .coloured(Colour::CYAN, |b| b
                    .write("emphasised")
                )
            )
        )
        .end_paragraph()
        .write("Hello, ")
        .italic(|b| b
            .write("world")
        )
        .write("!")
        .end_paragraph()
        .h1(|b| b.write("aag"))
        .h2(|b| b.write("aag"))
        .h3(|b| b.write("aag"))
        .write("aag")
        .end_paragraph()
        .write("Regular ")
        .italic(|b| b
            .write("Italic ")
            .bold(|b| b
                .write("Bold Italic ")
            )
        )
        .bold(|b| b
            .write("Bold")
        )
        .end_paragraph()
        .write("äÄöÖüÜß€")
        .end_paragraph()
        .write("Lorem ipsum dolor sit amet, consectetur adipiscing elit. Ut facilisis elit at massa placerat, in placerat est pretium. Curabitur consequat porta ante vel pharetra. Vestibulum sit amet mauris rhoncus, facilisis felis et, elementum arcu. In hac habitasse platea dictumst. Nam at felis non lectus aliquam consectetur nec quis tellus. Proin id dictum massa. Sed id condimentum mauris. Morbi eget dictum ligula, non faucibus ante. Morbi viverra ut diam vitae malesuada. Donec porta enim non porttitor euismod. Proin faucibus sit amet diam nec molestie. Fusce porta scelerisque lectus, quis ultrices augue maximus a.")
        .finish().await.expect("could not complete task");

        let test_button_background = Widget::new(
            ImageElement {
                size: Size {
                    width: Dimension::Points(20.0),
                    height: Dimension::Points(20.0),
                },
                colour: Colour::rgb(0.03, 0.03, 0.03),
                texture: texture_am.get(AssetPath::new(vec!["white.png".to_string()])),
            },
            Vec::new(),
            Vec::new(),
            Default::default(),
        );
        let test_button = Widget::new(
            Button,
            vec![test_button_background],
            Vec::new(),
            Default::default(),
        );

        let root = Widget::new(
            (),
            vec![test_text.0.read().unwrap().widget.clone(), test_button],
            vec![Box::new(ImageElement {
                size: Size {
                    width: Dimension::Points(100.0),
                    height: Dimension::Points(100.0),
                },
                colour: Colour {
                    r: 0.4,
                    g: 0.3,
                    b: 0.4,
                    a: 0.7,
                },
                texture: texture_am.get(AssetPath::new(vec!["white.png".to_string()])),
            })],
            Style {
                //align_self: stretch::style::AlignSelf::Stretch,
                //align_items: stretch::style::AlignItems::Stretch,
                //justify_content: stretch::style::JustifyContent::Center,
                flex_direction: stretch::style::FlexDirection::Column,
                ..Default::default()
            },
        );

        let ui = UI::new(
            root,
            Size {
                width: Number::Defined(100.0),
                height: Number::Defined(100.0),
            },
        );

        let mut app = Application {
            window,

            surface,
            device,
            queue,

            size,

            swap_chain_descriptor,
            swap_chain,

            last_frame_time: Instant::now(),
            fps_counter: InterpolatedStopwatch::new(100),

            texture_am,
            _font_am: font_am,
            camera,
            ui_camera,
            multi_batch,

            mouse_position: PhysicalPosition { x: 0.0, y: 0.0 },

            test_font_family,
            test_text,
            ui,
        };

        // Call resize at the start so that we initialise cameras etc with the correct aspect ratio.
        app.resize(size, Some(scale_factor));

        (app, event_loop)
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

        self.camera
            .update_window_size(new_size.width, new_size.height);
        let CameraData::Orthographic {
            ref mut view_height,
            ..
        } = self.ui_camera.get_data_mut();
        *view_height = new_size.height as f32;
        self.ui_camera
            .update_window_size(new_size.width, new_size.height);

        self.ui.update_size(Size {
            width: Number::Defined(new_size.width as f32),
            height: Number::Defined(new_size.height as f32),
        })
    }

    pub fn update_cursor(&mut self, pos: PhysicalPosition<f64>) {
        self.mouse_position = pos;
        self.ui.mouse_move(Point {
            x: pos.x as f32,
            y: pos.y as f32,
        });
    }

    pub fn mouse_input(&mut self, button: MouseButton, state: ElementState) {
        self.ui.mouse_input(button, state);
    }

    /// Renders a single frame, submitting it to the swap chain.
    pub async fn render(&mut self, mut profiler: ProfileSegmentGuard<'_>) {
        let this_frame_time = Instant::now();
        let delta_duration = this_frame_time - self.last_frame_time;
        self.last_frame_time = this_frame_time;
        let _delta_seconds = delta_duration.as_secs_f32();
        self.fps_counter.tick();

        if self.fps_counter.ticks % 100 == 0 {
            self.test_text
                .set_text(Arc::clone(&self.test_font_family))
                .write(&format!("{} frames", self.fps_counter.ticks))
                .finish();
            tracing::trace!(
                "{:.2} FPS",
                1.0 / self.fps_counter.average_time().as_secs_f64()
            );
        }

        {
            //let CameraData::Orthographic { ref mut eye, .. } = self.camera.get_data_mut();
            //eye.x += 0.5 * delta_seconds;
        }

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
                    load: LoadOp::Clear(Color {
                        r: 0.1,
                        g: 0.1,
                        b: 0.1,
                        a: 1.0,
                    }),
                    store: true,
                },
            }],
            depth_stencil_attachment: None,
        });
        // Drop the render pass to tell `wgpu` to stop recording commands for this render pass.
        drop(render_pass);
        // Send the render pass into the queue to be actually rendered.
        self.queue.submit(std::iter::once(encoder.finish()));

        {
            let _guard = profiler.task("background").time();
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
                            color: [1.0, 0.0, 0.0, 1.0],
                            tex_coords: [0.0, 1.0],
                        },
                        Vertex {
                            position: [x + SIZE * 0.4, -0.4 * SIZE + y, 0.0],
                            color: [0.0, 1.0, 0.0, 1.0],
                            tex_coords: [1.0, 1.0],
                        },
                        Vertex {
                            position: [x + SIZE * 0.4, 0.4 * SIZE + y, 0.0],
                            color: [0.0, 0.0, 1.0, 0.0],
                            tex_coords: [1.0, 0.0],
                        },
                        Vertex {
                            position: [x + SIZE * -0.4, 0.4 * SIZE + y, 0.0],
                            color: [1.0, 0.0, 1.0, 0.0],
                            tex_coords: [0.0, 0.0],
                        },
                    )
                });

            self.texture_am
                .get(AssetPath::new(vec!["test.png".to_string()]))
                .if_loaded(|tex| {
                    self.multi_batch
                        .batch
                        .render(&frame, tex, &self.camera, renderables);
                })
                .await;
        }

        {
            let guard = profiler.task("ui").time();
            self.multi_batch
                .render(
                    self.ui.generate_render_info(
                        Point {
                            x: self.size.width as f32 * -0.5,
                            y: self.size.height as f32 * -0.5,
                        },
                        /*Some(
                            self.texture_am
                                .get(AssetPath::new(vec!["white.png".to_string()])),
                        ),*/
                        None,
                    ),
                    &frame,
                    &self.ui_camera,
                    guard,
                )
                .await;
        }
    }

    /// Executes the application.
    pub fn run(mut self, event_loop: EventLoop<()>) {
        let mut profiler = qs_common::profile::CycleProfiler::new(25);

        event_loop.run(move |event, _, control_flow| {
            match event {
                Event::WindowEvent { event, window_id } if window_id == self.window.id() => {
                    match event {
                        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,

                        WindowEvent::KeyboardInput { input, .. } => {
                            if let KeyboardInput {
                                state: ElementState::Pressed,
                                virtual_keycode: Some(VirtualKeyCode::Escape),
                                ..
                            } = input
                            {
                                *control_flow = ControlFlow::Exit;
                            }
                        }

                        WindowEvent::CursorMoved { position, .. } => {
                            self.update_cursor(position);
                        }

                        WindowEvent::MouseInput { button, state, .. } => {
                            self.mouse_input(button, state);
                        }

                        WindowEvent::Resized(new_size) => self.resize(new_size, None),
                        WindowEvent::ScaleFactorChanged {
                            new_inner_size,
                            scale_factor,
                        } => self.resize(*new_inner_size, Some(scale_factor)),

                        _ => {}
                    }
                }

                Event::RedrawRequested(window_id) if window_id == self.window.id() => {
                    profiler.stopwatch.tick();
                    {
                        let mut main_segment = profiler.main_segment.time();
                        {
                            let render = main_segment.task("render").time();
                            futures::executor::block_on(self.render(render));
                        }
                    }
                    if profiler.main_segment.ticks % 100 == 0 {
                        tracing::trace!("{}", profiler);
                    }
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
