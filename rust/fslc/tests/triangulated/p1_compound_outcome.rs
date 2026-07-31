// SPDX-License-Identifier: Apache-2.0

use crate::claim::{
    AgreementEdges, Calibration, EvidenceRef, EvidenceState, ObservationEvidence, ObservationKind,
    ObserverEvidence, ScopeEvidence, TriangulatedClaim,
};
use crate::matrix_claim::Citation;
use crate::p1_mapping::{CliObservation, cli_result_to_session_action};
use serde_json::json;

const fn executable(path: &'static str, anchor: &'static str) -> EvidenceRef {
    EvidenceRef {
        by: Citation { path, anchor },
        state: EvidenceState::Executable,
    }
}

const REJECTING: &[EvidenceRef] = &[executable(
    "rust/fslc/tests/self_conformance.rs",
    "fn monitor_contract_violations_are_rejected()",
)];

/// Execute the same registered mapping seam used by the full C7 suite. The
/// common-mode fault replaces this selected function with the production
/// classifier before the registry checks its declared lineage.
pub fn exercise_registered_independent_observer() -> Result<(), String> {
    for (output, exit_code, expected) in [
        (json!({"result":"verified"}), 0, "verify_ok"),
        (
            json!({
                "result":"violated",
                "trace":[{"step":0,"state":{}}],
                "loc":{"line":1,"column":1},
                "violated_at_step":0
            }),
            1,
            "verify_violated",
        ),
    ] {
        let observed = CliObservation {
            subcommand: "verify",
            stdout_bytes: serde_json::to_vec(&output)
                .map_err(|error| format!("serialize P1 observation: {error}"))?,
            stderr_bytes: Vec::new(),
            output,
            exit_code,
            binary_revision: env!("FSLC_IMPLEMENTATION_FINGERPRINT"),
        };
        let action = cli_result_to_session_action(&observed)?;
        if action != expected {
            return Err(format!(
                "P1 observer mismatch: observation={observed:?} action={action:?} expected={expected:?}"
            ));
        }
    }
    Ok(())
}

pub fn claims() -> Vec<TriangulatedClaim> {
    vec![TriangulatedClaim {
        id: "p1.compound_outcome_conservation",
        contract: Citation {
            path: "docs/DESIGN-triangulated-assurance.md",
            anchor: "## P1 — compound outcome conservation",
        },
        common_observation: ObservationEvidence {
            observed_by: executable(
                "rust/fslc/tests/self_conformance.rs",
                "fn parse_cli_output(arguments: &[String], output: &Output) -> RawCliOutput",
            ),
            kind: ObservationKind::RawProcess,
            fields: &[
                "stdout_bytes",
                "stderr_bytes",
                "process_exit",
                "binary_revision",
            ],
        },
        model_observer: ObserverEvidence {
            observed_by: executable(
                "rust/fslc/tests/self_conformance.rs",
                "fn native_session_corpus_observations_replay_conformantly()",
            ),
            semantic_owner: "examples/self/fslc_session.fsl",
            semantic_lineage: &["self-spec session/fold/monitor", "native replay semantics"],
        },
        independent_observer: ObserverEvidence {
            observed_by: executable(
                "rust/fslc/tests/support/self_conformance_mapping.rs",
                "pub fn cli_result_to_session_action(",
            ),
            semantic_owner: "self_conformance independent mapping",
            semantic_lineage: &[
                "hand-written result/exit tuple mapping",
                "hand-written compound fold registry",
            ],
        },
        edges: AgreementEdges {
            model_world: executable(
                "rust/fslc/tests/self_conformance.rs",
                "fn native_session_corpus_observations_replay_conformantly()",
            ),
            oracle_world: executable(
                "rust/fslc/tests/self_conformance.rs",
                "fn session_mapping_rejects_result_exit_contradictions()",
            ),
            model_oracle: executable(
                "rust/fslc/tests/self_conformance.rs",
                "fn sweep_subverdicts_conform_to_the_fold_model()",
            ),
        },
        calibration: Calibration {
            accepting: executable(
                "rust/fslc/tests/self_conformance.rs",
                "fn native_monitor_observations_replay_conformantly()",
            ),
            rejecting: REJECTING,
            common_mode: Some(executable(
                "rust/fslc/tests/fault_operators/operators.txt",
                "shared-observer-lineage",
            )),
        },
        scope: ScopeEvidence {
            declared_by: Citation {
                path: "docs/DESIGN-triangulated-assurance.md",
                anchor: "## P1 — compound outcome conservation",
            },
            commands: &["check", "verify", "induction", "sweep", "chain", "analyze"],
            feature: "compound outcome conservation",
            domain: "registered native self-conformance corpus",
            backend: "native-z3 and solver-free replay",
            platform: "native Rust product gate platforms",
            corpus_revision: "working-tree citations plus FSLC_IMPLEMENTATION_FINGERPRINT",
        },
    }]
}
