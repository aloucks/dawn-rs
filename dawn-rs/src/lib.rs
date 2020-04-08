#[macro_use]
extern crate bitflags;

use std::{
    convert::TryInto,
    fmt,
    marker::PhantomData,
    mem, ptr, slice,
    sync::{Arc, Once},
};

use parking_lot::Mutex;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
use smallvec::SmallVec;
use unchecked_unwrap::UncheckedUnwrap;

use dawn_sys as sys;
use dawn_sys::WGPUCommandBuffer;

/// A buffer size that indicates the remaining buffer.
pub use sys::WGPU_WHOLE_SIZE as WHOLE_SIZE;

mod convert;

pub mod indirect;
pub mod native_swap_chain;
pub mod util;

static INIT: Once = Once::new();
static mut PROC_TABLE: mem::MaybeUninit<sys::DawnProcTable> = mem::MaybeUninit::uninit();

// Enum conversion regex:
//
// search:  pub const WGPU([^_]+)_([^:]+).*
// enums:   $2 = sys::WGPU$1_$2,
// flags:   const $2 = sys::WGPU$1_$2;

macro_rules! impl_handle_debug {
    ($Type:ty, $parent:ident, $reference:ident, $release:ident) => {
        impl fmt::Debug for $Type {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{:?}", self.raw)
            }
        }
    };
}

macro_rules! impl_handle_no_clone {
    ($Type:ty, $parent:ident, $reference:ident, $release:ident) => {
        impl Drop for $Type {
            fn drop(&mut self) {
                if !self.raw.is_null() {
                    unsafe {
                        (*PROC_TABLE.as_ptr()).$release.unchecked_unwrap()(self.raw);
                    }
                }
            }
        }

        impl_handle_debug!($Type, $parent, $reference, $release);
    };
}

macro_rules! impl_handle {
    ($Type:ident, $parent:ident, $reference:ident, $release:ident) => {
        impl Clone for $Type {
            fn clone(&self) -> $Type {
                if !self.raw.is_null() {
                    unsafe {
                        (*PROC_TABLE.as_ptr()).$reference.unchecked_unwrap()(self.raw);
                    }
                }
                $Type {
                    raw: self.raw,
                    $parent: self.$parent.clone(),
                }
            }
        }

        impl_handle_no_clone!($Type, $parent, $reference, $release);

        // impl_handle_debug!($Type, $parent, $reference, $release);

        // impl Drop for $Type {
        //     fn drop(&mut self) {
        //         if !self.raw.is_null() {
        //             unsafe {
        //                 (*PROC_TABLE.as_ptr()).$release.unchecked_unwrap()(self.raw);
        //             }
        //         }
        //     }
        // }

        // impl fmt::Debug for $Type {
        //     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        //         write!(f, "{:?}", self.raw)
        //     }
        // }

        // unsafe impl Send for $Type {}
        // unsafe impl Sync for $Type {}
    };
}

pub const DEFAULT_MAX_BIND_GROUPS: usize = 4;
pub const DEFAULT_MAX_DYNAMIC_UNIFORM_BUFFERS_PER_PIPELINE_LAYOUT: usize = 8;
pub const DEFAULT_MAX_DYNAMIC_STORAGE_BUFFERS_PER_PIPELINE_LAYOUT: usize = 4;
pub const DEFAULT_MAX_SAMPLED_TEXTURES_PER_SHADER_STAGE: usize = 16;
pub const DEFAULT_MAX_SAMPLERS_PER_SHADER_STAGE: usize = 16;
pub const DEFAULT_MAX_STORAGE_BUFFERS_PER_SHADER_STAGE: usize = 4;
pub const DEFAULT_MAX_STORAGE_TEXTURES_PER_SHADER_STAGE: usize = 4;
pub const DEFAULT_MAX_UNIFORM_BUFFERS_PER_SHADER_STAGE: usize = 12;

/// Set the global Dawn proc table used by dusk. This may be used to install
/// dawn wire, instrumentation, or to substitute another WebGPU implementation.
/// Note that calling this prior to `Instance::new()` will prevent the default
/// dawn native proc table from ever being installed. Installing a proc table
/// with `null` function pointers may result in undefined behavior.
pub unsafe fn set_dawn_proc_table(proc_table: sys::DawnProcTable) {
    INIT.call_once(|| {});
    PROC_TABLE.as_mut_ptr().write(proc_table);
}

#[derive(Debug)]
pub struct Instance {
    raw: sys::WGPUInstance,
}

#[derive(Debug)]
pub struct Adapter {
    instance: sys::WGPUInstance,
    adapter_index: usize,
}

impl Adapter {
    fn from_raw(instance: sys::WGPUInstance, adapter_index: usize) -> Adapter {
        unsafe { (*PROC_TABLE.as_ptr()).instanceReference.unchecked_unwrap()(instance) }
        Adapter {
            instance,
            adapter_index,
        }
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        unsafe { (*PROC_TABLE.as_ptr()).instanceRelease.unchecked_unwrap()(self.instance) }
    }
}

impl Clone for Adapter {
    fn clone(&self) -> Self {
        Adapter::from_raw(self.instance, self.adapter_index)
    }
}

unsafe impl Send for Adapter {}
unsafe impl Sync for Adapter {}

#[derive(Debug)]
struct DeviceInner {
    pub(crate) raw: sys::WGPUDevice,
    raw_default_queue: sys::WGPUQueue,
    adapter: Adapter,
    pub(crate) backend_type: BackendType,
}

impl Drop for DeviceInner {
    fn drop(&mut self) {
        unsafe {
            if !self.raw_default_queue.is_null() {
                sys::wgpuQueueRelease(self.raw_default_queue);
            }
            if !self.raw.is_null() {
                sys::wgpuDeviceRelease(self.raw);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Device {
    // Dawn is not currently thread-safe, so we synchronize all access to the underlying device,
    // including all access to the Queue.
    // https://bugs.chromium.org/p/dawn/issues/detail?id=334&q=thread&can=2
    pub(crate) inner: Arc<Mutex<DeviceInner>>,
    // Async notes:
    // https://bugs.chromium.org/p/dawn/issues/detail?id=119&q=&can=2
}

pub struct Surface {
    raw: sys::WGPUSurface,
    instance: Instance,
}
impl_handle!(Surface, instance, surfaceReference, surfaceRelease);

struct SwapChainInner {
    pub(crate) raw: sys::WGPUSwapChain,
    pub(crate) device: Device,
}
impl_handle!(SwapChainInner, device, swapChainReference, swapChainRelease);

// impl std::ops::Deref for SwapChain {
//     type Target = SwapChainInner;
//     fn deref(&self) -> &Self::Target {
//         &self.inner
//     }
// }

#[derive(Debug)]
pub struct SwapChain {
    pub(crate) inner: SwapChainInner,
    pub(crate) backend_type: BackendType,
    pub(crate) dawn_swap_chain_impl: Option<Arc<sys::DawnSwapChainImplementation>>,
}

pub struct Buffer {
    raw: sys::WGPUBuffer,
    device: Device,
}
impl_handle!(Buffer, device, bufferReference, bufferRelease);

pub struct Texture {
    raw: sys::WGPUTexture,
    device: Device,
}
impl_handle!(Texture, device, textureReference, textureRelease);

pub struct TextureView {
    raw: sys::WGPUTextureView,
    device: Device,
}
impl_handle!(
    TextureView,
    device,
    textureViewReference,
    textureViewRelease
);

pub struct Sampler {
    raw: sys::WGPUSampler,
    device: Device,
}
impl_handle!(Sampler, device, samplerReference, samplerRelease);

pub struct BindGroupLayout {
    raw: sys::WGPUBindGroupLayout,
    device: Device,
}
impl_handle!(
    BindGroupLayout,
    device,
    bindGroupLayoutReference,
    bindGroupLayoutRelease
);

pub struct BindGroup {
    raw: sys::WGPUBindGroup,
    device: Device,
}
impl_handle!(BindGroup, device, bindGroupReference, bindGroupRelease);

pub struct ShaderModule {
    raw: sys::WGPUShaderModule,
    device: Device,
}
impl_handle!(
    ShaderModule,
    device,
    shaderModuleReference,
    shaderModuleRelease
);

pub struct PipelineLayout {
    raw: sys::WGPUPipelineLayout,
    device: Device,
}
impl_handle!(
    PipelineLayout,
    device,
    pipelineLayoutReference,
    pipelineLayoutRelease
);

pub struct RenderPipeline {
    raw: sys::WGPURenderPipeline,
    device: Device,
}
impl_handle!(
    RenderPipeline,
    device,
    renderPipelineReference,
    renderPipelineRelease
);

pub struct ComputePipeline {
    raw: sys::WGPUComputePipeline,
    device: Device,
}
impl_handle!(
    ComputePipeline,
    device,
    computePipelineReference,
    computePipelineRelease
);

pub struct CommandEncoder {
    raw: sys::WGPUCommandEncoder,
    device: Device,
}
impl_handle_no_clone!(
    CommandEncoder,
    device,
    commandEncoderReference,
    commandEncoderRelease
);

pub struct CommandBuffer {
    raw: sys::WGPUCommandBuffer,
    _device: Device,
}
impl_handle_no_clone!(
    CommandBuffer,
    device,
    commandBufferReference,
    commandBufferRelease
);

pub struct Fence {
    raw: sys::WGPUFence,
    device: Device,
}
impl_handle!(Fence, device, fenceReference, fenceRelease);

pub struct Queue {
    raw: sys::WGPUQueue,
    device: Device,
    temp_commands: Vec<WGPUCommandBuffer>,
}
impl_handle_no_clone!(Queue, device, queueReference, queueRelease);

pub struct RenderBundle {
    raw: sys::WGPURenderBundle,
    device: Device,
}
impl_handle!(
    RenderBundle,
    device,
    renderBundleReference,
    renderBundleRelease
);

pub struct RenderBundleEncoder {
    raw: sys::WGPURenderBundleEncoder,
    device: Device, // keep-alive
}
impl_handle_no_clone!(
    RenderBundleEncoder,
    device,
    renderBundleEncoderReference,
    renderBundleEncoderRelease
);

pub struct ComputePassEncoder<'a> {
    raw: sys::WGPUComputePassEncoder,
    _device: Device,
    // keep-alive
    _p: PhantomData<&'a mut CommandEncoder>,
}
impl_handle_no_clone!(
    ComputePassEncoder<'_>,
    device,
    computePassEncoderReference,
    computePassEncoderRelease
);

pub struct RenderPassEncoder<'a> {
    raw: sys::WGPURenderPassEncoder,
    _device: Device,
    // keep-alive
    _p: PhantomData<&'a mut CommandEncoder>,
}
impl_handle_no_clone!(
    RenderPassEncoder<'_>,
    device,
    renderPassEncoderReference,
    renderPassEncoderRelease
);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum AdapterType {
    DiscreteGPU = sys::WGPUAdapterType_DiscreteGPU,
    IntegratedGPU = sys::WGPUAdapterType_IntegratedGPU,
    CPU = sys::WGPUAdapterType_CPU,
    Unknown = sys::WGPUAdapterType_Unknown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum AddressMode {
    ClampToEdge = sys::WGPUAddressMode_ClampToEdge,
    Repeat = sys::WGPUAddressMode_Repeat,
    MirrorRepeat = sys::WGPUAddressMode_MirrorRepeat,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum BackendType {
    Vulkan = sys::WGPUBackendType_Vulkan,
    Metal = sys::WGPUBackendType_Metal,
    D3D12 = sys::WGPUBackendType_D3D12,
    D3D11 = sys::WGPUBackendType_D3D11,
    OpenGL = sys::WGPUBackendType_OpenGL,
    OpenGLES = sys::WGPUBackendType_OpenGLES,
    Null = sys::WGPUBackendType_Null,
}

// #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
// #[repr(i32)]
// pub enum BindingType {
//     UniformBuffer = sys::WGPUBindingType_UniformBuffer,
//     StorageBuffer = sys::WGPUBindingType_StorageBuffer,
//     ReadonlyStorageBuffer = sys::WGPUBindingType_ReadonlyStorageBuffer,
//     Sampler = sys::WGPUBindingType_Sampler,
//     SampledTexture = sys::WGPUBindingType_SampledTexture,
//     StorageTexture = sys::WGPUBindingType_StorageTexture,
//     ReadonlyStorageTexture = sys::WGPUBindingType_ReadonlyStorageTexture,
//     WriteonlyStorageTexture = sys::WGPUBindingType_WriteonlyStorageTexture,
// }

// #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
// #[repr(i32)]
// pub enum BindingType {
//     UniformBuffer {
//         dynamic: bool,
//     },
//     StorageBuffer {
//         dynamic: bool,
//         // readonly: bool,
//     },
//     ReadonlyStorageBuffer {
//         dynamic: bool,
//         // readonly: bool,
//     },
//     Sampler {
//         comparison: bool,
//     },
//     SampledTexture {
//         dimension: TextureViewDimension,
//         component_type: TextureComponentType,
//         multisampled: bool,
//     },
//     StorageTexture {
//         dimension: TextureViewDimension,
//         component_type: TextureComponentType,
//         format: TextureFormat,
//         readonly: bool,
//     },
//     ReadonlyStorageTexture {
//         dimension: TextureViewDimension,
//         component_type: TextureComponentType,
//         format: TextureFormat,
//         // readonly: bool,
//     },
//     WriteonlyStorageTexture {
//         dimension: TextureViewDimension,
//         component_type: TextureComponentType,
//         format: TextureFormat,
//         // readonly: bool,
//     },
// }

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum BindingType {
    UniformBuffer {
        dynamic: bool,
    },
    StorageBuffer {
        dynamic: bool,
        readonly: bool,
    },
    // ReadonlyStorageBuffer {
    //     dynamic: bool,
    //     readonly: bool,
    // },
    Sampler {
        comparison: bool,
    },
    SampledTexture {
        dimension: TextureViewDimension,
        component_type: TextureComponentType,
        multisampled: bool,
    },
    StorageTexture {
        dimension: TextureViewDimension,
        component_type: TextureComponentType,
        format: TextureFormat,
        readonly: bool,
        writeonly: bool,
    },
    // ReadonlyStorageTexture {
    //     dimension: TextureViewDimension,
    //     component_type: TextureComponentType,
    //     format: TextureFormat,
    //     readonly: bool,
    // },
    // WriteonlyStorageTexture {
    //     dimension: TextureViewDimension,
    //     component_type: TextureComponentType,
    //     format: TextureFormat,
    //     readonly: bool,
    //     writeonly: bool,
    // },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum BlendFactor {
    Zero = sys::WGPUBlendFactor_Zero,
    One = sys::WGPUBlendFactor_One,
    SrcColor = sys::WGPUBlendFactor_SrcColor,
    OneMinusSrcColor = sys::WGPUBlendFactor_OneMinusSrcColor,
    SrcAlpha = sys::WGPUBlendFactor_SrcAlpha,
    OneMinusSrcAlpha = sys::WGPUBlendFactor_OneMinusSrcAlpha,
    DstColor = sys::WGPUBlendFactor_DstColor,
    OneMinusDstColor = sys::WGPUBlendFactor_OneMinusDstColor,
    DstAlpha = sys::WGPUBlendFactor_DstAlpha,
    OneMinusDstAlpha = sys::WGPUBlendFactor_OneMinusDstAlpha,
    SrcAlphaSaturated = sys::WGPUBlendFactor_SrcAlphaSaturated,
    BlendColor = sys::WGPUBlendFactor_BlendColor,
    OneMinusBlendColor = sys::WGPUBlendFactor_OneMinusBlendColor,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum BlendOperation {
    Add = sys::WGPUBlendOperation_Add,
    Subtract = sys::WGPUBlendOperation_Subtract,
    ReverseSubtract = sys::WGPUBlendOperation_ReverseSubtract,
    Min = sys::WGPUBlendOperation_Min,
    Max = sys::WGPUBlendOperation_Max,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum CompareFunction {
    Never = sys::WGPUCompareFunction_Never,
    Less = sys::WGPUCompareFunction_Less,
    LessEqual = sys::WGPUCompareFunction_LessEqual,
    Greater = sys::WGPUCompareFunction_Greater,
    GreaterEqual = sys::WGPUCompareFunction_GreaterEqual,
    Equal = sys::WGPUCompareFunction_Equal,
    NotEqual = sys::WGPUCompareFunction_NotEqual,
    Always = sys::WGPUCompareFunction_Always,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum CullMode {
    None = sys::WGPUCullMode_None,
    Front = sys::WGPUCullMode_Front,
    Back = sys::WGPUCullMode_Back,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ErrorFilter {
    None = sys::WGPUErrorFilter_None,
    Validation = sys::WGPUErrorFilter_Validation,
    OutOfMemory = sys::WGPUErrorFilter_OutOfMemory,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ErrorType {
    NoError = sys::WGPUErrorType_NoError,
    Validation = sys::WGPUErrorType_Validation,
    OutOfMemory = sys::WGPUErrorType_OutOfMemory,
    Unknown = sys::WGPUErrorType_Unknown,
    DeviceLost = sys::WGPUErrorType_DeviceLost,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FenceCompletionStatus {
    Success = sys::WGPUFenceCompletionStatus_Success,
    Error = sys::WGPUFenceCompletionStatus_Error,
    Unknown = sys::WGPUFenceCompletionStatus_Unknown,
    DeviceLost = sys::WGPUFenceCompletionStatus_DeviceLost,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FilterMode {
    Nearest = sys::WGPUFilterMode_Nearest,
    Linear = sys::WGPUFilterMode_Linear,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FrontFace {
    Ccw = sys::WGPUFrontFace_CCW,
    Cw = sys::WGPUFrontFace_CW,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum IndexFormat {
    Uint16 = sys::WGPUIndexFormat_Uint16,
    Uint32 = sys::WGPUIndexFormat_Uint32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum InputStepMode {
    Vertex = sys::WGPUInputStepMode_Vertex,
    Instance = sys::WGPUInputStepMode_Instance,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gpuloadop>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum LoadOp {
    Clear = sys::WGPULoadOp_Clear,
    Load = sys::WGPULoadOp_Load,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum PresentMode {
    Immediate = sys::WGPUPresentMode_Immediate,
    Mailbox = sys::WGPUPresentMode_Mailbox,
    Fifo = sys::WGPUPresentMode_Fifo,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gpuprimitivetopology>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum PrimitiveTopology {
    PointList = sys::WGPUPrimitiveTopology_PointList,
    LineList = sys::WGPUPrimitiveTopology_LineList,
    LineStrip = sys::WGPUPrimitiveTopology_LineStrip,
    TriangleList = sys::WGPUPrimitiveTopology_TriangleList,
    TriangleStrip = sys::WGPUPrimitiveTopology_TriangleStrip,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gpustenciloperation>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum StencilOperation {
    Keep = sys::WGPUStencilOperation_Keep,
    Zero = sys::WGPUStencilOperation_Zero,
    Replace = sys::WGPUStencilOperation_Replace,
    Invert = sys::WGPUStencilOperation_Invert,
    IncrementClamp = sys::WGPUStencilOperation_IncrementClamp,
    DecrementClamp = sys::WGPUStencilOperation_DecrementClamp,
    IncrementWrap = sys::WGPUStencilOperation_IncrementWrap,
    DecrementWrap = sys::WGPUStencilOperation_DecrementWrap,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gpustoreop>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum StoreOp {
    Store = sys::WGPUStoreOp_Store,
    Clear = sys::WGPUStoreOp_Clear,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gputextureaspect>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TextureAspect {
    All = sys::WGPUTextureAspect_All,
    StencilOnly = sys::WGPUTextureAspect_StencilOnly,
    DepthOnly = sys::WGPUTextureAspect_DepthOnly,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gputexturecomponenttype>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TextureComponentType {
    Float = sys::WGPUTextureComponentType_Float,
    Sint = sys::WGPUTextureComponentType_Sint,
    Uint = sys::WGPUTextureComponentType_Uint,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gputexturedimension>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TextureDimension {
    D1 = sys::WGPUTextureDimension_1D,
    D2 = sys::WGPUTextureDimension_2D,
    D3 = sys::WGPUTextureDimension_3D,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gputextureformat>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TextureFormat {
    Undefined = sys::WGPUTextureFormat_Undefined,
    R8Unorm = sys::WGPUTextureFormat_R8Unorm,
    R8Snorm = sys::WGPUTextureFormat_R8Snorm,
    R8Uint = sys::WGPUTextureFormat_R8Uint,
    R8Sint = sys::WGPUTextureFormat_R8Sint,
    R16Uint = sys::WGPUTextureFormat_R16Uint,
    R16Sint = sys::WGPUTextureFormat_R16Sint,
    R16Float = sys::WGPUTextureFormat_R16Float,
    RG8Unorm = sys::WGPUTextureFormat_RG8Unorm,
    RG8Snorm = sys::WGPUTextureFormat_RG8Snorm,
    RG8Uint = sys::WGPUTextureFormat_RG8Uint,
    RG8Sint = sys::WGPUTextureFormat_RG8Sint,
    R32Float = sys::WGPUTextureFormat_R32Float,
    R32Uint = sys::WGPUTextureFormat_R32Uint,
    R32Sint = sys::WGPUTextureFormat_R32Sint,
    RG16Uint = sys::WGPUTextureFormat_RG16Uint,
    RG16Sint = sys::WGPUTextureFormat_RG16Sint,
    RG16Float = sys::WGPUTextureFormat_RG16Float,
    RGBA8Unorm = sys::WGPUTextureFormat_RGBA8Unorm,
    RGBA8UnormSrgb = sys::WGPUTextureFormat_RGBA8UnormSrgb,
    RGBA8Snorm = sys::WGPUTextureFormat_RGBA8Snorm,
    RGBA8Uint = sys::WGPUTextureFormat_RGBA8Uint,
    RGBA8Sint = sys::WGPUTextureFormat_RGBA8Sint,
    BGRA8Unorm = sys::WGPUTextureFormat_BGRA8Unorm,
    BGRA8UnormSrgb = sys::WGPUTextureFormat_BGRA8UnormSrgb,
    RGB10A2Unorm = sys::WGPUTextureFormat_RGB10A2Unorm,
    RG11B10Float = sys::WGPUTextureFormat_RG11B10Float,
    RG32Float = sys::WGPUTextureFormat_RG32Float,
    RG32Uint = sys::WGPUTextureFormat_RG32Uint,
    RG32Sint = sys::WGPUTextureFormat_RG32Sint,
    RGBA16Uint = sys::WGPUTextureFormat_RGBA16Uint,
    RGBA16Sint = sys::WGPUTextureFormat_RGBA16Sint,
    RGBA16Float = sys::WGPUTextureFormat_RGBA16Float,
    RGBA32Float = sys::WGPUTextureFormat_RGBA32Float,
    RGBA32Uint = sys::WGPUTextureFormat_RGBA32Uint,
    RGBA32Sint = sys::WGPUTextureFormat_RGBA32Sint,
    Depth32Float = sys::WGPUTextureFormat_Depth32Float,
    Depth24Plus = sys::WGPUTextureFormat_Depth24Plus,
    Depth24PlusStencil8 = sys::WGPUTextureFormat_Depth24PlusStencil8,
    BC1RGBAUnorm = sys::WGPUTextureFormat_BC1RGBAUnorm,
    BC1RGBAUnormSrgb = sys::WGPUTextureFormat_BC1RGBAUnormSrgb,
    BC2RGBAUnorm = sys::WGPUTextureFormat_BC2RGBAUnorm,
    BC2RGBAUnormSrgb = sys::WGPUTextureFormat_BC2RGBAUnormSrgb,
    BC3RGBAUnorm = sys::WGPUTextureFormat_BC3RGBAUnorm,
    BC3RGBAUnormSrgb = sys::WGPUTextureFormat_BC3RGBAUnormSrgb,
    BC4RUnorm = sys::WGPUTextureFormat_BC4RUnorm,
    BC4RSnorm = sys::WGPUTextureFormat_BC4RSnorm,
    BC5RGUnorm = sys::WGPUTextureFormat_BC5RGUnorm,
    BC5RGSnorm = sys::WGPUTextureFormat_BC5RGSnorm,
    BC6HRGBUfloat = sys::WGPUTextureFormat_BC6HRGBUfloat,
    BC6HRGBSfloat = sys::WGPUTextureFormat_BC6HRGBSfloat,
    BC7RGBAUnorm = sys::WGPUTextureFormat_BC7RGBAUnorm,
    BC7RGBAUnormSrgb = sys::WGPUTextureFormat_BC7RGBAUnormSrgb,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gputextureviewdimension>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TextureViewDimension {
    Undefined = sys::WGPUTextureViewDimension_Undefined,
    D1 = sys::WGPUTextureViewDimension_1D,
    D2 = sys::WGPUTextureViewDimension_2D,
    D2Array = sys::WGPUTextureViewDimension_2DArray,
    Cube = sys::WGPUTextureViewDimension_Cube,
    CubeArray = sys::WGPUTextureViewDimension_CubeArray,
    D3 = sys::WGPUTextureViewDimension_3D,
}

/// <https://gpuweb.github.io/gpuweb/#enumdef-gpuvertexformat>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum VertexFormat {
    UChar2 = sys::WGPUVertexFormat_UChar2,
    UChar4 = sys::WGPUVertexFormat_UChar4,
    Char2 = sys::WGPUVertexFormat_Char2,
    Char4 = sys::WGPUVertexFormat_Char4,
    UChar2Norm = sys::WGPUVertexFormat_UChar2Norm,
    UChar4Norm = sys::WGPUVertexFormat_UChar4Norm,
    Char2Norm = sys::WGPUVertexFormat_Char2Norm,
    Char4Norm = sys::WGPUVertexFormat_Char4Norm,
    UShort2 = sys::WGPUVertexFormat_UShort2,
    UShort4 = sys::WGPUVertexFormat_UShort4,
    Short2 = sys::WGPUVertexFormat_Short2,
    Short4 = sys::WGPUVertexFormat_Short4,
    UShort2Norm = sys::WGPUVertexFormat_UShort2Norm,
    UShort4Norm = sys::WGPUVertexFormat_UShort4Norm,
    Short2Norm = sys::WGPUVertexFormat_Short2Norm,
    Short4Norm = sys::WGPUVertexFormat_Short4Norm,
    Half2 = sys::WGPUVertexFormat_Half2,
    Half4 = sys::WGPUVertexFormat_Half4,
    Float = sys::WGPUVertexFormat_Float,
    Float2 = sys::WGPUVertexFormat_Float2,
    Float3 = sys::WGPUVertexFormat_Float3,
    Float4 = sys::WGPUVertexFormat_Float4,
    UInt = sys::WGPUVertexFormat_UInt,
    UInt2 = sys::WGPUVertexFormat_UInt2,
    UInt3 = sys::WGPUVertexFormat_UInt3,
    UInt4 = sys::WGPUVertexFormat_UInt4,
    Int = sys::WGPUVertexFormat_Int,
    Int2 = sys::WGPUVertexFormat_Int2,
    Int3 = sys::WGPUVertexFormat_Int3,
    Int4 = sys::WGPUVertexFormat_Int4,
}

bitflags! {
    /// <https://gpuweb.github.io/gpuweb/#typedefdef-gpuBufferUsage>
    pub struct BufferUsage: i32 {
        const NONE = sys::WGPUBufferUsage_None;
        const MAP_READ = sys::WGPUBufferUsage_MapRead;
        const MAP_WRITE = sys::WGPUBufferUsage_MapWrite;
        const COPY_SRC = sys::WGPUBufferUsage_CopySrc;
        const COPY_DST = sys::WGPUBufferUsage_CopyDst;
        const INDEX = sys::WGPUBufferUsage_Index;
        const VERTEX = sys::WGPUBufferUsage_Vertex;
        const UNIFORM = sys::WGPUBufferUsage_Uniform;
        const STORAGE = sys::WGPUBufferUsage_Storage;
        const INDIRECT = sys::WGPUBufferUsage_Indirect;
    }
}

bitflags! {
    /// <https://gpuweb.github.io/gpuweb/#typedefdef-gpucolorwriteflags>
    pub struct ColorWrite: i32 {
        const NONE = sys::WGPUColorWriteMask_None;
        const RED = sys::WGPUColorWriteMask_Red;
        const GREEN = sys::WGPUColorWriteMask_Green;
        const BLUE = sys::WGPUColorWriteMask_Blue;
        const ALPHA = sys::WGPUColorWriteMask_Alpha;
        const ALL = sys::WGPUColorWriteMask_All;
    }
}

bitflags! {
    /// <https://gpuweb.github.io/gpuweb/#typedefdef-gpuShaderStage>
    pub struct ShaderStage: i32 {
        const NONE = sys::WGPUShaderStage_None;
        const VERTEX = sys::WGPUShaderStage_Vertex;
        const FRAGMENT = sys::WGPUShaderStage_Fragment;
        const COMPUTE = sys::WGPUShaderStage_Compute;
    }
}

bitflags! {
    /// <https://gpuweb.github.io/gpuweb/#typedefdef-gpuTextureUsage>
    pub struct TextureUsage: i32 {
        const NONE = sys::WGPUTextureUsage_None;
        const COPY_SRC = sys::WGPUTextureUsage_CopySrc;
        const COPY_DST = sys::WGPUTextureUsage_CopyDst;
        const SAMPLED = sys::WGPUTextureUsage_Sampled;
        const STORAGE = sys::WGPUTextureUsage_Storage;
        const OUTPUT_ATTACHMENT = sys::WGPUTextureUsage_OutputAttachment;
        #[doc(hidden)] // internal?
        const PRESENT = sys::WGPUTextureUsage_Present;
    }
}

// #[repr(i32)]
// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
// enum SType {
//     Invalid = sys::WGPUSType_Invalid,
//     SurfaceDescriptorFromMetalLayer = sys::WGPUSType_SurfaceDescriptorFromMetalLayer,
//     SurfaceDescriptorFromWindowsHWND = sys::WGPUSType_SurfaceDescriptorFromWindowsHWND,
//     SurfaceDescriptorFromXlib = sys::WGPUSType_SurfaceDescriptorFromXlib,
//     SurfaceDescriptorFromHTMLCanvasId = sys::WGPUSType_SurfaceDescriptorFromHTMLCanvasId,
// }

// #[repr(C)]
// #[derive(Debug, Copy, Clone)]
// pub struct ChainedStruct {
//     next: *const ChainedStruct,
//     sType: SType,
// }
//
// impl<'a> Default for ChainedStruct<'a> {
//     fn default() -> ChainedStruct<'a> {
//         unsafe {
//             mem::zeroed()
//         }
//     }
// }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AdapterProperties {
    pub name: String,
    pub adapter_type: AdapterType,
    pub backend_type: BackendType,
    pub vendor_id: u32,
    pub device_id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceDescriptor<'a> {
    pub required_extensions: Option<&'a [&'a str]>,
    pub force_enabled_toggles: Option<&'a [&'a str]>,
    pub force_disabled_toggles: Option<&'a [&'a str]>,
}

// #[derive(Debug, Copy, Clone)]
// pub struct SwapChainDescriptor {
//     pub usage: TextureUsage,
//     pub format: TextureFormat,
//     pub width: u32,
//     pub height: u32,
//     pub present_mode: PresentMode,
// }

/// <https://gpuweb.github.io/gpuweb/#dictdef-gpubufferbinding>
#[repr(C)]
#[derive(Debug, Clone)]
pub struct BufferBinding<'a> {
    pub buffer: &'a Buffer,
    pub offset: u64,
    pub size: u64,
}

/// https://gpuweb.github.io/gpuweb/#typedefdef-gpubindingresource
#[repr(C)]
#[derive(Debug, Clone)]
pub enum BindingResource<'a> {
    Sampler(&'a Sampler),
    TextureView(&'a TextureView),
    BufferBinding(BufferBinding<'a>),
}

/// <https://gpuweb.github.io/gpuweb/#dictdef-gpubindgroupentry>
/// GPUBindGroupBinding
#[repr(C)]
#[derive(Debug, Clone)]
pub struct BindGroupEntry<'a> {
    pub binding: u32,
    pub resource: BindingResource<'a>,
}

/// <https://gpuweb.github.io/gpuweb/#dictdef-gpubindgrouplayoutentry>
/// BindGroupLayoutBinding
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct BindGroupLayoutEntry {
    pub binding: u32,
    pub visibility: ShaderStage,
    pub ty: BindingType,
    // pub has_dynamic_offset: bool,
    // pub multisampled: bool,
    // pub texture_dimension: TextureViewDimension,
    // pub texture_component_type: TextureComponentType,
    // pub storage_texture_format: TextureFormat,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct BlendDescriptor {
    pub operation: BlendOperation,
    pub src_factor: BlendFactor,
    pub dst_factor: BlendFactor,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct BufferCopyView<'a> {
    pub buffer: &'a Buffer,
    pub offset: u64,
    pub bytes_per_row: u32,
    pub rows_per_image: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct BufferDescriptor<'a> {
    pub label: Option<&'a str>,
    pub usage: BufferUsage,
    pub size: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct CommandBufferDescriptor<'a> {
    pub label: Option<&'a str>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct CommandEncoderDescriptor<'a> {
    pub label: Option<&'a str>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ComputePassDescriptor<'a> {
    pub label: Option<&'a str>,
}

#[repr(C)]
#[derive(Debug)]
pub struct CreateBufferMapped<'a> {
    buffer: Buffer,
    pub data: &'a mut [u8],
}

impl<'a> CreateBufferMapped<'a> {
    pub fn finish(self) -> Buffer {
        let buffer = self.buffer.clone();
        drop(self);
        buffer
    }
}

impl<'a> Drop for CreateBufferMapped<'a> {
    fn drop(&mut self) {
        unsafe {
            if !self.buffer.raw.is_null() {
                sys::wgpuBufferUnmap(self.buffer.raw);
            }
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct DeviceProperties {
    pub texture_compression_bc: bool,
}

pub type Extensions = DeviceProperties;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Extent3d {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FenceDescriptor<'a> {
    // pub next_in_chain: *const ChainedStruct,
    // pub label: *const libc::c_char,
    pub initial_value: u64,
    pub label: Option<&'a str>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct InstanceDescriptor {
    // pub next_in_chain: *const ChainedStruct,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Origin3d {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct PipelineLayoutDescriptor<'a> {
    // pub next_in_chain: *const ChainedStruct,
    // pub label: *const libc::c_char,
    pub label: Option<&'a str>,
    // pub bind_group_layout_count: u32,
    pub bind_group_layouts: &'a [BindGroupLayout],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ProgrammableStageDescriptor<'a> {
    // pub next_in_chain: *const ChainedStruct,
    pub module: &'a ShaderModule,
    pub entry_point: &'a str,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RasterizationStateDescriptor {
    // pub next_in_chain: *const ChainedStruct,
    pub front_face: FrontFace,
    pub cull_mode: CullMode,
    pub depth_bias: i32,
    pub depth_bias_slope_scale: f32,
    pub depth_bias_clamp: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RenderBundleDescriptor<'a> {
    // pub next_in_chain: *const ChainedStruct,
    // pub label: *const libc::c_char,
    pub label: Option<&'a str>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RenderBundleEncoderDescriptor<'a> {
    // pub next_in_chain: *const ChainedStruct,
    // pub label: *const libc::c_char,
    pub label: Option<&'a str>,
    // pub color_formats_count: u32,
    pub color_formats: &'a [TextureFormat],
    pub depth_stencil_format: TextureFormat,
    pub sample_count: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RenderPassDepthStencilAttachmentDescriptor<'a> {
    pub attachment: &'a TextureView,
    pub depth_load_op: LoadOp,
    pub depth_store_op: StoreOp,
    pub clear_depth: f32,
    pub stencil_load_op: LoadOp,
    pub stencil_store_op: StoreOp,
    pub clear_stencil: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SamplerDescriptor<'a> {
    // pub next_in_chain: *const ChainedStruct,
    // pub label: *const libc::c_char,
    pub label: Option<&'a str>,
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub address_mode_w: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: FilterMode,
    pub lod_min_clamp: f32,
    pub lod_max_clamp: f32,
    pub compare: CompareFunction,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ShaderModuleDescriptor<'a> {
    // pub next_in_chain: *const ChainedStruct,
    // pub label: *const libc::c_char,
    pub label: Option<&'a str>,
    //pub codeSize: u32,
    pub code: &'a [u32],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct StencilStateFaceDescriptor {
    pub compare: CompareFunction,
    pub fail_op: StencilOperation,
    pub depth_fail_op: StencilOperation,
    pub pass_op: StencilOperation,
}

// #[repr(C)]
// #[derive(Debug, Copy, Clone)]
// pub struct SurfaceDescriptor {
//     // pub next_in_chain: *const ChainedStruct,
//     // pub label: *const libc::c_char,
// }
// #[repr(C)]
// #[derive(Debug, Copy, Clone)]
// pub struct SurfaceDescriptorFromHTMLCanvasId {
//     pub chain: ChainedStruct,
//     pub id: *const libc::c_char,
// }
// #[repr(C)]
// #[derive(Debug, Copy, Clone)]
// pub struct SurfaceDescriptorFromMetalLayer {
//     pub chain: ChainedStruct,
//     pub layer: *mut libc::c_void,
// }
// #[repr(C)]
// #[derive(Debug, Copy, Clone)]
// pub struct SurfaceDescriptorFromWindowsHWND {
//     pub chain: ChainedStruct,
//     pub hinstance: *mut libc::c_void,
//     pub hwnd: *mut libc::c_void,
// }
// #[repr(C)]
// #[derive(Debug, Copy, Clone)]
// pub struct SurfaceDescriptorFromXlib {
//     pub chain: ChainedStruct,
//     pub display: *mut libc::c_void,
//     pub window: u32,
// }

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SwapChainDescriptor<'a> {
    pub label: Option<&'a str>,
    pub usage: TextureUsage,
    pub format: TextureFormat,
    pub width: u32,
    pub height: u32,
    pub present_mode: PresentMode,
    pub implementation: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TextureViewDescriptor<'a> {
    pub label: Option<&'a str>,
    pub format: TextureFormat,
    pub dimension: TextureViewDimension,
    pub base_mip_level: u32,
    pub mip_level_count: u32,
    pub base_array_layer: u32,
    pub array_layer_count: u32,
    pub aspect: TextureAspect,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct VertexAttributeDescriptor {
    pub format: VertexFormat,
    pub offset: u64,
    pub shader_location: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct BindGroupDescriptor<'a> {
    pub label: Option<&'a str>,
    pub layout: &'a BindGroupLayout,
    pub entries: &'a [BindGroupEntry<'a>],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct BindGroupLayoutDescriptor<'a> {
    pub label: Option<&'a str>,
    pub entries: &'a [BindGroupLayoutEntry],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ColorStateDescriptor {
    pub format: TextureFormat,
    pub alpha_blend: BlendDescriptor,
    pub color_blend: BlendDescriptor,
    pub write_mask: ColorWrite,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ComputePipelineDescriptor<'a> {
    pub label: Option<&'a str>,
    pub layout: &'a PipelineLayout,
    pub compute_stage: ProgrammableStageDescriptor<'a>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct DepthStencilStateDescriptor {
    pub format: TextureFormat,
    pub depth_write_enabled: bool,
    pub depth_compare: CompareFunction,
    pub stencil_front: StencilStateFaceDescriptor,
    pub stencil_back: StencilStateFaceDescriptor,
    pub stencil_read_mask: u32,
    pub stencil_write_mask: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RenderPassColorAttachmentDescriptor<'a> {
    pub attachment: &'a TextureView,
    pub resolve_target: Option<&'a TextureView>,
    pub load_op: LoadOp,
    pub store_op: StoreOp,
    pub clear_color: Color,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TextureCopyView<'a> {
    pub texture: &'a Texture,
    pub mip_level: u32,
    pub array_layer: u32,
    pub origin: Origin3d,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TextureDescriptor<'a> {
    pub label: Option<&'a str>,
    pub usage: TextureUsage,
    pub dimension: TextureDimension,
    pub size: Extent3d,
    pub array_layer_count: u32,
    pub format: TextureFormat,
    pub mip_level_count: u32,
    pub sample_count: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct VertexBufferLayoutDescriptor<'a> {
    pub array_stride: u64,
    pub step_mode: InputStepMode,
    pub attributes: &'a [VertexAttributeDescriptor],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RenderPassDescriptor<'a> {
    pub label: Option<&'a str>,
    pub color_attachments: &'a [RenderPassColorAttachmentDescriptor<'a>],
    pub depth_stencil_attachment: Option<&'a RenderPassDepthStencilAttachmentDescriptor<'a>>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct VertexStateDescriptor<'a> {
    pub index_format: IndexFormat,
    pub vertex_buffers: &'a [VertexBufferLayoutDescriptor<'a>],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RenderPipelineDescriptor<'a> {
    pub label: Option<&'a str>,
    pub layout: &'a PipelineLayout,
    pub vertex_stage: ProgrammableStageDescriptor<'a>,
    pub fragment_stage: Option<ProgrammableStageDescriptor<'a>>,
    pub vertex_state: &'a VertexStateDescriptor<'a>,
    pub primitive_topology: PrimitiveTopology,
    pub rasterization_state: Option<&'a RasterizationStateDescriptor>,
    pub sample_count: u32,
    pub depth_stencil_state: Option<&'a DepthStencilStateDescriptor>,
    pub color_states: &'a [ColorStateDescriptor],
    pub sample_mask: u32,
    pub alpha_to_coverage_enabled: bool,
}

unsafe impl Send for Instance {}

unsafe impl Sync for Instance {}

impl Clone for Instance {
    fn clone(&self) -> Instance {
        unsafe {
            sys::wgpuInstanceReference(self.raw);
        }
        Instance { raw: self.raw }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            sys::wgpuInstanceRelease(self.raw);
        }
    }
}

fn init_procs() {
    INIT.call_once(|| unsafe {
        sys::dawn_native__GetProcs(PROC_TABLE.as_mut_ptr());
        sys::dawnProcSetProcs(PROC_TABLE.as_ptr());
    });
}

impl Instance {
    pub fn new() -> Instance {
        unsafe {
            init_procs();
            let descriptor = mem::zeroed();
            let raw = sys::wgpuCreateInstance(&descriptor);
            debug_assert_ne!(ptr::null_mut(), raw);
            Instance { raw }
        }
    }
}

impl Instance {
    pub fn enumerate_adapters(&self) -> Vec<Adapter> {
        unsafe {
            sys::dawn_native__Instance__DiscoverDefaultAdapters(self.raw);
            let count = sys::dawn_native__Instance__GetAdaptersCount(self.raw);
            (0..count)
                .map(|adapter_index| Adapter::from_raw(self.raw, adapter_index))
                .collect()
        }
    }

    pub fn create_surface<W: HasRawWindowHandle>(&self, window: &W) -> Surface {
        let raw_window_handle = window.raw_window_handle();

        unsafe {
            let mut raw_descriptor: sys::WGPUSurfaceDescriptor = mem::zeroed();
            let mut win32: sys::WGPUSurfaceDescriptorFromWindowsHWND = mem::zeroed();
            win32.chain.sType = sys::WGPUSType_SurfaceDescriptorFromWindowsHWND;

            #[allow(unused)]
            let mut xlib: sys::WGPUSurfaceDescriptorFromXlib = mem::zeroed();
            xlib.chain.sType = sys::WGPUSType_SurfaceDescriptorFromXlib;

            #[allow(unused)]
            let mut metal: sys::WGPUSurfaceDescriptorFromMetalLayer = mem::zeroed();
            metal.chain.sType = sys::WGPUSType_SurfaceDescriptorFromMetalLayer;

            match raw_window_handle {
                #[cfg(windows)]
                RawWindowHandle::Windows(handle) => {
                    win32.hinstance = handle.hinstance as _;
                    win32.hwnd = handle.hwnd as _;
                    raw_descriptor.nextInChain = &mut win32 as *mut _ as *const _;
                }
                #[cfg(any(
                    target_os = "linux",
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "netbsd",
                    target_os = "openbsd"
                ))]
                RawWindowHandle::Xlib(handle) => {
                    xlib.window = handle.window as _;
                    xlib.display = handle.display as _;
                    raw_descriptor.next_in_chain = &mut xlib as *mut _ as _;
                }
                #[cfg(target_os = "macos")]
                RawWindowHandle::MacOS(handle) => {
                    panic!("TODO: Metal (macOS)");
                    raw_descriptor.next_in_chain = &mut metal as *mut _ as *const _;
                }
                #[cfg(target_os = "ios")]
                RawWindowHandle::IOS(handle) => {
                    panic!("TODO: Metal (iOS)");
                    raw_descriptor.next_in_chain = &mut metal as *mut _ as *const _;
                }
                _ => {
                    panic!("unsupported platform: {:?}", raw_window_handle);
                }
            }

            let raw = sys::wgpuInstanceCreateSurface(self.raw, &raw_descriptor);
            debug_assert_ne!(ptr::null_mut(), raw);
            Surface {
                raw,
                instance: self.clone(),
            }
        }
    }
}

impl Adapter {
    pub fn properties(&self) -> AdapterProperties {
        unsafe {
            use std::ffi::CStr;
            let mut raw: sys::WGPUAdapterProperties = mem::zeroed();
            sys::dawn_native__Adapter__GetProperties(self.instance, self.adapter_index, &mut raw);
            AdapterProperties {
                name: CStr::from_ptr(raw.name).to_string_lossy().to_string(),
                vendor_id: raw.vendorID,
                device_id: raw.deviceID,
                adapter_type: convert::adapter_type(raw.adapterType),
                backend_type: convert::backend_type(raw.backendType),
            }
        }
    }

    pub fn extensions(&self) -> Extensions {
        unsafe {
            let raw =
                sys::dawn_native__Adapter__GetAdapterProperties(self.instance, self.adapter_index);

            DeviceProperties {
                texture_compression_bc: raw.textureCompressionBC,
            }
        }
    }

    pub fn create_device(&self, descriptor: &DeviceDescriptor) -> Device {
        use std::ffi::CString;

        let required_extensions: Vec<_> = descriptor
            .required_extensions
            .unwrap_or(&[])
            .iter()
            .map(|v| CString::new(v.as_bytes().to_vec()).unwrap())
            .collect();
        let raw_required_extensions: Vec<_> =
            required_extensions.iter().map(|s| s.as_ptr()).collect();

        let force_enabled_toggles: Vec<_> = descriptor
            .required_extensions
            .unwrap_or(&[])
            .iter()
            .map(|v| CString::new(v.as_bytes().to_vec()).unwrap())
            .collect();
        let raw_force_enabled_toggles: Vec<_> =
            force_enabled_toggles.iter().map(|s| s.as_ptr()).collect();

        let force_disabled_toggles: Vec<_> = descriptor
            .required_extensions
            .unwrap_or(&[])
            .iter()
            .map(|v| CString::new(v.as_bytes().to_vec()).unwrap())
            .collect();
        let raw_force_disabled_toggles: Vec<_> =
            force_disabled_toggles.iter().map(|s| s.as_ptr()).collect();

        unsafe {
            let raw_descriptor = sys::DeviceDescriptor {
                requiredExtensions: raw_required_extensions.as_ptr(),
                requiredExtensionsCount: raw_required_extensions.len(),
                forceEnabledToggles: raw_force_enabled_toggles.as_ptr(),
                forceEnabledTogglesCount: raw_force_enabled_toggles.len(),
                forceDisabledToggles: raw_force_disabled_toggles.as_ptr(),
                forceDisabledTogglesCount: raw_force_disabled_toggles.len(),
            };
            let raw = sys::dawn_native__Adapter__CreateDevice(
                self.instance,
                self.adapter_index,
                &raw_descriptor,
            );
            debug_assert_ne!(
                ptr::null_mut(),
                raw,
                "dawn_native__Adapter__CreateDevice failed"
            );
            let adapter = self.clone();
            let raw_default_queue = sys::wgpuDeviceCreateQueue(raw);
            debug_assert_ne!(ptr::null_mut(), raw_default_queue);
            let backend_type = self.properties().backend_type;
            let inner = DeviceInner {
                raw,
                raw_default_queue,
                adapter,
                backend_type,
            };
            Device {
                inner: Arc::new(Mutex::new(inner)),
            }
        }
    }
}

pub trait ErrorCallback {
    fn error(message: &str, error_type: ErrorType, userdata: *mut libc::c_void);
}

impl Device {
    pub fn raw(&self) -> sys::WGPUDevice {
        self.inner.lock().raw
    }

    pub fn set_error_callback<F: ErrorCallback>(&self) {
        extern "C" fn native_callback<F: ErrorCallback>(
            error_type: sys::WGPUErrorType,
            message: *const libc::c_char,
            userdata: *mut libc::c_void,
        ) {
            let message = unsafe { std::ffi::CStr::from_ptr(message).to_string_lossy() };
            let error_type: ErrorType = unsafe { mem::transmute(error_type) };
            F::error(&message, error_type, userdata);
        }

        unsafe {
            init_procs();
            (*PROC_TABLE.as_ptr())
                .deviceSetUncapturedErrorCallback
                .unwrap()(
                self.inner.lock().raw,
                Some(native_callback::<F>),
                ptr::null_mut(),
            );
        }
    }

    pub fn default_queue(&self) -> Queue {
        unsafe {
            let guard = self.inner.lock();
            sys::wgpuQueueReference(guard.raw_default_queue);
            Queue {
                raw: guard.raw_default_queue,
                device: self.clone(),
                temp_commands: Vec::new(),
            }
        }
    }

    pub fn create_swap_chain(
        &self,
        surface: Option<&Surface>,
        descriptor: &SwapChainDescriptor,
    ) -> SwapChain {
        let label = convert::label(descriptor.label);
        unsafe {
            let raw_descriptor = sys::WGPUSwapChainDescriptor {
                nextInChain: ptr::null_mut(),
                label: label.as_ptr(),
                usage: descriptor.usage.bits as _,
                format: descriptor.format as _,
                width: descriptor.width,
                height: descriptor.height,
                presentMode: descriptor.present_mode as _,
                implementation: descriptor.implementation,
            };
            let surface_raw = surface
                .map(|surface| surface.raw)
                .unwrap_or_else(ptr::null_mut);
            let guard = self.inner.lock();
            let backend_type = guard.backend_type;
            let raw = sys::wgpuDeviceCreateSwapChain(guard.raw, surface_raw, &raw_descriptor);
            drop(guard);
            debug_assert_ne!(ptr::null_mut(), raw);
            let inner = SwapChainInner {
                raw,
                device: self.clone(),
            };
            SwapChain {
                inner,
                backend_type,
                dawn_swap_chain_impl: None,
            }
        }
    }

    pub fn create_bind_group(&self, descriptor: &BindGroupDescriptor) -> BindGroup {
        let label = convert::label(descriptor.label);
        let mut raw_entries =
            SmallVec::<[sys::WGPUBindGroupBinding; DEFAULT_MAX_BIND_GROUPS]>::new();
        for entry in descriptor.entries.iter() {
            raw_entries.push(sys::WGPUBindGroupBinding {
                binding: entry.binding,
                buffer: match entry.resource {
                    BindingResource::BufferBinding(ref binding) => binding.buffer.raw,
                    _ => ptr::null_mut(),
                },
                offset: match entry.resource {
                    BindingResource::BufferBinding(ref binding) => binding.offset,
                    _ => 0,
                },
                size: match entry.resource {
                    BindingResource::BufferBinding(ref binding) => binding.size,
                    _ => 0,
                },
                sampler: match entry.resource {
                    BindingResource::Sampler(sampler) => sampler.raw,
                    _ => ptr::null_mut(),
                },
                textureView: match entry.resource {
                    BindingResource::TextureView(texture_view) => texture_view.raw,
                    _ => ptr::null_mut(),
                },
            });
        }
        let raw_descriptor = sys::WGPUBindGroupDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            layout: descriptor.layout.raw,
            bindingCount: raw_entries.len() as _,
            bindings: raw_entries.as_ptr(),
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateBindGroup(guard.raw, &raw_descriptor) };
        drop(guard);
        BindGroup {
            raw,
            device: self.clone(),
        }
    }

    // pub fn create_bind_group_layout(&self, descriptor: &BindGroupLayoutDescriptor) -> BindGroupLayout {
    //     let label = convert::label(descriptor.label);
    //     let mut entries = SmallVec::<[sys::WGPUBindGroupLayoutBinding; DEFAULT_MAX_BIND_GROUPS]>::new();
    //     for entry in descriptor.entries.iter() {
    //         entries.push(sys::WGPUBindGroupLayoutBinding {
    //             binding: entry.binding,
    //             visibility: entry.visibility.bits as _,
    //             type_: match entry.type_ {
    //                 BindingType::UniformBuffer { .. }  => sys::WGPUBindingType_UniformBuffer,
    //                 BindingType::StorageBuffer { .. } => sys::WGPUBindingType_StorageBuffer,
    //                 BindingType::ReadonlyStorageBuffer { .. } => sys::WGPUBindingType_ReadonlyStorageBuffer,
    //                 BindingType::Sampler { .. } => sys::WGPUBindingType_Sampler,
    //                 BindingType::SampledTexture { .. } => sys::WGPUBindingType_SampledTexture,
    //                 BindingType::StorageTexture { .. } => sys::WGPUBindingType_StorageTexture,
    //                 BindingType::ReadonlyStorageTexture { .. } => sys::WGPUBindingType_ReadonlyStorageTexture,
    //                 BindingType::WriteonlyStorageTexture { .. } => sys::WGPUBindingType_WriteonlyStorageTexture,
    //             },
    //             hasDynamicOffset: match entry.type_ {
    //                 BindingType::UniformBuffer { dynamic, .. }  => dynamic,
    //                 BindingType::StorageBuffer { dynamic, .. } => dynamic,
    //                 BindingType::ReadonlyStorageBuffer { dynamic, .. } => dynamic,
    //                 _ => false,
    //             },
    //             multisampled: match entry.type_ {
    //                 BindingType::SampledTexture { multisampled, .. }  => multisampled,
    //                 _ => false,
    //             },
    //             textureDimension: match entry.type_ {
    //                 BindingType::SampledTexture { dimension, .. }  => dimension as _,
    //                 BindingType::StorageTexture { dimension, .. } => dimension as _,
    //                 BindingType::ReadonlyStorageTexture { dimension, .. } => dimension as _,
    //                 BindingType::WriteonlyStorageTexture { dimension, .. } => dimension as _,
    //                 _ => TextureViewDimension::D1 as _,
    //             },
    //             textureComponentType: match entry.type_ {
    //                 BindingType::SampledTexture { component_type, .. }  => component_type as _,
    //                 BindingType::StorageTexture { component_type, .. } => component_type as _,
    //                 BindingType::ReadonlyStorageTexture { component_type, .. } => component_type as _,
    //                 BindingType::WriteonlyStorageTexture { component_type, .. } => component_type as _,
    //                 _ => TextureComponentType::Float as _,
    //             },
    //             storageTextureFormat: match entry.type_ {
    //                 BindingType::StorageTexture { format, .. } => format as _,
    //                 BindingType::ReadonlyStorageTexture { format, .. } => format as _,
    //                 BindingType::WriteonlyStorageTexture { format, .. } => format as _,
    //                 _ => TextureFormat::R8Sint as _,
    //             }
    //         });
    //     }
    //     let raw_descriptor = sys::WGPUBindGroupLayoutDescriptor {
    //         nextInChain: ptr::null_mut(),
    //         label: label.as_ptr(),
    //         bindingCount: descriptor.entries.len() as _,
    //         bindings: entries.as_ptr(),
    //     };
    //     let inner = self.inner.lock();
    //     let raw = unsafe { sys::wgpuDeviceCreateBindGroupLayout(inner.raw, &raw_descriptor) };
    //     BindGroupLayout {
    //         raw,
    //         device: self.clone(),
    //     }
    // }

    pub fn create_bind_group_layout(
        &self,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayout {
        let label = convert::label(descriptor.label);
        let mut raw_entries =
            SmallVec::<[sys::WGPUBindGroupLayoutBinding; DEFAULT_MAX_BIND_GROUPS]>::new();
        for entry in descriptor.entries.iter() {
            raw_entries.push(sys::WGPUBindGroupLayoutBinding {
                binding: entry.binding,
                visibility: entry.visibility.bits as _,
                type_: match entry.ty {
                    BindingType::UniformBuffer { .. } => sys::WGPUBindingType_UniformBuffer,
                    BindingType::StorageBuffer { readonly, .. } => match readonly {
                        false => sys::WGPUBindingType_StorageBuffer,
                        true => sys::WGPUBindingType_ReadonlyStorageBuffer,
                    },
                    BindingType::Sampler { .. } => sys::WGPUBindingType_Sampler,
                    BindingType::SampledTexture { .. } => sys::WGPUBindingType_SampledTexture,
                    BindingType::StorageTexture {
                        readonly,
                        writeonly,
                        ..
                    } => {
                        match (readonly, writeonly) {
                            (false, false) => sys::WGPUBindingType_StorageTexture,
                            (true, false) => sys::WGPUBindingType_ReadonlyStorageTexture,
                            (false, true) => sys::WGPUBindingType_WriteonlyStorageTexture,
                            // FIXME: setting both readonly and writeonly doesn't make sense
                            //        and this possibility is a result of combining these types
                            (true, true) => sys::WGPUBindingType_StorageTexture,
                        }
                    }
                },
                hasDynamicOffset: match entry.ty {
                    BindingType::UniformBuffer { dynamic, .. } => dynamic,
                    BindingType::StorageBuffer { dynamic, .. } => dynamic,
                    _ => false,
                },
                multisampled: match entry.ty {
                    BindingType::SampledTexture { multisampled, .. } => multisampled,
                    _ => false,
                },
                textureDimension: match entry.ty {
                    BindingType::SampledTexture { dimension, .. } => dimension as _,
                    BindingType::StorageTexture { dimension, .. } => dimension as _,
                    _ => TextureViewDimension::D1 as _,
                },
                textureComponentType: match entry.ty {
                    BindingType::SampledTexture { component_type, .. } => component_type as _,
                    BindingType::StorageTexture { component_type, .. } => component_type as _,
                    _ => TextureComponentType::Float as _,
                },
                storageTextureFormat: match entry.ty {
                    BindingType::StorageTexture { format, .. } => format as _,
                    _ => TextureFormat::R8Sint as _,
                },
            });
        }
        let raw_descriptor = sys::WGPUBindGroupLayoutDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            bindingCount: raw_entries.len() as _,
            bindings: raw_entries.as_ptr(),
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateBindGroupLayout(guard.raw, &raw_descriptor) };
        drop(guard);
        BindGroupLayout {
            raw,
            device: self.clone(),
        }
    }

    pub fn create_buffer(&self, descriptor: &BufferDescriptor) -> Buffer {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUBufferDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            size: descriptor.size,
            usage: descriptor.usage.bits as _,
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateBuffer(guard.raw, &raw_descriptor) };
        drop(guard);
        Buffer {
            raw,
            device: self.clone(),
        }
    }

    pub fn create_buffer_with_size(&self, size: usize, usage: BufferUsage) -> Buffer {
        let size = size as _;
        self.create_buffer(&BufferDescriptor {
            label: None,
            size,
            usage,
        })
    }

    pub fn create_buffer_with_data(&self, data: &[u8], usage: BufferUsage) -> Buffer {
        let size = data.len() as _;
        let mapped = self.create_buffer_mapped(&BufferDescriptor {
            label: None,
            size,
            usage: usage | BufferUsage::MAP_WRITE,
        });
        mapped.data.copy_from_slice(data);
        mapped.finish()
    }

    pub fn create_buffer_mapped(&self, descriptor: &BufferDescriptor) -> CreateBufferMapped {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUBufferDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            size: descriptor.size,
            usage: descriptor.usage.bits as _,
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateBufferMapped(guard.raw, &raw_descriptor) };
        drop(guard);
        let data: &mut [u8] =
            unsafe { slice::from_raw_parts_mut(raw.data as _, raw.dataLength.try_into().unwrap()) };
        let buffer = Buffer {
            raw: raw.buffer,
            device: self.clone(),
        };
        CreateBufferMapped { buffer, data }
    }

    pub fn create_buffer_mapped_with_size(
        &self,
        size: usize,
        usage: BufferUsage,
    ) -> CreateBufferMapped {
        let size = size as _;
        let descriptor = BufferDescriptor {
            label: None,
            size,
            usage,
        };
        self.create_buffer_mapped(&descriptor)
    }

    pub fn create_command_encoder(&self, descriptor: &CommandEncoderDescriptor) -> CommandEncoder {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUCommandEncoderDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateCommandEncoder(guard.raw, &raw_descriptor) };
        drop(guard);
        CommandEncoder {
            raw,
            device: self.clone(),
        }
    }

    pub fn create_pipeline_layout(&self, descriptor: &PipelineLayoutDescriptor) -> PipelineLayout {
        let label = convert::label(descriptor.label);
        let mut raw_bind_group_layouts =
            SmallVec::<[sys::WGPUBindGroupLayout; DEFAULT_MAX_BIND_GROUPS]>::new();
        for bind_group_layout in descriptor.bind_group_layouts {
            raw_bind_group_layouts.push(bind_group_layout.raw);
        }
        let raw_descriptor = sys::WGPUPipelineLayoutDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            bindGroupLayoutCount: raw_bind_group_layouts.len() as _,
            bindGroupLayouts: raw_bind_group_layouts.as_ptr(),
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreatePipelineLayout(guard.raw, &raw_descriptor) };
        drop(guard);
        PipelineLayout {
            raw,
            device: self.clone(),
        }
    }

    pub fn create_compute_pipeline(
        &self,
        descriptor: &ComputePipelineDescriptor,
    ) -> ComputePipeline {
        let label = convert::label(descriptor.label);
        let entry_point = convert::label(Some(descriptor.compute_stage.entry_point));
        let raw_descriptor = sys::WGPUComputePipelineDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            layout: descriptor.layout.raw,
            computeStage: sys::WGPUProgrammableStageDescriptor {
                nextInChain: ptr::null_mut(),
                module: descriptor.compute_stage.module.raw,
                entryPoint: entry_point.as_ptr(),
            },
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateComputePipeline(guard.raw, &raw_descriptor) };
        drop(guard);
        ComputePipeline {
            raw,
            device: self.clone(),
        }
    }

    pub fn create_render_pipeline(&self, descriptor: &RenderPipelineDescriptor) -> RenderPipeline {
        let label = convert::label(descriptor.label);
        let vertex_entry_point = convert::label(Some(descriptor.vertex_stage.entry_point));

        let mut fragment_entry_point = None;
        let fragment_stage = descriptor.fragment_stage.map(|stage| {
            fragment_entry_point = Some(convert::label(Some(stage.entry_point)));
            sys::WGPUProgrammableStageDescriptor {
                nextInChain: ptr::null_mut(),
                module: stage.module.raw,
                entryPoint: fragment_entry_point.as_ref().unwrap().as_ptr(),
            }
        });
        let raw_fragment_stage = fragment_stage
            .as_ref()
            .map(|stage| stage as _)
            .unwrap_or_else(ptr::null);

        let rasterization_state =
            descriptor
                .rasterization_state
                .map(|state| sys::WGPURasterizationStateDescriptor {
                    nextInChain: ptr::null_mut(),
                    frontFace: state.front_face as _,
                    cullMode: state.cull_mode as _,
                    depthBias: state.depth_bias,
                    depthBiasSlopeScale: state.depth_bias_slope_scale,
                    depthBiasClamp: state.depth_bias_clamp,
                });
        let raw_rasterization_state = rasterization_state
            .as_ref()
            .map(|state| state as _)
            .unwrap_or_else(ptr::null);

        let depth_stencil_state =
            descriptor
                .depth_stencil_state
                .map(|state| sys::WGPUDepthStencilStateDescriptor {
                    nextInChain: ptr::null_mut(),
                    format: state.format as _,
                    depthWriteEnabled: state.depth_write_enabled,
                    depthCompare: state.depth_compare as _,
                    stencilFront: sys::WGPUStencilStateFaceDescriptor {
                        compare: state.stencil_front.compare as _,
                        failOp: state.stencil_front.fail_op as _,
                        depthFailOp: state.stencil_front.depth_fail_op as _,
                        passOp: state.stencil_front.pass_op as _,
                    },
                    stencilBack: sys::WGPUStencilStateFaceDescriptor {
                        compare: state.stencil_back.compare as _,
                        failOp: state.stencil_back.fail_op as _,
                        depthFailOp: state.stencil_back.depth_fail_op as _,
                        passOp: state.stencil_back.pass_op as _,
                    },
                    stencilReadMask: state.stencil_read_mask as _,
                    stencilWriteMask: state.stencil_write_mask as _,
                });
        let raw_depth_stencil_state = depth_stencil_state
            .as_ref()
            .map(|state| state as _)
            .unwrap_or_else(ptr::null);

        let mut raw_color_states = Vec::with_capacity(descriptor.color_states.len());
        for color_state in descriptor.color_states.iter() {
            raw_color_states.push(sys::WGPUColorStateDescriptor {
                nextInChain: ptr::null_mut(),
                format: color_state.format as _,
                alphaBlend: sys::WGPUBlendDescriptor {
                    operation: color_state.alpha_blend.operation as _,
                    srcFactor: color_state.alpha_blend.src_factor as _,
                    dstFactor: color_state.alpha_blend.dst_factor as _,
                },
                colorBlend: sys::WGPUBlendDescriptor {
                    operation: color_state.color_blend.operation as _,
                    srcFactor: color_state.color_blend.src_factor as _,
                    dstFactor: color_state.color_blend.dst_factor as _,
                },
                writeMask: color_state.write_mask.bits as _,
            });
        }

        let attributes_count = 1 + descriptor
            .vertex_state
            .vertex_buffers
            .iter()
            .fold(0, |count, vertex_buffer| {
                count + vertex_buffer.attributes.len()
            });

        let mut vertex_attributes = Vec::with_capacity(attributes_count);

        let mut raw_vertex_buffers =
            Vec::with_capacity(descriptor.vertex_state.vertex_buffers.len());

        for vertex_buffer in descriptor.vertex_state.vertex_buffers.iter() {
            let attributes_offset = vertex_attributes.len();
            for attribute in vertex_buffer.attributes.iter() {
                vertex_attributes.push(sys::WGPUVertexAttributeDescriptor {
                    format: attribute.format as _,
                    offset: attribute.offset as _,
                    shaderLocation: attribute.shader_location as _,
                });
            }
            let raw_attributes = unsafe { vertex_attributes.as_ptr().add(attributes_offset) };
            raw_vertex_buffers.push(sys::WGPUVertexBufferLayoutDescriptor {
                arrayStride: vertex_buffer.array_stride,
                stepMode: vertex_buffer.step_mode as _,
                attributeCount: vertex_buffer.attributes.len() as _,
                attributes: raw_attributes,
            })
        }

        let raw_descriptor = sys::WGPURenderPipelineDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            layout: descriptor.layout.raw,
            vertexStage: sys::WGPUProgrammableStageDescriptor {
                nextInChain: ptr::null_mut(),
                module: descriptor.vertex_stage.module.raw,
                entryPoint: vertex_entry_point.as_ptr(),
            },
            fragmentStage: raw_fragment_stage,
            vertexState: &sys::WGPUVertexStateDescriptor {
                nextInChain: ptr::null_mut(),
                indexFormat: descriptor.vertex_state.index_format as _,
                vertexBufferCount: raw_vertex_buffers.len() as _,
                vertexBuffers: raw_vertex_buffers.as_ptr(),
            },
            primitiveTopology: descriptor.primitive_topology as _,
            rasterizationState: raw_rasterization_state,
            sampleCount: descriptor.sample_count,
            depthStencilState: raw_depth_stencil_state,
            colorStateCount: raw_color_states.len() as _,
            colorStates: raw_color_states.as_ptr(),
            sampleMask: descriptor.sample_mask,
            alphaToCoverageEnabled: descriptor.alpha_to_coverage_enabled,
        };

        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateRenderPipeline(guard.raw, &raw_descriptor) };
        drop(guard);
        RenderPipeline {
            raw,
            device: self.clone(),
        }
    }

    pub fn create_sampler(&self, descriptor: &SamplerDescriptor) -> Sampler {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUSamplerDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            addressModeU: descriptor.address_mode_u as _,
            addressModeV: descriptor.address_mode_v as _,
            addressModeW: descriptor.address_mode_w as _,
            magFilter: descriptor.mag_filter as _,
            minFilter: descriptor.min_filter as _,
            mipmapFilter: descriptor.mipmap_filter as _,
            lodMinClamp: descriptor.lod_min_clamp,
            lodMaxClamp: descriptor.lod_max_clamp,
            compare: descriptor.compare as _, // TODO
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateSampler(guard.raw, &raw_descriptor) };
        drop(guard);
        Sampler {
            raw,
            device: self.clone(),
        }
    }

    pub fn create_shader_module(&self, descriptor: &ShaderModuleDescriptor) -> ShaderModule {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUShaderModuleDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            code: descriptor.code.as_ptr(),
            codeSize: descriptor.code.len().try_into().unwrap(),
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateShaderModule(guard.raw, &raw_descriptor) };
        drop(guard);
        ShaderModule {
            raw,
            device: self.clone(),
        }
    }

    pub fn create_shader_module_with_code(&self, spirv: &[u32]) -> ShaderModule {
        self.create_shader_module(&ShaderModuleDescriptor {
            label: None,
            code: spirv,
        })
    }

    pub fn create_texture(&self, descriptor: &TextureDescriptor) -> Texture {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUTextureDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            usage: descriptor.usage.bits as _,
            dimension: descriptor.dimension as _,
            size: sys::WGPUExtent3D {
                width: descriptor.size.width,
                height: descriptor.size.height,
                depth: descriptor.size.depth,
            },
            arrayLayerCount: descriptor.array_layer_count,
            format: descriptor.format as _,
            mipLevelCount: descriptor.mip_level_count,
            sampleCount: descriptor.sample_count,
        };
        let guard = self.inner.lock();
        let raw = unsafe { sys::wgpuDeviceCreateTexture(guard.raw, &raw_descriptor) };
        drop(guard);
        Texture {
            raw,
            device: self.clone(),
        }
    }

    pub fn tick(&self) {
        let guard = self.inner.lock();
        unsafe {
            sys::wgpuDeviceTick(guard.raw);
        }
    }

    pub fn inject_error(&self, message: &str, ty: ErrorType) {
        let message = convert::label(Some(message));
        let guard = self.inner.lock();
        unsafe {
            sys::wgpuDeviceInjectError(guard.raw, ty as _, message.as_ptr());
        }
    }

    // pub fn push_error_scope(&self, filter: ErrorFilter) {
    //     let guard = self.inner.lock();
    //     unsafe {
    //         sys::wgpuDevicePushErrorScope(guard.raw, filter as _);
    //     }
    // }
    //
    // pub fn pop_error_scope<F: Fn()>(&self, callback: F) {
    //
    //     unsafe extern "C" fn raw_callback<F>(
    //         type_: sys::WGPUErrorType,
    //         message: *const libc::c_char,
    //         userdata: *mut libc::c_void,
    //     ) {
    //
    //     }
    //
    //     let userdata = ptr::null_mut();
    //
    //     let guard = self.inner.lock();
    //     unsafe {
    //         sys::wgpuDevicePopErrorScope(guard.raw, Some(raw_callback::<F>), userdata);
    //     }
    // }

    /// TODO
    pub fn set_uncaptured_error_callback(self) {}
}

impl SwapChain {
    pub fn present(&self) {
        let _guard = self.inner.device.inner.lock();
        unsafe { sys::wgpuSwapChainPresent(self.inner.raw) }
    }

    pub fn get_current_texture_view(&self) -> TextureView {
        let guard = self.inner.device.inner.lock();
        let raw = unsafe { sys::wgpuSwapChainGetCurrentTextureView(self.inner.raw) };
        drop(guard);
        TextureView {
            raw,
            device: self.inner.device.clone(),
        }
    }

    pub fn configure(
        &self,
        format: TextureFormat,
        allowed_usage: TextureUsage,
        width: u32,
        height: u32,
    ) {
        if self.backend_type == BackendType::D3D12 {
            // The D3D12 backend crashes if configured more than once. Window resizing
            // appearently doesn't require calling configure for this backend.
            return;
        }
        let _guard = self.inner.device.inner.lock();
        unsafe {
            sys::wgpuSwapChainConfigure(
                self.inner.raw,
                format as _,
                allowed_usage.bits as _,
                width,
                height,
            )
        }
    }
}

impl<'a> ComputePassEncoder<'a> {
    pub fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        unsafe {
            sys::wgpuComputePassEncoderDispatch(self.raw, x, y, z);
        }
    }

    pub fn dispatch_indirect(&mut self, indirect_buffer: &Buffer, indirect_offset: usize) {
        let indirect_offset = indirect_offset.try_into().unwrap();
        unsafe {
            sys::wgpuComputePassEncoderDispatchIndirect(
                self.raw,
                indirect_buffer.raw,
                indirect_offset,
            );
        }
    }

    pub fn end_pass(self) {
        unsafe {
            sys::wgpuComputePassEncoderEndPass(self.raw);
        }
    }

    pub fn insert_debug_marker(&mut self, group_label: &str) {
        let label = convert::label(Some(group_label));
        unsafe {
            sys::wgpuComputePassEncoderInsertDebugMarker(self.raw, label.as_ptr());
        }
    }

    pub fn push_debug_group(&mut self, group_label: &str) {
        let label = convert::label(Some(group_label));
        unsafe {
            sys::wgpuComputePassEncoderPushDebugGroup(self.raw, label.as_ptr());
        }
    }

    pub fn pop_debug_group(&mut self) {
        unsafe {
            sys::wgpuComputePassEncoderPopDebugGroup(self.raw);
        }
    }

    pub fn set_bind_group(
        &mut self,
        group_index: usize,
        group: &BindGroup,
        dynamic_offsets: &[u32],
    ) {
        let group_index = group_index.try_into().unwrap();
        unsafe {
            sys::wgpuComputePassEncoderSetBindGroup(
                self.raw,
                group_index,
                group.raw,
                dynamic_offsets.len() as _,
                dynamic_offsets.as_ptr(),
            );
        }
    }

    pub fn set_pipeline(&mut self, pipeline: &ComputePipeline) {
        unsafe {
            sys::wgpuComputePassEncoderSetPipeline(self.raw, pipeline.raw);
        }
    }
}

impl ComputePipeline {
    pub fn get_bind_group_layout(&mut self, group: usize) -> BindGroupLayout {
        let group = group.try_into().unwrap();
        let raw = unsafe { sys::wgpuComputePipelineGetBindGroupLayout(self.raw, group) };
        BindGroupLayout {
            raw,
            device: self.device.clone(),
        }
    }
}

impl Queue {
    pub fn create_fence(&self, descriptor: &FenceDescriptor) -> Fence {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUFenceDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            initialValue: descriptor.initial_value,
        };
        let guard = self.device.inner.lock();
        let raw = unsafe { sys::wgpuQueueCreateFence(self.raw, &raw_descriptor) };
        drop(guard);
        Fence {
            raw,
            device: self.device.clone(),
        }
    }

    pub fn create_fence_with(&self, initial_value: u64) -> Fence {
        self.create_fence(&FenceDescriptor {
            initial_value,
            label: None,
        })
    }

    pub fn submit(&mut self, commands: &[CommandBuffer]) {
        unsafe {
            self.temp_commands.clear();
            for command in commands.iter() {
                self.temp_commands.push(command.raw);
            }
            let commands = self.temp_commands.as_ptr();
            let command_count = self.temp_commands.len().try_into().unwrap();
            let _guard = self.device.inner.lock();
            sys::wgpuQueueSubmit(self.raw, command_count, commands);
        }
    }

    pub fn signal(&self, fence: &Fence, signal_value: u64) {
        unsafe {
            let _guard = self.device.inner.lock();
            sys::wgpuQueueSignal(self.raw, fence.raw, signal_value);
        }
    }
}

impl<'a> RenderPassEncoder<'a> {
    pub fn draw(
        &self,
        vertex_count: usize,
        instance_count: usize,
        first_vertex: usize,
        first_instance: usize,
    ) {
        unsafe {
            sys::wgpuRenderPassEncoderDraw(
                self.raw,
                vertex_count.try_into().unwrap(),
                instance_count.try_into().unwrap(),
                first_vertex.try_into().unwrap(),
                first_instance.try_into().unwrap(),
            )
        }
    }

    pub fn draw_indexed(
        &self,
        index_count: usize,
        instance_count: usize,
        first_index: usize,
        base_vertex: isize,
        first_instance: usize,
    ) {
        unsafe {
            sys::wgpuRenderPassEncoderDrawIndexed(
                self.raw,
                index_count.try_into().unwrap(),
                instance_count.try_into().unwrap(),
                first_index.try_into().unwrap(),
                base_vertex.try_into().unwrap(),
                first_instance.try_into().unwrap(),
            )
        }
    }

    pub fn draw_indirect(&self, indirect_buffer: &Buffer, indirect_offset: usize) {
        unsafe {
            sys::wgpuRenderPassEncoderDrawIndirect(
                self.raw,
                indirect_buffer.raw,
                indirect_offset.try_into().unwrap(),
            )
        }
    }

    pub fn draw_indexed_indirect(&self, indirect_buffer: &Buffer, indirect_offset: usize) {
        unsafe {
            sys::wgpuRenderPassEncoderDrawIndexedIndirect(
                self.raw,
                indirect_buffer.raw,
                indirect_offset.try_into().unwrap(),
            )
        }
    }

    pub fn end_pass(self) {
        unsafe {
            sys::wgpuRenderPassEncoderEndPass(self.raw);
        }
    }

    pub fn execute_bundles(&self, bundles: &[RenderBundle]) {
        let bundles_count = bundles.len().try_into().unwrap();
        let bundles: Vec<_> = bundles.iter().map(|bundle| bundle.raw).collect();
        unsafe {
            sys::wgpuRenderPassEncoderExecuteBundles(self.raw, bundles_count, bundles.as_ptr());
        }
    }

    pub fn insert_debug_marker(&mut self, group_label: &str) {
        let label = convert::label(Some(group_label));
        unsafe {
            sys::wgpuRenderPassEncoderInsertDebugMarker(self.raw, label.as_ptr());
        }
    }

    pub fn push_debug_group(&mut self, group_label: &str) {
        let label = convert::label(Some(group_label));
        unsafe {
            sys::wgpuRenderPassEncoderPushDebugGroup(self.raw, label.as_ptr());
        }
    }

    pub fn pop_debug_group(&mut self) {
        unsafe {
            sys::wgpuRenderPassEncoderPopDebugGroup(self.raw);
        }
    }

    pub fn set_bind_group(
        &mut self,
        group_index: usize,
        group: &BindGroup,
        dynamic_offsets: &[u32],
    ) {
        let group_index = group_index.try_into().unwrap();
        unsafe {
            sys::wgpuRenderPassEncoderSetBindGroup(
                self.raw,
                group_index,
                group.raw,
                dynamic_offsets.len() as _,
                dynamic_offsets.as_ptr(),
            );
        }
    }

    pub fn set_blend_color(&mut self, color: &Color) {
        unsafe {
            let color = sys::WGPUColor {
                r: color.r,
                g: color.g,
                b: color.b,
                a: color.a,
            };
            sys::wgpuRenderPassEncoderSetBlendColor(self.raw, &color)
        }
    }

    pub fn set_index_buffer(&mut self, index_buffer: &Buffer, offset: usize) {
        let offset = offset.try_into().unwrap();
        unsafe {
            sys::wgpuRenderPassEncoderSetIndexBuffer(self.raw, index_buffer.raw, offset);
        }
    }

    pub fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        unsafe {
            sys::wgpuRenderPassEncoderSetPipeline(self.raw, pipeline.raw);
        }
    }

    pub fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        unsafe {
            sys::wgpuRenderPassEncoderSetScissorRect(self.raw, x, y, width, height);
        }
    }

    pub fn set_stencil_reference(&mut self, reference: u32) {
        unsafe {
            sys::wgpuRenderPassEncoderSetStencilReference(self.raw, reference);
        }
    }

    pub fn set_vertex_buffer(&mut self, slot: usize, vertex_buffer: &Buffer, offset: usize) {
        let slot = slot.try_into().unwrap();
        let offset = offset.try_into().unwrap();
        unsafe {
            sys::wgpuRenderPassEncoderSetVertexBuffer(self.raw, slot, vertex_buffer.raw, offset);
        }
    }

    pub fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        unsafe {
            sys::wgpuRenderPassEncoderSetViewport(
                self.raw, x, y, width, height, min_depth, max_depth,
            );
        }
    }
}

impl RenderPipeline {
    pub fn get_bind_group_layout(&mut self, group: usize) -> BindGroupLayout {
        let group = group.try_into().unwrap();
        let raw = unsafe { sys::wgpuRenderPipelineGetBindGroupLayout(self.raw, group) };
        BindGroupLayout {
            raw,
            device: self.device.clone(),
        }
    }
}

impl Texture {
    pub fn create_view(&self, descriptor: &TextureViewDescriptor) -> TextureView {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUTextureViewDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            format: descriptor.format as _,
            dimension: descriptor.dimension as _,
            baseMipLevel: descriptor.base_mip_level,
            mipLevelCount: descriptor.mip_level_count,
            baseArrayLayer: descriptor.base_array_layer,
            arrayLayerCount: descriptor.array_layer_count,
            aspect: descriptor.aspect as _,
        };
        let _guard = self.device.inner.lock();
        let raw = unsafe { sys::wgpuTextureCreateView(self.raw, &raw_descriptor) };
        drop(_guard);
        TextureView {
            raw,
            device: self.device.clone(),
        }
    }
}

impl CommandEncoder {
    pub fn begin_compute_pass(
        &mut self,
        descriptor: &ComputePassDescriptor,
    ) -> ComputePassEncoder<'_> {
        let label = convert::label(descriptor.label);
        let raw_descriptor = sys::WGPUComputePassDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
        };
        let guard = self.device.inner.lock();
        let raw = unsafe { sys::wgpuCommandEncoderBeginComputePass(self.raw, &raw_descriptor) };
        drop(guard);
        ComputePassEncoder {
            raw,
            _device: self.device.clone(),
            _p: PhantomData,
        }
    }

    pub fn begin_render_pass(
        &mut self,
        descriptor: &RenderPassDescriptor,
    ) -> RenderPassEncoder<'_> {
        let label = convert::label(descriptor.label);

        let mut raw_color_attachments =
            SmallVec::<[sys::WGPURenderPassColorAttachmentDescriptor; 4]>::with_capacity(
                descriptor.color_attachments.len(),
            );
        for color_attachment in descriptor.color_attachments.iter() {
            raw_color_attachments.push(sys::WGPURenderPassColorAttachmentDescriptor {
                attachment: color_attachment.attachment.raw,
                resolveTarget: color_attachment
                    .resolve_target
                    .map(|a| a.raw)
                    .unwrap_or_else(ptr::null_mut),
                loadOp: color_attachment.load_op as _,
                storeOp: color_attachment.store_op as _,
                clearColor: sys::WGPUColor {
                    r: color_attachment.clear_color.r,
                    g: color_attachment.clear_color.g,
                    b: color_attachment.clear_color.b,
                    a: color_attachment.clear_color.a,
                },
            });
        }

        let depth_stencil_attachment = descriptor.depth_stencil_attachment.map(|a| {
            sys::WGPURenderPassDepthStencilAttachmentDescriptor {
                attachment: a.attachment.raw,
                depthLoadOp: a.depth_load_op as _,
                depthStoreOp: a.depth_store_op as _,
                clearDepth: a.clear_depth,
                stencilLoadOp: a.stencil_load_op as _,
                stencilStoreOp: a.stencil_store_op as _,
                clearStencil: a.clear_stencil,
            }
        });
        let raw_depth_stencil_attachment = depth_stencil_attachment
            .as_ref()
            .map(|a| a as *const _)
            .unwrap_or_else(ptr::null);

        let raw_descriptor = sys::WGPURenderPassDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
            colorAttachmentCount: raw_color_attachments.len() as _,
            colorAttachments: raw_color_attachments.as_ptr(),
            depthStencilAttachment: raw_depth_stencil_attachment,
        };
        let guard = self.device.inner.lock();
        let raw = unsafe { sys::wgpuCommandEncoderBeginRenderPass(self.raw, &raw_descriptor) };
        drop(guard);
        RenderPassEncoder {
            raw,
            _device: self.device.clone(),
            _p: PhantomData,
        }
    }

    pub fn copy_buffer_to_buffer(
        &mut self,
        source: &Buffer,
        source_offset: usize,
        destination: &Buffer,
        destination_offset: usize,
        size: usize,
    ) {
        let source_offset = source_offset.try_into().unwrap();
        let destination_offset = destination_offset.try_into().unwrap();
        let size = size.try_into().unwrap();
        let _guard = self.device.inner.lock();
        unsafe {
            sys::wgpuCommandEncoderCopyBufferToBuffer(
                self.raw,
                source.raw,
                source_offset,
                destination.raw,
                destination_offset,
                size,
            );
        }
    }

    pub fn copy_buffer_to_texture(
        &mut self,
        source: &BufferCopyView,
        destination: &TextureCopyView,
        copy_size: &Extent3d,
    ) {
        let raw_source = sys::WGPUBufferCopyView {
            nextInChain: ptr::null_mut(),
            buffer: source.buffer.raw,
            offset: source.offset.try_into().unwrap(),
            rowPitch: source.bytes_per_row.try_into().unwrap(),
            imageHeight: source.rows_per_image.try_into().unwrap(),
        };
        let raw_destination = sys::WGPUTextureCopyView {
            nextInChain: ptr::null_mut(),
            texture: destination.texture.raw,
            mipLevel: destination.mip_level,
            arrayLayer: destination.array_layer,
            origin: sys::WGPUOrigin3D {
                x: destination.origin.x,
                y: destination.origin.y,
                z: destination.origin.z,
            },
        };
        let raw_copy_size = sys::WGPUExtent3D {
            width: copy_size.width,
            height: copy_size.height,
            depth: copy_size.depth,
        };
        let _guard = self.device.inner.lock();
        unsafe {
            sys::wgpuCommandEncoderCopyBufferToTexture(
                self.raw,
                &raw_source,
                &raw_destination,
                &raw_copy_size,
            );
        }
    }

    pub fn copy_texture_to_buffer(
        &mut self,
        source: &TextureCopyView,
        destination: &BufferCopyView,
        copy_size: &Extent3d,
    ) {
        let raw_destination = sys::WGPUBufferCopyView {
            nextInChain: ptr::null_mut(),
            buffer: destination.buffer.raw,
            offset: destination.offset.try_into().unwrap(),
            rowPitch: destination.bytes_per_row.try_into().unwrap(),
            imageHeight: destination.rows_per_image.try_into().unwrap(),
        };
        let raw_source = sys::WGPUTextureCopyView {
            nextInChain: ptr::null_mut(),
            texture: source.texture.raw,
            mipLevel: source.mip_level,
            arrayLayer: source.array_layer,
            origin: sys::WGPUOrigin3D {
                x: source.origin.x,
                y: source.origin.y,
                z: source.origin.z,
            },
        };
        let raw_copy_size = sys::WGPUExtent3D {
            width: copy_size.width,
            height: copy_size.height,
            depth: copy_size.depth,
        };
        let _guard = self.device.inner.lock();
        unsafe {
            sys::wgpuCommandEncoderCopyTextureToBuffer(
                self.raw,
                &raw_source,
                &raw_destination,
                &raw_copy_size,
            );
        }
    }

    pub fn copy_texture_to_texture(
        &mut self,
        source: &TextureCopyView,
        destination: &TextureCopyView,
        copy_size: &Extent3d,
    ) {
        let raw_destination = sys::WGPUTextureCopyView {
            nextInChain: ptr::null_mut(),
            texture: destination.texture.raw,
            mipLevel: destination.mip_level,
            arrayLayer: destination.array_layer,
            origin: sys::WGPUOrigin3D {
                x: destination.origin.x,
                y: destination.origin.y,
                z: destination.origin.z,
            },
        };
        let raw_source = sys::WGPUTextureCopyView {
            nextInChain: ptr::null_mut(),
            texture: source.texture.raw,
            mipLevel: source.mip_level,
            arrayLayer: source.array_layer,
            origin: sys::WGPUOrigin3D {
                x: source.origin.x,
                y: source.origin.y,
                z: source.origin.z,
            },
        };
        let raw_copy_size = sys::WGPUExtent3D {
            width: copy_size.width,
            height: copy_size.height,
            depth: copy_size.depth,
        };
        let _guard = self.device.inner.lock();
        unsafe {
            sys::wgpuCommandEncoderCopyTextureToTexture(
                self.raw,
                &raw_source,
                &raw_destination,
                &raw_copy_size,
            );
        }
    }

    pub fn finish(self) -> CommandBuffer {
        let label = convert::label(None);
        let raw_descriptor = sys::WGPUCommandBufferDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
        };
        let _guard = self.device.inner.lock();
        let raw = unsafe { sys::wgpuCommandEncoderFinish(self.raw, &raw_descriptor) };
        drop(_guard);
        CommandBuffer {
            raw,
            _device: self.device.clone(),
        }
    }
}

impl RenderBundleEncoder {
    pub fn draw(
        &self,
        vertex_count: usize,
        instance_count: usize,
        first_vertex: usize,
        first_instance: usize,
    ) {
        unsafe {
            sys::wgpuRenderBundleEncoderDraw(
                self.raw,
                vertex_count.try_into().unwrap(),
                instance_count.try_into().unwrap(),
                first_vertex.try_into().unwrap(),
                first_instance.try_into().unwrap(),
            )
        }
    }

    pub fn draw_indexed(
        &self,
        index_count: usize,
        instance_count: usize,
        first_index: usize,
        base_vertex: isize,
        first_instance: usize,
    ) {
        unsafe {
            sys::wgpuRenderBundleEncoderDrawIndexed(
                self.raw,
                index_count.try_into().unwrap(),
                instance_count.try_into().unwrap(),
                first_index.try_into().unwrap(),
                base_vertex.try_into().unwrap(),
                first_instance.try_into().unwrap(),
            )
        }
    }

    pub fn draw_indirect(&self, indirect_buffer: &Buffer, indirect_offset: usize) {
        unsafe {
            sys::wgpuRenderBundleEncoderDrawIndirect(
                self.raw,
                indirect_buffer.raw,
                indirect_offset.try_into().unwrap(),
            )
        }
    }

    pub fn draw_indexed_indirect(&self, indirect_buffer: &Buffer, indirect_offset: usize) {
        unsafe {
            sys::wgpuRenderBundleEncoderDrawIndexedIndirect(
                self.raw,
                indirect_buffer.raw,
                indirect_offset.try_into().unwrap(),
            )
        }
    }

    pub fn finish(self) -> RenderBundle {
        let label = convert::label(None);
        let raw_descriptor = sys::WGPURenderBundleDescriptor {
            nextInChain: ptr::null_mut(),
            label: label.as_ptr(),
        };
        let guard = self.device.inner.lock();
        let raw = unsafe { sys::wgpuRenderBundleEncoderFinish(self.raw, &raw_descriptor) };
        drop(guard);
        RenderBundle {
            raw,
            device: self.device.clone(),
        }
    }

    pub fn insert_debug_marker(&mut self, group_label: &str) {
        let label = convert::label(Some(group_label));
        unsafe {
            sys::wgpuRenderBundleEncoderInsertDebugMarker(self.raw, label.as_ptr());
        }
    }

    pub fn push_debug_group(&mut self, group_label: &str) {
        let label = convert::label(Some(group_label));
        unsafe {
            sys::wgpuRenderBundleEncoderPushDebugGroup(self.raw, label.as_ptr());
        }
    }

    pub fn pop_debug_group(&mut self) {
        unsafe {
            sys::wgpuRenderBundleEncoderPopDebugGroup(self.raw);
        }
    }

    pub fn set_bind_group(
        &mut self,
        group_index: usize,
        group: &BindGroup,
        dynamic_offsets: &[u32],
    ) {
        let group_index = group_index.try_into().unwrap();
        unsafe {
            sys::wgpuRenderBundleEncoderSetBindGroup(
                self.raw,
                group_index,
                group.raw,
                dynamic_offsets.len() as _,
                dynamic_offsets.as_ptr(),
            );
        }
    }

    pub fn set_index_buffer(&mut self, index_buffer: &Buffer, offset: usize) {
        let offset = offset.try_into().unwrap();
        unsafe {
            sys::wgpuRenderBundleEncoderSetIndexBuffer(self.raw, index_buffer.raw, offset);
        }
    }

    pub fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        unsafe {
            sys::wgpuRenderBundleEncoderSetPipeline(self.raw, pipeline.raw);
        }
    }

    pub fn set_vertex_buffer(&mut self, slot: usize, vertex_buffer: &Buffer, offset: usize) {
        let slot = slot.try_into().unwrap();
        let offset = offset.try_into().unwrap();
        unsafe {
            sys::wgpuRenderBundleEncoderSetVertexBuffer(self.raw, slot, vertex_buffer.raw, offset);
        }
    }
}

impl Buffer {
    pub fn unmap(&self) {
        let _gaurd = self.device.inner.lock();
        unsafe { sys::wgpuBufferUnmap(self.raw) }
    }

    pub fn set_sub_data(&self, offset: usize, data: &[u8]) {
        let start = offset.try_into().unwrap();
        let count = data.len().try_into().unwrap();
        let raw_data = data.as_ptr() as _;
        let _gaurd = self.device.inner.lock();
        unsafe { sys::wgpuBufferSetSubData(self.raw, start, count, raw_data) }
    }

    /// TODO
    pub fn map_write_async(self) {}

    /// TODO
    pub fn map_read_async(self) {}
}
