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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{
    FsResolver, FslValue as Value, KernelModel, build_model, build_surface_model,
    parse_kernel_source,
};
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgreementFailure {
    pub case_id: String,
    pub edge: String,
    pub field: String,
    pub left: String,
    pub right: String,
}

impl fmt::Display for AgreementFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "case={} edge={} field={} left={} right={}",
            self.case_id, self.edge, self.field, self.left, self.right
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgreementObservation {
    pub verdict: Verdict,
    pub deadlock_step: Option<usize>,
    pub required_edges: Vec<&'static str>,
    pub property_location: Option<String>,
    pub completeness: AgreementCompleteness,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgreementCompleteness {
    pub requested_depth: usize,
    pub monitor_depth_reached: usize,
    pub explicit_depth_reached: Option<usize>,
    pub monitor_closure: bool,
    pub explicit_closure: Option<bool>,
    pub bmc_frontier_progress: bool,
}

struct MonitorObservation {
    verdict: Verdict,
    deadlock_step: Option<usize>,
    depth_reached: usize,
    closure: bool,
}

fn monitor_terminal_holds(id: &str, monitor: &Monitor) -> Result<bool, AgreementFailure> {
    let Some(terminal) = &monitor.model.terminal else {
        return Ok(false);
    };
    match fsl_runtime::eval(
        terminal,
        &monitor.state,
        &mut BTreeMap::new(),
        &monitor.model,
        None,
    )
    .map_err(|error| {
        disagreement(
            id,
            "earliest_deadlock",
            "terminal",
            "Boolean",
            error.message,
        )
    })? {
        Value::Bool(value) => Ok(value),
        value => Err(disagreement(
            id,
            "earliest_deadlock",
            "terminal",
            "Boolean",
            value,
        )),
    }
}

fn disagreement(
    id: &str,
    edge: &str,
    field: &str,
    left: impl fmt::Debug,
    right: impl fmt::Debug,
) -> AgreementFailure {
    AgreementFailure {
        case_id: id.to_owned(),
        edge: edge.to_owned(),
        field: field.to_owned(),
        left: format!("{left:?}"),
        right: format!("{right:?}"),
    }
}

fn require_equal<T: Eq + fmt::Debug>(
    id: &str,
    edge: &str,
    field: &str,
    left: &T,
    right: &T,
) -> Result<(), AgreementFailure> {
    if left == right {
        Ok(())
    } else {
        Err(disagreement(id, edge, field, left, right))
    }
}

pub fn require_deadlock_agreement(
    id: &str,
    monitor: Option<usize>,
    legacy_bfs: Option<usize>,
    explicit: Option<usize>,
    symbolic: Option<usize>,
) -> Result<(), AgreementFailure> {
    require_equal(
        id,
        "earliest_deadlock",
        "monitor_bfs",
        &monitor,
        &legacy_bfs,
    )?;
    require_equal(
        id,
        "earliest_deadlock",
        "bfs_explicit",
        &legacy_bfs,
        &explicit,
    )?;
    require_equal(
        id,
        "earliest_deadlock",
        "explicit_bmc",
        &explicit,
        &symbolic,
    )
}

fn property_location(
    id: &str,
    model: &KernelModel,
    verdict: &Verdict,
) -> Result<Option<String>, AgreementFailure> {
    let Verdict::Violated { kind, name, .. } = verdict else {
        return Ok(None);
    };
    let span = match kind.as_str() {
        "invariant" => model
            .invariants
            .iter()
            .find(|property| property.name == *name)
            .map(|property| property.span),
        "trans" => model
            .transitions
            .iter()
            .find(|property| property.name == *name)
            .map(|property| property.span),
        "ensures" => model
            .actions
            .iter()
            .find(|action| action.name == *name)
            .map(|action| action.span),
        _ => return Ok(Some(format!("implicit:{kind}:{name}"))),
    }
    .ok_or_else(|| disagreement(id, "property_location", "property", name, "missing"))?;
    Ok(Some(format!("{}:{}", span.start.line, span.start.column)))
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

/// Independently drive `Monitor` over the finite frontier. This deliberately
/// does not call either production BFS implementation: it calibrates that
/// their normalized verdict and concrete closure metadata still agree with
/// the public step/current-violation contract exposed by `Monitor` itself.
fn observe_monitor(
    id: &str,
    model: &KernelModel,
    depth: usize,
) -> Result<MonitorObservation, AgreementFailure> {
    let initial = Monitor::new(model.clone())
        .map_err(|error| disagreement(id, "monitor_bfs", "engine", "ok", error.message))?;
    let mut frontier = BTreeMap::from([(initial.state.clone(), initial)]);
    let mut seen: BTreeSet<_> = frontier.keys().cloned().collect();
    let mut deadlock_step = None;

    for level in 0..=depth {
        for monitor in frontier.values() {
            let violation = monitor.current_violation().map_err(|error| {
                disagreement(id, "monitor_bfs", "current_violation", "ok", error.message)
            })?;
            if violation.is_some() {
                return Ok(MonitorObservation {
                    verdict: Verdict::from(violation),
                    deadlock_step,
                    depth_reached: level,
                    closure: false,
                });
            }
        }

        let mut next = BTreeMap::new();
        for monitor in frontier.values() {
            let enabled = monitor
                .enabled()
                .map_err(|error| disagreement(id, "monitor_bfs", "enabled", "ok", error.message))?;
            if enabled.is_empty()
                && deadlock_step.is_none()
                && !monitor_terminal_holds(id, monitor)?
            {
                deadlock_step = Some(level);
            }
            if level == depth {
                continue;
            }
            for action in enabled {
                let mut child = monitor.clone();
                let stepped = child.step(&action).map_err(|error| {
                    disagreement(id, "monitor_bfs", "step", "ok", error.message)
                })?;
                if stepped.violation.is_some() {
                    return Ok(MonitorObservation {
                        verdict: Verdict::from(stepped.violation),
                        deadlock_step,
                        depth_reached: level + 1,
                        closure: false,
                    });
                }
                if seen.insert(child.state.clone()) {
                    next.insert(child.state.clone(), child);
                }
            }
        }
        if level == depth {
            return Ok(MonitorObservation {
                verdict: Verdict::Clean,
                deadlock_step,
                depth_reached: level,
                closure: false,
            });
        }
        if next.is_empty() {
            return Ok(MonitorObservation {
                verdict: Verdict::Clean,
                deadlock_step,
                depth_reached: level,
                closure: true,
            });
        }
        frontier = next;
    }
    unreachable!("bounded Monitor exploration returns from every loop path")
}

pub fn require_expected_violation(
    id: &str,
    observation: &AgreementObservation,
    expected_violation_step: Option<usize>,
) -> Result<(), AgreementFailure> {
    let actual_violation_step = match &observation.verdict {
        Verdict::Clean => None,
        Verdict::Violated { step, .. } => Some(*step),
    };
    require_equal(
        id,
        "generated_expectation",
        "violation_step",
        &expected_violation_step,
        &actual_violation_step,
    )
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
    compare_agreement(id, model, depth)
        .unwrap_or_else(|failure| panic!("FSL Logic agreement failed: {failure}"))
        .verdict
}

/// Compare the solver-free concrete lineage with native symbolic BMC without
/// majority voting. Every required edge is named so a failing generated case
/// has a stable semantic signature suitable for replay and shrinking.
// Keep the ordered engine/edge transaction visible in one function: splitting
// it would make it easier to return an observation before every edge ran.
#[allow(clippy::too_many_lines)]
pub fn compare_agreement(
    id: &str,
    model: &KernelModel,
    depth: usize,
) -> Result<AgreementObservation, AgreementFailure> {
    let monitor = observe_monitor(id, model, depth)?;
    let bfs_result = fsl_runtime::bfs(model.clone(), depth)
        .map_err(|error| disagreement(id, "monitor_bfs", "engine", "ok", error.message))?;
    let bfs_verdict = Verdict::from(bfs_result.violation.clone());
    require_equal(id, "monitor_bfs", "verdict", &monitor.verdict, &bfs_verdict)?;
    require_equal(
        id,
        "depth_completeness",
        "bfs_requested_depth",
        &depth,
        &bfs_result.depth,
    )?;

    let mut solver = fsl_solver_z3::Z3Solver::new()
        .map_err(|error| disagreement(id, "explicit_bmc", "solver", "created", error))?;
    let bmc_result = block_on(fsl_verifier::verify_bounded(model, &mut solver, depth))
        .map_err(|error| disagreement(id, "explicit_bmc", "engine", "ok", error.message))?;
    let bmc_verdict = Verdict::from(bmc_result.violation.clone().map(|violation| Violation {
        kind: violation.kind,
        name: violation.name,
        step: violation.step,
    }));
    require_equal(
        id,
        "depth_completeness",
        "bmc_requested_depth",
        &depth,
        &bmc_result.depth,
    )?;

    let has_leadsto = !model.leadstos.is_empty();
    let mut trace_compared = false;
    let explicit =
        if has_leadsto {
            let reason = fsl_runtime::explicit_unsupported_reason(model).ok_or_else(|| {
                disagreement(id, "bfs_explicit", "support", "leadsTo excluded", "allowed")
            })?;
            if !reason.contains("leadsTo") {
                return Err(disagreement(
                    id,
                    "bfs_explicit",
                    "support",
                    "leadsTo rejection",
                    reason,
                ));
            }
            None
        } else {
            let explicit_result = fsl_runtime::verify_explicit(model.clone(), depth, 1_000_000)
                .map_err(|error| disagreement(id, "bfs_explicit", "engine", "ok", error.message))?;
            let explicit_verdict = Verdict::from(explicit_result.violation.clone().map(
                |violation| Violation {
                    kind: violation.violation.kind,
                    name: violation.violation.name,
                    step: violation.violation.step,
                },
            ));
            require_equal(
                id,
                "depth_completeness",
                "explicit_requested_depth",
                &depth,
                &explicit_result.depth,
            )?;
            require_equal(
                id,
                "depth_completeness",
                "concrete_depth_reached",
                &monitor.depth_reached,
                &explicit_result.depth_reached,
            )?;
            require_equal(
                id,
                "depth_completeness",
                "concrete_closure",
                &monitor.closure,
                &explicit_result.closure,
            )?;
            require_equal(
                id,
                "bfs_explicit",
                "verdict",
                &bfs_verdict,
                &explicit_verdict,
            )?;
            require_equal(
                id,
                "explicit_bmc",
                "verdict",
                &explicit_verdict,
                &bmc_verdict,
            )?;

            // Engines intentionally stop their auxiliary observations at
            // different points after a violation. Reachable steps and action
            // coverage therefore have a common contract only for clean runs.
            if matches!(bmc_verdict, Verdict::Clean) {
                require_deadlock_agreement(
                    id,
                    monitor.deadlock_step,
                    bfs_result.deadlock_step,
                    explicit_result.deadlock_step,
                    bmc_result.deadlock_step,
                )?;
                let bfs_reachables = bfs_result
                    .reachables
                    .iter()
                    .map(|(name, witness)| (name.clone(), witness.as_ref().map(|item| item.step)))
                    .collect::<std::collections::BTreeMap<_, _>>();
                let explicit_reachables = explicit_result
                    .reachables
                    .iter()
                    .map(|(name, witness)| (name.clone(), witness.as_ref().map(|item| item.step)))
                    .collect::<std::collections::BTreeMap<_, _>>();
                let bmc_reachables = bmc_result
                    .reachables
                    .iter()
                    .map(|(name, witness)| (name.clone(), witness.as_ref().map(|item| item.step)))
                    .collect::<std::collections::BTreeMap<_, _>>();
                require_equal(
                    id,
                    "bfs_explicit",
                    "reachables",
                    &bfs_reachables,
                    &explicit_reachables,
                )?;
                require_equal(
                    id,
                    "explicit_bmc",
                    "reachables",
                    &explicit_reachables,
                    &bmc_reachables,
                )?;
                require_equal(
                    id,
                    "bfs_explicit",
                    "action_coverage",
                    &bfs_result.action_coverage,
                    &explicit_result.action_coverage,
                )?;
                require_equal(
                    id,
                    "explicit_bmc",
                    "action_coverage",
                    &explicit_result.action_coverage,
                    &bmc_result.action_coverage,
                )?;
            }
            if explicit_result.budget_exceeded {
                return Err(disagreement(
                    id,
                    "bfs_explicit",
                    "budget_exceeded",
                    false,
                    true,
                ));
            }
            if let (Some(explicit_violation), Some(bmc_violation)) =
                (&explicit_result.violation, &bmc_result.violation)
            {
                require_equal(
                    id,
                    "trace_explicit_bmc",
                    "trace",
                    &explicit_violation.trace,
                    &bmc_violation.trace,
                )?;
                trace_compared = explicit_violation.trace.len() > 1
                    && explicit_violation
                        .trace
                        .last()
                        .is_some_and(|step| step.action.is_some());
            }
            Some((explicit_verdict, explicit_result))
        };

    if let Some((_, explicit_result)) = &explicit
        && let Some(violation) = &explicit_result.violation
    {
        fsl_runtime::replay_trace(model.clone(), &violation.trace).map_err(|error| {
            disagreement(id, "replay", "explicit_trace", "replayable", error.message)
        })?;
    }
    if let Some(violation) = &bmc_result.violation {
        fsl_runtime::replay_trace(model.clone(), &violation.trace).map_err(|error| {
            disagreement(id, "replay", "bmc_trace", "replayable", error.message)
        })?;
    }

    sample_successor_admission(id, model, depth)?;

    let property_location = property_location(id, model, &bmc_verdict)?;
    let has_violation = !matches!(&bmc_verdict, Verdict::Clean);

    Ok(AgreementObservation {
        verdict: bmc_verdict,
        deadlock_step: bmc_result.deadlock_step,
        required_edges: if has_leadsto {
            vec!["depth_completeness", "replay", "successor_admission"]
        } else {
            let mut edges = vec![
                "monitor_bfs",
                "bfs_explicit",
                "explicit_bmc",
                "depth_completeness",
                "replay",
                "successor_admission",
            ];
            if trace_compared {
                edges.push("trace_explicit_bmc");
            }
            if !has_violation {
                edges.push("earliest_deadlock");
            }
            if has_violation {
                edges.push("property_location");
            }
            edges
        },
        property_location,
        completeness: AgreementCompleteness {
            requested_depth: depth,
            monitor_depth_reached: monitor.depth_reached,
            explicit_depth_reached: explicit.as_ref().map(|(_, result)| result.depth_reached),
            monitor_closure: monitor.closure,
            explicit_closure: explicit.as_ref().map(|(_, result)| result.closure),
            bmc_frontier_progress: bmc_result.frontier_progress,
        },
    })
}

/// Deliberately corrupt one already-normalized symbolic observation. This is
/// test-only calibration evidence that the named edge rejects disagreement;
/// it never changes an engine or shipped product path.
pub fn comparator_negative_control(
    id: &str,
    observation: &AgreementObservation,
) -> AgreementFailure {
    let corrupted = match &observation.verdict {
        Verdict::Clean => Verdict::Violated {
            kind: "invariant".to_owned(),
            name: "ComparatorControl".to_owned(),
            step: 0,
        },
        Verdict::Violated { .. } => Verdict::Clean,
    };
    require_equal(
        id,
        "explicit_bmc",
        "verdict",
        &observation.verdict,
        &corrupted,
    )
    .expect_err("deliberate comparator corruption must be rejected")
}

/// Walk `model` concretely for up to `depth` steps taking the first enabled
/// action at each state (deterministic: no randomness), and assert every
/// concrete step is admitted by the symbolic transition relation for the
/// same action instance and successor
/// (`fsl_verifier::transition_matches_step`, design step 7).
fn sample_successor_admission(
    id: &str,
    model: &KernelModel,
    depth: usize,
) -> Result<(), AgreementFailure> {
    let mut monitor = Monitor::new(model.clone())
        .map_err(|error| disagreement(id, "successor_admission", "monitor", "ok", error.message))?;
    let mut solver = fsl_solver_z3::Z3Solver::new()
        .map_err(|error| disagreement(id, "successor_admission", "solver", "created", error))?;
    for _ in 0..depth {
        let enabled = monitor.enabled().map_err(|error| {
            disagreement(id, "successor_admission", "enabled", "ok", error.message)
        })?;
        let Some(action) = enabled.into_iter().next() else {
            break;
        };
        let current: State = monitor.state.clone();
        let stepped = monitor.step(&action).map_err(|error| {
            disagreement(id, "successor_admission", "step", "ok", error.message)
        })?;
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
        .map_err(|error| {
            disagreement(
                id,
                "successor_admission",
                "symbolic_transition",
                "ok",
                error.message,
            )
        })?;
        if !admitted {
            return Err(disagreement(
                id,
                "successor_admission",
                "transition",
                "admitted",
                stepped.action,
            ));
        }
    }
    Ok(())
}
