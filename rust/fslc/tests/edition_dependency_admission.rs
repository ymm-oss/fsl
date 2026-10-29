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
        let child = scratch.0.join(&fixture.child).canonicalize().unwrap();
        for cmd in ["check", "verify"] {
            let args = if cmd == "check" {
                "check root_compose.fsl"
            } else {
                "verify root_compose.fsl --depth 2 --no-cache"
            };
            let expected = if cmd == "check" || row.variant == Variant::N {
                cases::expected_check(
                    &row,
                    &fixture,
                    child.to_str().unwrap(),
                    cases::fsl_envelope(),
                )
            } else {
                cases::twin_expected(&row, &fixture, &scratch, || {
                    let baseline =
                        common::command(env!("CARGO_BIN_EXE_fslc"), &scratch.0, args, true);
                    assert_eq!(
                        baseline.status.code(),
                        Some(0),
                        "canonical verification must finish"
                    );
                    let expected = cases::versioned_native_value(common::value(&baseline));
                    assert_eq!(expected["result"], "verified");
                    expected
                })
            };
            let output = common::command(env!("CARGO_BIN_EXE_fslc"), &scratch.0, args, true);
            let actual = cases::versioned_native_value(common::value(&output));
            let label = format!("{}/{cmd}", row.id);
            if row.variant == Variant::N {
                results.eq(
                    &label,
                    (output.status.code(), &actual, &output.stderr),
                    (Some(2), &expected, &Vec::<u8>::new()),
                );
                // A standalone classification is calibrated against literals,
                // never used to manufacture the dependency's expected finding.
                if row.edge != "checked_by" {
                    let standalone = common::invoke(
                        env!("CARGO_BIN_EXE_fslc"),
                        &["check".into(), child.to_string_lossy().into_owned()],
                        &scratch.0,
                        true,
                    );
                    let standalone_value =
                        cases::versioned_native_value(common::value(&standalone));
                    results.eq(
                        &format!("{label}/standalone"),
                        (&standalone_value, standalone.status.code()),
                        (&expected, Some(2)),
                    );
                }
            } else {
                let actual = cases::stable(actual);
                let expected = cases::stable(expected);
                if cmd == "verify" {
                    assert!(
                        actual.1.contains("/cost/elapsed_s")
                            && expected.1.contains("/cost/elapsed_s")
                    );
                }
                results.eq(
                    &label,
                    (output.status.code(), &actual, &output.stderr),
                    (Some(0), &expected, &Vec::<u8>::new()),
                );
            }
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
    let envelope = json!({"fsl":"1.0","result":"verified","cost":{"elapsed_s":0.1,"solver":{"checks":1,"check_elapsed_s":0.01,"memory_mb":17.0},"properties":[{"kind":"invariant","name":"P","checks":1,"elapsed_s":0.01}]}});
    assert_eq!(cases::stable(envelope.clone()).1.len(), 4);
    for pointer in [
        "/cost/elapsed_s",
        "/cost/solver/check_elapsed_s",
        "/cost/solver/memory_mb",
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
