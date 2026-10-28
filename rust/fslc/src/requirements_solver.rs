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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::pin;
    use std::rc::Rc;
    use std::task::{Context, Poll, Waker};

    use fsl_solver::{
        CheckFuture, ModelValue, SatResult, SmtSolver, SolverError, SolverResult, Sort,
        VerificationStatistics,
    };

    // This is an outcome-routing stub, not an SMT evaluator. Terms carry only
    // sorts; each fresh solver answers exactly one scripted query. No Z3 needed.
    struct StubSolver {
        answer: Option<SolverResult<SatResult>>,
        resets: Rc<Cell<usize>>,
    }

    macro_rules! binary_terms {
        ($($name:ident => $sort:ident),* $(,)?) => {
            $(fn $name(&self, _: &Sort, _: &Sort) -> SolverResult<Sort> {
                Ok(Sort::$sort)
            })*
        };
    }

    impl SmtSolver for StubSolver {
        type Term = Sort;

        fn version(&self) -> &'static str {
            "requirements-outcome-stub"
        }
        fn set_query_context(&mut self, _: &str, _: &str) {}
        fn statistics(&self) -> VerificationStatistics {
            VerificationStatistics::default()
        }
        fn sort(&self, term: &Sort) -> Sort {
            term.clone()
        }
        fn bool_value(&self, _: bool) -> Sort {
            Sort::Bool
        }
        fn int_value(&self, _: i64) -> Sort {
            Sort::Int
        }
        fn constant(&self, _: &str, sort: &Sort) -> SolverResult<Sort> {
            Ok(sort.clone())
        }
        fn not(&self, _: &Sort) -> SolverResult<Sort> {
            Ok(Sort::Bool)
        }
        fn and(&self, _: &[Sort]) -> SolverResult<Sort> {
            Ok(Sort::Bool)
        }
        fn or(&self, _: &[Sort]) -> SolverResult<Sort> {
            Ok(Sort::Bool)
        }
        binary_terms! {
            implies => Bool, equal => Bool,
            add => Int, sub => Int, mul => Int, div => Int, modulo => Int,
            lt => Bool, le => Bool, gt => Bool, ge => Bool,
        }
        fn ite(&self, _: &Sort, then_term: &Sort, _: &Sort) -> SolverResult<Sort> {
            Ok(then_term.clone())
        }
        fn neg(&self, _: &Sort) -> SolverResult<Sort> {
            Ok(Sort::Int)
        }
        fn const_array(&self, domain: &Sort, value: &Sort) -> SolverResult<Sort> {
            Ok(Sort::array(domain.clone(), value.clone()))
        }
        fn select(&self, array: &Sort, _: &Sort) -> SolverResult<Sort> {
            let Sort::Array { range, .. } = array else {
                panic!("expected array term, got {array:?}");
            };
            Ok(*range.clone())
        }
        fn store(&self, array: &Sort, _: &Sort, _: &Sort) -> SolverResult<Sort> {
            Ok(array.clone())
        }
        fn substitute(&self, term: &Sort, _: &[(Sort, Sort)]) -> SolverResult<Sort> {
            Ok(term.clone())
        }
        fn push(&mut self) {}
        fn pop(&mut self, _: u32) -> SolverResult<()> {
            Ok(())
        }
        fn reset(&mut self) -> SolverResult<()> {
            self.resets.set(self.resets.get() + 1);
            Ok(())
        }
        fn assert(&mut self, _: &Sort) -> SolverResult<()> {
            Ok(())
        }
        fn assert_and_track(&mut self, _: &Sort, _: &Sort) -> SolverResult<()> {
            panic!("unexpected tracked assertion");
        }
        fn check(&mut self) -> CheckFuture<'_> {
            let answer = self.answer.take().expect("one check per fresh solver");
            Box::pin(async move { answer })
        }
        fn check_assuming(&mut self, _: &[Sort]) -> CheckFuture<'_> {
            panic!("unexpected check_assuming");
        }
        fn unsat_core(&self) -> SolverResult<Vec<Sort>> {
            panic!("unexpected unsat_core");
        }
        fn model_eval(&self, term: &Sort) -> SolverResult<Option<ModelValue>> {
            // A concrete projection for SAT; the stub does not establish that
            // this assignment satisfies the query.
            Ok(Some(match term {
                Sort::Bool => ModelValue::Bool(false),
                Sort::Int => ModelValue::Int(0),
                Sort::Array { .. } => panic!("unexpected array projection"),
            }))
        }
    }

    const SOURCE: &str = r#"fsl 5
requirements R {
  number Amount
  actor Employee
  entity Claim {
    amount: Amount = 0
    lifecycle Draft -> Submitted
  }
  event submit = Employee submits Claim with amount Draft -> Submitted
  rule REQ-1 "bounds" {
    allow Employee submits Claim only when input amount > 5
    allow Employee submits Claim only when input amount < 4
    never Employee submits Claim when input amount == 0
  }
}
verify {
  instances Claim = 1
  values Amount = 0..2000000
}
"#;

    fn run(answers: Vec<SolverResult<SatResult>>) -> Option<Rejection> {
        let options = fsl_syntax::ParseOptions::with_unstable(
            fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
        );
        let (_, model) = crate::check_stages::load_kernel_model(
            SOURCE,
            "requirements-stub.fsl",
            &fsl_core::FsResolver::new("."),
            &options,
        )
        .expect("large domain must defer the guard check, not reject during lowering");
        let mut answers = VecDeque::from(answers);
        let expected_resets = answers.len();
        let resets = Rc::new(Cell::new(0));
        let mut new_solver = || {
            Ok(StubSolver {
                answer: Some(answers.pop_front().expect("unexpected solver query")),
                resets: Rc::clone(&resets),
            })
        };
        let result = {
            let mut future = pin!(refute(SOURCE, &model, &options, &mut new_solver));
            let Poll::Ready(result) = future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("stub solver future must be immediately ready");
            };
            result
        };
        assert!(answers.is_empty(), "all scripted queries must be consumed");
        assert_eq!(resets.get(), expected_resets, "reset every fresh solver");
        result
    }

    fn assert_guard_rejection(rejection: &Rejection, message: &str) {
        assert_eq!(rejection.code, fsl_core::RULE_CONFLICT_UNSATISFIABLE_GUARD);
        assert_eq!(
            rejection.span.python_loc(),
            serde_json::json!({"line": 9, "column": 3})
        );
        assert_eq!(rejection.message, message);
    }

    #[test]
    fn refute_sat_accepts_a_guard_counterexample() {
        assert_eq!(run(vec![Ok(SatResult::Sat)]), None);
    }

    #[test]
    fn refute_unsat_reports_only_the_minimal_conflicting_subset() {
        // Initial conflict; removing either bound makes it satisfiable;
        // removing the unrelated never rule leaves the conflict unsatisfiable.
        let rejection = run(vec![
            Ok(SatResult::Unsat),
            Ok(SatResult::Sat),
            Ok(SatResult::Sat),
            Ok(SatResult::Unsat),
        ])
        .expect("unsatisfiable guard set must be rejected");
        assert_guard_rejection(
            &rejection,
            "event 'submit' can never occur: when Claim is Draft, no value of its fields and inputs satisfies the rules \
             REQ-1 `allow Employee submits Claim only when input amount > 5`; \
             REQ-1 `allow Employee submits Claim only when input amount < 4` \
             (checked by the solver, because its fields and inputs take 2000001 values, more than the 1000000 enumerated)",
        );
    }

    #[test]
    fn refute_unknown_rejects_instead_of_accepting_or_claiming_a_conflict() {
        let rejection = run(vec![Ok(SatResult::Unknown)])
            .expect("unknown must reject fail-closed, not defer acceptance");
        assert_guard_rejection(
            &rejection,
            "the rule_conflict_unsatisfiable_guard check could not be decided by the solver \
             (enumeration skipped it because its fields and inputs take 2000001 values, more than the 1000000 enumerated): \
             solver returned unknown in invariant implication",
        );
    }

    #[test]
    fn refute_backend_error_rejects_instead_of_accepting_or_claiming_a_conflict() {
        let rejection = run(vec![Err(SolverError::new("injected backend failure"))])
            .expect("backend error must reject fail-closed");
        assert_guard_rejection(
            &rejection,
            "the rule_conflict_unsatisfiable_guard check could not be decided by the solver \
             (enumeration skipped it because its fields and inputs take 2000001 values, more than the 1000000 enumerated): \
             injected backend failure",
        );
    }
}
