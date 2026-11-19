//! Semantic predicates and actions used by the generated lexer.
//! In the C# implementation these are members of `IndentAwareLexer`.

use antlr4rust::int_stream::IntStream;
use antlr4rust::lexer::Lexer;
use std::cell::Cell;

thread_local! {
    // The generated predicates are free functions without access to our lexer wrapper,
    // and a file is always lexed to completion on one thread, so this is per-thread state.
    static IN_WHEN_CLAUSE: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn is_in_when_clause() -> bool {
    IN_WHEN_CLAUSE.with(Cell::get)
}

pub(crate) fn set_in_when_clause(value: bool) {
    IN_WHEN_CLAUSE.with(|c| c.set(value));
}

/// Looks ahead one character and returns whether it is whitespace or `>`,
/// i.e. whether a command keyword like `return` really ended (vs. `returnToMenu`).
pub(crate) fn is_end_of_command_keyword<'input, L>(lexer: &mut L) -> bool
where
    L: Lexer<'input>,
    L::Input: IntStream,
{
    let next = lexer.input().la(1);
    match u32::try_from(next).ok().and_then(char::from_u32) {
        Some(c) => c == '>' || c.is_whitespace(),
        // EOF is `(char)0` in C#, which is neither
        None => false,
    }
}
