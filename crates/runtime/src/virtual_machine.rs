//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner/VirtualMachine.cs>
//! and <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner/SmartVariableEvaluationVirtualMachine.cs>
//!
//! ## Implementation Notes
//! - The `Operand` extensions and the `Operator` enum were moved into upstream crates to make them not depend on the runtime.
//! - The C# VM is async and awaits its `IDialogueResponder`. Here, content is returned as batched [`DialogueEvent`]s instead
//!   and the VM pauses until [`VirtualMachine::continue_`] is called again.

pub(crate) use self::{execution_state::*, state::*};
use crate::Result;
use crate::markup::{LineParser, ParsedMarkup};
use crate::prelude::*;
use core::fmt::Debug;
use log::*;

mod execution_state;
mod state;

/// The function that the VM calls when the built-in `has_any_content` is not overridden in the library.
const HAS_ANY_CONTENT_FUNCTION: &str = "has_any_content";

/// Calls a library function. Abstracts over calling with and without a Bevy world.
pub(crate) type FunctionCaller<'a> =
    dyn FnMut(&dyn UntypedYarnFn, Vec<YarnValue>) -> YarnValue + 'a;

/// Borrows only the fields an [`EvaluationContext`] needs, so that the stack can be borrowed mutably at the same time.
macro_rules! context {
    ($vm:expr) => {
        EvaluationContext {
            program: $vm
                .program
                .as_ref()
                .expect("Internal error: the VM is running without a program"),
            library: &$vm.library,
            variable_storage: $vm.variable_storage.as_ref(),
            saliency_strategy: $vm.saliency_strategy.as_ref(),
        }
    };
}

#[derive(Debug, Clone)]
pub(crate) struct VirtualMachine {
    pub(crate) library: Library,
    pub(crate) program: Option<Program>,
    pub(crate) variable_storage: Box<dyn VariableStorage>,
    pub(crate) line_hints_enabled: bool,
    pub(crate) saliency_strategy: Box<dyn ContentSaliencyStrategy>,
    state: State,
    execution_state: ExecutionState,
    batched_events: Vec<DialogueEvent>,
    line_parser: LineParser,
    text_provider: Box<dyn TextProvider>,
    language_code: Option<Language>,
    saliency_candidates: Vec<ContentSaliencyOption>,
}

impl VirtualMachine {
    pub(crate) fn new(
        library: Library,
        variable_storage: Box<dyn VariableStorage>,
        line_parser: LineParser,
        text_provider: Box<dyn TextProvider>,
    ) -> Self {
        Self {
            library,
            variable_storage,
            line_parser,
            text_provider,
            saliency_strategy: Box::new(RandomBestLeastRecentlyViewedSaliencyStrategy),
            language_code: Default::default(),
            program: Default::default(),
            state: Default::default(),
            execution_state: Default::default(),
            batched_events: Default::default(),
            line_hints_enabled: Default::default(),
            saliency_candidates: Default::default(),
        }
    }

    pub(crate) fn text_provider(&self) -> &dyn TextProvider {
        self.text_provider.as_ref()
    }

    pub(crate) fn text_provider_mut(&mut self) -> &mut dyn TextProvider {
        self.text_provider.as_mut()
    }

    pub(crate) fn variable_storage(&self) -> &dyn VariableStorage {
        self.variable_storage.as_ref()
    }

    pub(crate) fn variable_storage_mut(&mut self) -> &mut dyn VariableStorage {
        self.variable_storage.as_mut()
    }

    pub(crate) fn set_language_code(&mut self, language_code: impl Into<Option<Language>>) {
        let language_code = language_code.into();
        self.language_code.clone_from(&language_code);
        self.line_parser.set_language_code(language_code.clone());
        self.text_provider.set_language(language_code);
    }

    pub(crate) fn reset_state(&mut self) {
        self.state = State::default();
        self.saliency_candidates.clear();
    }

    pub(crate) fn current_node_name(&self) -> Option<String> {
        self.state.current_node_name.clone()
    }

    pub(crate) fn set_execution_state(&mut self, execution_state: ExecutionState) -> &mut Self {
        self.execution_state = execution_state;
        if execution_state == ExecutionState::Stopped {
            self.reset_state()
        }
        self
    }

    /// # Implementation Notes
    /// The original does not reset the state upon calling this. I suspect that's a bug.
    pub(crate) fn stop(&mut self) -> Vec<DialogueEvent> {
        self.set_execution_state(ExecutionState::Stopped);
        self.batched_events.push(DialogueEvent::DialogueComplete);
        core::mem::take(&mut self.batched_events)
    }

    pub(crate) fn set_node(&mut self, node_name: impl Into<String>) -> Result<()> {
        self.set_node_with_state(node_name.into(), true)
    }

    fn set_node_with_state(&mut self, node_name: String, clear_state: bool) -> Result<()> {
        debug!("Loading node \"{node_name}\"");
        self.get_node_from_name(&node_name)?; // must exist

        if clear_state {
            self.reset_state();
        }
        self.state.current_node_name = Some(node_name.clone());
        self.state.program_counter = 0;

        self.batched_events
            .push(DialogueEvent::NodeStart(node_name.clone()));

        if self.line_hints_enabled {
            self.send_line_hints(&node_name);
        }
        Ok(())
    }

    fn send_line_hints(&mut self, node_name: &str) {
        // Every instruction that runs a line or adds an option signals a line that can appear to the player
        let string_ids = self
            .program
            .as_ref()
            .and_then(|program| program.line_ids_for_node(node_name))
            .unwrap_or_default();
        self.text_provider.accept_line_hints(&string_ids);
        self.batched_events
            .push(DialogueEvent::LineHints(string_ids));
    }

    pub(crate) fn pop_line_hints(&mut self) -> Option<Vec<LineId>> {
        match self.batched_events.pop() {
            Some(DialogueEvent::LineHints(string_ids)) => Some(string_ids),
            Some(event) => {
                self.batched_events.push(event);
                None
            }
            None => None,
        }
    }

    fn get_node_from_name(&self, node_name: &str) -> Result<&Node> {
        let program = self
            .program
            .as_ref()
            .ok_or(DialogueError::NoProgramLoaded)?;
        program
            .nodes
            .get(node_name)
            .ok_or_else(|| DialogueError::InvalidNode {
                node_name: node_name.to_owned(),
            })
    }

    /// Reports that a node was left and counts the visit if the node is tracked.
    ///
    /// Since Yarn Spinner 3, visit tracking is done here instead of by compiler-generated code.
    fn return_from_node(&mut self, node_name: &str) {
        let Ok(node) = self.get_node_from_name(node_name) else {
            return;
        };
        let tracking_variable = node.tracking_variable_name();
        self.batched_events
            .push(DialogueEvent::NodeComplete(node_name.to_owned()));

        if let Some(tracking_variable) = tracking_variable {
            match self.get_stored_or_initial_value(&tracking_variable) {
                Some(YarnValue::Number(count)) => {
                    if let Err(e) = self
                        .variable_storage
                        .set(tracking_variable, (count + 1.0).into())
                    {
                        error!("Failed to update the tracking variable for node {node_name}: {e}");
                    }
                }
                _ => error!("Failed to get the tracking variable for node {node_name}"),
            }
        }
    }

    /// Leaves the current node and every node that detoured into it.
    fn return_from_all_nodes(&mut self) {
        if let Some(current) = self.state.current_node_name.clone() {
            self.return_from_node(&current);
        }
        while let Some(call_site) = self.state.call_stack.pop() {
            self.return_from_node(&call_site.node_name);
        }
    }

    fn stop_running(&mut self) {
        self.set_execution_state(ExecutionState::Stopped);
        self.batched_events.push(DialogueEvent::DialogueComplete);
    }

    /// Resumes execution.
    pub(crate) fn continue_(
        &mut self,
        function_caller: &mut FunctionCaller,
    ) -> Result<Vec<DialogueEvent>> {
        self.assert_can_continue()?;
        self.set_execution_state(ExecutionState::Running);

        while self.execution_state == ExecutionState::Running {
            let node_name = self.state.current_node_name.clone().unwrap_or_bug();
            let instruction = self.get_node_from_name(&node_name)?.instructions
                [self.state.program_counter]
                .clone();
            // An instruction that jumps returns where to continue. Everything else continues with the next one.
            let next = self.run_instruction(&instruction, function_caller)?;

            let Some(node_name) = self.state.current_node_name.clone() else {
                // The instruction stopped the dialogue
                break;
            };
            self.state.program_counter = next.unwrap_or(self.state.program_counter + 1);

            if self.state.program_counter >= self.get_node_from_name(&node_name)?.instructions.len()
            {
                self.return_from_node(&node_name);
                self.stop_running();
                debug!("Run complete.");
            }
        }
        Ok(core::mem::take(&mut self.batched_events))
    }

    pub(crate) fn parse_markup(&mut self, line: &str) -> crate::markup::Result<ParsedMarkup> {
        self.line_parser.parse_markup(line)
    }

    /// Runs a series of tests to see if the [`VirtualMachine`] is in a state where [`VirtualMachine::continue_`] can be called.
    pub(crate) fn assert_can_continue(&self) -> Result<()> {
        if self.state.current_node_name.is_none() {
            Err(DialogueError::NoNodeSelectedOnContinue)
        } else if self.execution_state == ExecutionState::WaitingOnOptionSelection {
            Err(DialogueError::ContinueOnOptionSelectionError)
        } else {
            // ## Implementation note:
            // The other checks the original did are not needed because our relevant handlers cannot be `None` per our API.
            Ok(())
        }
    }

    pub(crate) fn unload_programs(&mut self) {
        self.program = None
    }

    /// `None` means that no option was selected. The dialogue then continues after the options.
    pub(crate) fn set_selected_option(
        &mut self,
        selected_option_id: Option<OptionId>,
    ) -> Result<()> {
        if self.execution_state != ExecutionState::WaitingOnOptionSelection {
            return Err(DialogueError::UnexpectedOptionSelectionError);
        }

        if let Some(selected_option_id) = selected_option_id {
            let Some((_, destination)) = self.state.current_options.get(selected_option_id.0)
            else {
                return Err(DialogueError::InvalidOptionIdError {
                    selected_option_id,
                    max_id: self.state.current_options.len().saturating_sub(1),
                });
            };
            // We now know what number option was selected; push the
            // corresponding instruction number to the stack, followed by 'an option was selected'.
            let destination = *destination;
            self.state.push(destination);
            self.state.push(true);
        } else {
            self.state.push(false);
        }

        // We no longer need the accumulated list of options; clear it
        // so that it's ready for the next one
        self.state.current_options.clear();

        // We're no longer in the WaitingForOptions state; we are now waiting for our game to let us continue
        self.set_execution_state(ExecutionState::WaitingForContinue);
        Ok(())
    }

    pub(crate) fn set_selected_option_by_line_id(
        &mut self,
        selected_line_id: LineId,
    ) -> Result<OptionId> {
        if self.execution_state != ExecutionState::WaitingOnOptionSelection {
            return Err(DialogueError::UnexpectedOptionSelectionError);
        }

        if let Some((selected_option, _)) = self
            .state
            .current_options
            .iter()
            .find(|(o, _)| o.line.id == selected_line_id)
        {
            let selected_option_id = selected_option.id;
            self.set_selected_option(Some(selected_option_id))
                .map(|_| selected_option_id)
        } else {
            let line_ids = self
                .state
                .current_options
                .iter()
                .map(|(o, _)| o.line.id.clone())
                .collect();
            Err(DialogueError::InvalidLineIdError {
                selected_line_id,
                line_ids,
            })
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.execution_state != ExecutionState::Stopped
    }

    pub(crate) fn is_waiting_for_option_selection(&self) -> bool {
        self.execution_state == ExecutionState::WaitingOnOptionSelection
    }

    /// Runs a single instruction. Returns the instruction to continue with if it is not simply the next one.
    fn run_instruction(
        &mut self,
        instruction: &Instruction,
        function_caller: &mut FunctionCaller,
    ) -> Result<Option<usize>> {
        let Some(instruction_type) = instruction.instruction_type.as_ref() else {
            panic!(
                "The Yarn script provided was compiled using an older compiler. \
                Please recompile it using Yarn Spinner 3."
            )
        };
        match instruction_type {
            InstructionType::JumpTo(jump) => return Ok(Some(jump.destination as usize)),
            InstructionType::PeekAndJump(_) => {
                // Jumps to the instruction whose number is on the stack.
                return Ok(Some(self.state.peek()));
            }
            InstructionType::RunLine(run_line) => {
                // Looks up a string from the string table and passes it to the client as a line
                let string_id: LineId = run_line.line_id.clone().into();
                let substitutions = self.pop_substitutions(run_line.substitution_count as usize);
                let line = self.prepare_line(string_id, &substitutions)?;

                self.batched_events.push(DialogueEvent::Line(line));

                // Implementation note:
                // The original awaits the line handler here. We wait until the user calls `continue_` instead.
                self.set_execution_state(ExecutionState::WaitingForContinue);
            }
            InstructionType::RunCommand(command) => {
                // Passes a string to the client as a custom command
                let command_text = self
                    .pop_substitutions(command.substitution_count as usize)
                    .into_iter()
                    .enumerate()
                    .fold(
                        command.command_text.clone(),
                        |command_text, (i, substitution)| {
                            command_text.replace(&format!("{{{i}}}"), &substitution)
                        },
                    );
                let command = Command::parse(command_text);

                self.batched_events.push(DialogueEvent::Command(command));
                self.set_execution_state(ExecutionState::WaitingForContinue);
            }
            InstructionType::AddOption(option) => {
                // Add an option to the current state
                let string_id: LineId = option.line_id.clone().into();
                let substitutions = self.pop_substitutions(option.substitution_count as usize);
                let line = self.prepare_line(string_id, &substitutions)?;

                // Indicates whether the VM believes that the
                // option should be shown to the user, based on any
                // conditions that were attached to the option.
                let line_condition_passed = if option.has_condition {
                    self.state.pop()
                } else {
                    true
                };

                // ## Implementation note:
                // The original calculates the ID in the `ShowOptions` instruction.
                let id = OptionId(self.state.current_options.len());
                self.state.current_options.push((
                    DialogueOption {
                        line,
                        id,
                        is_available: line_condition_passed,
                    },
                    option.destination as usize,
                ));
            }
            InstructionType::ShowOptions(_) => {
                // If we have no options to show, immediately stop.
                if self.state.current_options.is_empty() {
                    self.stop_running();
                    return Ok(None);
                }

                // We can't continue until our client tell us which option to pick
                self.set_execution_state(ExecutionState::WaitingOnOptionSelection);

                // Pass the options set to the client. The selection is reported with `set_selected_option`.
                let current_options = self
                    .state
                    .current_options
                    .iter()
                    .map(|(option, _)| option.clone())
                    .collect();
                self.batched_events
                    .push(DialogueEvent::Options(current_options));
            }
            InstructionType::PushString(push) => self.state.push(push.value.clone()),
            InstructionType::PushFloat(push) => self.state.push(push.value),
            InstructionType::PushBool(push) => self.state.push(push.value),
            InstructionType::JumpIfFalse(jump) => {
                // Jumps if the value on the top of the stack evaluates to the boolean value 'false'.
                let is_top_value_true: bool = self.state.peek();
                if !is_top_value_true {
                    return Ok(Some(jump.destination as usize));
                }
            }
            InstructionType::Pop(_) => {
                self.state.pop_value();
            }
            InstructionType::CallFunc(call) => {
                // Call a function, whose parameters are expected to be on the stack. Pushes the function's return value.
                let context = context!(self);
                let result = call_function(
                    &call.function_name,
                    &mut self.state.stack,
                    &context,
                    function_caller,
                )?;
                self.state.push(result);
            }
            InstructionType::PushVariable(push) => {
                // Get the contents of a variable, push that onto the stack.
                let variable_name = &push.variable_name;
                let loaded_value = match self.variable_storage.get(variable_name) {
                    Ok(value) => value,
                    Err(VariableStorageError::VariableNotFound { .. }) => {
                        let initial_value = self
                            .program
                            .as_ref()
                            .and_then(|program| program.initial_values.get(variable_name));
                        if let Some(initial_value) = initial_value {
                            // We don't have a value for this. The initial value is found in the program.
                            let initial_value = YarnValue::from(initial_value.clone());
                            self.variable_storage
                                .set(variable_name.clone(), initial_value.clone())?;
                            initial_value
                        } else {
                            // Not stored at all, so it must be a smart variable.
                            evaluate_smart_variable(variable_name, &self.context(), function_caller)?
                                .unwrap_or_else(|| panic!("The loaded program does not contain an initial value for the variable {variable_name}"))
                        }
                    }
                    Err(e) => return Err(e.into()),
                };
                self.state.push(loaded_value);
            }
            InstructionType::StoreVariable(store) => {
                // Store the top value on the stack in a variable.
                let top_value = self.state.peek_value().clone();
                self.variable_storage
                    .set(store.variable_name.clone(), top_value.into())?;
            }
            InstructionType::Stop(_) => {
                // Immediately stop execution, and report that fact.
                self.return_from_all_nodes();
                self.stop_running();
                debug!("Stopped");
                return Ok(None);
            }
            InstructionType::RunNode(run) => {
                return self.execute_jump_to_node(Some(run.node_name.clone()), false);
            }
            InstructionType::PeekAndRunNode(_) => return self.execute_jump_to_node(None, false),
            InstructionType::DetourToNode(detour) => {
                return self.execute_jump_to_node(Some(detour.node_name.clone()), true);
            }
            InstructionType::PeekAndDetourToNode(_) => {
                return self.execute_jump_to_node(None, true);
            }
            InstructionType::Return(_) => {
                if let Some(current) = self.state.current_node_name.clone() {
                    self.return_from_node(&current);
                }
                let Some(return_site) = self.state.call_stack.pop() else {
                    // We've reached the end of a node that nothing detoured into, so we're done.
                    self.stop_running();
                    return Ok(None);
                };
                self.set_node_with_state(return_site.node_name, false)?;
                return Ok(Some(return_site.instruction + 1));
            }
            InstructionType::AddSaliencyCandidate(candidate) => {
                let condition: bool = self.state.pop();
                self.saliency_candidates.push(ContentSaliencyOption {
                    complexity_score: candidate.complexity_score,
                    passing_condition_value_count: condition as usize,
                    failing_condition_value_count: !condition as usize,
                    destination: candidate.destination as usize,
                    ..ContentSaliencyOption::new(
                        candidate.content_id.clone(),
                        ContentSaliencyContentType::Line,
                    )
                });
            }
            InstructionType::AddSaliencyCandidateFromNode(candidate) => {
                let context = self.context();
                let node = self.get_node_from_name(&candidate.node_name)?;
                let mut option = node_saliency_option(node, &context, function_caller)?;
                option.destination = candidate.destination as usize;
                self.saliency_candidates.push(option);
            }
            InstructionType::SelectSaliencyCandidate(_) => {
                let candidates = core::mem::take(&mut self.saliency_candidates);
                let selection = self
                    .saliency_strategy
                    .query_best_content(&candidates, self.variable_storage.as_ref());
                if let Some(selection) = selection {
                    // The strategy may hand back a modified copy, so the destination is taken from our own candidate.
                    let Some(candidate) = candidates
                        .iter()
                        .find(|c| c.content_id == selection.content_id)
                    else {
                        return Err(DialogueError::InvalidSaliencySelection {
                            selected: selection.content_id,
                            available: candidates.into_iter().map(|c| c.content_id).collect(),
                        });
                    };
                    self.saliency_strategy
                        .content_was_selected(candidate, self.variable_storage.as_mut());
                    self.state.push(candidate.destination);
                    self.state.push(true);
                } else {
                    self.state.push(false);
                }
            }
        }
        Ok(None)
    }

    fn execute_jump_to_node(
        &mut self,
        node_name: Option<String>,
        is_detour: bool,
    ) -> Result<Option<usize>> {
        if is_detour {
            // Remember where to continue when the node returns
            self.state.call_stack.push(CallSite {
                node_name: self.state.current_node_name.clone().unwrap_or_bug(),
                instruction: self.state.program_counter,
            });
        } else {
            // We are jumping straight to another node. Unwind the current call stack and
            // issue a 'node ended' event for every node that we were in.
            self.return_from_all_nodes();
        }
        // If no name was given, the name of the node is on the top of the stack.
        let node_name = node_name.unwrap_or_else(|| self.state.peek_value().clone().into());
        self.set_node_with_state(node_name, !is_detour)?;
        Ok(Some(0))
    }

    fn context(&self) -> EvaluationContext<'_> {
        context!(self)
    }

    /// The value of a variable that is kept in the storage, falling back to the initial value of the program.
    fn get_stored_or_initial_value(&self, variable_name: &str) -> Option<YarnValue> {
        self.variable_storage.get(variable_name).ok().or_else(|| {
            let initial_value = self.program.as_ref()?.initial_values.get(variable_name)?;
            Some(initial_value.clone().into())
        })
    }

    /// Returns the current value of a smart variable, or `None` if `name` is not a smart variable.
    pub(crate) fn get_smart_variable(
        &self,
        name: &str,
        function_caller: &mut FunctionCaller,
    ) -> Result<Option<YarnValue>> {
        if self.program.is_none() {
            return Err(DialogueError::NoProgramLoaded);
        }
        evaluate_smart_variable(name, &self.context(), function_caller)
    }

    /// See [`Dialogue::get_saliency_options_for_node_group`].
    pub(crate) fn get_saliency_options_for_node_group(
        &self,
        node_group: &str,
        function_caller: &mut FunctionCaller,
    ) -> Result<Vec<ContentSaliencyOption>> {
        saliency_options_for_node_group(
            self.get_node_from_name(node_group)?,
            &self.context(),
            function_caller,
        )
    }

    /// See [`Dialogue::has_any_content`].
    pub(crate) fn has_any_content(
        &self,
        node_group: &str,
        function_caller: &mut FunctionCaller,
    ) -> Result<bool> {
        if self.program.is_none() {
            return Ok(false);
        }
        has_any_content(node_group, &self.context(), function_caller)
    }

    fn prepare_line(&mut self, string_id: LineId, substitutions: &[String]) -> Result<Line> {
        let line_text = self.text_provider.get_text(&string_id).ok_or_else(|| {
            DialogueError::LineProviderError {
                id: string_id.clone(),
                language_code: self.language_code.clone(),
            }
        })?;
        let substituted_text = expand_substitutions(&line_text, substitutions);
        let markup = self
            .parse_markup(&substituted_text)
            .map_err(DialogueError::MarkupParseError)?;
        let line = Line {
            id: string_id,
            text: markup.text,
            attributes: markup.attributes,
        };
        Ok(line)
    }

    fn pop_substitutions(&mut self, count: usize) -> Vec<String> {
        let mut values: Vec<_> = (0..count).map(|_| self.state.pop_value().into()).collect();
        values.reverse();
        values
    }
}

/// Everything that is needed to evaluate expressions without changing the state of the VM.
struct EvaluationContext<'a> {
    program: &'a Program,
    library: &'a Library,
    variable_storage: &'a dyn VariableStorage,
    saliency_strategy: &'a dyn ContentSaliencyStrategy,
}

/// Calls a function whose parameter count and parameters are on the stack.
fn call_function(
    function_name: &str,
    stack: &mut Vec<InternalValue>,
    context: &EvaluationContext,
    function_caller: &mut FunctionCaller,
) -> Result<InternalValue> {
    let mut pop = || {
        stack
            .pop()
            .unwrap_or_else(|| panic!("Tried to pop value, but the stack was empty."))
    };
    // The compiler places the number of parameters that were actually passed at the top of the stack.
    let parameter_count: usize = pop()
        .try_into()
        .unwrap_or_else(|e| panic!("Failed to read parameter count of {function_name}: {e:?}"));
    // Get the parameters, which were pushed in reverse
    let mut parameters: Vec<_> = (0..parameter_count).map(|_| pop().raw_value).collect();
    parameters.reverse();

    if let Some(function) = context.library.get(function_name) {
        let return_value = function_caller(function, parameters);
        // ## Implementation note:
        // Every function MUST return a valid typed value, so we skip the original's check for `void`.
        let r#type = Type::try_from(function.return_type()).unwrap_or_else(|e| {
            panic!("Failed to get Yarn type for return type id of function {function_name}: {e:?}")
        });
        Ok(InternalValue {
            raw_value: return_value,
            r#type,
        })
    } else if function_name == HAS_ANY_CONTENT_FUNCTION {
        let node_group = parameters
            .into_iter()
            .next()
            .map(String::from)
            .unwrap_or_default();
        Ok(has_any_content(&node_group, context, function_caller)?.into())
    } else {
        Err(DialogueError::FunctionNotFound {
            function_name: function_name.to_owned(),
            library: context.library.clone(),
        })
    }
}

/// Runs the node that computes a smart variable on a separate stack and returns the result.
/// Returns `None` if `name` is not a smart variable.
fn evaluate_smart_variable(
    name: &str,
    context: &EvaluationContext,
    function_caller: &mut FunctionCaller,
) -> Result<Option<YarnValue>> {
    if context.program.variable_kind(name) != VariableKind::Smart {
        return Ok(None);
    }
    let node = &context.program.nodes[name];
    let error = |message: String| DialogueError::SmartVariableError {
        name: name.to_owned(),
        message,
    };
    let mut stack: Vec<InternalValue> = Vec::new();
    let mut program_counter = 0;
    while let Some(instruction) = node.instructions.get(program_counter) {
        program_counter += 1;
        match instruction.instruction_type.as_ref() {
            Some(InstructionType::PushString(push)) => stack.push(push.value.clone().into()),
            Some(InstructionType::PushFloat(push)) => stack.push(push.value.into()),
            Some(InstructionType::PushBool(push)) => stack.push(push.value.into()),
            Some(InstructionType::Pop(_)) => {
                stack.pop();
            }
            Some(InstructionType::CallFunc(call)) => {
                let result =
                    call_function(&call.function_name, &mut stack, context, function_caller)?;
                stack.push(result);
            }
            Some(InstructionType::PushVariable(push)) => {
                let variable_name = &push.variable_name;
                let value = match context.variable_storage.get(variable_name) {
                    Ok(value) => value,
                    Err(_) => match context.program.initial_values.get(variable_name) {
                        Some(initial_value) => initial_value.clone().into(),
                        None => evaluate_smart_variable(variable_name, context, function_caller)?
                            .ok_or_else(|| {
                            error(format!("Failed to fetch any value for {variable_name}"))
                        })?,
                    },
                };
                stack.push(value.into());
            }
            Some(InstructionType::JumpIfFalse(jump)) => {
                let top = stack
                    .last()
                    .cloned()
                    .ok_or_else(|| error("the stack was empty".to_owned()))?;
                if !bool::try_from(top).map_err(|e| error(format!("{e}")))? {
                    program_counter = jump.destination as usize;
                }
            }
            Some(InstructionType::Stop(_)) => break,
            other => return Err(error(format!("Invalid instruction {other:?}"))),
        }
    }
    let result = stack
        .pop()
        .ok_or_else(|| error("stack did not contain a value after evaluation".to_owned()))?;
    if !stack.is_empty() {
        return Err(error(format!(
            "stack had {} dangling value(s)",
            stack.len()
        )));
    }
    Ok(Some(result.raw_value))
}

/// Evaluates the `when:` conditions of a member of a node group.
fn node_saliency_option(
    node: &Node,
    context: &EvaluationContext,
    function_caller: &mut FunctionCaller,
) -> Result<ContentSaliencyOption> {
    let mut option = ContentSaliencyOption {
        complexity_score: node.content_saliency_condition_complexity_score(),
        ..ContentSaliencyOption::new(node.name.clone(), ContentSaliencyContentType::Node)
    };
    for variable in node.content_saliency_condition_variables() {
        let passed = match evaluate_smart_variable(&variable, context, function_caller)? {
            Some(YarnValue::Boolean(passed)) => passed,
            _ => {
                return Err(DialogueError::SmartVariableError {
                    name: variable,
                    message: "Failed to evaluate saliency condition: not a boolean smart variable of the program".to_owned(),
                });
            }
        };
        if passed {
            option.passing_condition_value_count += 1;
        } else {
            option.failing_condition_value_count += 1;
        }
    }
    Ok(option)
}

fn saliency_options_for_node_group(
    node: &Node,
    context: &EvaluationContext,
    function_caller: &mut FunctionCaller,
) -> Result<Vec<ContentSaliencyOption>> {
    if !node.is_node_group_hub() {
        // Not a node group, but it's a valid node, so return the node itself as the only content
        return Ok(vec![ContentSaliencyOption {
            passing_condition_value_count: 1,
            ..ContentSaliencyOption::new(node.name.clone(), ContentSaliencyContentType::Node)
        }]);
    }
    context
        .program
        .nodes
        .values()
        .filter(|member| member.node_group().as_deref() == Some(node.name.as_str()))
        .map(|member| node_saliency_option(member, context, function_caller))
        .collect()
}

fn has_any_content(
    node_group: &str,
    context: &EvaluationContext,
    function_caller: &mut FunctionCaller,
) -> Result<bool> {
    let Some(node) = context.program.nodes.get(node_group) else {
        return Ok(false);
    };
    let options = saliency_options_for_node_group(node, context, function_caller)?;
    Ok(context
        .saliency_strategy
        .query_best_content(&options, context.variable_storage)
        .is_some())
}

/// Replaces all substitution markers in a text with the given substitution list.
///
/// This method replaces substitution markers
/// (for example, `{0}`) with the corresponding entry in `substitutions`.
/// If `test` contains a substitution marker whose
/// index is not present in `substitutions`, it is
/// ignored.
#[must_use]
fn expand_substitutions(text: &str, substitutions: &[String]) -> String {
    substitutions
        .iter()
        .enumerate()
        .fold(text.to_owned(), |text, (i, substitution)| {
            text.replace(&format!("{{{i}}}",), substitution)
        })
}
