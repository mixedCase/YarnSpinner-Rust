//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/Visitors/StringTableGeneratorVisitor.cs>

use crate::annotations::*;
use crate::diagnostic_descriptors as descriptors;
use crate::prelude::generated::{yarnspinnerparser::*, yarnspinnerparservisitor::*};
use crate::prelude::*;
use crate::string_table_manager::StringTableManager;
use antlr4rust::parser_rule_context::ParserRuleContext;
use antlr4rust::token::Token;
use antlr4rust::tree::{ParseTreeVisitorCompat, Tree};
use std::rc::Rc;

/// The prefix of the hashtag that makes a line a shadow of another line.
pub(crate) const SHADOW_ID_PREFIX: &str = "shadow:";

/// Registers every line of a file in the string table and assigns line IDs to lines that have none.
pub(crate) struct StringTableGeneratorVisitor<'a, 'input> {
    pub(crate) diagnostics: Vec<Diagnostic>,
    current_node_name: String,
    string_table_manager: &'a mut StringTableManager,
    annotations: &'a mut Annotations,
    file: &'a SourceFile<'input>,
    _dummy: (),
}

impl<'a, 'input> StringTableGeneratorVisitor<'a, 'input> {
    pub(crate) fn new(
        string_table_manager: &'a mut StringTableManager,
        annotations: &'a mut Annotations,
        file: &'a SourceFile<'input>,
    ) -> Self {
        Self {
            file,
            string_table_manager,
            annotations,
            diagnostics: Default::default(),
            current_node_name: Default::default(),
            _dummy: (),
        }
    }

    fn add_diagnostic(&mut self, ctx: &impl ParserRuleContext<'input>, description: Diagnostic) {
        self.diagnostics
            .push(self.file.diagnostic_at(ctx).described(description));
    }
}

impl<'input> ParseTreeVisitorCompat<'input> for StringTableGeneratorVisitor<'_, 'input> {
    type Node = YarnSpinnerParserContextType;

    type Return = ();

    fn temp_result(&mut self) -> &mut Self::Return {
        &mut self._dummy
    }
}

impl<'input> YarnSpinnerParserVisitorCompat<'input> for StringTableGeneratorVisitor<'_, 'input> {
    fn visit_node(&mut self, ctx: &NodeContext<'input>) -> Self::Return {
        self.current_node_name = self
            .annotations
            .node(self.file.index, ctx)
            .title
            .unwrap_or_else(|| "<unknown>".to_owned());
        // String table generator: don't crash if a node has no body
        if let Some(body) = ctx.body() {
            self.visit(body.as_ref());
        }
    }

    fn visit_line_statement(&mut self, ctx: &Line_statementContext<'input>) -> Self::Return {
        let hashtags = ctx.hashtag_all();
        let with_prefix = |prefix: &str| -> Vec<Rc<HashtagContextAll<'input>>> {
            hashtags
                .iter()
                .filter(|hashtag| {
                    hashtag
                        .text
                        .as_ref()
                        .is_some_and(|text| text.get_text().starts_with(prefix))
                })
                .cloned()
                .collect()
        };
        let line_ids = with_prefix(LINE_ID_PREFIX);
        let shadow_ids = with_prefix(SHADOW_ID_PREFIX);

        if line_ids.len() + shadow_ids.len() > 1 {
            // Too many IDs; a line may have either a single #line or a single #shadow tag.
            if !line_ids.is_empty() && !shadow_ids.is_empty() {
                for line_id in &line_ids {
                    for shadow_id in &shadow_ids {
                        let description =
                            || descriptors::LINES_CANT_HAVE_LINE_AND_SHADOW_TAG.create(&[]);
                        self.add_diagnostic(shadow_id.as_ref(), description());
                        self.add_diagnostic(line_id.as_ref(), description());
                    }
                }
                return;
            }
            for id in line_ids.iter().chain(&shadow_ids) {
                self.add_diagnostic(
                    id.as_ref(),
                    descriptors::MULTIPLE_LINE_OR_SHADOW_IDS_ON_A_LINE.create(&[]),
                );
            }
            return;
        }

        let text_of = |hashtag: &Rc<HashtagContextAll<'input>>| {
            hashtag.text.as_ref().unwrap().get_text().to_owned()
        };
        let line_id_tag = line_ids.first();
        let line_id: Option<LineId> = line_id_tag.map(|tag| text_of(tag).into());

        if let Some(line_id) = line_id.as_ref()
            && let Some(existing) = self.string_table_manager.get(line_id)
        {
            // Duplicate line ID. Report it on both lines.
            let description = || descriptors::DUPLICATE_LINE_ID.create(&[&line_id.0]);
            let existing_position = existing.position.clone();
            self.add_diagnostic(line_id_tag.unwrap().as_ref(), description());
            self.diagnostics
                .push(existing_position.described(description()));
            return;
        }

        let mut metadata = get_hashtag_texts(&hashtags);
        let key = self.file.key(ctx);
        if self.annotations.last_lines_before_options.contains(&key) {
            metadata.push("lastline".to_owned());
        }

        let shadow_line_id: Option<LineId> = shadow_ids.first().map(|tag| {
            format!(
                "{LINE_ID_PREFIX}{}",
                &text_of(tag)[SHADOW_ID_PREFIX.len()..]
            )
            .into()
        });

        let position = match line_id_tag {
            Some(tag) => self.file.diagnostic_at(tag.as_ref()),
            None => self.file.diagnostic_at(ctx),
        };
        let has_expressions = !ctx
            .line_formatted_text()
            .unwrap()
            .expression_all()
            .is_empty();
        let line_id = self.string_table_manager.insert(
            line_id,
            StringInfo {
                text: generate_formatted_text(&ctx.line_formatted_text().unwrap()),
                node_name: self.current_node_name.clone(),
                line_number: ctx.start().get_line_as_usize(),
                file_name: self.file.name.clone(),
                metadata,
                shadow_line_id: shadow_line_id.clone(),
                is_implicit_tag: false,
            },
            position,
            has_expressions,
        );
        self.annotations.lines.insert(
            key,
            LineAnnotation {
                line_id,
                shadow_line_id,
            },
        );
    }
}

fn generate_formatted_text(ctx: &Line_formatted_textContext) -> String {
    let mut expression_count = 0;
    let mut composed_string = String::new();
    // First, visit all of the nodes, which are either terminal
    // text nodes or expressions. if they're expressions, we
    // evaluate them, and inject a positional reference into the
    // final string.
    for child in ctx.get_children() {
        if child.get_child_count() == 0 {
            composed_string.push_str(&child.get_text());
        } else {
            // Expressions in the final string are denoted as the
            // index of the expression, surrounded by braces { }.
            // However, we don't need to write the braces here
            // ourselves, because the text itself that the parser
            // captured already has them. So, we just need to write
            // the expression count.
            composed_string.push_str(&expression_count.to_string());
            expression_count += 1;
        }
    }
    composed_string.trim().to_owned()
}

pub(crate) fn get_hashtag_texts(hashtags: &[Rc<HashtagContextAll>]) -> Vec<String> {
    hashtags
        .iter()
        .filter_map(|t| Some(t.text.as_ref()?.get_text().trim().to_owned()))
        .collect()
}
