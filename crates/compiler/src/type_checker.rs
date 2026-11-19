//! The Yarn Spinner 3 type checker: collects constraints between the types of expressions
//! while walking the parse tree and solves them.
//!
//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/TypeCheckerListener.cs>
//! and <https://github.com/YarnSpinnerTool/YarnSpinner/tree/v3.3.1/YarnSpinner.Compiler/TypeSolver>

mod declarations;
mod listener;
mod literal_values;
mod solver;
mod types;
mod user_defined_types;

pub(crate) use self::{
    declarations::*, listener::*, literal_values::*, solver::*, types::*, user_defined_types::*,
};
