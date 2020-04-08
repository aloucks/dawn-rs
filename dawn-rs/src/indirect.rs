//! Structures for indirect draw and dispatch

///
/// # Notes
///
/// | Platform | Structure |
/// | -------- | --------- |
/// | Vulkan   | `VkDrawIndirectCommand` |
/// | D3D12    | `D3D12_DRAW_ARGUMENTS` |
/// | Metal    | `MTLDrawPrimitivesIndirectArguments` |
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrawIndirectCommand {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub first_instance: i32,
}

///
/// # Notes
///
/// | Platform | Structure |
/// | -------- | --------- |
/// | Vulkan   | `VkDrawIndexedIndirectCommand` |
/// | D3D12    | `D3D12_DRAW_INDEXED_ARGUMENTS` |
/// | Metal    | `MTLDrawIndexedPrimitivesIndirectArguments` |
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrawIndexedIndirectCommand {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub base_vertex: i32,
    pub first_instance: i32,
}

///
/// # Notes
///
/// | Platform | Structure |
/// | -------- | --------- |
/// | Vulkan   | `VkDispatchIndirectCommand` |
/// | D3D12    | `D3D12_DISPATCH_ARGUMENTS` |
/// | Metal    | `MTLDispatchThreadgroupsIndirectArguments` |
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DispatchIndirectCommand {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}
