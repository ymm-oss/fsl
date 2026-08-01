// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::Command;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, FslValue, KernelModel, Span, TraceStep};

use crate::claim::{
    AgreementEdges, Calibration, EvidenceRef, EvidenceState, ObservationEvidence, ObservationKind,
    ObserverEvidence, ScopeEvidence, TriangulatedClaim,
};
use crate::matrix_claim::Citation;

const fn executable(path: &'static str, anchor: &'static str) -> EvidenceRef {
    EvidenceRef {
        by: Citation { path, anchor },
        state: EvidenceState::Executable,
    }
}

const REJECTING: &[EvidenceRef] = &[executable(
    "rust/fslc/tests/triangulated/p2_witness_replay.rs",
    "fn corrupting_state_step_kind_or_location_cuts_a_p2_edge()",
)];

pub fn claims() -> Vec<TriangulatedClaim> {
    vec![TriangulatedClaim {
        id: "p2.symbolic_concrete_witness",
        contract: Citation {
            path: "docs/DESIGN-triangulated-assurance.md",
            anchor: "## P2 — symbolic witness / concrete replay agreement",
        },
        common_observation: ObservationEvidence {
            observed_by: executable(
                "rust/fslc/tests/triangulated/p2_witness_replay.rs",
                "fn symbolic_witness_agrees_with_concrete_replay_and_identity()",
            ),
            kind: ObservationKind::RawTrace,
            fields: &[
                "trace",
                "step",
                "state",
                "violation_kind",
                "failed_location",
            ],
        },
        model_observer: ObserverEvidence {
            observed_by: executable(
                "rust/fsl-verifier/src/bmc.rs",
                "pub async fn verify_bounded<S: SmtSolver>",
            ),
            semantic_owner: "fsl-verifier symbolic BMC",
            semantic_lineage: &["symbolic evaluator", "SMT transition relation"],
        },
        independent_observer: ObserverEvidence {
            observed_by: executable(
                "rust/fsl-runtime/src/explicit.rs",
                "pub fn verify_explicit(",
            ),
            semantic_owner: "fsl-runtime explicit/Monitor",
            semantic_lineage: &["concrete evaluator", "solver-free BFS and replay"],
        },
        edges: AgreementEdges {
            model_world: executable(
                "rust/fslc/tests/triangulated/p2_witness_replay.rs",
                "fn symbolic_witness_agrees_with_concrete_replay_and_identity()",
            ),
            oracle_world: executable(
                "rust/fslc/tests/triangulated/p2_witness_replay.rs",
                "fn concrete_oracle_check(model: &KernelModel, observed: &WitnessObservation)",
            ),
            model_oracle: executable(
                "rust/fslc/tests/triangulated/p2_witness_replay.rs",
                "fn symbolic_witness_agrees_with_concrete_replay_and_identity()",
            ),
        },
        calibration: Calibration {
            accepting: executable(
                "rust/fslc/tests/triangulated/p2_witness_replay.rs",
                "fn symbolic_witness_agrees_with_concrete_replay_and_identity()",
            ),
            rejecting: REJECTING,
            common_mode: None,
        },
        scope: ScopeEvidence {
            declared_by: Citation {
                path: "docs/DESIGN-triangulated-assurance.md",
                anchor: "## P2 — symbolic witness / concrete replay agreement",
            },
            commands: &["verify", "replay"],
            feature: "invariant violation witness identity",
            domain: "finite issue_502 relation model at depth 1",
            backend: "native-z3 BMC versus solver-free explicit/Monitor",
            platform: "native Rust product gate platforms",
            corpus_revision: "rust/fslc/tests/fixtures/issue_502_reachable_empty.fsl",
        },
    }]
}

#[derive(Clone, Debug)]
struct WitnessObservation {
    kind: String,
    name: String,
    step: usize,
    trace: Vec<TraceStep>,
    failed_location: Span,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn load_model() -> KernelModel {
    let path = repository_root().join("rust/fslc/tests/fixtures/issue_502_reachable_empty.fsl");
    let source = std::fs::read_to_string(&path).expect("read P2 fixture");
    let resolver = FsResolver::new(path.parent().expect("fixture parent"));
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse P2 fixture");
    fsl_core::build_model(kernel).expect("build P2 model")
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn property_span(model: &KernelModel, name: &str) -> Result<Span, String> {
    model
        .invariants
        .iter()
        .chain(model.transitions.iter())
        .find(|property| property.name == name)
        .map(|property| property.span)
        .ok_or_else(|| format!("no invariant/trans property named '{name}'"))
}

fn symbolic_observation(model: &KernelModel) -> WitnessObservation {
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create P2 symbolic solver");
    let result = block_on(fsl_verifier::verify_bounded(model, &mut solver, 1))
        .expect("run P2 symbolic verifier");
    let violation = result.violation.expect("P2 symbolic violation");
    WitnessObservation {
        failed_location: property_span(model, &violation.name).expect("symbolic property span"),
        kind: violation.kind,
        name: violation.name,
        step: violation.step,
        trace: violation.trace,
    }
}

fn concrete_oracle_check(model: &KernelModel, observed: &WitnessObservation) -> Result<(), String> {
    fsl_runtime::replay_trace(model.clone(), &observed.trace)
        .map_err(|error| format!("concrete replay rejected trace: {error}"))?;
    let explicit = fsl_runtime::verify_explicit(model.clone(), 1, 10_000)
        .map_err(|error| format!("explicit oracle failed: {error}"))?;
    let explicit = explicit
        .violation
        .ok_or_else(|| "explicit oracle produced no violation".to_owned())?;
    let expected_location = property_span(model, &explicit.violation.name)?;
    if observed.kind != explicit.violation.kind
        || observed.name != explicit.violation.name
        || observed.step != explicit.violation.step
        || observed.trace != explicit.trace
        || observed.failed_location != expected_location
    {
        return Err(format!(
            "symbolic/concrete witness edge mismatch: symbolic={observed:?}; concrete={explicit:?}; location={expected_location:?}"
        ));
    }
    Ok(())
}

#[test]
fn symbolic_witness_agrees_with_concrete_replay_and_identity() {
    let model = load_model();
    let observed = symbolic_observation(&model);
    concrete_oracle_check(&model, &observed).expect("all P2 edges agree");
}

#[test]
fn corrupting_each_witness_identity_field_cuts_a_p2_edge() {
    let model = load_model();
    let observed = symbolic_observation(&model);

    let mut state = observed.clone();
    state.trace[0]
        .state
        .insert("triangulated_corruption".to_owned(), FslValue::Bool(true));

    let mut step = observed.clone();
    step.trace[0].step += 1;

    let mut kind = observed.clone();
    kind.kind = "trans".to_owned();

    let mut name = observed.clone();
    name.name = "MutatedName".to_owned();

    let mut action = observed.clone();
    action.trace[1]
        .action
        .as_mut()
        .expect("step-one witness action")
        .name = "mutated_action".to_owned();

    let mut location = observed.clone();
    location.failed_location.start.line += 1;

    for (field, corrupt) in [
        ("state", state),
        ("step", step),
        ("kind", kind),
        ("name", name),
        ("action", action),
        ("location", location),
    ] {
        assert!(
            concrete_oracle_check(&model, &corrupt).is_err(),
            "corrupt {field} must cut replay or identity agreement"
        );
    }
}

/// Primary detector shared by the P2 semantic fault operators. It checks the
/// public projection rather than merely observing that some violation exists:
/// result, exit, kind/name, step, property location, final state, action and
/// bounded completeness must remain bound to the replayed witness.
#[test]
fn p2_cli_observation_preserves_full_witness_identity() {
    let fixture = repository_root().join("rust/fslc/tests/fixtures/issue_502_reachable_empty.fsl");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture.to_str().expect("UTF-8 path"),
            "--depth",
            "1",
        ])
        .output()
        .expect("run native P2 verifier");
    assert_eq!(output.status.code(), Some(1), "P2 violation exit");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse P2 JSON report");
    assert_eq!(report["result"], "violated");
    assert_eq!(report["violation_kind"], "invariant");
    assert_eq!(report["invariant"], "NoTrivialSelfReach");
    assert_eq!(report["violated_at_step"], 1);
    assert_eq!(report["loc"], serde_json::json!({"line": 17, "column": 3}));
    assert_eq!(report["checked_to_depth"], 1);
    assert_eq!(report["completeness"], "bounded");
    assert_eq!(report["trace"][0]["step"], 0);
    assert_eq!(report["trace"][0]["state"]["r"], serde_json::json!([]));
    assert_eq!(report["trace"][1]["step"], 1);
    assert_eq!(
        report["trace"][1]["state"]["r"],
        serde_json::json!([[0, 0]])
    );
    assert_eq!(report["trace"][1]["action"]["name"], "link");
    assert_eq!(
        report["trace"][1]["action"]["params"],
        serde_json::json!({"a": 0, "b": 0})
    );
    assert_eq!(report["last_action"]["name"], "link");
    assert_eq!(
        report["last_action"]["loc"],
        serde_json::json!({"line": 13, "column": 3})
    );
}

#[test]
fn p2_cli_transition_identity_resolves_the_matching_property() {
    let fixture = repository_root().join("rust/fslc/tests/fixtures/assurance_trans_violation.fsl");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture.to_str().expect("UTF-8 path"),
            "--depth",
            "1",
        ])
        .output()
        .expect("run native P2 transition verifier");
    assert_eq!(output.status.code(), Some(1), "transition violation exit");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse transition JSON report");
    assert_eq!(report["result"], "violated");
    assert_eq!(report["violation_kind"], "trans");
    assert_eq!(report["trans"], "NeverDecrease");
    assert_eq!(report["invariant"], "NeverDecrease");
    assert_eq!(report["loc"], serde_json::json!({"line": 16, "column": 3}));
    assert_eq!(report["violated_at_step"], 1);
    assert_eq!(report["last_action"]["name"], "dec");
    assert_eq!(
        report["last_action"]["loc"],
        serde_json::json!({"line": 11, "column": 3})
    );
}
