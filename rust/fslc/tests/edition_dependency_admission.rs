// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_dependency_cases.rs"]
mod cases;
#[path = "support/edition_cases.rs"]
mod common;

use cases::{Observations, Variant};
use serde_json::{Value, json};

fn dependency_edition_cli_compose_and_implements_refusals_and_mixed_controls() {
    let mut results = Observations::default();
    for row in cases::flat_rows().into_iter().filter(|row| {
        !matches!(
            row.id.split('/').next().unwrap(),
            "E07" | "E08" | "E09" | "E10" | "E11" | "E12"
        )
    }) {
        let fixture = cases::fixture(&row);
        let scratch = common::Scratch::new();
        fixture.materialize(&scratch, "root_compose.fsl");
        for cmd in ["check", "verify"] {
            let args = if cmd == "check" {
                "check root_compose.fsl"
            } else {
                "verify root_compose.fsl --depth 2 --no-cache"
            };
            let expected = if cmd == "check" || row.variant == Variant::N {
                cases::expected_check(&row, &fixture, &fixture.child, cases::fsl_envelope())
            } else {
                cases::expected_verify(&row, &fixture, &fixture.child)
            };
            let output = common::command(env!("CARGO_BIN_EXE_fslc"), &scratch.0, args, true);
            let _ = cases::versioned_native_value(common::value(&output));
            let label = format!("{}/{cmd}", row.id);
            results.output(
                &label,
                &output,
                expected,
                if row.variant == Variant::N { 2 } else { 0 },
            );
        }
    }
    results.finish();
}

#[test]
fn dependency_edition_cli_adapter_executes_all_assigned_rows() {
    cases::exercise(
        "cli",
        &[dependency_edition_cli_compose_and_implements_refusals_and_mixed_controls],
    );
}

#[test]
fn dependency_edition_fixture_roles_are_valid() {
    let mut observations = Observations::default();
    for row in cases::flat_rows() {
        let fixture = cases::fixture(&row);
        let source = &fixture.files[&fixture.child];
        let result = fslc_rust::migration::admit_source(
            source,
            &fixture.child,
            None,
            &common::gated(),
            &serde_json::Map::new,
            false,
        );
        let actual: Value = match result {
            Ok(edition) => json!({"accepted":true,"warnings":edition.warnings()}),
            Err((value, status)) => json!({"accepted":false,"value":value,"status":status}),
        };
        let expected = if row.variant == Variant::N {
            json!({"accepted":false,"value":cases::refusal(source,&fixture.child,serde_json::Map::new()),"status":2})
        } else {
            json!({"accepted":true,"warnings":if row.variant==Variant::W {cases::findings(source,&fixture.child,"current")} else {Vec::new()}})
        };
        observations.eq(&row.id, &actual, &expected);
    }
    observations.finish();
}

#[test]
fn dependency_cost_exclusions_cannot_be_absent_on_both_sides() {
    let envelope = json!({"fsl":"1.0","result":"verified","cost":{"elapsed_s":0.1,"solver":{"checks":1,"check_elapsed_s":0.01,"memory_mb":17.0,"conflicts":0,"decisions":null,"propagations":1},"properties":[{"kind":"invariant","name":"P","checks":1,"elapsed_s":0.01}]}});
    assert_eq!(cases::stable(envelope.clone()).1.len(), 7);
    for pointer in [
        "/cost/elapsed_s",
        "/cost/solver/check_elapsed_s",
        "/cost/solver/memory_mb",
        "/cost/solver/conflicts",
        "/cost/solver/decisions",
        "/cost/solver/propagations",
        "/cost/properties/0/elapsed_s",
    ] {
        let mut missing = envelope.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        missing
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(
            std::panic::catch_unwind(|| cases::stable(missing.clone()) == cases::stable(missing))
                .is_err(),
            "dead exclusion must fail: {pointer}"
        );
    }
}
