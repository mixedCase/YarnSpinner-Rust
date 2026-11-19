//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/Visitors/NodeTrackingVisitor.cs>

use crate::annotations::Annotations;
use crate::parser::generated::yarnspinnerparser::*;
use crate::prelude::generated::yarnspinnerparservisitor::YarnSpinnerParserVisitorCompat;
use antlr4rust::token::Token;
use antlr4rust::tree::{ParseTree, ParseTreeVisitorCompat};
use std::collections::HashSet;

/// Finds the nodes whose visits must be counted: those that are passed to `visited` or `visited_count`
/// anywhere, and those with the header `tracking: always`.
pub(crate) struct NodeTrackingVisitor<'a> {
    pub(crate) tracking_nodes: HashSet<String>,
    pub(crate) ignoring_nodes: HashSet<String>,
    annotations: &'a Annotations,
    file_index: usize,
    _dummy: Option<String>,
}

impl<'a> NodeTrackingVisitor<'a> {
    pub(crate) fn new(annotations: &'a Annotations, file_index: usize) -> Self {
        Self {
            annotations,
            file_index,
            tracking_nodes: Default::default(),
            ignoring_nodes: Default::default(),
            _dummy: None,
        }
    }
}

impl ParseTreeVisitorCompat<'_> for NodeTrackingVisitor<'_> {
    type Node = YarnSpinnerParserContextType;
    type Return = Option<String>;

    fn temp_result(&mut self) -> &mut Self::Return {
        &mut self._dummy
    }
}

#[allow(non_snake_case)]
impl<'input> YarnSpinnerParserVisitorCompat<'input> for NodeTrackingVisitor<'_> {
    fn visit_function_call(&mut self, ctx: &Function_callContext<'input>) -> Self::Return {
        let function_name = ctx.FUNC_ID().map(|id| id.get_text()).unwrap_or_default();
        let parameters = ctx.expression_all();
        if ["visited", "visited_count"].contains(&function_name.as_str())
            && let Some(first) = parameters.first()
        {
            // We aren't bothering to test anything about the value itself.
            // If it isn't a static string we'll get back `None`, so we can ignore it.
            if let Some(node_name) = self.visit(first.as_ref()) {
                self.tracking_nodes.insert(node_name);
            }
        }
        // Calls can be nested, e.g. `max(visited_count("A"), visited_count("B"))`
        for parameter in parameters {
            self.visit(parameter.as_ref());
        }
        None
    }

    fn visit_valueString(&mut self, ctx: &ValueStringContext<'input>) -> Self::Return {
        Some(ctx.STRING()?.get_text().trim_matches('"').to_owned())
    }

    fn visit_node(&mut self, ctx: &NodeContext<'input>) -> Self::Return {
        let node = self.annotations.node(self.file_index, ctx);
        let tracking = ctx
            .header_all()
            .into_iter()
            .filter(|header| {
                header
                    .header_key
                    .as_ref()
                    .is_some_and(|key| key.get_text() == "tracking")
            })
            .filter_map(|header| Some(header.header_value.as_ref()?.get_text().to_owned()))
            .next_back();

        if let (Some(title), Some(tracking)) = (node.title, tracking) {
            match tracking.as_str() {
                "always" => {
                    self.tracking_nodes.insert(title);
                    if let Some(node_group) = node.node_group {
                        self.tracking_nodes.insert(node_group);
                    }
                }
                "never" => {
                    self.ignoring_nodes.insert(title);
                }
                _ => {}
            }
        }

        if let Some(body) = ctx.body() {
            return self.visit(body.as_ref());
        }
        None
    }
}
