// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Whether the guards next-edition rules add to an event can hold together
//! (design draft B.6, `rule_conflict_unsatisfiable_guard`).
//!
//! The question is asked over the event's finite type domains, not over the
//! reachable states (decisions/048 J3): each field of the entity and each
//! input of the event ranges over its declared type, the entity's stage is
//! the one the event leaves from, and the guards are conjoined. This module
//! decides it by enumerating the assignments, so the lowering (and with it
//! the LSP and the Worker, which have no solver) report the conflict the same
//! way. A problem whose domain product exceeds [`ENUMERATION_BOUND`] is left
//! to `fslc check`, which asks a solver about the same [`GuardProblem`].
//!
//! Only the finite fragment is decided: literals, the entity's fields, the
//! event's inputs, constants, enum members, Boolean connectives, comparison
//! and integer arithmetic. A guard outside it makes the problem
//! [`GuardDecision::Unsupported`], which reports nothing.

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{
    BusinessItem, Expr, Param, ProcessItem, RequirementsItem, Span, SpecItem, SurfaceRequirements,
    VerifyItem, stage_reference,
};

/// The guards rules add to an event cannot all hold, over the types'
/// domains, when the event leaves its stage.
pub const RULE_CONFLICT_UNSATISFIABLE_GUARD: &str = "rule_conflict_unsatisfiable_guard";

/// The largest domain product decided by enumeration.
pub const ENUMERATION_BOUND: u128 = 1_000_000;

/// The values one variable of a [`GuardProblem`] ranges over.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FiniteDomain {
    Bool,
    /// `lo..=hi`.
    Int {
        lo: i64,
        hi: i64,
    },
    /// The members of an enum.
    Members(Vec<String>),
}

impl FiniteDomain {
    fn size(&self) -> u128 {
        match self {
            Self::Bool => 2,
            Self::Int { lo, hi } => {
                u128::try_from(i128::from(*hi) - i128::from(*lo) + 1).unwrap_or(0)
            }
            Self::Members(members) => members.len() as u128,
        }
    }

    fn values(&self) -> Vec<FiniteValue> {
        match self {
            Self::Bool => vec![FiniteValue::Bool(false), FiniteValue::Bool(true)],
            Self::Int { lo, hi } => (*lo..=*hi).map(FiniteValue::Int).collect(),
            Self::Members(members) => members.iter().cloned().map(FiniteValue::Member).collect(),
        }
    }
}

/// A value of the finite fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FiniteValue {
    Bool(bool),
    Int(i64),
    /// An enum member, compared by name.
    Member(String),
}

/// One guard of a [`GuardProblem`]: the rule it comes from and its
/// condition, with every field spelled `Entity.field`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProblemGuard {
    pub requirement: String,
    pub detail: String,
    pub condition: Expr,
}

/// Can the guards rules add to one event hold together?
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuardProblem {
    pub event: String,
    pub entity: String,
    /// The stage the event leaves from.
    pub stage: String,
    pub span: Span,
    /// The free variables: the entity's fields (`Entity.field`) and the
    /// event's inputs, each with its domain. `None` when one of them has a
    /// type outside the finite fragment.
    pub variables: Option<Vec<(String, FiniteDomain)>>,
    /// The constants and the entity's stage ([`stage_reference`]).
    pub constants: BTreeMap<String, FiniteValue>,
    /// Every enum member and stage name a guard may spell bare.
    pub members: BTreeSet<String>,
    pub guards: Vec<ProblemGuard>,
}

/// The answer of [`GuardProblem::decide_by_enumeration`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GuardDecision {
    Satisfiable,
    /// No assignment satisfies the guards; the indexes of a minimal subset
    /// of them that is already unsatisfiable.
    Unsatisfiable(Vec<usize>),
    /// The domain product exceeds the bound.
    BeyondBound,
    /// A guard or a variable is outside the finite fragment.
    Unsupported,
}

fn binary_value(op: &str, left: FiniteValue, right: FiniteValue) -> Option<FiniteValue> {
    use FiniteValue::{Bool, Int};
    Some(match (op, left, right) {
        ("and", Bool(left), Bool(right)) => Bool(left && right),
        ("or", Bool(left), Bool(right)) => Bool(left || right),
        ("=>", Bool(left), Bool(right)) => Bool(!left || right),
        ("==", left, right) => Bool(left == right),
        ("!=", left, right) => Bool(left != right),
        ("<", Int(left), Int(right)) => Bool(left < right),
        ("<=", Int(left), Int(right)) => Bool(left <= right),
        (">", Int(left), Int(right)) => Bool(left > right),
        (">=", Int(left), Int(right)) => Bool(left >= right),
        ("+", Int(left), Int(right)) => Int(left.checked_add(right)?),
        ("-", Int(left), Int(right)) => Int(left.checked_sub(right)?),
        ("*", Int(left), Int(right)) => Int(left.checked_mul(right)?),
        ("/", Int(left), Int(right)) if right != 0 => Int(left.div_euclid(right)),
        ("%", Int(left), Int(right)) if right != 0 => Int(left.rem_euclid(right)),
        _ => return None,
    })
}

/// The names a condition may spell besides its variables.
struct Fragment<'a> {
    constants: &'a BTreeMap<String, FiniteValue>,
    members: &'a BTreeSet<String>,
}

/// Whether `expression` stays inside the finite fragment.
fn supported(expression: &Expr, problem: &Fragment<'_>, variables: &BTreeSet<&str>) -> bool {
    match expression {
        Expr::Num(_) | Expr::Bool(_) => true,
        Expr::Var(name) => {
            variables.contains(name.as_str())
                || problem.constants.contains_key(name)
                || problem.members.contains(name)
        }
        Expr::Binary { op, left, right } => {
            [
                "and", "or", "=>", "==", "!=", "<", "<=", ">", ">=", "+", "-", "*", "/", "%",
            ]
            .contains(&op.as_str())
                && supported(left, problem, variables)
                && supported(right, problem, variables)
        }
        Expr::Not(value) | Expr::Neg(value) => supported(value, problem, variables),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => [condition, then_expr, else_expr]
            .iter()
            .all(|part| supported(part, problem, variables)),
        _ => false,
    }
}

/// The value of `expression` under `assignment`; `None` where an operation
/// is undefined (a division by zero, an overflow) or ill-typed.
fn evaluate(
    expression: &Expr,
    problem: &Fragment<'_>,
    assignment: &BTreeMap<&str, &FiniteValue>,
) -> Option<FiniteValue> {
    Some(match expression {
        Expr::Num(value) => FiniteValue::Int(*value),
        Expr::Bool(value) => FiniteValue::Bool(*value),
        Expr::Var(name) => {
            if let Some(value) = assignment.get(name.as_str()) {
                (*value).clone()
            } else if let Some(value) = problem.constants.get(name) {
                value.clone()
            } else {
                FiniteValue::Member(name.clone())
            }
        }
        Expr::Binary { op, left, right } => binary_value(
            op,
            evaluate(left, problem, assignment)?,
            evaluate(right, problem, assignment)?,
        )?,
        Expr::Not(value) => match evaluate(value, problem, assignment)? {
            FiniteValue::Bool(value) => FiniteValue::Bool(!value),
            _ => return None,
        },
        Expr::Neg(value) => match evaluate(value, problem, assignment)? {
            FiniteValue::Int(value) => FiniteValue::Int(value.checked_neg()?),
            _ => return None,
        },
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => match evaluate(condition, problem, assignment)? {
            FiniteValue::Bool(true) => evaluate(then_expr, problem, assignment)?,
            FiniteValue::Bool(false) => evaluate(else_expr, problem, assignment)?,
            _ => return None,
        },
        _ => return None,
    })
}

impl GuardProblem {
    fn fragment(&self) -> Fragment<'_> {
        Fragment {
            constants: &self.constants,
            members: &self.members,
        }
    }

    /// The number of assignments, or `None` when a variable is outside the
    /// finite fragment.
    #[must_use]
    pub fn size(&self) -> Option<u128> {
        self.variables.as_ref().map(|variables| {
            variables
                .iter()
                .map(|(_, domain)| domain.size())
                .fold(1u128, u128::saturating_mul)
        })
    }

    /// Whether every guard stays inside the finite fragment.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        let Some(variables) = &self.variables else {
            return false;
        };
        let names = variables
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<BTreeSet<_>>();
        self.guards
            .iter()
            .all(|guard| supported(&guard.condition, &self.fragment(), &names))
    }

    fn satisfiable(&self, guards: &[usize]) -> bool {
        let variables = self.variables.as_deref().unwrap_or_default();
        let domains = variables
            .iter()
            .map(|(_, domain)| domain.values())
            .collect::<Vec<_>>();
        let mut indexes = vec![0usize; domains.len()];
        if domains.iter().any(Vec::is_empty) {
            return false;
        }
        loop {
            let assignment = variables
                .iter()
                .zip(&domains)
                .zip(&indexes)
                .map(|(((name, _), values), index)| (name.as_str(), &values[*index]))
                .collect::<BTreeMap<_, _>>();
            if guards.iter().all(|guard| {
                evaluate(
                    &self.guards[*guard].condition,
                    &self.fragment(),
                    &assignment,
                ) == Some(FiniteValue::Bool(true))
            }) {
                return true;
            }
            let mut position = 0;
            loop {
                if position == indexes.len() {
                    return false;
                }
                indexes[position] += 1;
                if indexes[position] < domains[position].len() {
                    break;
                }
                indexes[position] = 0;
                position += 1;
            }
        }
    }

    /// Decide the problem by enumerating every assignment when there are at
    /// most `bound` of them.
    #[must_use]
    pub fn decide_by_enumeration(&self, bound: u128) -> GuardDecision {
        if !self.is_supported() {
            return GuardDecision::Unsupported;
        }
        if self.size().is_none_or(|size| size > bound) {
            return GuardDecision::BeyondBound;
        }
        let mut kept = (0..self.guards.len()).collect::<Vec<_>>();
        if self.satisfiable(&kept) {
            return GuardDecision::Satisfiable;
        }
        let mut index = 0;
        while index < kept.len() {
            let without = kept
                .iter()
                .enumerate()
                .filter(|(position, _)| *position != index)
                .map(|(_, guard)| *guard)
                .collect::<Vec<_>>();
            if self.satisfiable(&without) {
                index += 1;
            } else {
                kept = without;
            }
        }
        GuardDecision::Unsatisfiable(kept)
    }

    /// The diagnostic for an unsatisfiable `core` of this problem's guards.
    #[must_use]
    pub fn unsatisfiable_message(&self, core: &[usize]) -> String {
        let rules = core
            .iter()
            .map(|index| {
                let guard = &self.guards[*index];
                format!("{} `{}`", guard.requirement, guard.detail)
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "event '{}' can never occur: when {} is {}, no value of its fields and inputs \
             satisfies the rules {rules}",
            self.event, self.entity, self.stage
        )
    }
}

/// The type domains and constants of a requirements document.
struct Declarations {
    domains: BTreeMap<String, FiniteDomain>,
    constants: BTreeMap<String, FiniteValue>,
    members: BTreeSet<String>,
}

fn constant_value(
    expression: &Expr,
    constants: &BTreeMap<String, FiniteValue>,
) -> Option<FiniteValue> {
    let fragment = Fragment {
        constants,
        members: &BTreeSet::new(),
    };
    if !supported(expression, &fragment, &BTreeSet::new()) {
        return None;
    }
    evaluate(expression, &fragment, &BTreeMap::new())
}

fn int_range(
    lo: &Expr,
    hi: &Expr,
    constants: &BTreeMap<String, FiniteValue>,
) -> Option<FiniteDomain> {
    match (
        constant_value(lo, constants)?,
        constant_value(hi, constants)?,
    ) {
        (FiniteValue::Int(lo), FiniteValue::Int(hi)) => Some(FiniteDomain::Int { lo, hi }),
        _ => None,
    }
}

fn declarations(requirements: &SurfaceRequirements) -> Declarations {
    let mut constants = BTreeMap::new();
    let mut domains = BTreeMap::from([("Bool".to_owned(), FiniteDomain::Bool)]);
    let mut members = BTreeSet::new();
    let mut numbers = Vec::new();
    let mut entities = Vec::new();
    let mut instances = BTreeMap::new();
    let mut values = BTreeMap::new();
    for item in &requirements.items {
        match item {
            RequirementsItem::Common(SpecItem::Const { name, value, .. }) => {
                if let Some(value) = constant_value(value, &constants) {
                    constants.insert(name.clone(), value);
                }
            }
            RequirementsItem::Common(SpecItem::Enum {
                name,
                members: declared,
                ..
            }) => {
                members.extend(declared.iter().cloned());
                domains.insert(name.clone(), FiniteDomain::Members(declared.clone()));
            }
            RequirementsItem::Common(SpecItem::Type { name, lo, hi, .. }) => {
                if let Some(domain) = int_range(lo, hi, &constants) {
                    domains.insert(name.clone(), domain);
                }
            }
            RequirementsItem::Common(SpecItem::Number(name, _)) => numbers.push(name.clone()),
            RequirementsItem::Common(SpecItem::Entity(name, _)) => entities.push(name.clone()),
            RequirementsItem::Process(BusinessItem::Process { name, items, .. }) => {
                entities.push(name.name().to_owned());
                for process_item in items {
                    if let ProcessItem::Stages(stages, _) = process_item {
                        members.extend(stages.iter().cloned());
                    }
                }
            }
            RequirementsItem::Common(SpecItem::VerifyBounds { items, .. }) => {
                for bound in items {
                    match bound {
                        VerifyItem::Instances(name, count, _) => {
                            instances.insert(name.clone(), *count);
                        }
                        VerifyItem::Values(name, lo, hi, _) => {
                            values.insert(name.clone(), (lo.as_ref().clone(), hi.as_ref().clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for number in numbers {
        if let Some((lo, hi)) = values.get(&number)
            && let Some(domain) = int_range(lo, hi, &constants)
        {
            domains.insert(number, domain);
        }
    }
    for entity in entities {
        if let Some(count) = instances.get(&entity) {
            domains.insert(
                entity,
                FiniteDomain::Int {
                    lo: 0,
                    hi: count - 1,
                },
            );
        }
    }
    Declarations {
        domains,
        constants,
        members,
    }
}

/// `expression` with each bare field name of the entity spelled
/// `Entity.field`; inputs, which shadow fields of the same name, stay bare.
fn spell_fields(
    expression: Expr,
    fields: &BTreeSet<&str>,
    inputs: &BTreeSet<&str>,
    entity: &str,
) -> Expr {
    let recurse = |expression: Expr| spell_fields(expression, fields, inputs, entity);
    let boxed = |expression: Box<Expr>| Box::new(recurse(*expression));
    match expression {
        Expr::Var(name) if fields.contains(name.as_str()) && !inputs.contains(name.as_str()) => {
            Expr::Var(format!("{entity}.{name}"))
        }
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: boxed(left),
            right: boxed(right),
        },
        Expr::Not(value) => Expr::Not(boxed(value)),
        Expr::Neg(value) => Expr::Neg(boxed(value)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => Expr::Conditional {
            condition: boxed(condition),
            then_expr: boxed(then_expr),
            else_expr: boxed(else_expr),
            spans,
        },
        other => other,
    }
}

/// One [`GuardProblem`] per event a rule guards, in document order.
#[must_use]
pub fn rule_guard_problems(requirements: &SurfaceRequirements) -> Vec<GuardProblem> {
    if requirements.next.is_none() {
        return Vec::new();
    }
    let declarations = declarations(requirements);
    let mut problems = Vec::new();
    for item in &requirements.items {
        let RequirementsItem::Process(BusinessItem::Process {
            name,
            fields,
            items,
            ..
        }) = item
        else {
            continue;
        };
        let fields = fields
            .as_ref()
            .map(|fields| fields.fields.as_slice())
            .unwrap_or_default();
        for process_item in items {
            if let ProcessItem::Transition(transition) = process_item
                && transition
                    .event
                    .as_ref()
                    .is_some_and(|event| !event.rule_guards.is_empty())
            {
                problems.push(event_problem(
                    &declarations,
                    name.name(),
                    fields,
                    transition,
                ));
            }
        }
    }
    problems
}

/// The [`GuardProblem`] of one event with rule guards.
fn event_problem(
    declarations: &Declarations,
    entity: &str,
    fields: &[fsl_syntax::ProcessField],
    transition: &fsl_syntax::ProcessTransition,
) -> GuardProblem {
    let inputs = transition
        .inputs
        .iter()
        .map(|input| match input {
            Param::Typed(name, type_name) => (name.as_str(), Some(type_name.name.as_str())),
            Param::Range(name, _, _) => (name.as_str(), None),
        })
        .collect::<Vec<_>>();
    let input_names = inputs
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    let field_names = fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    let guards = transition
        .event
        .iter()
        .flat_map(|event| &event.rule_guards)
        .map(|guard| ProblemGuard {
            requirement: guard.requirement.clone(),
            detail: guard.detail.clone(),
            condition: spell_fields(guard.condition.clone(), &field_names, &input_names, entity),
        })
        .collect::<Vec<_>>();
    let mentioned = guards
        .iter()
        .flat_map(|guard| variable_names(&guard.condition))
        .collect::<BTreeSet<_>>();
    let variables = fields
        .iter()
        .map(|field| {
            (
                format!("{entity}.{}", field.name),
                Some(field.type_name.name.as_str()),
            )
        })
        .chain(
            inputs
                .iter()
                .map(|(name, type_name)| ((*name).to_owned(), *type_name)),
        )
        .filter(|(name, _)| mentioned.contains(name))
        .map(|(name, type_name)| {
            type_name
                .and_then(|type_name| declarations.domains.get(type_name))
                .map(|domain| (name, domain.clone()))
        })
        .collect::<Option<Vec<_>>>();
    let mut constants = declarations.constants.clone();
    constants.insert(
        stage_reference(entity),
        FiniteValue::Member(transition.source.clone()),
    );
    GuardProblem {
        event: transition.name.clone(),
        entity: entity.to_owned(),
        stage: transition.source.clone(),
        span: transition.span,
        variables,
        constants,
        members: declarations.members.clone(),
        guards,
    }
}

fn variable_names(expression: &Expr) -> Vec<String> {
    match expression {
        Expr::Var(name) => vec![name.clone()],
        Expr::Binary { left, right, .. } => {
            let mut names = variable_names(left);
            names.extend(variable_names(right));
            names
        }
        Expr::Not(value) | Expr::Neg(value) => variable_names(value),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => [condition, then_expr, else_expr]
            .iter()
            .flat_map(|part| variable_names(part))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str) -> Expr {
        Expr::Var(name.to_owned())
    }

    fn binary(op: &str, left: Expr, right: Expr) -> Expr {
        Expr::Binary {
            op: op.to_owned(),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn problem(domain: FiniteDomain, guards: Vec<Expr>) -> GuardProblem {
        let position = fsl_syntax::SourcePos {
            offset: 0,
            line: 1,
            column: 1,
        };
        GuardProblem {
            event: "e".to_owned(),
            entity: "E".to_owned(),
            stage: "S".to_owned(),
            span: Span {
                start: position,
                end: position,
            },
            variables: Some(vec![("E.x".to_owned(), domain)]),
            constants: BTreeMap::from([("LIMIT".to_owned(), FiniteValue::Int(1))]),
            members: BTreeSet::new(),
            guards: guards
                .into_iter()
                .enumerate()
                .map(|(index, condition)| ProblemGuard {
                    requirement: format!("R{index}"),
                    detail: String::new(),
                    condition,
                })
                .collect(),
        }
    }

    #[test]
    fn a_minimal_conflicting_subset_is_reported() {
        let guards = vec![
            binary(">", var("E.x"), var("LIMIT")),
            binary(">=", var("E.x"), Expr::Num(0)),
            binary("<=", var("E.x"), var("LIMIT")),
        ];
        assert_eq!(
            problem(FiniteDomain::Int { lo: 0, hi: 3 }, guards).decide_by_enumeration(10),
            GuardDecision::Unsatisfiable(vec![0, 2])
        );
    }

    #[test]
    fn a_satisfiable_conjunction_and_the_bound_and_the_fragment() {
        let guards = vec![binary(">", var("E.x"), var("LIMIT"))];
        let domain = FiniteDomain::Int { lo: 0, hi: 3 };
        assert_eq!(
            problem(domain.clone(), guards.clone()).decide_by_enumeration(10),
            GuardDecision::Satisfiable
        );
        assert_eq!(
            problem(domain.clone(), guards).decide_by_enumeration(3),
            GuardDecision::BeyondBound
        );
        let outside = vec![Expr::Index(Box::new(var("m")), Box::new(var("E.x")))];
        assert_eq!(
            problem(domain, outside).decide_by_enumeration(10),
            GuardDecision::Unsupported
        );
    }

    #[test]
    fn a_domain_beyond_the_bound_is_not_enumerated() {
        let guards = vec![binary("<", var("E.x"), Expr::Num(0))];
        let huge = FiniteDomain::Int {
            lo: 0,
            hi: 2_000_000,
        };
        assert_eq!(
            problem(huge, guards).decide_by_enumeration(ENUMERATION_BOUND),
            GuardDecision::BeyondBound
        );
    }
}
