#[macro_use]
extern crate memoffset;

use std::convert::TryInto;
use std::time::{Duration, Instant};

use glfw::{Context, WindowEvent};

use dawn::{
    native_swap_chain, util, AdapterType, BackendType, BindGroupDescriptor, BindGroupEntry,
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType, BlendDescriptor,
    BlendFactor, BlendOperation, BufferBinding, BufferDescriptor, BufferUsage, Color,
    ColorStateDescriptor, ColorWrite, CullMode, DeviceDescriptor, FrontFace, IndexFormat,
    InputStepMode, Instance, LoadOp, PipelineLayoutDescriptor, PresentMode, PrimitiveTopology,
    ProgrammableStageDescriptor, RasterizationStateDescriptor, RenderPassColorAttachmentDescriptor,
    RenderPassDescriptor, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderStage, StoreOp,
    TextureFormat, TextureUsage, VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
    VertexFormat, VertexStateDescriptor,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut glfw = glfw::init(glfw::FAIL_ON_ERRORS.clone()).expect("glfwInit failed");

    let mut select_backend_type = None;
    let mut select_adapter_type = None;

    let args = std::env::args().collect::<Vec<_>>();

    for (i, arg) in args.iter().enumerate() {
        match args.get(i + 1) {
            Some(backend) if arg == "--backend" => match backend.to_ascii_lowercase().as_str() {
                "vulkan" => select_backend_type = Some(BackendType::Vulkan),
                "d3d12" => select_backend_type = Some(BackendType::D3D12),
                _ => {}
            },
            Some(adapter) if arg == "--adapter" => match adapter.to_ascii_lowercase().as_str() {
                "discrete" => select_adapter_type = Some(AdapterType::DiscreteGPU),
                "integrated" => select_adapter_type = Some(AdapterType::IntegratedGPU),
                _ => {}
            },
            _ => {}
        }
    }

    let instance = Instance::new();
    let adapters = instance.enumerate_adapters();

    let adapter = adapters.iter().find(|adapter| {
        let properties = adapter.properties();
        let backend_matches = select_backend_type
            .map(|backend_type| backend_type == properties.backend_type)
            .unwrap_or(true);
        let adapter_matches = select_adapter_type
            .map(|adapter_type| adapter_type == properties.adapter_type)
            .unwrap_or(true);
        backend_matches && adapter_matches
    });

    let adapter = adapter.or(adapters.first()).expect("No adapters found");

    println!("{:#?}", adapter.properties());

    let (width, height) = (800, 600);

    glfw.window_hint(glfw::WindowHint::ClientApi(glfw::ClientApiHint::NoApi));

    let (mut window, _) = glfw
        .create_window(width, height, "GLFW Window", glfw::WindowMode::Windowed)
        .expect("create window failed");

    window.set_all_polling(true);

    let device = adapter.create_device(&DeviceDescriptor::default());

    struct PrintError;
    impl dawn::ErrorCallback for PrintError {
        fn error(msg: &str, err: dawn::ErrorType, _: *mut libc::c_void) {
            let bt = backtrace::Backtrace::new();
            eprintln!("[{:?}] {}", err, msg);
            eprintln!("{:?}", bt);
            eprintln!();
        }
    }

    device.set_error_callback::<PrintError>();

    let swapchain_format = TextureFormat::RGBA8Unorm;
    let swapchain_usage = TextureUsage::OUTPUT_ATTACHMENT | TextureUsage::PRESENT;

    let mut queue = device.default_queue();

    let swap_chain = match adapter.properties().backend_type {
        BackendType::Vulkan => {
            let vk_instance = native_swap_chain::get_vulkan_instance(&device);
            let vk_surface = unsafe {
                let mut vk_surface = std::mem::zeroed();
                let ret = glfw::ffi::glfwCreateWindowSurface(
                    vk_instance,
                    window.window_ptr(),
                    std::ptr::null(),
                    &mut vk_surface,
                );
                assert_eq!(ret, 0, "glfwCreateWindowSurface failed");
                vk_surface
            };
            native_swap_chain::create_swap_chain(
                &device,
                native_swap_chain::NativeSwapChainDescriptor {
                    width,
                    height,
                    present_mode: PresentMode::Fifo,
                    params: native_swap_chain::NativeSwapChainSurfaceParams::Vulkan {
                        surface: vk_surface,
                    },
                },
            )
        }
        BackendType::D3D12 => native_swap_chain::create_swap_chain(
            &device,
            native_swap_chain::NativeSwapChainDescriptor {
                width,
                height,
                present_mode: PresentMode::Fifo,
                params: native_swap_chain::NativeSwapChainSurfaceParams::D3D12 {
                    hwnd: window.get_win32_window(),
                },
            },
        ),
        backend_type => unimplemented!("{:?}", backend_type),
    };

    swap_chain.configure(swapchain_format, swapchain_usage, width, height);

    let vertex_shader = device.create_shader_module(&ShaderModuleDescriptor {
        label: None,
        code: &util::spirv(include_bytes!("triangle.vert.spv")),
    });

    let fragment_shader = device.create_shader_module(&ShaderModuleDescriptor {
        label: None,
        code: &util::spirv(include_bytes!("triangle.frag.spv")),
    });

    let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: None,
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStage::VERTEX,
            ty: BindingType::UniformBuffer { dynamic: false },
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[bind_group_layout.clone()],
    });

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct Uniforms {
        proj: [[f32; 4]; 4],
        time: f32,
    }

    let uniforms_size_bytes = std::mem::size_of::<Uniforms>();

    let uniform_buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: uniforms_size_bytes as _,
        usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
    });

    #[rustfmt::skip]
    let mut uniforms = Uniforms {
        proj: [
            [1.0,  0.0,  0.0,  0.0],
            [0.0,  1.0,  0.0,  0.0],
            [0.0,  0.0, -0.5,  0.0],
            [0.0,  0.0,  0.5,  1.0],
        ],
        time: 0.0,
    };

    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &bind_group_layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::BufferBinding(BufferBinding {
                buffer: &uniform_buffer,
                offset: 0,
                size: uniforms_size_bytes as _,
            }),
        }],
    });

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct Vertex {
        position: [f32; 3],
        color: [f32; 3],
    }

    #[rustfmt::skip]
    let vertices = &[
        Vertex { position: [-0.5, -0.5, 0.0], color: [1.0, 0.0, 0.0] },
        Vertex { position: [ 0.5, -0.5, 0.0], color: [0.0, 1.0, 0.0] },
        Vertex { position: [ 0.0,  0.5, 0.0], color: [0.0, 0.0, 1.0] },
    ];

    let vertices_size_bytes = std::mem::size_of::<Vertex>() * vertices.len();

    let vertex_buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: vertices_size_bytes as _,
        usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
    });

    let staging_vertex_buffer = device.create_buffer_mapped(&BufferDescriptor {
        label: None,
        size: vertices_size_bytes as _,
        usage: BufferUsage::COPY_SRC | BufferUsage::MAP_WRITE,
    });

    staging_vertex_buffer
        .data
        .copy_from_slice(byte_slice(&*vertices));

    let mut encoder = device.create_command_encoder(&Default::default());

    encoder.copy_buffer_to_buffer(
        &staging_vertex_buffer.finish(),
        0,
        &vertex_buffer,
        0,
        vertices_size_bytes,
    );

    queue.submit(&[encoder.finish()]);

    let color_replace = BlendDescriptor {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::Zero,
        operation: BlendOperation::Add,
    };

    let render_pipeline_descriptor = RenderPipelineDescriptor {
        label: None,
        layout: &pipeline_layout,
        primitive_topology: PrimitiveTopology::TriangleList,
        vertex_stage: ProgrammableStageDescriptor {
            entry_point: "main",
            module: &vertex_shader,
        },
        fragment_stage: Some(ProgrammableStageDescriptor {
            entry_point: "main",
            module: &fragment_shader,
        }),
        color_states: &[ColorStateDescriptor {
            format: swapchain_format,
            write_mask: ColorWrite::ALL,
            color_blend: color_replace,
            alpha_blend: color_replace,
        }],
        sample_mask: 0xFFFFFFFF,
        depth_stencil_state: None,
        rasterization_state: Some(&RasterizationStateDescriptor {
            front_face: FrontFace::Ccw,
            cull_mode: CullMode::None,
            depth_bias: 0,
            depth_bias_slope_scale: 0.0,
            depth_bias_clamp: 0.0,
        }),
        sample_count: 1,
        vertex_state: &VertexStateDescriptor {
            index_format: IndexFormat::Uint16,
            vertex_buffers: &[VertexBufferLayoutDescriptor {
                step_mode: InputStepMode::Vertex,
                array_stride: std::mem::size_of::<Vertex>() as _,
                attributes: &[
                    VertexAttributeDescriptor {
                        format: VertexFormat::Float3,
                        offset: offset_of!(Vertex, position) as _,
                        shader_location: 0,
                    },
                    VertexAttributeDescriptor {
                        format: VertexFormat::Float3,
                        offset: offset_of!(Vertex, color) as _,
                        shader_location: 1,
                    },
                ],
            }],
        },
        alpha_to_coverage_enabled: false,
    };

    let pipeline = device.create_render_pipeline(&render_pipeline_descriptor);

    let start = Instant::now();

    let mut last_frame_time = Instant::now();
    let mut last_fps_time = Instant::now();
    let mut frame_count = 0;

    let mut render_fn = || {
        frame_count += 1;

        if last_fps_time.elapsed() > Duration::from_millis(1000) {
            println!("FPS: {}", frame_count);
            frame_count = 0;
            last_fps_time = Instant::now();
        }

        let frame_view = swap_chain.get_current_texture_view();
        let frame_time = Instant::now();

        uniforms.time = (start.elapsed().as_millis() as f32) / 1000.0;

        uniform_buffer.set_sub_data(0, byte_cast(&uniforms));

        let mut encoder = device.create_command_encoder(&Default::default());
        let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[RenderPassColorAttachmentDescriptor {
                attachment: &frame_view,
                clear_color: Color {
                    r: 0.1,
                    g: 0.1,
                    b: 0.1,
                    a: 1.0,
                },
                load_op: LoadOp::Clear,
                store_op: StoreOp::Store,
                resolve_target: None,
            }],
            depth_stencil_attachment: None,
        });

        render_pass.set_pipeline(&pipeline);
        render_pass.set_vertex_buffer(0, &vertex_buffer, 0);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(3, 1, 0, 1);
        render_pass.end_pass();

        queue.submit(&[encoder.finish()]);
        swap_chain.present();
        device.tick();

        last_frame_time = frame_time;
    };

    while !window.should_close() {
        glfw.poll_events_unbuffered(|_, (_, event)| {
            match event {
                WindowEvent::FramebufferSize(width, height) => {
                    let width = width.try_into().unwrap();
                    let height = height.try_into().unwrap();
                    swap_chain.configure(swapchain_format, swapchain_usage, width, height);
                }
                WindowEvent::Refresh => {
                    render_fn();
                }
                _ => {}
            }
            None
        });

        render_fn();
    }

    Ok(())
}

pub fn byte_stride<T: Copy>(_: &[T]) -> usize {
    std::mem::size_of::<T>()
}

pub fn byte_length<T: Copy>(values: &[T]) -> usize {
    byte_stride(values) * values.len()
}

pub fn byte_offset<T: Copy>(count: usize) -> usize {
    std::mem::size_of::<T>() * count
}

pub fn byte_slice<T: Copy>(values: &[T]) -> &[u8] {
    let len = byte_length(values) as usize;
    unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, len) }
}

pub fn byte_cast<T: Copy>(value: &T) -> &[u8] {
    let len = std::mem::size_of::<T>();
    unsafe { std::slice::from_raw_parts(value as *const T as *const u8, len) }
}
