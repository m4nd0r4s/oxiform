//! `Literal` and `Enum`: the values allowed, by kind, mapped to their index.

use super::{DeclarationError, declaration_error};
use crate::description::{ChoiceDescription, ChoiceValue};
use crate::plan::ChoicePlan;

pub(super) fn compile_choice(
    description: &ChoiceDescription,
) -> Result<ChoicePlan, DeclarationError> {
    let mut plan = ChoicePlan {
        name: description.name.clone(),
        is_enum: description.is_enum,
        ..ChoicePlan::default()
    };
    if description.values.is_empty() {
        return Err(declaration_error(&description.name, "no value is allowed"));
    }
    for (index, value) in description.values.iter().enumerate() {
        let index = u32::try_from(index).expect("fewer than 2^32 choices");
        let previous = match value {
            ChoiceValue::Null => plan.null.replace(index),
            ChoiceValue::Bool(flag) => plan.bools[usize::from(*flag)].replace(index),
            ChoiceValue::Integer(number) => plan.integers.insert(*number, index),
            ChoiceValue::String(text) => plan.strings.insert(text.clone(), index),
        };
        let shown = python_repr(value);
        if previous.is_some() {
            return Err(declaration_error(
                &description.name,
                format!("the value {shown} is allowed twice"),
            ));
        }
        plan.shown.push(shown);
        plan.values.push(value.clone());
    }
    Ok(plan)
}

/// A value as Python prints it: `'a'`, `1`, `True`, `None`.
pub(crate) fn python_repr(value: &ChoiceValue) -> String {
    match value {
        ChoiceValue::Null => "None".into(),
        ChoiceValue::Bool(true) => "True".into(),
        ChoiceValue::Bool(false) => "False".into(),
        ChoiceValue::Integer(number) => number.to_string(),
        ChoiceValue::String(text) => format!("{text:?}").replacen('"', "'", 2),
    }
}
