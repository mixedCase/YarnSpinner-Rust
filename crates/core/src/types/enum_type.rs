//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner/Types/EnumType.cs>
use crate::prelude::*;
use crate::types::{Type, TypeProperties};
use core::hash::{Hash, Hasher};

pub(crate) fn enum_type_properties(enum_type: &EnumType) -> TypeProperties {
    TypeProperties::from_name("Enum")
        .with_description(enum_type.description.clone())
        .with_methods(yarn_library! {
            Operator::EqualTo => enum_equal_to,
            Operator::NotEqualTo => |a: YarnValue, b: YarnValue| !enum_equal_to(a, b),
        })
}

/// Enum cases are compared by raw value: as strings if the left one is a string, otherwise as integers.
fn enum_equal_to(a: YarnValue, b: YarnValue) -> bool {
    match (&a, &b) {
        (YarnValue::String(a), b) => *a == String::from(b.clone()),
        (a, b) => match (f32::try_from(a), f32::try_from(b)) {
            (Ok(a), Ok(b)) => a as i32 == b as i32,
            _ => false,
        },
    }
}

/// A user-defined enum type: a named set of cases that each have a raw value of type
/// [`Type::Number`] or [`Type::String`].
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq, Default, Hash))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub struct EnumType {
    /// The name of the enum.
    pub name: String,
    /// The documentation comment of the enum, if any.
    pub description: String,
    /// The type of the raw values: [`Type::Number`] or [`Type::String`].
    #[cfg_attr(feature = "bevy", reflect(ignore))]
    pub raw_type: Box<Type>,
    /// The cases of this enum, in declaration order.
    pub cases: Vec<EnumCase>,
}

/// A single case of an [`EnumType`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub struct EnumCase {
    /// The name of the case, e.g. `Apple` in `Food.Apple`
    pub name: String,
    /// The raw value that represents this case at runtime.
    pub value: YarnValue,
    /// The documentation comment of the case, if any.
    pub description: String,
}

impl EnumType {
    /// Returns the case with the given name.
    pub fn case(&self, name: &str) -> Option<&EnumCase> {
        self.cases.iter().find(|case| case.name == name)
    }

    /// The value a variable of this type has when nothing else is known: the first case.
    pub fn default_value(&self) -> Option<YarnValue> {
        self.cases.first().map(|case| case.value.clone())
    }
}

// Like in the original, types are identified by name alone.
impl PartialEq for EnumType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for EnumType {}

impl Hash for EnumType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl From<EnumType> for Type {
    fn from(enum_type: EnumType) -> Self {
        Type::Enum(enum_type)
    }
}
