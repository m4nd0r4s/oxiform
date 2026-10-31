//! Direct access to `__slots__` members, by the offset their descriptor records.

use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::PyType;

/// The byte offset of the `__slots__` member `name` of `cls`, found along its MRO, when the
/// attribute is a plain writable object slot.
pub(crate) fn slot_offset(cls: &Bound<'_, PyType>, name: &str) -> PyResult<Option<isize>> {
    let py = cls.py();
    let member_type = py.import("types")?.getattr("MemberDescriptorType")?;
    for klass in cls.getattr("__mro__")?.try_iter()? {
        let Some(descriptor) = klass?.getattr("__dict__")?.get_item(name).ok() else {
            continue;
        };
        if !descriptor.get_type().is(&member_type) {
            return Ok(None);
        }
        // SAFETY: `descriptor` is a member descriptor, whose C layout is `PyMemberDescrObject`;
        // it stays alive as long as the class, which every user of the offset keeps.
        let member =
            unsafe { &*(*descriptor.as_ptr().cast::<ffi::PyMemberDescrObject>()).d_member };
        let writable = member.flags & ffi::Py_READONLY == 0;
        let usable = member.type_code == ffi::Py_T_OBJECT_EX && writable && member.offset > 0;
        return Ok(usable.then_some(member.offset));
    }
    Ok(None)
}

/// The value in the slot at `offset` of `object`, or `None` when the slot is unset.
///
/// # Safety
///
/// `offset` must come from [`slot_offset`] for the exact type of `object` or one of its bases.
pub(crate) unsafe fn read_slot<'py>(
    object: &Bound<'py, PyAny>,
    offset: isize,
) -> Option<Bound<'py, PyAny>> {
    // SAFETY: the caller guarantees the offset is an object slot of this layout; the slot
    // holds a strong reference or NULL, and we take a new reference to it.
    unsafe {
        let slot = object
            .as_ptr()
            .cast::<u8>()
            .offset(offset)
            .cast::<*mut ffi::PyObject>();
        Bound::from_borrowed_ptr_or_opt(object.py(), *slot)
    }
}
