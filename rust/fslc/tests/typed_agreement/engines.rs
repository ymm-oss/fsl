// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Builds a model from generated FSL source and runs the three native
//! engines named in `docs/DESIGN-conformance-harness.md`'s C6 section --
//! `fsl_runtime::bfs` ("Monitor BFS"), `fsl_runtime::verify_explicit`
//! ("explicit"), and `fsl_verifier::verify_bounded` ("BMC", native Z3) --
//! comparing verdicts, replaying evidence through `replay_trace`, and
//! sampling successor admission through `transition_matches_step`.
//!
//! `block_on` is copied from `rust/fsl-verifier/tests/expression_agreement.rs`
//! rather than shared, matching that file's own choice not to add a tokio
//! dependency for one hand-rolled poll loop.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, KernelModel, build_model, build_surface_model, parse_kernel_source};
use fsl_runtime::{Monitor, State, Violation};
use fsl_syntax::{Expr, SpecItem};

use crate::generator::ExpressionBuild;

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

/// Parse and typecheck one generated FSL source string.
///
/// # Panics
///
/// Panics (failing the calling test) when the model does not build: per the
/// brief, a generator that emits an invalid model is a generator bug, not
/// evidence to report.
#[must_use]
pub fn build(id: &str, source: &str) -> KernelModel {
    let resolver = FsResolver::new(".");
    let kernel = parse_kernel_source(source, &resolver)
        .unwrap_or_else(|error| panic!("generator bug: '{id}' did not parse: {error}\n{source}"));
    build_model(kernel).unwrap_or_else(|error| {
        panic!("generator bug: '{id}' did not typecheck: {error}\n{source}")
    })
}

/// Build one expression-axis model through the ordinary parse/lower/typecheck
/// path. `EnumMemberTypedAst` additionally replaces the parsed enum token
/// with the typed `Expr::EnumMember` form before re-running
/// `build_surface_model`, the public semantic gate specifically provided for
/// typed AST mutations.
#[must_use]
pub fn build_expression(id: &str, source: &str, build_kind: ExpressionBuild) -> KernelModel {
    if build_kind == ExpressionBuild::ParsedSource {
        return build(id, source);
    }

    let resolver = FsResolver::new(".");
    let kernel = parse_kernel_source(source, &resolver)
        .unwrap_or_else(|error| panic!("generator bug: '{id}' did not parse: {error}\n{source}"));
    let mut syntax = kernel.into_syntax();
    let expression = syntax
        .items
        .iter_mut()
        .find_map(|item| match item {
            SpecItem::Invariant { name, expr, .. } if name == "Variant" => Some(expr.as_mut()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("generator bug: '{id}' has no Variant invariant"));
    let Expr::Binary { right, .. } = expression else {
        panic!("generator bug: '{id}' EnumMember probe is not a binary expression");
    };
    assert!(
        matches!(right.as_ref(), Expr::Var(name) if name == "Pending"),
        "generator bug: '{id}' EnumMember probe no longer has Pending on the right: {right:?}"
    );
    **right = Expr::EnumMember {
        type_name: "Status".to_owned(),
        member: "Pending".to_owned(),
    };

    build_surface_model(syntax).unwrap_or_else(|error| {
        panic!("generator bug: '{id}' typed AST did not typecheck: {error}\n{source}")
    })
}

/// A verdict normalized across the three engines: no violation, or the
/// `(kind, name, step)` of the first one found. All three engines share the
/// same `Violation` step convention (rooted in `Monitor`'s step counter), so
/// no "proved"-vs-"verified" wording normalization is needed here the way
/// the CLI-level corpus sweep in `explicit_engine.rs` needs one -- that
/// wording is a CLI presentation-layer artifact, not part of these structs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    Clean,
    Violated {
        kind: String,
        name: String,
        step: usize,
    },
}

impl From<Option<Violation>> for Verdict {
    fn from(violation: Option<Violation>) -> Self {
        match violation {
            None => Verdict::Clean,
            Some(violation) => Verdict::Violated {
                kind: violation.kind,
                name: violation.name,
                step: violation.step,
            },
        }
    }
}

/// Run all engines relevant to `model` at `depth`, assert three-way
/// agreement on the invariant/trans/reachable/type-bound verdict, and
/// return that agreed verdict.
///
/// `LeadsTo` properties are out of scope for `verify_explicit`
/// (`fsl_runtime::explicit_unsupported_reason` rejects any model that
/// declares one) and are invisible to `fsl_runtime::bfs` (it never reads
/// `model.leadstos`), so a model with a `leadsTo` property runs BMC only;
/// see the module doc and `docs/DESIGN-conformance-harness.md` C6 for why
/// asserting bfs/explicit "agreement" there would be vacuous rather than
/// evidence.
///
/// # Panics
///
/// Panics on disagreement, an engine error other than the documented
/// leadsTo boundary, or a witness/successor check failure.
pub fn run_agreement(id: &str, model: &KernelModel, depth: usize) -> Verdict {
    let bfs_result = fsl_runtime::bfs(model.clone(), depth)
        .unwrap_or_else(|error| panic!("'{id}': Monitor BFS engine errored: {error}"));
    let bfs_verdict = Verdict::from(bfs_result.violation.clone());

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create Z3 solver");
    let bmc_result = block_on(fsl_verifier::verify_bounded(model, &mut solver, depth))
        .unwrap_or_else(|error| panic!("'{id}': BMC engine errored: {error}"));
    let bmc_verdict = Verdict::from(bmc_result.violation.clone().map(|violation| Violation {
        kind: violation.kind,
        name: violation.name,
        step: violation.step,
    }));

    let has_leadsto = !model.leadstos.is_empty();
    let explicit = if has_leadsto {
        let reason = fsl_runtime::explicit_unsupported_reason(model).unwrap_or_else(|| {
            panic!("'{id}': model declares leadsTo but explicit_unsupported_reason allowed it")
        });
        assert!(
            reason.contains("leadsTo"),
            "'{id}': explicit rejection reason changed shape, re-check the documented boundary: {reason}"
        );
        None
    } else {
        let explicit_result = fsl_runtime::verify_explicit(model.clone(), depth, 1_000_000)
            .unwrap_or_else(|error| panic!("'{id}': explicit engine errored: {error}"));
        let explicit_verdict =
            Verdict::from(
                explicit_result
                    .violation
                    .clone()
                    .map(|violation| Violation {
                        kind: violation.violation.kind,
                        name: violation.violation.name,
                        step: violation.violation.step,
                    }),
            );
        assert_eq!(
            bfs_verdict, explicit_verdict,
            "'{id}': Monitor BFS and explicit disagree (bfs={bfs_verdict:?} explicit={explicit_verdict:?})"
        );
        assert_eq!(
            explicit_verdict, bmc_verdict,
            "'{id}': explicit and BMC disagree (explicit={explicit_verdict:?} bmc={bmc_verdict:?})"
        );
        Some((explicit_verdict, explicit_result))
    };

    if !has_leadsto {
        assert_eq!(
            bfs_verdict, bmc_verdict,
            "'{id}': Monitor BFS and BMC disagree (bfs={bfs_verdict:?} bmc={bmc_verdict:?})"
        );
    }

    if let Some((_, explicit_result)) = &explicit
        && let Some(violation) = &explicit_result.violation
    {
        fsl_runtime::replay_trace(model.clone(), &violation.trace).unwrap_or_else(|error| {
            panic!("'{id}': explicit violation trace did not replay: {error}")
        });
    }
    if let Some(violation) = &bmc_result.violation {
        fsl_runtime::replay_trace(model.clone(), &violation.trace)
            .unwrap_or_else(|error| panic!("'{id}': BMC violation trace did not replay: {error}"));
    }

    sample_successor_admission(id, model, depth);

    bmc_verdict
}

/// Walk `model` concretely for up to `depth` steps taking the first enabled
/// action at each state (deterministic: no randomness), and assert every
/// concrete step is admitted by the symbolic transition relation for the
/// same action instance and successor
/// (`fsl_verifier::transition_matches_step`, design step 7).
fn sample_successor_admission(id: &str, model: &KernelModel, depth: usize) {
    let mut monitor = Monitor::new(model.clone())
        .unwrap_or_else(|error| panic!("'{id}': Monitor::new errored: {error}"));
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create Z3 solver");
    for _ in 0..depth {
        let enabled = monitor
            .enabled()
            .unwrap_or_else(|error| panic!("'{id}': Monitor::enabled errored: {error}"));
        let Some(action) = enabled.into_iter().next() else {
            break;
        };
        let current: State = monitor.state.clone();
        let stepped = monitor
            .step(&action)
            .unwrap_or_else(|error| panic!("'{id}': Monitor::step errored: {error}"));
        if stepped.violation.is_some() {
            break;
        }
        let admitted = block_on(fsl_verifier::transition_matches_step(
            model,
            &mut solver,
            &current,
            &stepped.action,
            &stepped.params,
            &stepped.state,
        ))
        .unwrap_or_else(|error| panic!("'{id}': transition_matches_step errored: {error}"));
        assert!(
            admitted,
            "'{id}': a real concrete step by '{}' was not admitted by the symbolic transition relation",
            stepped.action
        );
    }
}
