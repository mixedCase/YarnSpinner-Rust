//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner/VirtualMachine.cs>, which we split into multiple files

use crate::prelude::*;
use core::fmt::Debug;

/// Where to continue when a node that was entered with `<<detour>>` returns.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub(crate) struct CallSite {
    pub(crate) node_name: String,
    /// The detour instruction itself; execution resumes after it.
    pub(crate) instruction: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq, Default))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub(crate) struct State {
    /// The name of the node that we're currently in.
    pub(crate) current_node_name: Option<String>,

    /// The instruction number in the current node.
    pub(crate) program_counter: usize,

    /// The current list of options that will be delivered
    /// when the next ShowOptions instruction is encountered,
    /// along with the instruction each one jumps to.
    pub(crate) current_options: Vec<(DialogueOption, usize)>,

    /// The value stack.
    pub(crate) stack: Vec<InternalValue>,

    /// The nodes to return to, innermost detour last.
    pub(crate) call_stack: Vec<CallSite>,
}

impl State {
    pub(crate) fn push(&mut self, value: impl Into<InternalValue>) {
        self.stack.push(value.into())
    }

    /// Pops a value from the stack and tries to convert it to the specified type.
    ///
    /// ## Panics
    /// - Panics on an empty stack to mirror C# behavior.
    /// - Panics if the value cannot be converted to the specified type.
    pub(crate) fn pop<T>(&mut self) -> T
    where
        T: TryFrom<InternalValue>,
        <T as TryFrom<InternalValue>>::Error: Debug,
    {
        self.pop_value()
            .try_into()
            .unwrap_or_else(|e| panic!("Failed to convert popped value: {e:?}",))
    }

    /// Pops a value from the stack. Panics on an empty stack to mirror C# behavior.
    pub(crate) fn pop_value(&mut self) -> InternalValue {
        self.stack
            .pop()
            .unwrap_or_else(|| panic!("Tried to pop value, but the stack was empty."))
    }

    /// Copies the top value of the stack and tries to convert it to the specified type.
    ///
    /// ## Panics
    /// - Panics on an empty stack to mirror C# behavior.
    /// - Panics if the value cannot be converted to the specified type.
    pub(crate) fn peek<T>(&self) -> T
    where
        T: TryFrom<InternalValue>,
        <T as TryFrom<InternalValue>>::Error: Debug,
    {
        self.peek_value()
            .clone()
            .try_into()
            .unwrap_or_else(|e| panic!("Failed to convert peeked value: {e:?}",))
    }

    /// Peeks the top value of the stack. Panics on an empty stack to mirror C# behavior.
    pub(crate) fn peek_value(&self) -> &InternalValue {
        self.stack
            .last()
            .unwrap_or_else(|| panic!("Tried to peek value, but the stack was empty."))
    }
}
