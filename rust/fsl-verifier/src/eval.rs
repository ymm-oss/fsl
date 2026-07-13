// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{
    FslValue, KernelBinder as Binder, KernelExpr as Expr, KernelModel, Pattern, TypeDef, TypeRef,
};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::value::{
    Bindings, SymbolicState, SymbolicValue, bool_term, coerce, concrete_value, i64_index, int_term,
    ite_value, logical_equal, select_finite,
};

type BinderCandidates<T> = Vec<(String, SymbolicValue<T>)>;

#[allow(clippy::too_many_lines)]
pub(crate) fn eval<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match expr {
        Expr::Num(value) => Ok(SymbolicValue::Scalar {
            ty: TypeRef::Int,
            term: solver.int_value(*value),
        }),
        Expr::Bool(value) => Ok(SymbolicValue::Scalar {
            ty: TypeRef::Bool,
            term: solver.bool_value(*value),
        }),
        Expr::None => Ok(SymbolicValue::None),
        Expr::Some(inner) => {
            let value = eval(solver, model, inner, state, bindings, old_state)?;
            let inner_ty = value
                .ty()
                .cloned()
                .ok_or_else(|| VerifyError::new("some() requires a typed value"))?;
            Ok(SymbolicValue::Option {
                ty: TypeRef::Option(Box::new(inner_ty)),
                present: solver.bool_value(true),
                value: Box::new(value),
            })
        }
        Expr::Set(items) => Ok(SymbolicValue::SetLiteral(
            items
                .iter()
                .map(|item| eval(solver, model, item, state, bindings, old_state))
                .collect::<Result<_, _>>()?,
        )),
        Expr::Seq(items) => Ok(SymbolicValue::SeqLiteral(
            items
                .iter()
                .map(|item| eval(solver, model, item, state, bindings, old_state))
                .collect::<Result<_, _>>()?,
        )),
        Expr::Struct { name, fields } => {
            eval_struct_literal(solver, model, name, fields, state, bindings, old_state)
        }
        Expr::Var(name) => lookup(solver, model, name, state, bindings),
        Expr::Call { name, .. } => Err(VerifyError::new(format!(
            "unexpanded predicate call '{name}'"
        ))),
        Expr::Index(base, index) => {
            let base = eval(solver, model, base, state, bindings, old_state)?;
            let index = eval(solver, model, index, state, bindings, old_state)?;
            eval_index(solver, model, &base, &index)
        }
        Expr::Field(base, field) => {
            let base = eval(solver, model, base, state, bindings, old_state)?;
            let SymbolicValue::Struct { fields, .. } = base else {
                return Err(VerifyError::new("field access requires a struct"));
            };
            fields
                .get(field)
                .cloned()
                .ok_or_else(|| VerifyError::new(format!("unknown struct field '{field}'")))
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let receiver = eval(solver, model, receiver, state, bindings, old_state)?;
            eval_method(
                solver, model, receiver, name, args, state, bindings, old_state,
            )
        }
        Expr::Binary { op, left, right } => {
            eval_binary(solver, model, op, left, right, state, bindings, old_state)
        }
        Expr::Neg(inner) => {
            let value = eval(solver, model, inner, state, bindings, old_state)?;
            Ok(SymbolicValue::Scalar {
                ty: TypeRef::Int,
                term: solver.neg(int_term(&value)?)?,
            })
        }
        Expr::Not(inner) => {
            let value = eval(solver, model, inner, state, bindings, old_state)?;
            Ok(bool_value(solver, solver.not(bool_term(&value)?)?))
        }
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => {
            let condition = eval(solver, model, condition, state, bindings, old_state)?;
            let then_value = eval(solver, model, then_expr, state, bindings, old_state)?;
            let else_value = eval(solver, model, else_expr, state, bindings, old_state)?;
            ite_value(
                solver,
                model,
                bool_term(&condition)?,
                &then_value,
                &else_value,
            )
        }
        Expr::Is { expr, pattern } => {
            let value = eval(solver, model, expr, state, bindings, old_state)?;
            eval_pattern(solver, value, pattern, bindings)
        }
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => eval_quantified(
            solver, model, quantifier, binder, body, state, bindings, old_state,
        ),
        Expr::Count {
            name,
            type_name,
            condition,
        } => {
            let ty = qualified_type(type_name.namespace.as_deref(), &type_name.name)?;
            let mut terms = Vec::new();
            for value in model.domain_values(&TypeRef::Named(ty.clone()))? {
                let mut local = bindings.clone();
                local.insert(
                    name.clone(),
                    concrete_value(solver, model, &TypeRef::Named(ty.clone()), &value)?,
                );
                let condition = eval(solver, model, condition, state, &mut local, old_state)?;
                terms.push(solver.ite(
                    bool_term(&condition)?,
                    &solver.int_value(1),
                    &solver.int_value(0),
                )?);
            }
            Ok(int_value(solver, sum_terms(solver, &terms)?))
        }
        Expr::Sum {
            name,
            type_name,
            body,
            condition,
        } => {
            let ty = qualified_type(type_name.namespace.as_deref(), &type_name.name)?;
            let mut terms = Vec::new();
            for value in model.domain_values(&TypeRef::Named(ty.clone()))? {
                let mut local = bindings.clone();
                local.insert(
                    name.clone(),
                    concrete_value(solver, model, &TypeRef::Named(ty.clone()), &value)?,
                );
                let body = eval(solver, model, body, state, &mut local, old_state)?;
                let term = if let Some(condition) = condition {
                    let condition = eval(solver, model, condition, state, &mut local, old_state)?;
                    solver.ite(
                        bool_term(&condition)?,
                        int_term(&body)?,
                        &solver.int_value(0),
                    )?
                } else {
                    int_term(&body)?.clone()
                };
                terms.push(term);
            }
            Ok(int_value(solver, sum_terms(solver, &terms)?))
        }
        Expr::UnaryNamed { name, expr, .. } => match name.as_str() {
            "old" => eval(
                solver,
                model,
                expr,
                old_state.ok_or_else(|| VerifyError::new("old() used without old state"))?,
                bindings,
                None,
            ),
            "abs" => {
                let value = eval(solver, model, expr, state, bindings, old_state)?;
                let term = int_term(&value)?;
                let nonnegative = solver.ge(term, &solver.int_value(0))?;
                Ok(int_value(
                    solver,
                    solver.ite(&nonnegative, term, &solver.neg(term)?)?,
                ))
            }
            _ => Err(VerifyError::new(format!(
                "unsupported unary expression '{name}'"
            ))),
        },
        Expr::BinaryNamed { name, left, right } => {
            let left = eval(solver, model, left, state, bindings, old_state)?;
            let right = eval(solver, model, right, state, bindings, old_state)?;
            let condition = match name.as_str() {
                "min" => solver.le(int_term(&left)?, int_term(&right)?)?,
                "max" => solver.ge(int_term(&left)?, int_term(&right)?)?,
                _ => {
                    return Err(VerifyError::new(format!(
                        "unsupported binary expression '{name}'"
                    )));
                }
            };
            Ok(int_value(
                solver,
                solver.ite(&condition, int_term(&left)?, int_term(&right)?)?,
            ))
        }
        Expr::TernaryNamed { name, .. } => Err(VerifyError::new(format!(
            "unsupported ternary expression '{name}'"
        ))),
        Expr::BinderNamed { name, binder } => {
            if name != "unique" && name != "exactly_one" {
                return Err(VerifyError::new(format!(
                    "unsupported binder expression '{name}'"
                )));
            }
            let terms = binder_conditions(solver, model, binder, state, bindings, old_state)?;
            let counts = terms
                .iter()
                .map(|term| {
                    solver
                        .ite(term, &solver.int_value(1), &solver.int_value(0))
                        .map_err(VerifyError::from)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let count = sum_terms(solver, &counts)?;
            let condition = if name == "unique" {
                solver.le(&count, &solver.int_value(1))?
            } else {
                solver.equal(&count, &solver.int_value(1))?
            };
            Ok(bool_value(solver, condition))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_struct_literal<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    name: &str,
    fields: &[(String, Expr)],
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let Some(TypeDef::Struct { fields: expected }) = model.types.get(name) else {
        return Err(VerifyError::new(format!("unknown struct type '{name}'")));
    };
    let expressions = fields.iter().cloned().collect::<BTreeMap<_, _>>();
    Ok(SymbolicValue::Struct {
        ty: TypeRef::Named(name.to_owned()),
        fields: expected
            .iter()
            .map(|(field, ty)| {
                let expr = expressions
                    .get(field)
                    .ok_or_else(|| VerifyError::new(format!("missing struct field '{field}'")))?;
                let value = eval(solver, model, expr, state, bindings, old_state)?;
                Ok((field.clone(), coerce(solver, model, value, ty)?))
            })
            .collect::<Result<_, VerifyError>>()?,
    })
}

fn lookup<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    name: &str,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if let Some(value) = bindings.get(name).or_else(|| state.get(name)) {
        return Ok(value.clone());
    }
    if let Some(value) = model.consts.get(name) {
        let ty = match value {
            FslValue::Int(_) => TypeRef::Int,
            FslValue::Bool(_) => TypeRef::Bool,
            _ => return Err(VerifyError::new(format!("unsupported const '{name}'"))),
        };
        return concrete_value(solver, model, &ty, value);
    }
    if let Some(value @ FslValue::Enum { type_name, .. }) = model.enum_members.get(name) {
        return concrete_value(solver, model, &TypeRef::Named(type_name.clone()), value);
    }
    Err(VerifyError::new(format!("unknown identifier '{name}'")))
}

fn eval_index<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    base: &SymbolicValue<S::Term>,
    index: &SymbolicValue<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match base {
        SymbolicValue::Map { ty, entries } => {
            let TypeRef::Map(key_ty, _) = ty else {
                unreachable!();
            };
            select_finite(solver, model, entries, index, key_ty)
        }
        SymbolicValue::Seq { slots, .. } => {
            let entries = slots
                .iter()
                .enumerate()
                .map(|(index, value)| Ok((FslValue::Int(i64_index(index)?), value.clone())))
                .collect::<Result<Vec<_>, VerifyError>>()?;
            select_finite(solver, model, &entries, index, &TypeRef::Int)
        }
        _ => Err(VerifyError::new("indexing requires a map or sequence")),
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn eval_method<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    receiver: SymbolicValue<S::Term>,
    name: &str,
    args: &[Expr],
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let values = args
        .iter()
        .map(|arg| eval(solver, model, arg, state, bindings, old_state))
        .collect::<Result<Vec<_>, _>>()?;
    match receiver {
        SymbolicValue::Set { ty, entries } => {
            let TypeRef::Set(element_ty) = &ty else {
                unreachable!();
            };
            match (name, values.as_slice()) {
                ("contains", [value]) => {
                    let terms = entries
                        .iter()
                        .map(|(element, present)| {
                            let element = concrete_value(solver, model, element_ty, element)?;
                            let same = logical_equal(solver, model, value, &element)?;
                            Ok(solver.and(&[same, present.clone()])?)
                        })
                        .collect::<Result<Vec<_>, VerifyError>>()?;
                    Ok(bool_value(solver, solver.or(&terms)?))
                }
                ("add" | "remove", [value]) => {
                    let added = name == "add";
                    let entries = entries
                        .into_iter()
                        .map(|(element, present)| {
                            let symbolic = concrete_value(solver, model, element_ty, &element)?;
                            let same = logical_equal(solver, model, value, &symbolic)?;
                            Ok((
                                element,
                                solver.ite(&same, &solver.bool_value(added), &present)?,
                            ))
                        })
                        .collect::<Result<_, VerifyError>>()?;
                    Ok(SymbolicValue::Set { ty, entries })
                }
                ("size", []) => {
                    let terms = entries
                        .iter()
                        .map(|(_, present)| {
                            solver
                                .ite(present, &solver.int_value(1), &solver.int_value(0))
                                .map_err(VerifyError::from)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(int_value(solver, sum_terms(solver, &terms)?))
                }
                _ => Err(VerifyError::new(format!("invalid Set method '{name}'"))),
            }
        }
        SymbolicValue::Seq { ty, slots, len } => {
            let TypeRef::Seq(element_ty, capacity) = &ty else {
                unreachable!();
            };
            match (name, values.as_slice()) {
                ("push", [value]) => {
                    let value = coerce(solver, model, value.clone(), element_ty)?;
                    let slots = slots
                        .iter()
                        .enumerate()
                        .map(|(index, old)| {
                            let matches =
                                solver.equal(&len, &solver.int_value(i64_index(index)?))?;
                            ite_value(solver, model, &matches, &value, old)
                        })
                        .collect::<Result<_, VerifyError>>()?;
                    Ok(SymbolicValue::Seq {
                        ty,
                        slots,
                        len: solver.add(&len, &solver.int_value(1))?,
                    })
                }
                ("pop", []) => {
                    let mut shifted = slots.iter().skip(1).cloned().collect::<Vec<_>>();
                    if let Some(last) = slots.last() {
                        shifted.push(last.clone());
                    }
                    Ok(SymbolicValue::Seq {
                        ty,
                        slots: shifted,
                        len: solver.sub(&len, &solver.int_value(1))?,
                    })
                }
                ("head", []) => slots
                    .first()
                    .cloned()
                    .ok_or_else(|| VerifyError::new("head() on zero-capacity sequence")),
                ("at", [index]) => {
                    let entries = slots
                        .iter()
                        .enumerate()
                        .map(|(index, value)| Ok((FslValue::Int(i64_index(index)?), value.clone())))
                        .collect::<Result<Vec<_>, VerifyError>>()?;
                    select_finite(solver, model, &entries, index, &TypeRef::Int)
                }
                ("contains", [value]) => {
                    let mut terms = Vec::new();
                    for (index, slot) in slots.iter().enumerate() {
                        let active = solver.lt(&solver.int_value(i64_index(index)?), &len)?;
                        let same = logical_equal(solver, model, slot, value)?;
                        terms.push(solver.and(&[active, same])?);
                    }
                    Ok(bool_value(solver, solver.or(&terms)?))
                }
                ("size", []) => Ok(int_value(solver, len)),
                _ => Err(VerifyError::new(format!(
                    "invalid Seq<{element_ty:?}, {capacity}> method '{name}'"
                ))),
            }
        }
        SymbolicValue::Relation { .. } => Err(VerifyError::new(
            "relation methods are not implemented in the current verifier slice",
        )),
        _ => Err(VerifyError::new(
            "method receiver has no collection methods",
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_binary<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    op: &str,
    left: &Expr,
    right: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let left = eval(solver, model, left, state, bindings, old_state)?;
    let right = eval(solver, model, right, state, bindings, old_state)?;
    match op {
        "and" => Ok(bool_value(
            solver,
            solver.and(&[bool_term(&left)?.clone(), bool_term(&right)?.clone()])?,
        )),
        "or" => Ok(bool_value(
            solver,
            solver.or(&[bool_term(&left)?.clone(), bool_term(&right)?.clone()])?,
        )),
        "=>" => Ok(bool_value(
            solver,
            solver.implies(bool_term(&left)?, bool_term(&right)?)?,
        )),
        "==" | "!=" => {
            let equal = logical_equal(solver, model, &left, &right)?;
            Ok(bool_value(
                solver,
                if op == "==" {
                    equal
                } else {
                    solver.not(&equal)?
                },
            ))
        }
        "+" | "-" | "*" | "/" | "%" => {
            let term = match op {
                "+" => solver.add(int_term(&left)?, int_term(&right)?)?,
                "-" => solver.sub(int_term(&left)?, int_term(&right)?)?,
                "*" => solver.mul(int_term(&left)?, int_term(&right)?)?,
                "/" => solver.div(int_term(&left)?, int_term(&right)?)?,
                "%" => solver.modulo(int_term(&left)?, int_term(&right)?)?,
                _ => unreachable!(),
            };
            Ok(int_value(solver, term))
        }
        "<" | "<=" | ">" | ">=" => {
            let term = match op {
                "<" => solver.lt(int_term(&left)?, int_term(&right)?)?,
                "<=" => solver.le(int_term(&left)?, int_term(&right)?)?,
                ">" => solver.gt(int_term(&left)?, int_term(&right)?)?,
                ">=" => solver.ge(int_term(&left)?, int_term(&right)?)?,
                _ => unreachable!(),
            };
            Ok(bool_value(solver, term))
        }
        _ => Err(VerifyError::new(format!("unknown operator '{op}'"))),
    }
}

fn eval_pattern<S: SmtSolver>(
    solver: &S,
    value: SymbolicValue<S::Term>,
    pattern: &Pattern,
    bindings: &mut Bindings<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match (value, pattern) {
        (SymbolicValue::None, Pattern::None) => Ok(bool_value(solver, solver.bool_value(true))),
        (SymbolicValue::None, Pattern::Some(_)) => Ok(bool_value(solver, solver.bool_value(false))),
        (SymbolicValue::Option { present, .. }, Pattern::None) => {
            Ok(bool_value(solver, solver.not(&present)?))
        }
        (SymbolicValue::Option { present, value, .. }, Pattern::Some(name)) => {
            bindings.insert(name.clone(), *value);
            Ok(bool_value(solver, present))
        }
        _ => Err(VerifyError::new("is pattern requires an Option value")),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_quantified<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    quantifier: &str,
    binder: &Binder,
    body: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let candidates = binder_candidates(solver, model, binder, state, bindings, old_state)?;
    let mut terms = Vec::new();
    for (name, value, membership) in candidates {
        let mut local = bindings.clone();
        local.insert(name, value);
        let body = eval(solver, model, body, state, &mut local, old_state)?;
        let body = bool_term(&body)?.clone();
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?;
        let condition = match (membership, where_term) {
            (Some(membership), Some(where_term)) => Some(solver.and(&[membership, where_term])?),
            (Some(condition), None) | (None, Some(condition)) => Some(condition),
            (None, None) => None,
        };
        terms.push(if let Some(condition) = condition {
            if quantifier == "forall" {
                solver.implies(&condition, &body)?
            } else {
                solver.and(&[condition, body])?
            }
        } else {
            body
        });
    }
    let term = if quantifier == "forall" {
        solver.and(&terms)?
    } else {
        solver.or(&terms)?
    };
    Ok(bool_value(solver, term))
}

fn binder_conditions<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<Vec<S::Term>, VerifyError> {
    let candidates = binder_candidates(solver, model, binder, state, bindings, old_state)?;
    let mut terms = Vec::new();
    for (name, value, membership) in candidates {
        let mut local = bindings.clone();
        local.insert(name, value);
        let where_term = binder_where(solver, model, binder, state, &mut local, old_state)?;
        terms.push(match (membership, where_term) {
            (Some(membership), Some(where_term)) => solver.and(&[membership, where_term])?,
            (Some(condition), None) | (None, Some(condition)) => condition,
            (None, None) => solver.bool_value(true),
        });
    }
    Ok(terms)
}

type ConditionalBinderCandidates<T> = Vec<(String, SymbolicValue<T>, Option<T>)>;

fn binder_candidates<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<ConditionalBinderCandidates<S::Term>, VerifyError> {
    let Binder::Collection {
        name, collection, ..
    } = binder
    else {
        return binder_values(solver, model, binder).map(|values| {
            values
                .into_iter()
                .map(|(name, value)| (name, value, None))
                .collect()
        });
    };
    let mut local = bindings.clone();
    let collection = eval(solver, model, collection, state, &mut local, old_state)?;
    match collection {
        SymbolicValue::Set { ty, entries } => {
            let TypeRef::Set(element_ty) = ty else {
                unreachable!();
            };
            entries
                .into_iter()
                .map(|(element, present)| {
                    Ok((
                        name.clone(),
                        concrete_value(solver, model, &element_ty, &element)?,
                        Some(present),
                    ))
                })
                .collect()
        }
        SymbolicValue::Seq { slots, len, .. } => slots
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                Ok((
                    name.clone(),
                    value,
                    Some(solver.lt(&solver.int_value(i64_index(index)?), &len)?),
                ))
            })
            .collect(),
        _ => Err(VerifyError::new(
            "collection binder expects a Set or Seq expression",
        )),
    }
}

pub(crate) fn binder_values<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
) -> Result<BinderCandidates<S::Term>, VerifyError> {
    match binder {
        Binder::Typed {
            name, type_name, ..
        } => {
            let type_name = qualified_type(type_name.namespace.as_deref(), &type_name.name)?;
            let ty = TypeRef::Named(type_name);
            model
                .domain_values(&ty)?
                .into_iter()
                .map(|value| Ok((name.clone(), concrete_value(solver, model, &ty, &value)?)))
                .collect()
        }
        Binder::Range { name, lo, hi } => {
            let lo = static_int(lo, model)?;
            let hi = static_int(hi, model)?;
            (lo..=hi)
                .map(|value| {
                    Ok((
                        name.clone(),
                        SymbolicValue::Scalar {
                            ty: TypeRef::Int,
                            term: solver.int_value(value),
                        },
                    ))
                })
                .collect()
        }
        Binder::Collection { .. } => Err(VerifyError::new(
            "collection binders are not implemented in the current verifier slice",
        )),
    }
}

pub(crate) fn binder_where<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    binder: &Binder,
    state: &SymbolicState<S::Term>,
    bindings: &mut Bindings<S::Term>,
    old_state: Option<&SymbolicState<S::Term>>,
) -> Result<Option<S::Term>, VerifyError> {
    let where_expr = match binder {
        Binder::Typed { where_expr, .. } | Binder::Collection { where_expr, .. } => {
            where_expr.as_deref()
        }
        Binder::Range { .. } => None,
    };
    where_expr
        .map(|expr| {
            let value = eval(solver, model, expr, state, bindings, old_state)?;
            Ok(bool_term(&value)?.clone())
        })
        .transpose()
}

fn static_int(expr: &Expr, model: &KernelModel) -> Result<i64, VerifyError> {
    match expr {
        Expr::Num(value) => Ok(*value),
        Expr::Var(name) => match model.consts.get(name) {
            Some(FslValue::Int(value)) => Ok(*value),
            _ => Err(VerifyError::new(format!(
                "'{name}' is not an integer const"
            ))),
        },
        Expr::Neg(inner) => static_int(inner, model)?
            .checked_neg()
            .ok_or_else(|| VerifyError::new("integer overflow in static negation")),
        Expr::Binary { op, left, right } => {
            let left = static_int(left, model)?;
            let right = static_int(right, model)?;
            match op.as_str() {
                "+" => left.checked_add(right),
                "-" => left.checked_sub(right),
                "*" => left.checked_mul(right),
                _ => None,
            }
            .ok_or_else(|| VerifyError::new("invalid static integer expression"))
        }
        _ => Err(VerifyError::new("binder bound is not a static integer")),
    }
}

fn qualified_type(namespace: Option<&str>, name: &str) -> Result<String, VerifyError> {
    if namespace.is_some() {
        Err(VerifyError::new(
            "qualified type remained after kernel lowering",
        ))
    } else {
        Ok(name.to_owned())
    }
}

fn sum_terms<S: SmtSolver>(solver: &S, terms: &[S::Term]) -> Result<S::Term, VerifyError> {
    let mut sum = solver.int_value(0);
    for term in terms {
        sum = solver.add(&sum, term)?;
    }
    Ok(sum)
}

fn bool_value<S: SmtSolver>(solver: &S, term: S::Term) -> SymbolicValue<S::Term> {
    let _ = solver;
    SymbolicValue::Scalar {
        ty: TypeRef::Bool,
        term,
    }
}

fn int_value<S: SmtSolver>(solver: &S, term: S::Term) -> SymbolicValue<S::Term> {
    let _ = solver;
    SymbolicValue::Scalar {
        ty: TypeRef::Int,
        term,
    }
}
