//! The compiler's view of types, which unlike [`Type`] includes type variables that the solver has yet to resolve.
//!
//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/tree/v3.3.1/YarnSpinner/Types>
//! and <https://github.com/YarnSpinnerTool/YarnSpinner/tree/v3.3.1/YarnSpinner.Compiler/TypeSolver/Terms>

use std::collections::HashMap;
use std::fmt::{self, Display};
use std::rc::Rc;
use yarnspinner_core::prelude::*;
use yarnspinner_core::types::{EnumType, FunctionType};

/// A type term: either a type variable or a type literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Ty {
    /// A type that is not known yet. Identified by name.
    Var(Rc<str>),
    /// The type of expressions that failed to type check.
    Error,
    Any,
    Boolean,
    Number,
    String,
    /// The parent of all user-defined enums.
    EnumBase,
    /// A user-defined enum.
    Enum(Rc<EnumType>),
    Function(Rc<FnTy>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FnTy {
    pub(crate) parameters: Vec<Ty>,
    pub(crate) return_type: Ty,
    pub(crate) variadic_parameter_type: Option<Ty>,
}

impl FnTy {
    /// The type of the parameter at `index`, taking variadic parameters into account.
    pub(crate) fn parameter_at(&self, index: usize) -> Option<&Ty> {
        self.parameters
            .get(index)
            .or(self.variadic_parameter_type.as_ref())
    }
}

/// The types that every program knows.
pub(crate) const BUILTIN_TYPES: [Ty; 5] = [Ty::String, Ty::Number, Ty::Boolean, Ty::Any, Ty::Error];

impl Ty {
    pub(crate) fn var(name: impl AsRef<str>) -> Self {
        Ty::Var(Rc::from(name.as_ref()))
    }

    pub(crate) fn is_var(&self) -> bool {
        matches!(self, Ty::Var(_))
    }

    /// Corresponds to `TypeBase.Name`
    pub(crate) fn name(&self) -> String {
        match self {
            Ty::Var(name) => name.to_string(),
            Ty::Error => "<ERROR>".to_owned(),
            Ty::Any => "Any".to_owned(),
            Ty::Boolean => "Bool".to_owned(),
            Ty::Number => "Number".to_owned(),
            Ty::String => "String".to_owned(),
            Ty::EnumBase => "Enum".to_owned(),
            Ty::Enum(enum_type) => enum_type.name.clone(),
            Ty::Function(function) => {
                let parameters: Vec<_> = function.parameters.iter().map(Ty::name).collect();
                format!(
                    "({}) -> {}",
                    parameters.join(", "),
                    function.return_type.name()
                )
            }
        }
    }

    fn parent(&self) -> Option<Ty> {
        match self {
            Ty::Boolean | Ty::Number | Ty::String | Ty::EnumBase | Ty::Function(_) => Some(Ty::Any),
            Ty::Enum(_) => Some(Ty::EnumBase),
            Ty::Var(_) | Ty::Error | Ty::Any => None,
        }
    }

    /// How many ancestors a type literal has. More specific types are deeper.
    pub(crate) fn type_depth(&self) -> usize {
        std::iter::successors(self.parent(), Ty::parent).count()
    }

    fn is_ancestor_of(&self, other: &Ty) -> bool {
        std::iter::successors(Some(other.clone()), Ty::parent).any(|ancestor| ancestor == *self)
    }

    /// Whether a value of this type literal can be used where `other` is expected:
    /// when `other` is the same type or an ancestor, or the raw type of this enum.
    pub(crate) fn is_convertible_to(&self, other: &Ty) -> bool {
        matches!(self, Ty::Enum(enum_type) if Ty::from(enum_type.raw_type.as_ref()) == *other)
            || other.is_ancestor_of(self)
    }

    /// Whether this type literal has a member (i.e. is an enum with a case) of this name.
    pub(crate) fn has_member(&self, name: &str) -> bool {
        matches!(self, Ty::Enum(enum_type) if enum_type.case(name).is_some())
    }

    /// Replaces all type variables that the substitution knows.
    pub(crate) fn substitute(&self, substitution: &Substitution) -> Ty {
        match self {
            Ty::Var(name) => match substitution.get(name) {
                Some(ty) => ty.substitute(substitution),
                None => self.clone(),
            },
            Ty::Function(function) => Ty::Function(Rc::new(FnTy {
                parameters: function
                    .parameters
                    .iter()
                    .map(|p| p.substitute(substitution))
                    .collect(),
                return_type: function.return_type.substitute(substitution),
                variadic_parameter_type: function
                    .variadic_parameter_type
                    .as_ref()
                    .map(|p| p.substitute(substitution)),
            })),
            _ => self.clone(),
        }
    }

    /// The value a variable of this type has when it was never declared.
    pub(crate) fn default_value(&self) -> Option<YarnValue> {
        match self {
            Ty::Boolean => Some(false.into()),
            Ty::Number => Some(0.0.into()),
            Ty::String => Some("".into()),
            Ty::Enum(enum_type) => enum_type.default_value(),
            _ => None,
        }
    }

    /// Converts to the public type. Unresolved and erroneous types become [`Type::Any`],
    /// which only happens when the compilation fails anyway.
    pub(crate) fn to_type(&self) -> Type {
        match self {
            Ty::Var(_) | Ty::Error | Ty::Any | Ty::EnumBase => Type::Any,
            Ty::Boolean => Type::Boolean,
            Ty::Number => Type::Number,
            Ty::String => Type::String,
            Ty::Enum(enum_type) => Type::Enum(enum_type.as_ref().clone()),
            Ty::Function(function) => Type::Function(FunctionType {
                parameters: function
                    .parameters
                    .iter()
                    .map(|p| Some(p.to_type()))
                    .collect(),
                return_type: Box::new(Some(function.return_type.to_type())),
                variadic_parameter_type: Box::new(
                    function.variadic_parameter_type.as_ref().map(Ty::to_type),
                ),
            }),
        }
    }
}

impl From<&Type> for Ty {
    fn from(r#type: &Type) -> Self {
        let optional = |ty: &Option<Type>| ty.as_ref().map(Ty::from).unwrap_or(Ty::Any);
        match r#type {
            Type::Any => Ty::Any,
            Type::Boolean => Ty::Boolean,
            Type::Number => Ty::Number,
            Type::String => Ty::String,
            Type::Enum(enum_type) => Ty::Enum(Rc::new(enum_type.clone())),
            Type::Function(function) => Ty::Function(Rc::new(FnTy {
                parameters: function.parameters.iter().map(optional).collect(),
                return_type: optional(&function.return_type),
                variadic_parameter_type: function
                    .variadic_parameter_type
                    .as_ref()
                    .as_ref()
                    .map(Ty::from),
            })),
        }
    }
}

impl Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

/// What the solver found out so far: the types of type variables.
pub(crate) type Substitution = HashMap<Rc<str>, Ty>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_convert_to_their_raw_type_and_ancestors() {
        let food = Ty::Enum(Rc::new(EnumType {
            name: "Food".to_owned(),
            raw_type: Box::new(Type::String),
            ..Default::default()
        }));
        assert!(food.is_convertible_to(&Ty::String));
        assert!(food.is_convertible_to(&Ty::Any));
        assert!(food.is_convertible_to(&food));
        assert!(!food.is_convertible_to(&Ty::Number));
        assert!(!Ty::String.is_convertible_to(&food));
        assert_eq!(2, food.type_depth());
        assert_eq!(1, Ty::Number.type_depth());
        assert!(!Ty::Error.is_convertible_to(&Ty::Any));
    }
}
