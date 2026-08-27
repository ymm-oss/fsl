// SPDX-License-Identifier: Apache-2.0

//! Native CLI ownership for the business, requirements, and governance
//! induction cases previously compared only by
//! `tools/check_rust_dialect_parity.py` (issue #761 F2).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

struct Case {
    path: &'static str,
    spec: &'static str,
    invariants: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        path: "examples/e2e/1_business.fsl",
        spec: "ExpenseToBe",
        invariants: &["_bounds_claim_stage"],
    },
    Case {
        path: "examples/e2e/2_requirements.fsl",
        spec: "ExpenseRequirements",
        invariants: &["_bounds_claim_amount", "_bounds_claim_stage"],
    },
    Case {
        path: "examples/consulting/governance_controls.fsl",
        spec: "ExpenseTransformationControls",
        invariants: &["_governance_catalog_ok"],
    },
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_induction(path: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            path,
            "--depth",
            "8",
            "--engine",
            "induction",
            "--k",
            "1",
            "--deadlock",
            "warn",
        ])
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for induction case {path}: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn string_set(value: &Value) -> BTreeSet<&str> {
    value
        .as_array()
        .expect("expected JSON array")
        .iter()
        .map(|item| item.as_str().expect("expected string array item"))
        .collect()
}

#[test]
fn business_requirements_and_governance_pin_native_induction_contracts() {
    for case in CASES {
        let (output, exit) = run_induction(case.path);
        assert_eq!(
            exit, 0,
            "{}: produced exit={exit}, expected=0; {output:#}",
            case.path
        );
        assert_eq!(output["result"], "proved", "{}: {output:#}", case.path);
        assert_eq!(output["spec"], case.spec, "{}: {output:#}", case.path);
        assert_eq!(output["engine"], "induction", "{}: {output:#}", case.path);
        assert_eq!(
            output["completeness"], "unbounded",
            "{}: {output:#}",
            case.path
        );
        assert_eq!(output["checked_to_depth"], 8, "{}: {output:#}", case.path);
        assert_eq!(output["base_depth"], 8, "{}: {output:#}", case.path);
        assert_eq!(
            string_set(&output["invariants_checked"]),
            case.invariants.iter().copied().collect(),
            "{}: {output:#}",
            case.path
        );

        let k_used = output["k_used"]
            .as_object()
            .unwrap_or_else(|| panic!("{}: expected k_used object; {output:#}", case.path));
        assert_eq!(
            k_used.len(),
            case.invariants.len(),
            "{}: {output:#}",
            case.path
        );
        for invariant in case.invariants {
            assert_eq!(
                k_used.get(*invariant),
                Some(&Value::from(1)),
                "{}: invariant={invariant}; {output:#}",
                case.path
            );
        }

        match case.path {
            "examples/e2e/1_business.fsl" => {
                assert_eq!(
                    output["leads_to"]["CTRL-1"]["checked_to_depth"], 8,
                    "{output:#}"
                );
                assert_eq!(
                    output["leads_to"]["CTRL-2"]["checked_to_depth"], 8,
                    "{output:#}"
                );
                assert_eq!(
                    output["reachables"]["CanPay"]["witnessed_at_step"], 3,
                    "{output:#}"
                );
            }
            "examples/e2e/2_requirements.fsl" => {
                assert_eq!(output["implements"]["abs"], "ExpenseToBe", "{output:#}");
                assert_eq!(output["implements"]["result"], "refines", "{output:#}");
            }
            "examples/consulting/governance_controls.fsl" => {
                assert_eq!(
                    output["action_coverage"]["_governance_noop"]["covered"], false,
                    "{output:#}"
                );
                assert_eq!(
                    output["action_coverage"]["_governance_noop"]["requirement"]["id"], "GOV",
                    "{output:#}"
                );
            }
            _ => unreachable!("unregistered dialect induction case"),
        }
    }
}
