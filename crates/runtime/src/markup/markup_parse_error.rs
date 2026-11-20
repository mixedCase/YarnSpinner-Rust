//! Adapted from the diagnostics of <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner/YarnSpinner.Markup/LineParser.cs>

use crate::prelude::*;
use core::error::Error;
use core::fmt;

/// The markup of a line could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq, Hash))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
#[non_exhaustive]
pub enum MarkupParseError {
    /// The markup is malformed, or a replacement marker like `[select/]` could not be replaced.
    InvalidMarkup {
        /// The line, after the name of the character was wrapped into a `[character]` marker.
        input: String,
        /// What is wrong with the line. Never empty.
        diagnostics: Vec<MarkupDiagnostic>,
    },
}

/// A problem with the markup of a line.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq, Hash))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub struct MarkupDiagnostic {
    /// The description of the problem.
    pub message: String,
    /// The position of the problem in the line, if it can be attributed to one.
    pub column: Option<usize>,
}

impl MarkupDiagnostic {
    pub(crate) fn new(message: impl Into<String>, column: usize) -> Self {
        Self {
            message: message.into(),
            column: Some(column),
        }
    }

    pub(crate) fn without_position(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            column: None,
        }
    }
}

impl Error for MarkupParseError {}

impl fmt::Display for MarkupParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarkupParseError::InvalidMarkup { input, diagnostics } => {
                write!(f, "Error parsing line \"{input}\":")?;
                for diagnostic in diagnostics {
                    match diagnostic.column {
                        Some(column) => {
                            write!(f, " {} (at position {column}).", diagnostic.message)?
                        }
                        None => write!(f, " {}.", diagnostic.message)?,
                    }
                }
                Ok(())
            }
        }
    }
}
