//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/Compiler.cs>,
//! <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/FileCompiler.cs>,
//! <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/SmartVariableCompiler.cs>
//! and <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/NodeGroupCompiler.cs>

use crate::Result;
use crate::annotations::*;
use crate::diagnostic_descriptors as descriptors;
use crate::prelude::generated::yarnspinnerparser::*;
use crate::prelude::generated::yarnspinnerparserlistener::YarnSpinnerParserListener;
use crate::listeners::token_range;
use crate::prelude::*;
use crate::string_table_manager::StringTableManager;
use crate::type_checker::*;
use crate::visitors::*;
use antlr4rust::parser_rule_context::ParserRuleContext;
use antlr4rust::token::Token;
use antlr4rust::tree::{ParseTree, ParseTreeListener, ParseTreeVisitorCompat, Tree};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use yarnspinner_core::types::FunctionType;

/// The language version from which on the features introduced by Yarn Spinner 3 are available.
const VERSION_WITH_YARN_SPINNER_3_FEATURES: u32 = 3;

/// Compile Yarn code, as specified by a compilation job.
pub(crate) fn compile(compiler: &Compiler) -> Result<Compilation> {
    let chars: Vec<Vec<u32>> = compiler
        .files
        .iter()
        .map(|file| {
            // Strip the BOM from the source string if it is present before compiling.
            // Rust does not do this by default
            // https://github.com/rust-lang/rfcs/issues/2428
            let source = file.source.strip_prefix('\u{feff}').unwrap_or(&file.source);
            source.chars().map(|c| c as u32).collect()
        })
        .collect();

    let (compilation, diagnostics) = compile_files(compiler, &chars);

    // Make sure that diagnostics are unique, and that errors fail the compilation.
    let mut unique_diagnostics: Vec<Diagnostic> = Vec::new();
    let mut seen = HashSet::new();
    for diagnostic in diagnostics {
        if seen.insert(diagnostic.clone()) {
            unique_diagnostics.push(diagnostic);
        }
    }
    match compilation {
        Some(compilation) if !unique_diagnostics.has_errors() => Ok(Compilation {
            warnings: unique_diagnostics,
            ..compilation
        }),
        _ => Err(CompilerError(unique_diagnostics)),
    }
}

/// Returns the compilation, unless the errors among the returned diagnostics prevented it.
fn compile_files<'input>(
    compiler: &'input Compiler,
    chars: &'input [Vec<u32>],
) -> (Option<Compilation>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut state = TypeCheckState {
        known_types: BUILTIN_TYPES.to_vec(),
        language_version: compiler.language_version,
        ..Default::default()
    };

    // All variable and function declarations that we know about before reading the files
    for declaration in compiler
        .variable_declarations
        .iter()
        .cloned()
        .chain(get_declarations_from_library(&Library::standard_library()))
        .chain(get_declarations_from_library(&compiler.library))
        .chain(engine_provided_function_declarations())
    {
        if state.declarations.get(&declaration.name).is_none() {
            state.declarations.add(Decl::from(&declaration));
        }
    }
    let known_declaration_names: HashSet<String> = state
        .declarations
        .iter()
        .map(|declaration| declaration.name.clone())
        .collect();

    // First pass: parse all files and generate their syntax trees
    let files: Vec<SourceFile<'input>> = compiler
        .files
        .iter()
        .zip(chars)
        .enumerate()
        .map(|(index, (file, chars))| SourceFile {
            index,
            name: file.file_name.clone(),
            source: &file.source,
            parsed: parse_syntax_tree(file, chars, &mut diagnostics),
        })
        .collect();
    let all_nodes: Vec<(&SourceFile<'input>, Rc<NodeContextAll<'input>>)> = files
        .iter()
        .flat_map(|file| file.parsed.tree.node_all().into_iter().map(move |node| (file, node)))
        .collect();

    // Give every node the title from its title header. Members of node groups get a unique title later.
    for (file, node) in &all_nodes {
        let title = node
            .title_header_all()
            .first()
            .and_then(|header| header.title.as_ref().map(|t| t.get_text().trim().to_owned()));
        state.annotations.nodes.insert(
            file.key(node.as_ref()),
            NodeAnnotation {
                title,
                node_group: None,
            },
        );
    }

    let mut string_table = StringTableManager::default();
    for file in &files {
        // Find the lines that precede options, which is noted in their metadata.
        // We do this BEFORE we build our string table, otherwise the tags will get missed.
        let mut last_line_tagger = LastLineBeforeOptionsVisitor {
            file_index: file.index,
            ..Default::default()
        };
        last_line_tagger.visit(file.parsed.tree.as_ref());
        state
            .annotations
            .last_lines_before_options
            .extend(last_line_tagger.last_lines);

        let mut visitor =
            StringTableGeneratorVisitor::new(&mut string_table, &mut state.annotations, file);
        visitor.visit(file.parsed.tree.as_ref());
        diagnostics.extend(visitor.diagnostics);
    }
    add_diagnostics_for_shadow_lines(&mut string_table, &mut diagnostics);

    // Ensure that all node names in this compilation are unique, except for node groups.
    add_errors_for_invalid_node_names(&all_nodes, &state.annotations, &mut diagnostics);

    // Nodes with 'when' headers are members of the node group that is named like their title.
    for (file, node) in &all_nodes {
        let key = file.key(node.as_ref());
        let Some(title) = state.annotations.nodes[&key].title.clone() else {
            continue;
        };
        if node.when_header_all().is_empty() {
            continue;
        }
        let subtitle = get_header_value(node, "subtitle").filter(|subtitle| !subtitle.is_empty());
        let unique_title = match subtitle {
            // The node's unique title is derived from its original title and its subtitle.
            Some(subtitle) => format!("{title}.{subtitle}"),
            // Otherwise, from its original title, the name of the file it's in, and its position in the file.
            None => {
                let line = node.start().get_line();
                let checksum = crc32_checksum_string(&format!("{}{title}{line}", file.name));
                format!("{title}.{checksum}")
            }
        };
        state.annotations.nodes.insert(
            key,
            NodeAnnotation {
                title: Some(unique_title),
                node_group: Some(title),
            },
        );
    }

    // Collect what the nodes refer to
    let mut metadata = NodeMetadataListener::default();
    for file in &files {
        metadata.file = Some(file.clone());
        metadata = *YarnSpinnerParserTreeWalker::walk(Box::new(metadata), file.parsed.tree.as_ref())
            .expect("Internal error: tree walk failed");
    }
    for (destination, position) in &metadata.jumps {
        if !metadata.node_titles.contains(destination) {
            diagnostics.push(
                position
                    .clone()
                    .described(descriptors::UNDEFINED_NODE.create(&[destination])),
            );
        }
    }

    if compiler.compilation_type == CompilationType::StringsOnly {
        // Stop at this point
        let compilation = Compilation {
            contains_implicit_string_tags: string_table.contains_implicit_string_tags(),
            string_table: string_table.into_string_table(),
            ..Default::default()
        };
        return (Some(compilation), diagnostics);
    }

    // Find the enums before type checking, so that it doesn't matter where they are declared.
    for file in &files {
        let listener = UserDefinedTypesListener {
            file: file.clone(),
            known_types: std::mem::take(&mut state.known_types),
            diagnostics: std::mem::take(&mut diagnostics),
        };
        let listener =
            YarnSpinnerParserTreeWalker::walk(Box::new(listener), file.parsed.tree.as_ref())
                .expect("Internal error: tree walk failed");
        state.known_types = listener.known_types;
        diagnostics = listener.diagnostics;
    }

    // Type check every file. This finds the declarations and as many types as possible.
    let mut file_tags = HashMap::new();
    for file in &files {
        let listener = TypeCheckerListener::new(file.clone(), std::mem::take(&mut state));
        let listener =
            YarnSpinnerParserTreeWalker::walk(Box::new(listener), file.parsed.tree.as_ref())
                .expect("Internal error: tree walk failed");
        file_tags.insert(file.name.clone(), listener.file_tags);
        state = listener.state;
        state.failing_constraints = apply_solution(&state.solution, &state.failing_constraints);
    }
    diagnostics.append(&mut state.diagnostics);

    // Warn about variables that are used without being declared
    for declaration in state.declarations.iter().filter(|declaration| {
        declaration.is_implicit
            && declaration.is_variable()
            && !declaration.name.starts_with("$Yarn.Internal")
    }) {
        diagnostics.push(
            declaration
                .diagnostic()
                .described(descriptors::UNDEFINED_VARIABLE.create(&[&declaration.name])),
        );
    }

    // Constraints that failed when they were encountered may be solvable now that everything was seen.
    // Solving one may make others solvable, so try until nothing changes anymore.
    loop {
        let mut any_succeeded = false;
        for constraint in state.failing_constraints.clone() {
            any_succeeded |= try_solve(
                &[constraint],
                &state.known_types,
                &mut diagnostics,
                &mut state.solution,
            );
        }
        state.failing_constraints = apply_solution(&state.solution, &state.failing_constraints);
        if !any_succeeded || state.failing_constraints.is_empty() {
            break;
        }
    }
    // Whatever is left can't be solved
    for constraint in &state.failing_constraints {
        diagnostics.push(constraint.failure_diagnostic(&state.solution));
    }

    // Determine which nodes need to track how often they were visited
    let mut tracking_nodes = HashSet::new();
    let mut ignoring_nodes = HashSet::new();
    for file in &files {
        let mut visitor = NodeTrackingVisitor::new(&state.annotations, file.index);
        visitor.visit(file.parsed.tree.as_ref());
        tracking_nodes.extend(visitor.tracking_nodes);
        ignoring_nodes.extend(visitor.ignoring_nodes);
    }
    tracking_nodes.retain(|node| !ignoring_nodes.contains(node));
    let mut sorted_tracking_nodes: Vec<_> = tracking_nodes.iter().collect();
    sorted_tracking_nodes.sort();
    for node in sorted_tracking_nodes {
        state.declarations.add(Decl {
            name: Library::generate_unique_visited_variable_for_node(node),
            ty: Ty::Number,
            default_value: Some(0.0.into()),
            description: Some(format!(
                "The generated variable for tracking visits of node {node}"
            )),
            source_file_name: DeclarationSource::External,
            source_node_name: None,
            is_implicit: false,
            is_inline_expansion: false,
            range: None,
            initial_value: None,
            declaration_site: None,
        });
    }

    // Apply the solution to all declarations
    for declaration in state.declarations.iter_mut() {
        declaration.ty = declaration.ty.substitute(&state.solution);
        if declaration.is_implicit && !declaration.ty.is_var() && declaration.default_value.is_none()
        {
            // Implicit declarations start out with the default value of their type
            declaration.default_value = declaration.ty.default_value();
        }
        if declaration.ty.is_var() {
            // The type of this variable could not be determined. The expressions that use it report the error.
            declaration.ty = Ty::Error;
        }
    }

    // Apply the solution to the expressions, and report the ones whose type is still unknown
    for file in &files {
        let tree: Rc<ActualParserContext<'input>> = file.parsed.tree.clone();
        resolve_expression_types(file, &tree, &mut state, &mut diagnostics);
    }

    resolve_initial_values(&mut state.declarations, &mut diagnostics);

    // Smart variables are computed, so they can't be assigned
    for (variable_name, position) in &metadata.set_statements {
        if let Some(declaration) = state
            .declarations
            .get(variable_name)
            .filter(|declaration| declaration.is_inline_expansion)
        {
            let expression = declaration
                .initial_value
                .as_ref()
                .map(|(file, expression)| {
                    expression.get_text_with_whitespace(files[*file].parsed.tokens())
                })
                .unwrap_or_else(|| "(unknown)".to_owned());
            diagnostics.push(
                position.clone().described(
                    descriptors::SMART_VARIABLE_READ_ONLY.create(&[variable_name, &expression]),
                ),
            );
        }
    }

    if compiler.language_version < VERSION_WITH_YARN_SPINNER_3_FEATURES {
        diagnostics = add_diagnostics_for_unavailable_features(&files, &state, diagnostics);
    }

    // Empty nodes are not compiled
    let mut empty_nodes = HashSet::new();
    for (file, node) in &all_nodes {
        if node.body().is_some_and(|body| body.statement_all().is_empty()) {
            let title = state.annotations.node(file.index, node.as_ref()).title;
            diagnostics.push(file.diagnostic_at(node.as_ref()).described(
                descriptors::EMPTY_NODE.create(&[title.as_deref().unwrap_or("(missing title)")]),
            ));
            empty_nodes.extend(title);
        }
    }

    for declaration in state.declarations.iter().filter(|declaration| {
        declaration.is_variable()
            && !declaration.name.starts_with("$Yarn.Internal")
            && !known_declaration_names.contains(&declaration.name)
            && !metadata.variable_references.contains(&declaration.name)
    }) {
        diagnostics.push(
            declaration
                .diagnostic()
                .described(descriptors::UNUSED_VARIABLE.create(&[&declaration.name])),
        );
    }

    // The declarations that this compilation found, as opposed to the ones it was given
    let declarations: Vec<Declaration> = state
        .declarations
        .iter()
        .filter(|declaration| !known_declaration_names.contains(&declaration.name))
        .map(Declaration::from)
        .collect();
    let user_defined_types: Vec<Type> = state
        .known_types
        .iter()
        .filter(|ty| !BUILTIN_TYPES.contains(ty))
        .map(Ty::to_type)
        .collect();

    if compiler.compilation_type == CompilationType::DeclarationsOnly {
        // Stop at this point
        let compilation = Compilation {
            declarations,
            file_tags,
            user_defined_types,
            ..Default::default()
        };
        return (Some(compilation), diagnostics);
    }

    if diagnostics.has_errors() {
        // We have errors, so we can't safely generate code.
        return (None, diagnostics);
    }

    // No errors! Go ahead and generate the code for all parsed files.
    let variable_declarations: HashMap<String, Decl<'input>> = state
        .declarations
        .iter()
        .filter(|declaration| declaration.is_variable())
        .map(|declaration| (declaration.name.clone(), declaration.clone()))
        .collect();
    let context = CodeGenerationContext {
        annotations: &state.annotations,
        variable_declarations: &variable_declarations,
    };
    let mut compiled_nodes: Vec<NodeEmitter> = Vec::new();

    for (file, node) in &all_nodes {
        let annotation = state.annotations.node(file.index, node.as_ref());
        // Nodes without a title were reported earlier
        let Some(title) = annotation.title else {
            continue;
        };
        if empty_nodes.contains(&title) {
            continue;
        }
        let mut emitter = NodeEmitter::new(&title, &file.name);
        emitter.add_header("title", &title);
        if let Some(node_group) = annotation.node_group {
            // The node is part of a group; this lets the runtime find the members of the group
            emitter.add_header(Node::NODE_GROUP_HEADER, node_group);
        }
        for header in node.header_all() {
            let Some(key) = header.header_key.as_ref() else {
                continue;
            };
            // Use the header value if provided, else fall back to the empty string. This means that
            // a header like "foo: \n" will be stored as 'foo', '', consistent with how it was typed.
            let value = header.header_value.as_ref().map(|value| value.get_text());
            emitter.add_header(key.get_text(), value.unwrap_or_default());
        }
        if let Some(body) = node.body() {
            let mut visitor = CodeGenerationVisitor::new(&context, file.index, &mut emitter);
            for statement in body.statement_all() {
                visitor.visit(statement.as_ref());
            }
            // We have exited the body; emit a 'return' instruction here.
            emitter.emit(position_of(&*body.stop()), ReturnInstruction {});
        }
        if tracking_nodes.contains(&title) {
            // This node needs to be tracked. The runtime counts its visits in this variable.
            emitter.add_header(
                Node::TRACKING_VARIABLE_NAME_HEADER,
                Library::generate_unique_visited_variable_for_node(&title),
            );
        }
        compiled_nodes.push(emitter);
    }

    // Each smart variable gets a node that computes its value, for when the game asks for it
    for declaration in state
        .declarations
        .iter()
        .filter(|declaration| declaration.is_inline_expansion)
    {
        let (file, expression) = declaration.initial_value.clone().unwrap();
        compiled_nodes.push(compile_smart_variable(
            &context,
            file,
            &files[file].name,
            &declaration.name,
            Some(expression.as_ref()),
        ));
    }

    // Each node group gets a 'hub' node that selects the member to run
    let mut node_groups: Vec<(String, Vec<&(&SourceFile<'input>, Rc<NodeContextAll<'input>>)>)> =
        Vec::new();
    for entry in &all_nodes {
        let Some(node_group) = state.annotations.node(entry.0.index, entry.1.as_ref()).node_group
        else {
            continue;
        };
        match node_groups.iter_mut().find(|(name, _)| *name == node_group) {
            Some((_, members)) => members.push(entry),
            None => node_groups.push((node_group, vec![entry])),
        }
    }
    for (node_group, members) in node_groups {
        compile_node_group(&context, &node_group, &members, &tracking_nodes, &mut compiled_nodes);
    }

    // Store the initial values of all variables in the program
    let mut initial_values = HashMap::new();
    for declaration in state.declarations.iter() {
        // Functions and smart variables have no stored value
        if !declaration.is_variable() || declaration.is_inline_expansion || declaration.ty == Ty::Error
        {
            continue;
        }
        let Some(default_value) = declaration.default_value.clone() else {
            let message = format!(
                "Declaration for {} has no default value, which is a syntax error",
                declaration.name
            );
            diagnostics.push(
                declaration
                    .diagnostic()
                    .described(descriptors::INTERNAL_ERROR.create(&[&message])),
            );
            continue;
        };
        let value = match default_value {
            YarnValue::Number(value) => Operand::from(value),
            YarnValue::String(value) => Operand::from(value),
            YarnValue::Boolean(value) => Operand::from(value),
        };
        initial_values.insert(declaration.name.clone(), value);
    }

    let mut program = Program {
        language_version: compiler.language_version as i32,
        ..Default::default()
    };
    program.initial_values.extend(initial_values);
    let mut debug_info = HashMap::new();
    for emitter in compiled_nodes {
        debug_info.insert(emitter.node.name.clone(), emitter.debug_info);
        program.nodes.insert(emitter.node.name.clone(), emitter.node);
    }

    let compilation = Compilation {
        program: Some(program),
        contains_implicit_string_tags: string_table.contains_implicit_string_tags(),
        string_table: string_table.into_string_table(),
        declarations,
        file_tags,
        debug_info,
        user_defined_types,
        warnings: Vec::new(),
    };
    (Some(compilation), diagnostics)
}

/// The functions that every [`yarnspinner_runtime::prelude::Dialogue`] provides in addition to the standard library.
fn engine_provided_function_declarations() -> Vec<Declaration> {
    [
        ("visited", Type::Boolean),
        ("visited_count", Type::Number),
        ("has_any_content", Type::Boolean),
    ]
    .into_iter()
    .map(|(name, return_type)| {
        let function_type = FunctionType {
            parameters: vec![Some(Type::String)],
            return_type: Box::new(Some(return_type)),
            variadic_parameter_type: Box::new(None),
        };
        Declaration::new(name, function_type).with_source_file_name(DeclarationSource::External)
    })
    .collect()
}

/// The trimmed value of the first header with this key (ignoring case) that has a value.
fn get_header_value<'input>(node: &NodeContextAll<'input>, key: &str) -> Option<String> {
    get_header(node, key).map(|header| header.header_value.as_ref().unwrap().get_text().trim().to_owned())
}

fn get_header<'input>(
    node: &NodeContextAll<'input>,
    key: &str,
) -> Option<Rc<HeaderContextAll<'input>>> {
    node.header_all().into_iter().find(|header| {
        header.header_value.is_some()
            && header
                .header_key
                .as_ref()
                .is_some_and(|header_key| header_key.get_text().eq_ignore_ascii_case(key))
    })
}

/// Checks the lines that are shadows of other lines (`#shadow:<line id>`) and gives them the text of their source.
fn add_diagnostics_for_shadow_lines(
    string_table: &mut StringTableManager,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let shadow_lines: Vec<(LineId, LineId)> = string_table
        .iter()
        .filter_map(|(id, entry)| Some((id.clone(), entry.info.shadow_line_id.clone()?)))
        .collect();
    for (line_id, source_line_id) in shadow_lines {
        let shadow = string_table.get(&line_id).unwrap().clone();
        let Some(source) = string_table.get(&source_line_id).cloned() else {
            // No source line found for this shadow line, so we can't keep processing this line.
            let id = &source_line_id.0;
            diagnostics.push(shadow.position.described(
                descriptors::UNKNOWN_LINE_ID_FOR_SHADOW_LINE.create(&[id]),
            ));
            continue;
        };
        if source.has_expressions {
            // Lines must not have inline expressions
            diagnostics.push(
                shadow
                    .position
                    .clone()
                    .described(descriptors::SHADOW_LINES_CANT_HAVE_EXPRESSIONS.create(&[])),
            );
        }
        if source.info.text != shadow.info.text {
            // Lines must be identical
            diagnostics.push(shadow.position.described(
                descriptors::SHADOW_LINES_MUST_HAVE_SAME_TEXT_AS_SOURCE.create(&[]),
            ));
        }
        // ## Implementation note
        // The original removes the text of the shadow line. We keep a copy of the source text, see `StringInfo::shadow_line_id`.
        string_table.get_mut(&line_id).unwrap().info.text = source.info.text;
    }
}

fn add_errors_for_invalid_node_names<'input>(
    all_nodes: &[(&SourceFile<'input>, Rc<NodeContextAll<'input>>)],
    annotations: &Annotations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Group the nodes by title, keeping the order in which the titles first appear
    let mut nodes_by_title: Vec<(String, Vec<&(&SourceFile<'input>, Rc<NodeContextAll<'input>>)>)> =
        Vec::new();
    for entry in all_nodes {
        let Some(title) = annotations.node(entry.0.index, entry.1.as_ref()).title else {
            continue;
        };
        match nodes_by_title.iter_mut().find(|(name, _)| *name == title) {
            Some((_, group)) => group.push(entry),
            None => nodes_by_title.push((title, vec![entry])),
        }
    }

    // Find groups of nodes with the same name and generate diagnostics for each
    for (title, group) in nodes_by_title.iter().filter(|(_, group)| group.len() > 1) {
        // If any of them have 'when' headers, then all of them must have one: they form a node group.
        let has_when_header = |node: &NodeContextAll<'input>| !node.when_header_all().is_empty();
        if group.iter().any(|(_, node)| has_when_header(node)) {
            for (file, node) in group.iter().filter(|(_, node)| !has_when_header(node)) {
                let position = match node.title_header_all().first() {
                    Some(title_header) => file.diagnostic_at(title_header.as_ref()),
                    None => file.diagnostic_at(node.as_ref()),
                };
                diagnostics.push(
                    position.described(descriptors::NODE_GROUP_MISSING_WHEN.create(&[title])),
                );
            }

            // Subtitles make the members of a node group distinguishable, so they must be unique.
            let subtitles: Vec<_> = group
                .iter()
                .filter_map(|(_, node)| get_header_value(node, "subtitle"))
                .collect();
            let mut reported = HashSet::new();
            for subtitle in &subtitles {
                if subtitles.iter().filter(|other| *other == subtitle).count() > 1
                    && reported.insert(subtitle)
                {
                    for (file, node) in group {
                        let position = match get_header(node, "subtitle") {
                            Some(header) => file.diagnostic_at(header.as_ref()),
                            None => file.diagnostic_at(node.as_ref()),
                        };
                        diagnostics.push(position.described(
                            descriptors::DUPLICATE_SUBTITLE.create(&[title, subtitle]),
                        ));
                    }
                }
            }
            continue;
        }

        // More than one node has this name! Report an error on both.
        for (file, node) in group {
            if let Some(title_header) = node.title_header_all().first() {
                diagnostics.push(
                    file.diagnostic_at(title_header.as_ref())
                        .described(descriptors::DUPLICATE_NODE_TITLE.create(&[title])),
                );
            }
        }
    }

    let invalid_title_characters = Regex::new(r"(^[0-9]|[\.\[<>\]{}\|:\s#\$])").unwrap();
    for (file, node) in all_nodes {
        let title_headers = node.title_header_all();
        if annotations.node(file.index, node.as_ref()).title.is_none() {
            let position = match node.body() {
                Some(body) if body.get_child_count() > 0 => file.diagnostic_at(body.as_ref()),
                _ => file.diagnostic_at(node.as_ref()),
            };
            diagnostics.push(position.described(descriptors::NODE_MISSING_TITLE.create(&[])));
        }
        if let Some(second_title) = title_headers.get(1) {
            diagnostics.push(
                file.diagnostic_at(second_title.as_ref())
                    .described(descriptors::NODE_HAS_MORE_THAN_ONE_TITLE.create(&[])),
            );
        }
        // Subtitles become part of the name of the node, so they follow the same rules as titles.
        if let Some(header) = get_header(node, "subtitle") {
            let token = header.header_value.as_ref().unwrap();
            let subtitle = token.get_text().trim().to_owned();
            if let Some(invalid) = invalid_title_characters.find(&subtitle) {
                let line = token.get_line_as_usize().saturating_sub(1);
                let character = token.get_column_as_usize() + subtitle[..invalid.start()].chars().count();
                let range = Position { line, character }..Position {
                    line,
                    character: character + 1,
                };
                diagnostics.push(file.diagnostic_at_range(range).described(
                    descriptors::INVALID_NODE_NAME.create(&["subtitle", invalid.as_str()]),
                ));
            }
        }
    }
}

/// Collects what nodes refer to: other nodes and variables.
/// Corresponds to the parts of `NodeMetadataVisitor` that the compiler itself needs.
#[derive(Default)]
struct NodeMetadataListener<'input> {
    file: Option<SourceFile<'input>>,
    /// The titles of all nodes, as they are written in the source.
    node_titles: HashSet<String>,
    /// The destinations of all `<<jump>>` and `<<detour>>` statements that name a node.
    jumps: Vec<(String, Diagnostic)>,
    variable_references: HashSet<String>,
    /// The variables that `<<set>>` statements assign to.
    set_statements: Vec<(String, Diagnostic)>,
}

impl<'input> NodeMetadataListener<'input> {
    fn add_jump(&mut self, destination: Option<(String, Diagnostic)>) {
        self.jumps.extend(destination);
    }

    fn file(&self) -> &SourceFile<'input> {
        self.file.as_ref().unwrap()
    }
}

impl<'input> ParseTreeListener<'input, YarnSpinnerParserContextType> for NodeMetadataListener<'input> {}

#[allow(non_snake_case)]
impl<'input> YarnSpinnerParserListener<'input> for NodeMetadataListener<'input> {
    fn exit_title_header(&mut self, ctx: &Title_headerContext<'input>) {
        if let Some(title) = ctx.title.as_ref() {
            self.node_titles.insert(title.get_text().trim().to_owned());
        }
    }

    fn exit_jumpToNodeName(&mut self, ctx: &JumpToNodeNameContext<'input>) {
        let destination = ctx.destination.as_ref().map(|destination| {
            (
                destination.get_text().to_owned(),
                self.file().diagnostic_at_token(destination.as_ref()),
            )
        });
        self.add_jump(destination);
    }

    fn exit_detourToNodeName(&mut self, ctx: &DetourToNodeNameContext<'input>) {
        let destination = ctx.destination.as_ref().map(|destination| {
            (
                destination.get_text().to_owned(),
                self.file().diagnostic_at_token(destination.as_ref()),
            )
        });
        self.add_jump(destination);
    }

    fn exit_valueVar(&mut self, ctx: &ValueVarContext<'input>) {
        self.variable_references.insert(ctx.get_text());
    }

    fn exit_set_statement(&mut self, ctx: &Set_statementContext<'input>) {
        if let Some(variable) = ctx.variable() {
            let name = variable.get_text();
            let position = self.file.as_ref().unwrap().diagnostic_at(variable.as_ref());
            self.variable_references.insert(name.clone());
            self.set_statements.push((name, position));
        }
    }
}

/// Applies the solution of the type checker to all expressions below `tree`.
/// Reports the outermost expressions whose type could not be determined.
fn resolve_expression_types<'input>(
    file: &SourceFile<'input>,
    tree: &Rc<ActualParserContext<'input>>,
    state: &mut TypeCheckState<'input>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if tree.get_child_count() == 0 {
        // Terminal nodes and empty rules have no type
        return;
    }
    let key = (file.index, tree.get_hashable_interval());
    if let Some(ty) = state.annotations.types.get(&key) {
        let ty = ty.substitute(&state.solution);
        let is_undetermined = ty.is_var();
        state.annotations.types.insert(key, ty);
        if is_undetermined {
            let text = tree.get_text_with_whitespace(file.parsed.tokens());
            diagnostics.push(
                file.diagnostic_at_range(token_range(&*tree.start(), &*tree.stop()))
                    .described(descriptors::EXPRESSION_TYPE_UNDETERMINED.create(&[&text])),
            );
            // Don't report every child of this expression as well
            return;
        }
    }
    for child in tree.get_children() {
        resolve_expression_types(file, &child, state, diagnostics);
    }
}

/// Determines the initial values of declared variables, or that they are smart variables.
/// Corresponds to `TypeCheckerListener.ResolveInitialValues`.
fn resolve_initial_values(declarations: &mut Declarations, diagnostics: &mut Vec<Diagnostic>) {
    for declaration in declarations.iter_mut() {
        let Some((_, expression)) = declaration.initial_value.clone() else {
            continue;
        };
        if let Some(value) = literal_expression_value(&expression) {
            // The initial value is a constant
            declaration.default_value = Some(value);
            continue;
        }
        let member_reference = match expression.as_ref() {
            ExpressionContextAll::ExpValueContext(value) => match value.value().as_deref() {
                Some(ValueContextAll::ValueTypeMemberReferenceContext(reference)) => {
                    reference.typeMemberReference()
                }
                _ => None,
            },
            _ => None,
        };
        let Some(member_reference) = member_reference else {
            // Not a constant, so this is a smart variable: the expression is evaluated when the variable is read
            declaration.is_inline_expansion = true;
            continue;
        };
        // The initial value is an enum case, which is a constant as well
        let member_name = member_reference
            .memberName
            .as_ref()
            .map(|name| name.get_text().to_owned())
            .unwrap_or_default();
        let message = match &declaration.ty {
            Ty::Enum(enum_type) => match enum_type.case(&member_name) {
                Some(case) => {
                    declaration.default_value = Some(case.value.clone());
                    continue;
                }
                None => format!("{} doesn't have a member named {member_name}", enum_type.name),
            },
            // The value was converted to the raw type of the enum by an explicit type, e.g. `as string`
            _ => format!("Can't determine the type of {}", expression.get_text()),
        };
        diagnostics.push(
            declaration
                .diagnostic()
                .described(descriptors::INVALID_MEMBER_ACCESS.create(&[&message])),
        );
    }

    // Smart variables must not depend on themselves
    let smart_variables: HashMap<String, Rc<ExpressionContextAll>> = declarations
        .iter()
        .filter(|declaration| declaration.is_inline_expansion)
        .map(|declaration| {
            let (_, expression) = declaration.initial_value.clone().unwrap();
            (declaration.name.clone(), expression)
        })
        .collect();
    for declaration in declarations.iter().filter(|declaration| declaration.is_inline_expansion) {
        let mut visited = HashSet::new();
        let mut stack = vec![declaration.name.clone()];
        let mut loops_through = None;
        while let Some(name) = stack.pop() {
            let Some(expression) = smart_variables.get(&name) else {
                continue;
            };
            for dependency in referenced_variables(expression.as_ref()) {
                if dependency == declaration.name {
                    loops_through = Some(name.clone());
                } else if visited.insert(dependency.clone()) {
                    stack.push(dependency);
                }
            }
        }
        if let Some(loops_through) = loops_through {
            diagnostics.push(declaration.diagnostic().described(
                descriptors::SMART_VARIABLE_LOOP.create(&[&loops_through, &declaration.name]),
            ));
        }
    }
}

/// The names of all variables that an expression reads.
fn referenced_variables<'input>(tree: &ActualParserContext<'input>) -> Vec<String> {
    if tree.get_rule_index() == RULE_variable && tree.get_child_count() > 0 {
        return vec![tree.get_text()];
    }
    tree.get_children()
        .flat_map(|child| referenced_variables(child.as_ref()))
        .collect()
}

/// Reports the use of features that the language version of the project doesn't have.
/// Corresponds to `PreviewFeatureVisitor`.
fn add_diagnostics_for_unavailable_features<'input>(
    files: &[SourceFile<'input>],
    state: &TypeCheckState<'input>,
    mut diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    struct FeatureListener<'input> {
        file: SourceFile<'input>,
        language_version: u32,
        diagnostics: Vec<Diagnostic>,
    }
    impl<'input> FeatureListener<'input> {
        fn add_error(&mut self, ctx: &impl ParserRuleContext<'input>, feature: &str) {
            self.diagnostics.push(self.file.diagnostic_at(ctx).described(
                descriptors::LANGUAGE_VERSION_TOO_LOW.create(&[&unavailable_feature_message(
                    feature,
                    self.language_version,
                )]),
            ));
        }
    }
    impl<'input> ParseTreeListener<'input, YarnSpinnerParserContextType> for FeatureListener<'input> {}
    #[allow(non_snake_case)]
    impl<'input> YarnSpinnerParserListener<'input> for FeatureListener<'input> {
        fn enter_enum_statement(&mut self, ctx: &Enum_statementContext<'input>) {
            self.add_error(ctx, "enums");
        }
        fn enter_line_group_statement(&mut self, ctx: &Line_group_statementContext<'input>) {
            self.add_error(ctx, "line groups");
        }
        fn enter_lineOnceCondition(&mut self, ctx: &LineOnceConditionContext<'input>) {
            self.add_error(ctx, "'once' conditions");
        }
        fn enter_once_statement(&mut self, ctx: &Once_statementContext<'input>) {
            self.add_error(ctx, "'once' statements");
        }
    }

    for file in files {
        let listener = FeatureListener {
            file: file.clone(),
            language_version: state.language_version,
            diagnostics,
        };
        diagnostics =
            YarnSpinnerParserTreeWalker::walk(Box::new(listener), file.parsed.tree.as_ref())
                .expect("Internal error: tree walk failed")
                .diagnostics;
    }
    for declaration in state.declarations.iter().filter(|d| d.is_inline_expansion) {
        diagnostics.push(declaration.diagnostic().described(
            descriptors::LANGUAGE_VERSION_TOO_LOW.create(&[&unavailable_feature_message(
                "smart variables",
                state.language_version,
            )]),
        ));
    }
    diagnostics
}

fn unavailable_feature_message(feature: &str, language_version: u32) -> String {
    format!(
        "Language feature \"{feature}\" is not available at language version {language_version}; it requires version {VERSION_WITH_YARN_SPINNER_3_FEATURES} or later"
    )
}

/// Compiles the node that computes the value of a smart variable.
fn compile_smart_variable<'input>(
    context: &CodeGenerationContext<'_, 'input>,
    file: usize,
    file_name: &str,
    node_name: &str,
    expression: Option<&ExpressionContextAll<'input>>,
) -> NodeEmitter {
    let mut emitter = NodeEmitter::new(node_name, file_name);
    emitter.add_header("tags", Program::SMART_VARIABLE_NODE_TAG);
    if let Some(expression) = expression {
        CodeGenerationVisitor::new(context, file, &mut emitter).visit(expression);
    }
    emitter
}

/// Compiles the conditions of the members of a node group into smart variables,
/// and the 'hub' node that selects and runs one of the members.
fn compile_node_group<'input>(
    context: &CodeGenerationContext<'_, 'input>,
    node_group_name: &str,
    members: &[&(&SourceFile<'input>, Rc<NodeContextAll<'input>>)],
    tracking_nodes: &HashSet<String>,
    compiled_nodes: &mut Vec<NodeEmitter>,
) {
    let title_of = |file: &SourceFile<'input>, node: &NodeContextAll<'input>| {
        context.annotations.node(file.index, node).title
    };
    let once_position = |when_header: &When_headerContextAll<'input>| {
        when_header
            .header_expression
            .as_ref()
            .and_then(|expression| expression.once.as_ref())
            .and_then(|once| position_of(once.as_ref()))
    };
    let is_once = |when_header: &When_headerContextAll<'input>| {
        when_header
            .header_expression
            .as_ref()
            .is_some_and(|expression| expression.once.is_some())
    };

    for (file, node) in members {
        let Some(title) = title_of(file, node) else {
            continue;
        };
        if !compiled_nodes.iter().any(|compiled| compiled.node.name == title) {
            // The node wasn't compiled (it's empty), so it can't be run
            continue;
        }
        let once_variable = Library::generate_unique_content_viewed_variable_name(&title);
        let mut condition_variable_names = Vec::new();
        let mut total_complexity = 0;

        // Every 'when' header becomes a smart variable that tells whether its condition passes
        for (index, when_header) in node.when_header_all().into_iter().enumerate() {
            let Some(header_expression) = when_header.header_expression.clone() else {
                continue;
            };
            let expression = header_expression.expression();
            let is_always = header_expression.always.is_some();
            let is_once = header_expression.once.is_some();

            let smart_variable_name =
                format!("$Yarn.Internal.{node_group_name}.{title}.Condition.{index}");
            condition_variable_names.push(smart_variable_name.clone());
            let mut emitter = compile_smart_variable(
                context,
                file.index,
                &file.name,
                &smart_variable_name,
                expression.as_deref(),
            );

            if !is_always {
                total_complexity += usize::from(is_once)
                    + expression
                        .as_ref()
                        .map_or(0, |expression| expression_complexity(expression));
            }
            if is_always {
                // 'when: always' has no expression; it always passes
                let position = header_expression
                    .always
                    .as_ref()
                    .and_then(|always| position_of(always.as_ref()));
                emitter.emit(position, PushBoolInstruction { value: true });
            }
            if is_once {
                // 'when: once' passes as long as the node did not run yet
                let position = once_position(&when_header);
                emit_once_variable_test(&mut emitter, position, &once_variable);
                if expression.is_some() {
                    // 'when: once if <expression>' needs both to pass
                    emit_and(&mut emitter, position);
                }
            }
            compiled_nodes.push(emitter);
        }

        // Tell the runtime which variables to evaluate for this node, and how specific its conditions are
        let compiled_node = compiled_nodes
            .iter_mut()
            .find(|compiled| compiled.node.name == title)
            .unwrap();
        compiled_node.add_header(
            Node::CONTENT_SALIENCY_CONDITION_VARIABLES_HEADER,
            condition_variable_names.join(";"),
        );
        compiled_node.add_header(
            Node::CONTENT_SALIENCY_CONDITION_COMPLEXITY_SCORE_HEADER,
            total_complexity.to_string(),
        );
    }

    // Now generate the 'hub' node: the node that is run when the node group is run.
    // It asks the runtime to select the best member, and then detours into it.
    let mut hub = NodeEmitter::new(node_group_name, "<generated>");
    hub.add_header(Node::NODE_IS_HUB_NODE_HEADER, "1");
    let all_members_never_track = members.iter().all(|(_, node)| {
        get_header(node, "tracking")
            .is_some_and(|header| header.header_value.as_ref().unwrap().get_text() == "never")
    });
    if tracking_nodes.contains(node_group_name) && !all_members_never_track {
        hub.add_header(
            Node::TRACKING_VARIABLE_NAME_HEADER,
            Library::generate_unique_visited_variable_for_node(node_group_name),
        );
    }

    let runnable_members: Vec<_> = members
        .iter()
        .filter_map(|(file, node)| Some((node, title_of(file, node)?)))
        .filter(|(_, title)| compiled_nodes.iter().any(|compiled| compiled.node.name == *title))
        .collect();

    let add_candidate_instructions: Vec<usize> = runnable_members
        .iter()
        .map(|(_, title)| {
            hub.emit(
                None,
                AddSaliencyCandidateFromNodeInstruction {
                    node_name: title.clone(),
                    destination: -1,
                },
            )
        })
        .collect();

    hub.emit(None, SelectSaliencyCandidateInstruction {});
    let jump_to_no_content_available = hub.emit(None, JumpIfFalseInstruction { destination: -1 });
    hub.emit(None, PopInstruction {});
    hub.emit(None, PeekAndJumpInstruction {});

    // No content was available, so there is nothing to do
    hub.jump_to_here(jump_to_no_content_available);
    hub.emit(None, ReturnInstruction {});

    for ((node, title), add_candidate) in runnable_members.iter().zip(add_candidate_instructions) {
        hub.jump_to_here(add_candidate);
        if let Some(once_header) = node.when_header_all().iter().find(|header| is_once(header)) {
            // This node may only run once. Remember that it ran.
            let once_variable = Library::generate_unique_content_viewed_variable_name(title);
            emit_set_once_variable(&mut hub, once_position(once_header), &once_variable);
        }
        hub.emit(
            None,
            DetourToNodeInstruction {
                node_name: title.clone(),
            },
        );
        hub.emit(None, ReturnInstruction {});
    }
    compiled_nodes.push(hub);
}
