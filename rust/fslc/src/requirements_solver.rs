// SPDX-License-Identifier: Apache-2.0

//! The solver half of the next-edition requirements checks (decisions/048
//! J3): decision tables and the guards rules and tables add to events.
//!
//! fsl-core enumerates their finite domains when the product stays within
//! [`fsl_core::DOMAIN_BOUND`], so every surface reports those diagnostics
//! alike. What it defers (a larger domain, or a condition reading more than
//! the entity's fields and the event's inputs) comes back from
//! [`fsl_core::solver_obligations`] as invariants over the whole state; native
//! `check` asks the solver whether every type-correct state satisfies each,
//! which is the same question over the same domains, and reports with the
//! diagnostic code the enumeration would have used.

use fsl_core::{
    DecisionObligation, GuardConflict, KernelModel, PropertyDef, SolverObligation, TypeRef,
};
use fsl_syntax::Expr;
use fslc_rust::verification_output::{DiagnosticClass, render_semantic_error};
use serde_json::{Map, Value};

use crate::process::{block_on_native, parse_options};

/// Whether every type-correct state of `model`, extended with the variables
/// `extra_state`, satisfies `property`, or the state that does not.
fn implied(
    model: &KernelModel,
    extra_state: &[(String, TypeRef)],
    property: &Expr,
    span: fsl_syntax::Span,
) -> Result<fsl_verifier::ImplicationResult, String> {
    let mut antecedent = model.clone();
    antecedent.invariants.clear();
    antecedent.state.extend(extra_state.iter().cloned());
    let mut consequent = antecedent.clone();
    consequent.invariants = vec![PropertyDef {
        name: "requirements_obligation".to_owned(),
        expr: property.clone(),
        span,
        meta: None,
        annotations: fsl_syntax::Annotations::default(),
    }];
    fsl_solver_z3::Z3Solver::new()
        .map_err(|error| error.to_string())
        .and_then(|mut solver| {
            block_on_native(fsl_verifier::invariant_implication(
                &antecedent,
                &consequent,
                &mut solver,
            ))
            .map_err(|error| error.to_string())
        })
}

/// A decision table's check: the message when the solver refutes it.
fn table(model: &KernelModel, obligation: &DecisionObligation) -> Option<String> {
    Some(
        match implied(model, &[], &obligation.property, obligation.span) {
            Ok(fsl_verifier::ImplicationResult::Implied) => return None,
            Ok(fsl_verifier::ImplicationResult::Counterexample(state)) => {
                let state = fslc_rust::state_json(&state);
                let shown = obligation
                    .states
                    .iter()
                    .filter_map(|name| state.get(name).map(|value| format!("{name} = {value}")))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{} (checked by the solver, because {}); counterexample: {shown}",
                    obligation.message, obligation.reason
                )
            }
            Err(error) => format!(
                "the {} check of a decision table could not be decided by the solver \
             (enumeration skipped it because {}): {error}",
                obligation.code, obligation.reason
            ),
        },
    )
}

/// An event's guards: the message when no value satisfies them, naming a
/// minimal conflicting subset as the enumeration does.
fn guards(model: &KernelModel, conflict: &GuardConflict) -> Option<String> {
    let inputs = conflict.input_state();
    let conflicting = |subset: &[usize]| {
        implied(model, &inputs, &conflict.property(subset), conflict.span)
            .map(|result| matches!(result, fsl_verifier::ImplicationResult::Implied))
    };
    let undecided = |error: String| {
        format!(
            "the {} check could not be decided by the solver (enumeration skipped it because \
             {}): {error}",
            fsl_core::RULE_CONFLICT_UNSATISFIABLE_GUARD,
            conflict.reason
        )
    };
    let mut kept = (0..conflict.len()).collect::<Vec<_>>();
    match conflicting(&kept) {
        Ok(true) => {}
        Ok(false) => return None,
        Err(error) => return Some(undecided(error)),
    }
    let mut index = 0;
    while index < kept.len() {
        let without = kept
            .iter()
            .enumerate()
            .filter(|(position, _)| *position != index)
            .map(|(_, guard)| *guard)
            .collect::<Vec<_>>();
        match conflicting(&without) {
            Ok(true) => kept = without,
            Ok(false) => index += 1,
            Err(error) => return Some(undecided(error)),
        }
    }
    conflict.reported(&kept).then(|| {
        format!(
            "{} (checked by the solver, because {})",
            conflict.message(&kept),
            conflict.reason
        )
    })
}

/// The rendered rejection of the first obligation the solver refutes, or
/// `None` when every one holds (or `source` has none).
pub(crate) fn check(
    source: &str,
    model: &KernelModel,
    envelope: &dyn Fn() -> Map<String, Value>,
) -> Option<(Value, i32)> {
    // Only the next edition has these checks; the published language is not
    // parsed a second time.
    if !parse_options()
        .unstable()
        .contains(fsl_syntax::UnstableFeature::Fsl5)
    {
        return None;
    }
    let Ok(fsl_syntax::SurfaceDocument::Requirements(requirements)) =
        fsl_syntax::parse_surface_document_with(source, parse_options())
    else {
        return None;
    };
    requirements.next.as_ref()?;
    for obligation in fsl_core::solver_obligations(&requirements, model) {
        let (code, span, message) = match &obligation {
            SolverObligation::Table(table_obligation) => (
                table_obligation.code,
                table_obligation.span,
                table(model, table_obligation),
            ),
            SolverObligation::Guards(conflict) => (
                fsl_core::RULE_CONFLICT_UNSATISFIABLE_GUARD,
                conflict.span,
                guards(model, conflict),
            ),
        };
        let Some(message) = message else {
            continue;
        };
        return Some((
            render_semantic_error(
                envelope(),
                &message,
                Some(span.python_loc()),
                &DiagnosticClass::Core(fsl_core::CoreErrorKind::Semantics),
                Some(code),
                None,
            ),
            2,
        ));
    }
    None
}
