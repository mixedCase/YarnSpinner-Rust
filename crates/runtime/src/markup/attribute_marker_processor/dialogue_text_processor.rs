//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/da39c7195107d8211f21c263e4084f773b84eaff/YarnSpinner/YarnSpinner/Dialogue.cs>, which was split into multiple files.

use crate::markup::{
    AttributeMarkerProcessor, MarkupAttribute, MarkupDiagnostic, MarkupValue,
    ReplacementMarkerResult,
};
use crate::prelude::*;
use bevy_platform::collections::HashSet;
use icu_plurals::PluralCategory;

/// Replaces the built-in markers `select`, `plural` and `ordinal` by text.
/// Corresponds to `BuiltInMarkupReplacer` of the original.
#[derive(Default, Debug, Clone)]
pub(crate) struct DialogueTextProcessor;

impl DialogueTextProcessor {
    pub(crate) fn new() -> Self {
        Self
    }
}

// Emulates a regex that matches any `%` as long as it's not preceded by a `\`.
// This is needed because the `regex` crate does not support negative lookbehind
static CANDIDATE_VALUE_PLACEHOLDER: &str = "%";
static INVALID_VALUE_PLACEHOLDER: &str = "\\%";

impl AttributeMarkerProcessor for DialogueTextProcessor {
    fn process_replacement_marker(
        &self,
        marker: &MarkupAttribute,
        child_builder: &mut String,
        child_attributes: &mut Vec<MarkupAttribute>,
        language_code: Option<&Language>,
    ) -> ReplacementMarkerResult {
        match replacement_text(marker, child_builder, child_attributes, language_code) {
            Ok(replacement) => {
                child_builder.push_str(&replacement);
                ReplacementMarkerResult::default()
            }
            Err(message) => ReplacementMarkerResult {
                diagnostics: vec![MarkupDiagnostic::new(message, marker.position)],
                invisible_characters: 0,
            },
        }
    }

    fn clone_box(&self) -> Box<dyn AttributeMarkerProcessor> {
        Box::new(self.clone())
    }
}

/// The text that a `select`, `plural` or `ordinal` marker stands for, or what is wrong with the marker.
fn replacement_text(
    marker: &MarkupAttribute,
    child_builder: &str,
    child_attributes: &[MarkupAttribute],
    language_code: Option<&Language>,
) -> core::result::Result<String, String> {
    let name = marker.name.as_str();
    if !child_builder.is_empty() || !child_attributes.is_empty() {
        return Err(format!("'{name}' markup only works on self-closing tags."));
    }
    let value_prop = marker.properties.get("value").ok_or_else(|| {
        format!("no 'value' property was found on the marker, {name} requires this to exist.")
    })?;
    let value = value_prop.to_string();

    // Apply the "select" marker
    if name == "select" {
        let replacement = marker
            .properties
            .get(&value)
            .ok_or_else(|| format!("no replacement value for {value} was found"))?;
        return Ok(replace_value_placeholders(&replacement.to_string(), &value));
    }

    // If it's not "select", then it's "plural" or "ordinal"
    let numeric_value = match value_prop {
        MarkupValue::Integer(value) => *value as f32,
        MarkupValue::Float(value) => *value,
        _ => {
            return Err(format!(
                "Asked to pluralise '{value}' but this is a type that does not support pluralisation."
            ));
        }
    };
    let language_code = language_code.ok_or_else(|| {
        format!("Dialogue language is not set. '{name}' markers cannot be used unless one is set.")
    })?;

    // Implementation note: no need to fiddle with locales here because ICU already does fallbacks for us.

    // I would love to cache this, but `icu_plural::PluralRules` is not `Send` because it contains an `Rc`, so even a mutex can't help here :(
    let pluralization = Pluralization::new(language_code.clone());
    let plural_case = match name {
        "plural" => pluralization.get_cardinal_plural_case(numeric_value),
        "ordinal" => pluralization.get_ordinal_plural_case(numeric_value),
        _ => {
            return Err(format!(
                "Asked to perform replacement for {name}, a marker we don't handle."
            ));
        }
    };
    let plural_case_name = plural_case_name(plural_case);

    // Now that we know the plural case, we can select the appropriate replacement text for it
    let replacement = marker.properties.get(plural_case_name).ok_or_else(|| {
        format!("no replacement for {value}'s plural case of {plural_case_name} was found.")
    })?;
    Ok(replace_value_placeholders(&replacement.to_string(), &value))
}

fn replace_value_placeholders(text: &str, value: &str) -> String {
    let candidates: HashSet<_> = text
        .match_indices(CANDIDATE_VALUE_PLACEHOLDER)
        .map(|(i, _)| i)
        .collect();
    let invalids: HashSet<_> = text
        .match_indices(INVALID_VALUE_PLACEHOLDER)
        .map(|(i, _)| i + 1)
        .collect();
    let indices: HashSet<_> = candidates.difference(&invalids).collect();

    text.chars()
        .enumerate()
        .map(|(i, c)| {
            if indices.contains(&i) {
                value.to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn plural_case_name(plural_case: PluralCategory) -> &'static str {
    match plural_case {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}
