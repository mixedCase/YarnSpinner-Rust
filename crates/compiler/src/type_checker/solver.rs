//! The constraint solver of the type checker.
//!
//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/tree/v3.3.1/YarnSpinner.Compiler/TypeSolver>
//!
//! ## Implementation notes
//!
//! The C# constraint class hierarchy is an enum here. Constraints are first simplified into
//! a formula of equalities ([`Formula`]), which is the only form the original solver handles after
//! simplification as well. Quirks of the original are kept on purpose and marked as such.

use super::types::*;
use crate::diagnostic_descriptors as descriptors;
use crate::prelude::*;
use std::rc::Rc;

/// Produces the code and message of the diagnostic for a failed constraint, given what the solver found out.
pub(crate) type DiagnosticProvider = Rc<dyn Fn(&Substitution) -> Diagnostic>;

#[derive(Clone)]
pub(crate) struct Constraint {
    pub(crate) kind: ConstraintKind,
    /// The diagnostic to report on failure. Has its position set, the provider only adds code and message.
    pub(crate) source: Rc<Diagnostic>,
    pub(crate) diagnostic_provider: DiagnosticProvider,
}

#[derive(Clone)]
pub(crate) enum ConstraintKind {
    Equality(Ty, Ty),
    /// A value of the first type can be used where the second type is expected.
    Convertible(Ty, Ty),
    HasMember(Ty, String),
    HasName(Ty, String),
    /// At least one of the equalities holds.
    AnyEquality(Vec<(Ty, Ty)>),
}

impl Constraint {
    pub(crate) fn failure_diagnostic(&self, substitution: &Substitution) -> Diagnostic {
        (self.diagnostic_provider)(substitution)
    }

    pub(crate) fn apply_substitution(&self, substitution: &Substitution) -> Self {
        let s = |ty: &Ty| ty.substitute(substitution);
        let kind = match &self.kind {
            ConstraintKind::Equality(a, b) => ConstraintKind::Equality(s(a), s(b)),
            ConstraintKind::Convertible(from, to) => ConstraintKind::Convertible(s(from), s(to)),
            ConstraintKind::HasMember(ty, member) => ConstraintKind::HasMember(s(ty), member.clone()),
            ConstraintKind::HasName(ty, name) => ConstraintKind::HasName(s(ty), name.clone()),
            ConstraintKind::AnyEquality(equalities) => {
                ConstraintKind::AnyEquality(equalities.iter().map(|(a, b)| (s(a), s(b))).collect())
            }
        };
        Self { kind, ..self.clone() }
    }

    /// Whether the constraint holds no matter what the type variables turn out to be.
    pub(crate) fn is_tautological(&self) -> bool {
        match &self.kind {
            ConstraintKind::Equality(a, b) => a == b,
            ConstraintKind::Convertible(from, to) => {
                from == to || (!from.is_var() && !to.is_var() && from.is_convertible_to(to))
            }
            ConstraintKind::HasMember(ty, member) => ty.has_member(member),
            ConstraintKind::HasName(ty, name) => !ty.is_var() && ty.name() == *name,
            ConstraintKind::AnyEquality(equalities) => equalities.iter().any(|(a, b)| a == b),
        }
    }

    pub(crate) fn is_equality_without_errors(&self) -> bool {
        matches!(&self.kind, ConstraintKind::Equality(a, b) if *a != Ty::Error && *b != Ty::Error)
    }

    fn simplify(&self, substitution: &Substitution, known_types: &[Ty]) -> Formula {
        match &self.kind {
            ConstraintKind::Equality(a, b) => Formula::Eq(a.clone(), b.clone()),
            ConstraintKind::AnyEquality(equalities) => Formula::Or(
                equalities
                    .iter()
                    .map(|(a, b)| Formula::Eq(a.clone(), b.clone()))
                    .collect(),
            )
            .simplify(),
            ConstraintKind::HasMember(ty, member) => Formula::Or(
                known_types
                    .iter()
                    .filter(|known| known.has_member(member))
                    .map(|known| Formula::Eq(ty.clone(), known.clone()))
                    .collect(),
            )
            .simplify(),
            ConstraintKind::HasName(ty, name) => Formula::Or(
                known_types
                    .iter()
                    .filter(|known| known.name() == *name)
                    .map(|known| Formula::Eq(ty.clone(), known.clone()))
                    .collect(),
            )
            .simplify(),
            ConstraintKind::Convertible(from, to) => {
                simplify_convertible(from, to, substitution, known_types)
            }
        }
    }
}

fn simplify_convertible(
    from: &Ty,
    to: &Ty,
    substitution: &Substitution,
    known_types: &[Ty],
) -> Formula {
    if from == to {
        return Formula::True;
    }
    // More specific types are tried first
    let by_depth = |mut types: Vec<Ty>| {
        types.sort_by_key(|ty| std::cmp::Reverse(ty.type_depth()));
        types
    };
    let substituted_from = from.substitute(substitution);
    let substituted_to = to.substitute(substitution);

    let pairs: Vec<(Ty, Ty)> = match (substituted_from.is_var(), substituted_to.is_var()) {
        (false, false) => {
            return if substituted_from.is_convertible_to(&substituted_to) {
                // The conversion is valid, so there is nothing left to solve
                Formula::Eq(substituted_from.clone(), substituted_from)
            } else {
                // Not convertible; this equality will fail to unify and produce our diagnostic
                Formula::Eq(substituted_from, substituted_to)
            };
        }
        (false, true) => by_depth(
            known_types
                .iter()
                .filter(|other| substituted_from.is_convertible_to(other))
                .cloned()
                .collect(),
        )
        .into_iter()
        .map(|to_type| (substituted_from.clone(), to_type))
        .collect(),
        (true, false) => by_depth(
            known_types
                .iter()
                .filter(|other| other.is_convertible_to(&substituted_to))
                .cloned()
                .collect(),
        )
        .into_iter()
        .map(|from_type| (from_type, substituted_to.clone()))
        .collect(),
        // Neither is known: all we can say is that they're the same. (sic: the original pairs them crosswise.)
        (true, true) => vec![(to.clone(), from.clone())],
    };

    let mut possibilities: Vec<_> = pairs
        .into_iter()
        .map(|(from_type, to_type)| {
            Formula::And(vec![
                Formula::Eq(from_type, from.clone()),
                Formula::Eq(to_type, to.clone()),
            ])
        })
        .collect();
    if possibilities.len() == 1 {
        possibilities.remove(0)
    } else {
        Formula::Or(possibilities)
    }
}

/// A constraint that was simplified down to equalities.
#[derive(Debug, Clone, PartialEq)]
enum Formula {
    True,
    False,
    Eq(Ty, Ty),
    And(Vec<Formula>),
    Or(Vec<Formula>),
}

impl Formula {
    fn is_tautological(&self) -> bool {
        match self {
            Formula::True => true,
            Formula::False => false,
            Formula::Eq(a, b) => a == b,
            Formula::And(children) => !children.is_empty() && children.iter().all(Self::is_tautological),
            Formula::Or(children) => children.iter().any(Self::is_tautological),
        }
    }

    fn simplify(self) -> Formula {
        if matches!(self, Formula::And(_) | Formula::Or(_)) && self.is_tautological() {
            return Formula::True;
        }
        match self {
            Formula::And(children) => {
                if children.contains(&Formula::False) {
                    return Formula::False;
                }
                Formula::And(
                    children
                        .into_iter()
                        .map(Self::simplify)
                        .filter(|child| *child != Formula::True)
                        .collect(),
                )
            }
            Formula::Or(mut children) if children.len() == 1 => children.remove(0).simplify(),
            Formula::Or(children) => Formula::Or(
                children
                    .into_iter()
                    .map(Self::simplify)
                    .filter(|child| *child != Formula::False)
                    .collect(),
            ),
            other => other,
        }
    }

    /// Converts to disjunctive normal form: a list of alternatives, of which one must hold.
    fn into_alternatives(self) -> Vec<Formula> {
        match self {
            Formula::Or(children) => cartesian_product(
                children.into_iter().map(Self::into_alternatives).collect(),
            )
            .into_iter()
            // sic: flattening the product means that one unsatisfiable (empty) child makes
            // the whole disjunction unsatisfiable, like in the original.
            .flatten()
            .fold(Vec::new(), |mut distinct, alternative| {
                if !distinct.contains(&alternative) {
                    distinct.push(alternative);
                }
                distinct
            }),
            Formula::And(children) => cartesian_product(
                children.into_iter().map(Self::into_alternatives).collect(),
            )
            .into_iter()
            .map(|product| Formula::And(product.into_iter().flat_map(Self::flatten_and).collect()))
            .collect(),
            leaf => vec![leaf],
        }
    }

    fn flatten_and(self) -> Vec<Formula> {
        match self {
            Formula::And(children) => children.into_iter().flat_map(Self::flatten_and).collect(),
            other => vec![other],
        }
    }
}

fn cartesian_product<T: Clone>(sequences: Vec<Vec<T>>) -> Vec<Vec<T>> {
    sequences
        .into_iter()
        .fold(vec![Vec::new()], |accumulator, sequence| {
            accumulator
                .iter()
                .flat_map(|accumulated| {
                    sequence.iter().map(move |item| {
                        let mut product = accumulated.clone();
                        product.push(item.clone());
                        product
                    })
                })
                .collect()
        })
}

/// Tries to find types for the type variables so that all `constraints` hold.
/// On success, `substitution` is extended with what was found out.
pub(crate) fn try_solve(
    constraints: &[Constraint],
    known_types: &[Ty],
    diagnostics: &mut Vec<Diagnostic>,
    substitution: &mut Substitution,
) -> bool {
    let children: Vec<_> = constraints
        .iter()
        .filter(|constraint| !constraint.is_tautological())
        .map(|constraint| constraint.simplify(substitution, known_types))
        .collect();
    if children.is_empty() {
        return true;
    }
    let formula = Formula::And(children).simplify();
    let alternatives = match Formula::Or(formula.into_alternatives()).simplify() {
        Formula::Or(alternatives) => alternatives,
        other => vec![other],
    };

    // ## Implementation note
    // The original clones the substitution for every candidate. With thousands of variables, that
    // gets slow. Unification only ever adds entries, so we record what each candidate added
    // and remove it again before trying the next one.
    let mut successful_additions: Vec<Vec<(Rc<str>, Ty)>> = Vec::new();
    for candidate in alternatives {
        let terms = match candidate {
            Formula::True => vec![],
            Formula::False => continue,
            Formula::And(terms) => terms,
            other => vec![other],
        };
        let mut added = Vec::new();
        let is_solved = terms.iter().all(|term| match term {
            Formula::Eq(a, b) => try_unify(a, b, substitution, &mut added),
            Formula::True => true,
            Formula::False => false,
            other => unreachable!("Internal error: didn't expect to see {other:?} in a solver candidate"),
        });
        let additions: Vec<_> = added
            .into_iter()
            .map(|variable| {
                let ty = substitution.remove(&variable).unwrap();
                (variable, ty)
            })
            .collect();
        if is_solved {
            successful_additions.push(additions);
        }
    }

    match successful_additions.len() {
        0 => false,
        1 => {
            substitution.extend(successful_additions.remove(0));
            true
        }
        _ => {
            if let Some(merged) = try_merge_substitutions(&successful_additions) {
                substitution.extend(merged);
                true
            } else {
                // Multiple substitutions apply, so the type is ambiguous
                for constraint in constraints {
                    let mut diagnostic = constraint.source.as_ref().clone();
                    let ambiguous = descriptors::TYPE_INFERENCE_FAILURE
                        .create(&["The type of this expression is ambiguous."]);
                    diagnostic.message = ambiguous.message;
                    diagnostic.code = ambiguous.code;
                    diagnostic.severity = ambiguous.severity;
                    diagnostics.push(diagnostic);
                }
                false
            }
        }
    }
}

/// When several substitutions solve the constraints, the most specific type wins for each variable.
/// Takes what each successful candidate added to the common substitution, and returns what to add to it.
fn try_merge_substitutions(additions: &[Vec<(Rc<str>, Ty)>]) -> Option<Vec<(Rc<str>, Ty)>> {
    let mut merged: Vec<(Rc<str>, Vec<Ty>)> = Vec::new();
    for (variable, ty) in additions.iter().flatten() {
        match merged.iter_mut().find(|(name, _)| name == variable) {
            Some((_, candidates)) => {
                if !candidates.contains(ty) {
                    candidates.push(ty.clone());
                }
            }
            None => merged.push((variable.clone(), vec![ty.clone()])),
        }
    }
    let depth = |ty: &Ty| if ty.is_var() { usize::MAX } else { ty.type_depth() };
    merged
        .into_iter()
        .map(|(variable, candidates)| {
            let best_depth = candidates.iter().map(depth).max()?;
            let mut best = candidates.into_iter().filter(|ty| depth(ty) == best_depth);
            let ty = best.next()?;
            best.next().is_none().then_some((variable, ty))
        })
        .collect()
}

/// Makes `x` and `y` equal by extending the substitution. The variables that were added are appended to `added`.
pub(crate) fn try_unify(
    x: &Ty,
    y: &Ty,
    substitution: &mut Substitution,
    added: &mut Vec<Rc<str>>,
) -> bool {
    match (x, y) {
        _ if x == y => true,
        (Ty::Var(variable), other) | (other, Ty::Var(variable)) => {
            unify_variable(variable, other, substitution, added)
        }
        (Ty::Function(x), Ty::Function(y)) => {
            x.parameters.len() == y.parameters.len()
                && x.parameters
                    .iter()
                    .zip(&y.parameters)
                    .all(|(x, y)| try_unify(x, y, substitution, added))
                && try_unify(&x.return_type, &y.return_type, substitution, added)
        }
        _ => false,
    }
}

fn unify_variable(
    variable: &Rc<str>,
    term: &Ty,
    substitution: &mut Substitution,
    added: &mut Vec<Rc<str>>,
) -> bool {
    if let Some(known) = substitution.get(variable).cloned() {
        return try_unify(&known, term, substitution, added);
    }
    if let Ty::Var(term_variable) = term
        && let Some(known) = substitution.get(term_variable).cloned()
    {
        return try_unify(&Ty::Var(variable.clone()), &known, substitution, added);
    }
    if occurs_check(variable, term, substitution) {
        return false;
    }
    substitution.insert(variable.clone(), term.clone());
    added.push(variable.clone());
    true
}

/// Whether `variable` occurs anywhere inside `term`, which would make the type infinite.
fn occurs_check(variable: &Rc<str>, term: &Ty, substitution: &Substitution) -> bool {
    match term {
        Ty::Var(term_variable) if term_variable == variable => true,
        Ty::Var(term_variable) => substitution
            .get(term_variable)
            .is_some_and(|known| occurs_check(variable, known, substitution)),
        Ty::Function(function) => {
            function
                .parameters
                .iter()
                .any(|parameter| occurs_check(variable, parameter, substitution))
                || occurs_check(variable, &function.return_type, substitution)
        }
        _ => false,
    }
}

/// Applies what the solver knows to the constraints and drops the ones that are now known to hold.
pub(crate) fn apply_solution(substitution: &Substitution, constraints: &[Constraint]) -> Vec<Constraint> {
    constraints
        .iter()
        .map(|constraint| constraint.apply_substitution(substitution))
        .filter(|constraint| !constraint.is_tautological())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constraint(kind: ConstraintKind) -> Constraint {
        let source = Rc::new(Diagnostic::from_message("failed"));
        let provider_source = source.clone();
        Constraint {
            kind,
            source,
            diagnostic_provider: Rc::new(move |_| provider_source.as_ref().clone()),
        }
    }

    #[test]
    fn solves_equalities_transitively() {
        let mut substitution = Substitution::new();
        let constraints = [
            constraint(ConstraintKind::Equality(Ty::var("a"), Ty::var("b"))),
            constraint(ConstraintKind::Equality(Ty::var("b"), Ty::Number)),
        ];
        assert!(try_solve(&constraints, &BUILTIN_TYPES, &mut vec![], &mut substitution));
        assert_eq!(Ty::Number, Ty::var("a").substitute(&substitution));
        assert!(apply_solution(&substitution, &constraints).is_empty());
    }

    #[test]
    fn rejects_conflicting_equalities() {
        let mut substitution = Substitution::new();
        let constraints = [
            constraint(ConstraintKind::Equality(Ty::var("a"), Ty::String)),
            constraint(ConstraintKind::Equality(Ty::var("a"), Ty::Number)),
        ];
        assert!(!try_solve(&constraints, &BUILTIN_TYPES, &mut vec![], &mut substitution));
    }

    #[test]
    fn disjunction_is_narrowed_by_other_constraints() {
        let mut substitution = Substitution::new();
        let constraints = [
            constraint(ConstraintKind::AnyEquality(vec![
                (Ty::var("a"), Ty::Number),
                (Ty::var("a"), Ty::String),
            ])),
            constraint(ConstraintKind::Equality(Ty::var("a"), Ty::String)),
        ];
        assert!(try_solve(&constraints, &BUILTIN_TYPES, &mut vec![], &mut substitution));
        assert_eq!(Ty::String, Ty::var("a").substitute(&substitution));
    }

    #[test]
    fn convertible_to_any_keeps_the_specific_type() {
        let mut substitution = Substitution::new();
        let constraints = [constraint(ConstraintKind::Convertible(Ty::Number, Ty::Any))];
        assert!(try_solve(&constraints, &BUILTIN_TYPES, &mut vec![], &mut substitution));
        let constraints = [constraint(ConstraintKind::Convertible(Ty::String, Ty::Number))];
        assert!(!try_solve(&constraints, &BUILTIN_TYPES, &mut vec![], &mut substitution));
    }
}
