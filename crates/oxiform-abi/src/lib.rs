//! The embedding C ABI of oxiform.
//!
//! A native Python extension (Douro's `douro._native`, say) that wants to validate and serialize
//! with oxiform links **this crate and not oxiform's engine**: oxiform owns the plan cache and
//! every compiled type of the process (the single-owner rule: one copy of the statics, in the
//! `oxiform._oxiform` extension), and exports a table of functions through a `PyCapsule`
//! that the host finds at import. The crate holds the `#[repr(C)]` types, the version check and a
//! safe Rust layer over the table; it depends on nothing (and on PyO3 only with the `pyo3` feature,
//! for finding the capsule).
//!
//! # Versioning
//!
//! The table carries `abi_major`, `abi_minor` and its own `struct_size`. An incompatible change (a
//! field removed, moved or reinterpreted, a function that behaves differently) bumps the major
//! version and exports a new capsule (`_ABI_V2`); oxiform keeps exporting the capsule of the
//! previous major for at least two minor releases. A host built for major N refuses an oxiform
//! that does not export N, with an `ImportError` that says what to install. Additive changes (a
//! function at the end of the table, a field at the end of a size-prefixed struct, a new constant)
//! bump the minor version: a host built for minor M works with any oxiform that exports M or later,
//! and refuses an earlier one at import.
//!
//! # Ownership and lifetimes
//!
//! | Thing | Who allocates | Who frees | Valid for |
//! | --- | --- | --- | --- |
//! | input `data`, `source` strings, options, limits | the host | the host | the call: oxiform never keeps a pointer to them |
//! | the decoded object (`out_value`) | oxiform | the host: it holds one reference and decrements it with the interpreter attached | until released |
//! | an `OxPlan` | oxiform | the host: `plan_release` once per reference | until the last reference is released; the compiled type stays in the plan cache anyway |
//! | an `OxReport` and its views | oxiform | the host: `report_free` (or a successful `report_combine`, which consumes it) | until freed; `report_in_source` invalidates the views |
//! | output bytes | the host (`OxBuffer`) | the host | oxiform appends and never keeps `data`; it grows the buffer only through the host's `reserve` callback, so no memory crosses the allocator of two cdylibs |
//! | `PyObject*` arguments (`target`, `obj`) | the host | the host | borrowed for the call |
//! | static strings (`status_message`, `oxiform_version`, codes) | oxiform | nobody | forever |
//!
//! # Threads
//!
//! Functions marked *attached* in [`OxiformApi`] need the calling thread to be attached to the
//! interpreter (the GIL held, or a thread state in a free-threaded build such as 3.14t); inside an
//! extension function that is already so. They run Python code while they work (validators,
//! default factories), so the host's buffers must not change during the call, and they leave a
//! raised exception in the calling thread when they report one. Plans are immutable and shared
//! without locks: several threads may decode through one plan at once on a free-threaded build,
//! and each decode needs its own thread to be attached. Reports and `plan_retain`/`plan_release`
//! need no interpreter.

mod api;
mod buffer;
mod host;
mod prefixed;
mod types;
mod views;

#[cfg(feature = "pyo3")]
mod import;

pub use api::*;
pub use buffer::VecBuffer;
pub use host::*;
pub use prefixed::{SizePrefixed, TooSmall, read_prefixed};
pub use types::*;
pub use views::{ChoiceValue, ContextValue, IssueView, ReportView, Segment};

#[cfg(feature = "pyo3")]
pub use import::{import, import_version};

#[cfg(test)]
mod tests {
    use super::*;

    fn header(major: u32, minor: u32, size: usize) -> OxiformApiHeader {
        OxiformApiHeader {
            abi_major: major,
            abi_minor: minor,
            struct_size: size as u32,
            reserved: 0,
        }
    }

    #[test]
    fn the_same_version_is_accepted() {
        assert_eq!(check_header(header(1, 0, 400), 1, 0, 400), Ok(()));
    }

    #[test]
    fn a_newer_minor_and_a_longer_table_are_accepted() {
        assert_eq!(check_header(header(1, 3, 600), 1, 1, 400), Ok(()));
    }

    #[test]
    fn another_major_is_refused_and_the_message_says_what_to_install() {
        let error = check_header(header(2, 0, 400), 1, 0, 400).unwrap_err();
        assert_eq!(
            error,
            VersionError::Major {
                wanted: 1,
                available: vec![2]
            }
        );
        let message = error.to_string();
        assert!(message.contains("major version 1"), "{message}");
        assert!(message.contains("install an oxiform release"), "{message}");
    }

    #[test]
    fn an_older_minor_is_refused_with_both_versions() {
        let error = check_header(header(1, 0, 400), 1, 2, 400).unwrap_err();
        assert_eq!(
            error.to_string(),
            "this extension needs oxiform ABI 1.2 or newer, but the installed oxiform exports ABI 1.0; upgrade oxiform"
        );
    }

    #[test]
    fn a_table_shorter_than_the_hosts_is_refused() {
        let error = check_header(header(1, 5, 100), 1, 5, 400).unwrap_err();
        assert_eq!(
            error,
            VersionError::Size {
                needed: 400,
                found: 100
            }
        );
    }

    #[test]
    fn the_header_is_the_start_of_the_table() {
        assert_eq!(core::mem::offset_of!(OxiformApi, abi_major), 0);
        assert_eq!(core::mem::offset_of!(OxiformApi, abi_minor), 4);
        assert_eq!(core::mem::offset_of!(OxiformApi, struct_size), 8);
        assert_eq!(size_of::<OxiformApiHeader>(), 16);
    }

    #[test]
    fn the_capsule_names_follow_the_major_version() {
        assert_eq!(CAPSULE_ATTRIBUTE, format!("_ABI_V{ABI_MAJOR}"));
        assert_eq!(
            CAPSULE_NAME.to_str().unwrap(),
            format!("{MODULE}.{CAPSULE_ATTRIBUTE}")
        );
    }

    #[test]
    fn a_vec_buffer_grows_through_its_reserve_callback() {
        let mut buffer = VecBuffer::from_vec(b"head:".to_vec());
        buffer.with_raw(|raw| {
            // SAFETY: `raw` is a valid buffer from `with_raw`; this does what oxiform does.
            unsafe {
                let reserve = (*raw).reserve.unwrap();
                assert_eq!(reserve(raw, 5 + 4096), 0);
                assert!((*raw).capacity >= 5 + 4096);
                core::ptr::copy_nonoverlapping(b"body".as_ptr(), (*raw).data.add((*raw).len), 4);
                (*raw).len += 4;
            }
        });
        assert_eq!(buffer.as_slice(), b"head:body");
    }
}
