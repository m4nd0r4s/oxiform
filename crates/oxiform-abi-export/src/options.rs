//! The options structs of the ABI, as the engine's options.

use oxiform_abi::{
    OX_ENCODE_INDENT, OX_ENCODE_SORT_KEYS, OX_INVALID_ARGUMENT, OX_MODE_LAX, OX_MODE_STRICT,
    OX_PAIRS_CASE_INSENSITIVE, OX_UNKNOWN_FORBID, OX_UNKNOWN_IGNORE, OX_UNKNOWN_MODEL,
    OX_UNSUPPORTED, OxDecodeOptions, OxEncodeOptions, OxLimits, OxPairsOptions, SizePrefixed,
    TooSmall, read_prefixed,
};
use oxiform_core::description::UnknownFields;
use oxiform_core::limits::Limits;
use oxiform_core::options::{Mode, Options};

use crate::Error;

fn too_small<T: SizePrefixed>(name: &str) -> impl Fn(TooSmall) -> Error + use<T> {
    let name = name.to_owned();
    move |error| {
        Error::new(
            OX_INVALID_ARGUMENT,
            format!(
                "{name}.struct_size is {}, less than the {} bytes of ABI {}: set struct_size to \
                 the size_of the struct you were built against",
                error.given,
                error.needed,
                T::SINCE
            ),
        )
    }
}

fn size(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// The options of a decode call. A null pointer is the defaults.
///
/// # Safety
///
/// `pointer` must be null, or point to a readable `OxDecodeOptions` (as much of it as its
/// `struct_size` says) and, when its `limits` is not null, to a readable `OxLimits`.
pub unsafe fn decode_options(pointer: *const OxDecodeOptions) -> Result<Options, Error> {
    if pointer.is_null() {
        return Ok(Options::default());
    }
    // SAFETY: the caller guarantees `pointer` is readable as `struct_size` bytes.
    let raw = unsafe { read_prefixed(pointer) }.map_err(too_small::<OxDecodeOptions>("options"))?;
    if raw.flags != 0 {
        return Err(Error::new(
            OX_UNSUPPORTED,
            format!(
                "options.flags is {:#x} but this oxiform defines no flags in ABI 1.0; clear it, or \
                 upgrade oxiform if you were built against a later minor version",
                raw.flags
            ),
        ));
    }
    if !raw.context.is_null() {
        return Err(Error::new(
            OX_UNSUPPORTED,
            "options.context is reserved (ox.Context, OX-11) and this oxiform does not read it \
             yet; pass NULL, or upgrade oxiform to a release that supports the validation context",
        ));
    }
    let mode = match raw.mode {
        OX_MODE_STRICT => Mode::Strict,
        OX_MODE_LAX => Mode::Lax,
        other => {
            return Err(Error::new(
                OX_INVALID_ARGUMENT,
                format!("options.mode must be OX_MODE_STRICT (0) or OX_MODE_LAX (1), not {other}"),
            ));
        }
    };
    Ok(Options {
        mode,
        // SAFETY: the caller guarantees a non-null `limits` is a readable `OxLimits`.
        limits: unsafe { limits_of(raw.limits) }?,
        max_errors: max_errors_of(raw.max_errors),
        unknown: unknown_of(raw.unknown)?,
    })
}

/// What a decode call does with names that match no field.
fn unknown_of(value: u32) -> Result<Option<UnknownFields>, Error> {
    match value {
        OX_UNKNOWN_MODEL => Ok(None),
        OX_UNKNOWN_IGNORE => Ok(Some(UnknownFields::Ignore)),
        OX_UNKNOWN_FORBID => Ok(Some(UnknownFields::Forbid)),
        other => Err(Error::new(
            OX_INVALID_ARGUMENT,
            format!(
                "options.unknown must be OX_UNKNOWN_MODEL (0), OX_UNKNOWN_IGNORE (1) or \
                 OX_UNKNOWN_FORBID (2), not {other}"
            ),
        )),
    }
}

fn max_errors_of(value: u64) -> usize {
    if value == 0 {
        Options::DEFAULT_MAX_ERRORS
    } else {
        size(value)
    }
}

/// The limits a host passes: null is the defaults.
///
/// # Safety
///
/// `pointer` must be null or point to a readable `OxLimits` (as much as its `struct_size` says).
unsafe fn limits_of(pointer: *const OxLimits) -> Result<Limits, Error> {
    if pointer.is_null() {
        return Ok(Limits::DEFAULT);
    }
    // SAFETY: the caller guarantees a non-null `pointer` is a readable `OxLimits`.
    let raw: OxLimits =
        unsafe { read_prefixed(pointer) }.map_err(too_small::<OxLimits>("limits"))?;
    // 0 in a field is the default of that field, as it is for `max_errors`: a host that fills
    // in one limit of a zeroed struct does not reject every document.
    let or_default = |value: u64, default: usize| {
        if value == 0 { default } else { size(value) }
    };
    let defaults = Limits::DEFAULT;
    Ok(Limits {
        max_bytes: or_default(raw.max_bytes, defaults.max_bytes),
        max_depth: or_default(raw.max_depth, defaults.max_depth).min(Limits::DEPTH_CEILING),
        max_items: or_default(raw.max_items, defaults.max_items),
        max_str_len: or_default(raw.max_str_len, defaults.max_str_len),
        max_int_digits: or_default(raw.max_int_digits, defaults.max_int_digits),
    })
}

/// The options of a `decode_pairs` call: always lax, and whether names match without regard to
/// case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairsParams {
    pub options: Options,
    pub case_insensitive: bool,
}

/// The options of a `decode_pairs` call (ABI 1.1). A null pointer is the defaults.
///
/// # Safety
///
/// `pointer` must be null, or point to a readable `OxPairsOptions` (as much of it as its
/// `struct_size` says) and, when its `limits` is not null, to a readable `OxLimits`.
pub unsafe fn pairs_options(pointer: *const OxPairsOptions) -> Result<PairsParams, Error> {
    let lax = Options {
        mode: Mode::Lax,
        ..Options::default()
    };
    if pointer.is_null() {
        return Ok(PairsParams {
            options: lax,
            case_insensitive: false,
        });
    }
    // SAFETY: the caller guarantees `pointer` is readable as `struct_size` bytes.
    let raw = unsafe { read_prefixed(pointer) }.map_err(too_small::<OxPairsOptions>("options"))?;
    let unknown_flags = raw.flags & !OX_PAIRS_CASE_INSENSITIVE;
    if unknown_flags != 0 {
        return Err(Error::new(
            OX_UNSUPPORTED,
            format!(
                "options.flags has the bits {unknown_flags:#x}, which this oxiform does not know; \
                 clear them, or upgrade oxiform if you were built against a later minor version"
            ),
        ));
    }
    Ok(PairsParams {
        options: Options {
            mode: Mode::Lax,
            // SAFETY: the caller guarantees a non-null `limits` is a readable `OxLimits`.
            limits: unsafe { limits_of(raw.limits) }?,
            max_errors: max_errors_of(raw.max_errors),
            unknown: unknown_of(raw.unknown)?,
        },
        case_insensitive: raw.flags & OX_PAIRS_CASE_INSENSITIVE != 0,
    })
}

/// The options of an encode call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EncodeParams {
    pub sort_keys: bool,
    pub indent: Option<usize>,
}

/// The options of an encode call. A null pointer is the defaults.
///
/// # Safety
///
/// `pointer` must be null, or point to a readable `OxEncodeOptions` (as much as its `struct_size`
/// says).
pub unsafe fn encode_options(pointer: *const OxEncodeOptions) -> Result<EncodeParams, Error> {
    if pointer.is_null() {
        return Ok(EncodeParams::default());
    }
    // SAFETY: the caller guarantees `pointer` is readable as `struct_size` bytes.
    let raw = unsafe { read_prefixed(pointer) }.map_err(too_small::<OxEncodeOptions>("options"))?;
    let unknown_flags = raw.flags & !(OX_ENCODE_SORT_KEYS | OX_ENCODE_INDENT);
    if unknown_flags != 0 {
        return Err(Error::new(
            OX_UNSUPPORTED,
            format!(
                "options.flags has the bits {unknown_flags:#x}, which this oxiform does not know; \
                 clear them, or upgrade oxiform if you were built against a later minor version"
            ),
        ));
    }
    Ok(EncodeParams {
        sort_keys: raw.flags & OX_ENCODE_SORT_KEYS != 0,
        indent: (raw.flags & OX_ENCODE_INDENT != 0).then_some(raw.indent as usize),
    })
}
