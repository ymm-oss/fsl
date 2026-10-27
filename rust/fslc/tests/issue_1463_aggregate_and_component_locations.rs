// SPDX-License-Identifier: Apache-2.0

#[path = "support/issue_1463_aggregate_and_component_locations.rs"]
#[allow(dead_code)]
mod forms;

#[path = "support/issue_1463_nested_aggregates_and_sync_ownership.rs"]
mod nested_forms;

use serde_json::Value;
use std::path::Path;
use std::process::Command;

#[test]
fn aggregate_source_types_and_component_locations() {
    let cases = forms::cases();
    assert_eq!(cases.len(), 75);
    run_cases(&cases, "aggregate-and-component-locations");
}

#[test]
fn nested_aggregates_and_sync_ownership() {
    let cases = nested_forms::cases();
    assert_eq!(cases.len(), 87);
    run_cases(&cases, "nested-aggregates-and-sync-ownership");
}

fn run_cases(cases: &[forms::Case], label: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1463-{label}-{}",
        std::process::id()
    ));
    for (index, case) in cases.iter().enumerate() {
        let path = case.materialize(&root, index);
        let mut rejection = None;
        let routes: &[&str] = if case.id.starts_with("nested/") {
            // Preserve explicit execution, not the existing symbolic limitation (#1480).
            &["check", "explicit"]
        } else {
            &["check", "bmc", "induction", "explicit"]
        };
        for &route in routes {
            let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
            command.env("FSLC_CACHE", "off").env_remove("FSLC_UNSTABLE");
            if route == "check" {
                command.args(["check"]).arg(&path);
            } else {
                command.arg("verify").arg(&path).args([
                    "--engine",
                    route,
                    "--depth",
                    "4",
                    "--no-cache",
                ]);
            }
            let output = command.output().expect("CLI");
            assert!(
                output.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
            if case.rejection {
                assert_eq!(
                    output.status.code(),
                    Some(2),
                    "{} {route}: {report}",
                    case.id
                );
                assert_eq!(report["result"], "error");
                assert_eq!(report["kind"], "semantics");
                let (line, column) = case.location();
                assert_eq!(
                    report["loc"],
                    serde_json::json!({"line":line,"column":column}),
                    "{} {route}: {report}",
                    case.id
                );
                nested_forms::assert_message(case, report["message"].as_str().expect("message"));
                if let Some(expected) = &rejection {
                    assert_eq!(&report, expected, "{}: full CLI envelope", case.id);
                } else {
                    rejection = Some(report);
                }
            } else if route == "check" {
                assert_eq!(output.status.code(), Some(0), "{}: {report}", case.id);
                assert_eq!(report["result"], "ok");
            } else if case.id.starts_with("nested/set/") {
                // Measured on 6e99702c and head, all 24 exact controls x2:
                // check accepts, but this equality family has an existing engine limit.
                assert_eq!(output.status.code(), Some(2), "{}: {report}", case.id);
                assert_eq!(report["result"], "error");
                assert_eq!(report["kind"], "semantics");
                assert_eq!(report["message"], "'x' is not an integer const");
                assert!(report["loc"].is_null());
            } else {
                assert_eq!(
                    output.status.code(),
                    Some(i32::from(case.negative)),
                    "{} {route}: {report}",
                    case.id
                );
                assert_eq!(
                    report["result"],
                    if case.negative {
                        "violated"
                    } else if route == "bmc" {
                        "verified"
                    } else {
                        "proved"
                    },
                    "{} {route}: {report}",
                    case.id
                );
            }
        }
    }
}

#[test]
fn component_diagnostic_context_preserves_existing_kernel_export_boundary() {
    let case = forms::Case { id: "valid-component".into(), source: "compose Root { use Safe as part from \"safe.fsl\" }".into(), files: vec![("safe.fsl".into(), "spec Safe { state { done: Bool } init { done = false } action go() { done = true } invariant Capture { done or not done } }".into())], rejection: false, negative: false, public_name: None, marker: "use Safe" };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1463-component-kernel-export-{}",
        std::process::id()
    ));
    let path = case.materialize(&root, 0);
    for version in ["1", "2"] {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .env("FSLC_CACHE", "off")
            .args(["kernel"])
            .arg(&path)
            .args(["--kernel-version", version])
            .output()
            .expect("kernel");
        // #1482 already rejects compose projection for both public majors.
        assert_eq!(output.status.code(), Some(2));
        let report: Value = serde_json::from_slice(&output.stdout).expect("kernel error");
        assert_eq!(report["result"], "error");
        assert!(
            report["message"]
                .as_str()
                .unwrap()
                .contains("cannot preserve component source filenames")
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("component-check:"));
    }
}
