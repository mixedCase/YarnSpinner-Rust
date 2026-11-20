//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/Visitors/UserDefinedTypesListener.cs>

use super::*;
use crate::annotations::*;
use crate::diagnostic_descriptors as descriptors;
use crate::prelude::generated::yarnspinnerparser::*;
use crate::prelude::generated::yarnspinnerparserlistener::YarnSpinnerParserListener;
use crate::prelude::*;
use antlr4rust::token::Token;
use antlr4rust::tree::ParseTreeListener;
use std::rc::Rc;
use yarnspinner_core::types::{EnumCase, EnumType};

/// Finds the enums that a file declares, so that they are known before type checking begins.
///
/// ## Implementation notes
///
/// antlr4rust requires listeners to live as long as the input, so listeners own their data
/// and hand it back after the walk instead of borrowing it.
pub(crate) struct UserDefinedTypesListener<'input> {
    pub(crate) file: SourceFile<'input>,
    pub(crate) known_types: Vec<Ty>,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl<'input> ParseTreeListener<'input, YarnSpinnerParserContextType>
    for UserDefinedTypesListener<'input>
{
}

impl<'input> UserDefinedTypesListener<'input> {
    fn error(&mut self, at: Diagnostic, message: &str) {
        self.diagnostics
            .push(at.described(descriptors::ENUM_DECLARATION_ERROR.create(&[message])));
    }
}

impl<'input> YarnSpinnerParserListener<'input> for UserDefinedTypesListener<'input> {
    fn exit_enum_statement(&mut self, ctx: &Enum_statementContext<'input>) {
        let Some(name_token) = ctx.name.as_ref() else {
            // The statement failed to parse
            return;
        };
        let name = name_token.get_text().to_owned();
        if self.known_types.iter().any(|t| t.name() == name) {
            self.diagnostics.push(
                self.file
                    .diagnostic_at(ctx)
                    .described(descriptors::REDECLARATION_OF_EXISTING_TYPE.create(&[&name])),
            );
            return;
        }
        let description = get_document_comments(self.file.parsed.tokens(), ctx, false);

        struct Case {
            name: String,
            description: String,
            value: Option<YarnValue>,
            /// `None` if the raw value is not a literal
            raw_type: Option<Ty>,
            at: Diagnostic,
        }
        let mut cases = Vec::new();
        for case in ctx.enum_case_statement_all() {
            let Some(case_name) = case.name.as_ref() else {
                return;
            };
            let value = case.rawValue.as_ref().map(|raw_value| {
                let value = literal_value(raw_value);
                if value.is_none() {
                    self.diagnostics
                        .push(
                            self.file.diagnostic_at(ctx).described(
                                descriptors::INVALID_LITERAL_VALUE
                                    .create(&["Expected a constant type"]),
                            ),
                        );
                }
                value
            });
            cases.push(Case {
                name: case_name.get_text().to_owned(),
                description: get_document_comments(self.file.parsed.tokens(), case.as_ref(), true),
                raw_type: value.as_ref().map(|value| match value {
                    Some(YarnValue::Number(_)) => Ty::Number,
                    Some(YarnValue::String(_)) => Ty::String,
                    Some(YarnValue::Boolean(_)) => Ty::Boolean,
                    None => Ty::Error,
                }),
                value: value.flatten(),
                at: self.file.diagnostic_at(case.as_ref()),
            });
        }

        let mut all_raw_types: Vec<Ty> = Vec::new();
        for raw_type in cases.iter().filter_map(|case| case.raw_type.clone()) {
            if !all_raw_types.contains(&raw_type) {
                all_raw_types.push(raw_type);
            }
        }

        let raw_type = match all_raw_types.as_slice() {
            [] => {
                // No raw values were given, so the cases are numbered
                for (index, case) in cases.iter_mut().enumerate() {
                    case.value = Some((index as f32).into());
                }
                Ty::Number
            }
            [raw_type] => {
                if ![Ty::String, Ty::Number].contains(raw_type) {
                    self.error(
                        self.file.diagnostic_at(ctx),
                        &format!("Enum raw values must be String or Number, not {raw_type}"),
                    );
                    return;
                }
                let mut any_missing = false;
                for case in cases.iter().filter(|case| case.raw_type.is_none()) {
                    any_missing = true;
                    self.error(case.at.clone(), &format!("Enum case {} must also have a raw value (if any cases have a value, then they all must have one)", case.name));
                }
                if any_missing {
                    return;
                }
                let mut any_duplicate_values = false;
                for case in &cases {
                    let duplicates = cases
                        .iter()
                        .filter(|other| other.value == case.value)
                        .count()
                        - 1;
                    if duplicates > 0 {
                        any_duplicate_values = true;
                        let value = case.value.clone().map(String::from).unwrap_or_default();
                        self.error(case.at.clone(), &format!("Enum case {} must have a unique raw value ({value} is used by {duplicates} other case(s).)", case.name));
                    }
                }
                if any_duplicate_values {
                    return;
                }
                for case in &cases {
                    if let Some(YarnValue::Number(number)) = case.value
                        && number.fract() != 0.0
                    {
                        self.error(
                            self.file.diagnostic_at(ctx),
                            "Number raw values on enum cases must be integers",
                        );
                    }
                }
                raw_type.clone()
            }
            raw_types => {
                let names: Vec<_> = raw_types.iter().map(Ty::name).collect();
                self.error(
                    self.file.diagnostic_at_token(name_token.as_ref()),
                    &format!("Enum member raw values may only be of a single type (they can't be a combination of {})", names.join(" and ")),
                );
                return;
            }
        };

        let mut any_duplicate_names = false;
        for (index, case) in cases.iter().enumerate() {
            if cases[..index].iter().any(|other| other.name == case.name) {
                any_duplicate_names = true;
                self.error(
                    case.at.clone(),
                    &format!("Enum case {} must have a unique name.", case.name),
                );
            }
        }
        if any_duplicate_names {
            return;
        }

        self.known_types.push(Ty::Enum(Rc::new(EnumType {
            name,
            description,
            raw_type: Box::new(raw_type.to_type()),
            cases: cases
                .into_iter()
                .map(|case| EnumCase {
                    name: case.name,
                    value: case.value.unwrap_or(YarnValue::Number(0.0)),
                    description: case.description,
                })
                .collect(),
        })));
    }
}
