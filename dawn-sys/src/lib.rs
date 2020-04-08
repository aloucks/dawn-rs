//! # Dawn WebGPU Bindings
//!
//! ## Dawn
//!
//! <https://dawn.googlesource.com/dawn>
//!
//! ### Building
//!
//! Dawn requires [ninja] and [depot_tools].
//!
//! ## WebGPU Spec
//!
//! <https://gpuweb.github.io/gpuweb>
//!
//!
//! [depot_tools]: https://commondatastorage.googleapis.com/chrome-infra-docs/flat/depot_tools/docs/html/depot_tools_tutorial.html#_setting_up
//! [ninja]: https://ninja-build.org

pub type VkInstance = usize;
pub type VkSurfaceKHR = u64;
pub type HWND = *mut libc::c_void;

// Note: Using `#[cfg(feature="bindgen")]` and `#[cfg(note(feature="bindgen"))]` on modules with
// the same name breaks intellijs ability to code complete or goto def. Using conditional `include!`
// seems to be fine though..
//
// https://github.com/intellij-rust/intellij-rust/issues/4693

/// WebGPU API.
///
/// Note that these are currently built from the generated Dawn headers and not from the work-in-progress
/// official headers.
///
/// <https://github.com/webgpu-native/webgpu-headers>
#[allow(non_snake_case)]
#[allow(non_upper_case_globals)]
pub mod webgpu {
    #[cfg(not(feature = "bindgen"))]
    include!("webgpu.rs");

    #[cfg(feature = "bindgen")]
    include!(concat!(env!("OUT_DIR"), "/webgpu.rs"));
}

pub use webgpu::*;

/// Dawn function loader interface.
#[allow(non_snake_case)]
#[allow(non_upper_case_globals)]
pub mod dawn_proc_table {
    #[cfg(not(feature = "bindgen"))]
    include!("dawn_proc_table.rs");

    #[cfg(feature = "bindgen")]
    include!(concat!(env!("OUT_DIR"), "/dawn_proc_table.rs"));
}

pub use dawn_proc_table::*;

#[allow(non_snake_case)]
#[allow(non_upper_case_globals)]
pub mod dawn_wsi {
    #[cfg(not(feature = "bindgen"))]
    include!("dawn_wsi.rs");

    #[cfg(feature = "bindgen")]
    include!(concat!(env!("OUT_DIR"), "/dawn_wsi.rs"));
}

pub use dawn_wsi::*;

// A physical device and backend.
// #[repr(C)]
// #[derive(Debug, Copy, Clone)]
// pub struct DuskAdapter {
//     instance: WGPUInstance,
//     index: u32,
// }

// impl DuskAdapter {
//     pub fn instance(&self) -> WGPUInstance {
//         self.instance
//     }
// }

// unsafe impl Send for DuskAdapter {}
// unsafe impl Sync for DuskAdapter {}

// /// Optional features and settings.
// #[repr(C)]
// #[derive(Debug, Copy, Clone)]
// #[allow(non_snake_case)]
// pub struct DuskDeviceDescriptor {
//     pub requiredExtensions: *const *const libc::c_char,
//     pub requiredExtensionsCount: usize,

//     pub forceEnabledToggles: *const *const libc::c_char,
//     pub forceEnabledTogglesCount: usize,

//     pub forceDisabledToggles: *const *const libc::c_char,
//     pub forceDisabledTogglesCount: usize,
// }

// impl Default for DuskDeviceDescriptor {
//     fn default() -> DuskDeviceDescriptor {
//         unsafe { std::mem::zeroed() }
//     }
// }

#[repr(C)]
#[derive(Debug, Copy, Clone)]
#[allow(non_snake_case)]
pub struct DeviceDescriptor {
    pub requiredExtensions: *const *const libc::c_char,
    pub requiredExtensionsCount: usize,

    pub forceEnabledToggles: *const *const libc::c_char,
    pub forceEnabledTogglesCount: usize,

    pub forceDisabledToggles: *const *const libc::c_char,
    pub forceDisabledTogglesCount: usize,
}

impl Default for DeviceDescriptor {
    fn default() -> DeviceDescriptor {
        unsafe { std::mem::zeroed() }
    }
}

extern "C" {
    /// Set the dawn proc table. Call with a valid proc table before calling any `wgpu` functions.
    pub fn dawnProcSetProcs(proc_table: *const DawnProcTable);

    // /// Populates the default dawn proc table.
    // pub fn duskDawnNativeGetProcs(proc_table: *mut DawnProcTable);

    // /// Enumerate the backend adapters. Pass `null` for the `adapters` and a valid pointer to `count`
    // /// to populate the number of adapters. Call again with a valid `adapters` pointer to an array of
    // /// length `count`.
    // ///
    // /// Returns `0` on success.
    // pub fn duskEnumerateAdapters(
    //     instance: WGPUInstance,
    //     adapters: *mut DuskAdapter,
    //     count: *mut u32,
    // ) -> i32;

    // /// Populates the adapter properties with physical device and backend information,
    // pub fn duskGetAdapterProperties(
    //     adapter: *const DuskAdapter,
    //     properties: *mut WGPUAdapterProperties,
    // ) -> i32;

    // /// Populates the device properties with feature capabilities.
    // pub fn duskGetDeviceProperties(
    //     adapter: *const DuskAdapter,
    //     properties: *mut WGPUDeviceProperties,
    // ) -> i32;

    // /// Creates a device.
    // pub fn duskCreateDevice(
    //     adapter: *const DuskAdapter,
    //     descriptor: *const DuskDeviceDescriptor,
    //     device: *mut WGPUDevice,
    // ) -> i32;

    // pub fn dawnNativeVulkanCreateNativeSwapChainImpl(
    //     device: WGPUDevice,
    //     surface: VkSurfaceKHR,
    //     implementation: *mut dawn_wsi::DawnSwapChainImplementation,
    // );

    // pub fn dawnNativeVulkanGetInstance(device: WGPUDevice) -> VkInstance;

    // --

    /// Populate a proc table with the Dawn Native procs.
    pub fn dawn_native__GetProcs(proc_table: *mut DawnProcTable);

    pub fn dawn_native__Instance__DiscoverDefaultAdapters(instance: WGPUInstance);

    pub fn dawn_native__Instance__GetAdaptersCount(instance: WGPUInstance) -> usize;

    pub fn dawn_native__Adapter__GetAdapterProperties(
        instance: WGPUInstance,
        adapter_index: usize,
    ) -> WGPUDeviceProperties;

    pub fn dawn_native__Adapter__GetProperties(
        instance: WGPUInstance,
        adapter_index: usize,
        properties: *mut WGPUAdapterProperties,
    );

    pub fn dawn_native__vulkan__GetInstance(device: WGPUDevice) -> VkInstance;

    pub fn dawn_native__Adapter__CreateDevice(
        instance: WGPUInstance,
        adapter_index: usize,
        descriptor: *const DeviceDescriptor,
    ) -> WGPUDevice;

    pub fn dawn_native__vulkan__GetNativeSwapChainPreferredFormat(
        swap_chain_impl: *const DawnSwapChainImplementation,
    ) -> WGPUTextureFormat;

    pub fn dawn_native__vulkan__CreateNativeSwapChainImpl(
        device: WGPUDevice,
        surface: VkSurfaceKHR,
    ) -> DawnSwapChainImplementation;

    #[cfg(windows)]
    pub fn dawn_native__d3d12__GetNativeSwapChainPreferredFormat(
        swap_chain_impl: *const DawnSwapChainImplementation,
    ) -> WGPUTextureFormat;

    #[cfg(windows)]
    pub fn dawn_native__d3d12__CreateNativeSwapChainImpl(
        device: WGPUDevice,
        hwnd: HWND,
    ) -> DawnSwapChainImplementation;
}
