//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/Visitors/LiteralValueVisitor.cs>

use crate::prelude::generated::yarnspinnerparser::*;
use crate::prelude::*;
use antlr4rust::tree::ParseTree;

/// The value of a `value` context if it is a literal: a number, string or boolean.
pub(crate) fn literal_value(value: &ValueContextAll) -> Option<YarnValue> {
    match value {
        ValueContextAll::ValueNumberContext(number) => {
            Some(number.get_text().parse::<f32>().unwrap_or_default().into())
        }
        ValueContextAll::ValueStringContext(string) => {
            Some(string.get_text().trim_matches('"').to_owned().into())
        }
        ValueContextAll::ValueTrueContext(_) => Some(true.into()),
        ValueContextAll::ValueFalseContext(_) => Some(false.into()),
        _ => None,
    }
}

/// The value of an expression that is a literal or a negated number literal.
pub(crate) fn literal_expression_value(expression: &ExpressionContextAll) -> Option<YarnValue> {
    let value_of = |expression: &ExpressionContextAll| match expression {
        ExpressionContextAll::ExpValueContext(value) => literal_value(value.value()?.as_ref()),
        _ => None,
    };
    match expression {
        ExpressionContextAll::ExpNegativeContext(negative) => {
            match value_of(negative.expression()?.as_ref())? {
                YarnValue::Number(number) => Some((-number).into()),
                _ => None,
            }
        }
        expression => value_of(expression),
    }
}
