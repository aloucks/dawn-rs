use dawn::*;

fn main() {
    let glfw = glfw::init(glfw::FAIL_ON_ERRORS).expect("glfwInit failed");

    let instance = Instance::new();
    let adapters = instance.enumerate_adapters();
    let adapter = adapters.iter().find(|adapter| {
        let properties = adapter.properties();
        properties.backend_type == BackendType::Vulkan
            && properties.adapter_type == AdapterType::DiscreteGPU
    });
    let adapter = adapter.or(adapters.first()).expect("No adapters");

    let (window, _) = glfw
        .create_window(800, 600, "GLFW Window", glfw::WindowMode::Windowed)
        .expect("create window failed");

    let surface = instance.create_surface(&window);

    let device = adapter.create_device(&DeviceDescriptor::default());

    let implementation = 0;

    let swap_chain = device.create_swap_chain(
        Some(&surface),
        &SwapChainDescriptor {
            label: None,
            format: TextureFormat::BGRA8UnormSrgb,
            width: 800,
            height: 600,
            present_mode: PresentMode::Mailbox,
            usage: TextureUsage::OUTPUT_ATTACHMENT,
            implementation,
        },
    );

    let _queue = device.default_queue();

    swap_chain.present();

    device.inject_error("Test error", ErrorType::Validation);

    device.tick();

    std::thread::sleep(std::time::Duration::from_millis(100));
}
