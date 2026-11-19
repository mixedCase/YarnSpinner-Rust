//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/TypeCheckerListener.cs>

use super::*;
use crate::annotations::*;
use crate::diagnostic_descriptors::{self as descriptors, DiagnosticDescriptor};
use crate::prelude::generated::yarnspinnerparser::*;
use crate::prelude::generated::yarnspinnerparserlistener::YarnSpinnerParserListener;
use crate::listeners::token_range;
use crate::prelude::*;
use antlr4rust::parser_rule_context::ParserRuleContext;
use antlr4rust::token::Token;
use antlr4rust::tree::{ParseTree, ParseTreeListener};
use std::rc::Rc;

/// The project file version from which on calling an undeclared function is an error.
const VERSION_WITH_MANDATORY_FUNCTION_DECLARATIONS: u32 = 5;

/// What the type checker knows. Shared between the files of a compilation.
#[derive(Default)]
pub(crate) struct TypeCheckState<'input> {
    pub(crate) declarations: Declarations<'input>,
    pub(crate) known_types: Vec<Ty>,
    pub(crate) solution: Substitution,
    /// The constraints that could not be solved when they were encountered.
    /// They may become solvable once more is known.
    pub(crate) failing_constraints: Vec<Constraint>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) annotations: Annotations,
    pub(crate) language_version: u32,
    pub(crate) type_variable_count: usize,
}

pub(crate) struct TypeCheckerListener<'input> {
    file: SourceFile<'input>,
    /// Owned during the walk, see the implementation notes of [`UserDefinedTypesListener`].
    pub(crate) state: TypeCheckState<'input>,
    type_equations: Vec<Constraint>,
    current_node_name: Option<String>,
    current_node: NodeAnnotation,
    pub(crate) file_tags: Vec<String>,
}

impl<'input> TypeCheckerListener<'input> {
    pub(crate) fn new(file: SourceFile<'input>, state: TypeCheckState<'input>) -> Self {
        Self {
            file,
            state,
            type_equations: Vec::new(),
            current_node_name: None,
            current_node: Default::default(),
            file_tags: Vec::new(),
        }
    }

    fn type_of(&self, ctx: &impl ParserRuleContext<'input>) -> Ty {
        self.state.annotations.type_of(self.file.index, ctx)
    }

    fn type_of_optional<T: ParserRuleContext<'input>>(&self, ctx: Option<Rc<T>>) -> Ty {
        ctx.map(|ctx| self.type_of(ctx.as_ref()))
            .unwrap_or(Ty::Error)
    }

    fn set_type(&mut self, ctx: &impl ParserRuleContext<'input>, ty: Ty) {
        self.state.annotations.types.insert(self.file.key(ctx), ty);
    }

    fn generate_type_variable(&mut self, name: Option<&str>) -> Ty {
        match name {
            Some(name) => Ty::var(format!("T({name})")),
            None => {
                let variable = Ty::var(format!("T{}", self.state.type_variable_count));
                self.state.type_variable_count += 1;
                variable
            }
        }
    }

    /// Adds a constraint that reports the diagnostic `descriptor` at `ctx` when it can't be satisfied.
    /// `arguments` produces the message arguments from what the solver knows at that point.
    fn add_constraint(
        &mut self,
        kind: ConstraintKind,
        ctx: &impl ParserRuleContext<'input>,
        descriptor: DiagnosticDescriptor,
        arguments: impl Fn(&Substitution) -> Vec<String> + 'static,
    ) {
        let source = Rc::new(self.file.diagnostic_at(ctx));
        let position = source.clone();
        self.type_equations.push(Constraint {
            kind,
            source,
            diagnostic_provider: Rc::new(move |substitution| {
                let arguments = arguments(substitution);
                let arguments: Vec<_> = arguments.iter().map(String::as_str).collect();
                position
                    .as_ref()
                    .clone()
                    .described(descriptor.create(&arguments))
            }),
        });
    }

    /// Adds a constraint that reports a type checker error with `message` when it can't be satisfied.
    fn add_type_constraint(
        &mut self,
        kind: ConstraintKind,
        ctx: &impl ParserRuleContext<'input>,
        message: impl Fn(&Substitution) -> String + 'static,
    ) {
        self.add_constraint(kind, ctx, descriptors::TYPE_CHECKER_ERROR, move |s| {
            vec![message(s)]
        });
    }

    fn add_diagnostic(&mut self, at: Diagnostic, description: Diagnostic) {
        self.state.diagnostics.push(at.described(description));
    }

    fn register_once_variable(
        &mut self,
        ctx: &impl ParserRuleContext<'input>,
        once_variable_name: String,
        description: String,
    ) {
        if self.state.declarations.get(&once_variable_name).is_some() {
            let message = format!("Redeclaration of existing 'once' variable {once_variable_name}");
            self.add_diagnostic(
                self.file.diagnostic_at(ctx),
                descriptors::INTERNAL_ERROR.create(&[&message]),
            );
            return;
        }
        self.state.declarations.add(Decl {
            name: once_variable_name,
            ty: Ty::Boolean,
            default_value: Some(false.into()),
            description: Some(description),
            source_file_name: DeclarationSource::External,
            source_node_name: None,
            is_implicit: true,
            is_inline_expansion: false,
            range: None,
            initial_value: None,
            declaration_site: None,
        });
    }

    /// Checks that the expression of a condition is a boolean.
    fn add_boolean_condition_constraint(
        &mut self,
        expression: Option<Rc<ExpressionContextAll<'input>>>,
        ctx: &impl ParserRuleContext<'input>,
        what: &'static str,
    ) {
        if let Some(expression) = expression {
            let ty = self.type_of(expression.as_ref());
            self.add_type_constraint(
                ConstraintKind::Equality(ty.clone(), Ty::Boolean),
                ctx,
                move |s| format!("{what} must be a Bool, not a {}", ty.substitute(s)),
            );
        }
    }

    fn check_binary_operands(
        &mut self,
        ctx: &impl ParserRuleContext<'input>,
        op: String,
        a: Ty,
        b: Ty,
    ) {
        self.add_type_constraint(ConstraintKind::Equality(a.clone(), b.clone()), ctx, move |s| {
            format!(
                "Operation '{op}'s values must both be the same type, not {} and {}",
                a.substitute(s),
                b.substitute(s)
            )
        });
    }
}

impl<'input> ParseTreeListener<'input, YarnSpinnerParserContextType>
    for TypeCheckerListener<'input>
{
}

#[allow(non_snake_case)]
impl<'input> YarnSpinnerParserListener<'input> for TypeCheckerListener<'input> {
    fn exit_file_hashtag(&mut self, ctx: &File_hashtagContext<'input>) {
        if let Some(text) = ctx.text.as_ref() {
            self.file_tags.push(text.get_text().to_owned());
        }
    }

    fn enter_node(&mut self, ctx: &NodeContext<'input>) {
        self.current_node = self.state.annotations.node(self.file.index, ctx);
    }

    fn exit_title_header(&mut self, ctx: &Title_headerContext<'input>) {
        self.current_node_name = ctx.title.as_ref().map(|title| title.get_text().to_owned());
    }

    fn exit_declare_statement(&mut self, ctx: &Declare_statementContext<'input>) {
        let variable_context = ctx.variable();
        let name = variable_context
            .as_ref()
            .and_then(|variable| variable.VAR_ID())
            .map(|id| id.get_text());
        let display_name = name.clone().unwrap_or_else(|| "unknown".to_owned());

        // Does this variable name already exist in our declarations?
        let existing = name
            .as_deref()
            .and_then(|name| self.state.declarations.get(name))
            .cloned();
        if let Some(existing) = existing.as_ref().filter(|existing| !existing.is_implicit) {
            // An explicit declaration for this variable already exists. Report it on both.
            let description =
                || descriptors::REDECLARATION_OF_EXISTING_VARIABLE.create(&[&display_name]);
            self.add_diagnostic(self.file.diagnostic_at(ctx), description());
            if let Some(site) = existing.declaration_site.clone() {
                self.add_diagnostic(site, description());
            }
            return;
        }

        // The type of the variable is whatever the type of its initial value is.
        let type_identifier = self.generate_type_variable(Some(&format!(
            "{} declaration",
            name.as_deref().unwrap_or_default()
        )));
        let Some(expression) = ctx.expression() else {
            return;
        };
        let expression_type = self.type_of(expression.as_ref());
        {
            let (name, type_identifier, expression_type) =
                (display_name.clone(), type_identifier.clone(), expression_type.clone());
            let expression_text = expression.get_text();
            self.add_type_constraint(
                ConstraintKind::Equality(type_identifier.clone(), expression_type.clone()),
                ctx,
                move |s| {
                    format!("The type of {name}'s initial value \"{expression_text}\" ({}) doesn't match the type of the variable {}.", expression_type.substitute(s), type_identifier.substitute(s))
                },
            );
        }

        if let Some(type_token) = ctx.type_.as_ref() {
            // The declaration has an explicit type: `<<declare $x = 1 as number>>`
            let type_name = type_token.get_text().to_owned();
            let language_type = match type_name.as_str() {
                "string" => Some(Ty::String),
                "number" => Some(Ty::Number),
                "bool" => Some(Ty::Boolean),
                _ => None,
            };
            let expression_text = expression.get_text_with_whitespace(self.file.parsed.tokens());
            let (name, type_identifier_for_message) = (display_name.clone(), type_identifier.clone());
            let declared_type_name = language_type
                .as_ref()
                .map(Ty::name)
                .unwrap_or_else(|| type_name.clone());
            let kind = match language_type {
                Some(language_type) => ConstraintKind::Equality(type_identifier.clone(), language_type),
                // Not a built-in type, so it must be the name of a user-defined one
                None => ConstraintKind::HasName(type_identifier.clone(), type_name),
            };
            self.add_constraint(
                kind,
                ctx,
                descriptors::DECLARATION_VALUE_DOESNT_MATCH_TYPE,
                move |s| {
                    vec![
                        name.clone(),
                        declared_type_name.clone(),
                        expression_text.clone(),
                        type_identifier_for_message.substitute(s).name(),
                    ]
                },
            );
        }

        let description = get_document_comments(self.file.parsed.tokens(), ctx, true);

        if let Some(existing) = existing {
            // The variable was used before it was declared. The uses must agree with the declaration.
            let (name, declared, used) =
                (display_name.clone(), type_identifier.clone(), existing.ty.clone());
            self.add_type_constraint(
                ConstraintKind::Equality(existing.ty.clone(), type_identifier.clone()),
                ctx,
                move |s| {
                    format!(
                        "{name} was declared to be a {}, but it was used elsewhere as a {}",
                        declared.substitute(s),
                        used.substitute(s)
                    )
                },
            );
            self.state.declarations.remove(&existing.name);
        }

        self.state.declarations.add(Decl {
            name: name.unwrap_or_else(|| "<unknown>".to_owned()),
            ty: type_identifier,
            description: (!description.is_empty()).then_some(description),
            default_value: None,
            source_file_name: DeclarationSource::File(self.file.name.clone()),
            source_node_name: self.current_node_name.clone(),
            range: variable_context.as_ref().map(|variable| ctx_range(variable.as_ref())),
            is_implicit: false,
            is_inline_expansion: false,
            initial_value: Some((self.file.index, expression)),
            declaration_site: Some(self.file.diagnostic_at(ctx)),
        });
    }

    fn exit_valueTrue(&mut self, ctx: &ValueTrueContext<'input>) {
        self.set_type(ctx, Ty::Boolean);
    }

    fn exit_valueFalse(&mut self, ctx: &ValueFalseContext<'input>) {
        self.set_type(ctx, Ty::Boolean);
    }

    fn exit_valueNumber(&mut self, ctx: &ValueNumberContext<'input>) {
        self.set_type(ctx, Ty::Number);
    }

    fn exit_valueString(&mut self, ctx: &ValueStringContext<'input>) {
        self.set_type(ctx, Ty::String);
    }

    fn exit_valueVar(&mut self, ctx: &ValueVarContext<'input>) {
        let ty = self.type_of_optional(ctx.variable());
        self.set_type(ctx, ty);
    }

    fn exit_valueTypeMemberReference(&mut self, ctx: &ValueTypeMemberReferenceContext<'input>) {
        let reference = ctx.typeMemberReference();
        let member_name = reference
            .as_ref()
            .and_then(|reference| reference.memberName.as_ref())
            .map(|member| member.get_text().to_owned());
        let (Some(reference), Some(member_name)) = (reference, member_name) else {
            self.set_type(ctx, Ty::Error);
            return;
        };
        let type_name = reference
            .typeName
            .as_ref()
            .map(|name| name.get_text().to_owned());

        // If exactly one known type fits the name (if given) and has this member, we know the type right away.
        let mut matching_types = self.state.known_types.iter().filter(|known| {
            type_name.as_ref().is_none_or(|name| known.name() == *name)
                && known.has_member(&member_name)
        });
        if let (Some(matching), None) = (matching_types.next(), matching_types.next()) {
            let matching = matching.clone();
            self.set_type(ctx, matching);
            return;
        }

        let ty = self.generate_type_variable(None);
        self.set_type(ctx, ty.clone());
        if let Some(type_name) = type_name {
            {
                let type_name = type_name.clone();
                self.add_type_constraint(
                    ConstraintKind::HasName(ty.clone(), type_name.clone()),
                    reference.as_ref(),
                    move |_| format!("No type called {type_name} could be found"),
                );
            }
            let member = member_name.clone();
            self.add_constraint(
                ConstraintKind::HasMember(ty, member_name),
                reference.as_ref(),
                descriptors::INVALID_MEMBER_ACCESS,
                move |_| vec![format!("{type_name} doesn't have a member named {member}")],
            );
        } else {
            let (member, resolved) = (member_name.clone(), ty.clone());
            self.add_type_constraint(
                ConstraintKind::HasMember(ty, member_name),
                reference.as_ref(),
                move |s| match resolved.substitute(s) {
                    Ty::Var(_) => {
                        format!("No type containing a member named {member} could be found")
                    }
                    resolved => format!("Type {resolved} does not have a member named {member}"),
                },
            );
        }
    }

    fn exit_variable(&mut self, ctx: &VariableContext<'input>) {
        let Some(variable_id) = ctx.VAR_ID() else {
            self.set_type(ctx, Ty::Error);
            return;
        };
        let name = variable_id.get_text();
        let ty = match self.state.declarations.get(&name) {
            Some(declaration) => declaration.ty.clone(),
            None => {
                // The variable is not declared, so declare it implicitly. Its type follows from how it's used.
                let ty = self.generate_type_variable(Some(&name));
                self.state.declarations.add(Decl {
                    name: name.clone(),
                    ty: ty.clone(),
                    description: Some(format!(
                        "Implicitly declared in {}, node {}",
                        self.file.name,
                        self.current_node_name.as_deref().unwrap_or_default()
                    )),
                    default_value: None,
                    range: Some(token_range(&*variable_id.symbol, &*variable_id.symbol)),
                    is_implicit: true,
                    is_inline_expansion: false,
                    source_file_name: DeclarationSource::File(self.file.name.clone()),
                    source_node_name: self.current_node_name.clone(),
                    initial_value: None,
                    declaration_site: None,
                });
                ty
            }
        };
        self.set_type(ctx, ty);
    }

    fn exit_expParens(&mut self, ctx: &ExpParensContext<'input>) {
        let ty = self.type_of_optional(ctx.expression());
        self.set_type(ctx, ty);
    }

    fn exit_expAddSub(&mut self, ctx: &ExpAddSubContext<'input>) {
        let ty = self.generate_type_variable(None);
        self.set_type(ctx, ty.clone());
        let a = self.type_of_optional(ctx.expression(0));
        let b = self.type_of_optional(ctx.expression(1));
        let op = ctx.op.as_ref().unwrap().get_text().to_owned();

        // The result must be a type that the operation is available for...
        let permitted_types: &[Ty] = if op == "+" {
            &[Ty::Number, Ty::String]
        } else {
            &[Ty::Number]
        };
        let cant_be_used = {
            let (op, a) = (op.clone(), a.clone());
            move |s: &Substitution| {
                format!("Operation '{op}' can't be used with a value of type {}", a.substitute(s))
            }
        };
        let kind = match permitted_types {
            [single] => ConstraintKind::Equality(ty.clone(), single.clone()),
            types => ConstraintKind::AnyEquality(
                types.iter().map(|t| (ty.clone(), t.clone())).collect(),
            ),
        };
        self.add_type_constraint(kind, ctx, cant_be_used.clone());
        // ...and the same type as both of the operands.
        self.add_type_constraint(ConstraintKind::Equality(ty, a.clone()), ctx, cant_be_used);
        self.check_binary_operands(ctx, op, a, b);
    }

    fn exit_expMultDivMod(&mut self, ctx: &ExpMultDivModContext<'input>) {
        let ty = self.generate_type_variable(None);
        self.set_type(ctx, ty.clone());
        let a = self.type_of_optional(ctx.expression(0));
        let b = self.type_of_optional(ctx.expression(1));
        let op = ctx.op.as_ref().unwrap().get_text().to_owned();

        {
            let op = op.clone();
            self.add_type_constraint(
                ConstraintKind::Equality(ty.clone(), Ty::Number),
                ctx,
                move |_| format!("Operation '{op}' can only be used with Number"),
            );
        }
        {
            let (op, a) = (op.clone(), a.clone());
            self.add_type_constraint(ConstraintKind::Equality(ty, a.clone()), ctx, move |s| {
                format!("Operation '{op}' can't be used with a value of type {}", a.substitute(s))
            });
        }
        self.check_binary_operands(ctx, op, a, b);
    }

    fn exit_expComparison(&mut self, ctx: &ExpComparisonContext<'input>) {
        self.set_type(ctx, Ty::Boolean);
        let a = self.type_of_optional(ctx.expression(0));
        let b = self.type_of_optional(ctx.expression(1));
        let op = ctx.op.as_ref().unwrap().get_text().to_owned();
        self.check_binary_operands(ctx, op, a, b);
    }

    fn exit_expEquality(&mut self, ctx: &ExpEqualityContext<'input>) {
        self.set_type(ctx, Ty::Boolean);
        let a = self.type_of_optional(ctx.expression(0));
        let b = self.type_of_optional(ctx.expression(1));
        let op = ctx.op.as_ref().unwrap().get_text().to_owned();
        self.check_binary_operands(ctx, op, a, b);
    }

    fn exit_expAndOrXor(&mut self, ctx: &ExpAndOrXorContext<'input>) {
        let (Some(expression_0), Some(expression_1)) = (ctx.expression(0), ctx.expression(1)) else {
            self.set_type(ctx, Ty::Error);
            return;
        };
        let type_0 = self.type_of(expression_0.as_ref());
        let type_1 = self.type_of(expression_1.as_ref());
        self.set_type(ctx, Ty::Boolean);
        let op = ctx.op.as_ref().unwrap().get_text().to_owned();
        {
            let (op, type_0) = (op.clone(), type_0.clone());
            self.add_type_constraint(
                ConstraintKind::Equality(type_0.clone(), Ty::Boolean),
                ctx,
                move |s| format!("{op} operands must be Bool, not {}", type_0.substitute(s)),
            );
        }
        let (a, b) = (type_0.clone(), type_1.clone());
        self.add_type_constraint(ConstraintKind::Equality(type_0, type_1), ctx, move |_| {
            format!("{op} operands must be the same type, not {a} and {b}")
        });
    }

    fn exit_expNot(&mut self, ctx: &ExpNotContext<'input>) {
        self.set_type(ctx, Ty::Boolean);
        let ty = self.type_of_optional(ctx.expression());
        let op = ctx.op.as_ref().unwrap().get_text().to_owned();
        self.add_type_constraint(ConstraintKind::Equality(ty.clone(), Ty::Boolean), ctx, move |s| {
            format!("{op} operand must be Bool, not {}", ty.substitute(s))
        });
    }

    fn exit_expNegative(&mut self, ctx: &ExpNegativeContext<'input>) {
        self.set_type(ctx, Ty::Number);
        let ty = self.type_of_optional(ctx.expression());
        let op = ctx.op.as_ref().unwrap().get_text().to_owned();
        self.add_type_constraint(ConstraintKind::Equality(ty.clone(), Ty::Number), ctx, move |s| {
            format!("{op} operand must be Number, not {}", ty.substitute(s))
        });
    }

    fn exit_expValue(&mut self, ctx: &ExpValueContext<'input>) {
        let ty = self.type_of_optional(ctx.value());
        self.set_type(ctx, ty);
    }

    fn exit_if_clause(&mut self, ctx: &If_clauseContext<'input>) {
        if let Some(expression) = ctx.expression() {
            self.add_boolean_condition_constraint(
                Some(expression.clone()),
                expression.as_ref(),
                "if statement's expression",
            );
        }
    }

    fn exit_else_if_clause(&mut self, ctx: &Else_if_clauseContext<'input>) {
        if let Some(expression) = ctx.expression() {
            self.add_boolean_condition_constraint(
                Some(expression.clone()),
                expression.as_ref(),
                "else if statement's expression",
            );
        }
    }

    fn exit_set_statement(&mut self, ctx: &Set_statementContext<'input>) {
        let variable_type = self.type_of_optional(ctx.variable());
        let expression_type = self.type_of_optional(ctx.expression());
        let variable_name = ctx
            .variable()
            .map(|variable| variable.get_text())
            .unwrap_or_else(|| "<unknown>".to_owned());
        let (variable, expression) = (variable_type.clone(), expression_type.clone());
        self.add_type_constraint(
            ConstraintKind::Convertible(expression_type, variable_type),
            ctx,
            move |s| {
                format!(
                    "{variable_name} ({}) cannot be assigned a {}",
                    variable.substitute(s),
                    expression.substitute(s)
                )
            },
        );
    }

    fn exit_valueFunc(&mut self, ctx: &ValueFuncContext<'input>) {
        let ty = self.type_of_optional(ctx.function_call());
        self.set_type(ctx, ty);
    }

    fn exit_function_call(&mut self, ctx: &Function_callContext<'input>) {
        let function_id = ctx.FUNC_ID();
        let function_name = function_id.as_ref().map(|id| id.get_text());
        let display_name = function_name
            .clone()
            .unwrap_or_else(|| "(unknown function)".to_owned());
        let at_function_name = match function_id.as_ref() {
            Some(id) => self.file.diagnostic_at_token(&*id.symbol),
            None => self.file.diagnostic_at(ctx),
        };

        let declaration = function_name
            .as_deref()
            .and_then(|name| self.state.declarations.get(name));
        let Some((Ty::Function(function_type), is_implicit)) =
            declaration.map(|declaration| (declaration.ty.clone(), declaration.is_implicit))
        else {
            if self.state.language_version >= VERSION_WITH_MANDATORY_FUNCTION_DECLARATIONS {
                self.add_diagnostic(
                    at_function_name,
                    descriptors::UNKNOWN_FUNCTION.create(&[&display_name]),
                );
            } else {
                // Older projects may call functions that the compiler doesn't know. Their type is whatever fits.
                let ty = self.generate_type_variable(Some(&format!(
                    "Return from unknown function {}",
                    function_name.as_deref().unwrap_or("(unknown)")
                )));
                self.set_type(ctx, ty);
                self.add_diagnostic(
                    at_function_name,
                    descriptors::USE_OF_UNDECLARED_FUNCTIONS_IN_V4_IS_DEPRECATED
                        .create(&[&display_name]),
                );
            }
            return;
        };

        // Check each parameter of the function
        let parameters = ctx.expression_all();
        let actual = parameters.len();
        let expected = function_type.parameters.len();
        if actual != expected && function_type.variadic_parameter_type.is_none() {
            let plural = |count: usize| if count == 1 { "parameter" } else { "parameters" };
            let message = if is_implicit {
                format!("{display_name} was called elsewhere with {expected} {}, but is called with {actual} {} here", plural(expected), plural(actual))
            } else {
                format!("{display_name} expects {expected} {}, not {actual}", plural(expected))
            };
            self.add_diagnostic(
                at_function_name,
                descriptors::WRONG_FUNCTION_PARAMETERS.create(&[&message]),
            );
            return;
        }

        let ty = self.generate_type_variable(None);
        self.set_type(ctx, ty.clone());

        for (index, parameter) in parameters.iter().enumerate() {
            let Some(expected_type) = function_type.parameter_at(index).cloned() else {
                let message = format!("Unexpected parameter in call to function {display_name}");
                self.add_diagnostic(
                    self.file.diagnostic_at(parameter.as_ref()),
                    descriptors::WRONG_FUNCTION_PARAMETERS.create(&[&message]),
                );
                continue;
            };
            let actual_type = self.type_of(parameter.as_ref());
            let parameter_text = parameter.get_text();
            let (from, to) = (actual_type.clone(), expected_type.clone());
            self.add_type_constraint(
                ConstraintKind::Convertible(actual_type, expected_type),
                parameter.as_ref(),
                move |s| {
                    format!(
                        "{parameter_text} ({}) is not convertible to {}",
                        from.substitute(s),
                        to.substitute(s)
                    )
                },
            );
        }

        // The type of this function call is the return type of the function
        let return_type = function_type.return_type.clone();
        let (call_type, declared_return_type) = (ty.clone(), return_type.clone());
        self.add_type_constraint(ConstraintKind::Equality(ty, return_type), ctx, move |_| {
            format!("Call to {display_name} returns {declared_return_type}, not {call_type}")
        });
    }

    fn exit_lineCondition(&mut self, ctx: &LineConditionContext<'input>) {
        self.add_boolean_condition_constraint(ctx.expression(), ctx, "line condition's expression");
    }

    fn exit_lineOnceCondition(&mut self, ctx: &LineOnceConditionContext<'input>) {
        self.add_boolean_condition_constraint(ctx.expression(), ctx, "line condition's expression");
    }

    fn exit_line_statement(&mut self, ctx: &Line_statementContext<'input>) {
        // ## Implementation note
        // The original registers the 'once' variable when exiting the condition and searches
        // upwards for the line. Doing it from the line is equivalent and needs no parent lookup.
        let Some(condition) = ctx.line_condition() else {
            return;
        };
        if !matches!(condition.as_ref(), Line_conditionContextAll::LineOnceConditionContext(_)) {
            return;
        }
        let Some(line_id) = self
            .state
            .annotations
            .line(self.file.index, ctx)
            .map(|line| line.line_id.clone())
        else {
            return;
        };
        let once_variable_name =
            Library::generate_unique_content_viewed_variable_name(&line_id.0);
        let description = format!(
            "'once' line condition for line ID {line_id}, in file {}, node {}, line {}",
            self.file.name,
            self.current_node_name.as_deref().unwrap_or_default(),
            condition.start().get_line()
        );
        self.register_once_variable(condition.as_ref(), once_variable_name, description);
    }

    fn exit_once_primary_clause(&mut self, ctx: &Once_primary_clauseContext<'input>) {
        self.add_boolean_condition_constraint(ctx.expression(), ctx, "line condition's expression");
        let line = ctx.start().get_line();
        let once_variable_name = Library::generate_unique_command_block_viewed_variable_name(
            &self.file.name,
            self.current_node_name.as_deref().unwrap_or_default(),
            usize::try_from(line).unwrap_or_default(),
        );
        let description = format!(
            "'once' statement in file {}, node {}, line {line}",
            self.file.name,
            self.current_node_name.as_deref().unwrap_or_default(),
        );
        self.register_once_variable(ctx, once_variable_name.clone(), description);
        self.state
            .annotations
            .once_variables
            .insert(self.file.key(ctx), once_variable_name);
    }

    fn exit_jumpToExpression(&mut self, ctx: &JumpToExpressionContext<'input>) {
        if let Some(expression) = ctx.expression() {
            let ty = self.type_of(expression.as_ref());
            let from = ty.clone();
            self.add_type_constraint(ConstraintKind::Convertible(ty, Ty::String), ctx, move |s| {
                format!(
                    "jump statement's expression must be convertible to String, but {} is not",
                    from.substitute(s)
                )
            });
        }
    }

    fn exit_once_statement(&mut self, ctx: &Once_statementContext<'input>) {
        let expression = ctx
            .once_primary_clause()
            .and_then(|clause| clause.expression());
        self.add_boolean_condition_constraint(expression, ctx, "once statement's expression");
    }

    fn exit_header_when_expression(&mut self, ctx: &Header_when_expressionContext<'input>) {
        if let Some(expression) = ctx.expression() {
            let ty = self.type_of(expression.as_ref());
            self.add_type_constraint(
                ConstraintKind::Equality(ty.clone(), Ty::Boolean),
                ctx,
                move |s| {
                    format!(
                        "'when' header expressions must be a Bool or 'always', not a {}",
                        ty.substitute(s)
                    )
                },
            );
        }
        if ctx.once.is_some()
            && let Some(title) = self.current_node.title.clone()
        {
            let once_variable = Library::generate_unique_content_viewed_variable_name(&title);
            self.register_once_variable(
                ctx,
                once_variable,
                format!("'once' node condition for {title}"),
            );
        }
    }

    fn exit_statement(&mut self, _ctx: &StatementContext<'input>) {
        // We're done with a statement: solve what we can, so that later statements see concrete types.
        if self.type_equations.is_empty() {
            return;
        }
        self.type_equations = apply_solution(&self.state.solution, &self.type_equations);
        if self.type_equations.is_empty() {
            return;
        }

        // Equalities are solved first, because they pin types down, then everything else.
        let (equalities, other_constraints): (Vec<_>, Vec<_>) = self
            .type_equations
            .iter()
            .cloned()
            .partition(Constraint::is_equality_without_errors);
        for constraints in [equalities, other_constraints] {
            if !try_solve(
                &constraints,
                &self.state.known_types,
                &mut self.state.diagnostics,
                &mut self.state.solution,
            ) {
                self.state.failing_constraints.extend(constraints);
            }
        }
        self.type_equations = apply_solution(&self.state.solution, &self.type_equations);
    }

    fn exit_dialogue(&mut self, _ctx: &DialogueContext<'input>) {
        // Whatever is still unsolved at the end of the file gets another chance once all files are checked.
        let remaining = std::mem::take(&mut self.type_equations);
        self.state.failing_constraints.extend(remaining);
    }
}
