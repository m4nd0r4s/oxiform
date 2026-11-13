//! A safe layer over the table for hosts written in Rust: handles that release themselves, the
//! decode options as plain Rust values, and the reports as borrowing views.

use core::fmt;
use core::ptr::NonNull;

use crate::VecBuffer;
use crate::api::OxiformApi;
use crate::types::{
    OX_ENCODE_INDENT, OX_ENCODE_SORT_KEYS, OX_FORMAT_JSON, OX_FORMAT_PAIRS, OX_FORMAT_URLENCODED,
    OX_INVALID, OX_MODE_LAX, OX_MODE_STRICT, OX_OK, OX_PAIRS_CASE_INSENSITIVE, OX_UNKNOWN_FORBID,
    OX_UNKNOWN_IGNORE, OX_UNKNOWN_MODEL, OxBuffer, OxDecodeOptions, OxEncodeOptions, OxLimits,
    OxPair, OxPairsOptions, OxPlan, OxPyObject, OxReport, OxReportPart, OxStr,
};
use crate::views::ReportView;

/// A status code other than [`OX_OK`] (and, for a decode, other than `OX_INVALID`).
///
/// For the calls that run with the interpreter, a Python exception is set in the thread when the
/// code is `OX_PYTHON_ERROR`, and for the other codes too except `OX_INTERNAL` of a call that needs
/// none; fetch it before anything else touches the thread state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status(pub i32);

/// The table of the installed oxiform, with a safe layer on top.
#[derive(Clone, Copy)]
pub struct Api {
    table: &'static OxiformApi,
}

// SAFETY: the table is immutable (function pointers and pointers to static strings) and lives
// for the whole process, so sharing a reference to it between threads is sound.
unsafe impl Send for Api {}
// SAFETY: as above.
unsafe impl Sync for Api {}

impl fmt::Debug for Api {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Api")
            .field("abi", &self.version())
            .finish()
    }
}

/// How strictly values are coerced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Strict,
    Lax,
}

/// What to do with keys that match no field (OX-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Unknown {
    /// Each model decides.
    #[default]
    Model,
    Ignore,
    Forbid,
}

/// The options of a decode call.
#[derive(Debug, Clone, Copy, Default)]
pub struct DecodeOptions {
    pub mode: Mode,
    /// Issues collected before the decode stops: 0 is the default (100).
    pub max_errors: u64,
    /// `None`: the default limits.
    pub limits: Option<OxLimits>,
    pub unknown: Unknown,
}

/// The options of a `decode_pairs` call (ABI 1.1). Pairs are always read in lax mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct PairsOptions {
    /// Match names without regard to ASCII case, as header names are matched.
    pub case_insensitive: bool,
    /// Issues collected before the decode stops: 0 is the default (100).
    pub max_errors: u64,
    /// `None`: the default limits.
    pub limits: Option<OxLimits>,
    pub unknown: Unknown,
}

/// The options of an encode call.
#[derive(Debug, Clone, Copy, Default)]
pub struct EncodeOptions {
    pub sort_keys: bool,
    /// Spaces per level; `None`: compact.
    pub indent: Option<u32>,
}

/// What a decode returned.
#[derive(Debug)]
pub enum Decoded {
    /// The decoded object: a new reference the host owns.
    Value(NonNull<OxPyObject>),
    /// The input is invalid.
    Invalid(Report),
}

impl Api {
    /// Wraps a table that passed [`crate::table_from_pointer`].
    pub fn new(table: &'static OxiformApi) -> Self {
        Self { table }
    }

    /// The raw table, for the calls this layer does not wrap.
    pub fn table(&self) -> &'static OxiformApi {
        self.table
    }

    /// `(major, minor)` of the table.
    pub fn version(&self) -> (u32, u32) {
        (self.table.abi_major, self.table.abi_minor)
    }

    /// The version of the oxiform package.
    pub fn oxiform_version(&self) -> &'static str {
        // SAFETY: `oxiform_version` is a static UTF-8 string of the table.
        unsafe { static_text(self.table.oxiform_version) }
    }

    /// A sentence that says what `status` means and what to do.
    pub fn status_message(&self, status: Status) -> &'static str {
        // SAFETY: the function takes any integer and returns a static string.
        unsafe { static_text((self.table.status_message)(status.0)) }
    }

    /// The default limits, to change one of them.
    pub fn limits_default(&self) -> OxLimits {
        let mut limits = OxLimits::zeroed();
        // SAFETY: `limits` is a valid struct with its `struct_size` set.
        unsafe { (self.table.limits_default)(&raw mut limits) };
        limits
    }

    /// No size limits (nesting still stops at 128 levels).
    pub fn limits_none(&self) -> OxLimits {
        let mut limits = OxLimits::zeroed();
        // SAFETY: as above.
        unsafe { (self.table.limits_none)(&raw mut limits) };
        limits
    }

    /// The plan of `target`, compiled on first use and shared with `ox.json.decode`.
    ///
    /// # Safety
    ///
    /// The thread must be attached to the interpreter and `target` must be a valid `PyObject*`.
    pub unsafe fn plan_for(&self, target: *mut OxPyObject) -> Result<Plan, Status> {
        let mut plan = core::ptr::null_mut();
        // SAFETY: the caller guarantees an attached thread and a valid `target`; `plan` is a valid
        // out pointer.
        let status = unsafe { (self.table.plan_for)(target, &raw mut plan) };
        match (status, NonNull::new(plan)) {
            (OX_OK, Some(pointer)) => Ok(Plan {
                api: *self,
                pointer,
            }),
            _ => Err(Status(status)),
        }
    }

    /// Reads one JSON document from `data` as the type of `plan`.
    ///
    /// # Safety
    ///
    /// The thread must be attached to the interpreter. The `Decoded::Value` is a new reference the
    /// caller must release with the interpreter attached.
    pub unsafe fn decode_json(
        &self,
        plan: &Plan,
        data: &[u8],
        options: &DecodeOptions,
    ) -> Result<Decoded, Status> {
        let limits = options.limits;
        let mut raw = OxDecodeOptions::new();
        raw.mode = match options.mode {
            Mode::Strict => OX_MODE_STRICT,
            Mode::Lax => OX_MODE_LAX,
        };
        raw.max_errors = options.max_errors;
        raw.unknown = unknown_value(options.unknown);
        raw.limits = limits
            .as_ref()
            .map_or(core::ptr::null(), core::ptr::from_ref);
        let mut value = core::ptr::null_mut();
        let mut report = core::ptr::null_mut();
        // SAFETY: the caller guarantees an attached thread; `plan` is alive; `data` is a slice
        // (readable for its length, not changed during the call as it is borrowed); `raw` and the
        // limits it points to live through the call; the out pointers are valid.
        let status = unsafe {
            (self.table.decode)(
                plan.pointer.as_ptr(),
                OX_FORMAT_JSON,
                data.as_ptr(),
                data.len(),
                &raw const raw,
                &raw mut value,
                &raw mut report,
            )
        };
        self.decoded(status, value, report)
    }

    /// Reads a query string or an `application/x-www-form-urlencoded` body (`%XX` and `+`
    /// decoded) as the model of `plan`, in lax mode (ABI 1.1).
    ///
    /// # Safety
    ///
    /// The thread must be attached to the interpreter. The `Decoded::Value` is a new reference the
    /// caller must release with the interpreter attached.
    pub unsafe fn decode_urlencoded(
        &self,
        plan: &Plan,
        data: &[u8],
        options: &PairsOptions,
    ) -> Result<Decoded, Status> {
        // SAFETY: forwarded; `data` is a slice and there are no pairs.
        unsafe { self.decode_pairs_raw(plan, OX_FORMAT_URLENCODED, data, &[], options) }
    }

    /// Reads name/value pairs as they are (headers, cookies) as the model of `plan`, in lax mode
    /// (ABI 1.1).
    ///
    /// # Safety
    ///
    /// The thread must be attached to the interpreter. The `Decoded::Value` is a new reference the
    /// caller must release with the interpreter attached.
    pub unsafe fn decode_pairs(
        &self,
        plan: &Plan,
        pairs: &[(&str, &str)],
        options: &PairsOptions,
    ) -> Result<Decoded, Status> {
        let raw: Vec<OxPair> = pairs
            .iter()
            .map(|(name, value)| OxPair {
                name: OxStr::from_str(name),
                value: OxStr::from_str(value),
            })
            .collect();
        // SAFETY: forwarded; `raw` borrows from `pairs`, which outlives the call.
        unsafe { self.decode_pairs_raw(plan, OX_FORMAT_PAIRS, &[], &raw, options) }
    }

    /// # Safety
    ///
    /// The thread must be attached to the interpreter; `data` and `pairs` are valid for the call.
    unsafe fn decode_pairs_raw(
        &self,
        plan: &Plan,
        format: u32,
        data: &[u8],
        pairs: &[OxPair],
        options: &PairsOptions,
    ) -> Result<Decoded, Status> {
        let limits = options.limits;
        let mut raw = OxPairsOptions::new();
        if options.case_insensitive {
            raw.flags |= OX_PAIRS_CASE_INSENSITIVE;
        }
        raw.max_errors = options.max_errors;
        raw.unknown = unknown_value(options.unknown);
        raw.limits = limits
            .as_ref()
            .map_or(core::ptr::null(), core::ptr::from_ref);
        let mut value = core::ptr::null_mut();
        let mut report = core::ptr::null_mut();
        // The slices may be empty, and an empty slice's pointer is dangling, not null: pass null
        // for the source that is not used, as the contract says.
        let data_pointer = if data.is_empty() {
            core::ptr::null()
        } else {
            data.as_ptr()
        };
        let pairs_pointer = if pairs.is_empty() {
            core::ptr::null()
        } else {
            pairs.as_ptr()
        };
        // SAFETY: the caller guarantees an attached thread; `plan` is alive; `data` and `pairs`
        // are slices readable for their lengths and unchanged during the call; `raw` and the
        // limits it points to live through the call; the out pointers are valid.
        let status = unsafe {
            (self.table.decode_pairs)(
                plan.pointer.as_ptr(),
                format,
                data_pointer,
                data.len(),
                pairs_pointer,
                pairs.len(),
                &raw const raw,
                &raw mut value,
                &raw mut report,
            )
        };
        self.decoded(status, value, report)
    }

    fn decoded(
        &self,
        status: i32,
        value: *mut OxPyObject,
        report: *mut OxReport,
    ) -> Result<Decoded, Status> {
        match (status, NonNull::new(value), NonNull::new(report)) {
            (OX_OK, Some(value), _) => Ok(Decoded::Value(value)),
            (OX_INVALID, _, Some(pointer)) => Ok(Decoded::Invalid(Report {
                api: *self,
                pointer,
            })),
            _ => Err(Status(status)),
        }
    }

    /// Appends `obj` as JSON to `buffer`; as the declared type of `plan`, when there is one.
    ///
    /// # Safety
    ///
    /// The thread must be attached to the interpreter and `obj` must be a valid `PyObject*`.
    pub unsafe fn encode_json(
        &self,
        obj: *mut OxPyObject,
        plan: Option<&Plan>,
        options: &EncodeOptions,
        buffer: &mut VecBuffer,
    ) -> Result<(), Status> {
        let mut raw = OxEncodeOptions::new();
        if options.sort_keys {
            raw.flags |= OX_ENCODE_SORT_KEYS;
        }
        if let Some(indent) = options.indent {
            raw.flags |= OX_ENCODE_INDENT;
            raw.indent = indent;
        }
        let plan = plan.map_or(core::ptr::null(), |plan| plan.pointer.as_ptr().cast_const());
        let status = buffer.with_raw(|raw_buffer: *mut OxBuffer| {
            // SAFETY: the caller guarantees an attached thread and a valid `obj`; `plan` is null or
            // alive; `raw` and `raw_buffer` live through the call.
            unsafe { (self.table.encode)(OX_FORMAT_JSON, obj, plan, &raw const raw, raw_buffer) }
        });
        if status == OX_OK {
            Ok(())
        } else {
            Err(Status(status))
        }
    }

    /// One report for the parts of a request, in order of priority: see `report_combine`.
    pub fn combine_reports(
        &self,
        parts: Vec<(&str, Report)>,
        max_errors: u64,
    ) -> Result<Report, Status> {
        let raw: Vec<OxReportPart> = parts
            .iter()
            .map(|(source, report)| OxReportPart {
                source: OxStr::from_str(source),
                report: report.pointer.as_ptr(),
            })
            .collect();
        let mut combined = core::ptr::null_mut();
        // SAFETY: `raw` holds `raw.len()` valid parts whose sources are borrowed from `parts` and
        // whose reports are live handles this layer owns; `combined` is a valid out pointer.
        let status = unsafe {
            (self.table.report_combine)(raw.as_ptr(), raw.len(), max_errors, &raw mut combined)
        };
        match (status, NonNull::new(combined)) {
            (OX_OK, Some(pointer)) => {
                // Every part's report was consumed: do not free them again.
                for (_, report) in parts {
                    core::mem::forget(report);
                }
                Ok(Report {
                    api: *self,
                    pointer,
                })
            }
            _ => Err(Status(status)),
        }
    }
}

fn unknown_value(unknown: Unknown) -> u32 {
    match unknown {
        Unknown::Model => OX_UNKNOWN_MODEL,
        Unknown::Ignore => OX_UNKNOWN_IGNORE,
        Unknown::Forbid => OX_UNKNOWN_FORBID,
    }
}

/// # Safety
///
/// `text` must be a static UTF-8 string.
unsafe fn static_text(text: OxStr) -> &'static str {
    // SAFETY: the caller guarantees `text` is static UTF-8.
    unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(text.ptr, text.len)) }
}

/// One reference to a plan, released when it is dropped.
pub struct Plan {
    api: Api,
    pointer: NonNull<OxPlan>,
}

// SAFETY: a plan is immutable, reference counted atomically, and `plan_release` attaches itself
// when it frees the last reference, so a handle can be used and dropped from any thread.
unsafe impl Send for Plan {}
// SAFETY: as above.
unsafe impl Sync for Plan {}

impl fmt::Debug for Plan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Plan({:p})", self.pointer)
    }
}

impl Plan {
    /// The type the plan was made for, borrowed from the plan.
    pub fn target(&self) -> *mut OxPyObject {
        // SAFETY: `pointer` is a live plan.
        unsafe { (self.api.table.plan_target)(self.pointer.as_ptr()) }
    }

    /// The raw handle, for the calls of the table this layer does not wrap. It is valid while this
    /// `Plan` lives.
    pub fn as_ptr(&self) -> *const OxPlan {
        self.pointer.as_ptr()
    }

    /// The address that identifies the plan: two handles to the same plan share it.
    pub fn address(&self) -> usize {
        self.pointer.as_ptr() as usize
    }
}

impl Clone for Plan {
    fn clone(&self) -> Self {
        // SAFETY: `pointer` is a live plan; this adds the reference the clone owns.
        unsafe { (self.api.table.plan_retain)(self.pointer.as_ptr()) };
        Self {
            api: self.api,
            pointer: self.pointer,
        }
    }
}

impl Drop for Plan {
    fn drop(&mut self) {
        // SAFETY: drops the one reference this handle owns.
        unsafe { (self.api.table.plan_release)(self.pointer.as_ptr()) };
    }
}

/// The issues of a failed decode, freed when dropped.
pub struct Report {
    api: Api,
    pointer: NonNull<OxReport>,
}

// SAFETY: a report is plain data owned by the handle, and its functions need no interpreter, so
// it can move to any thread (not be shared: `report_in_source` rewrites it).
unsafe impl Send for Report {}

impl fmt::Debug for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Report({:p})", self.pointer)
    }
}

impl Report {
    /// The raw handle, for the calls of the table this layer does not wrap. It is valid while this
    /// `Report` lives.
    pub fn as_ptr(&self) -> *mut OxReport {
        self.pointer.as_ptr()
    }

    /// The issues, borrowed from the report.
    pub fn view(&self) -> ReportView<'_> {
        // SAFETY: `pointer` is a live report, and its view lives until the report is freed or
        // rewritten, which `&self` rules out while the view is borrowed.
        unsafe { ReportView::from_raw(&*(self.api.table.report_view)(self.pointer.as_ptr())) }
    }

    /// Makes every issue an issue found in `source`: the source becomes the first segment of every
    /// path.
    pub fn in_source(&mut self, source: &str) -> Result<(), Status> {
        // SAFETY: `pointer` is a live report; `source` is a valid string.
        let status = unsafe {
            (self.api.table.report_in_source)(self.pointer.as_ptr(), OxStr::from_str(source))
        };
        if status == OX_OK {
            Ok(())
        } else {
            Err(Status(status))
        }
    }
}

impl Drop for Report {
    fn drop(&mut self) {
        // SAFETY: frees the one report this handle owns.
        unsafe { (self.api.table.report_free)(self.pointer.as_ptr()) };
    }
}
