//! The `#[repr(C)]` types of the ABI.
//!
//! Layout rules (section 9.5 of the specification):
//!
//! - Structs a host fills in and passes to oxiform ([`OxDecodeOptions`], [`OxLimits`],
//!   [`OxEncodeOptions`], [`OxBuffer`]) are **size-prefixed**: their first field is `struct_size`,
//!   the `size_of` of the struct the host was built against. New fields are only ever appended, and
//!   oxiform reads only the `struct_size` bytes it was given (a field the host did not know of keeps
//!   its default), so an older host runs on a newer oxiform without changes.
//! - Structs oxiform returns carry the stride of every array of structs they point to
//!   ([`OxReportView`]: `issue_size`, `segment_size`, `entry_size`, `variant_size`,
//!   `choice_size`), so a later minor can append fields to [`OxIssue`], [`OxPathSegment`],
//!   [`OxContextEntry`] (and so [`OxContextValue`], its last field), [`OxVariant`] and
//!   [`OxChoiceValue`] and a host built for an earlier one still walks the arrays: step by the
//!   stride, read only the fields you know. [`OxStr`] is frozen within a major version.
//! - A host must **skip** a `kind` it does not know (`OxContextValue::kind`,
//!   `OxChoiceValue::kind`, `OxPathSegment::kind`): a later minor may add kinds, and an entry of an
//!   unknown kind is not an error.
//! - Enumerations are plain `u32` constants and flags are `u32` bit sets, never Rust enums or
//!   `bool`, so an unknown value from a newer peer is detected, not undefined behavior.

use core::ffi::c_void;
use core::marker::{PhantomData, PhantomPinned};

macro_rules! opaque {
    ($($(#[$meta:meta])* $name:ident),* $(,)?) => {$(
        $(#[$meta])*
        #[repr(C)]
        pub struct $name {
            _data: [u8; 0],
            _marker: PhantomData<(*mut u8, PhantomPinned)>,
        }
    )*};
}

opaque! {
    /// A `PyObject` (`pyo3::ffi::PyObject`, `PyObject*` in C).
    OxPyObject,
    /// A compiled type: oxiform's plan of one Python type, shared with the plan cache of
    /// `ox.json.decode`. Reference counted: [`OxiformApi::plan_for`] returns one reference,
    /// `plan_retain` adds one and `plan_release` drops one. Thread safe (`Send + Sync`).
    OxPlan,
    /// The issues of one failed decode, owned by oxiform until `report_free`.
    OxReport,
}

// --- status codes -----------------------------------------------------------------------------

/// The call succeeded.
pub const OX_OK: i32 = 0;
/// A decode found the input invalid: `out_report` holds the issues. Only `decode` returns it.
pub const OX_INVALID: i32 = 1;
/// A Python exception was raised, and is set in the thread state of the caller (fetch it with
/// `PyErr_Fetch`/`PyErr::fetch`): a `SchemaError` from `plan_for`, an `EncodeError`, an exception
/// of a validator or default factory.
pub const OX_PYTHON_ERROR: i32 = 2;
/// An argument is wrong: a null pointer, a value out of range, a `struct_size` that is too small,
/// a source that is empty. The call did nothing. Calls that run with the interpreter also set a
/// `ValueError` that says what to pass.
pub const OX_INVALID_ARGUMENT: i32 = 3;
/// The call asks for something this oxiform does not have: a format, a flag or a context that
/// belongs to a later minor version. Calls that run with the interpreter also set an exception.
pub const OX_UNSUPPORTED: i32 = 4;
/// The output buffer could not make room: its `reserve` callback failed, is missing and the buffer
/// is full, or broke its contract. The buffer is unchanged.
pub const OX_BUFFER: i32 = 5;
/// A bug in oxiform (a panic was caught at the boundary). Report it.
pub const OX_INTERNAL: i32 = 6;

// --- enumerations and flags -------------------------------------------------------------------

/// The only format `decode` and `encode` read and write.
pub const OX_FORMAT_JSON: u32 = 1;
/// A source of `decode_pairs` (ABI 1.1): a raw query string or an
/// `application/x-www-form-urlencoded` body in `data[..len]`, `%XX` and `+` decoded.
pub const OX_FORMAT_URLENCODED: u32 = 2;
/// A source of `decode_pairs` (ABI 1.1): `pair_count` [`OxPair`]s, such as headers or cookies, read
/// as they are (no percent-decoding).
pub const OX_FORMAT_PAIRS: u32 = 3;

/// `OxPairsOptions::flags` (ABI 1.1): match names without regard to ASCII case, as header names are
/// matched.
pub const OX_PAIRS_CASE_INSENSITIVE: u32 = 1;

/// The input must already have the type (`"42"` is not an `int`).
pub const OX_MODE_STRICT: u32 = 0;
/// Text is read as the type the field asks for (`"42"` is an `int`).
pub const OX_MODE_LAX: u32 = 1;

/// Each model decides what to do with keys that match no field.
pub const OX_UNKNOWN_MODEL: u32 = 0;
/// Unknown keys are skipped, whatever the models say (OX-9).
pub const OX_UNKNOWN_IGNORE: u32 = 1;
/// Unknown keys are an `unknown_field` issue, whatever the models say (OX-9).
pub const OX_UNKNOWN_FORBID: u32 = 2;

/// `OxEncodeOptions::flags`: write the keys of every map in order.
pub const OX_ENCODE_SORT_KEYS: u32 = 1;
/// `OxEncodeOptions::flags`: indent by `OxEncodeOptions::indent` spaces.
pub const OX_ENCODE_INDENT: u32 = 2;

/// `OxReportView::kind`: the input was read to the end and did not match the type.
pub const OX_REPORT_VALIDATION: u32 = 0;
/// `OxReportView::kind`: reading stopped (malformed input, or a limit was exceeded).
pub const OX_REPORT_DECODE: u32 = 1;

/// `OxPathSegment::kind`: a key of a map, as written in the input.
pub const OX_SEGMENT_KEY: u32 = 0;
/// `OxPathSegment::kind`: an index into a sequence.
pub const OX_SEGMENT_INDEX: u32 = 1;

/// `OxContextValue::kind`: `integer` is 0 or 1.
pub const OX_CTX_BOOL: u32 = 0;
/// `OxContextValue::kind`: `integer`.
pub const OX_CTX_INTEGER: u32 = 1;
/// `OxContextValue::kind`: `float`.
pub const OX_CTX_FLOAT: u32 = 2;
/// `OxContextValue::kind`: `text`.
pub const OX_CTX_TEXT: u32 = 3;
/// `OxContextValue::kind`: `items` points to `len` `i64`.
pub const OX_CTX_INTEGERS: u32 = 4;
/// `OxContextValue::kind`: `items` points to `len` [`OxStr`].
pub const OX_CTX_TEXTS: u32 = 5;
/// `OxContextValue::kind`: `items` points to `len` [`OxChoiceValue`].
pub const OX_CTX_VALUES: u32 = 6;
/// `OxContextValue::kind`: `items` points to `len` [`OxVariant`].
pub const OX_CTX_VARIANTS: u32 = 7;

/// `OxChoiceValue::kind`.
pub const OX_CHOICE_NULL: u32 = 0;
/// `OxChoiceValue::kind`: `integer` is 0 or 1.
pub const OX_CHOICE_BOOL: u32 = 1;
/// `OxChoiceValue::kind`: `integer`.
pub const OX_CHOICE_INTEGER: u32 = 2;
/// `OxChoiceValue::kind`: `text`.
pub const OX_CHOICE_STRING: u32 = 3;

// --- strings ----------------------------------------------------------------------------------

/// A UTF-8 string that is not NUL-terminated. Whoever holds the `OxStr` does not own the bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxStr {
    pub ptr: *const u8,
    pub len: usize,
}

impl OxStr {
    pub const EMPTY: Self = Self {
        ptr: core::ptr::NonNull::<u8>::dangling().as_ptr(),
        len: 0,
    };

    pub const fn from_str(text: &str) -> Self {
        Self {
            ptr: text.as_ptr(),
            len: text.len(),
        }
    }
}

// --- options ----------------------------------------------------------------------------------

/// `ox.Limits` (section 7 of the specification), size-prefixed.
///
/// In a limits struct a host fills in, **0 in a size field means "the default of this field"**
/// (as `max_errors` does), so `{ .struct_size = sizeof l, .max_bytes = N }` changes one limit and
/// leaves the others at their defaults. To remove a limit, set it to `u64::MAX` (or start from
/// `limits_none`); a limit of 0 itself (reject every document) cannot be written, and 1 is the
/// smallest. `max_depth` is at most 128 whatever is given.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxLimits {
    /// `size_of::<OxLimits>()` of the struct the caller was built against.
    pub struct_size: u32,
    pub reserved: u32,
    /// 0: the default (64 MiB).
    pub max_bytes: u64,
    /// 0: the default (128). At most 128, whatever is given.
    pub max_depth: u64,
    /// 0: the default (1,000,000).
    pub max_items: u64,
    /// 0: the default (16 MiB).
    pub max_str_len: u64,
    /// 0: the default (4300).
    pub max_int_digits: u64,
}

impl OxLimits {
    /// The size of this struct in ABI 1.0: the least `struct_size` oxiform accepts.
    pub const SIZE_V1_0: usize =
        core::mem::offset_of!(OxLimits, max_int_digits) + core::mem::size_of::<u64>();

    /// Zeroed, with `struct_size` set, for `limits_default` or `limits_none` to fill in.
    pub const fn zeroed() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>() as u32,
            reserved: 0,
            max_bytes: 0,
            max_depth: 0,
            max_items: 0,
            max_str_len: 0,
            max_int_digits: 0,
        }
    }
}

/// The options of one decode call, size-prefixed. Start from [`OxDecodeOptions::new`] and set
/// what differs.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxDecodeOptions {
    /// `size_of::<OxDecodeOptions>()` of the struct the caller was built against.
    pub struct_size: u32,
    /// [`OX_MODE_STRICT`] or [`OX_MODE_LAX`].
    pub mode: u32,
    /// Issues collected before the decode stops; 0 is the default (100); 1 is "fail fast".
    pub max_errors: u64,
    /// Borrowed for the call, or null for the default limits.
    pub limits: *const OxLimits,
    /// [`OX_UNKNOWN_MODEL`], [`OX_UNKNOWN_IGNORE`] or [`OX_UNKNOWN_FORBID`] (OX-9).
    pub unknown: u32,
    /// Must be 0 in ABI 1.0; a bit this oxiform does not know is [`OX_UNSUPPORTED`].
    pub flags: u32,
    /// The validation context (`ox.Context`, OX-11): a borrowed `PyObject*`. Reserved: must be
    /// null in ABI 1.0, and anything else is [`OX_UNSUPPORTED`], never silently ignored.
    pub context: *mut OxPyObject,
}

impl OxDecodeOptions {
    /// The size of this struct in ABI 1.0: the least `struct_size` oxiform accepts.
    pub const SIZE_V1_0: usize =
        core::mem::offset_of!(OxDecodeOptions, context) + core::mem::size_of::<*mut OxPyObject>();

    /// Strict mode, default limits, `max_errors` 100, models decide on unknown keys.
    pub const fn new() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>() as u32,
            mode: OX_MODE_STRICT,
            max_errors: 0,
            limits: core::ptr::null(),
            unknown: OX_UNKNOWN_MODEL,
            flags: 0,
            context: core::ptr::null_mut(),
        }
    }
}

impl Default for OxDecodeOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// The options of one `decode_pairs` call (ABI 1.1), size-prefixed. Pairs are always read in lax
/// mode, so there is no `mode`. Start from [`OxPairsOptions::new`] and set what differs.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxPairsOptions {
    /// `size_of::<OxPairsOptions>()` of the struct the caller was built against.
    pub struct_size: u32,
    /// [`OX_PAIRS_CASE_INSENSITIVE`]; a bit this oxiform does not know is [`OX_UNSUPPORTED`].
    pub flags: u32,
    /// Issues collected before the decode stops; 0 is the default (100); 1 is "fail fast".
    pub max_errors: u64,
    /// Borrowed for the call, or null for the default limits. `max_bytes` bounds the whole
    /// source, `max_items` the number of pairs and of distinct names, `max_str_len` each decoded
    /// name and value.
    pub limits: *const OxLimits,
    /// [`OX_UNKNOWN_MODEL`], [`OX_UNKNOWN_IGNORE`] or [`OX_UNKNOWN_FORBID`] (OX-9).
    pub unknown: u32,
    pub reserved: u32,
}

impl OxPairsOptions {
    /// The size of this struct in ABI 1.1: the least `struct_size` `decode_pairs` accepts.
    pub const SIZE_V1_1: usize =
        core::mem::offset_of!(OxPairsOptions, reserved) + core::mem::size_of::<u32>();

    /// Case-sensitive names, default limits, `max_errors` 100, models decide on unknown names.
    pub const fn new() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>() as u32,
            flags: 0,
            max_errors: 0,
            limits: core::ptr::null(),
            unknown: OX_UNKNOWN_MODEL,
            reserved: 0,
        }
    }
}

impl Default for OxPairsOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// One name and value of [`OX_FORMAT_PAIRS`]: UTF-8 bytes, borrowed for the call. Frozen within a
/// major version, like [`OxStr`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxPair {
    pub name: OxStr,
    pub value: OxStr,
}

/// The options of one encode call, size-prefixed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxEncodeOptions {
    /// `size_of::<OxEncodeOptions>()` of the struct the caller was built against.
    pub struct_size: u32,
    /// [`OX_ENCODE_SORT_KEYS`] and [`OX_ENCODE_INDENT`]; an unknown bit is [`OX_UNSUPPORTED`].
    pub flags: u32,
    /// Spaces per level, with [`OX_ENCODE_INDENT`].
    pub indent: u32,
    pub reserved: u32,
}

impl OxEncodeOptions {
    /// The size of this struct in ABI 1.0: the least `struct_size` oxiform accepts.
    pub const SIZE_V1_0: usize =
        core::mem::offset_of!(OxEncodeOptions, reserved) + core::mem::size_of::<u32>();

    /// Compact output, keys in the order of the fields.
    pub const fn new() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>() as u32,
            flags: 0,
            indent: 0,
            reserved: 0,
        }
    }
}

impl Default for OxEncodeOptions {
    fn default() -> Self {
        Self::new()
    }
}

// --- the output buffer ------------------------------------------------------------------------

/// Makes room in a host buffer: afterwards `capacity >= min_capacity`, `data` points to at least
/// `capacity` bytes (it may have moved, and the first `len` bytes must have moved with it) and the
/// result is 0. Any other result means "no room": oxiform gives up with [`OX_BUFFER`].
pub type OxReserveFn = unsafe extern "C" fn(buffer: *mut OxBuffer, min_capacity: usize) -> i32;

/// A growable byte buffer **owned by the host**, size-prefixed.
///
/// oxiform appends at `data + len`: it never frees, reallocates or keeps `data`; to grow it calls
/// `reserve`, so the memory is always allocated and freed by the host's allocator, never crossing
/// the allocator boundary of two cdylibs. `len` bytes of `data` are initialized; the bytes from
/// `len` to `capacity` are scratch. A call that fails leaves `len` as it was.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxBuffer {
    /// `size_of::<OxBuffer>()` of the struct the caller was built against.
    pub struct_size: u32,
    pub reserved: u32,
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
    /// Called to make room; `None` makes the buffer fixed (a write that does not fit is [`OX_BUFFER`]).
    pub reserve: Option<OxReserveFn>,
    /// Free for the host (the `reserve` callback finds its state here); oxiform never reads it.
    pub user_data: *mut c_void,
}

impl OxBuffer {
    /// The size of this struct in ABI 1.0: the least `struct_size` oxiform accepts.
    pub const SIZE_V1_0: usize =
        core::mem::offset_of!(OxBuffer, user_data) + core::mem::size_of::<*mut c_void>();
}

// --- the issues -------------------------------------------------------------------------------

/// What a decode that returned [`OX_INVALID`] found. Everything it points to belongs to the
/// `OxReport` and is valid until `report_free` (or `report_in_source`, which rewrites it).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxReportView {
    pub struct_size: u32,
    /// [`OX_REPORT_VALIDATION`] or [`OX_REPORT_DECODE`].
    pub kind: u32,
    /// 1 when collection stopped at `max_errors`.
    pub truncated: u32,
    /// The stride of `issues`, and of every array of [`OxIssue`] below it (the issues of a union's
    /// variants). Step by this, not by `size_of::<OxIssue>()`.
    pub issue_size: u32,
    /// The stride of every array of [`OxPathSegment`].
    pub segment_size: u32,
    /// The stride of every array of [`OxContextEntry`].
    pub entry_size: u32,
    /// The stride of every array of [`OxVariant`].
    pub variant_size: u32,
    /// The stride of every array of [`OxChoiceValue`].
    pub choice_size: u32,
    pub issues: *const OxIssue,
    pub issue_count: usize,
}

/// One problem found in the input: the `{code, path, pointer, message, input, ctx}` of
/// section 4.7, with the same values `ox.ValidationError.errors()` has.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxIssue {
    /// The stable code (`"too_short"`, section 11.2).
    pub code: OxStr,
    pub message: OxStr,
    /// The path as a JSON Pointer (RFC 6901), escaped.
    pub pointer: OxStr,
    /// The received value as text, truncated; only when `has_input` is 1.
    pub input: OxStr,
    pub has_input: u32,
    pub reserved: u32,
    pub path: *const OxPathSegment,
    pub path_len: usize,
    pub context: *const OxContextEntry,
    pub context_len: usize,
}

/// One step of a path: a key or an index.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxPathSegment {
    /// [`OX_SEGMENT_KEY`] or [`OX_SEGMENT_INDEX`].
    pub kind: u32,
    pub reserved: u32,
    pub index: u64,
    pub key: OxStr,
}

/// One entry of the `ctx` of an issue.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxContextEntry {
    pub key: OxStr,
    pub value: OxContextValue,
}

/// A value of `ctx`; which fields are set depends on `kind` (`OX_CTX_*`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxContextValue {
    pub kind: u32,
    pub reserved: u32,
    /// `OX_CTX_BOOL` (0 or 1) and `OX_CTX_INTEGER`.
    pub integer: i64,
    pub float: f64,
    pub text: OxStr,
    /// The elements of the list kinds, `len` of them.
    pub items: *const c_void,
    pub len: usize,
}

/// A value of a `Literal`, which can be of mixed types.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxChoiceValue {
    pub kind: u32,
    pub reserved: u32,
    pub integer: i64,
    pub text: OxStr,
}

/// The issues of one variant of a union that was tried.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxVariant {
    pub name: OxStr,
    /// `issue_count` issues, `OxReportView::issue_size` bytes apart.
    pub issues: *const OxIssue,
    pub issue_count: usize,
}

/// One part of `report_combine`: the issues found in `source`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxReportPart {
    /// `"body"`, `"query"`, ...: borrowed for the call.
    pub source: OxStr,
    /// Consumed by a successful `report_combine`.
    pub report: *mut OxReport,
}
