//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/Visitors/CodeGenerationVisitor.cs>

use crate::annotations::*;
use crate::prelude::generated::yarnspinnerlexer;
use crate::prelude::generated::yarnspinnerparser::*;
use crate::prelude::generated::yarnspinnerparservisitor::YarnSpinnerParserVisitorCompat;
use crate::prelude::*;
use crate::type_checker::{Decl, Ty};
use antlr4rust::parser_rule_context::ParserRuleContext;
use antlr4rust::token::Token;
use antlr4rust::tree::{ParseTree, ParseTreeVisitorCompat, Tree};
use std::collections::HashMap;
use std::rc::Rc;
use yarnspinner_core::prelude::*;

/// A node that is being generated, along with its debug information.
/// Corresponds to the `ICodeEmitter` interface of the original.
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeEmitter {
    pub(crate) node: Node,
    pub(crate) debug_info: DebugInfo,
}

impl NodeEmitter {
    pub(crate) fn new(node_name: impl Into<String>, file_name: impl Into<String>) -> Self {
        let node_name = node_name.into();
        Self {
            node: Node {
                name: node_name.clone(),
                ..Default::default()
            },
            debug_info: DebugInfo {
                node_name,
                file_name: file_name.into(),
                ..Default::default()
            },
        }
    }

    /// Appends an instruction and returns its index, for [`NodeEmitter::set_destination`].
    pub(crate) fn emit(
        &mut self,
        position: Option<Position>,
        instruction: impl Into<Instruction>,
    ) -> usize {
        let index = self.node.instructions.len();
        self.debug_info.line_positions.insert(index, position);
        self.node.instructions.push(instruction.into());
        index
    }

    /// The index that the next emitted instruction will have.
    pub(crate) fn next_instruction(&self) -> usize {
        self.node.instructions.len()
    }

    /// Makes the jumping instruction at `index` jump to the next emitted instruction.
    pub(crate) fn jump_to_here(&mut self, index: usize) {
        let destination = self.next_instruction();
        let is_jump = self.node.instructions[index].set_destination(destination);
        assert!(is_jump, "Internal error: instruction {index} has no destination");
    }

    pub(crate) fn add_header(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.node.headers.push(Header {
            key: key.into(),
            value: value.into(),
        });
    }
}

/// Everything the code generator needs to know about the compilation.
pub(crate) struct CodeGenerationContext<'a, 'input> {
    pub(crate) annotations: &'a Annotations,
    /// The declarations of all variables, by name.
    pub(crate) variable_declarations: &'a HashMap<String, Decl<'input>>,
}

pub(crate) struct CodeGenerationVisitor<'a, 'input> {
    context: &'a CodeGenerationContext<'a, 'input>,
    /// The file that the context being visited belongs to. Changes while expanding smart variables.
    file: usize,
    emitter: &'a mut NodeEmitter,
    _dummy: (),
}

pub(crate) fn token_to_operator(token: i32) -> Option<Operator> {
    // operators for the standard expressions
    match token {
        yarnspinnerlexer::OPERATOR_LOGICAL_LESS_THAN_EQUALS => Some(Operator::LessThanOrEqualTo),
        yarnspinnerlexer::OPERATOR_LOGICAL_GREATER_THAN_EQUALS => {
            Some(Operator::GreaterThanOrEqualTo)
        }
        yarnspinnerlexer::OPERATOR_LOGICAL_LESS => Some(Operator::LessThan),
        yarnspinnerlexer::OPERATOR_LOGICAL_GREATER => Some(Operator::GreaterThan),
        yarnspinnerlexer::OPERATOR_LOGICAL_EQUALS => Some(Operator::EqualTo),
        yarnspinnerlexer::OPERATOR_LOGICAL_NOT_EQUALS => Some(Operator::NotEqualTo),
        yarnspinnerlexer::OPERATOR_LOGICAL_AND => Some(Operator::And),
        yarnspinnerlexer::OPERATOR_LOGICAL_OR => Some(Operator::Or),
        yarnspinnerlexer::OPERATOR_LOGICAL_XOR => Some(Operator::Xor),
        yarnspinnerlexer::OPERATOR_LOGICAL_NOT => Some(Operator::Not),
        yarnspinnerlexer::OPERATOR_MATHS_ADDITION => Some(Operator::Add),
        yarnspinnerlexer::OPERATOR_MATHS_SUBTRACTION => Some(Operator::Subtract),
        yarnspinnerlexer::OPERATOR_MATHS_MULTIPLICATION => Some(Operator::Multiply),
        yarnspinnerlexer::OPERATOR_MATHS_DIVISION => Some(Operator::Divide),
        yarnspinnerlexer::OPERATOR_MATHS_MODULUS => Some(Operator::Modulo),
        _ => None,
    }
}

/// The name of the library function that implements `operator` for values of type `ty`.
pub(crate) fn get_function_name(ty: &Ty, operator: Operator) -> String {
    ty.to_type().get_canonical_name_for_method(&operator.to_string())
}

pub(crate) fn position_of(token: &(impl Token + ?Sized)) -> Option<Position> {
    Some(Position {
        line: token.get_line_as_usize().saturating_sub(1),
        character: token.get_column_as_usize(),
    })
}

/// Emits code that leaves `not $once_variable` on the stack.
pub(crate) fn emit_once_variable_test(
    emitter: &mut NodeEmitter,
    position: Option<Position>,
    once_variable: &str,
) {
    emitter.emit(
        position,
        PushVariableInstruction {
            variable_name: once_variable.to_owned(),
        },
    );
    // one argument for 'not'
    emitter.emit(position, PushFloatInstruction { value: 1.0 });
    emitter.emit(
        position,
        CallFunctionInstruction {
            function_name: get_function_name(&Ty::Boolean, Operator::Not),
        },
    );
}

/// Emits code that replaces the two booleans on top of the stack by whether both are true.
pub(crate) fn emit_and(emitter: &mut NodeEmitter, position: Option<Position>) {
    emitter.emit(position, PushFloatInstruction { value: 2.0 });
    emitter.emit(
        position,
        CallFunctionInstruction {
            function_name: get_function_name(&Ty::Boolean, Operator::And),
        },
    );
}

/// Emits code that marks 'once' content as seen.
pub(crate) fn emit_set_once_variable(
    emitter: &mut NodeEmitter,
    position: Option<Position>,
    once_variable: &str,
) {
    emitter.emit(position, PushBoolInstruction { value: true });
    emitter.emit(
        position,
        StoreVariableInstruction {
            variable_name: once_variable.to_owned(),
        },
    );
    emitter.emit(position, PopInstruction {});
}

/// How many boolean operators (`and`, `or`, `xor`) an expression contains.
fn boolean_operator_count<'input>(expression: &ExpressionContextAll<'input>) -> usize {
    use ExpressionContextAll::*;
    let count = |expressions: Vec<Rc<ExpressionContextAll<'input>>>| {
        expressions
            .iter()
            .map(|expression| boolean_operator_count(expression))
            .sum::<usize>()
    };
    match expression {
        ExpAndOrXorContext(ctx) => 1 + count(ctx.expression_all()),
        ExpMultDivModContext(ctx) => count(ctx.expression_all()),
        ExpComparisonContext(ctx) => count(ctx.expression_all()),
        ExpAddSubContext(ctx) => count(ctx.expression_all()),
        ExpEqualityContext(ctx) => count(ctx.expression_all()),
        ExpParensContext(ctx) => count(ctx.expression().into_iter().collect()),
        ExpNegativeContext(ctx) => count(ctx.expression().into_iter().collect()),
        ExpNotContext(ctx) => count(ctx.expression().into_iter().collect()),
        ExpValueContext(ctx) => match ctx.value().as_deref() {
            Some(ValueContextAll::ValueFuncContext(function)) => function
                .function_call()
                .map_or(0, |call| count(call.expression_all())),
            _ => 0,
        },
        Error(_) => 0,
    }
}

/// The complexity of a condition: one for the expression itself plus one for every boolean operator in it.
pub(crate) fn expression_complexity<'input>(expression: &ExpressionContextAll<'input>) -> usize {
    1 + boolean_operator_count(expression)
}

/// Corresponds to `Line_conditionContext.ConditionCount`
fn condition_count<'input>(condition: &Line_conditionContextAll<'input>) -> usize {
    let (once, expression) = match condition {
        Line_conditionContextAll::LineOnceConditionContext(once) => (1, once.expression()),
        Line_conditionContextAll::LineConditionContext(condition) => (0, condition.expression()),
        Line_conditionContextAll::Error(_) => (0, None),
    };
    once + expression.map_or(0, |expression| expression_complexity(expression.as_ref()))
}

impl<'a, 'input: 'a> CodeGenerationVisitor<'a, 'input> {
    pub(crate) fn new(
        context: &'a CodeGenerationContext<'a, 'input>,
        file: usize,
        emitter: &'a mut NodeEmitter,
    ) -> Self {
        Self {
            context,
            file,
            emitter,
            _dummy: (),
        }
    }

    fn type_of(&self, ctx: &impl ParserRuleContext<'input>) -> Ty {
        self.context.annotations.type_of(self.file, ctx)
    }

    fn line_id(&self, line_statement: &Line_statementContext<'input>) -> LineId {
        self.context.annotations.line_id(self.file, line_statement)
    }

    fn start_of(ctx: &impl ParserRuleContext<'input>) -> Option<Position> {
        position_of(&*ctx.start())
    }

    fn stop_of(ctx: &impl ParserRuleContext<'input>) -> Option<Position> {
        position_of(&*ctx.stop())
    }

    /// Visits all expressions in a formatted text, which leaves their values on the stack, and returns their count.
    /// The text itself has already been assembled by the string table generator.
    fn generate_code_for_expressions_in_formatted_text(
        &mut self,
        nodes: impl Iterator<Item = Rc<ActualParserContext<'input>>>,
    ) -> usize {
        nodes
            .filter(|child| child.get_child_count() > 0)
            .map(|child| self.visit(child.as_ref()))
            .count()
    }

    /// Emits code that calls a method appropriate for the operator on the type, given the operands.
    fn generate_code_for_operation(
        &mut self,
        operator: Operator,
        operator_token: &impl Token,
        ty: &Ty,
        operands: &[Rc<ActualParserContext<'input>>],
    ) {
        // Generate code for each of the operands, so that their value is now on the stack.
        for operand in operands {
            self.visit(operand.as_ref());
        }
        let position = position_of(operator_token);
        // Indicate that we are pushing this many items for comparison
        self.emitter.emit(
            position,
            PushFloatInstruction {
                value: operands.len() as f32,
            },
        );
        self.emitter.emit(
            position,
            CallFunctionInstruction {
                function_name: get_function_name(ty, operator),
            },
        );
    }

    fn generate_code_for_binary_operation(
        &mut self,
        operator_token: &impl Token,
        left: Option<Rc<ExpressionContextAll<'input>>>,
        right: Option<Rc<ExpressionContextAll<'input>>>,
    ) {
        let operator = token_to_operator(operator_token.get_token_type()).unwrap();
        let (left, right) = (left.unwrap(), right.unwrap());
        // The implementation is chosen by the type of the first operand
        let ty = self.type_of(left.as_ref());
        self.generate_code_for_operation(operator, operator_token, &ty, &[left, right]);
    }

    /// Emits a clause of an if statement. Returns the jump to the end of the statement that needs its destination set.
    fn generate_clause(
        &mut self,
        clause_stop: Option<Position>,
        children: &[Rc<StatementContextAll<'input>>],
        expression: Option<Rc<ExpressionContextAll<'input>>>,
    ) -> usize {
        // handling the expression (if it has one) will only be called on ifs and elseifs
        let jump_to_end_of_clause = expression.as_ref().map(|expression| {
            // Code-generate the expression
            self.visit(expression.as_ref());
            self.emitter.emit(
                Self::start_of(expression.as_ref()),
                JumpIfFalseInstruction { destination: -1 },
            )
        });

        // running through all of the children statements
        for child in children {
            self.visit(child.as_ref());
        }

        let jump_to_end_of_if_statement = self
            .emitter
            .emit(clause_stop, JumpToInstruction { destination: -1 });

        if let Some(jump_to_end_of_clause) = jump_to_end_of_clause {
            self.emitter.jump_to_here(jump_to_end_of_clause);
            // The value of the expression is still on the stack
            self.emitter.emit(clause_stop, PopInstruction {});
        }
        jump_to_end_of_if_statement
    }

    /// Emits code that leaves whether the condition of a line passes on the stack, if the line has a condition.
    /// Returns the name of the 'once' variable if it has a 'once' condition.
    fn evaluate_line_condition(
        &mut self,
        line_statement: &Line_statementContext<'input>,
    ) -> Option<String> {
        let condition = line_statement.line_condition()?;
        match condition.as_ref() {
            Line_conditionContextAll::LineOnceConditionContext(once) => {
                let once_variable = Library::generate_unique_content_viewed_variable_name(
                    &self.line_id(line_statement).0,
                );
                // Test to see if the 'once' variable for this content is false
                let position = once.COMMAND_ONCE().and_then(|token| position_of(&*token.symbol));
                emit_once_variable_test(self.emitter, position, &once_variable);

                // If the condition has an expression, evaluate that too and 'and' it with the 'once' test
                if let Some(expression) = once.expression() {
                    self.visit(expression.as_ref());
                    emit_and(self.emitter, Self::start_of(expression.as_ref()));
                }
                Some(once_variable)
            }
            Line_conditionContextAll::LineConditionContext(condition) => {
                // Evaluate the condition and put it on the stack
                self.visit(condition.expression()?.as_ref());
                None
            }
            Line_conditionContextAll::Error(_) => None,
        }
    }

    fn emit_jump_to_named_node(&mut self, position: Option<Position>, node_name: String, detour: bool) {
        if detour {
            self.emitter
                .emit(position, DetourToNodeInstruction { node_name });
        } else {
            self.emitter.emit(position, RunNodeInstruction { node_name });
        }
    }

    fn emit_jump_to_expression(
        &mut self,
        position: Option<Position>,
        expression: &ExpressionContextAll<'input>,
        detour: bool,
    ) {
        // Evaluate the expression, and jump to the result on the stack.
        self.visit(expression);
        if detour {
            self.emitter.emit(position, PeekAndDetourToNode {});
        } else {
            self.emitter.emit(position, PeekAndRunNodeInstruction {});
        }
    }
}

impl<'a, 'input: 'a> ParseTreeVisitorCompat<'input> for CodeGenerationVisitor<'a, 'input> {
    type Node = YarnSpinnerParserContextType;
    type Return = ();

    fn temp_result(&mut self) -> &mut Self::Return {
        &mut self._dummy
    }
}

#[allow(non_snake_case)]
impl<'a, 'input: 'a> YarnSpinnerParserVisitorCompat<'input> for CodeGenerationVisitor<'a, 'input> {
    /// a regular ol' line of text
    fn visit_line_statement(&mut self, ctx: &Line_statementContext<'input>) -> Self::Return {
        let line_id = self.line_id(ctx);

        // Evaluate any condition on the line statement.
        self.evaluate_line_condition(ctx);
        let condition = ctx.line_condition();
        let condition_position = condition.as_ref().and_then(|c| Self::start_of(c.as_ref()));

        // Jump over this line if the line's condition is false.
        let jump_if_condition_false = condition.as_ref().map(|_| {
            self.emitter
                .emit(condition_position, JumpIfFalseInstruction { destination: -1 })
        });

        if let Some(Line_conditionContextAll::LineOnceConditionContext(_)) = condition.as_deref() {
            // The line has a 'once' condition that has evaluated to true.
            // Set the corresponding variable for it so that it doesn't appear again.
            let once_variable =
                Library::generate_unique_content_viewed_variable_name(&line_id.0);
            emit_set_once_variable(self.emitter, condition_position, &once_variable);
        }

        // Evaluate the inline expressions and push the results onto the stack.
        let formatted_text = ctx.line_formatted_text().unwrap();
        let expression_count =
            self.generate_code_for_expressions_in_formatted_text(formatted_text.get_children());

        // Run the line.
        self.emitter.emit(
            Self::start_of(formatted_text.as_ref()),
            RunLineInstruction {
                line_id: line_id.0,
                substitution_count: expression_count as i32,
            },
        );

        if let Some(jump) = jump_if_condition_false {
            // We generated a jump instruction to jump over the line. Update its destination to immediately after the line.
            self.emitter.jump_to_here(jump);
            // We evaluated a condition, and the value is still on the stack. Pop the bool off the stack.
            self.emitter.emit(condition_position, PopInstruction {});
        }
    }

    /// A set command: explicitly setting a value to an expression <<set $foo to 1>>
    fn visit_set_statement(&mut self, ctx: &Set_statementContext<'input>) -> Self::Return {
        // Ensure that the correct result is on the stack by evaluating the
        // expression. If this assignment includes an operation (e.g. +=), do that work here too.
        let operator_token = ctx.op.as_ref().unwrap();
        let expression = ctx.expression().unwrap();
        let variable = ctx.variable().unwrap();
        let operator = match operator_token.get_token_type() {
            yarnspinnerlexer::OPERATOR_ASSIGNMENT => None,
            yarnspinnerlexer::OPERATOR_MATHS_ADDITION_EQUALS => Some(Operator::Add),
            yarnspinnerlexer::OPERATOR_MATHS_SUBTRACTION_EQUALS => Some(Operator::Subtract),
            yarnspinnerlexer::OPERATOR_MATHS_MULTIPLICATION_EQUALS => Some(Operator::Multiply),
            yarnspinnerlexer::OPERATOR_MATHS_DIVISION_EQUALS => Some(Operator::Divide),
            yarnspinnerlexer::OPERATOR_MATHS_MODULUS_EQUALS => Some(Operator::Modulo),
            _ => unreachable!("Internal error: invalid assignment operator"),
        };
        match operator {
            None => self.visit(expression.as_ref()),
            Some(operator) => {
                let ty = self.type_of(expression.as_ref());
                self.generate_code_for_operation(
                    operator,
                    operator_token.as_ref(),
                    &ty,
                    &[variable.clone(), expression],
                );
            }
        }

        // now store the variable and clean up the stack
        let position = Self::start_of(ctx);
        self.emitter.emit(
            position,
            StoreVariableInstruction {
                variable_name: variable.get_text(),
            },
        );
        self.emitter.emit(position, PopInstruction {});
    }

    /// semi-free form text that gets passed along to the game for things
    /// like <<turn fred left>> or <<unlockAchievement FacePlant>>
    fn visit_command_statement(&mut self, ctx: &Command_statementContext<'input>) -> Self::Return {
        let Some(formatted_text) = ctx.command_formatted_text() else {
            return;
        };
        let mut composed_string = String::new();
        let mut expression_count = 0;
        for node in formatted_text.get_children() {
            if node.get_child_count() == 0 {
                // Terminal node
                composed_string.push_str(&node.get_text());
            } else {
                // Generate code for evaluating the expression at runtime
                self.visit(node.as_ref());
                // Don't include the '{' and '}', because it will have been added as a terminal node already
                composed_string.push_str(&expression_count.to_string());
                expression_count += 1;
            }
        }

        let position = Self::start_of(formatted_text.as_ref());
        // [sic] TODO: look into replacing this as it seems a bit odd
        if composed_string == "stop" {
            // "stop" is a special command that immediately stops execution
            self.emitter.emit(position, StopInstruction {});
        } else {
            self.emitter.emit(
                position,
                RunCommandInstruction {
                    command_text: composed_string,
                    substitution_count: expression_count,
                },
            );
        }
    }

    /// handles emitting the correct instructions for the function
    fn visit_function_call(&mut self, ctx: &Function_callContext<'input>) -> Self::Return {
        // generate the instructions for all of the parameters
        let parameters = ctx.expression_all();
        for parameter in &parameters {
            self.visit(parameter.as_ref());
        }
        let position = Self::start_of(ctx);
        // push the number of parameters onto the stack
        self.emitter.emit(
            position,
            PushFloatInstruction {
                value: parameters.len() as f32,
            },
        );
        // then call the function itself
        self.emitter.emit(
            position,
            CallFunctionInstruction {
                function_name: ctx.FUNC_ID().unwrap().get_text(),
            },
        );
    }

    /// if statement ifclause (elseifclause)* (elseclause)? <<endif>>
    fn visit_if_statement(&mut self, ctx: &If_statementContext<'input>) -> Self::Return {
        let mut jumps_to_end_of_if_statement = Vec::new();

        // handle the if
        let if_clause = ctx.if_clause().unwrap();
        jumps_to_end_of_if_statement.push(self.generate_clause(
            Self::stop_of(if_clause.as_ref()),
            &if_clause.statement_all(),
            if_clause.expression(),
        ));

        // all elseifs
        for else_if_clause in ctx.else_if_clause_all() {
            jumps_to_end_of_if_statement.push(self.generate_clause(
                Self::stop_of(else_if_clause.as_ref()),
                &else_if_clause.statement_all(),
                else_if_clause.expression(),
            ));
        }

        // the else, if there is one
        if let Some(else_clause) = ctx.else_clause() {
            jumps_to_end_of_if_statement.push(self.generate_clause(
                Self::stop_of(else_clause.as_ref()),
                &else_clause.statement_all(),
                None,
            ));
        }

        for jump in jumps_to_end_of_if_statement {
            self.emitter.jump_to_here(jump);
        }
    }

    /// for the shortcut options (-> line of text <<if expression>> indent statements dedent)+
    fn visit_shortcut_option_statement(
        &mut self,
        ctx: &Shortcut_option_statementContext<'input>,
    ) -> Self::Return {
        let shortcuts = ctx.shortcut_option_all();
        let mut add_option_instructions = Vec::new();
        let mut once_variables = Vec::new();

        // For each option, evaluate its associated line_statement, and use that as the
        // option text. Then, add this option to the list of upcoming options.
        for shortcut in &shortcuts {
            let line_statement = shortcut.line_statement().unwrap();
            // Get the line ID for the shortcut's line
            let line_id = self.line_id(&line_statement);

            // This line statement may have a condition on it. If it does, emit code that
            // evaluates the condition, and add a flag on the 'Add Option' instruction that
            // indicates that a condition exists.
            once_variables.push(self.evaluate_line_condition(&line_statement));
            let has_condition = line_statement.line_condition().is_some();

            // Figure out the text that we want to add. This will involve evaluating any inline expressions.
            let expression_count = self.generate_code_for_expressions_in_formatted_text(
                line_statement.line_formatted_text().unwrap().get_children(),
            );

            // And add this option to the list.
            add_option_instructions.push(self.emitter.emit(
                Self::start_of(line_statement.as_ref()),
                AddOptionInstruction {
                    line_id: line_id.0,
                    destination: -1,
                    substitution_count: expression_count as i32,
                    has_condition,
                },
            ));
        }

        let position = Self::start_of(ctx);
        // All of the options that we intend to show are now ready to go. Show them.
        self.emitter.emit(position, ShowOptionsInstruction {});
        // If no option was selected, 'false' is on the stack and we fall through to after the options.
        let mut jumps_to_end_of_group = vec![
            self.emitter
                .emit(position, JumpIfFalseInstruction { destination: -1 }),
        ];
        // we didn't jump so top of the stack contains a true. clean this up by popping it off
        self.emitter.emit(position, PopInstruction {});
        // The top of the stack now contains the instruction we want to jump to. Jump to it now.
        self.emitter.emit(position, PeekAndJumpInstruction {});

        // We'll now emit the code associated with each option.
        for ((shortcut, add_option), once_variable) in shortcuts
            .iter()
            .zip(add_option_instructions)
            .zip(once_variables)
        {
            // Make this option's AddOption instruction point at where we are now.
            self.emitter.jump_to_here(add_option);
            // top of the stack contains our jumped destination, popping that off the stack
            self.emitter.emit(None, PopInstruction {});

            if let Some(once_variable) = once_variable {
                // This option has a 'once' condition on it. Generate code that sets the
                // corresponding 'once' variable to true, so that we don't see it again.
                let position = Self::start_of(shortcut.line_statement().unwrap().as_ref());
                emit_set_once_variable(self.emitter, position, &once_variable);
            }

            // Run through all the children statements of the option.
            for child in shortcut.statement_all() {
                self.visit(child.as_ref());
            }

            // Jump to the end of this shortcut option group.
            jumps_to_end_of_group.push(self.emitter.emit(
                Self::stop_of(shortcut.as_ref()),
                JumpToInstruction { destination: -1 },
            ));
        }

        // We made it to the end! Update all jump-to-end instructions to point to where we are now.
        for jump in jumps_to_end_of_group {
            self.emitter.jump_to_here(jump);
        }
    }

    /// Each line group works like this:
    /// - For each item in the line group, evaluate its condition if it has one, or else push
    ///   true onto the stack. Then, tell the VM to add a candidate for that item.
    /// - Next, tell the VM to select the best item. If one was selected, then the stack will
    ///   contain (true, destination); otherwise, it will contain (false).
    /// - If we had content, jump to the appropriate destination, which contains code for
    ///   displaying the line, and then jumps to the end of the line group's instructions.
    ///   (If the item's condition was a 'once' condition, we update the 'once' flag here.)
    /// - If we didn't, jump straight to the end immediately.
    fn visit_line_group_statement(
        &mut self,
        ctx: &Line_group_statementContext<'input>,
    ) -> Self::Return {
        let items = ctx.line_group_item_all();
        let mut once_variables = Vec::new();
        let mut add_candidate_instructions = Vec::new();

        for item in &items {
            let line_statement = item.line_statement().unwrap();
            let line_id = self.line_id(&line_statement);
            let position = Self::start_of(line_statement.as_ref());

            let condition_count = match line_statement.line_condition() {
                Some(condition) => {
                    // This line group item has a condition. Evaluate it - this will result in
                    // its value being left on the stack.
                    once_variables.push(self.evaluate_line_condition(&line_statement));
                    condition_count(condition.as_ref())
                }
                None => {
                    // There is no expression; push 'true' onto the stack and note that it had a complexity of zero
                    once_variables.push(None);
                    self.emitter.emit(position, PushBoolInstruction { value: true });
                    0
                }
            };

            // Add this line group item. We'll update where it jumps to later.
            add_candidate_instructions.push(self.emitter.emit(
                position,
                AddSaliencyCandidateInstruction {
                    content_id: line_id.0,
                    complexity_score: condition_count as i32,
                    destination: -1,
                },
            ));
        }

        let position = Self::start_of(ctx);
        // We've added all of our candidates; now query which one to jump to
        self.emitter
            .emit(position, SelectSaliencyCandidateInstruction {});
        // The top of the stack now contains 'true' if a piece of content was selected, and 'false' if not.
        // If it's false, immediately jump to the end of this entire line group.
        let mut jumps_to_end_of_line_group = vec![
            self.emitter
                .emit(position, JumpIfFalseInstruction { destination: -1 }),
        ];
        // Pop the 'has content' value off the top of the stack.
        self.emitter.emit(position, PopInstruction {});
        // Jump to the instruction indicated by the top of the stack.
        self.emitter.emit(position, PeekAndJumpInstruction {});

        // Now generate the code for each of the lines in the group.
        for ((item, add_candidate), once_variable) in items
            .iter()
            .zip(add_candidate_instructions)
            .zip(once_variables)
        {
            let line_statement = item.line_statement().unwrap();
            let position = Self::start_of(line_statement.as_ref());
            // Ensure that the 'add candidate' instruction that points us to here has the correct destination
            self.emitter.jump_to_here(add_candidate);
            // We got here via a peek-and-jump; we can discard the top of the stack now.
            self.emitter.emit(position, PopInstruction {});

            if let Some(once_variable) = once_variable {
                // We have a once variable for this line group item. Emit code that sets it to true.
                emit_set_once_variable(self.emitter, position, &once_variable);
            }

            // Run this line. (We don't visit the line statement, because that would evaluate its condition again.)
            let formatted_text = line_statement.line_formatted_text().unwrap();
            let expression_count =
                self.generate_code_for_expressions_in_formatted_text(formatted_text.get_children());
            self.emitter.emit(
                Self::start_of(formatted_text.as_ref()),
                RunLineInstruction {
                    line_id: self.line_id(&line_statement).0,
                    substitution_count: expression_count as i32,
                },
            );

            for child in item.statement_all() {
                self.visit(child.as_ref());
            }

            jumps_to_end_of_line_group.push(self.emitter.emit(
                Self::stop_of(item.as_ref()),
                JumpToInstruction { destination: -1 },
            ));
        }

        for jump in jumps_to_end_of_line_group {
            self.emitter.jump_to_here(jump);
        }
    }

    /// (expression)
    fn visit_expParens(&mut self, ctx: &ExpParensContext<'input>) -> Self::Return {
        self.visit(ctx.expression().unwrap().as_ref())
    }

    /// -expression
    fn visit_expNegative(&mut self, ctx: &ExpNegativeContext<'input>) -> Self::Return {
        let ty = self.type_of(ctx);
        self.generate_code_for_operation(
            Operator::UnarySubtract,
            ctx.op.as_ref().unwrap().as_ref(),
            &ty,
            &[ctx.expression().unwrap()],
        )
    }

    /// (not NOT !)expression
    fn visit_expNot(&mut self, ctx: &ExpNotContext<'input>) -> Self::Return {
        let ty = self.type_of(ctx);
        self.generate_code_for_operation(
            Operator::Not,
            ctx.op.as_ref().unwrap().as_ref(),
            &ty,
            &[ctx.expression().unwrap()],
        )
    }

    fn visit_expValue(&mut self, ctx: &ExpValueContext<'input>) -> Self::Return {
        self.visit(ctx.value().unwrap().as_ref())
    }

    /// * / %
    fn visit_expMultDivMod(&mut self, ctx: &ExpMultDivModContext<'input>) -> Self::Return {
        self.generate_code_for_binary_operation(
            ctx.op.as_ref().unwrap().as_ref(),
            ctx.expression(0),
            ctx.expression(1),
        )
    }

    /// + -
    fn visit_expAddSub(&mut self, ctx: &ExpAddSubContext<'input>) -> Self::Return {
        self.generate_code_for_binary_operation(
            ctx.op.as_ref().unwrap().as_ref(),
            ctx.expression(0),
            ctx.expression(1),
        )
    }

    /// < <= > >=
    fn visit_expComparison(&mut self, ctx: &ExpComparisonContext<'input>) -> Self::Return {
        self.generate_code_for_binary_operation(
            ctx.op.as_ref().unwrap().as_ref(),
            ctx.expression(0),
            ctx.expression(1),
        )
    }

    /// == !=
    fn visit_expEquality(&mut self, ctx: &ExpEqualityContext<'input>) -> Self::Return {
        self.generate_code_for_binary_operation(
            ctx.op.as_ref().unwrap().as_ref(),
            ctx.expression(0),
            ctx.expression(1),
        )
    }

    /// and && or || xor ^
    fn visit_expAndOrXor(&mut self, ctx: &ExpAndOrXorContext<'input>) -> Self::Return {
        self.generate_code_for_binary_operation(
            ctx.op.as_ref().unwrap().as_ref(),
            ctx.expression(0),
            ctx.expression(1),
        )
    }

    fn visit_valueVar(&mut self, ctx: &ValueVarContext<'input>) -> Self::Return {
        self.visit(ctx.variable().unwrap().as_ref())
    }

    fn visit_valueNumber(&mut self, ctx: &ValueNumberContext<'input>) -> Self::Return {
        let value: f32 = ctx.NUMBER().unwrap().get_text().parse().unwrap();
        self.emitter
            .emit(Self::start_of(ctx), PushFloatInstruction { value });
    }

    fn visit_valueTrue(&mut self, ctx: &ValueTrueContext<'input>) -> Self::Return {
        self.emitter
            .emit(Self::start_of(ctx), PushBoolInstruction { value: true });
    }

    fn visit_valueFalse(&mut self, ctx: &ValueFalseContext<'input>) -> Self::Return {
        self.emitter
            .emit(Self::start_of(ctx), PushBoolInstruction { value: false });
    }

    fn visit_variable(&mut self, ctx: &VariableContext<'input>) -> Self::Return {
        let variable_name = ctx.VAR_ID().unwrap().get_text();

        // Get the declaration for this variable
        let declaration = self
            .context
            .variable_declarations
            .get(&variable_name)
            .unwrap_or_else(|| {
                panic!("Internal error during code generation: variable {variable_name} has no declaration")
            });

        // Is this variable a 'smart variable'?
        if declaration.is_inline_expansion {
            // Then code-generate its value expression. It may come from a different file.
            let (file, expression) = declaration.initial_value.clone().unwrap();
            let previous_file = std::mem::replace(&mut self.file, file);
            self.visit(expression.as_ref());
            self.file = previous_file;
        } else {
            // Otherwise, generate the code that fetches the variable from storage
            self.emitter.emit(
                Self::start_of(ctx),
                PushVariableInstruction { variable_name },
            );
        }
    }

    fn visit_valueString(&mut self, ctx: &ValueStringContext<'input>) -> Self::Return {
        // [sic] stripping the " off the front and back actually is this what we want?
        let value = ctx.STRING().unwrap().get_text().trim_matches('"').to_owned();
        self.emitter
            .emit(Self::start_of(ctx), PushStringInstruction { value });
    }

    /// all we need do is visit the function itself, it will handle everything
    fn visit_valueFunc(&mut self, ctx: &ValueFuncContext<'input>) -> Self::Return {
        self.visit(ctx.function_call().unwrap().as_ref())
    }

    /// A reference to an enum case, which compiles to its raw value.
    fn visit_valueTypeMemberReference(
        &mut self,
        ctx: &ValueTypeMemberReferenceContext<'input>,
    ) -> Self::Return {
        let ty = self.type_of(ctx);
        let member_name = ctx
            .typeMemberReference()
            .unwrap()
            .memberName
            .as_ref()
            .unwrap()
            .get_text()
            .to_owned();
        let Ty::Enum(enum_type) = &ty else {
            panic!("Internal error during code generation: type {ty} has no member {member_name}");
        };
        let case = enum_type.case(&member_name).unwrap_or_else(|| {
            panic!("Internal error during code generation: type {ty} has no member {member_name}")
        });
        let position = Self::start_of(ctx);
        match case.value.clone() {
            YarnValue::String(value) => self.emitter.emit(position, PushStringInstruction { value }),
            YarnValue::Number(value) => self.emitter.emit(position, PushFloatInstruction { value }),
            YarnValue::Boolean(_) => panic!(
                "Internal error: {ty}.{member_name} has a boolean raw value, which is not allowed."
            ),
        };
    }

    fn visit_declare_statement(&mut self, _ctx: &Declare_statementContext<'input>) -> Self::Return {
        // Declare statements do not participate in code generation
    }

    fn visit_enum_statement(&mut self, _ctx: &Enum_statementContext<'input>) -> Self::Return {
        // Enum statements do not participate in code generation
    }

    /// A <<jump>> command, which immediately jumps to another node, given its name.
    fn visit_jumpToNodeName(&mut self, ctx: &JumpToNodeNameContext<'input>) -> Self::Return {
        let destination = ctx.destination.as_ref().unwrap().get_text().to_owned();
        self.emit_jump_to_named_node(Self::start_of(ctx), destination, false);
    }

    /// A <<jump>> command, which immediately jumps to another node, given an expression that resolves to a node's name.
    fn visit_jumpToExpression(&mut self, ctx: &JumpToExpressionContext<'input>) -> Self::Return {
        self.emit_jump_to_expression(Self::start_of(ctx), ctx.expression().unwrap().as_ref(), false);
    }

    fn visit_detourToNodeName(&mut self, ctx: &DetourToNodeNameContext<'input>) -> Self::Return {
        let destination = ctx.destination.as_ref().unwrap().get_text().to_owned();
        self.emit_jump_to_named_node(Self::start_of(ctx), destination, true);
    }

    fn visit_detourToExpression(&mut self, ctx: &DetourToExpressionContext<'input>) -> Self::Return {
        self.emit_jump_to_expression(Self::start_of(ctx), ctx.expression().unwrap().as_ref(), true);
    }

    fn visit_return_statement(&mut self, ctx: &Return_statementContext<'input>) -> Self::Return {
        self.emitter.emit(Self::start_of(ctx), ReturnInstruction {});
    }

    fn visit_once_statement(&mut self, ctx: &Once_statementContext<'input>) -> Self::Return {
        let primary_clause = ctx.once_primary_clause().unwrap();
        let once_variable = self
            .context
            .annotations
            .once_variables
            .get(&(self.file, crate::visitors::GetHashableInterval::get_hashable_interval(primary_clause.as_ref())))
            .expect("Internal error: once statement primary clause is missing a once variable")
            .clone();
        let position = Self::start_of(primary_clause.as_ref());

        // Test to see if the 'once' variable for this block is false
        emit_once_variable_test(self.emitter, position, &once_variable);

        // If the block has a condition, evaluate that too and 'and' it with the 'once' test
        if let Some(expression) = primary_clause.expression() {
            self.visit(expression.as_ref());
            emit_and(self.emitter, position);
        }

        let jump_over_primary_clause = self
            .emitter
            .emit(position, JumpIfFalseInstruction { destination: -1 });

        // We are running the block. Remember that, so that it doesn't run again.
        emit_set_once_variable(self.emitter, position, &once_variable);

        for statement in primary_clause.statement_all() {
            self.visit(statement.as_ref());
        }

        if let Some(alternate_clause) = ctx.once_alternate_clause() {
            // The alternate clause runs if the primary one didn't, so the primary one jumps over it at its end.
            let jump_over_alternate_clause = self.emitter.emit(
                Self::stop_of(primary_clause.as_ref()),
                JumpToInstruction { destination: -1 },
            );
            self.emitter.jump_to_here(jump_over_primary_clause);
            for statement in alternate_clause.statement_all() {
                self.visit(statement.as_ref());
            }
            self.emitter.jump_to_here(jump_over_alternate_clause);
        } else {
            self.emitter.jump_to_here(jump_over_primary_clause);
        }

        // The result of the test is still on the stack
        self.emitter.emit(Self::stop_of(ctx), PopInstruction {});
    }
}
