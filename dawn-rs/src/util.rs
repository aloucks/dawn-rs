use std::{mem, ptr};

pub fn spirv(code: &[u8]) -> Vec<u32> {
    let byte_count = code.len();
    let word_extra = if byte_count % 4 > 0 { 1 } else { 0 };
    let word_count = ((byte_count / mem::size_of::<u32>()) + word_extra).max(1);
    let mut words: Vec<u32> = vec![0; word_count as usize];

    unsafe {
        ptr::copy_nonoverlapping(code.as_ptr(), words.as_mut_ptr() as *mut u8, byte_count);
    }

    words
}
