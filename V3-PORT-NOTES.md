# Yarn Spinner 3 port notes

Branch: `yarn-spinner-3`. Target: YarnSpinner C# **v3.3.1** (submodule `third-party/YarnSpinner`).

## Base

Branched from `main` (0.10, Bevy 0.20) and **merged `pr-284`** (which contains `pr-278` and the
`antlr4rust` migration of #280/#281) instead of starting fresh:

- pr-284 already did the mechanical, hard-to-review parts: `antlr-rust` -> `antlr4rust` 0.5.2
  (required for the v3 grammar), the `oneof` instruction protobuf, `NodeBuilder`/`emit!`, VM call stack.
- It merged into 0.10 with only 5 trivial conflicts. `project.rs` conflicts were resolved in favour
  of `main` (0.10 API kept); the pr-278 `Project` v3 rewrite was dropped.
- pr-284 was generated against v3.0.0; everything was regenerated against v3.3.1.

## Tooling

- Parser: `ANTLR_JAR=... ./scripts/generate-parser.sh` (jar from
  https://github.com/antlr4rust/antlr4/releases/tag/v0.5.0, needs java; on NixOS:
  `nix shell nixpkgs#jre_headless -c ./scripts/generate-parser.sh`). The script applies all manual patches.
- Protobuf: `nix shell nixpkgs#protobuf -c cargo run -p yarnspinner_codegen --features proto --bin generate_proto`.

## Status

Feature checklist (C# v3.3.1 parity):

| Feature | Status |
|---|---|
| v3 compiled program format (`oneof` instructions, no labels, `language_version`) | done, programs are instruction-identical to `ysc` 3.3.1 for 26 upstream test cases (`yarnc_tests.rs`) |
| `<<detour>>` / `<<return>>` | done |
| `<<once>>` blocks (with `if`, `else`), `once` on lines, options, line group items | done |
| line conditions | done |
| line groups (`=>`) | done |
| node groups (`when:` headers: expression, `once`, `once if`, `always`; `subtitle`) | done |
| saliency: First, Best, BestLeastRecentlyViewed, RandomBestLeastRecentlyViewed + `ContentSaliencyStrategy` trait | done |
| enums (raw values, `.Case` shorthand, enum -> raw type conversion for functions) | done |
| smart variables (inline expansion + evaluation nodes + `Dialogue::get_smart_variable`) | done |
| shadow lines (`#shadow:`) | done (see deviation below) |
| variadic functions | done (`Vec<T>` as last parameter of a registered function) |
| v3 type checker / constraint solver | done (port of `TypeCheckerListener` + `TypeSolver`) |
| visit tracking in the VM (`$Yarn.Internal.TrackingVariable` header) | done |
| options fall through when none selected (3.1) | done (`Dialogue::set_no_option_selected`) |
| `has_any_content`, `min`, `max`, `format`, `Dialogue::get_header_value` etc. | done |
| undeclared functions are errors from language version 5 (3.3), warning below | done (`Compiler::language_version`) |
| escaped `:` in lines (3.2) | done (minimal, in the existing markup parser) |
| stable diagnostic codes (3.2) | done (`Diagnostic::code`) |
| `bevy_yarnspinner`: builds, saliency strategy selection | done (`DialogueRunnerBuilder::with_content_saliency_strategy`, `DialogueRunner::set_content_saliency_strategy`) |

## Test results

- Upstream test cases (`third-party/YarnSpinner/Tests/TestCases` + `ParseFailures`, 66 files): all pass
  (`cargo test -p yarnspinner --test language_tests test_sources`).
- `cargo test -p yarnspinner` (all integration tests): pass. `yarnc_tests` needs `tools/ysc` (3.3.1) and a
  .NET 9 runtime in `PATH` (`nix shell nixpkgs#dotnet-sdk_9 -c cargo test -p yarnspinner --test yarnc_tests`).

## Breaking API changes (0.10 -> this branch) and migrations

`yarnspinner` (core/compiler/runtime):

- Compiled programs use the v3 format. Recompile all `.yarn` files; `.yarnc` files from ysc 2.x no longer load.
- `DialogueOption::destination_node` was removed (v3 options jump to instructions, not nodes). No replacement needed:
  select options by `id` or line ID as before.
- `Dialogue::get_line_id_for_node` is deprecated (raw text nodes don't exist in v3).
- `DialogueEvent::DialogueComplete` (and the preceding `NodeComplete`) now arrive in the `continue_` call *after* the one
  that delivered the last line, because every node ends with an explicit `Return` instruction, like in C#.
  Migration: keep calling `continue_` while `Dialogue::can_continue()`; don't assume the batch with the last line also
  contains `DialogueComplete`.
- A plain `<<jump>>` to a node no longer emits `NodeComplete` for the *destination* (pr-284/2.x quirk); `NodeComplete` is
  emitted for every node that is left, including nodes left through a jump out of a detour.
- Calling an undeclared function is a compile error (YS0013). Migration: register the function in the `Library` passed to
  `Compiler::extend_library`, or compile with `.with_language_version(4)` to get a warning instead.
- `<<call f()>>` is no longer a statement (removed from the 3.x grammar); it is an ordinary command named `call`.
- `Compilation::warnings` now also contains diagnostics of the new severity `DiagnosticSeverity::Info`
  (e.g. YS0010 unused variable) and new warnings (e.g. YS0003 use of undeclared variable, YS0012 jump to undefined node,
  YS0033 empty node). Filter by `severity` if you print them.
- New public fields (breaks struct literals and exhaustive matches): `Diagnostic::code`, `Declaration::is_inline_expansion`,
  `StringInfo::shadow_line_id`, `Compilation::user_defined_types`, `Compiler::language_version`,
  `FunctionType::variadic_parameter_type`, enum variants `Type::Enum`, `DiagnosticSeverity::Info`,
  `DialogueError::{SmartVariableError, InvalidSaliencySelection}`.
- `Type::name()` returns `Cow<'static, str>` instead of `&'static str`.
- Error messages of the type checker changed to the C# v3 wording.
- Implicit line IDs are now `line:<8 hex digits>` like in C# (previously decimal). Tag your lines (`Compiler::tag_lines`)
  if you persist line IDs.
- Library operator names for subtraction changed to the C# names (`Number.Minus`, `Number.UnaryMinus`); only relevant if
  you call these library functions by name.
- `CURRENT_PROJECT_FILE_VERSION` is 5; `.yarnproject` files of versions 2 to 5 load.

`bevy_yarnspinner`:

- `DialogueOption::destination_node` removed (see above). `InnerDialogue::get_line_id_for_node` deprecated.
- New: `DialogueRunnerBuilder::with_content_saliency_strategy`, `DialogueRunner::{set_content_saliency_strategy,
  content_saliency_strategy, is_node_group, has_any_content, get_smart_variable, get_header_value, select_no_option}`.

## Deliberate deviations from C# 3.3.1

- The async `IDialogueResponder` API of 3.3 is not ported; `Dialogue` keeps the 0.10 event API and its `Library`.
- Shadow lines: `StringInfo::text` holds a copy of the source line's text (C#: `null`), plus `shadow_line_id`.
- Enum case raw values are not compiled into instructions (ysc 3.3.1 pushes them onto the stack and leaks them).
- Saliency strategies receive the variable storage as an argument instead of owning it.
- Booleans are formatted `true`/`false` (C#: `True`/`False`), as in 0.10.

## Known gaps

- Markup: the `LineParser` is still the 2.x port plus `\:`. Not ported from 3.x: the rewritten parser
  (`nomarkup` handling, `trimwhitespace` property, replacement marker API with invisible characters, markup
  diagnostics, implicit `[character]` processing by marker processors). `MarkupTests.cs` of v3 are not ported.
- Compiler: syntax validation warnings (YS0019-YS0022, YS0048, YS0064), markup diagnostics at compile time (YS0063),
  unreachable code analysis (YS0008), `NodeMetadata`, `ILineTagGenerator`/`DescriptiveLineTagGenerator`,
  `CompilationJob.TypeDeclarations` and forward declared types, `.ysls.json` handling are not ported.
- `format()` ignores alignment/format specifiers.
- New C# unit tests (`SaliencyTests.cs`, `SmartVariableTests.cs`, v3 parts of `TypeTests.cs`, `DialogueTests.cs`)
  are only partially ported; behaviour is covered by the upstream test plans.

## Architecture of the port (read this first when resuming)

- `crates/core`: `Type::Enum(EnumType)`, `FunctionType::variadic_parameter_type`, `Vec<T>` as variadic
  `YarnFn` parameter, v3 std library (`min`, `max`, `format`, `Enum.EqualTo`...), `Node`/`Program` header helpers
  (`generated/ext.rs`), operator names now match C# (`Number.Minus`, `Number.UnaryMinus`).
- `crates/runtime`: `virtual_machine.rs` rewritten after C# 3.3.1 (single state + call stack, saliency,
  smart variable evaluation, visit tracking in the VM). `saliency.rs` has the strategy trait + 4 built-ins.
  The C# 3.3 async `IDialogueResponder` API is deliberately NOT ported: the event-batch API of 0.10 is kept.
- `crates/compiler`:
  - `compiler/run_compilation.rs`: the whole pipeline in C# order (one function, replaces `compilation_steps/`),
    plus smart variable / node group compilers.
  - `type_checker/`: port of `TypeCheckerListener` + `TypeSolver` (`solver.rs`), enums (`user_defined_types.rs`).
  - `annotations.rs`: side tables replacing the C# partial-class properties on parser contexts
    (types, line IDs, node titles/groups, once variables), keyed by (file index, token interval).
  - `visitors/code_generation_visitor.rs`: port of the v3 `CodeGenerationVisitor`.
  - `diagnostic_descriptors.rs`: generated from `YarnSpinner.Diagnostics/Definitions` (codes `YS0001`...).
  - pr-284's `NodeBuilder`, `emit!` and the `compiler_macros` crate were removed again (replaced by `NodeEmitter`).
- Tests: `crates/yarnspinner/tests/test_base/test_plan.rs` implements the v3 `.testplan` format (runs, saliency,
  hashtags, `select: 0`). `language_tests::test_sources` runs every upstream test case and lists failures.
