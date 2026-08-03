// SPDX-License-Identifier: Apache-2.0

use super::{engines, generator};

/// Minimize explicit model structure while preserving the same named semantic
/// failure. Every candidate re-enters the ordinary parser/typechecker gate.
pub fn shrink_case(
    original: &generator::LogicCase,
    signature: &str,
    fails_with: impl Fn(&generator::LogicCase) -> Option<String>,
) -> generator::LogicCase {
    let mut best = original.clone();
    loop {
        let mut reduced = false;
        for candidate in generator::structural_shrink_candidates(&best) {
            let _ = engines::build(&candidate.case_id, &candidate.source);
            if fails_with(&candidate).as_deref() == Some(signature) {
                best = candidate;
                reduced = true;
                break;
            }
        }
        if !reduced {
            return best;
        }
    }
}

#[test]
fn structural_shrinker_preserves_named_failure_and_replays_minimum_case() {
    let original = generator::logic_case(672_673, 11);
    let signature = "explicit_bmc:verdict";
    let minimized = shrink_case(&original, signature, |case| {
        let model = engines::build(&case.case_id, &case.source);
        let observation = engines::compare_agreement(&case.case_id, &model, case.depth)
            .expect("uncorrupted baseline agreement");
        let failure = engines::comparator_negative_control(&case.case_id, &observation);
        Some(format!("{}:{}", failure.edge, failure.field))
    });
    assert_eq!(minimized.seed, original.seed);
    assert_eq!(minimized.index, original.index);
    assert_eq!(minimized.domain_kind, generator::DomainKind::Range);
    assert_eq!(minimized.domain_size, 1);
    assert_eq!(minimized.state_vars, 1);
    assert_eq!(minimized.action_count, 1);
    assert!(!minimized.guarded);
    assert!(!minimized.fair);
    assert!(!minimized.expected_violation);
    assert_eq!(minimized.depth, 1);
    assert_eq!(minimized.property_kind, generator::PropertyKind::Invariant);
    assert!(minimized.source.len() < original.source.len());

    let model = engines::build(&minimized.case_id, &minimized.source);
    let observation = engines::compare_agreement(&minimized.case_id, &model, minimized.depth)
        .expect("replay baseline agreement");
    let failure = engines::comparator_negative_control(&minimized.case_id, &observation);
    assert_eq!(format!("{}:{}", failure.edge, failure.field), signature);
}

#[test]
fn generated_expectation_rejects_a_common_mode_false_negative() {
    let case = generator::logic_case(672_673, 0);
    assert_eq!(case.expected_violation_step, Some(0));
    let model = engines::build(&case.case_id, &case.source);
    let mut observation = engines::compare_agreement(&case.case_id, &model, case.depth)
        .expect("all engines observe the injected violation");
    observation.verdict = engines::Verdict::Clean;
    let failure = engines::require_expected_violation(
        &case.case_id,
        &observation,
        case.expected_violation_step,
    )
    .expect_err("the external generated expectation must reject common false agreement");
    assert_eq!(failure.edge, "generated_expectation");
    assert_eq!(failure.field, "violation_step");
}
