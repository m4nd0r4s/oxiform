//! Appending to the buffer of the host.

use oxiform_abi::{OX_BUFFER, OX_INVALID_ARGUMENT, OxBuffer};

use crate::Error;

/// Checks that `buffer` can be appended to: not null, of at least the size of ABI 1.0, with
/// `len <= capacity`. A call checks it before it does any work, so a bad buffer costs nothing.
///
/// # Safety
///
/// As [`append`].
pub unsafe fn check_buffer(buffer: *mut OxBuffer) -> Result<(), Error> {
    if buffer.is_null() {
        return Err(Error::new(
            OX_INVALID_ARGUMENT,
            "buffer is NULL; pass an OxBuffer with struct_size set",
        ));
    }
    // SAFETY: the caller guarantees `buffer` is readable; `struct_size` is its first field. Fields
    // are read through the raw pointer, never through a reference, because `reserve` may change
    // them.
    let given = unsafe { (&raw const (*buffer).struct_size).read() } as usize;
    if given < OxBuffer::SIZE_V1_0 {
        return Err(Error::new(
            OX_INVALID_ARGUMENT,
            format!(
                "buffer.struct_size is {given}, less than the {} bytes of ABI 1.0: set struct_size \
                 to size_of::<OxBuffer>()",
                OxBuffer::SIZE_V1_0
            ),
        ));
    }
    // SAFETY: `given >= SIZE_V1_0`, so the fields of ABI 1.0 are readable.
    let (len, capacity) = unsafe {
        (
            (&raw const (*buffer).len).read(),
            (&raw const (*buffer).capacity).read(),
        )
    };
    if len > capacity {
        return Err(Error::new(
            OX_INVALID_ARGUMENT,
            format!("buffer.len ({len}) is greater than buffer.capacity ({capacity})"),
        ));
    }
    Ok(())
}

/// Appends `bytes` to the host's `buffer`, growing it through its `reserve` callback.
///
/// The buffer is touched only when the bytes fit, or after `reserve` succeeded; on any error it is
/// as it was (but for what `reserve` itself did, which keeps the `len` bytes).
///
/// # Safety
///
/// `buffer` must be null (an error) or point to a readable and writable `OxBuffer` whose `data`
/// points to `capacity` bytes, the first `len` initialized, and `reserve` must keep that contract.
pub unsafe fn append(buffer: *mut OxBuffer, bytes: &[u8]) -> Result<(), Error> {
    // SAFETY: forwarded from the caller.
    unsafe { check_buffer(buffer) }?;
    // SAFETY: `check_buffer` passed, so the fields of ABI 1.0 are readable.
    let (len, capacity) = unsafe {
        (
            (&raw const (*buffer).len).read(),
            (&raw const (*buffer).capacity).read(),
        )
    };
    let needed = len
        .checked_add(bytes.len())
        .ok_or_else(|| Error::new(OX_BUFFER, "the output does not fit in the address space"))?;
    if bytes.is_empty() {
        return Ok(());
    }
    if capacity < needed {
        // SAFETY: as above.
        let reserve = unsafe { (&raw const (*buffer).reserve).read() };
        let Some(reserve) = reserve else {
            return Err(Error::new(
                OX_BUFFER,
                format!(
                    "the buffer is fixed at {capacity} bytes with {len} used and the output needs \
                     {} more; set buffer.reserve to a callback that grows it, or give it more room",
                    bytes.len()
                ),
            ));
        };
        // SAFETY: the host provided `reserve` for exactly this call with this buffer.
        let result = unsafe { reserve(buffer, needed) };
        if result != 0 {
            return Err(Error::new(
                OX_BUFFER,
                format!(
                    "the reserve callback of the buffer failed (it returned {result}) when asked \
                     for {needed} bytes; the output was not written"
                ),
            ));
        }
    }
    // SAFETY: the fields of ABI 1.0 are readable (checked above); `reserve` may have changed them.
    let (data, len_now, capacity_now) = unsafe {
        (
            (&raw const (*buffer).data).read(),
            (&raw const (*buffer).len).read(),
            (&raw const (*buffer).capacity).read(),
        )
    };
    if data.is_null() || len_now != len || capacity_now < needed {
        return Err(Error::new(
            OX_BUFFER,
            "the reserve callback of the buffer broke its contract: it must leave len as it was, \
             set capacity to at least the size asked for and data to a block of that capacity",
        ));
    }
    // SAFETY: `data` points to `capacity_now >= needed` bytes (the host's contract, which `reserve`
    // just restated), so `len..needed` is writable, and `bytes` is a slice of ours that cannot
    // overlap the host's block.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), data.add(len), bytes.len());
        (&raw mut (*buffer).len).write(needed);
    }
    Ok(())
}
