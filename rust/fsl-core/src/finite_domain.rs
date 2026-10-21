// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The one finite-domain enumeration of the next-edition requirements checks
//! (decisions/048 J3, J10): decision tables ([`crate::decision_table`]) and
//! rule guards ([`crate::rule_guards`]) ask their questions over the types'
//! domains, not over the reachable states, and both answer them here.
//!
//! A question is decided by enumerating the values of the names it reads
//! (fields spelled `Entity.field`, event inputs) in fsl-core, so the
//! language server and the browser Worker report the same diagnostics as the
//! CLI. One whose domain product exceeds [`DOMAIN_BOUND`], or whose
//! expressions read something the enumeration does not evaluate, is deferred:
//! [`solver_obligations`] hands it to native `check`, which asks the solver
//! the same question over the same domains.

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{Expr, QualifiedName, SurfaceRequirements};
use serde_json::Value;

use crate::{CoreError, FslValue, KernelModel, KernelSpec, TypeDef, TypeRef};

/// The largest product of domain sizes enumerated; a larger question goes to
/// the solver.
pub const DOMAIN_BOUND: u128 = 1_000_000;

/// One value per enumerated name.
pub(crate) type Assignment = BTreeMap<String, FslValue>;

/// The names one enumeration ranges over, each with its domain.
pub(crate) type Domains = Vec<(String, Vec<FslValue>)>;

pub(crate) fn display(value: &FslValue) -> String {
    match value {
        FslValue::Int(value) => value.to_string(),
        FslValue::Bool(value) => value.to_string(),
        FslValue::Enum { member, .. } => member.clone(),
        other => format!("{other:?}"),
    }
}

/// The finite type `type_name` names, the type of `what`.
fn finite_type(type_name: &QualifiedName, what: &str) -> Result<TypeRef, String> {
    Ok(match type_name.name.as_str() {
        "Bool" if type_name.namespace.is_none() => TypeRef::Bool,
        "Int" if type_name.namespace.is_none() => {
            return Err(format!("{what} is an unbounded Int"));
        }
        _ => TypeRef::Named(type_name.namespace.as_ref().map_or_else(
            || type_name.name.clone(),
            |namespace| format!("{namespace}.{}", type_name.name),
        )),
    })
}

/// The number of values of `ty`, counted from its definition without
/// listing them.
fn size(model: &KernelModel, ty: &TypeRef, what: &str) -> Result<u128, String> {
    let span = |lo: i64, hi: i64| u128::try_from(i128::from(hi) - i128::from(lo) + 1).unwrap_or(0);
    match ty {
        TypeRef::Bool => Ok(2),
        TypeRef::Range(lo, hi) => Ok(span(*lo, *hi)),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => Ok(span(*lo, *hi)),
            Some(TypeDef::Enum { members, .. }) => Ok(members.len() as u128),
            _ => Err(format!("{what} has no finite domain")),
        },
        _ => Err(format!("{what} has no finite domain")),
    }
}

/// The domains of `names` (each a name, its type and what it is), for one
/// enumeration over `what`.
///
/// The product of the domain sizes is counted from the types first, with
/// overflow checked, and nothing is listed when it exceeds [`DOMAIN_BOUND`]:
/// such a question goes to the solver without the enumeration allocating
/// its domains.
///
/// # Errors
///
/// Says why the names cannot be enumerated: a type without a finite domain,
/// an empty domain, or a product past the bound.
pub(crate) fn domains(
    model: &KernelModel,
    names: &[(String, &QualifiedName, String)],
    what: &str,
) -> Result<Domains, String> {
    let types = names
        .iter()
        .map(|(_, type_name, of)| finite_type(type_name, of))
        .collect::<Result<Vec<_>, _>>()?;
    let mut product = Some(1u128);
    for ((_, _, of), ty) in names.iter().zip(&types) {
        let values = size(model, ty, of)?;
        product = product.and_then(|product| product.checked_mul(values));
    }
    match product {
        None => {
            return Err(format!(
                "its {what} take more than {} values, more than the {DOMAIN_BOUND} enumerated",
                u128::MAX
            ));
        }
        Some(product) if product > DOMAIN_BOUND => {
            return Err(format!(
                "its {what} take {product} values, more than the {DOMAIN_BOUND} enumerated"
            ));
        }
        Some(0) => return Err("a domain is empty".to_owned()),
        Some(_) => {}
    }
    names
        .iter()
        .zip(&types)
        .map(|((name, _, of), ty)| {
            model
                .domain_values(ty)
                .map(|values| (name.clone(), values))
                .map_err(|_| format!("{of} has no finite domain"))
        })
        .collect()
}

/// Visit every assignment of `domains` (the last name fastest, from each
/// domain's first value), each extended with `fixed`, until `visit` returns
/// `false`.
///
/// # Errors
///
/// Returns the first error `visit` returns.
pub(crate) fn enumerate(
    domains: &Domains,
    fixed: &Assignment,
    mut visit: impl FnMut(&Assignment) -> Result<bool, String>,
) -> Result<(), String> {
    let mut assignment = fixed.clone();
    for (name, values) in domains {
        assignment.insert(name.clone(), values[0].clone());
    }
    let mut odometer = vec![0usize; domains.len()];
    loop {
        if !visit(&assignment)? {
            return Ok(());
        }
        let mut position = domains.len();
        loop {
            if position == 0 {
                return Ok(());
            }
            position -= 1;
            odometer[position] += 1;
            let (name, values) = &domains[position];
            if odometer[position] < values.len() {
                assignment.insert(name.clone(), values[odometer[position]].clone());
                break;
            }
            odometer[position] = 0;
            assignment.insert(name.clone(), values[0].clone());
        }
    }
}

/// Record the variables (names `is_variable` accepts) and constants
/// `expression` reads, or say why it cannot be enumerated.
pub(crate) fn names(
    expression: &Expr,
    is_variable: &dyn Fn(&str) -> bool,
    model: &KernelModel,
    variables: &mut BTreeSet<String>,
    constants: &mut BTreeMap<String, FslValue>,
) -> Result<(), String> {
    let mut recurse =
        |expression: &Expr| names(expression, is_variable, model, variables, constants);
    match expression {
        Expr::Num(_) | Expr::Bool(_) | Expr::EnumMember { .. } => Ok(()),
        Expr::Var(name) if is_variable(name) => {
            variables.insert(name.clone());
            Ok(())
        }
        Expr::Var(name) => {
            if let Some(value) = model.consts.get(name) {
                constants.insert(name.clone(), value.clone());
                Ok(())
            } else if model.enum_members.contains_key(name) {
                Ok(())
            } else {
                Err(format!(
                    "it reads '{name}', which is not a field of the entity"
                ))
            }
        }
        Expr::Binary { left, right, .. } | Expr::BinaryNamed { left, right, .. } => {
            recurse(left)?;
            recurse(right)
        }
        Expr::Neg(value) | Expr::Not(value) | Expr::UnaryNamed { expr: value, .. } => {
            recurse(value)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            recurse(condition)?;
            recurse(then_expr)?;
            recurse(else_expr)
        }
        _ => Err("its condition uses an expression the enumeration does not evaluate".to_owned()),
    }
}

/// `expression` at one assignment.
///
/// # Errors
///
/// Says why the expression has no value there.
pub(crate) fn evaluate(
    expression: &Expr,
    assignment: &Assignment,
    model: &KernelModel,
) -> Result<FslValue, String> {
    let recurse = |expression: &Expr| evaluate(expression, assignment, model);
    let int = |value: FslValue| match value {
        FslValue::Int(value) => Ok(value),
        other => Err(format!("expected an integer, found {other:?}")),
    };
    let boolean = |value: FslValue| match value {
        FslValue::Bool(value) => Ok(value),
        other => Err(format!("expected a Boolean, found {other:?}")),
    };
    let overflow = || "integer overflow".to_owned();
    Ok(match expression {
        Expr::Num(value) => FslValue::Int(*value),
        Expr::Bool(value) => FslValue::Bool(*value),
        Expr::EnumMember { type_name, member } => FslValue::Enum {
            type_name: type_name.clone(),
            member: member.clone(),
        },
        Expr::Var(name) => assignment
            .get(name)
            .or_else(|| model.consts.get(name))
            .or_else(|| model.enum_members.get(name))
            .cloned()
            .ok_or_else(|| format!("'{name}' has no value"))?,
        Expr::Neg(value) => {
            FslValue::Int(int(recurse(value)?)?.checked_neg().ok_or_else(overflow)?)
        }
        Expr::Not(value) => FslValue::Bool(!boolean(recurse(value)?)?),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            if boolean(recurse(condition)?)? {
                recurse(then_expr)?
            } else {
                recurse(else_expr)?
            }
        }
        Expr::UnaryNamed { name, expr, .. } if name == "abs" => {
            FslValue::Int(int(recurse(expr)?)?.checked_abs().ok_or_else(overflow)?)
        }
        Expr::BinaryNamed { name, left, right } if name == "min" || name == "max" => {
            let (left, right) = (int(recurse(left)?)?, int(recurse(right)?)?);
            FslValue::Int(if name == "min" {
                left.min(right)
            } else {
                left.max(right)
            })
        }
        Expr::Binary { op, left, right } => match op.as_str() {
            "and" => FslValue::Bool(boolean(recurse(left)?)? && boolean(recurse(right)?)?),
            "or" => FslValue::Bool(boolean(recurse(left)?)? || boolean(recurse(right)?)?),
            "=>" => FslValue::Bool(!boolean(recurse(left)?)? || boolean(recurse(right)?)?),
            "==" => FslValue::Bool(recurse(left)? == recurse(right)?),
            "!=" => FslValue::Bool(recurse(left)? != recurse(right)?),
            _ => {
                let (left, right) = (int(recurse(left)?)?, int(recurse(right)?)?);
                match op.as_str() {
                    "<" => FslValue::Bool(left < right),
                    "<=" => FslValue::Bool(left <= right),
                    ">" => FslValue::Bool(left > right),
                    ">=" => FslValue::Bool(left >= right),
                    "+" => FslValue::Int(left.checked_add(right).ok_or_else(overflow)?),
                    "-" => FslValue::Int(left.checked_sub(right).ok_or_else(overflow)?),
                    "*" => FslValue::Int(left.checked_mul(right).ok_or_else(overflow)?),
                    "/" if right != 0 => FslValue::Int(left.div_euclid(right)),
                    "%" if right != 0 => FslValue::Int(left.rem_euclid(right)),
                    _ => return Err(format!("operator '{op}' is not evaluated here")),
                }
            }
        },
        _ => return Err("expression is not evaluated here".to_owned()),
    })
}

/// Run the enumerated checks of a lowered requirements document's decision
/// tables and rule guards over one model, returning their warnings.
///
/// # Errors
///
/// Returns the first table or guard the enumeration refutes.
pub(crate) fn check(
    tables: &crate::decision_table::Tables,
    guards: &crate::rule_guards::RuleGuards,
    kernel: &KernelSpec,
) -> Result<Vec<Value>, CoreError> {
    if tables.is_empty() && guards.is_empty() {
        return Ok(Vec::new());
    }
    // A model error is reported by the stage that builds the model.
    let Ok(model) = crate::build_model(kernel.clone()) else {
        return Ok(Vec::new());
    };
    let mut warnings = tables.check(&model)?;
    warnings.extend(guards.check(&model)?);
    Ok(warnings)
}

/// One question the enumeration deferred, for the solver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolverObligation {
    /// A decision-table check: the spec is rejected when the property is
    /// refuted.
    Table(crate::decision_table::DecisionObligation),
    /// An event's guards: the spec is rejected when the property (no value
    /// satisfies them) holds for a minimal subset with a rule's guard.
    Guards(crate::rule_guards::GuardConflict),
}

/// The checks of `requirements` that enumeration could not decide, for the
/// native `check` to discharge with the solver.
#[must_use]
pub fn solver_obligations(
    requirements: &SurfaceRequirements,
    model: &KernelModel,
) -> Vec<SolverObligation> {
    crate::decision_table::Tables::of(requirements)
        .obligations(model)
        .into_iter()
        .map(SolverObligation::Table)
        .chain(
            crate::rule_guards::RuleGuards::of(requirements)
                .obligations(model)
                .into_iter()
                .map(SolverObligation::Guards),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(types: &str) -> KernelModel {
        let source =
            format!("spec M {{\n{types}\n  state {{ x: Bool }}\n  init {{ x = false }}\n}}\n");
        crate::build_model(
            crate::parse_kernel_source(&source, &crate::FsResolver::new(".")).expect("kernel"),
        )
        .expect("model")
    }

    fn named(name: &str) -> QualifiedName {
        QualifiedName {
            namespace: None,
            name: name.to_owned(),
        }
    }

    /// decisions/048 J3: the bound is decided from the types' sizes before
    /// any value is listed, so a product past it (here one no memory could
    /// hold, and one whose product overflows) is refused at once.
    #[test]
    fn a_product_past_the_bound_is_refused_before_any_value_is_listed() {
        let model = model("  type Huge = 0..9223372036854775806\n  type Small = 0..1");
        let huge = named("Huge");
        let small = named("Small");
        let refused = domains(
            &model,
            &[("h".to_owned(), &huge, "field 'h'".to_owned())],
            "fields",
        )
        .expect_err("past the bound");
        assert_eq!(
            refused,
            format!(
                "its fields take 9223372036854775807 values, more than the {DOMAIN_BOUND} enumerated"
            )
        );
        let overflowing = (0..3)
            .map(|index| (format!("h{index}"), &huge, format!("field 'h{index}'")))
            .collect::<Vec<_>>();
        let refused = domains(&model, &overflowing, "fields").expect_err("overflow");
        assert!(
            refused.starts_with("its fields take more than"),
            "{refused}"
        );
        let listed = domains(
            &model,
            &[("s".to_owned(), &small, "field 's'".to_owned())],
            "fields",
        )
        .expect("within the bound");
        assert_eq!(
            listed,
            vec![("s".to_owned(), vec![FslValue::Int(0), FslValue::Int(1)])]
        );
    }
}
