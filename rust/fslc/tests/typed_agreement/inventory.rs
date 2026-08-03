// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;

pub fn inventory() -> Value {
    serde_json::from_str(include_str!("inventory.v1.json")).expect("valid FSL Logic inventory JSON")
}

#[test]
fn generation_inventory_is_coupled_to_registries_and_test_anchors() {
    let inventory = inventory();
    assert_eq!(inventory["schema"], "fslc.fsl-logic-inventory.v1");
    let dialects = inventory["dialects"]
        .as_object()
        .expect("dialect posture object");
    let declared = dialects.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let registered = fsl_syntax::DIALECT_KEYWORDS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        declared, registered,
        "new or removed dialect requires an explicit FSL Logic posture"
    );
    assert!(
        dialects
            .values()
            .all(|posture| posture.as_str().is_some_and(|text| !text.trim().is_empty())),
        "every dialect needs a non-empty posture"
    );

    let relations_source = include_str!("relations.rs");
    let relations = inventory["metamorphic_relations"]
        .as_array()
        .expect("metamorphic relation inventory");
    assert_eq!(relations.len(), 7, "R1-R7 inventory must stay complete");
    for relation in relations {
        for key in ["positive", "negative"] {
            let anchor = relation[key].as_str().expect("relation test anchor");
            assert_eq!(
                relations_source.matches(anchor).count(),
                1,
                "metamorphic {key} anchor '{anchor}' must occur exactly once"
            );
        }
    }

    let suite_source = include_str!("../typed_agreement.rs");
    let companion_axes = inventory["companion_axes"]
        .as_object()
        .expect("companion axis inventory");
    for axis in ["expression_and_type_variants", "partial_operations"] {
        let entry = companion_axes[axis]
            .as_object()
            .expect("companion axis row");
        for key in ["evidence_test", "boundary_evidence_test"] {
            let Some(anchor) = entry.get(key).and_then(Value::as_str) else {
                continue;
            };
            assert_eq!(
                suite_source.matches(anchor).count() + relations_source.matches(anchor).count(),
                1,
                "companion axis {axis} has stale {key} anchor '{anchor}'"
            );
        }
    }
    let partial_operations = companion_axes["partial_operations"]["values"]
        .as_array()
        .expect("partial operation values")
        .iter()
        .map(|value| value.as_str().expect("partial operation name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        partial_operations,
        BTreeSet::from(["head", "pop", "at", "index", "divide", "remainder"]),
        "head/pop/at/index/divide/remainder inventory must stay complete"
    );
}
