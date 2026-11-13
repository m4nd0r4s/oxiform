//! A buffer of the host with a chosen behaviour, to show what oxiform does with each.
//!
//! The memory is allocated and freed here, by the allocator of this module: oxiform only writes
//! into it, and asks for more room through the `reserve` callback.

use core::ffi::c_void;

use oxiform_abi::OxBuffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferMode {
    /// `reserve` grows the buffer, as a `Vec<u8>` does.
    Grow,
    /// No `reserve`: what does not fit is an error.
    Fixed,
    /// `reserve` always fails.
    Refuse,
    /// `reserve` says yes and does nothing: it breaks the contract.
    Lie,
}

struct State {
    raw: OxBuffer,
    storage: Vec<u8>,
    mode: BufferMode,
    calls: usize,
}

/// The state is boxed: the `OxBuffer` and the `user_data` that points back to it keep their
/// address while oxiform calls into it.
pub struct HostBuffer {
    state: Box<State>,
}

unsafe extern "C" fn reserve(buffer: *mut OxBuffer, min_capacity: usize) -> i32 {
    // SAFETY: oxiform calls `reserve` with the buffer this module gave it, whose `user_data` is
    // the `State` that owns it, boxed and alive for the whole call.
    let state = unsafe { &mut *(*buffer).user_data.cast::<State>() };
    state.calls += 1;
    match state.mode {
        BufferMode::Refuse => 5,
        BufferMode::Lie | BufferMode::Fixed => 0,
        BufferMode::Grow => {
            // SAFETY: `raw.len <= capacity == storage.capacity()` and the first `len` bytes are
            // initialized (by us, or by oxiform before it asked for more room).
            unsafe { state.storage.set_len(state.raw.len) };
            let additional = min_capacity.saturating_sub(state.storage.len());
            if state.storage.try_reserve(additional).is_err() {
                return 1;
            }
            state.raw.data = state.storage.as_mut_ptr();
            state.raw.capacity = state.storage.capacity();
            0
        }
    }
}

impl HostBuffer {
    /// A buffer with `capacity` bytes (at least those of `prefix`), holding `prefix`.
    pub fn new(mode: BufferMode, capacity: usize, prefix: &[u8]) -> Self {
        let capacity = capacity.max(prefix.len());
        let storage = if mode == BufferMode::Grow {
            let mut storage = Vec::with_capacity(capacity);
            storage.extend_from_slice(prefix);
            storage
        } else {
            let mut storage = vec![0; capacity];
            storage[..prefix.len()].copy_from_slice(prefix);
            storage
        };
        let mut state = Box::new(State {
            raw: OxBuffer {
                struct_size: size_of::<OxBuffer>() as u32,
                reserved: 0,
                data: core::ptr::null_mut(),
                len: prefix.len(),
                capacity: storage.capacity(),
                reserve: if mode == BufferMode::Fixed {
                    None
                } else {
                    Some(reserve)
                },
                user_data: core::ptr::null_mut(),
            },
            storage,
            mode,
            calls: 0,
        });
        state.raw.data = state.storage.as_mut_ptr();
        state.raw.user_data = core::ptr::from_mut::<State>(&mut state).cast::<c_void>();
        Self { state }
    }

    /// The buffer struct to pass to oxiform.
    pub fn raw(&mut self) -> *mut OxBuffer {
        &raw mut self.state.raw
    }

    /// How many times oxiform asked for room.
    pub fn reserve_calls(&self) -> usize {
        self.state.calls
    }

    /// The bytes in the buffer: the prefix, and what oxiform appended.
    pub fn written(&self) -> &[u8] {
        let raw = &self.state.raw;
        if raw.data.is_null() || raw.len > raw.capacity {
            return &[];
        }
        // SAFETY: `data` points to `capacity >= len` bytes of `storage`, the first `len`
        // initialized, and nothing changes them while the returned slice is borrowed from `self`.
        unsafe { core::slice::from_raw_parts(raw.data, raw.len) }
    }
}
