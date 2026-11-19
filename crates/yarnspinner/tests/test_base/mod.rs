//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/da39c7195107d8211f21c263e4084f773b84eaff/YarnSpinner.Tests/TestBase.cs>
//!
//! ## Implementations notes
//! Methods used for upgrade testing were not ported since we don't offer any upgrade functionality.
//! This includes `DirectorySources`.
//!
//! Methods for tests we didn't port are also naturally not included. This includes `FormatParseTreeAsText`

// Allowed because this is a common file and not all tests use the methods provided.
// Everything is actually used, but the checker doesn't recognize it because all integration test files
// are separately compiled as their own crate.
#![allow(dead_code)]

use crate::prelude::*;
#[cfg(feature = "bevy")]
use bevy::prelude::World;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::result::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use yarnspinner::compiler::*;
use yarnspinner::core::*;
use yarnspinner::runtime::*;

mod extensions;
mod logger;
mod paths;
mod test_plan;
mod text_provider;
use logger::*;
pub use text_provider::SharedTextProvider;
use yarnspinner::log::{self, LevelFilter, SetLoggerError};

pub mod prelude {
    #[allow(unused_imports)] // False positive
    pub use crate::test_base::{extensions::*, paths::*, test_plan::*, *};
}

pub fn init_logger(runtime_errors_cause_failure: Arc<AtomicBool>) -> Result<(), SetLoggerError> {
    let logger = TestLogger::new(runtime_errors_cause_failure);
    log::set_boxed_logger(Box::new(logger)).map(|()| log::set_max_level(LevelFilter::Info))
}

#[derive(Debug)]
pub struct TestBase {
    pub dialogue: Dialogue,
    pub test_plan: Option<TestPlan>,
    pub string_table: SharedTextProvider,
    pub variable_storage: MemoryVariableStorage,
    /// The string table of the compilation, for looking up the metadata of lines
    pub string_infos: HashMap<LineId, StringInfo>,
    runtime_errors_cause_failure: Arc<AtomicBool>,
}

impl Default for TestBase {
    fn default() -> Self {
        let runtime_errors_cause_failure = Arc::new(AtomicBool::new(true));
        if let Err(_e) = init_logger(runtime_errors_cause_failure.clone()) {
            // We've set the logger twice, that's alright for the tests.
        }
        let variable_storage = MemoryVariableStorage::new();
        let string_table = SharedTextProvider::new(StringTableTextProvider::new());

        let mut dialogue = Dialogue::new(
            Box::new(variable_storage.clone()),
            Box::new(string_table.clone()),
        );
        dialogue
            .library_mut()
            .add_function("assert", |value: YarnValue| {
                let is_truthy: bool = value.try_into().unwrap();
                assert!(is_truthy);
                true
            });
        // Like the original test suite, we don't use the random default strategy
        dialogue.set_content_saliency_strategy(Box::new(BestLeastRecentlyViewedSaliencyStrategy));

        Self {
            dialogue,
            runtime_errors_cause_failure,
            variable_storage,
            string_table,
            string_infos: Default::default(),
            test_plan: Default::default(),
        }
    }
}

impl TestBase {
    #[must_use]
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the current test plan to one loaded from a given path.
    #[must_use]
    pub fn read_test_plan(self, path: impl AsRef<Path>) -> Self {
        self.with_test_plan(TestPlan::read(path))
    }

    #[must_use]
    pub fn with_test_plan(mut self, test_plan: TestPlan) -> Self {
        self.test_plan.replace(test_plan);
        self
    }

    #[must_use]
    pub fn with_runtime_errors_do_not_cause_failure(self) -> Self {
        self.runtime_errors_cause_failure
            .store(false, Ordering::Relaxed);
        self
    }

    #[must_use]
    pub fn with_compilation(self, compilation: Compilation) -> Self {
        let string_table = compilation.string_table;
        self.with_program(compilation.program.unwrap())
            .with_string_table(string_table)
    }

    #[must_use]
    pub fn extend_library(mut self, extend_fn: impl Fn(&mut Library)) -> Self {
        let library = self.dialogue.library_mut();
        extend_fn(library);
        self
    }

    #[must_use]
    pub fn with_program(mut self, program: Program) -> Self {
        self.dialogue.add_program(program);
        self
    }

    #[must_use]
    pub fn with_string_table(mut self, string_table: HashMap<LineId, StringInfo>) -> Self {
        self.string_infos.clone_from(&string_table);
        let string_table: HashMap<_, _> = string_table
            .into_iter()
            .map(|(id, info)| (id, info.text))
            .collect();
        let mut string_table_provider = StringTableTextProvider::new();
        string_table_provider.extend_base_language(string_table.clone());
        string_table_provider.extend_translation("en-US", string_table);
        self.string_table.replace(string_table_provider);
        self.dialogue.set_language_code(Language::from("en-US"));
        self
    }

    /// Executes the named node, and checks any assertions made during
    /// execution. Fails the test if an assertion made in Yarn fails.
    /// Runs the test plan: every run starts the dialogue, lets it run to its end
    /// and compares what happened with the expectations of the run.
    pub fn run_standard_testcase(&mut self) -> &mut Self {
        let test_plan = self.test_plan.clone().unwrap_or_else(TestPlan::new);
        for run in &test_plan.runs {
            self.evaluate_run(run, self.test_plan.is_some());
        }
        self
    }

    fn evaluate_run(&mut self, run: &Run, check_expectations: bool) {
        // Apply the actions of the run before starting it
        let mut start_node = run.start_node.clone();
        let mut selections = std::collections::VecDeque::new();
        for step in run.steps.iter().filter(|step| step.is_action()) {
            match step {
                Step::Select(selection) => selections.push_back(*selection),
                Step::SetBool(variable, value) => self.set_variable(variable, (*value).into()),
                Step::SetNumber(variable, value) => self.set_variable(variable, (*value).into()),
                Step::SetSaliency(mode) => {
                    let strategy: Box<dyn ContentSaliencyStrategy> = match mode.as_str() {
                        "first" => Box::new(FirstSaliencyStrategy),
                        "best" => Box::new(BestSaliencyStrategy),
                        "best_least_recently_seen" => {
                            Box::new(BestLeastRecentlyViewedSaliencyStrategy)
                        }
                        other => panic!("Unknown saliency strategy '{other}'"),
                    };
                    self.dialogue.set_content_saliency_strategy(strategy);
                }
                Step::JumpToNode(node) => start_node.clone_from(node),
                _ => unreachable!(),
            }
        }

        // Run the dialogue and remember what happened
        let mut moments = std::collections::VecDeque::new();
        self.dialogue.set_node(start_node).unwrap();
        #[cfg(feature = "bevy")]
        let mut world = World::default();
        while self.dialogue.can_continue() {
            #[cfg(feature = "bevy")]
            let events = self.dialogue.continue_with_world(&mut world);
            #[cfg(not(feature = "bevy"))]
            let events = self.dialogue.continue_();
            let events =
                events.unwrap_or_else(|e| panic!("Encountered error while running dialogue: {e}"));

            for event in events {
                match event {
                    DialogueEvent::Line(line) => {
                        println!("Line: {}", line.text);
                        moments.push_back(Step::ExpectLine {
                            hashtags: self.metadata(&line.id),
                            text: Some(line.text),
                        });
                    }
                    DialogueEvent::Options(options) => {
                        println!("Options:");
                        for option in options {
                            println!(" - {} (available: {})", option.line.text, option.is_available);
                            moments.push_back(Step::ExpectOption {
                                hashtags: self.metadata(&option.line.id),
                                text: option.line.text,
                                enabled: option.is_available,
                            });
                        }
                        // Without a test plan, we always pick the first option
                        let selection = match selections.pop_front() {
                            Some(selection) => selection,
                            None if !check_expectations => Some(0),
                            None => panic!("Received options, but the test plan has no selection left"),
                        };
                        println!("[Selecting option {selection:?}]");
                        match selection {
                            Some(selection) => self
                                .dialogue
                                .set_selected_option(OptionId(selection))
                                .unwrap(),
                            None => self.dialogue.set_no_option_selected().unwrap(),
                        };
                    }
                    DialogueEvent::Command(command) => {
                        println!("Command: {}", command.raw);
                        // We don't need to get the composed string for a command because it's been
                        // done for us in the virtual machine: commands are not localised.
                        moments.push_back(Step::ExpectCommand(command.raw));
                    }
                    DialogueEvent::NodeComplete(_)
                    | DialogueEvent::NodeStart(_)
                    | DialogueEvent::LineHints(_)
                    | DialogueEvent::DialogueComplete => {}
                }
            }
        }
        if !check_expectations {
            return;
        }

        // Compare what happened with what was expected
        for step in run.steps.iter().filter(|step| !step.is_action()) {
            if *step == Step::ExpectStop {
                break;
            }
            let moment = moments
                .pop_front()
                .unwrap_or_else(|| panic!("Expected {step:?}, but the dialogue was already over"));
            let contains_all = |actual: &[String], expected: &[String]| {
                for hashtag in expected {
                    assert!(actual.contains(hashtag), "Expected {step:?}, got {moment:?}");
                }
            };
            match (step, &moment) {
                (
                    Step::ExpectLine { text, hashtags },
                    Step::ExpectLine {
                        text: actual_text,
                        hashtags: actual_hashtags,
                    },
                ) => {
                    if text.is_some() {
                        assert_eq!(text, actual_text);
                    }
                    contains_all(actual_hashtags, hashtags);
                }
                (
                    Step::ExpectOption {
                        text,
                        hashtags,
                        enabled,
                    },
                    Step::ExpectOption {
                        text: actual_text,
                        hashtags: actual_hashtags,
                        enabled: actual_enabled,
                    },
                ) => {
                    assert_eq!(text, actual_text);
                    assert_eq!(enabled, actual_enabled, "Availability of option {text}");
                    contains_all(actual_hashtags, hashtags);
                }
                (Step::ExpectCommand(text), Step::ExpectCommand(actual_text)) => {
                    assert_eq!(text, actual_text);
                }
                _ => panic!("Expected {step:?}, got {moment:?}"),
            }
        }
        assert!(
            moments.is_empty(),
            "The dialogue produced more content than expected: {moments:?}"
        );
    }

    fn set_variable(&mut self, variable: &str, value: YarnValue) {
        println!("INFO: Variable {variable} set to {value}");
        self.dialogue
            .variable_storage_mut()
            .set(variable.to_owned(), value)
            .unwrap();
    }

    fn metadata(&self, line_id: &LineId) -> Vec<String> {
        self.string_infos
            .get(line_id)
            .map(|info| info.metadata.clone())
            .unwrap_or_default()
    }

    pub fn file_sources(subdir: impl AsRef<Path>) -> impl Iterator<Item = PathBuf> {
        let subdir: PathBuf = PathBuf::from(subdir.as_ref());
        let path = test_data_path().join(&subdir);
        let allowed_extensions = ["node", "yarn"].map(OsStr::new);
        fs::read_dir(&path)
            .unwrap_or_else(|e| panic!("Failed to read directory {}: {e}", path.display()))
            .filter_map(|entry| {
                entry
                    .map_err(|e| {
                        println!("Warning: failed to read a directory entry: {e:?}");
                        e
                    })
                    .ok()
            })
            .filter(move |entry| {
                entry
                    .path()
                    .extension()
                    .map(|ext| allowed_extensions.contains(&ext))
                    .unwrap_or_default()
            })
            // don't include ".upgraded.yarn" (used in upgrader tests)
            .filter(|entry| !entry.path().ends_with(".upgraded.yarn"))
            .map(move |entry| subdir.join(entry.file_name()))
    }
}
