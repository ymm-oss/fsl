// SPDX-License-Identifier: Apache-2.0

//! Federated Triangulated Assurance registry and aggregator (#670).

#[path = "triangulated/claim.rs"]
mod claim;
#[allow(dead_code)]
#[path = "assurance/claim.rs"]
mod matrix_claim;
#[path = "triangulated/p1_compound_outcome.rs"]
mod p1_compound_outcome;
#[path = "support/self_conformance_mapping.rs"]
mod p1_mapping;
#[path = "triangulated/p2_witness_replay.rs"]
mod p2_witness_replay;
#[path = "triangulated/p3_dialect_dispatch.rs"]
mod p3_dialect_dispatch;

use claim::{Registry, TriangulatedClaim};

const REQUIRED_CLAIMS: &[&str] = &[
    "p1.compound_outcome_conservation",
    "p2.symbolic_concrete_witness",
    "p3.token_dialect_dispatch",
];

fn registry() -> Registry {
    let claims: Vec<TriangulatedClaim> = [
        p1_compound_outcome::claims(),
        p2_witness_replay::claims(),
        p3_dialect_dispatch::claims(),
    ]
    .into_iter()
    .flatten()
    .collect();
    Registry {
        required_ids: REQUIRED_CLAIMS,
        claims,
    }
}

#[test]
fn every_required_triangulated_claim_is_registered_exactly_once() {
    registry()
        .check_complete()
        .expect("triangulated registry completeness");
}

#[test]
fn every_registered_claim_has_raw_independent_executable_evidence() {
    p1_compound_outcome::exercise_registered_independent_observer()
        .expect("P1 registered independent observer");
    registry()
        .check_claims()
        .expect("triangulated claim validation");
}

#[test]
fn registry_has_a_calibrated_common_mode_fault() {
    registry()
        .check_common_mode_calibration()
        .expect("common-mode calibration");
}

#[cfg(test)]
mod negative_controls {
    use super::*;
    use crate::claim::{EvidenceState, ObservationKind};

    #[test]
    fn missing_claim_and_stale_claim_fail_completeness() {
        let mut claims = registry().claims;
        claims.pop();
        claims.push(claims[0]);
        let broken = Registry {
            required_ids: REQUIRED_CLAIMS,
            claims,
        };
        assert!(broken.check_complete().is_err());
    }

    #[test]
    fn missing_edge_and_fabricated_citation_fail_closed() {
        let mut broken = registry().claims[0];
        broken.edges.model_world.by.anchor = "fabricated-edge-anchor-670";
        assert!(broken.check().is_err());
    }

    #[test]
    fn skipped_or_unknown_edge_is_not_executable_evidence() {
        for state in [EvidenceState::Skipped, EvidenceState::Unknown] {
            let mut broken = registry().claims[0];
            broken.edges.oracle_world.state = state;
            assert!(broken.check().is_err(), "state {state:?} must fail");
        }
    }

    #[test]
    fn shared_semantic_decision_owner_or_lineage_fails_independence() {
        let mut shared_owner = registry().claims[0];
        shared_owner.independent_observer.semantic_owner =
            shared_owner.model_observer.semantic_owner;
        assert!(shared_owner.check().is_err());

        let mut shared_lineage = registry().claims[0];
        shared_lineage.independent_observer.semantic_lineage =
            shared_lineage.model_observer.semantic_lineage;
        assert!(shared_lineage.check().is_err());
    }

    #[test]
    fn preclassified_verdict_and_missing_raw_fields_are_rejected() {
        let mut preclassified = registry().claims[0];
        preclassified.common_observation.kind = ObservationKind::PreclassifiedVerdict;
        assert!(preclassified.check().is_err());

        let mut incomplete = registry().claims[0];
        incomplete.common_observation.fields = &["stdout_bytes", "process_exit"];
        assert!(incomplete.check().is_err());
    }

    #[test]
    fn missing_calibration_is_detected_at_registry_boundary() {
        let mut missing_rejecting = registry().claims[0];
        missing_rejecting.calibration.rejecting = &[];
        assert!(missing_rejecting.check().is_err());

        let claims = registry()
            .claims
            .into_iter()
            .map(|mut claim| {
                claim.calibration.common_mode = None;
                claim
            })
            .collect();
        let broken = Registry {
            required_ids: REQUIRED_CLAIMS,
            claims,
        };
        assert!(broken.check_common_mode_calibration().is_err());
    }
}
