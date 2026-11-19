//! The diagnostics the compiler can produce, with their stable codes.
//!
//! Generated from <https://github.com/YarnSpinnerTool/YarnSpinner/tree/v3.3.1/YarnSpinner.Diagnostics/Definitions>.
//! See <https://docs.yarnspinner.dev/write-yarn-scripts/yarn-spinner-editor/errors> for what each code means.

#![allow(dead_code)] // the complete catalogue is kept, even if not every diagnostic is produced yet

use crate::prelude::*;

/// Describes a kind of [`Diagnostic`]: its code, default severity and message template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiagnosticDescriptor {
    pub(crate) code: &'static str,
    template: &'static str,
    severity: DiagnosticSeverity,
}

impl DiagnosticDescriptor {
    /// Creates a diagnostic with the placeholders `{0}`, `{1}`, ... of the template replaced by `args`.
    pub(crate) fn create(&self, args: &[&str]) -> Diagnostic {
        let message = args
            .iter()
            .enumerate()
            .fold(self.template.to_owned(), |message, (i, arg)| {
                message.replace(&format!("{{{i}}}"), arg)
            });
        Diagnostic::from_message(message)
            .with_code(self.code)
            .with_severity(self.severity)
    }
}

/// Variable has conflicting implicit type declarations
pub(crate) const IMPLICIT_VARIABLE_TYPE_CONFLICT: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0001",
    template: "Variable {0} has been implicitly declared with multiple types: {1}",
    severity: DiagnosticSeverity::Error,
};

/// A type mismatch occurred during type checking.
pub(crate) const TYPE_MISMATCH: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0002",
    template: "Type mismatch: expected {0}, got {1}",
    severity: DiagnosticSeverity::Error,
};

/// Variable used without being declared
pub(crate) const UNDEFINED_VARIABLE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0003",
    template: "Variable '{0}' is used but not declared. Declare it with: <<declare {0} = value>>",
    severity: DiagnosticSeverity::Warning,
};

/// Missing node delimiter (=== or ---)
pub(crate) const MISSING_DELIMITER: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0004",
    template: "Missing node delimiter",
    severity: DiagnosticSeverity::Error,
};

/// Syntax error in Yarn script
pub(crate) const SYNTAX_ERROR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0005",
    template: "Syntax error: {0}",
    severity: DiagnosticSeverity::Error,
};

/// Unclosed command (missing >>).
pub(crate) const UNCLOSED_COMMAND: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0006",
    template: "Unclosed command: missing >>",
    severity: DiagnosticSeverity::Error,
};

/// A control flow block was not properly closed
pub(crate) const UNCLOSED_SCOPE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0007",
    template: "Unclosed scope: expected an <<{0}>> to match the <<{1}>> statement on line {2}",
    severity: DiagnosticSeverity::Error,
};

/// Unreachable code detected
pub(crate) const UNREACHABLE_CODE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0008",
    template: "Unreachable code detected",
    severity: DiagnosticSeverity::Warning,
};

/// Node has no references to it
pub(crate) const UNREFERENCED_NODE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0009",
    template: "Node '{0}' is never referenced",
    severity: DiagnosticSeverity::Error,
};

/// Variable is declared but never used
pub(crate) const UNUSED_VARIABLE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0010",
    template: "Variable '{0}' is declared but never used",
    severity: DiagnosticSeverity::Info,
};

/// Duplicate node title
pub(crate) const DUPLICATE_NODE_TITLE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0011",
    template: "Duplicate node title: '{0}'",
    severity: DiagnosticSeverity::Error,
};

/// Jump to undefined node
pub(crate) const UNDEFINED_NODE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0012",
    template: "Jump to undefined node: '{0}'",
    severity: DiagnosticSeverity::Warning,
};

/// Unknown function
pub(crate) const UNKNOWN_FUNCTION: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0013",
    template: "Unknown function call: no declaration found for {0}",
    severity: DiagnosticSeverity::Error,
};

/// Function called with incorrect parameters
pub(crate) const WRONG_FUNCTION_PARAMETERS: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0014",
    template: "Invalid function call: {0}",
    severity: DiagnosticSeverity::Error,
};

/// Cyclic dependency between nodes detected.
pub(crate) const CYCLIC_DEPENDENCY: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0015",
    template: "Cyclic dependency detected: {0}",
    severity: DiagnosticSeverity::Info,
};

/// Character name not defined in project configuration
pub(crate) const UNKNOWN_CHARACTER: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0016",
    template: "Unknown character: '{0}'",
    severity: DiagnosticSeverity::Info,
};

/// Lines cannot have both a '#line' tag and a '#shadow' tag.
pub(crate) const LINES_CANT_HAVE_LINE_AND_SHADOW_TAG: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0017",
    template: "Lines cannot have both a '#line' tag and a '#shadow' tag.",
    severity: DiagnosticSeverity::Error,
};

/// Duplicate line ID
pub(crate) const DUPLICATE_LINE_ID: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0018",
    template: "Duplicate line ID '{0}'",
    severity: DiagnosticSeverity::Error,
};

/// Line content after a command
pub(crate) const LINE_CONTENT_AFTER_COMMAND: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0019",
    template: "Dialogue \"{0}\" content found following a command. Commands should be on their own line.",
    severity: DiagnosticSeverity::Warning,
};

/// Line content before a command.
pub(crate) const COMMAND_FOLLOWING_LINE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0020",
    template: "Command \"{0}\" found following a line of dialogue. Commands should start on a new line.",
    severity: DiagnosticSeverity::Error,
};

/// Stray command end marker without matching start marker.
pub(crate) const STRAY_COMMAND_END: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0021",
    template: "Stray '>>' without matching '<<'. Did you forget to open the command?",
    severity: DiagnosticSeverity::Warning,
};

/// Command keyword appearing outside of command markers
pub(crate) const UNENCLOSED_COMMAND: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0022",
    template: "'{0}' command must be enclosed in '<<' and '>>'. Did you mean '<<{0} ...'?",
    severity: DiagnosticSeverity::Warning,
};

/// Node title or subtitle contains invalid characters
pub(crate) const INVALID_NODE_NAME: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0027",
    template: "Unexpected '{1}' in node {0}. Titles can only contain letters, numbers, and underscores.",
    severity: DiagnosticSeverity::Error,
};

/// Can't determine type of variable given its usage.
pub(crate) const TYPE_INFERENCE_FAILURE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0028",
    template: "Can't determine type of {0} given its usage. Manually specify its type with a declare statement.",
    severity: DiagnosticSeverity::Error,
};

/// Can't determine the type of an expression.
pub(crate) const EXPRESSION_TYPE_UNDETERMINED: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0029",
    template: "Can't determine the type of the expression {0}.",
    severity: DiagnosticSeverity::Error,
};

/// Smart variable cannot be modified.
pub(crate) const SMART_VARIABLE_READ_ONLY: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0030",
    template: "{0} cannot be modified (it's a smart variable and is always equal to {1})",
    severity: DiagnosticSeverity::Error,
};

/// All nodes in a group must have a 'when' clause.
pub(crate) const NODE_GROUP_MISSING_WHEN: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0031",
    template: "All nodes in the group '{0}' must have a 'when' clause (use 'when: always' if you want this node to not have any conditions).",
    severity: DiagnosticSeverity::Error,
};

/// Duplicate subtitle in a node group
pub(crate) const DUPLICATE_SUBTITLE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0032",
    template: "More than one node in group {0} has subtitle {1}.",
    severity: DiagnosticSeverity::Error,
};

/// Node is empty and will not be compiled.
pub(crate) const EMPTY_NODE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0033",
    template: "Node \"{0}\" is empty and will not be included in the compiled output.",
    severity: DiagnosticSeverity::Warning,
};

/// Function cannot be used in Yarn Spinner scripts
pub(crate) const INVALID_LIBRARY_FUNCTION: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0034",
    template: "Function {0} cannot be used in Yarn Spinner scripts: {1}",
    severity: DiagnosticSeverity::Error,
};

/// Enum declaration error
pub(crate) const ENUM_DECLARATION_ERROR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0035",
    template: "{0}",
    severity: DiagnosticSeverity::Error,
};

/// Language version too low for feature.
pub(crate) const LANGUAGE_VERSION_TOO_LOW: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0036",
    template: "{0}",
    severity: DiagnosticSeverity::Error,
};

/// Invalid literal value
pub(crate) const INVALID_LITERAL_VALUE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0037",
    template: "{0}",
    severity: DiagnosticSeverity::Error,
};

/// Invalid member access
pub(crate) const INVALID_MEMBER_ACCESS: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0038",
    template: "{0}",
    severity: DiagnosticSeverity::Error,
};

/// Redeclaration of existing variable
pub(crate) const REDECLARATION_OF_EXISTING_VARIABLE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0039",
    template: "Redeclaration of existing variable {0}",
    severity: DiagnosticSeverity::Error,
};

/// Redeclaration of existing type
pub(crate) const REDECLARATION_OF_EXISTING_TYPE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0040",
    template: "Can't create a new type {0}: a type with this name already exists",
    severity: DiagnosticSeverity::Error,
};

/// Internal compiler error
pub(crate) const INTERNAL_ERROR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0041",
    template: "Internal compiler error: {0}",
    severity: DiagnosticSeverity::Error,
};

/// Unknown line ID for shadow line.
pub(crate) const UNKNOWN_LINE_ID_FOR_SHADOW_LINE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0042",
    template: "Unknown line ID {0} for shadow line",
    severity: DiagnosticSeverity::Error,
};

/// "Shadow lines contains an expression"
pub(crate) const SHADOW_LINES_CANT_HAVE_EXPRESSIONS: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0043",
    template: "Shadow lines must not have expressions",
    severity: DiagnosticSeverity::Error,
};

/// Shadow line contains different text than its source
pub(crate) const SHADOW_LINES_MUST_HAVE_SAME_TEXT_AS_SOURCE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0044",
    template: "Shadow lines must have the same text as their source",
    severity: DiagnosticSeverity::Error,
};

/// Smart variable creates a reference loop
pub(crate) const SMART_VARIABLE_LOOP: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0045",
    template: "Smart variables cannot contain reference loops (referencing {0} here creates a loop for the smart variable {1}).",
    severity: DiagnosticSeverity::Error,
};

/// Variable declaration has a null default value.
pub(crate) const NULL_DEFAULT_VALUE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0046",
    template: "Variable declaration {0} (type {1}) has a null default value. This is not allowed.",
    severity: DiagnosticSeverity::Error,
};

/// Expression failed to resolve in a reasonable time.
pub(crate) const TYPE_SOLVER_TIMEOUT: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0047",
    template: "Expression failed to resolve in a reasonable time ({0}). Try simplifying this expression.",
    severity: DiagnosticSeverity::Error,
};

/// Dialogue begins and ends with single chevrons
pub(crate) const SINGULAR_COMMAND_WRAP: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0048",
    template: "Line {0} has single '<' and '>' wrapping it. Did you mean to make this a command?",
    severity: DiagnosticSeverity::Warning,
};

/// An error occurred in resolving this expression's type.
pub(crate) const TYPE_CHECKER_ERROR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0050",
    template: "{0}",
    severity: DiagnosticSeverity::Error,
};

/// Node is missing its title
pub(crate) const NODE_MISSING_TITLE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0051",
    template: "Nodes must have a title",
    severity: DiagnosticSeverity::Error,
};

/// Node has more than one title
pub(crate) const NODE_HAS_MORE_THAN_ONE_TITLE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0052",
    template: "Nodes must have a single title header",
    severity: DiagnosticSeverity::Error,
};

/// Variable's declared type doesn't match the type of its initial value
pub(crate) const DECLARATION_VALUE_DOESNT_MATCH_TYPE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0053",
    template: "{0} is declared to be a {1}, but its initial value '{2}' is a {3}",
    severity: DiagnosticSeverity::Error,
};

/// Command is not recognized or has invalid syntax
pub(crate) const UNKNOWN_COMMAND: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0060",
    template: "Unknown command: {0}",
    severity: DiagnosticSeverity::Warning,
};

/// Command was called with the wrong number of parameters
pub(crate) const WRONG_COMMAND_PARAMETER_COUNT: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0061",
    template: "Command {0} was called with {1} parameters, but expected {2}",
    severity: DiagnosticSeverity::Warning,
};

/// Dialogue has multiple '#line' or '#shadow' IDs.
pub(crate) const MULTIPLE_LINE_OR_SHADOW_IDS_ON_A_LINE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0062",
    template: "Dialogue has multiple '#line' or '#shadow' IDs.",
    severity: DiagnosticSeverity::Error,
};

/// Dialogue has malformed or invalid markup.
pub(crate) const MARKUP_FAILED_TO_PARSE: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0063",
    template: "Dialogue has malformed or invalid markup. {0}",
    severity: DiagnosticSeverity::Warning,
};

/// Extra chevrons on the same line as a command.
pub(crate) const ROGUE_CHEVRON_WITH_COMMAND: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0064",
    template: "Command \"<<{0}>>\" has extra chevrons on the same line as it.",
    severity: DiagnosticSeverity::Warning,
};

/// Use of undeclared functions is deprecated
pub(crate) const USE_OF_UNDECLARED_FUNCTIONS_IN_V4_IS_DEPRECATED: DiagnosticDescriptor = DiagnosticDescriptor {
    code: "YS0065",
    template: "Function {0} is unknown. You should declare it in your game so that the Yarn Spinner compiler is aware of it. This will become an error in future versions.",
    severity: DiagnosticSeverity::Warning,
};

