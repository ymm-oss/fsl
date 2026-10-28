// SPDX-License-Identifier: Apache-2.0

//! The solver half of the next-edition requirements checks (decisions/048
//! J3): decision tables and the guards rules and tables add to events.
//!
//! fsl-core enumerates their finite domains when the product stays within
//! [`fsl_core::DOMAIN_BOUND`]; a table or guard it refutes fails the lowering
//! itself, so every surface rejects it. What it defers (a larger domain, or a
//! condition reading more than the entity's fields and the event's inputs)
//! comes back from [`fsl_core::solver_obligations`] as invariants over the
//! whole state. [`refute`] asks a solver whether every type-correct state
//! satisfies each, which is the same question over the same domains, and
//! returns the diagnostic the enumeration would have reported.
//!
//! Every surface that accepts a spec asks it with its own solver: native
//! `check` and the browser Worker's through
//! [`crate::check_stages::run_check_stages`], the other native commands where
//! they load the spec, the Worker's `verify` after its load, and the language
//! server through [`crate::source_diagnostic::solver_diagnostics`]. A spec
//! `check` rejects here is rejected by all of them with the same diagnostic.

use fsl_core::{
    DecisionObligation, GuardConflict, KernelModel, PropertyDef, SolverObligation, TypeRef,
};
use fsl_syntax::{Expr, Span};
use serde_json::{Map, Value};

use crate::spec_load::{SemanticDiagnostic, SpecLoadError};
use crate::verification_output::DiagnosticClass;

/// A requirements check the solver refuted, or could not decide: the spec is
/// rejected with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// The diagnostic code the enumeration uses for the same check.
    pub code: &'static str,
    pub message: String,
    /// The table, row or event the check is about.
    pub span: Span,
}

impl Rejection {
    /// The rejection as the classified load failure every command renders.
    #[must_use]
    pub fn diagnostic(&self) -> SemanticDiagnostic {
        SemanticDiagnostic {
            message: self.message.clone(),
            loc: Some(self.span.python_loc()),
            class: DiagnosticClass::Core(fsl_core::CoreErrorKind::Semantics),
            diagnostic_code: Some(self.code),
            hint: None,
            quick_fix: None,
        }
    }

    /// [`Self::diagnostic`] as a load failure.
    #[must_use]
    pub fn load_error(&self) -> SpecLoadError {
        SpecLoadError::Semantic(Box::new(self.diagnostic()))
    }

    /// The error envelope, on `envelope`.
    #[must_use]
    pub fn render(&self, envelope: Map<String, Value>) -> Value {
        crate::spec_load::render_spec_load_error(envelope, &self.load_error())
    }
}

/// A fresh solver for one question, or why none could be made.
pub type NewSolver<'a, S> = dyn FnMut() -> Result<S, String> + 'a;

/// Whether every type-correct state of `model`, extended with the variables
/// `extra_state`, satisfies `property`, or the state that does not.
async fn implied<S: fsl_solver::SmtSolver>(
    model: &KernelModel,
    extra_state: &[(String, TypeRef)],
    property: &Expr,
    span: Span,
    new_solver: &mut NewSolver<'_, S>,
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
    let mut solver = new_solver()?;
    let result = fsl_verifier::invariant_implication(&antecedent, &consequent, &mut solver)
        .await
        .map_err(|error| error.to_string());
    // The Worker's backend is one global solver behind every `Z3JsSolver`;
    // leave it without this question's assertions for the next user.
    fsl_solver::SmtSolver::reset(&mut solver).map_err(|error| error.to_string())?;
    result
}

/// A decision table's check: the message when the solver refutes it.
async fn table<S: fsl_solver::SmtSolver>(
    model: &KernelModel,
    obligation: &DecisionObligation,
    new_solver: &mut NewSolver<'_, S>,
) -> Option<String> {
    Some(
        match implied(
            model,
            &[],
            &obligation.property,
            obligation.span,
            new_solver,
        )
        .await
        {
            Ok(fsl_verifier::ImplicationResult::Implied) => return None,
            Ok(fsl_verifier::ImplicationResult::Counterexample(state)) => {
                let state = crate::state_json(&state);
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
async fn guards<S: fsl_solver::SmtSolver>(
    model: &KernelModel,
    conflict: &GuardConflict,
    new_solver: &mut NewSolver<'_, S>,
) -> Option<String> {
    let inputs = conflict.input_state();
    let undecided = |error: String| {
        format!(
            "the {} check could not be decided by the solver (enumeration skipped it because \
             {}): {error}",
            fsl_core::RULE_CONFLICT_UNSATISFIABLE_GUARD,
            conflict.reason
        )
    };
    let mut kept = (0..conflict.len()).collect::<Vec<_>>();
    match implied(
        model,
        &inputs,
        &conflict.property(&kept),
        conflict.span,
        new_solver,
    )
    .await
    {
        Ok(fsl_verifier::ImplicationResult::Implied) => {}
        Ok(fsl_verifier::ImplicationResult::Counterexample(_)) => return None,
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
        match implied(
            model,
            &inputs,
            &conflict.property(&without),
            conflict.span,
            new_solver,
        )
        .await
        {
            Ok(fsl_verifier::ImplicationResult::Implied) => kept = without,
            Ok(fsl_verifier::ImplicationResult::Counterexample(_)) => index += 1,
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

/// The first obligation of `source` the solver refutes (or cannot decide),
/// or `None` when every one holds or `source` has none: only a next-edition
/// requirements document read under the `fsl5` gate has any, so nothing else
/// is parsed a second time or reaches `new_solver`.
pub async fn refute<S: fsl_solver::SmtSolver>(
    source: &str,
    model: &KernelModel,
    options: &fsl_syntax::ParseOptions,
    new_solver: &mut NewSolver<'_, S>,
) -> Option<Rejection> {
    if !options
        .unstable()
        .contains(fsl_syntax::UnstableFeature::Fsl5)
    {
        return None;
    }
    let Ok(fsl_syntax::SurfaceDocument::Requirements(requirements)) =
        fsl_syntax::parse_surface_document_with(source, options)
    else {
        return None;
    };
    requirements.next.as_ref()?;
    for obligation in fsl_core::solver_obligations(&requirements, model) {
        let (code, span, message) = match &obligation {
            SolverObligation::Table(table_obligation) => (
                table_obligation.code,
                table_obligation.span,
                table(model, table_obligation, new_solver).await,
            ),
            SolverObligation::Guards(conflict) => (
                fsl_core::RULE_CONFLICT_UNSATISFIABLE_GUARD,
                conflict.span,
                guards(model, conflict, new_solver).await,
            ),
        };
        if let Some(message) = message {
            return Some(Rejection {
                code,
                message,
                span,
            });
        }
    }
    None
}
