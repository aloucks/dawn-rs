#ifndef DAWNC_H

#define DAWNC_EXPORT WGPU_EXPORT extern "C"

#include <dawn_native/Instance.h>
#include <dawn_native/Adapter.h>
#include <dawn_native/Device.h>
#include <dawn_native/VulkanBackend.h>
#include <dawn/webgpu.h>
#include <dawn/webgpu_cpp.h>
#include <dawn/dawn_proc.h>
#include <dawn/dawn_proc_table.h>
#include <dawn/dawn_wsi.h>
#include <vulkan/vulkan.h>

#ifdef _WIN32
#include <dawn_native/D3D12Backend.h>
#endif

struct DeviceDescriptor {
    const char** requiredExtensions;
    size_t requiredExtensionsCount;

    const char** forceEnabledToggles;
    size_t forceEnabledTogglesCount;

    const char** forceDisabledToggles;
    size_t forceDisabledTogglesCount;
};

DAWNC_EXPORT void dawn_native__GetProcs(DawnProcTable* procTable);
DAWNC_EXPORT void dawn_native__Instance__DiscoverDefaultAdapters(const WGPUInstance instance);
DAWNC_EXPORT size_t dawn_native__Instance__GetAdaptersCount(const WGPUInstance instance);
DAWNC_EXPORT WGPUDeviceProperties dawn_native__Adapter__GetAdapterProperties(WGPUInstance instance, size_t adapterIndex);
DAWNC_EXPORT void dawn_native__Adapter__GetProperties(WGPUInstance instance, size_t adapterIndex, WGPUAdapterProperties* properties);
DAWNC_EXPORT VkInstance dawn_native__vulkan__GetInstance(WGPUDevice device);
DAWNC_EXPORT WGPUDevice dawn_native__Adapter__CreateDevice(WGPUInstance instance, size_t adapterIndex, const DeviceDescriptor* descriptor);
DAWNC_EXPORT WGPUTextureFormat dawn_native__vulkan__GetNativeSwapChainPreferredFormat(const DawnSwapChainImplementation* swapChainImpl);
DAWNC_EXPORT DawnSwapChainImplementation dawn_native__vulkan__CreateNativeSwapChainImpl(WGPUDevice device, VkSurfaceKHR surface);

#ifdef _WIN32
DAWNC_EXPORT WGPUTextureFormat dawn_native__d3d12__GetNativeSwapChainPreferredFormat(const DawnSwapChainImplementation* swapChainImpl);
DAWNC_EXPORT DawnSwapChainImplementation dawn_native__d3d12__CreateNativeSwapChainImpl(WGPUDevice device, HWND hwnd);
#endif // _WIN32

#endif // DAWNC_H