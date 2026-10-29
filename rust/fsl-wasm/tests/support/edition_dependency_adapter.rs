// SPDX-License-Identifier: Apache-2.0

#[path = "../../../fslc/tests/support/edition_dependency_cases.rs"]
mod cases;
use super::cases as common;
use super::{TEST_SOLVER_VERSION, block_on, native_z3};
use crate::{Request, check_with};

fn expected_envelope() -> serde_json::Map<String, serde_json::Value> {
    serde_json::from_value(serde_json::json!({"fsl":"1.0","versions":{
        "verifier":{"name":"fsl-wasm","version":env!("CARGO_PKG_VERSION")},
        "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
        "solver":{"name":"z3","backend":"z3-solver-wasm","version":TEST_SOLVER_VERSION}}}))
    .unwrap()
}

fn dependency_edition_worker_check_uses_child_edition_and_ignores_unused_files() {
    let mut observations = cases::Observations::default();
    for row in cases::worker_rows() {
        let fixture = cases::fixture(&row);
        let request: Request = serde_json::from_value(serde_json::json!({"cmd":"check","source":fixture.root,"source_file":"root_compose.fsl","files":fixture.files,"options":{"depth":2},"unstable":["fsl5"]})).unwrap();
        let mut calls = 0;
        let actual = block_on(check_with(&request, TEST_SOLVER_VERSION, &mut || {
            calls += 1;
            native_z3()
        }));
        let expected = cases::expected_check(&row, &fixture, &fixture.child, expected_envelope());
        observations.eq(
            &format!("{}/worker-check", row.id),
            (&actual, calls),
            (&expected, 0),
        );
    }
    let row = cases::row("E01/P");
    let fixture = cases::fixture(&row);
    let mut request: Request = serde_json::from_value(serde_json::json!({"cmd":"check","source":fixture.root,"source_file":"root_compose.fsl","files":fixture.files,"unstable":["fsl5"]})).unwrap();
    request
        .files
        .insert("unused.fsl".into(), cases::next_leaf());
    let actual = block_on(check_with(&request, TEST_SOLVER_VERSION, &mut native_z3));
    let expected = cases::expected_check(&row, &fixture, &fixture.child, expected_envelope());
    observations.eq("C01/unused", &actual, &expected);
    request.source = cases::next_leaf();
    let actual = block_on(check_with(&request, TEST_SOLVER_VERSION, &mut native_z3));
    let expected = cases::refusal(&request.source, "root_compose.fsl", expected_envelope());
    observations.eq("C01/root-refusal", &actual, &expected);
    request.options.deadlock = "invalid-option".into();
    let mut calls = 0;
    let actual = block_on(crate::verify_with(
        &request,
        TEST_SOLVER_VERSION,
        &mut || {
            calls += 1;
            native_z3()
        },
    ));
    let mut expected = serde_json::Value::Object(expected_envelope());
    expected["result"] = serde_json::json!("error");
    expected["kind"] = serde_json::json!("usage");
    expected["message"] = serde_json::json!("--deadlock must be warn, error, or ignore");
    observations.eq("C01/option-priority", (&actual, calls), (&expected, 0));
    observations.finish();
}

#[test]
fn dependency_edition_worker_adapter_executes_all_assigned_rows() {
    cases::exercise(
        "worker",
        &[dependency_edition_worker_check_uses_child_edition_and_ignores_unused_files],
    );
}
