// SPDX-License-Identifier: Apache-2.0

#[path = "../../../fslc/tests/support/edition_cases.rs"]
mod cases;

use super::{block_on, native_z3, TEST_SOLVER_VERSION};
use crate::{check_with, verify_with, Request};
use serde_json::{json, Value};

fn request(cmd: &str, source: &str, gate: bool) -> Request {
    serde_json::from_value(json!({
        "cmd": cmd, "source": source, "source_file": "source.fsl",
        "files": {"abstract.fsl": cases::ABSTRACT},
        // Verify's main BMC uses the browser bridge, not the requirements seam.
        // This accepted-options control stops there; actual native solver reach
        // is independently exercised by the deferred-requirements case below.
        "options": {"depth": 2, "deadlock": "edition-priority-probe"},
        "unstable": if gate { vec!["fsl5"] } else { vec![] }
    })).expect("same Request JSON shape as run")
}

fn run(request: &Request, calls: &mut usize) -> Value {
    let mut solver = || { *calls += 1; native_z3() };
    match request.cmd.as_str() {
        "check" => block_on(check_with(request, TEST_SOLVER_VERSION, &mut solver)),
        "verify" => block_on(verify_with(request, TEST_SOLVER_VERSION, &mut solver)),
        _ => panic!("unassigned Worker row"),
    }
}

#[test]
fn edition_adapter_executes_both_worker_rows_and_rejects_before_requirements_solver() {
    let mut results = cases::Results::default();
    let mut executed = Vec::new();
    for row in cases::rows().into_iter().filter(|row| row.owner == "worker") {
        executed.push(row.key.to_owned());
        for &(code, _, body) in cases::WARNING_CASES {
            let body = cases::legacy(code, body);
            // A real model error avoids browser BMC on native but must lose to
            // the source-edition refusal. No model/solver shortcut is asserted
            // to exercise browser JSON dispatch (owned by test-browser.mjs).
            let body = if code == "deprecated_domain_enum_union" { body } else { body.replace("invariant Typed {", "invariant Typed { missing_name and") };
            let source = format!("fsl 5\n{body}");
            let req = request(row.argv, &source, true);
            let expected_findings = cases::findings(&source, "source.fsl", fsl_syntax::Edition::Fsl5);
            assert!(expected_findings.iter().any(|finding| finding["code"] == code));
            let mut expected = crate::envelope(TEST_SOLVER_VERSION);
            expected.extend(serde_json::from_value::<serde_json::Map<String, Value>>(json!({
                "result":"error", "kind": if code == "deprecated_domain_enum_union" { "deprecated_domain_enum_union" } else { "unsupported_in_edition" },
                "edition":"next", "fsl_edition":5, "findings": expected_findings
            })).unwrap());
            let output = run(&req, &mut 0);
            results.check(&format!("{}/refuse/{code}", row.key), &output, &expected, output == Value::Object(expected.clone()));
            for header in ["", "fsl 4\n"] {
                let current = format!("{header}{body}");
                let output = run(&request(row.argv, &current, true), &mut 0);
                let expected = cases::findings(&current, "source.fsl", fsl_syntax::Edition::Fsl4);
                results.check(&format!("preserved/{}/current/{code}/{header:?}", row.key), &output["warnings"], &expected, output["warnings"] == json!(expected));
            }
            let output = run(&request(row.argv, &source, false), &mut 0);
            results.check(&format!("preserved/{}/gate-off/{code}", row.key), &output, "header rejected by ungated parser", output["kind"] == "parse" && output["diagnostic_code"] == "FSL-DIALECT-UNKNOWN" && output["findings"].is_null());
        }
        let source = format!("fsl 5\n{}", cases::DOMAIN);
        let output = run(&request(row.argv, &source, true), &mut 0);
        let expected = if row.argv == "check" { "ok" } else { "error" };
        results.check(&format!("preserved/{}/canonical", row.key), &output, expected, output["result"] == expected && output["fsl_edition"] == 5 && output["findings"].is_null());
        // Existing large-domain fixture is canonical fsl 5; the real native
        // requirements solver rejects it before browser BMC. Check it twice.
        let canonical = include_str!("../../../fslc/tests/fixtures/fsl5_solver/large_domain.fsl");
        for repeat in 0..2 {
            let mut req = request(row.argv, canonical, true);
            req.options.deadlock = "warn".to_owned();
            let mut calls = 0;
            let output = run(&req, &mut calls);
            results.check(&format!("preserved/{}/native-solver/{repeat}", row.key), (&output, calls), "decide_not_exhaustive and solver called", output["diagnostic_code"] == "decide_not_exhaustive" && calls > 0);
            // Accepted metadata legacy form + deferred solver rejection: next
            // must reject it BEFORE asking the injected native solver.
            let insertion = req.source.find('{').expect("requirements body") + 1;
            req.source.insert_str(insertion, "\n  invariant EditionProbe \"REQ-PROBE-001: legacy\" { true }\n");
            calls = 0;
            let output = run(&req, &mut calls);
            results.check(&format!("{}/solver-not-consumed/{repeat}", row.key), (&output, calls), "unsupported_in_edition and zero solver calls", output["kind"] == "unsupported_in_edition" && calls == 0);
        }
    }
    cases::assignment("worker", &executed);
    results.finish();
}
