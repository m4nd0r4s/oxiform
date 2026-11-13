//! Calls the validators of `@ox.validator` for the executor and turns what they raise into
//! rejections (spec section 4.3).
//!
//! `ox.Invalid`, `ValueError` and `TypeError` reject the value. Every other exception, and the
//! errors oxiform itself raises for a validator that is misused (a missing context, a field
//! validator that returns nothing), propagate untouched.

use oxiform_core::issues::PathSegment;
use oxiform_core::plan::StructId;
use oxiform_core::sink::{Rejection, Verdict};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyString, PyTuple};

use crate::description::Validator;
use crate::python_objects::{invalid_error, schema_error};
use crate::sink::PythonSink;

/// The message of a rejection whose text may hold what the validator was given.
const WITHHELD: &str =
    "the validator rejected the value (the message is withheld: the model holds an ox.Secret)";

/// What a call of one validator came to.
enum Outcome<'py> {
    Returned(Bound<'py, PyAny>),
    Rejected(Rejection),
}

impl<'run, 'py> PythonSink<'run, 'py> {
    /// Runs the validators of field `field` of `structure` on `value`, in order, each on what
    /// the one before returned.
    pub(crate) fn run_field_validators(
        &mut self,
        structure: StructId,
        field: usize,
        mut value: Bound<'py, PyAny>,
    ) -> PyResult<Verdict<Bound<'py, PyAny>>> {
        let compiled = self.compiled;
        let validators = &compiled.runtime.structs[structure.index()].field_validators[field];
        let plan_field = &compiled.plan.struct_plan(structure).fields[field];
        for validator in validators {
            let arguments = [validator.cls.bind(self.py).clone(), value.clone()];
            let secret = || compiled.plan.node_holds_a_secret(plan_field.value);
            match self.call(validator, &arguments, None, secret)? {
                Outcome::Rejected(rejection) => return Ok(Verdict::Reject(vec![rejection])),
                Outcome::Returned(returned) => {
                    if returned.is_none() && !compiled.plan.accepts_none(plan_field.value) {
                        return Err(self.forgot_to_return(validator, &plan_field.name));
                    }
                    value = returned;
                }
            }
        }
        Ok(Verdict::Accept(value))
    }

    /// Runs the validators of the whole model on `instance`, in order.
    pub(crate) fn run_model_validators(
        &mut self,
        structure: StructId,
        instance: Bound<'py, PyAny>,
    ) -> PyResult<Verdict<Bound<'py, PyAny>>> {
        let compiled = self.compiled;
        for validator in &compiled.runtime.structs[structure.index()].model_validators {
            let arguments = [instance.clone()];
            let secret = || compiled.plan.holds_secret(structure);
            match self.call(validator, &arguments, Some(structure), secret)? {
                Outcome::Rejected(rejection) => return Ok(Verdict::Reject(vec![rejection])),
                Outcome::Returned(returned) if !returned.is_none() => {
                    return Err(PyErr::from_type(
                        schema_error(self.py)?.clone(),
                        format!(
                            "{}: a model validator checks the instance and returns None, not \
                             {}: raise ox.Invalid to reject it, and change nothing you return",
                            validator.name,
                            returned.get_type().name()?
                        ),
                    ));
                }
                Outcome::Returned(_) => {}
            }
        }
        Ok(Verdict::Accept(instance))
    }

    /// Calls one validator. `model` is the structure of a model validator, whose rejection path
    /// starts with the Python name of a field. `holds_secret` is asked only when a message is
    /// about to be shown.
    fn call(
        &self,
        validator: &Validator,
        arguments: &[Bound<'py, PyAny>],
        model: Option<StructId>,
        holds_secret: impl FnOnce() -> bool,
    ) -> PyResult<Outcome<'py>> {
        let py = self.py;
        let keywords = match (&validator.context, &self.context) {
            (Some(name), Some(context)) => {
                let keywords = PyDict::new(py);
                keywords.set_item(name.bind(py), context)?;
                Some(keywords)
            }
            (Some(name), None) if validator.context_none_when_absent => {
                let keywords = PyDict::new(py);
                keywords.set_item(name.bind(py), py.None())?;
                Some(keywords)
            }
            (Some(_), None) if validator.context_required => {
                return Err(PyTypeError::new_err(format!(
                    "the validator {} needs a validation context and this decode was given \
                     none: pass context=... to the decode call (to ox.json.decode, \
                     ox.urlencoded.decode, ox.pairs.decode or cfg.load; options.context in the \
                     C ABI)",
                    validator.name
                )));
            }
            _ => None,
        };
        let function = validator.function.bind(py);
        let arguments = PyTuple::new(py, arguments)?;
        match function.call(arguments, keywords.as_ref()) {
            Ok(returned) => Ok(Outcome::Returned(returned)),
            Err(error) => self
                .rejection(error, model, holds_secret)
                .map(Outcome::Rejected),
        }
    }

    /// The rejection that `error` stands for, or `error` itself when it is not one.
    fn rejection(
        &self,
        error: PyErr,
        model: Option<StructId>,
        holds_secret: impl FnOnce() -> bool,
    ) -> PyResult<Rejection> {
        let py = self.py;
        if error.is_instance(py, invalid_error(py)?) {
            let invalid = error.value(py);
            let code: String = invalid.getattr("code")?.extract()?;
            let mut path = Vec::new();
            for segment in invalid.getattr("path")?.try_iter()? {
                let segment = segment?;
                path.push(if segment.is_instance_of::<PyInt>() {
                    PathSegment::Index(segment.extract()?)
                } else {
                    PathSegment::Key(segment.cast_into::<PyString>()?.to_string())
                });
            }
            if let (Some(structure), Some(PathSegment::Key(first))) = (model, path.first_mut()) {
                let fields = &self.compiled.plan.struct_plan(structure).fields;
                if let Some(field) = fields.iter().find(|field| field.name == *first) {
                    first.clone_from(&field.wire_name);
                }
            }
            return Ok(Rejection {
                code: (code != "invalid").then_some(code),
                message: invalid.getattr("message")?.extract()?,
                path,
            });
        }
        if error.is_instance_of::<PyValueError>(py) || error.is_instance_of::<PyTypeError>(py) {
            let text = error.value(py).str()?.to_string();
            let message = if holds_secret() {
                WITHHELD.to_owned()
            } else if text.is_empty() {
                "invalid value".to_owned()
            } else {
                text
            };
            return Ok(Rejection {
                code: None,
                message,
                path: Vec::new(),
            });
        }
        Err(error)
    }

    fn forgot_to_return(&self, validator: &Validator, field: &str) -> PyErr {
        match schema_error(self.py) {
            Ok(class) => PyErr::from_type(
                class.clone(),
                format!(
                    "{} returned None for the field '{field}', which does not accept None: a \
                     field validator returns the value (did you forget `return value`?)",
                    validator.name
                ),
            ),
            Err(error) => error,
        }
    }
}
