//! Reading the size-prefixed structs a host passes.

/// Why a size-prefixed struct was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooSmall {
    pub given: usize,
    pub needed: usize,
}

/// A struct whose first field is `struct_size: u32`.
///
/// # Safety
///
/// Implementors are `#[repr(C)]` with `struct_size: u32` as the first field, and every bit pattern of
/// every field is valid (integers, floats, raw pointers and `Option` of function pointers only).
pub unsafe trait SizePrefixed: Copy {
    /// The size of the struct in ABI 1.0, the least a caller may pass.
    const SIZE_V1_0: usize;

    /// The defaults a field the caller does not know of keeps.
    fn defaults() -> Self;
}

/// Reads the struct `pointer` points to: the `struct_size` bytes the caller gave, on top of the
/// defaults of the fields it did not know. `struct_size` of the result is the size given.
///
/// # Safety
///
/// `pointer` must be non-null, aligned for `T`, and its first four bytes must be readable, and
/// then `struct_size` bytes (the caller's claim, which this checks only against the minimum) must
/// be readable.
pub unsafe fn read_prefixed<T: SizePrefixed>(pointer: *const T) -> Result<T, TooSmall> {
    // SAFETY: the caller guarantees the first four bytes, the `u32` `struct_size`, are readable
    // and `pointer` is aligned (which a `u32` at offset 0 is too).
    let given = unsafe { pointer.cast::<u32>().read() } as usize;
    if given < T::SIZE_V1_0 {
        return Err(TooSmall {
            given,
            needed: T::SIZE_V1_0,
        });
    }
    let mut value = T::defaults();
    let count = given.min(size_of::<T>());
    // SAFETY: `count` bytes are readable at `pointer` (the caller's claim of `struct_size`, capped
    // at the size of `T`), `value` is a local of at least `count` bytes, they cannot overlap, and
    // every bit pattern is valid for `T` (`SizePrefixed`).
    unsafe {
        core::ptr::copy_nonoverlapping(
            pointer.cast::<u8>(),
            core::ptr::from_mut(&mut value).cast::<u8>(),
            count,
        );
    }
    Ok(value)
}

macro_rules! prefixed {
    ($type:ty, $defaults:expr) => {
        // SAFETY: `#[repr(C)]`, `struct_size: u32` first, and only integers, floats, raw
        // pointers and `Option` of function pointers as fields.
        unsafe impl SizePrefixed for $type {
            const SIZE_V1_0: usize = <$type>::SIZE_V1_0;
            fn defaults() -> Self {
                $defaults
            }
        }
    };
}

prefixed!(crate::OxLimits, crate::OxLimits::zeroed());
prefixed!(crate::OxDecodeOptions, crate::OxDecodeOptions::new());
prefixed!(crate::OxEncodeOptions, crate::OxEncodeOptions::new());
prefixed!(
    crate::OxBuffer,
    crate::OxBuffer {
        struct_size: size_of::<crate::OxBuffer>() as u32,
        reserved: 0,
        data: core::ptr::null_mut(),
        len: 0,
        capacity: 0,
        reserve: None,
        user_data: core::ptr::null_mut(),
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OxDecodeOptions, OxLimits};

    #[test]
    fn reads_a_struct_of_the_size_it_has() {
        let mut options = OxDecodeOptions::new();
        options.mode = crate::OX_MODE_LAX;
        options.max_errors = 7;
        // SAFETY: a valid, aligned struct whose `struct_size` is its size.
        let read = unsafe { read_prefixed(&raw const options) }.unwrap();
        assert_eq!((read.mode, read.max_errors), (crate::OX_MODE_LAX, 7));
    }

    #[test]
    fn a_struct_longer_than_known_is_read_up_to_the_known_part() {
        #[repr(C)]
        struct Future {
            known: OxLimits,
            added_later: [u64; 4],
        }
        let mut known = OxLimits::zeroed();
        known.max_items = 9;
        let future = Future {
            known,
            added_later: [u64::MAX; 4],
        };
        let mut future = future;
        future.known.struct_size = size_of::<Future>() as u32;
        // SAFETY: the struct starts with a valid `OxLimits` and `struct_size` is its real size.
        let read = unsafe { read_prefixed(&raw const future.known) }.unwrap();
        assert_eq!(read.max_items, 9);
        assert_eq!(read.struct_size as usize, size_of::<Future>());
    }

    #[test]
    fn a_struct_shorter_than_abi_1_0_is_refused() {
        let mut options = OxDecodeOptions::new();
        options.struct_size = (OxDecodeOptions::SIZE_V1_0 - 1) as u32;
        // SAFETY: a valid, aligned struct; only its first four bytes are trusted before the check.
        let error = unsafe { read_prefixed(&raw const options) }.unwrap_err();
        assert_eq!(error.needed, OxDecodeOptions::SIZE_V1_0);
    }
}
