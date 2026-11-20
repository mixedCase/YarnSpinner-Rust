//! Tests for the features that Yarn Spinner 3 added, as far as they concern the Rust API.
//! The language behaviour itself is covered by the upstream test plans, see `language_tests::test_sources`.
//!
//! Partially adapted from `DialogueTests.cs`, `SaliencyTests.cs` and `SmartVariableTests.cs` of
//! <https://github.com/YarnSpinnerTool/YarnSpinner/tree/v3.3.1/YarnSpinner.Tests>

use test_base::prelude::*;
use yarnspinner::compiler::*;
use yarnspinner::core::*;
use yarnspinner::runtime::*;

mod test_base;

#[test]
fn test_variadic_functions() {
    let test_base = TestBase::new()
        .extend_library(|library| {
            library
                // A function with a single variadic parameter
                .add_function("variadic_add", |a: Vec<i32>| a.iter().sum::<i32>())
                // A function with a normal parameter, followed by a variadic parameter
                .add_function("variadic_string_add", |s: String, a: Vec<i32>| {
                    format!("{s}{}", a.iter().sum::<i32>())
                });
        })
        .with_test_plan(
            TestPlan::new()
                .expect_line("6")
                .expect_line("s6")
                .expect_line("0")
                .expect_line("s0")
                .expect_line("0")
                .expect_line("0")
                .expect_line("0")
                .expect_line("6")
                .expect_line("s6")
                .expect_stop(),
        );

    let result = Compiler::from_test_source(
        r#"
<<declare $smart_variadic_add = variadic_add(1,2,3)>>
<<declare $smart_variadic_string_add = variadic_string_add("s", 1,2,3)>>

{variadic_add(1,2,3)}
{variadic_string_add("s",1,2,3)}
{variadic_add()}
{variadic_string_add("s")}
{variadic_add($a)} // infer these types to be Number
{variadic_string_add($b)} // infer to be String
{variadic_string_add($c, $d)} // infer to be String, Number
{$smart_variadic_add}
{$smart_variadic_string_add}
"#,
    )
    .extend_library(test_base.dialogue.library().clone())
    .compile()
    .unwrap();

    for (name, expected_type) in [
        ("$a", Type::Number),
        ("$b", Type::String),
        ("$c", Type::String),
        ("$d", Type::Number),
    ] {
        let declaration = result.declarations.iter().find(|d| d.name == name).unwrap();
        assert_eq!(expected_type, declaration.r#type, "{name}");
    }
    let smart_variable = result
        .declarations
        .iter()
        .find(|d| d.name == "$smart_variadic_add")
        .unwrap();
    assert!(smart_variable.is_inline_expansion);

    let mut test_base = test_base.with_compilation(result);

    // Dynamically evaluate the smart variables
    let dialogue = &test_base.dialogue;
    #[cfg(not(feature = "bevy"))]
    {
        assert_eq!(
            Some(YarnValue::Number(6.0)),
            dialogue.get_smart_variable("$smart_variadic_add").unwrap()
        );
        assert_eq!(
            Some(YarnValue::String("s6".to_owned())),
            dialogue
                .get_smart_variable("$smart_variadic_string_add")
                .unwrap()
        );
        assert_eq!(None, dialogue.get_smart_variable("$a").unwrap());
    }
    assert_eq!(
        VariableKind::Smart,
        dialogue.variable_kind("$smart_variadic_add")
    );
    assert_eq!(VariableKind::Stored, dialogue.variable_kind("$a"));
    assert_eq!(VariableKind::Unknown, dialogue.variable_kind("$nope"));

    test_base.run_standard_testcase();
}

#[test]
fn test_smart_variables_cannot_be_set_or_loop() {
    let result = Compiler::from_test_source(
        "<<declare $money = 5>>\n<<declare $rich = $money > 10>>\n<<set $rich = true>>",
    )
    .compile()
    .unwrap_err();
    assert!(result.0.iter().any(|d| {
        d.code.as_deref() == Some("YS0030")
            && d.message
                == "$rich cannot be modified (it's a smart variable and is always equal to $money > 10)"
    }));

    let result =
        Compiler::from_test_source("<<declare $a = $b + 1>>\n<<declare $b = $a + 1>>\n{$a}")
            .compile()
            .unwrap_err();
    assert!(result.0.iter().any(|d| d.code.as_deref() == Some("YS0045")));
}

#[test]
fn test_enums_are_user_defined_types() {
    let result = Compiler::from_test_source(
        r#"
/// What the player likes to eat
<<enum Food>>
    <<case Apple>>
    <<case Orange>> /// A citrus
<<endenum>>
<<enum Door>>
    <<case Open = "open">>
    <<case Closed = "closed">>
<<endenum>>
<<declare $favourite = Food.Orange>>
<<declare $door = .Closed>>
<<if $favourite == .Orange>>
    orange
<<endif>>
"#,
    )
    .compile()
    .unwrap();

    let Some(Type::Enum(food)) = result
        .user_defined_types
        .iter()
        .find(|t| t.name() == "Food")
    else {
        panic!("expected an enum named Food");
    };
    assert_eq!("What the player likes to eat", food.description);
    assert_eq!(Type::Number, *food.raw_type);
    assert_eq!(
        Some(YarnValue::Number(1.0)),
        food.case("Orange").map(|c| c.value.clone())
    );
    assert_eq!("A citrus", food.case("Orange").unwrap().description);

    let declaration = |name: &str| result.declarations.iter().find(|d| d.name == name).unwrap();
    assert_eq!("Food", declaration("$favourite").r#type.name());
    assert_eq!(
        Some(YarnValue::Number(1.0)),
        declaration("$favourite").default_value
    );
    assert_eq!("Door", declaration("$door").r#type.name());
    assert_eq!(
        Some(YarnValue::String("closed".to_owned())),
        declaration("$door").default_value
    );

    TestBase::new()
        .with_test_plan(TestPlan::new().expect_line("orange"))
        .with_compilation(result)
        .run_standard_testcase();

    // Enums of different types can't be compared, and unknown cases are errors
    for source in [
        "<<enum A>>\n<<case X>>\n<<endenum>>\n<<enum B>>\n<<case Y>>\n<<endenum>>\n{A.X == B.Y}",
        "<<enum A>>\n<<case X>>\n<<endenum>>\n{A.Nope}",
        "<<enum A>>\n<<case X = 1>>\n<<case Y = \"y\">>\n<<endenum>>",
        "<<enum A>>\n<<case X = 1>>\n<<case X = 2>>\n<<endenum>>",
    ] {
        assert!(
            Compiler::from_test_source(source).compile().is_err(),
            "{source}"
        );
    }
}

const NODE_GROUP_SOURCE: &str = "title: Start
---
<<declare $friendly = true>>
<<declare $times = 0>>
<<detour Guard>>
Back in start
===
title: Guard
when: $friendly
---
Guard: Hello, friend!
===
title: Guard
subtitle: Angry
when: not $friendly and $times == 0
---
Guard: Halt!
===
title: Guard
when: once
---
Guard: I have not seen you before.
===
title: Plain
---
Plain
===
";

#[test]
fn test_node_groups_can_be_queried() {
    let result = Compiler::from_source(NODE_GROUP_SOURCE).compile().unwrap();
    let test_base = TestBase::new().with_compilation(result);
    let mut dialogue = test_base.dialogue;

    assert!(dialogue.is_node_group("Guard"));
    assert!(!dialogue.is_node_group("Plain"));
    assert!(!dialogue.is_node_group("Guard.Angry"));
    assert!(!dialogue.is_node_group("DoesNotExist"));
    assert_eq!(
        Some("Guard".to_owned()),
        dialogue.get_header_value("Guard.Angry", Node::NODE_GROUP_HEADER)
    );
    assert_eq!(
        Some("Angry".to_owned()),
        dialogue.get_header_value("Guard.Angry", "subtitle")
    );

    #[cfg(not(feature = "bevy"))]
    {
        let options = |dialogue: &Dialogue| {
            let mut options = dialogue
                .get_saliency_options_for_node_group("Guard")
                .unwrap();
            options.sort_by_key(|option| option.complexity_score);
            options
        };
        let before = options(&dialogue);
        assert_eq!(3, before.len());
        // `when: $friendly`, `when: once` and the two-term condition of the angry guard
        assert_eq!(
            vec![1, 1, 2],
            before
                .iter()
                .map(|o| o.complexity_score)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            2,
            before
                .iter()
                .filter(|o| o.failing_condition_value_count == 0)
                .count()
        );
        assert!(
            before
                .iter()
                .all(|o| o.content_type == ContentSaliencyContentType::Node)
        );
        assert_eq!("Guard.Angry", before[2].content_id);
        assert_eq!(1, before[2].failing_condition_value_count);

        dialogue
            .variable_storage_mut()
            .set("$friendly".to_owned(), false.into())
            .unwrap();
        let after = options(&dialogue);
        assert_eq!(0, after[2].failing_condition_value_count);
        assert_eq!(1, after[2].passing_condition_value_count);

        assert!(dialogue.has_any_content("Guard").unwrap());
        assert!(dialogue.has_any_content("Plain").unwrap());
        assert!(!dialogue.has_any_content("DoesNotExist").unwrap());

        // A plain node is its own, always available, content
        let plain = dialogue
            .get_saliency_options_for_node_group("Plain")
            .unwrap();
        assert_eq!(1, plain.len());
        assert_eq!("Plain", plain[0].content_id);
    }
}

/// Collects the texts of all lines and the names of all nodes that were entered and left.
fn run_and_log(dialogue: &mut Dialogue, node: &str) -> Vec<String> {
    #[cfg(feature = "bevy")]
    let mut world = bevy::prelude::World::default();
    let mut log = Vec::new();
    dialogue.set_node(node).unwrap();
    while dialogue.can_continue() {
        #[cfg(feature = "bevy")]
        let events = dialogue.continue_with_world(&mut world).unwrap();
        #[cfg(not(feature = "bevy"))]
        let events = dialogue.continue_().unwrap();
        for event in events {
            match event {
                DialogueEvent::Line(line) => log.push(line.text),
                DialogueEvent::NodeStart(node) => log.push(format!("start {node}")),
                DialogueEvent::NodeComplete(node) => log.push(format!("complete {node}")),
                DialogueEvent::DialogueComplete => log.push("dialogue complete".to_owned()),
                DialogueEvent::Options(_) => dialogue.set_no_option_selected().map(|_| ()).unwrap(),
                _ => {}
            }
        }
    }
    log
}

#[test]
fn test_saliency_strategy_decides_which_node_runs() {
    let result = Compiler::from_source(NODE_GROUP_SOURCE).compile().unwrap();
    let mut dialogue = TestBase::new().with_compilation(result).dialogue;

    // The first strategy takes the first node whose conditions pass, in the order of the program
    dialogue.set_content_saliency_strategy(Box::new(FirstSaliencyStrategy));
    let log = run_and_log(&mut dialogue, "Start");
    let guard_line = |log: &[String]| log.iter().find(|l| l.starts_with("Guard:")).cloned();
    assert!(guard_line(&log).is_some());
    // Detouring into the group enters the hub and the member, and returns through both
    let position = |entry: &str| log.iter().position(|l| l == entry).unwrap();
    assert!(position("start Guard") < position("Back in start"));
    assert!(position("complete Guard") < position("Back in start"));
    assert_eq!(Some("dialogue complete"), log.last().map(String::as_str));

    // The best strategy prefers the most specific passing condition
    dialogue
        .variable_storage_mut()
        .set("$friendly".to_owned(), false.into())
        .unwrap();
    dialogue.set_content_saliency_strategy(Box::new(BestSaliencyStrategy));
    let log = run_and_log(&mut dialogue, "Start");
    assert_eq!(Some("Guard: Halt!".to_owned()), guard_line(&log));

    // A custom strategy can do whatever it wants
    #[derive(Debug, Clone)]
    struct NothingEver;
    impl ContentSaliencyStrategy for NothingEver {
        fn query_best_content(
            &self,
            _content: &[ContentSaliencyOption],
            _variable_storage: &dyn VariableStorage,
        ) -> Option<ContentSaliencyOption> {
            None
        }
        fn content_was_selected(
            &mut self,
            _content: &ContentSaliencyOption,
            _variable_storage: &mut dyn VariableStorage,
        ) {
        }
        fn clone_box(&self) -> Box<dyn ContentSaliencyStrategy> {
            Box::new(self.clone())
        }
    }
    dialogue.set_content_saliency_strategy(Box::new(NothingEver));
    let log = run_and_log(&mut dialogue, "Start");
    assert_eq!(None, guard_line(&log));
    assert!(log.contains(&"Back in start".to_owned()));
}

#[test]
fn test_options_fall_through_when_nothing_is_selected() {
    let result = Compiler::from_test_source(
        "-> Option 1 <<if false>>\n    Not this\n-> Option 2 <<if false>>\n    Nor this\nAfter the options",
    )
    .compile()
    .unwrap();
    let mut dialogue = TestBase::new().with_compilation(result).dialogue;
    let log = run_and_log(&mut dialogue, "Start");
    assert_eq!(
        vec![
            "start Start",
            "After the options",
            "complete Start",
            "dialogue complete"
        ],
        log
    );
}

#[test]
fn test_once_variables_can_be_reset() {
    let result = Compiler::from_test_source("Only once <<once>> #line:abc123\nAlways")
        .compile()
        .unwrap();
    let mut dialogue = TestBase::new().with_compilation(result).dialogue;
    let lines = |log: Vec<String>| -> Vec<String> {
        log.into_iter()
            .filter(|l| l == "Only once" || l == "Always")
            .collect()
    };
    assert_eq!(
        vec!["Only once", "Always"],
        lines(run_and_log(&mut dialogue, "Start"))
    );
    assert_eq!(vec!["Always"], lines(run_and_log(&mut dialogue, "Start")));

    let once_variable = Library::generate_unique_content_viewed_variable_name("line:abc123");
    dialogue
        .variable_storage_mut()
        .set(once_variable, false.into())
        .unwrap();
    assert_eq!(
        vec!["Only once", "Always"],
        lines(run_and_log(&mut dialogue, "Start"))
    );
}

#[test]
fn test_shadow_lines_share_text_but_not_metadata() {
    let result = Compiler::from_test_source(
        "This is a line. #line:source #apple\nThis is a line. #shadow:source #banana",
    )
    .compile()
    .unwrap();
    assert_eq!(2, result.string_table.len());
    // Shadow lines don't count as untagged lines
    assert!(!result.contains_implicit_string_tags);
    let source = &result.string_table[&LineId("line:source".to_owned())];
    assert_eq!(None, source.shadow_line_id);
    let (shadow_id, shadow) = result
        .string_table
        .iter()
        .find(|(id, _)| id.0 != "line:source")
        .unwrap();
    assert!(shadow_id.0.starts_with("line:sh_"));
    assert_eq!(
        Some(LineId("line:source".to_owned())),
        shadow.shadow_line_id
    );
    assert_eq!(source.text, shadow.text);
    assert!(shadow.metadata.contains(&"banana".to_owned()));
    assert!(!shadow.metadata.contains(&"apple".to_owned()));

    for (source, code) in [
        ("A line. #shadow:nope", "YS0042"),
        ("A line. #line:a\nAnother line. #shadow:a", "YS0044"),
        ("A {1} line. #line:a\nA {1} line. #shadow:a", "YS0043"),
        ("A line. #line:a #shadow:b", "YS0017"),
        ("A line. #line:a #line:b", "YS0062"),
    ] {
        let diagnostics = Compiler::from_test_source(source).compile().unwrap_err().0;
        assert!(
            diagnostics.iter().any(|d| d.code.as_deref() == Some(code)),
            "{source} should fail with {code}, got {diagnostics:?}"
        );
    }
}
