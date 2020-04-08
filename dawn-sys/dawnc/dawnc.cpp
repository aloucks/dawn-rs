
#include "dawnc.h"

WGPUAdapterType ConvertAdapterType(wgpu::AdapterType adapterType) {
    switch (adapterType) {
        case wgpu::AdapterType::CPU: 
            return WGPUAdapterType_CPU;
        case wgpu::AdapterType::DiscreteGPU: 
            return WGPUAdapterType_DiscreteGPU;
        case wgpu::AdapterType::IntegratedGPU:
            return WGPUAdapterType_IntegratedGPU;
        case wgpu::AdapterType::Unknown:
            return WGPUAdapterType_Unknown;
    }
}

WGPUBackendType ConvertBackendType(wgpu::BackendType backendType) {
    switch (backendType) {
        case wgpu::BackendType::Vulkan: 
            return  WGPUBackendType_Vulkan;
        case wgpu::BackendType::D3D12: 
            return WGPUBackendType_D3D12;
        case wgpu::BackendType::D3D11: 
            return WGPUBackendType_D3D11;
        case wgpu::BackendType::Metal:
            return WGPUBackendType_Metal;
        case wgpu::BackendType::OpenGL:
            return WGPUBackendType_OpenGL;
        case wgpu::BackendType::OpenGLES:
            return WGPUBackendType_OpenGLES;
        case wgpu::BackendType::Null:
            return WGPUBackendType_Null;
    }
}

class InstanceHack {
    public:
        dawn_native::InstanceBase* mImpl = nullptr;
};

void dawn_native__GetProcs(DawnProcTable* procTable) {
    *procTable = dawn_native::GetProcs();
}

void dawn_native__Instance__DiscoverDefaultAdapters(const WGPUInstance instance) {
    InstanceHack instanceHack;
    instanceHack.mImpl = reinterpret_cast<dawn_native::InstanceBase*>(instance);
    auto dawnInstance = reinterpret_cast<dawn_native::Instance*>(&instanceHack);
    dawnInstance->DiscoverDefaultAdapters();
}

size_t dawn_native__Instance__GetAdaptersCount(const WGPUInstance instance) {
    InstanceHack instanceHack;
    instanceHack.mImpl = reinterpret_cast<dawn_native::InstanceBase*>(instance);
    auto dawnInstance = reinterpret_cast<dawn_native::Instance*>(&instanceHack);
    auto dawnAdapters = dawnInstance->GetAdapters();
    return dawnAdapters.size();
}

WGPUDeviceProperties dawn_native__Adapter__GetAdapterProperties(WGPUInstance instance, size_t adapterIndex) {
    InstanceHack instanceHack;
    instanceHack.mImpl = reinterpret_cast<dawn_native::InstanceBase*>(instance);
    auto dawnInstance = reinterpret_cast<dawn_native::Instance*>(&instanceHack);
    dawnInstance->DiscoverDefaultAdapters();
    auto dawnAdapters = dawnInstance->GetAdapters();
    auto dawnAdapter = &dawnAdapters[adapterIndex];
    return dawnAdapter->GetAdapterProperties();
}

void dawn_native__Adapter__GetProperties(WGPUInstance instance, size_t adapterIndex, WGPUAdapterProperties* properties) {
    InstanceHack instanceHack;
    instanceHack.mImpl = reinterpret_cast<dawn_native::InstanceBase*>(instance);
    auto dawnInstance = reinterpret_cast<dawn_native::Instance*>(&instanceHack);
    dawnInstance->DiscoverDefaultAdapters();
    auto dawnAdapters = dawnInstance->GetAdapters();
    auto dawnAdapter = &dawnAdapters[adapterIndex];
    wgpu::AdapterProperties adapterProperties;
    dawnAdapter->GetProperties(&adapterProperties);
    properties->name = adapterProperties.name;
    properties->deviceID = adapterProperties.deviceID;
    properties->vendorID = adapterProperties.vendorID;
    properties->nextInChain = reinterpret_cast<const WGPUChainedStruct*>(adapterProperties.nextInChain);
    properties->adapterType = ConvertAdapterType(adapterProperties.adapterType);
    properties->backendType = ConvertBackendType(adapterProperties.backendType);
}

VkInstance dawn_native__vulkan__GetInstance(WGPUDevice device) {
    return dawn_native::vulkan::GetInstance(device);
}

WGPUDevice dawn_native__Adapter__CreateDevice(WGPUInstance instance, size_t adapterIndex, const DeviceDescriptor* descriptor) {
    dawn_native::DeviceDescriptor dawnDeviceDescriptor;
    if (descriptor != nullptr) {
        for (size_t i = 0; i < descriptor->requiredExtensionsCount; i++) {
            auto name = descriptor->requiredExtensions[i];
            dawnDeviceDescriptor.requiredExtensions.push_back(name);
        }
        for (size_t i = 0; i < descriptor->forceEnabledTogglesCount; i++) {
            auto name = descriptor->forceEnabledToggles[i];
            dawnDeviceDescriptor.forceEnabledToggles.push_back(name);
        }
        for (size_t i = 0; i < descriptor->forceDisabledTogglesCount; i++) {
            auto name = descriptor->forceDisabledToggles[i];
            dawnDeviceDescriptor.forceDisabledToggles.push_back(name);
        }
    }
    InstanceHack instanceHack;
    instanceHack.mImpl = reinterpret_cast<dawn_native::InstanceBase*>(instance);
    auto dawnInstance = reinterpret_cast<dawn_native::Instance*>(&instanceHack);
    dawnInstance->DiscoverDefaultAdapters();
    auto dawnAdapters = dawnInstance->GetAdapters();
    auto dawnAdapter = &dawnAdapters[adapterIndex];
    return dawnAdapter->CreateDevice(&dawnDeviceDescriptor);
}

WGPUTextureFormat dawn_native__vulkan__GetNativeSwapChainPreferredFormat(const DawnSwapChainImplementation* swapChainImpl) {
    return dawn_native::vulkan::GetNativeSwapChainPreferredFormat(swapChainImpl);
}

DawnSwapChainImplementation dawn_native__vulkan__CreateNativeSwapChainImpl(WGPUDevice device, VkSurfaceKHR surface) {
    return dawn_native::vulkan::CreateNativeSwapChainImpl(device, surface);
}

#ifdef _WIN32
WGPUTextureFormat dawn_native__d3d12__GetNativeSwapChainPreferredFormat(const DawnSwapChainImplementation* swapChainImpl) {
    return dawn_native::d3d12::GetNativeSwapChainPreferredFormat(swapChainImpl);
}

DawnSwapChainImplementation dawn_native__d3d12__CreateNativeSwapChainImpl(WGPUDevice device, HWND hwnd) {
    return dawn_native::d3d12::CreateNativeSwapChainImpl(device, hwnd);
}
#endif

