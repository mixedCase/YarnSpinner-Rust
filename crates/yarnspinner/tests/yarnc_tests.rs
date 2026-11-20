use std::fs::{create_dir_all, remove_dir_all};
use std::path::{Path, PathBuf};
use std::process::Command;
use test_base::prelude::*;
use yarnspinner_core::prelude::*;

mod test_base;

fn ysc() -> PathBuf {
    let local = project_root_path().join("../../tools/ysc");
    if local.exists() {
        local
    } else {
        PathBuf::from("ysc")
    }
}

/// Compiles `yarn_file` with ysc into a fresh temp dir and returns the decoded [`Program`].
fn compile_with_ysc(yarn_file: &Path) -> Program {
    // One directory per input, because tests run in parallel
    let out_dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("yarnc_tests")
        .join(yarn_file.file_stem().unwrap());
    let _ = remove_dir_all(&out_dir);
    create_dir_all(&out_dir).unwrap();

    let output = Command::new(ysc())
        .arg("compile")
        .arg(yarn_file)
        .arg("-o")
        .arg(&out_dir)
        .arg("-n")
        .arg("out")
        .output()
        .expect("ysc should be installed, see ./scripts/install-ysc.sh");
    assert!(
        output.status.success(),
        "ysc failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let bytes = std::fs::read(out_dir.join("out.yarnc")).unwrap();
    let program = Program::from_bytes(&bytes).unwrap();
    assert!(
        program.nodes.values().all(|n| !n.instructions.is_empty()),
        "ysc produced empty nodes. This is a known issue when compiling
        yarnspinner 2.5.0 code with ysc 3.x. Do you have the right version
        installed? See ./scripts/install-ysc.sh"
    );
    program
}

fn hello_world_path() -> PathBuf {
    project_root_path().join("tests/assets/hello_world.yarn")
}

#[test]
fn ysc_program_survives_prost_round_trip() {
    let program = compile_with_ysc(&hello_world_path());
    // The bytes between rust compilation and ysc compilation may differ, so we
    // avoid comparing them here. Protobuf doesn't guarantee order.
    let round_tripped = Program::from_bytes(&program.to_bytes()).unwrap();
    assert_eq!(program, round_tripped);
}

/// Compiles upstream test cases with both `ysc` and this compiler and compares the programs.
/// This checks the code generator instruction by instruction against the reference implementation.
#[test]
fn rust_compiler_generates_same_programs_as_ysc() {
    let mut mismatches = Vec::new();
    for test_case in [
        "Commands",
        "DecimalNumbers",
        "Detours",
        "Detours-MayDetourToThemselves",
        // "Enums" is left out on purpose: ysc 3.3.1 generates code for the raw values of enum cases,
        // which pushes them onto the stack where they stay forever. We don't replicate that.
        "Escaping",
        "FormatFunctions",
        "IfStatements",
        "Indentation",
        "InlineExpressions",
        "Jumps",
        "LineGroups",
        "Lines",
        "NodeGroups",
        "NodeGroupsContentQuerying",
        "NodeGroupsWithImplicitDeclarations",
        "NodeGroupVisitTracking",
        "Once",
        "ShadowLines",
        "ShortcutOptions",
        "SmartVariables",
        "Smileys",
        "Types",
        "VariableStorage",
        "VeryLargeFileOfVariables",
        "VisitCount",
        "Visited",
        "VisitTracking",
    ] {
        // Generated names contain a hash of the file name, so both compilers must see the same spelling of it
        let path = test_data_path()
            .join("TestCases")
            .join(format!("{test_case}.yarn"))
            .canonicalize()
            .unwrap();
        let expected = compile_with_ysc(&path);
        let actual = yarnspinner::compiler::Compiler::new()
            .read_file(&path)
            .compile()
            .unwrap()
            .program
            .unwrap();
        let node_names = |program: &Program| program.nodes.keys().cloned().collect::<Vec<_>>();
        if node_names(&expected) != node_names(&actual) {
            mismatches.push(format!(
                "{test_case}: nodes differ\n  ysc:  {:?}\n  rust: {:?}",
                node_names(&expected),
                node_names(&actual)
            ));
            continue;
        }
        if expected.initial_values != actual.initial_values {
            mismatches.push(format!(
                "{test_case}: initial values differ\n  ysc:  {:?}\n  rust: {:?}",
                expected.initial_values, actual.initial_values
            ));
        }
        for (name, expected_node) in &expected.nodes {
            let actual_node = &actual.nodes[name];
            if expected_node.headers != actual_node.headers {
                mismatches.push(format!(
                    "{test_case}/{name}: headers differ\n  ysc:  {:?}\n  rust: {:?}",
                    expected_node.headers, actual_node.headers
                ));
            }
            if expected_node.instructions != actual_node.instructions {
                let first_difference = expected_node
                    .instructions
                    .iter()
                    .zip(&actual_node.instructions)
                    .position(|(a, b)| a != b)
                    .unwrap_or(
                        expected_node
                            .instructions
                            .len()
                            .min(actual_node.instructions.len()),
                    );
                mismatches.push(format!(
                    "{test_case}/{name}: instruction {first_difference} differs ({} vs {} instructions)\n  ysc:  {:?}\n  rust: {:?}",
                    expected_node.instructions.len(),
                    actual_node.instructions.len(),
                    expected_node.instructions.get(first_difference),
                    actual_node.instructions.get(first_difference)
                ));
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
