//! Facts about parse tree nodes that the compiler passes hand to each other.
//!
//! ## Implementation notes
//!
//! The C# implementation stores these as properties on the parser contexts via partial classes
//! (`ITypedContext.Type`, `Line_statementContext.LineID`, `NodeContext.NodeTitle` and so on).
//! Our generated contexts are immutable, so we keep them in tables keyed by file and token interval.

use crate::listeners::token_range;
use crate::prelude::*;
use crate::type_checker::Ty;
use crate::visitors::{GetHashableInterval, HashableInterval};
use antlr4rust::parser_rule_context::ParserRuleContext;
use antlr4rust::token::Token;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Identifies a parser context: the index of its file and its token interval.
pub(crate) type CtxKey = (usize, HashableInterval);

#[derive(Debug, Clone, Default)]
pub(crate) struct Annotations {
    /// The types of expressions, values, variables and function calls.
    pub(crate) types: HashMap<CtxKey, Ty>,
    /// Keyed by line statement.
    pub(crate) lines: HashMap<CtxKey, LineAnnotation>,
    /// Keyed by node.
    pub(crate) nodes: HashMap<CtxKey, NodeAnnotation>,
    /// The variable that remembers whether a `<<once>>` block ran, keyed by its primary clause.
    pub(crate) once_variables: HashMap<CtxKey, String>,
    /// The line statements that are directly followed by options.
    pub(crate) last_lines_before_options: HashSet<CtxKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineAnnotation {
    pub(crate) line_id: LineId,
    pub(crate) shadow_line_id: Option<LineId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NodeAnnotation {
    /// The unique name of the node. For members of a node group, this is not what the `title` header says.
    pub(crate) title: Option<String>,
    /// The name of the node group that this node is a member of.
    pub(crate) node_group: Option<String>,
}

impl Annotations {
    pub(crate) fn type_of<'input>(
        &self,
        file: usize,
        ctx: &(impl ParserRuleContext<'input> + ?Sized),
    ) -> Ty {
        self.types
            .get(&(file, ctx.get_hashable_interval()))
            .cloned()
            .unwrap_or(Ty::Error)
    }

    pub(crate) fn line<'input>(
        &self,
        file: usize,
        ctx: &(impl ParserRuleContext<'input> + ?Sized),
    ) -> Option<&LineAnnotation> {
        self.lines.get(&(file, ctx.get_hashable_interval()))
    }

    /// The line ID that was registered for the line statement in the string table.
    pub(crate) fn line_id<'input>(
        &self,
        file: usize,
        ctx: &(impl ParserRuleContext<'input> + ?Sized),
    ) -> LineId {
        self.line(file, ctx)
            .expect("Internal error: line does not have a line ID")
            .line_id
            .clone()
    }

    pub(crate) fn node<'input>(
        &self,
        file: usize,
        ctx: &(impl ParserRuleContext<'input> + ?Sized),
    ) -> NodeAnnotation {
        self.nodes
            .get(&(file, ctx.get_hashable_interval()))
            .cloned()
            .unwrap_or_default()
    }
}

/// A file being compiled: its parse tree plus what is needed to report diagnostics about it.
#[derive(Clone)]
pub(crate) struct SourceFile<'input> {
    pub(crate) index: usize,
    pub(crate) name: String,
    /// The lines of the source text, for showing the surroundings of a diagnostic
    pub(crate) lines: std::rc::Rc<Vec<&'input str>>,
    pub(crate) parsed: FileParseResult<'input>,
}

impl<'input> SourceFile<'input> {
    pub(crate) fn key(&self, ctx: &(impl ParserRuleContext<'input> + ?Sized)) -> CtxKey {
        (self.index, ctx.get_hashable_interval())
    }

    /// An empty diagnostic that points at the given range of this file.
    pub(crate) fn diagnostic_at_range(&self, range: Range<Position>) -> Diagnostic {
        // The lines that the range covers, plus two lines above and below
        let first_line = range.start.line.saturating_sub(2);
        let last_line = (range.end.line + 2).min(self.lines.len().saturating_sub(1));
        let context = self
            .lines
            .get(first_line..=last_line)
            .unwrap_or_default()
            .join("\n");
        Diagnostic::from_message("")
            .with_file_name(&self.name)
            .with_range(range)
            .with_context(context)
            .with_start_line(first_line)
    }

    /// An empty diagnostic that points at the given context.
    pub(crate) fn diagnostic_at(
        &self,
        ctx: &(impl ParserRuleContext<'input> + ?Sized),
    ) -> Diagnostic {
        self.diagnostic_at_range(ctx_range(ctx))
    }

    pub(crate) fn diagnostic_at_token(&self, token: &(impl Token + ?Sized)) -> Diagnostic {
        self.diagnostic_at_range(token_range(token, token))
    }
}

pub(crate) fn ctx_range<'input>(
    ctx: &(impl ParserRuleContext<'input> + ?Sized),
) -> Range<Position> {
    token_range(&*ctx.start(), &*ctx.stop())
}

/// Fills in what a [`crate::diagnostic_descriptors::DiagnosticDescriptor`] produced into a diagnostic that already has its position.
pub(crate) trait DiagnosticExt {
    fn described(self, description: Diagnostic) -> Diagnostic;
}

impl DiagnosticExt for Diagnostic {
    fn described(self, description: Diagnostic) -> Diagnostic {
        Diagnostic {
            message: description.message,
            code: description.code,
            severity: description.severity,
            ..self
        }
    }
}
