// SPDX-License-Identifier: Apache-2.0

//! Liveness symmetry reduction for `symmetric type` / `symmetric enum`
//! (issue #461; `docs/LANGUAGE.md` §2 type notes, `docs/DESIGN-temporal.md`
//! §2.5.1).
//!
//! During `leadsTo` lasso and deadlock-stall search, a designated
//! representative state (the lasso loop head, or the stalled state) is
//! constrained to the canonical -- lexicographically smallest under a global
//! renaming -- permutation of each symmetric type's per-entity rows. Rows are
//! built from `Map<SymmetricType, V>` and `Set<SymmetricType>` state
//! variables, in source (declaration) order, skipping any `V` that itself
//! mentions a symmetric identity type. This ports the frozen Python
//! reference's `_symmetry_canonical_constraint` (`src/fslc/bmc.py`).
//!
//! Soundness: the transition relation and properties of a valid `symmetric`
//! model are equivariant under a single global renaming of that type's
//! values. Given any lasso or stall counterexample, the global permutation
//! that sorts the representative state's row vector yields an equally valid
//! counterexample, so constraining only the designated representative state
//! (never every intermediate state) cannot hide a genuine violation.

use fsl_core::{KernelModel, TypeDef, TypeRef};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::value::{SymbolicState, SymbolicValue, bool_term, concrete_value, int_term};

fn symmetric_type_names(model: &KernelModel) -> Vec<&str> {
    model
        .types
        .iter()
        .filter(|(_, definition)| {
            matches!(
                definition,
                TypeDef::Domain {
                    symmetric: true,
                    ..
                } | TypeDef::Enum {
                    symmetric: true,
                    ..
                }
            )
        })
        .map(|(name, _)| name.as_str())
        .collect()
}

/// Whether `ty` mentions the named symmetric type anywhere in its shape.
/// Mirrors the frozen Python reference's `_type_ref_mentions`: a `relation`
/// endpoint is not inspected (matching Python's untagged fallthrough), which
/// is conservative -- at worst it under-recognizes a symmetric mention and
/// the caller's map-value flattening then contributes nothing for that
/// shape (see `flatten_row_terms`), never an unsound term.
fn type_ref_mentions(model: &KernelModel, ty: &TypeRef, target: &str) -> bool {
    match ty {
        TypeRef::Named(name) => {
            if name == target {
                return true;
            }
            match model.types.get(name) {
                Some(TypeDef::Struct { fields }) => fields
                    .iter()
                    .any(|(_, field_ty)| type_ref_mentions(model, field_ty, target)),
                _ => false,
            }
        }
        TypeRef::Map(key, value) => {
            type_ref_mentions(model, key, target) || type_ref_mentions(model, value, target)
        }
        TypeRef::Set(inner) | TypeRef::Option(inner) | TypeRef::Seq(inner, _) => {
            type_ref_mentions(model, inner, target)
        }
        TypeRef::Int | TypeRef::Bool | TypeRef::Range(_, _) | TypeRef::Relation(_, _) => false,
    }
}

/// Coerce a Boolean-sorted row element to the Int sort so a row (possibly
/// mixing `Map` scalar values and `Set` membership indicators) can be
/// lexicographically compared with `<`/`==`. Mirrors
/// `_symmetry_term_as_int`.
fn coerce_int<S: SmtSolver>(
    solver: &S,
    term: &S::Term,
    ty: &TypeRef,
) -> Result<S::Term, VerifyError> {
    if matches!(ty, TypeRef::Bool) {
        Ok(solver.ite(term, &solver.int_value(1), &solver.int_value(0))?)
    } else {
        Ok(term.clone())
    }
}

fn default_term<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    ty: &TypeRef,
) -> Result<S::Term, VerifyError> {
    let value = model.default_value(ty)?;
    let symbolic = concrete_value(solver, model, ty, &value)?;
    let term = if matches!(ty, TypeRef::Bool) {
        bool_term(&symbolic)?.clone()
    } else {
        int_term(&symbolic)?.clone()
    };
    coerce_int(solver, &term, ty)
}

/// Append one scalar/option/struct-of-scalars `Map` value's row contribution.
/// Mirrors `_symmetry_map_value_terms`: nested `Map`/`Set`/`Seq`/`Relation`
/// values (which cannot occur here anyway, since the caller already excludes
/// any `V` mentioning a symmetric type, but may still contain a *non*-
/// symmetric collection) contribute nothing, matching Python's fallthrough
/// `return []`.
fn flatten_row_terms<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: &SymbolicValue<S::Term>,
    out: &mut Vec<S::Term>,
) -> Result<(), VerifyError> {
    match value {
        SymbolicValue::Scalar { ty, term } => out.push(coerce_int(solver, term, ty)?),
        SymbolicValue::Option { ty, present, value } => {
            let TypeRef::Option(inner_ty) = ty else {
                return Ok(());
            };
            out.push(coerce_int(solver, present, &TypeRef::Bool)?);
            let inner_term = match value.as_ref() {
                SymbolicValue::Scalar { term, .. } => coerce_int(solver, term, inner_ty)?,
                _ => return Ok(()),
            };
            let default = default_term(solver, model, inner_ty)?;
            out.push(solver.ite(present, &inner_term, &default)?);
        }
        SymbolicValue::Struct { fields, .. } => {
            for field_value in fields.values() {
                flatten_row_terms(solver, model, field_value, out)?;
            }
        }
        SymbolicValue::None
        | SymbolicValue::Map { .. }
        | SymbolicValue::Set { .. }
        | SymbolicValue::Seq { .. }
        | SymbolicValue::Relation { .. }
        | SymbolicValue::SetLiteral(_)
        | SymbolicValue::SeqLiteral(_) => {}
    }
    Ok(())
}

/// Build one per-entity row per symmetric-type value, in state-variable
/// declaration order. Returns one row (possibly empty) per domain value of
/// `type_name`, in ascending value order.
fn rows_for_type<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
    type_name: &str,
) -> Result<Vec<Vec<S::Term>>, VerifyError> {
    let values = model.domain_values(&TypeRef::Named(type_name.to_owned()))?;
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let mut rows: Vec<Vec<S::Term>> = values.iter().map(|_| Vec::new()).collect();
    for (name, ty) in &model.state {
        match ty {
            TypeRef::Map(key_ty, value_ty) => {
                if !matches!(key_ty.as_ref(), TypeRef::Named(named) if named == type_name) {
                    continue;
                }
                if type_ref_mentions(model, value_ty, type_name) {
                    continue;
                }
                let Some(SymbolicValue::Map { entries, .. }) = state.get(name) else {
                    continue;
                };
                for (entity, row) in values.iter().zip(rows.iter_mut()) {
                    if let Some((_, symbolic)) = entries.iter().find(|(key, _)| key == entity) {
                        flatten_row_terms(solver, model, symbolic, row)?;
                    }
                }
            }
            TypeRef::Set(element_ty) => {
                if !matches!(element_ty.as_ref(), TypeRef::Named(named) if named == type_name) {
                    continue;
                }
                let Some(SymbolicValue::Set { entries, .. }) = state.get(name) else {
                    continue;
                };
                for (entity, row) in values.iter().zip(rows.iter_mut()) {
                    if let Some((_, term)) = entries.iter().find(|(key, _)| key == entity) {
                        row.push(coerce_int(solver, term, &TypeRef::Bool)?);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(rows)
}

/// `left <=_lex right` over two equal-length Int-sorted term vectors.
/// Mirrors `_symmetry_lex_le`.
fn lex_le<S: SmtSolver>(
    solver: &S,
    left: &[S::Term],
    right: &[S::Term],
) -> Result<S::Term, VerifyError> {
    if left.is_empty() {
        return Ok(solver.bool_value(true));
    }
    let mut cases = Vec::new();
    let mut equal_prefix = solver.bool_value(true);
    for (a, b) in left.iter().zip(right) {
        let strictly_less = solver.and(&[equal_prefix.clone(), solver.lt(a, b)?])?;
        cases.push(strictly_less);
        equal_prefix = solver.and(&[equal_prefix, solver.equal(a, b)?])?;
    }
    cases.push(equal_prefix);
    Ok(solver.or(&cases)?)
}

/// The symmetry-breaking constraint for one representative state: for every
/// `symmetric` type with at least one contributing `Map`/`Set` state
/// variable, its per-entity rows (in ascending entity-value order) must be
/// lexicographically non-decreasing. `true` when no symmetric type has any
/// contributing state variable.
///
/// # Errors
///
/// Returns [`VerifyError`] for a missing state variable or solver failure.
pub(crate) fn canonical_constraint<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
) -> Result<S::Term, VerifyError> {
    let mut parts = Vec::new();
    for type_name in symmetric_type_names(model) {
        let rows = rows_for_type(solver, model, state, type_name)?;
        if rows.is_empty() || rows[0].is_empty() {
            continue;
        }
        for pair in rows.windows(2) {
            parts.push(lex_le(solver, &pair[0], &pair[1])?);
        }
    }
    if parts.is_empty() {
        Ok(solver.bool_value(true))
    } else {
        Ok(solver.and(&parts)?)
    }
}
