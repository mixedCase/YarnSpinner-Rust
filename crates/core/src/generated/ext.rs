//! Contains extensions to generated types that in the original implementation are sprinkled around the repo via partial classes

use crate::generated::instruction::InstructionType;
use crate::prelude::*;
use core::error::Error;
use core::fmt::{Debug, Display};
use prost::Message;

impl From<String> for Operand {
    fn from(s: String) -> Self {
        Self {
            value: Some(OperandValue::StringValue(s)),
        }
    }
}

impl From<f32> for Operand {
    fn from(f: f32) -> Self {
        Self {
            value: Some(OperandValue::FloatValue(f)),
        }
    }
}

impl From<usize> for Operand {
    fn from(f: usize) -> Self {
        Self::from(f as f32)
    }
}

impl From<bool> for Operand {
    fn from(b: bool) -> Self {
        Self {
            value: Some(OperandValue::BoolValue(b)),
        }
    }
}

impl TryFrom<Operand> for String {
    type Error = ();

    fn try_from(value: Operand) -> Result<Self, Self::Error> {
        match value.value {
            Some(OperandValue::StringValue(s)) => Ok(s),
            _ => Err(()),
        }
    }
}

impl TryFrom<Operand> for f32 {
    type Error = ();

    fn try_from(value: Operand) -> Result<Self, Self::Error> {
        match value.value {
            Some(OperandValue::FloatValue(f)) => Ok(f),
            _ => Err(()),
        }
    }
}

impl TryFrom<Operand> for usize {
    type Error = ();

    fn try_from(value: Operand) -> Result<Self, Self::Error> {
        match value.value {
            // [sic] TODO: we only have float operands, which is
            // unpleasant. we should make 'int' operands a
            // valid type, but doing that implies that the
            // language differentiates between floats and
            // ints, which it doesn't.
            Some(OperandValue::FloatValue(f)) => Ok(f as usize),
            _ => Err(()),
        }
    }
}

impl TryFrom<Operand> for bool {
    type Error = ();

    fn try_from(value: Operand) -> Result<Self, Self::Error> {
        match value.value {
            Some(OperandValue::BoolValue(b)) => Ok(b),
            _ => Err(()),
        }
    }
}

impl From<Operand> for YarnValue {
    fn from(value: Operand) -> Self {
        let value = value.value.unwrap();
        match value {
            OperandValue::StringValue(s) => s.into(),
            OperandValue::FloatValue(f) => f.into(),
            OperandValue::BoolValue(b) => b.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub struct InvalidOpCodeError(pub i32);

impl Error for InvalidOpCodeError {}

impl Display for InvalidOpCodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?} is not a valid OpCode", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "bevy", reflect(opaque, Debug, PartialEq))]
pub struct DecodeError(prost::DecodeError);

impl Error for DecodeError {}

impl Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Could not decode program: {:?} ", self.0)
    }
}

impl Program {
    /// Creates a new Program by merging multiple Programs together.
    ///
    /// The new program will contain every node from every input program.
    /// Returns [`None`] if the input is empty.
    pub fn combine(programs: Vec<Program>) -> Option<Self> {
        if programs.is_empty() {
            return None;
        }
        let mut output = Program::default();
        for program in programs {
            for (node_name, node) in program.nodes {
                assert!(
                    !output.nodes.contains_key(&node_name),
                    "This program already contains a node named {node_name}",
                );
                output.nodes.insert(node_name, node);
            }
            output.initial_values.extend(program.initial_values);
        }
        Some(output)
    }

    /// Loader for compiled `.yarnc` files.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        Self::decode(bytes).map_err(DecodeError)
    }

    /// Encodes this [`Program`] into bytes, which can be saved as a `.yarnc` file.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }
}

macro_rules! instruction_from {
    ($( $enum_val:ident($instruction:ty) ),+) => {
        $(
            impl From<$instruction> for Instruction {
                fn from(value: $instruction) -> Self {
                    Self { instruction_type: Some(InstructionType::$enum_val(value)) }
                }
            }
        )*
    };
}

instruction_from! {
    JumpTo(JumpToInstruction),
    PeekAndJump(PeekAndJumpInstruction),
    RunLine(RunLineInstruction),
    RunCommand(RunCommandInstruction),
    AddOption(AddOptionInstruction),
    ShowOptions(ShowOptionsInstruction),
    PushString(PushStringInstruction),
    PushFloat(PushFloatInstruction),
    PushBool(PushBoolInstruction),
    JumpIfFalse(JumpIfFalseInstruction),
    Pop(PopInstruction),
    CallFunc(CallFunctionInstruction),
    PushVariable(PushVariableInstruction),
    StoreVariable(StoreVariableInstruction),
    Stop(StopInstruction),
    RunNode(RunNodeInstruction),
    PeekAndRunNode(PeekAndRunNodeInstruction),
    DetourToNode(DetourToNodeInstruction),
    PeekAndDetourToNode(PeekAndDetourToNode),
    Return(ReturnInstruction),
    AddSaliencyCandidate(AddSaliencyCandidateInstruction),
    AddSaliencyCandidateFromNode(AddSaliencyCandidateFromNodeInstruction),
    SelectSaliencyCandidate(SelectSaliencyCandidateInstruction)
}

impl Instruction {
    /// Sets the jump destination of this instruction. Returns `false` if this kind of instruction has none.
    pub fn set_destination(&mut self, destination: usize) -> bool {
        let destination = destination as i32;
        match self.instruction_type.as_mut() {
            Some(InstructionType::JumpTo(i)) => i.destination = destination,
            Some(InstructionType::AddOption(i)) => i.destination = destination,
            Some(InstructionType::JumpIfFalse(i)) => i.destination = destination,
            Some(InstructionType::AddSaliencyCandidate(i)) => i.destination = destination,
            Some(InstructionType::AddSaliencyCandidateFromNode(i)) => i.destination = destination,
            _ => return false,
        }
        true
    }
}

/// How the value of a variable is determined. New in Yarn Spinner 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariableKind {
    /// The variable is not known to the program.
    Unknown,
    /// The value is kept in the variable storage (or is still the initial value from the program).
    Stored,
    /// A smart variable: the value is computed from an expression every time it is read.
    Smart,
}

impl Program {
    /// The tag on the generated nodes that compute smart variables.
    pub const SMART_VARIABLE_NODE_TAG: &'static str = "Yarn.SmartVariable";

    /// Returns the nodes that compute the values of smart variables. The node name is the variable name.
    pub fn smart_variable_nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes
            .values()
            .filter(|node| node.tags().iter().any(|tag| tag == Self::SMART_VARIABLE_NODE_TAG))
    }

    /// Returns how the value of the variable `name` is determined.
    pub fn variable_kind(&self, name: &str) -> VariableKind {
        if self.initial_values.contains_key(name) {
            VariableKind::Stored
        } else if self
            .nodes
            .get(name)
            .is_some_and(|node| node.tags().iter().any(|tag| tag == Self::SMART_VARIABLE_NODE_TAG))
        {
            VariableKind::Smart
        } else {
            VariableKind::Unknown
        }
    }

    /// Returns the IDs of all lines and options the node can show.
    pub fn line_ids_for_node(&self, node_name: &str) -> Option<Vec<LineId>> {
        let node = self.nodes.get(node_name)?;
        Some(
            node.instructions
                .iter()
                .filter_map(|instruction| match instruction.instruction_type.as_ref()? {
                    InstructionType::RunLine(line) => Some(line.line_id.clone().into()),
                    InstructionType::AddOption(option) => Some(option.line_id.clone().into()),
                    _ => None,
                })
                .collect(),
        )
    }
}

impl Node {
    /// Header holding the name of the variable that counts how often this node was visited.
    pub const TRACKING_VARIABLE_NAME_HEADER: &'static str = "$Yarn.Internal.TrackingVariable";
    /// Header holding the `;`-separated smart variables that must all be true for this node group member to run.
    pub const CONTENT_SALIENCY_CONDITION_VARIABLES_HEADER: &'static str =
        "$Yarn.Internal.ContentSaliencyVariables";
    /// Header holding the complexity score of the `when:` conditions of this node group member.
    pub const CONTENT_SALIENCY_CONDITION_COMPLEXITY_SCORE_HEADER: &'static str =
        "$Yarn.Internal.ContentSaliencyComplexity";
    /// Header marking the generated node that selects a member of a node group.
    pub const NODE_IS_HUB_NODE_HEADER: &'static str = "$Yarn.Internal.NodeGroupHub";
    /// Header holding the name of the node group this node is a member of.
    pub const NODE_GROUP_HEADER: &'static str = "$Yarn.Internal.NodeGroup";

    /// Returns the trimmed value of the first header with the given key.
    pub fn get_header(&self, key: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|header| header.key == key)
            .map(|header| header.value.trim().to_owned())
    }

    /// The tags of the `tags:` header. Empty if the header is missing or blank.
    pub fn tags(&self) -> Vec<String> {
        self.get_header("tags")
            .map(|tags| tags.split_whitespace().map(Into::into).collect())
            .unwrap_or_default()
    }

    /// The name of the variable that counts the visits of this node, if the node is tracked.
    pub fn tracking_variable_name(&self) -> Option<String> {
        self.get_header(Self::TRACKING_VARIABLE_NAME_HEADER)
    }

    /// Whether this is the generated node that selects which member of a node group runs.
    pub fn is_node_group_hub(&self) -> bool {
        self.get_header(Self::NODE_IS_HUB_NODE_HEADER).is_some()
    }

    /// The name of the node group this node is a member of.
    pub fn node_group(&self) -> Option<String> {
        self.get_header(Self::NODE_GROUP_HEADER)
    }

    /// The smart variables that must all evaluate to `true` for this node group member to be able to run.
    pub fn content_saliency_condition_variables(&self) -> Vec<String> {
        self.get_header(Self::CONTENT_SALIENCY_CONDITION_VARIABLES_HEADER)
            .map(|variables| {
                variables
                    .split(';')
                    .filter(|variable| !variable.is_empty())
                    .map(Into::into)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The complexity score of the `when:` conditions of this node, or -1 if it has none.
    pub fn content_saliency_condition_complexity_score(&self) -> i32 {
        self.get_header(Self::CONTENT_SALIENCY_CONDITION_COMPLEXITY_SCORE_HEADER)
            .and_then(|score| score.parse().ok())
            .unwrap_or(-1)
    }
}
