use dawn_sys as sys;

use std::{ffi, mem, ptr};

use crate::{AdapterType, BackendType};

pub fn adapter_type(v: i32) -> AdapterType {
    match v {
        sys::WGPUAdapterType_DiscreteGPU => AdapterType::DiscreteGPU,
        sys::WGPUAdapterType_IntegratedGPU => AdapterType::IntegratedGPU,
        sys::WGPUAdapterType_CPU => AdapterType::CPU,
        sys::WGPUAdapterType_Unknown => AdapterType::Unknown,
        _ => panic!("invalid adapter type: {}", v),
    }
}

pub fn backend_type(v: i32) -> BackendType {
    match v {
        sys::WGPUBackendType_Vulkan => BackendType::Vulkan,
        sys::WGPUBackendType_Metal => BackendType::Metal,
        sys::WGPUBackendType_D3D11 => BackendType::D3D11,
        sys::WGPUBackendType_D3D12 => BackendType::D3D12,
        sys::WGPUBackendType_OpenGL => BackendType::OpenGL,
        sys::WGPUBackendType_OpenGLES => BackendType::OpenGLES,
        sys::WGPUBackendType_Null => BackendType::Null,
        _ => panic!("invalid backend type: {}", v),
    }
}

// 30 + 1 byte for len + 1 byte for discriminate = 32 bytes for Label::Inline
const LABEL_MAX_INLINE_WITH_NULL_LEN: usize = 30;

pub enum Label {
    Inline {
        len: u8,
        data: [u8; LABEL_MAX_INLINE_WITH_NULL_LEN],
    },
    Heap(ffi::CString),
    Empty,
}

#[test]
fn label_enum_size() {
    assert_eq!(32, std::mem::size_of::<Label>());
}

impl Label {
    pub fn from(label: Option<&str>) -> Label {
        match label {
            Some(label) => Label::from_str(label),
            None => Label::Empty,
        }
    }

    pub fn from_str(label: &str) -> Label {
        if label.len() < LABEL_MAX_INLINE_WITH_NULL_LEN - 1 {
            let mut data = unsafe {
                let mut data: mem::MaybeUninit<[u8; LABEL_MAX_INLINE_WITH_NULL_LEN]> =
                    mem::MaybeUninit::uninit();
                let bytes = label.as_bytes();
                bytes.as_ptr().copy_to(data.as_mut_ptr() as _, bytes.len());
                data.assume_init()
            };
            data[label.len()] = 0;
            Label::Inline {
                data,
                len: label.len() as _,
            }
        } else {
            Label::Heap(ffi::CString::new(label.to_string()).unwrap())
        }
    }

    pub fn as_ptr(&self) -> *const std::os::raw::c_char {
        match self {
            Label::Inline { data, .. } => data.as_ptr() as _,
            Label::Heap(s) => s.as_ptr() as _,
            Label::Empty => ptr::null(),
        }
    }
}

pub fn label(label: Option<&str>) -> Label {
    Label::from(label)
}
