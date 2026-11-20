//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner/YarnSpinner.Markup/IAttributeMarkerProcessor.cs>

pub(crate) use self::dialogue_text_processor::*;
use crate::markup::{MarkupAttribute, MarkupDiagnostic};
use crate::prelude::*;
use core::fmt::Debug;

mod dialogue_text_processor;

/// Replaces a marker by text while a line is parsed, like `[select/]` does.
pub(crate) trait AttributeMarkerProcessor: Debug + Send + Sync {
    /// Processes the marker `marker`. `child_builder` holds the text that the marker encloses and
    /// `child_attributes` the attributes inside it, with positions relative to that text. Both may be
    /// changed; whatever `child_builder` holds afterwards takes the place of the marker and its contents.
    fn process_replacement_marker(
        &self,
        marker: &MarkupAttribute,
        child_builder: &mut String,
        child_attributes: &mut Vec<MarkupAttribute>,
        language_code: Option<&Language>,
    ) -> ReplacementMarkerResult;
    fn clone_box(&self) -> Box<dyn AttributeMarkerProcessor>;
}

/// What went wrong while replacing a marker, and how many of the characters that were added are not visible
/// to the player (for example rich text tags). The attributes that follow are shifted back by that amount.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReplacementMarkerResult {
    pub(crate) diagnostics: Vec<MarkupDiagnostic>,
    pub(crate) invisible_characters: usize,
}

impl Clone for Box<dyn AttributeMarkerProcessor> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
