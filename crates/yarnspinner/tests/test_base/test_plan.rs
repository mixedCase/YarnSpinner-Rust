//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Tests/TestPlan.cs>
//! and the grammar <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Tests/TestPlan/YarnSpinnerTestPlan.g4>
//!
//! ## Implementation notes
//!
//! The original parses test plans with an ANTLR grammar. The format is simple enough for a hand-written parser.

use std::fs;
use std::path::Path;

/// A test plan: a list of runs, each of which starts the dialogue and describes what is expected to happen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TestPlan {
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub start_node: String,
    pub steps: Vec<Step>,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            start_node: "Start".to_owned(),
            steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Expect to see a line. `None` means any text.
    ExpectLine {
        text: Option<String>,
        hashtags: Vec<String>,
    },
    ExpectOption {
        text: String,
        hashtags: Vec<String>,
        enabled: bool,
    },
    ExpectCommand(String),
    /// Expect the dialogue to stop here. This is optional; a 'stop' at the end of a run is assumed.
    ExpectStop,
    /// Select the option with this zero-based index the next time options are shown.
    /// `None` selects no option, which makes the dialogue continue after the options.
    Select(Option<usize>),
    /// Set a variable before the run starts.
    SetBool(String, bool),
    SetNumber(String, f32),
    /// Use this saliency strategy: `first`, `best` or `best_least_recently_seen`.
    SetSaliency(String),
    /// Start the run at this node.
    JumpToNode(String),
}

impl Step {
    /// Actions are applied before the run starts, expectations are compared with what the run produced.
    pub fn is_action(&self) -> bool {
        matches!(
            self,
            Step::Select(_)
                | Step::SetBool(..)
                | Step::SetNumber(..)
                | Step::SetSaliency(_)
                | Step::JumpToNode(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// A keyword like `line:` (without the colon) or a bare word like `stop`, `true`, `42`, `$variable`
    Word(String),
    /// Text in backticks
    Text(String),
    Hashtag(String),
    Separator,
    Disabled,
    Any,
    Equals,
}

fn tokenize(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let take_while = |start: usize, predicate: &dyn Fn(char) -> bool| -> usize {
        let mut end = start;
        while end < chars.len() && predicate(chars[end]) {
            end += 1;
        }
        end
    };
    while i < chars.len() {
        let rest: String = chars[i..chars.len().min(i + 10)].iter().collect();
        match chars[i] {
            c if c.is_whitespace() => i += 1,
            '/' if rest.starts_with("//") => i = take_while(i, &|c| c != '\n'),
            '`' => {
                let end = take_while(i + 1, &|c| c != '`');
                assert!(end < chars.len(), "Unterminated text in test plan");
                tokens.push(Token::Text(chars[i + 1..end].iter().collect()));
                i = end + 1;
            }
            '#' => {
                let end = take_while(i + 1, &|c| !c.is_whitespace() && c != '#');
                tokens.push(Token::Hashtag(chars[i + 1..end].iter().collect()));
                i = end;
            }
            '-' if rest.starts_with("---") => {
                tokens.push(Token::Separator);
                i += 3;
            }
            '[' if rest.starts_with("[disabled]") => {
                tokens.push(Token::Disabled);
                i += "[disabled]".len();
            }
            '*' => {
                tokens.push(Token::Any);
                i += 1;
            }
            '=' => {
                tokens.push(Token::Equals);
                i += 1;
            }
            _ => {
                let end = take_while(i, &|c| c.is_alphanumeric() || c == '_' || c == '$');
                assert!(end > i, "Syntax error in test plan at: {rest}");
                tokens.push(Token::Word(chars[i..end].iter().collect()));
                // The colon after a keyword carries no information
                i = if chars.get(end) == Some(&':') { end + 1 } else { end };
            }
        }
    }
    tokens
}

impl TestPlan {
    pub fn new() -> Self {
        Self {
            runs: vec![Run::default()],
        }
    }

    pub fn read(path: impl AsRef<Path>) -> Self {
        Self::parse(&fs::read_to_string(path).unwrap())
    }

    pub fn parse(source: &str) -> Self {
        let mut tokens = tokenize(source).into_iter().peekable();
        let mut runs = vec![Run::default()];
        let word = |token: Option<Token>| match token {
            Some(Token::Word(word)) => word,
            other => panic!("Syntax error in test plan: expected a word, got {other:?}"),
        };
        let text = |token: Option<Token>| match token {
            // In Rust, booleans are not capitalized when converted to strings.
            // But in C# and hence the upstream test plans, they are.
            Some(Token::Text(text)) => text.replace("True", "true").replace("False", "false"),
            other => panic!("Syntax error in test plan: expected `text`, got {other:?}"),
        };
        while let Some(token) = tokens.next() {
            let hashtags = |tokens: &mut std::iter::Peekable<std::vec::IntoIter<Token>>| {
                let mut hashtags = Vec::new();
                while let Some(Token::Hashtag(hashtag)) = tokens.peek() {
                    hashtags.push(hashtag.clone());
                    tokens.next();
                }
                hashtags
            };
            let run = runs.last_mut().unwrap();
            let step = match token {
                Token::Separator => {
                    // An environment declaration is not a run of its own
                    if !(run.steps.is_empty() && run.start_node == "Start") {
                        runs.push(Run::default());
                    }
                    continue;
                }
                Token::Word(keyword) => match keyword.as_str() {
                    "environment" => {
                        word(tokens.next());
                        continue;
                    }
                    "start" => {
                        run.start_node = word(tokens.next());
                        continue;
                    }
                    "line" => {
                        let text = match tokens.next() {
                            Some(Token::Any) => None,
                            token => Some(text(token)),
                        };
                        Step::ExpectLine {
                            text,
                            hashtags: hashtags(&mut tokens),
                        }
                    }
                    "option" => {
                        let text = text(tokens.next());
                        let hashtags = hashtags(&mut tokens);
                        let enabled = tokens.next_if_eq(&Token::Disabled).is_none();
                        Step::ExpectOption {
                            text,
                            hashtags,
                            enabled,
                        }
                    }
                    "command" => Step::ExpectCommand(text(tokens.next())),
                    "stop" => Step::ExpectStop,
                    // 1-indexed in the test plan, 0-indexed in the code. 0 means 'select nothing'.
                    "select" => {
                        Step::Select(word(tokens.next()).parse::<usize>().unwrap().checked_sub(1))
                    }
                    "set" => {
                        let variable = word(tokens.next());
                        assert_eq!(Some(Token::Equals), tokens.next(), "Expected '=' after {variable}");
                        let value = word(tokens.next());
                        match value.parse::<bool>() {
                            Ok(value) => Step::SetBool(variable, value),
                            Err(_) => Step::SetNumber(variable, value.parse().unwrap()),
                        }
                    }
                    "saliency" => Step::SetSaliency(word(tokens.next())),
                    "node" => Step::JumpToNode(word(tokens.next())),
                    other => panic!("Syntax error in test plan: unknown step '{other}'"),
                },
                other => panic!("Syntax error in test plan: unexpected {other:?}"),
            };
            run.steps.push(step);
        }
        Self { runs }
    }

    fn push(mut self, step: Step) -> Self {
        self.runs.last_mut().unwrap().steps.push(step);
        self
    }

    pub fn expect_line(self, line: impl Into<String>) -> Self {
        self.push(Step::ExpectLine {
            text: Some(line.into()),
            hashtags: Vec::new(),
        })
    }

    pub fn expect_option(self, line: impl Into<String>) -> Self {
        self.push(Step::ExpectOption {
            text: line.into(),
            hashtags: Vec::new(),
            enabled: true,
        })
    }

    pub fn expect_command(self, line: impl Into<String>) -> Self {
        self.push(Step::ExpectCommand(line.into()))
    }

    /// Selects the option with the given zero-based index.
    pub fn then_select(self, selection: usize) -> Self {
        self.push(Step::Select(Some(selection)))
    }

    pub fn expect_stop(self) -> Self {
        self.push(Step::ExpectStop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_test_plan() {
        let plan = TestPlan::parse(
            "environment: unity\n---\nstart: Other // comment\nline: `Hello there` #tag #other\nline: *\noption: `A` [disabled]\noption: `B` #b\nselect: 2\ncommand: `jump up`\n---\nset: $x = true\nset: $y = 3\nsaliency: best\nnode: Next\nstop",
        );
        assert_eq!(2, plan.runs.len());
        assert_eq!("Other", plan.runs[0].start_node);
        assert_eq!(
            vec![
                Step::ExpectLine {
                    text: Some("Hello there".to_owned()),
                    hashtags: vec!["tag".to_owned(), "other".to_owned()]
                },
                Step::ExpectLine {
                    text: None,
                    hashtags: vec![]
                },
                Step::ExpectOption {
                    text: "A".to_owned(),
                    hashtags: vec![],
                    enabled: false
                },
                Step::ExpectOption {
                    text: "B".to_owned(),
                    hashtags: vec!["b".to_owned()],
                    enabled: true
                },
                Step::Select(Some(1)),
                Step::ExpectCommand("jump up".to_owned()),
            ],
            plan.runs[0].steps
        );
        assert_eq!(
            vec![
                Step::SetBool("$x".to_owned(), true),
                Step::SetNumber("$y".to_owned(), 3.0),
                Step::SetSaliency("best".to_owned()),
                Step::JumpToNode("Next".to_owned()),
                Step::ExpectStop,
            ],
            plan.runs[1].steps
        );
    }
}
