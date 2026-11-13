//! Finding the capsule of the installed oxiform from a PyO3 extension module.

use pyo3::exceptions::PyImportError;
use pyo3::prelude::*;
use pyo3::types::PyCapsule;

use crate::api::{
    ABI_MAJOR, ABI_MINOR, MAJORS_ATTRIBUTE, MODULE, OxiformApi, OxiformApiHeader, VersionError,
    table_from_pointer,
};
use crate::host::Api;

/// The table of the installed oxiform, for the version this crate was built for
/// ([`ABI_MAJOR`].[`ABI_MINOR`]). Call it once, when the extension module is imported: a host
/// built for another major version, or for a newer minor, fails here with an `ImportError` that
/// says what to install.
pub fn import(py: Python<'_>) -> PyResult<Api> {
    import_version(py, ABI_MAJOR, ABI_MINOR)
}

/// [`import`] for a given version: for tests of the refusal, and for hosts that support several
/// majors.
pub fn import_version(py: Python<'_>, major: u32, minor: u32) -> PyResult<Api> {
    let module = py.import(MODULE).map_err(|error| {
        PyImportError::new_err(format!(
            "this extension needs oxiform ({MODULE} could not be imported: {error}); \
             install it with `pip install oxiform`"
        ))
    })?;
    let majors: Vec<u32> = module
        .getattr(MAJORS_ATTRIBUTE)
        .and_then(|majors| majors.extract())
        .map_err(|_| {
            PyImportError::new_err(
                "the installed oxiform does not export the embedding ABI; upgrade oxiform to a \
                 release that does",
            )
        })?;
    if !majors.contains(&major) {
        return Err(PyImportError::new_err(
            VersionError::Major {
                wanted: major,
                available: majors,
            }
            .to_string(),
        ));
    }
    let attribute = format!("_ABI_V{major}");
    let name =
        std::ffi::CString::new(format!("{MODULE}.{attribute}")).expect("a name without NUL bytes");
    let capsule = module
        .getattr(attribute.as_str())?
        .cast_into::<PyCapsule>()?;
    let pointer = capsule.pointer_checked(Some(&name))?;
    // SAFETY: the capsule is named as oxiform names the table of this major version, so its
    // pointer is the table, which starts with the header and lives for the process.
    let table =
        unsafe { table_from_pointer(pointer.as_ptr().cast::<OxiformApiHeader>(), major, minor) }
            .map_err(|error| PyImportError::new_err(error.to_string()))?;
    let _: &'static OxiformApi = table;
    Ok(Api::new(table))
}
