//! The function table, and the check a host makes before it uses one.

use core::ffi::CStr;
use core::fmt;

use crate::types::{
    OxBuffer, OxDecodeOptions, OxEncodeOptions, OxLimits, OxPair, OxPairsOptions, OxPlan,
    OxPyObject, OxReport, OxReportPart, OxReportView, OxStr,
};

/// Incompatible changes bump the major version: a host built for major N refuses another.
pub const ABI_MAJOR: u32 = 1;
/// Additive changes (a new function at the end of the table, a new field at the end of a struct)
/// bump the minor version. A host built for minor M needs an oxiform that exports at least M.
pub const ABI_MINOR: u32 = 1;

/// The module that exports the capsules.
pub const MODULE: &str = "oxiform._oxiform";
/// The capsule of this major version, an attribute of [`MODULE`].
pub const CAPSULE_ATTRIBUTE: &str = "_ABI_V1";
/// The name of the capsule of this major version, as `PyCapsule_GetPointer` wants it.
pub const CAPSULE_NAME: &CStr = c"oxiform._oxiform._ABI_V1";
/// The attribute of [`MODULE`] that lists every major version the installed oxiform exports.
pub const MAJORS_ATTRIBUTE: &str = "_ABI_MAJORS";

/// The first four fields of every version of the table, in this order, so a host can read the
/// version before it knows how long the table is.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxiformApiHeader {
    pub abi_major: u32,
    pub abi_minor: u32,
    /// The size in bytes of the table the capsule points to.
    pub struct_size: u32,
    pub reserved: u32,
}

/// The function table of ABI 1, exported through the capsule `oxiform._oxiform._ABI_V1`.
///
/// **Calling conventions.** Functions marked *attached* run Python code or touch Python objects:
/// the calling thread must be attached to the interpreter (hold the GIL, or be a thread of
/// a free-threaded build with a thread state), which is the normal state inside an extension
/// function. They leave a raised exception in the calling thread's state, which the host fetches,
/// when they return [`crate::OX_PYTHON_ERROR`] (and for the other errors that say so). The others
/// need no interpreter and may be called from any thread.
///
/// **Panics** never cross the boundary: a panic inside oxiform is caught and returned as
/// [`crate::OX_INTERNAL`].
#[repr(C)]
pub struct OxiformApi {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub struct_size: u32,
    pub reserved: u32,
    /// The version of the oxiform package that exports this table, `"0.0.1"`. Static.
    pub oxiform_version: OxStr,

    /// A sentence that says what a status code means and what to do. Static, never null.
    pub status_message: unsafe extern "C" fn(status: i32) -> OxStr,

    /// Fills `*out` (whose `struct_size` the caller set) with the default limits.
    pub limits_default: unsafe extern "C" fn(out: *mut OxLimits) -> i32,
    /// Fills `*out` with no size limits (nesting still stops at 128 levels).
    pub limits_none: unsafe extern "C" fn(out: *mut OxLimits) -> i32,

    /// *Attached.* The plan of `target` (any type expression: a class, `list[User]`, an
    /// `Annotated`, ...), compiled on first use and shared with `ox.json.decode`: the process has one
    /// plan cache, in oxiform. `target` is borrowed. On [`crate::OX_OK`], `*out` is one reference
    /// to release with `plan_release`. A declaration error is a `SchemaError`
    /// ([`crate::OX_PYTHON_ERROR`]).
    pub plan_for: unsafe extern "C" fn(target: *mut OxPyObject, out: *mut *mut OxPlan) -> i32,
    /// Adds a reference to `plan`. Any thread.
    pub plan_retain: unsafe extern "C" fn(plan: *const OxPlan),
    /// Drops a reference. Any thread (it attaches itself when it must free the last one); null is a
    /// no-op.
    pub plan_release: unsafe extern "C" fn(plan: *const OxPlan),
    /// The type the plan was made for, borrowed: valid while the caller holds a reference to the
    /// plan. Any thread, but using the object needs the interpreter.
    pub plan_target: unsafe extern "C" fn(plan: *const OxPlan) -> *mut OxPyObject,

    /// *Attached.* Reads one document of `format` from `data[..len]` as the type of `plan`, with
    /// `options` (null: the defaults).
    ///
    /// `data` is borrowed for the call and may be freed after it: the result never refers to it. It
    /// must not change during the call, and need not be valid UTF-8 or NUL-terminated (`len == 0`
    /// may come with a null `data`). On [`crate::OX_OK`], `*out_value` is a new reference to the
    /// decoded object, which the host owns. On [`crate::OX_INVALID`], `*out_report` is a report the
    /// host owns and must free with `report_free`. `out_value` and `out_report` are set to null
    /// first.
    pub decode: unsafe extern "C" fn(
        plan: *const OxPlan,
        format: u32,
        data: *const u8,
        len: usize,
        options: *const OxDecodeOptions,
        out_value: *mut *mut OxPyObject,
        out_report: *mut *mut OxReport,
    ) -> i32,

    /// *Attached.* Writes `obj` as `format` onto the end of `buffer`. With a `plan`, `obj` is
    /// written as the declared type (OX-2): the fields of the type, whatever else the class of `obj`
    /// has; with a null `plan`, as the class of the value. `obj` and `plan` are borrowed. The
    /// output is built aside and appended once, so a failed call (an `EncodeError` is
    /// [`crate::OX_PYTHON_ERROR`]) leaves the buffer as it was.
    pub encode: unsafe extern "C" fn(
        format: u32,
        obj: *mut OxPyObject,
        plan: *const OxPlan,
        options: *const OxEncodeOptions,
        buffer: *mut OxBuffer,
    ) -> i32,

    /// The issues of `report`: valid until the report is freed or rewritten. Any thread.
    pub report_view: unsafe extern "C" fn(report: *const OxReport) -> *const OxReportView,
    /// Makes every issue of `report` an issue found in `source` (`"body"`, `"query"`, ...): the
    /// source becomes the first segment of every path and pointer (OX-4). `source` must not be
    /// empty. The views of `report` are invalid afterwards: call `report_view` again.
    pub report_in_source: unsafe extern "C" fn(report: *mut OxReport, source: OxStr) -> i32,
    /// One report for the parts of a request, `parts[i].report` found in `parts[i].source` (OX-4),
    /// in order of priority: at most `max_errors` issues are kept (0 is 100), the first ones. On
    /// [`crate::OX_OK`] every part's report is consumed (do not free them) and `*out` is the new
    /// report; on any error nothing is consumed.
    pub report_combine: unsafe extern "C" fn(
        parts: *const OxReportPart,
        count: usize,
        max_errors: u64,
        out: *mut *mut OxReport,
    ) -> i32,
    /// Frees a report. Any thread; null is a no-op.
    pub report_free: unsafe extern "C" fn(report: *mut OxReport),

    // --- ABI 1.1: everything below was added at the end of the table. A host built for 1.0 never
    // reads past `report_free`, and a host built for 1.1 refuses an oxiform that exports 1.0.
    /// *Attached.* Reads a source of pairs (OX-3) as the model of `plan`, in lax mode, with
    /// `options` (null: the defaults).
    ///
    /// With [`crate::OX_FORMAT_URLENCODED`] the source is the query string or form body
    /// `data[..len]` (`%XX` and `+` decoded, UTF-8 strictly; `pairs` must be null and `pair_count`
    /// 0). With [`crate::OX_FORMAT_PAIRS`] it is the `pair_count` pairs at `pairs`, read as they
    /// are (headers, cookies; `data` must be null and `len` 0). A name that is repeated gives its
    /// values in order to a field that is a list, a tuple or a set, and is a `duplicate_key` issue
    /// for any other field. `plan` must be the plan of a model: anything else is
    /// [`crate::OX_INVALID_ARGUMENT`]. Memory, results and statuses are those of `decode`;
    /// the issues name the wire name of the field (`/page`), ready for `report_in_source`.
    pub decode_pairs: unsafe extern "C" fn(
        plan: *const OxPlan,
        format: u32,
        data: *const u8,
        len: usize,
        pairs: *const OxPair,
        pair_count: usize,
        options: *const OxPairsOptions,
        out_value: *mut *mut OxPyObject,
        out_report: *mut *mut OxReport,
    ) -> i32,
}

/// The size of the table in ABI 1.0, up to and including `report_free`: the least a table can be
/// for a host built for 1.0 (which is also the size of the 1.0 table, so that table is a prefix
/// of this one).
pub const API_SIZE_V1_0: usize = core::mem::offset_of!(OxiformApi, report_free)
    + size_of::<unsafe extern "C" fn(*mut OxReport)>();

/// Why a host cannot use the installed oxiform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    /// The installed oxiform exports other majors only.
    Major { wanted: u32, available: Vec<u32> },
    /// The installed oxiform has the major version, but an earlier minor.
    Minor {
        wanted: (u32, u32),
        found: (u32, u32),
    },
    /// The table is shorter than the host's version of it.
    Size { needed: u32, found: u32 },
}

impl fmt::Display for VersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Major { wanted, available } => {
                let available = available
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    formatter,
                    "this extension was built for oxiform ABI major version {wanted}, but the \
                     installed oxiform exports ABI major version(s) [{available}]; install an \
                     oxiform release that exports ABI {wanted}, or rebuild this extension against \
                     the oxiform-abi crate of the installed release"
                )
            }
            Self::Minor { wanted, found } => write!(
                formatter,
                "this extension needs oxiform ABI {}.{} or newer, but the installed oxiform \
                 exports ABI {}.{}; upgrade oxiform",
                wanted.0, wanted.1, found.0, found.1
            ),
            Self::Size { needed, found } => write!(
                formatter,
                "the oxiform ABI table is {found} bytes long and this extension reads {needed}; \
                 upgrade oxiform"
            ),
        }
    }
}

impl std::error::Error for VersionError {}

/// Checks the header of a table against the version a host was built for: the same major, a
/// minor at least as new, and a table at least `table_size` long (the size of the host's
/// version of [`OxiformApi`]).
pub fn check_header(
    header: OxiformApiHeader,
    wanted_major: u32,
    wanted_minor: u32,
    table_size: usize,
) -> Result<(), VersionError> {
    if header.abi_major != wanted_major {
        // Reached when the capsule of the wanted major is present but says otherwise.
        return Err(VersionError::Major {
            wanted: wanted_major,
            available: vec![header.abi_major],
        });
    }
    if header.abi_minor < wanted_minor {
        return Err(VersionError::Minor {
            wanted: (wanted_major, wanted_minor),
            found: (header.abi_major, header.abi_minor),
        });
    }
    let needed = u32::try_from(table_size).unwrap_or(u32::MAX);
    if header.struct_size < needed {
        return Err(VersionError::Size {
            needed,
            found: header.struct_size,
        });
    }
    Ok(())
}

/// The table behind a capsule pointer, after checking its version.
///
/// # Safety
///
/// `pointer` must point to the table of an oxiform that exports the capsule of the wanted major
/// version (its first fields are an [`OxiformApiHeader`], and `struct_size` bytes are readable),
/// and the table must stay alive for the whole process, as the one of oxiform does.
pub unsafe fn table_from_pointer(
    pointer: *const OxiformApiHeader,
    wanted_major: u32,
    wanted_minor: u32,
) -> Result<&'static OxiformApi, VersionError> {
    // SAFETY: the caller guarantees `pointer` points to at least a header.
    let header = unsafe { pointer.read() };
    check_header(header, wanted_major, wanted_minor, size_of::<OxiformApi>())?;
    // SAFETY: the header says `struct_size >= size_of::<OxiformApi>()` bytes are readable, the
    // table has the layout of `OxiformApi` up to there for the same major version, and the
    // caller guarantees it lives for the whole process.
    Ok(unsafe { &*pointer.cast::<OxiformApi>() })
}
