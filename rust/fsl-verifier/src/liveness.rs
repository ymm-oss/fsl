// SPDX-License-Identifier: Apache-2.0

//! Shared symbolic liveness constraints for verifier engines.

use std::collections::BTreeMap;

use fsl_core::binder_range::{RangePlan, RangeUnknown};
use fsl_core::{FslValue, KernelBinder, KernelModel, LeadsToDef, TypeRef};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::bmc::CheckedStateCarriers;
use crate::eval::{
    enumerated_binder_binding, eval_in, integer_range_error, static_binder_values, static_int,
};
use crate::value::{
    SymbolicState, TypedBinding, TypedBindings as Bindings, bool_term, concrete_value,
};

#[derive(Clone)]
pub(crate) struct LeadstoBinding<T> {
    pub(crate) concrete: BTreeMap<String, FslValue>,
    pub(crate) symbolic: Bindings<T>,
}

pub(crate) fn leadsto_bindings<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    property: &LeadsToDef,
) -> Result<Vec<LeadstoBinding<S::Term>>, VerifyError> {
    let mut types = BTreeMap::new();
    let concrete = model.static_leadsto_bindings_with::<VerifyError>(
        property,
        |_, binder, bounds| {
            let (name, ty) = match binder {
                KernelBinder::Typed {
                    name, type_name, ..
                } => {
                    if type_name.namespace.is_some() {
                        static_binder_values(solver, model, binder)?;
                    }
                    let ty = if type_name.name == "Bool" {
                        TypeRef::Bool
                    } else {
                        TypeRef::Named(type_name.name.clone())
                    };
                    model.domain_contains(&ty, &FslValue::Int(0))?;
                    (name, ty)
                }
                KernelBinder::Range { name, .. } => {
                    if let Some((lo, hi)) = bounds {
                        validate_static_leadsto_range(model, lo, hi)?;
                    }
                    (name, TypeRef::Int)
                }
                KernelBinder::Collection { .. } => {
                    static_binder_values(solver, model, binder)?;
                    return Ok(());
                }
            };
            types.insert(name.clone(), ty);
            Ok(())
        },
        |_, endpoint, prefix| {
            if prefix.is_some() {
                return Ok(());
            }
            validate_static_leadsto_range(model, endpoint, endpoint)
        },
    )?;
    concrete
        .into_iter()
        .map(|concrete| {
            let mut symbolic = Bindings::new();
            for (name, value) in &concrete {
                symbolic.insert(
                    name.clone(),
                    enumerated_binder_binding(
                        solver,
                        model,
                        TypedBinding(
                            concrete_value(solver, model, &types[name], value)?,
                            Some(types[name].clone()),
                        ),
                    ),
                );
            }
            Ok(LeadstoBinding { concrete, symbolic })
        })
        .collect()
}

fn validate_static_leadsto_range(
    model: &KernelModel,
    lo: &fsl_core::KernelExpr,
    hi: &fsl_core::KernelExpr,
) -> Result<(), VerifyError> {
    let plan = model.plan_integer_range(lo, hi, &[]);
    if !matches!(
        plan,
        RangePlan::Static { .. } | RangePlan::Unknown(RangeUnknown::ConstantFoldFailed)
    ) {
        return Err(integer_range_error(&plan));
    }
    static_int(lo, model)?;
    static_int(hi, model)?;
    if !matches!(plan, RangePlan::Static { .. }) {
        return Err(integer_range_error(&plan));
    }
    Ok(())
}

pub(crate) fn leadsto_condition<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &fsl_core::KernelExpr,
    state: &SymbolicState<S::Term>,
    binding: &Bindings<S::Term>,
) -> Result<S::Term, VerifyError> {
    leadsto_condition_in(solver, model, expr, state, binding, None)
}

pub(crate) fn leadsto_condition_in<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &fsl_core::KernelExpr,
    state: &SymbolicState<S::Term>,
    binding: &Bindings<S::Term>,
    ctx: Option<&CheckedStateCarriers<'_, S::Term>>,
) -> Result<S::Term, VerifyError> {
    let mut binding = binding.clone();
    let value = eval_in(solver, model, expr, state, &mut binding, None, ctx)?;
    Ok(bool_term(&value)?.clone())
}
