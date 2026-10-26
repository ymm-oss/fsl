// SPDX-License-Identifier: Apache-2.0

//! #1337: compare each failed testgen CLI envelope with verify of the same
//! document, not with the structurally different scenarios envelope.

#[path = "support/report_cli.rs"]
mod cli;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

const BROKEN: &str = "tests/fixtures/chain/requirements_broken_implements.fsl";
const GOVERNANCE: &str = "rust/fslc/tests/fixtures/issue_1303/governance.fsl";
const MISSING: &str = "rust/fslc/tests/fixtures/governance_missing_dependency.fsl";
const FALLBACK: &str = "rust/fslc/tests/fixtures/inline_implements_fail_closed/impl_iv.fsl";
const PARSE_ERROR: &str =
    "rust/fslc/tests/fixtures/inline_implements_preserve_progress/req_inline_progress.fsl";
static NEXT_OUTPUT: AtomicUsize = AtomicUsize::new(0);

fn output_path() -> PathBuf {
    let serial = NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_nanos();
    let dir = cli::repository_root().join(format!(
        "rust/target/report-acceptance/{}-{timestamp}-{serial}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create worktree-local output directory");
    let output = dir.join("harness.py");
    assert!(
        !output.exists(),
        "output must start absent: {}",
        output.display()
    );
    output
}

/// These are the four ambient leaf kinds observed twice in Sigma. In particular
/// no whole cost branch, counter, location, verdict, or assurance is discarded.
fn remove_metric(object: &mut Map<String, Value>, key: &str, positive: bool) {
    let value = object.remove(key).expect("ambient exclusion must be live");
    assert!(
        value.as_f64().is_some_and(|number| {
            number.is_finite() && number >= 0.0 && (!positive || number > 0.0)
        }),
        "{key} must be a finite {} number: {value}",
        if positive { "positive" } else { "nonnegative" }
    );
}

fn normalize_cost(envelope: &mut Value) {
    let cost = envelope["cost"]
        .as_object_mut()
        .expect("failure cost object");
    // Wall time and solver-check time varied between the two observations.
    remove_metric(cost, "elapsed_s", false);
    let solver = cost["solver"].as_object_mut().expect("solver cost object");
    remove_metric(solver, "check_elapsed_s", false);
    // Allocator high-water differed even for the same document: 18.1 vs 17.47.
    remove_metric(solver, "memory_mb", true);
    let properties = cost["properties"].as_array_mut().expect("property costs");
    assert!(
        !properties.is_empty(),
        "property timing exclusions are live"
    );
    for property in properties {
        // Each observed property timing varied; retain kind/name/checks.
        remove_metric(
            property.as_object_mut().expect("property cost"),
            "elapsed_s",
            false,
        );
    }
}

fn assert_envelopes_equal(mut generated: Value, mut verified: Value, has_cost: bool) {
    // main.rs:1403 calls with_version_metadata (owner at :17195), whereas
    // testgen's run_verify_from_source result has no CLI wrapper metadata.
    // Assert both sides of that observed asymmetry, so the exclusion cannot
    // become dead if versions disappears from verify as well.
    assert!(generated.get("versions").is_none(), "{generated:#}");
    let versions = verified
        .as_object_mut()
        .expect("verify envelope")
        .remove("versions")
        .expect("verify-only versions exclusion must be live");
    assert_eq!(
        versions,
        json!({
            "core": {"name": "fsl-core", "version": env!("CARGO_PKG_VERSION")},
            "verifier": {"name": "fslc-rust", "version": env!("CARGO_PKG_VERSION")},
            "solver": {"name": "z3", "backend": "native-z3", "version": fsl_solver_z3::version()},
        })
    );
    if has_cost {
        normalize_cost(&mut generated);
        normalize_cost(&mut verified);
    } else {
        assert!(generated.get("cost").is_none(), "{generated:#}");
        assert!(verified.get("cost").is_none(), "{verified:#}");
    }
    assert_eq!(
        generated, verified,
        "compare the complete remaining envelopes"
    );
}

fn failure_pair(fixture: &str, expected_status: i32) -> (Value, Value) {
    let output = output_path();
    let (generated, status) = cli::run_json(
        "testgen",
        fixture,
        3,
        &["-o", output.to_str().expect("UTF-8 output path")],
    );
    let (verified, verify_status) = cli::run_json("verify", fixture, 3, &[]);
    assert_eq!(verify_status, expected_status, "verify: {verified:#}");
    assert_eq!(
        status,
        verify_status,
        "{fixture}: produced testgen exit {status}, result={}, harness_exists={}; expected exit {verify_status}, no harness",
        generated["result"],
        output.exists()
    );
    assert!(
        !output.exists(),
        "failed testgen wrote a harness: {}",
        output.display()
    );
    (generated, verified)
}

fn assert_failure(fixture: &str, expected_status: i32) {
    let (generated, verified) = failure_pair(fixture, expected_status);
    assert_envelopes_equal(generated, verified, expected_status == 1);
}

#[test]
fn kernel_seam_failure_matches_verify_and_writes_no_harness() {
    assert_failure(BROKEN, 1);
}

#[test]
fn kernel_preservation_failure_matches_verify_and_writes_no_harness() {
    assert_failure(GOVERNANCE, 1);
}

#[test]
fn preservation_contract_error_matches_verify_and_writes_no_harness() {
    assert_failure(MISSING, 2);
}

#[test]
fn fallback_primary_failure_preserves_the_complete_verify_envelope() {
    assert_failure(FALLBACK, 1);
}

#[test]
fn seam_parse_error_preserves_the_complete_verify_envelope() {
    assert_failure(PARSE_ERROR, 2);
}

fn assert_generates(fixture: &str) {
    let output = output_path();
    let (generated, status) = cli::run_json(
        "testgen",
        fixture,
        3,
        &["-o", output.to_str().expect("UTF-8 output path")],
    );
    assert_eq!(status, 0, "{generated:#}");
    assert_eq!(generated["result"], "generated", "{generated:#}");
    assert_eq!(generated["target"], "pytest", "{generated:#}");
    assert_eq!(generated["output"], output.to_str().expect("UTF-8 path"));
    let harness = std::fs::read_to_string(&output).expect("passing testgen writes a harness");
    assert!(harness.contains("def test_"), "{harness}");
}

#[test]
fn passing_seam_still_generates_a_harness() {
    assert_generates("tests/fixtures/chain/requirements.fsl");
}

#[test]
fn passing_preservation_still_generates_a_harness() {
    assert_generates("rust/fslc/tests/fixtures/issue_1303/governance_refines.fsl");
}

#[test]
fn exclusions_reject_a_leaf_missing_on_both_sides() {
    let (generated, verified) = failure_pair(FALLBACK, 1);
    assert_envelopes_equal(generated.clone(), verified.clone(), true);
    let mut paths = vec![
        "/cost/elapsed_s".to_owned(),
        "/cost/solver/check_elapsed_s".to_owned(),
        "/cost/solver/memory_mb".to_owned(),
        "/versions".to_owned(),
    ];
    for index in 0..verified["cost"]["properties"]
        .as_array()
        .expect("properties")
        .len()
    {
        paths.push(format!("/cost/properties/{index}/elapsed_s"));
    }
    for path in paths {
        let mut generated = generated.clone();
        let mut verified = verified.clone();
        let (parent, key) = path.rsplit_once('/').expect("leaf pointer");
        for envelope in [&mut generated, &mut verified] {
            envelope
                .pointer_mut(parent)
                .expect("parent exists")
                .as_object_mut()
                .expect("parent object")
                .remove(key);
        }
        let failure =
            std::panic::catch_unwind(|| assert_envelopes_equal(generated, verified, true))
                .expect_err("dead exclusion unexpectedly passed");
        assert!(
            failure
                .downcast_ref::<String>()
                .is_some_and(|message| message.contains("exclusion must be live")),
            "missing {path} must fail for the live-exclusion check"
        );
    }
}
