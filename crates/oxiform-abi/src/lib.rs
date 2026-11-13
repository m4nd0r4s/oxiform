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
//! | input `data`, `source` strings, options, limits | the host | the host | the call: oxiform never keeps a pointer to them. `data` must not change during the decode (validators run Python): pass owned or immutable bytes |
//! | the decoded object (`out_value`) | oxiform | the host: it holds one reference and decrements it with the interpreter attached | until released |
//! | an `OxPlan` | oxiform | the host: `plan_release` once per reference | until the last reference is released; the compiled type stays in the plan cache anyway. Release plans before the interpreter shuts down: afterwards `plan_release` leaks the handle |
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

    #[test]
    fn arrays_are_walked_by_their_strides_and_unknown_kinds_are_skipped() {
        // What a later minor could return: every struct longer than this host knows, with an
        // unknown kind of path segment, context value and choice value among the known ones.
        #[repr(C)]
        struct Wide<T> {
            base: T,
            added_later: [u64; 3],
        }
        fn wide<T>(base: T) -> Wide<T> {
            Wide {
                base,
                added_later: [u64::MAX; 3],
            }
        }
        let segment = |kind, index, key: &'static str| OxPathSegment {
            kind,
            reserved: 0,
            index,
            key: OxStr::from_str(key),
        };
        let segments = [
            wide(segment(OX_SEGMENT_KEY, 0, "a")),
            wide(segment(99, 0, "from the future")),
            wide(segment(OX_SEGMENT_INDEX, 7, "")),
        ];
        let choices = [
            wide(OxChoiceValue {
                kind: OX_CHOICE_INTEGER,
                reserved: 0,
                integer: 4,
                text: OxStr::EMPTY,
            }),
            wide(OxChoiceValue {
                kind: 99,
                reserved: 0,
                integer: 0,
                text: OxStr::EMPTY,
            }),
            wide(OxChoiceValue {
                kind: OX_CHOICE_STRING,
                reserved: 0,
                integer: 0,
                text: OxStr::from_str("s"),
            }),
        ];
        let value = |kind, integer, items: *const core::ffi::c_void, len| OxContextValue {
            kind,
            reserved: 0,
            integer,
            float: 0.0,
            text: OxStr::EMPTY,
            items,
            len,
        };
        let entries = [
            wide(OxContextEntry {
                key: OxStr::from_str("n"),
                value: value(OX_CTX_INTEGER, 5, core::ptr::null(), 0),
            }),
            wide(OxContextEntry {
                key: OxStr::from_str("future"),
                value: value(99, 0, core::ptr::null(), 0),
            }),
            wide(OxContextEntry {
                key: OxStr::from_str("allowed"),
                value: value(OX_CTX_VALUES, 0, choices.as_ptr().cast(), choices.len()),
            }),
        ];
        let issue = wide(OxIssue {
            code: OxStr::from_str("literal"),
            message: OxStr::from_str("m"),
            pointer: OxStr::from_str("/a/7"),
            input: OxStr::EMPTY,
            has_input: 0,
            reserved: 0,
            path: segments.as_ptr().cast(),
            path_len: segments.len(),
            context: entries.as_ptr().cast(),
            context_len: entries.len(),
        });
        let raw = OxReportView {
            struct_size: size_of::<OxReportView>() as u32,
            kind: OX_REPORT_VALIDATION,
            truncated: 0,
            issue_size: size_of::<Wide<OxIssue>>() as u32,
            segment_size: size_of::<Wide<OxPathSegment>>() as u32,
            entry_size: size_of::<Wide<OxContextEntry>>() as u32,
            variant_size: size_of::<Wide<OxVariant>>() as u32,
            choice_size: size_of::<Wide<OxChoiceValue>>() as u32,
            issues: core::ptr::from_ref(&issue).cast(),
            issue_count: 1,
        };
        // SAFETY: `raw` describes arrays that live through this test, with the strides given.
        let view = unsafe { ReportView::from_raw(&raw) };
        let issue = view.issues().next().unwrap();
        assert_eq!(
            issue.path().collect::<Vec<_>>(),
            [Segment::Key("a"), Segment::Index(7)]
        );
        let context: Vec<_> = issue.context().collect();
        assert_eq!(context.len(), 2);
        assert_eq!(context[0], ("n", ContextValue::Integer(5)));
        assert_eq!(
            context[1],
            (
                "allowed",
                ContextValue::Values(vec![ChoiceValue::Integer(4), ChoiceValue::String("s")])
            )
        );
    }
}
