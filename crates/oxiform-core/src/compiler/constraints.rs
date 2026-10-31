//! Which constraints each type accepts, and whether their values make sense together.

use regex::Regex;

use super::{DeclarationError, declaration_error};
use crate::description::{Bound, Constraints, Literal};
use crate::plan::{LengthRange, NumberChecks, StringChecks};

pub(super) fn number_checks(
    declared: &Constraints,
    type_name: &str,
    location: &str,
) -> Result<NumberChecks, DeclarationError> {
    reject_length(declared, type_name, location)?;
    reject_pattern(declared, type_name, location)?;
    for bound in [declared.minimum, declared.maximum].into_iter().flatten() {
        if let Bound::Float(value) = bound
            && value.is_nan()
        {
            return Err(declaration_error(location, "a numeric limit cannot be NaN"));
        }
    }
    if let (Some(minimum), Some(maximum)) = (declared.minimum, declared.maximum)
        && bound_greater(minimum, maximum)
    {
        return Err(declaration_error(
            location,
            "the Ge limit is greater than the Le limit",
        ));
    }
    let one_of = match &declared.one_of {
        None => None,
        Some(_) if type_name == "float" => {
            return Err(declaration_error(
                location,
                "OneOf is not supported on float",
            ));
        }
        Some(literals) => Some(integer_literals(literals, location)?),
    };
    Ok(NumberChecks {
        minimum: declared.minimum,
        maximum: declared.maximum,
        one_of,
    })
}

pub(super) fn string_checks(
    declared: &Constraints,
    location: &str,
) -> Result<StringChecks, DeclarationError> {
    reject_bounds(declared, "str", location)?;
    let length = length_range(declared, location)?;
    let pattern = match &declared.pattern {
        None => None,
        Some(source) => Some(Regex::new(source).map_err(|error| {
            declaration_error(location, format!("invalid Pattern {source:?}: {error}"))
        })?),
    };
    let one_of = match &declared.one_of {
        None => None,
        Some(literals) => Some(string_literals(literals, location)?),
    };
    Ok(StringChecks {
        length,
        pattern,
        one_of,
    })
}

pub(super) fn length_only(
    declared: &Constraints,
    type_name: &str,
    location: &str,
) -> Result<LengthRange, DeclarationError> {
    reject_bounds(declared, type_name, location)?;
    reject_pattern(declared, type_name, location)?;
    if declared.one_of.is_some() {
        return Err(not_applicable("OneOf", type_name, location));
    }
    length_range(declared, location)
}

fn length_range(declared: &Constraints, location: &str) -> Result<LengthRange, DeclarationError> {
    let range = LengthRange {
        minimum: declared.min_length,
        maximum: declared.max_length,
    };
    if let (Some(minimum), Some(maximum)) = (range.minimum, range.maximum)
        && minimum > maximum
    {
        return Err(declaration_error(
            location,
            "the Len minimum is greater than its maximum",
        ));
    }
    Ok(range)
}

fn integer_literals(literals: &[Literal], location: &str) -> Result<Vec<i64>, DeclarationError> {
    non_empty(literals, location)?;
    literals
        .iter()
        .map(|literal| match literal {
            Literal::Integer(value) => Ok(*value),
            Literal::String(value) => Err(declaration_error(
                location,
                format!("OneOf value {value:?} is not an int"),
            )),
        })
        .collect()
}

fn string_literals(literals: &[Literal], location: &str) -> Result<Vec<String>, DeclarationError> {
    non_empty(literals, location)?;
    literals
        .iter()
        .map(|literal| match literal {
            Literal::String(value) => Ok(value.clone()),
            Literal::Integer(value) => Err(declaration_error(
                location,
                format!("OneOf value {value} is not a str"),
            )),
        })
        .collect()
}

fn non_empty(literals: &[Literal], location: &str) -> Result<(), DeclarationError> {
    if literals.is_empty() {
        return Err(declaration_error(
            location,
            "OneOf needs at least one value",
        ));
    }
    Ok(())
}

fn bound_greater(left: Bound, right: Bound) -> bool {
    match (left, right) {
        (Bound::Integer(left), Bound::Integer(right)) => left > right,
        (left, right) => bound_as_float(left) > bound_as_float(right),
    }
}

fn bound_as_float(bound: Bound) -> f64 {
    match bound {
        Bound::Integer(value) => value as f64,
        Bound::Float(value) => value,
    }
}

fn reject_bounds(
    declared: &Constraints,
    type_name: &str,
    location: &str,
) -> Result<(), DeclarationError> {
    if declared.minimum.is_some() {
        return Err(not_applicable("Ge", type_name, location));
    }
    if declared.maximum.is_some() {
        return Err(not_applicable("Le", type_name, location));
    }
    Ok(())
}

fn reject_length(
    declared: &Constraints,
    type_name: &str,
    location: &str,
) -> Result<(), DeclarationError> {
    if declared.min_length.is_some() || declared.max_length.is_some() {
        return Err(not_applicable("Len", type_name, location));
    }
    Ok(())
}

fn reject_pattern(
    declared: &Constraints,
    type_name: &str,
    location: &str,
) -> Result<(), DeclarationError> {
    if declared.pattern.is_some() {
        return Err(not_applicable("Pattern", type_name, location));
    }
    Ok(())
}

fn not_applicable(constraint: &str, type_name: &str, location: &str) -> DeclarationError {
    declaration_error(
        location,
        format!("{constraint} does not apply to {type_name}"),
    )
}
