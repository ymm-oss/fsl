// SPDX-License-Identifier: Apache-2.0

//! The solver half of the decision-table checks (decisions/048 J3).
//!
//! fsl-core enumerates a table's field domains when their product stays
//! within [`fsl_core::DOMAIN_BOUND`], so every surface reports those
//! diagnostics alike. A larger table (or one whose conditions read more than
//! the entity's fields) comes back from [`fsl_core::decision_solver_obligations`]
//! as invariants over the whole state; native `check` asks the solver whether
//! every type-correct state satisfies each, which is the same question over
//! the same domains, and reports the first counterexample with the
//! diagnostic code the enumeration would have used.

use fsl_core::{KernelModel, PropertyDef};
use fslc_rust::verification_output::{DiagnosticClass, render_semantic_error};
use serde_json::{Map, Value};

use crate::process::{block_on_native, parse_options};

/// The rendered rejection of the first decision-table obligation the solver
/// refutes, or `None` when every one holds (or `source` has no table).
pub(crate) fn check(
    source: &str,
    model: &KernelModel,
    envelope: &dyn Fn() -> Map<String, Value>,
) -> Option<(Value, i32)> {
    // Only the next edition has decision tables; the published language is
    // not parsed a second time.
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
    for obligation in fsl_core::decision_solver_obligations(&requirements, model) {
        let mut antecedent = model.clone();
        antecedent.invariants.clear();
        let mut consequent = model.clone();
        consequent.invariants = vec![PropertyDef {
            name: "decide_obligation".to_owned(),
            expr: obligation.property.clone(),
            span: obligation.span,
            meta: None,
            annotations: fsl_syntax::Annotations::default(),
        }];
        let outcome = fsl_solver_z3::Z3Solver::new()
            .map_err(|error| error.to_string())
            .and_then(|mut solver| {
                block_on_native(fsl_verifier::invariant_implication(
                    &antecedent,
                    &consequent,
                    &mut solver,
                ))
                .map_err(|error| error.to_string())
            });
        let message = match outcome {
            Ok(fsl_verifier::ImplicationResult::Implied) => continue,
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
        };
        return Some((
            render_semantic_error(
                envelope(),
                &message,
                Some(obligation.span.python_loc()),
                &DiagnosticClass::Core(fsl_core::CoreErrorKind::Semantics),
                Some(obligation.code),
                None,
            ),
            2,
        ));
    }
    None
}
