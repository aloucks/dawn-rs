use crate::{BackendType, Device, PresentMode, SwapChain, SwapChainDescriptor, TextureUsage};

use dawn_sys as sys;

use std::sync::Arc;

pub enum NativeSwapChainSurfaceParams {
    D3D12 { hwnd: sys::HWND },
    Vulkan { surface: sys::VkSurfaceKHR },
}

pub struct NativeSwapChainDescriptor {
    pub params: NativeSwapChainSurfaceParams,
    pub width: u32,
    pub height: u32,
    pub present_mode: PresentMode,
}

pub fn create_swap_chain(device: &Device, descriptor: NativeSwapChainDescriptor) -> SwapChain {
    let guard = device.inner.lock();
    let backend_type = guard.backend_type;
    let (dawn_swap_chain_impl, format) = match descriptor.params {
        NativeSwapChainSurfaceParams::D3D12 { hwnd } => {
            assert_eq!(
                BackendType::D3D12,
                backend_type,
                "native swap chain params do not match device backend"
            );
            unsafe {
                let dawn_swap_chain_impl =
                    sys::dawn_native__d3d12__CreateNativeSwapChainImpl(guard.raw, hwnd);
                let format = sys::dawn_native__d3d12__GetNativeSwapChainPreferredFormat(
                    &dawn_swap_chain_impl,
                );
                (Arc::new(dawn_swap_chain_impl), format)
            }
        }
        NativeSwapChainSurfaceParams::Vulkan { surface } => {
            assert_eq!(
                BackendType::Vulkan,
                backend_type,
                "native swap chain params do not match device backend"
            );
            unsafe {
                let dawn_swap_chain_impl =
                    sys::dawn_native__vulkan__CreateNativeSwapChainImpl(guard.raw, surface);
                let format = sys::dawn_native__vulkan__GetNativeSwapChainPreferredFormat(
                    &dawn_swap_chain_impl,
                );
                (Arc::new(dawn_swap_chain_impl), format)
            }
        }
    };
    let descriptor = SwapChainDescriptor {
        label: None,
        width: descriptor.width,
        height: descriptor.height,
        format: unsafe { std::mem::transmute(format) },
        present_mode: descriptor.present_mode,
        usage: TextureUsage::OUTPUT_ATTACHMENT,
        implementation: dawn_swap_chain_impl.as_ref() as *const _ as u64,
    };
    drop(guard);
    let mut swap_chain = device.create_swap_chain(None, &descriptor);
    unsafe {
        sys::wgpuSwapChainConfigure(
            swap_chain.inner.raw,
            format as _,
            descriptor.usage.bits as _,
            descriptor.width,
            descriptor.height,
        )
    }
    swap_chain.dawn_swap_chain_impl = Some(dawn_swap_chain_impl);
    swap_chain
}

pub fn get_vulkan_instance(device: &Device) -> sys::VkInstance {
    let guard = device.inner.lock();
    let backend_type = guard.backend_type;
    assert_eq!(
        BackendType::Vulkan,
        backend_type,
        "device backend is not vulkan"
    );
    unsafe { dawn_sys::dawn_native__vulkan__GetInstance(guard.raw) }
}
