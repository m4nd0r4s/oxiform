//! A host buffer backed by a `Vec<u8>`, for hosts written in Rust.

use core::ffi::c_void;

use crate::types::OxBuffer;

/// Output bytes in a `Vec<u8>` of the host's allocator, grown by oxiform through `reserve`.
#[derive(Debug, Default)]
pub struct VecBuffer {
    bytes: Vec<u8>,
}

unsafe extern "C" fn reserve_vec(buffer: *mut OxBuffer, min_capacity: usize) -> i32 {
    // SAFETY: oxiform calls `reserve` with the buffer `with_raw` gave it, which no one else uses
    // meanwhile.
    let buffer = unsafe { &mut *buffer };
    // SAFETY: `with_raw` stored a pointer to its `Vec` in `user_data`, and the `Vec` outlives the
    // call, which `with_raw` is in.
    let bytes = unsafe { &mut *buffer.user_data.cast::<Vec<u8>>() };
    // SAFETY: `buffer.len <= capacity == bytes.capacity()` and oxiform initialized the first
    // `buffer.len` bytes before it asked for more room.
    unsafe { bytes.set_len(buffer.len) };
    let additional = min_capacity.saturating_sub(bytes.len());
    if bytes.try_reserve(additional).is_err() {
        return 1;
    }
    buffer.data = bytes.as_mut_ptr();
    buffer.capacity = bytes.capacity();
    0
}

impl VecBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// A buffer that already holds `bytes`: oxiform appends after them.
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.bytes
    }

    /// Runs `call` with an [`OxBuffer`] over this buffer, then takes over the bytes it wrote.
    pub fn with_raw<R>(&mut self, call: impl FnOnce(*mut OxBuffer) -> R) -> R {
        let mut raw = OxBuffer {
            struct_size: size_of::<OxBuffer>() as u32,
            reserved: 0,
            data: self.bytes.as_mut_ptr(),
            len: self.bytes.len(),
            capacity: self.bytes.capacity(),
            reserve: Some(reserve_vec),
            user_data: core::ptr::from_mut(&mut self.bytes).cast::<c_void>(),
        };
        let result = call(&raw mut raw);
        if raw.len <= self.bytes.capacity() && raw.data == self.bytes.as_mut_ptr() {
            // SAFETY: oxiform initialized the first `raw.len` bytes of `data`, which is the
            // allocation of `bytes` (checked), with room for them (checked).
            unsafe { self.bytes.set_len(raw.len) };
        }
        result
    }
}
