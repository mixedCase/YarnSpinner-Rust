use crate::parser_rule_context_ext::ParserRuleContextExt;
use crate::prelude::*;
use annotate_snippets::renderer::DecorStyle;
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use antlr4rust::rule_context::CustomRuleContext;
use antlr4rust::token::Token;
use antlr4rust::token_factory::TokenFactory;
use core::fmt;
use std::fmt::{Display, Formatter};
use std::ops::Range;
use yarnspinner_core::prelude::*;

/// A diagnostic message that describes an error, warning or informational
/// message that the user can take action on.
///
/// Diagnostics are presented to the user as the result of compilation,
/// through the [`Compilation`]'s [`Compilation::warnings`] field when not an error.
/// Otherwise, they are contained in [`CompilerError`], which is in the [`Err`] variant
/// of the [`Result`] returned by [`Compiler::compile`].
///
/// ## Implementation notes
///
/// The properties marked as `Obsolete` were not implemented.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq, Hash))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub struct Diagnostic {
    /// The path, URI or file-name that the issue occurred in.
    pub file_name: Option<String>,

    /// The range of the file indicated by the [`Diagnostic::file_name`] that the issue occurred in.
    pub range: Option<Range<Position>>,

    /// The description of the issue.
    pub message: String,

    /// The source text of [`Diagnostic::file_name`] containing the issue.
    pub context: Option<String>,

    /// The severity of the issue.
    pub severity: DiagnosticSeverity,

    /// The line the context starts on.
    pub start_line: usize,

    /// The stable code identifying the kind of diagnostic, e.g. `YS0050`.
    /// Not set for a few diagnostics that only exist in this implementation.
    pub code: Option<String>,
}

impl Diagnostic {
    pub(crate) fn from_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file_name: Default::default(),
            range: Default::default(),
            context: Default::default(),
            severity: Default::default(),
            start_line: Default::default(),
            code: Default::default(),
        }
    }

    pub(crate) fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Like [`Diagnostic::with_parser_context`], but for a range of tokens.
    pub(crate) fn with_tokens<'input>(
        self,
        start: &impl Token,
        stop: &impl Token,
        token_stream: &ActualTokenStream<'input>,
    ) -> Self {
        use antlr4rust::token_stream::TokenStream;
        self.with_range_in_source(token_range(start, stop), &token_stream.get_all_text())
    }

    /// Sets the range and derives the context shown when printing from the source text.
    pub(crate) fn with_range_in_source(self, range: Range<Position>, source: &str) -> Self {
        let lines_around = lines_around_range(source, &range, 2);
        self.with_range(range)
            .with_context(lines_around.lines)
            .with_start_line(lines_around.first_line)
    }

    pub(crate) fn with_parser_context<'input, T>(
        self,
        ctx: &T,
        token_stream: &ActualTokenStream<'input>,
    ) -> Self
    where
        T: ParserRuleContextExt<'input>,
    <<<<T as CustomRuleContext<'input>>::TF as TokenFactory<'input>>::Inner as Token>::Data as ToOwned>::Owned:
        Into<String>
{
        let lines_above_and_below_offending_line = 2;
        let lines_around = ctx.get_lines_around(token_stream, lines_above_and_below_offending_line);
        let range = ctx.range();

        self.with_range(range)
            .with_context(lines_around.lines)
            .with_start_line(lines_around.first_line)
    }

    pub(crate) fn with_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    pub(crate) fn with_range(mut self, range: impl Into<Range<Position>>) -> Self {
        self.range = Some(range.into());
        self
    }

    pub(crate) fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub(crate) fn with_start_line(mut self, start_line: usize) -> Self {
        self.start_line = start_line;
        self
    }

    pub(crate) fn with_severity(mut self, severity: DiagnosticSeverity) -> Self {
        self.severity = severity;
        self
    }
}

impl Display for Diagnostic {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let label = &self.message;
        let level = match self.severity {
            DiagnosticSeverity::Error => Level::ERROR,
            DiagnosticSeverity::Warning => Level::WARNING,
            DiagnosticSeverity::Info => Level::INFO,
        };
        let report = level.primary_title(label).id(self.code.as_deref().unwrap_or("YS0000")).element(
            Snippet::source(self.context.as_deref().unwrap_or("<unknown line>"))
                .line_start(self.start_line + 1)
                .path(self.file_name.as_deref())
                .fold(false)
                .annotation(AnnotationKind::Primary.span(convert_absolute_range_to_relative(self))),
        );
        // Using `plain` until https://github.com/tokio-rs/tracing/issues/3378 is resolved... bleh
        let renderer = Renderer::plain().decor_style(DecorStyle::Unicode);
        let annotations = renderer.render(&[report]);
        writeln!(f, "{annotations}")?;

        Ok(())
    }
}

fn convert_absolute_range_to_relative(diagnostic: &Diagnostic) -> Range<usize> {
    let Some(range) = diagnostic.range.as_ref() else {
        return 0..1;
    };
    let Some(context) = diagnostic.context.as_ref() else {
        return 0..1;
    };

    let relative_start_line = range.start.line - diagnostic.start_line;
    let annotated_lines = range.end.line - range.start.line;
    let line_lengths: Vec<_> = context
        .lines()
        .map(|line| line.chars().count() + 1)
        .collect();
    let relative_start =
        line_lengths.iter().take(relative_start_line).sum::<usize>() + range.start.character;
    let relative_end: usize = line_lengths
        .iter()
        .take(relative_start_line + annotated_lines)
        .sum::<usize>()
        + range.end.character
        // - 1 because the Diagnostic range is exclusive, but the annotation range is inclusive
        - 1;
    let mut char_indices = context.char_indices().map(|(i, _)| i);
    let byte_start = char_indices.clone().nth(relative_start).unwrap();
    let byte_end = char_indices.nth(relative_end).unwrap_or(byte_start);
    byte_start..(byte_end + 1)
}

/// Trait implemented for `Vec<Diagnostic>` to provide utility methods.
pub trait DiagnosticVec {
    /// Returns `true` if any of the [`Diagnostic`]s in the vector are of [`DiagnosticSeverity::Error`].
    fn has_errors(&self) -> bool;
}

impl DiagnosticVec for Vec<Diagnostic> {
    fn has_errors(&self) -> bool {
        self.iter().any(|d| d.severity == DiagnosticSeverity::Error)
    }
}

/// The severity of the issue.
///
/// ## Implementation notes
///
/// The `Info` variant was not implemented because it was unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq, Default, Hash))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub enum DiagnosticSeverity {
    /// An error.
    ///
    /// If a Yarn source file contains errors, it cannot be compiled,
    /// and the compilation process will fail.
    #[default]
    Error,

    /// A warning.
    ///
    /// Warnings represent possible problems that the user should fix,
    /// but do not cause the compilation process to fail.
    Warning,

    /// An informational diagnostic, like a variable that is declared but never used.
    /// Reported alongside the warnings. New in Yarn Spinner 3.
    Info,
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DiagnosticSeverity::Error => f.write_str("Error"),
            DiagnosticSeverity::Warning => f.write_str("Warning"),
            DiagnosticSeverity::Info => f.write_str("Info"),
        }
    }
}

/// The range from the start of `start` to the end of `stop`, with zero-based lines.
pub(crate) fn token_range(
    start: &(impl Token + ?Sized),
    stop: &(impl Token + ?Sized),
) -> Range<Position> {
    let to_usize = |value: isize| usize::try_from(value).unwrap_or_default();
    let stop_length = to_usize(stop.get_stop() as isize - stop.get_start() as isize + 1);
    let start_position = Position {
        line: to_usize(start.get_line() as isize).saturating_sub(1),
        character: to_usize(start.get_column() as isize),
    };
    let stop_position = Position {
        line: to_usize(stop.get_line() as isize).saturating_sub(1),
        character: to_usize(stop.get_column() as isize) + stop_length,
    };
    start_position..stop_position
}

/// The lines of `source` that `range` covers, plus `surrounding_lines` lines above and below.
fn lines_around_range(
    source: &str,
    range: &Range<Position>,
    surrounding_lines: usize,
) -> LinesAroundResult {
    let first_line = range.start.line.saturating_sub(surrounding_lines);
    let line_count = range.end.line.saturating_sub(first_line) + surrounding_lines + 1;
    let lines = source
        .lines()
        .skip(first_line)
        .take(line_count)
        .collect::<Vec<_>>()
        .join("\n");
    LinesAroundResult { lines, first_line }
}
