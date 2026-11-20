//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner/YarnSpinner.Markup/LineParser.cs>
//!
//! The parser of Yarn Spinner 3 works in three steps: the line is split into tokens ([`lex_markup`]),
//! the tokens are assembled into a tree of markers and text ([`LineParser::build_markup_tree`]),
//! and the tree is walked to produce the final text and its attributes ([`LineParser::walk_tree`]).
//! Markers that have a processor registered, like `[select/]`, are replaced by text while walking.
//!
//! ## Implementation notes
//!
//! - Positions and lengths of attributes are counted in text elements (grapheme clusters), as in previous versions of this crate.
//!   The original counts UTF-16 code units.
//! - Malformed markup is reported as [`MarkupParseError`]. The original silently returns the unparsed line.

use crate::markup::*;
use crate::prelude::*;
use bevy_platform::collections::HashMap;
use once_cell::sync::Lazy;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// A result type for the line parser
pub type Result<T> = core::result::Result<T, MarkupParseError>;

/// The name of the implicitly-generated `character` attribute.
pub const CHARACTER_ATTRIBUTE: &str = "character";

/// The name of the 'name' property, on the implicitly-generated `character` attribute.
pub const CHARACTER_ATTRIBUTE_NAME_PROPERTY: &str = "name";

/// The name of the property to use to signify that trailing whitespace should be trimmed
/// if a tag had preceding whitespace or begins the line. This property must be a bool value.
pub const TRIM_WHITESPACE_PROPERTY: &str = "trimwhitespace";

/// The name of the marker whose contents are not parsed as markup.
const NO_MARKUP_ATTRIBUTE: &str = "nomarkup";

/// The property that marks the parts of an attribute that was split to fix overlapping markers.
const INTERNAL_INCREMENT: &str = "_internalIncrementingProperty";

/// Matches everything up to the first colon that is not escaped with a backslash, followed by optional whitespace.
/// The first group is the name of the character.
static IMPLICIT_CHARACTER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^((?:[^:\\]|\\.)*):\s*").unwrap());

/// Matches a `[character` at the start of the string, which means that the line names its character explicitly.
static CHARACTER_MARKER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\[character").unwrap());

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub(crate) struct LineParser {
    /// A map for the names of attributes to an object that can generate replacement text for those attributes.
    #[cfg_attr(feature = "bevy", reflect(ignore))]
    #[cfg_attr(feature = "serde", serde(skip))]
    marker_processors: HashMap<String, Box<dyn AttributeMarkerProcessor>>,
    #[cfg_attr(feature = "bevy", reflect(ignore))]
    language_code: Option<Language>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenType {
    Text,
    OpenMarker,
    CloseMarker,
    CloseSlash,
    Identifier,
    Error,
    Start,
    End,
    Equals,
    StringValue,
    NumberValue,
    BooleanValue,
    InterpolatedValue,
}

/// A piece of the line. `start` and `end` are inclusive indices into the characters of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Token {
    token_type: TokenType,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexerMode {
    Text,
    Tag,
    Value,
}

fn lex_markup(input: &[char]) -> Vec<Token> {
    use TokenType::*;
    let token = |token_type, start, end| Token {
        token_type,
        start,
        end,
    };
    let mut tokens = vec![token(Start, 0, 0)];
    if input.is_empty() {
        tokens.push(token(End, 0, 0));
        return tokens;
    }
    let is_identifier_part = |c: char| c.is_alphanumeric() || c == '_' || c == '|';
    let mut mode = LexerMode::Text;
    let mut position = 0;
    while position < input.len() {
        let c = input[position];
        let peek = |position: usize| input.get(position + 1).copied();
        match mode {
            LexerMode::Text => {
                let last = tokens.last_mut().unwrap();
                let last_is_text = last.token_type == Text;
                // An escaped bracket is text, not the start of a marker
                let is_escaped = last_is_text && input[last.end] == '\\';
                if c == '[' && !is_escaped {
                    tokens.push(token(OpenMarker, position, position));
                    mode = LexerMode::Tag;
                } else if last_is_text {
                    last.end = position;
                } else {
                    tokens.push(token(Text, position, position));
                }
            }
            LexerMode::Tag => match c {
                ']' => {
                    tokens.push(token(CloseMarker, position, position));
                    mode = LexerMode::Text;
                }
                '/' => tokens.push(token(CloseSlash, position, position)),
                '=' => {
                    tokens.push(token(Equals, position, position));
                    mode = LexerMode::Value;
                }
                c if c.is_alphanumeric() => {
                    let start = position;
                    while peek(position).is_some_and(is_identifier_part) {
                        position += 1;
                    }
                    tokens.push(token(Identifier, start, position));
                }
                c if !c.is_whitespace() => {
                    // Something that can't be part of a marker
                    tokens.push(token(Error, position, position));
                    mode = LexerMode::Text;
                }
                _ => {}
            },
            LexerMode::Value if c.is_whitespace() => {}
            LexerMode::Value => {
                let start = position;
                let token_type = if c.is_ascii_digit() || c == '-' {
                    let is_number_part = |c: char| c.is_ascii_digit() || c == '.';
                    let mut ran_off_the_end = false;
                    if peek(position).is_some_and(is_number_part) {
                        loop {
                            position += 1;
                            match peek(position) {
                                None => {
                                    ran_off_the_end = true;
                                    break;
                                }
                                Some(next) if !is_number_part(next) => break,
                                _ => {}
                            }
                        }
                    }
                    let text: String = input[start..=position].iter().collect();
                    if !ran_off_the_end && text.parse::<f32>().is_ok() {
                        NumberValue
                    } else {
                        Error
                    }
                } else if c == '"' {
                    // The string ends at the first quote that is not escaped
                    let closing_quote = (position + 1..input.len())
                        .find(|&i| input[i] == '"' && input[i - 1] != '\\');
                    match closing_quote {
                        Some(closing_quote) => {
                            position = closing_quote;
                            StringValue
                        }
                        None => Error,
                    }
                } else if c == '{' {
                    // The value of an expression that was not substituted
                    let mut exited = false;
                    while position + 1 < input.len() {
                        position += 1;
                        if input[position] == '}' {
                            exited = true;
                            break;
                        }
                    }
                    if exited { InterpolatedValue } else { Error }
                } else {
                    // A single word
                    while peek(position).is_some_and(char::is_alphanumeric) {
                        position += 1;
                    }
                    let text: String = input[start..=position].iter().collect();
                    if ["true", "True", "false", "False"].contains(&text.as_str()) {
                        BooleanValue
                    } else {
                        StringValue
                    }
                };
                tokens.push(token(token_type, start, position));
                mode = LexerMode::Tag;
            }
        }
        position += 1;
    }
    tokens.push(token(End, position, input.len() - 1));
    tokens
}

/// A node of the markup tree: either a piece of text, or a marker with children. The root is a marker without a name.
#[derive(Debug, Clone, Default)]
struct MarkupTreeNode {
    name: Option<String>,
    /// The position of the first character of the marker in the line
    source_position: Option<usize>,
    children: Vec<usize>,
    properties: Vec<(String, MarkupValue)>,
    /// Set for text nodes
    text: Option<String>,
}

impl MarkupTreeNode {
    fn property(&self, name: &str) -> Option<&MarkupValue> {
        self.properties
            .iter()
            .find(|(property_name, _)| property_name == name)
            .map(|(_, value)| value)
    }
}

/// The nodes of a markup tree. Nodes refer to each other by index. The root is at index 0.
type MarkupTree = Vec<MarkupTreeNode>;

/// What [`LineParser::walk_tree`] keeps track of across the whole tree.
#[derive(Default)]
struct WalkState {
    /// The text or marker that precedes the node being visited. It decides whether whitespace is trimmed.
    sibling: Option<usize>,
    /// How many characters the replacement markers added that are not visible, like rich text tags.
    invisible_characters: usize,
    diagnostics: Vec<MarkupDiagnostic>,
}

/// Returns a new string whose textual value is the same as this string, but whose binary representation is in Unicode normalization form C.
pub(crate) fn normalize(string: &str) -> String {
    string.nfc().to_string()
}

/// The number of text elements in a text
fn length_of(text: &str) -> usize {
    text.graphemes(true).count()
}

impl LineParser {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register_marker_processor(
        mut self,
        attribute_name: impl Into<String>,
        processor: Box<dyn AttributeMarkerProcessor>,
    ) -> Self {
        let attribute_name = attribute_name.into();
        let previous_value = self
            .marker_processors
            .insert(attribute_name.clone(), processor);
        assert_or_bug!(
            previous_value.is_none(),
            "A marker processor for the attribute '{attribute_name}' has already been added."
        );
        self
    }

    pub(crate) fn set_language_code(&mut self, language_code: impl Into<Option<Language>>) {
        self.language_code = language_code.into();
    }

    /// Parses a line of text, and produces a [`ParsedMarkup`] containing the processed text and its attributes.
    pub(crate) fn parse_markup(&mut self, input: &str) -> Result<ParsedMarkup> {
        if input.is_empty() {
            // We got a null input; return an empty markup parse result
            return Ok(ParsedMarkup::new());
        }
        let input = normalize(input);
        // The character name is found before all other processing takes place,
        // so that text produced by replacement markers is never mistaken for one.
        let input = add_implicit_character_attribute(&input);
        // Escaped colons only exist to not be mistaken for the end of a character name
        let input = input.replace("\\:", ":");
        let error = |diagnostics| MarkupParseError::InvalidMarkup {
            input: input.clone(),
            diagnostics,
        };

        let characters: Vec<char> = input.chars().collect();
        let tokens = lex_markup(&characters);
        let (tree, diagnostics) = self.build_markup_tree(&tokens, &characters);
        if !diagnostics.is_empty() {
            return Err(error(diagnostics));
        }

        let mut text = String::new();
        let mut attributes = Vec::new();
        let mut state = WalkState::default();
        self.walk_tree(&tree, 0, &mut text, &mut attributes, &mut state, 0);
        if !state.diagnostics.is_empty() {
            return Err(error(state.diagnostics));
        }

        squish_split_attributes(&mut attributes);
        attributes.sort_by_key(|attribute| attribute.source_position);
        Ok(ParsedMarkup { text, attributes })
    }

    /// Assembles tokens into a tree. Markers that overlap, like in `[a][b]text[/a][/b]`,
    /// are split so that they nest: `[a][b]text[/b][/a][b][/b]`. The parts are merged again later.
    fn build_markup_tree(
        &self,
        tokens: &[Token],
        input: &[char],
    ) -> (MarkupTree, Vec<MarkupDiagnostic>) {
        use TokenType::*;
        let mut tree: MarkupTree = vec![MarkupTreeNode::default()];
        let mut diagnostics = Vec::new();
        let mut next_internal_id = 1;

        let text_of = |token: Token| -> String { input[token.start..=token.end].iter().collect() };
        // Tokens beyond the end of the line are `End` tokens
        let at = |index: usize| -> Token {
            tokens.get(index).copied().unwrap_or(Token {
                token_type: End,
                start: input.len(),
                end: input.len().saturating_sub(1),
            })
        };
        let matches = |index: usize, pattern: &[TokenType]| {
            pattern
                .iter()
                .enumerate()
                .all(|(offset, token_type)| at(index + offset).token_type == *token_type)
        };

        // The markers that were opened and not closed yet, innermost last. The root is never closed.
        let mut open_nodes: Vec<usize> = vec![0];
        // The names of the markers that were closed while another marker inside them was still open
        let mut unmatched_closes: Vec<String> = Vec::new();

        let mut index = 0;
        while at(index).token_type != End {
            let current = at(index);
            match current.token_type {
                Text => {
                    if !unmatched_closes.is_empty() {
                        clean_up_unmatched_closes(
                            &mut tree,
                            &mut open_nodes,
                            &mut unmatched_closes,
                            &mut diagnostics,
                            &mut next_internal_id,
                        );
                    }
                    add_child(
                        &mut tree,
                        &open_nodes,
                        MarkupTreeNode {
                            text: Some(text_of(current)),
                            source_position: Some(current.start),
                            ..Default::default()
                        },
                    );
                }
                OpenMarker => 'marker: {
                    if matches(index, &[OpenMarker, CloseSlash, CloseMarker]) {
                        // [/] closes all open markers
                        index += 2;
                        while open_nodes.len() > 1 {
                            let closed = open_nodes.pop().unwrap();
                            if let Some(name) = tree[closed].name.as_ref()
                                && let Some(position) = unmatched_closes
                                    .iter()
                                    .position(|unmatched| unmatched == name)
                            {
                                unmatched_closes.remove(position);
                            }
                        }
                        for remaining in unmatched_closes.drain(..) {
                            diagnostics.push(MarkupDiagnostic::new(
                                format!("asked to close \"{remaining}\" markup but there is no corresponding opening. Is [/{remaining}] a typo?"),
                                at(index).start,
                            ));
                        }
                        break 'marker;
                    }
                    if matches(index, &[OpenMarker, CloseSlash, Identifier, CloseMarker]) {
                        // [/name] closes the marker of that name
                        let close_id_token = at(index + 2);
                        let close_id = text_of(close_id_token);
                        index += 3;
                        if open_nodes.len() == 1 {
                            diagnostics.push(MarkupDiagnostic::new(
                                format!("Asked to close \"{close_id}\", but we don't have an open marker for it."),
                                close_id_token.start,
                            ));
                        } else if tree[*open_nodes.last().unwrap()].name.as_deref()
                            == Some(&close_id)
                        {
                            open_nodes.pop();
                        } else {
                            // The marker is closed while markers inside it are still open.
                            // We fix that up when we know what follows.
                            unmatched_closes.push(close_id);
                        }
                        break 'marker;
                    }
                    if matches(index, &[OpenMarker, CloseSlash]) {
                        diagnostics.push(MarkupDiagnostic::new(
                            format!(
                                "Error parsing markup, detected invalid token {:?}, following a close.",
                                at(index + 2).token_type
                            ),
                            current.start,
                        ));
                        break 'marker;
                    }
                    let id_token = at(index + 1);
                    if id_token.token_type != Identifier {
                        diagnostics.push(MarkupDiagnostic::new(
                            format!(
                                "Error parsing markup, detected invalid token {:?}, following an open marker.",
                                id_token.token_type
                            ),
                            id_token.start,
                        ));
                        break 'marker;
                    }
                    if !unmatched_closes.is_empty() {
                        clean_up_unmatched_closes(
                            &mut tree,
                            &mut open_nodes,
                            &mut unmatched_closes,
                            &mut diagnostics,
                            &mut next_internal_id,
                        );
                    }
                    let id = text_of(id_token);

                    if matches(index, &[OpenMarker, Identifier, CloseMarker]) {
                        if id == NO_MARKUP_ATTRIBUTE {
                            // Everything up to [/nomarkup] is text, no matter what it looks like
                            let first_token_after_no_markup = at(index + 3);
                            let mut found_exit = false;
                            while at(index).token_type != End {
                                if matches(
                                    index,
                                    &[OpenMarker, CloseSlash, Identifier, CloseMarker],
                                ) && text_of(at(index + 2)) == NO_MARKUP_ATTRIBUTE
                                {
                                    let text: String = input
                                        [first_token_after_no_markup.start..at(index).start]
                                        .iter()
                                        .collect();
                                    let no_markup = add_child(
                                        &mut tree,
                                        &open_nodes,
                                        MarkupTreeNode {
                                            name: Some(NO_MARKUP_ATTRIBUTE.to_owned()),
                                            source_position: Some(current.start),
                                            ..Default::default()
                                        },
                                    );
                                    add_child(
                                        &mut tree,
                                        &[no_markup],
                                        MarkupTreeNode {
                                            text: Some(text),
                                            ..Default::default()
                                        },
                                    );
                                    index += 3;
                                    found_exit = true;
                                    break;
                                }
                                index += 1;
                            }
                            if !found_exit {
                                diagnostics.push(MarkupDiagnostic::new(
                                    "we entered nomarkup mode but didn't find an exit token",
                                    current.start,
                                ));
                            }
                        } else {
                            // A marker without properties: [name]
                            let marker = add_child(
                                &mut tree,
                                &open_nodes,
                                MarkupTreeNode {
                                    name: Some(id),
                                    source_position: Some(current.start),
                                    ..Default::default()
                                },
                            );
                            open_nodes.push(marker);
                            index += 2;
                        }
                        break 'marker;
                    }

                    if at(index + 2).token_type == Error {
                        let invalid_name: String =
                            input[id_token.start..at(index + 2).end].iter().collect();
                        diagnostics.push(MarkupDiagnostic::new(
                            format!("Error parsing markup, invalid name: \"{invalid_name}\""),
                            id_token.start,
                        ));
                        index += 2;
                        break 'marker;
                    }

                    // A marker with properties. They are added by the identifier tokens that follow.
                    let marker = add_child(
                        &mut tree,
                        &open_nodes,
                        MarkupTreeNode {
                            name: Some(id),
                            source_position: Some(current.start),
                            ..Default::default()
                        },
                    );
                    open_nodes.push(marker);
                    // In the shorthand `[name=value]`, the name is also the name of the first property
                    if at(index + 2).token_type != Equals {
                        index += 1;
                    }
                }
                Identifier => {
                    let id = text_of(current);
                    let value_token = at(index + 2);
                    let value_text = text_of(value_token);
                    let value = if matches(index, &[Identifier, Equals, NumberValue]) {
                        value_text
                            .parse::<u32>()
                            .map(MarkupValue::Integer)
                            .or_else(|_| value_text.parse::<f32>().map(MarkupValue::Float))
                            .ok()
                    } else if matches(index, &[Identifier, Equals, BooleanValue]) {
                        Some(MarkupValue::Bool(value_text.eq_ignore_ascii_case("true")))
                    } else if matches(index, &[Identifier, Equals, StringValue]) {
                        let is_quoted = value_text.len() >= 2
                            && value_text.starts_with('"')
                            && value_text.ends_with('"');
                        Some(MarkupValue::String(if is_quoted {
                            // sic: all backslashes are removed, not only the ones that escape something
                            let unescaped = value_text.replace('\\', "");
                            unescaped[1..unescaped.len() - 1].to_owned()
                        } else {
                            value_text.clone()
                        }))
                    } else if matches(index, &[Identifier, Equals, InterpolatedValue]) {
                        Some(MarkupValue::String(
                            value_text.trim_matches(['{', '}']).to_owned(),
                        ))
                    } else {
                        diagnostics.push(MarkupDiagnostic::new(
                            format!(
                                "Expected to find a property and it's value, but instead found \"{id} {} {value_text}\".",
                                text_of(at(index + 1))
                            ),
                            at(index + 1).start,
                        ));
                        index += 3;
                        continue;
                    };
                    match value {
                        Some(value) => tree[*open_nodes.last().unwrap()]
                            .properties
                            .push((id, value)),
                        None => diagnostics.push(MarkupDiagnostic::new(
                            format!(
                                "failed to convert the value {value_text} into a valid property"
                            ),
                            value_token.start,
                        )),
                    }
                    index += 2;
                }
                CloseSlash => {
                    if matches(index, &[CloseSlash, CloseMarker]) && open_nodes.len() > 1 {
                        // A self-closing marker: [name /]. Unless it says otherwise, it trims the whitespace after it.
                        let top = open_nodes.pop().unwrap();
                        if tree[top].property(TRIM_WHITESPACE_PROPERTY).is_none() {
                            tree[top]
                                .properties
                                .push((TRIM_WHITESPACE_PROPERTY.to_owned(), true.into()));
                        }
                        index += 1;
                    } else {
                        diagnostics.push(MarkupDiagnostic::new(
                            "Encountered an unexpected closing slash",
                            current.start,
                        ));
                    }
                }
                // Markers are closed by the `OpenMarker` case; everything else is consumed by its predecessor
                _ => {}
            }
            index += 1;
        }

        // ## Implementation note
        // The original only does this if more than one close is unmatched, which rejects
        // overlapping markers at the very end of a line, like `[a][b]text[/a][/b]`.
        if !unmatched_closes.is_empty() {
            clean_up_unmatched_closes(
                &mut tree,
                &mut open_nodes,
                &mut unmatched_closes,
                &mut diagnostics,
                &mut next_internal_id,
            );
        }
        if open_nodes.len() > 1 {
            let names: Vec<_> = open_nodes
                .iter()
                .rev()
                .filter_map(|node| tree[*node].name.as_ref())
                .map(|name| format!("[{name}]"))
                .collect();
            diagnostics.push(MarkupDiagnostic::without_position(format!(
                "parsing finished with unclosed attributes still on the stack: {}",
                names.join(", ")
            )));
        }
        (tree, diagnostics)
    }

    /// Appends the text of the node at `index` to `builder` and the attributes inside it to `attributes`.
    /// `offset` is the position of `builder` in the final text.
    fn walk_tree(
        &self,
        tree: &MarkupTree,
        index: usize,
        builder: &mut String,
        attributes: &mut Vec<MarkupAttribute>,
        state: &mut WalkState,
        offset: usize,
    ) {
        let node = &tree[index];
        if let Some(text) = node.text.as_ref() {
            let mut line = text.as_str();
            // If the marker before this text asks for it, the whitespace that follows the marker is removed
            let trims_whitespace = state
                .sibling
                .and_then(|sibling| tree[sibling].property(TRIM_WHITESPACE_PROPERTY))
                .is_some_and(|value| *value == MarkupValue::Bool(true));
            if trims_whitespace && line.starts_with(char::is_whitespace) {
                let mut characters = line.chars();
                characters.next();
                line = characters.as_str();
            }
            // Brackets are escaped so that they aren't markup. Now they're just brackets.
            builder.push_str(&line.replace("\\[", "[").replace("\\]", "]"));
            state.sibling = Some(index);
            return;
        }

        let mut child_builder = String::new();
        let mut child_attributes = Vec::new();
        let position = length_of(builder) + offset;
        for child in &node.children {
            self.walk_tree(
                tree,
                *child,
                &mut child_builder,
                &mut child_attributes,
                state,
                position,
            );
        }

        if let Some(name) = node.name.as_ref() {
            let mut attribute = MarkupAttribute {
                name: name.clone(),
                position,
                length: length_of(&child_builder),
                properties: node.properties.iter().cloned().collect(),
                source_position: node.source_position.unwrap_or_default(),
            };
            if let Some(processor) = self.marker_processors.get(name) {
                // This marker is replaced by text. It doesn't become an attribute itself.
                let result = processor.process_replacement_marker(
                    &attribute,
                    &mut child_builder,
                    &mut child_attributes,
                    self.language_code.as_ref(),
                );
                state.diagnostics.extend(result.diagnostics);
                state.invisible_characters += result.invisible_characters;
            } else {
                attribute.position = position.saturating_sub(state.invisible_characters);
                attributes.push(attribute);
                state.sibling = Some(index);
            }
        }
        builder.push_str(&child_builder);
        attributes.append(&mut child_attributes);
    }
}

fn add_child(tree: &mut MarkupTree, open_nodes: &[usize], node: MarkupTreeNode) -> usize {
    let index = tree.len();
    tree.push(node);
    tree[*open_nodes.last().unwrap()].children.push(index);
    index
}

/// Closes the markers named in `unmatched_closes`, which are open somewhere below the innermost open marker.
/// The markers above them are closed too and opened again afterwards, so that the tree stays a tree.
/// These split markers get an ID, so that [`squish_split_attributes`] can merge them again.
fn clean_up_unmatched_closes(
    tree: &mut MarkupTree,
    open_nodes: &mut Vec<usize>,
    unmatched_closes: &mut Vec<String>,
    diagnostics: &mut Vec<MarkupDiagnostic>,
    next_internal_id: &mut u32,
) {
    let mut orphans = Vec::new();
    while !unmatched_closes.is_empty() && open_nodes.len() > 1 {
        let top = open_nodes.pop().unwrap();
        if tree[top].property(INTERNAL_INCREMENT).is_none() {
            tree[top]
                .properties
                .push((INTERNAL_INCREMENT.to_owned(), (*next_internal_id).into()));
            *next_internal_id += 1;
        }
        let Some(name) = tree[top].name.clone() else {
            continue;
        };
        match unmatched_closes
            .iter()
            .position(|unmatched| *unmatched == name)
        {
            Some(position) => {
                unmatched_closes.remove(position);
            }
            None => orphans.push(top),
        }
    }
    if !unmatched_closes.is_empty() {
        for unmatched in unmatched_closes.drain(..) {
            diagnostics.push(MarkupDiagnostic::without_position(format!(
                "asked to close \"{unmatched}\" markup but there is no corresponding opening. Is [/{unmatched}] a typo?"
            )));
        }
        return;
    }
    // Reopen the markers that were only closed to keep the nesting valid, outermost first
    for orphan in orphans.into_iter().rev() {
        let clone = MarkupTreeNode {
            children: Vec::new(),
            ..tree[orphan].clone()
        };
        let reopened = add_child(tree, open_nodes, clone);
        open_nodes.push(reopened);
    }
}

/// Merges the attributes that were split by [`clean_up_unmatched_closes`] back into one.
fn squish_split_attributes(attributes: &mut Vec<MarkupAttribute>) {
    let mut merged: Vec<(MarkupValue, MarkupAttribute)> = Vec::new();
    attributes.retain(|attribute| {
        let Some(id) = attribute.properties.get(INTERNAL_INCREMENT) else {
            return true;
        };
        match merged.iter_mut().find(|(merged_id, _)| merged_id == id) {
            Some((_, existing)) => {
                existing.position = existing.position.min(attribute.position);
                existing.length += attribute.length;
            }
            None => merged.push((id.clone(), attribute.clone())),
        }
        false
    });
    for (_, mut attribute) in merged {
        // The ID has done its job
        attribute.properties.remove(INTERNAL_INCREMENT);
        attributes.push(attribute);
    }
}

/// If the line starts with a character name, like `Mae: Hello`, wraps the name into a `[character]` marker.
fn add_implicit_character_attribute(input: &str) -> String {
    if CHARACTER_MARKER.is_match(input) {
        return input.to_owned();
    }
    IMPLICIT_CHARACTER
        .replace(input, |captures: &regex::Captures| {
            // Quotes in the name would end the property value early
            let name = captures[1].replace('"', "\\\"");
            format!("[character name=\"{name}\"]{}[/character]", &captures[0])
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_types(line: &str) -> Vec<TokenType> {
        let characters: Vec<char> = line.chars().collect();
        lex_markup(&characters)
            .into_iter()
            .map(|token| token.token_type)
            .collect()
    }

    #[test]
    fn lexes_markers_and_properties() {
        use TokenType::*;
        assert_eq!(
            vec![
                Start,
                Text,
                OpenMarker,
                Identifier,
                Identifier,
                Equals,
                NumberValue,
                Identifier,
                Equals,
                StringValue,
                Identifier,
                Equals,
                BooleanValue,
                CloseSlash,
                CloseMarker,
                Text,
                End
            ],
            token_types("a [b c=1.5 d=\"e \\\" f\" g=true /] h")
        );
        assert_eq!(vec![Start, Text, End], token_types("\\[b\\]"));
        assert_eq!(
            vec![
                Start, OpenMarker, Identifier, Equals, Error, Identifier, End
            ],
            token_types("[a=\"unterminated")
        );
    }

    #[test]
    fn splits_and_merges_overlapping_markers() {
        let markup = LineParser::new().parse_markup("[a]A[b]B[/a]C[/b]").unwrap();
        assert_eq!("ABC", markup.text);
        let attribute = |name: &str| markup.attributes.iter().find(|a| a.name == name).unwrap();
        assert_eq!((0, 2), (attribute("a").position, attribute("a").length));
        assert_eq!((1, 2), (attribute("b").position, attribute("b").length));
        assert!(attribute("b").properties.is_empty());
    }

    #[test]
    fn reports_malformed_markup() {
        for line in [
            "[a]unclosed",
            "closed [/a] without open",
            "[a b=]x[/a]",
            "[nomarkup]x",
        ] {
            assert!(LineParser::new().parse_markup(line).is_err(), "{line}");
        }
    }
}
