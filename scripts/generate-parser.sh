#!/usr/bin/env bash
# Regenerates the ANTLR lexer/parser from the grammar in the YarnSpinner submodule
# and applies the manual adjustments documented in
# crates/compiler/src/parser/generated/readme.md.
#
# Usage: ANTLR_JAR=path/to/antlr4-4.13.3-SNAPSHOT-complete.jar ./scripts/generate-parser.sh
# (jar: https://github.com/antlr4rust/antlr4/releases/tag/v0.5.0; needs `java` in PATH)
set -euo pipefail
cd "$(dirname "$0")/.."
: "${ANTLR_JAR:?set ANTLR_JAR to the antlr4rust tool jar}"
OUT=crates/compiler/src/parser/generated
TMP=$(mktemp -d)
cp third-party/YarnSpinner/YarnSpinner.Compiler/Grammars/YarnSpinner{Lexer,Parser}.g4 "$TMP/"
(cd "$TMP" && java -jar "$ANTLR_JAR" -Dlanguage=Rust YarnSpinnerLexer.g4 YarnSpinnerParser.g4 -visitor -Xexact-output-dir -o out)
rm -f "$OUT"/*.rs "$OUT"/*.interp "$OUT"/*.tokens
cp "$TMP"/out/* "$OUT/"
rm -rf "$TMP"

P="$OUT/yarnspinnerparser.rs"
L="$OUT/yarnspinnerlexer.rs"
# The generator emits a wrong trait name for rules with labelled alternatives.
sed -i 's/YarnSpinnerParserParserContext/YarnSpinnerParserContext/g' "$P"
# `node` is a grammar rule, so `visit_node` is ambiguous and recurses forever otherwise.
sed -i 's/^\(\s*\)visitor\.visit_node(self);/\1YarnSpinnerParserVisitor::visit_node(visitor, self);/' "$P"
# Lexer predicates/actions live in crate::parser::lexer_support (the C# version has them on the lexer base class).
sed -i 's/IsInWhenClause();/crate::parser::lexer_support::is_in_when_clause()/g; s/SetInWhenClause(\(true\|false\));/crate::parser::lexer_support::set_in_when_clause(\1);/g; s/IsEndOfCommandKeyword()/crate::parser::lexer_support::is_end_of_command_keyword(recog)/g' "$L"
grep -q "lexer_support::is_end_of_command_keyword" "$L" || { echo "lexer predicate patch failed"; exit 1; }
echo "done"
