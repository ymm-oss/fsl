// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita
//! `--as-of` on `causal analyze` and `causal ledger` must name a real
//! Gregorian date as ASCII `YYYY-MM-DD` (#1422). A malformed value used to
//! exit 0 and silently count nothing as stale.
#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use serde_json::{Value, json};
use std::path::Path;

const ROUTES: [&[&str]; 3] = [
    &["analyze", "--projection", "causal_evidence_graph"],
    &["analyze", "--profile", "causal-review"],
    &["ledger"],
];
const MODEL: &str = "examples/causal/subscription_retention.fsl";
const EVIDENCE: &str = "examples/causal/evidence/onboarding-2026.causal.json";
const LIFECYCLE: &str = "examples/causal/evidence/onboarding-2026.lifecycle.json";

fn run(route: &[&str], evidence: &str, as_of: &str) -> (Value, i32, Vec<u8>) {
    let output = fslc_cli::command()
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args(["causal", route[0], MODEL])
        .args(&route[1..])
        .args([
            "--evidence",
            evidence,
            "--lifecycle",
            LIFECYCLE,
            "--as-of",
            as_of,
        ])
        .output()
        .unwrap();
    (
        serde_json::from_slice(&output.stdout).unwrap(),
        output.status.code().unwrap(),
        output.stderr,
    )
}

fn refusal(as_of: &str) -> (Value, i32, Vec<u8>) {
    (
        json!({"fsl": "1.0", "result": "error", "kind": "usage", "message": format!(
            "--as-of requires a valid Gregorian date in YYYY-MM-DD format, got '{as_of}'"
        )}),
        2,
        vec![],
    )
}

fn stale_findings(output: &Value) -> usize {
    output.to_string().matches("\"stale_evidence\"").count()
}

#[test]
fn malformed_as_of_is_a_usage_refusal_on_every_route() {
    for as_of in [
        "2099-02-30",
        "2099-13-01",
        "garbage",
        "",
        "2099-+4-01",
        "+099-04-01",
        "-099-04-01",
        "2099-04-+1",
        "2099-4-01",
        "2099-04-01T00:00:00Z",
        "２０９９-04-01",
    ] {
        for route in ROUTES {
            assert_eq!(
                run(route, EVIDENCE, as_of),
                refusal(as_of),
                "{as_of} {route:?}"
            );
        }
    }
}

#[test]
fn as_of_is_refused_before_evidence_is_read() {
    for route in ROUTES {
        assert_eq!(
            run(
                route,
                "examples/causal/evidence/missing.causal.json",
                "garbage"
            ),
            refusal("garbage"),
            "{route:?}"
        );
    }
}

#[test]
fn valid_until_boundary_is_kept_for_well_formed_dates() {
    for route in ROUTES {
        let (kept, code, stderr) = run(route, EVIDENCE, "2027-03-31");
        assert_eq!((code, stderr.as_slice()), (0, &b""[..]), "{kept}");
        assert_eq!(stale_findings(&kept), 0, "{route:?} {kept}");
        let (stale, code, stderr) = run(route, EVIDENCE, "2027-04-01");
        assert_eq!((code, stderr.as_slice()), (0, &b""[..]), "{stale}");
        assert!(stale_findings(&stale) > 0, "{route:?} {stale}");
    }
}

#[test]
fn refusal_comparator_rejects_success_other_kind_and_other_value() {
    let expected = refusal("garbage");
    assert_ne!(refusal("garbag"), expected);
    let mut wrong = expected.clone();
    wrong.0["kind"] = json!("semantics");
    assert_ne!(wrong, expected);
    assert_ne!((expected.0.clone(), 0, vec![]), expected);
    assert_ne!((expected.0.clone(), 2, b"warning".to_vec()), expected);
}
