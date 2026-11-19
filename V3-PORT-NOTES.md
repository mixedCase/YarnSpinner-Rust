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
