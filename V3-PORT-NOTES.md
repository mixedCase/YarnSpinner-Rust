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

See checklist below (updated as work proceeds).

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
