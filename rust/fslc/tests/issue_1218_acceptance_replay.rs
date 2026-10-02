// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native contract for #1218: `verify --instances` / `--values` (and every
//! `sweep` cell, which always runs with overrides) still replays the
//! requirements `acceptance`/`forbidden` scenarios.
//!
//! `docs/manual/LANGUAGE.md` specifies a per-scenario skip: under overrides, a
//! scenario whose replay fails *purely* because it references a value outside
//! the overridden bounds is skipped with an `acceptance_skipped` /
//! `forbidden_skipped` warning; every other failure stays a hard error. The
//! frozen Python reference implements it (`tests/test_acceptance_override_skip.py`,
//! ported one-for-one below). Before #1218 the native CLI replayed nothing
//! under overrides, so a false acceptance read as `verified` / exit 0 even
//! when the override equalled the declared range.
//!
//! Every detector names the mutation it kills; the main one is restoring the
//! pre-#1218 skip of the whole replay under overrides (`origin/main`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/issue_1218_acceptance_replay";
/// Shared with #1008: a sound seam and a false, in-range acceptance.
const FAILING_ACCEPTANCE: &str =
    "rust/fslc/tests/fixtures/issue_1008_not_evaluated/impl_failing_acceptance.fsl";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn fixture(name: &str) -> String {
    format!("{FIXTURE_DIR}/{name}")
}

fn run_with_env(arguments: &[&str], env: &[(&str, &Path)]) -> (Value, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.args(arguments).current_dir(repository_root());
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn verify(name: &str, depth: &str, extra: &[&str]) -> (Value, i32) {
    let path = if name == FAILING_ACCEPTANCE {
        name.to_owned()
    } else {
        fixture(name)
    };
    let mut arguments = vec!["verify", path.as_str(), "--depth", depth, "--no-cache"];
    arguments.extend_from_slice(extra);
    run_with_env(&arguments, &[])
}

fn skip_warnings<'a>(output: &'a Value, kind: &str) -> Vec<&'a Value> {
    output["warnings"]
        .as_array()
        .map(|warnings| {
            warnings
                .iter()
                .filter(|warning| warning["kind"] == kind)
                .collect()
        })
        .unwrap_or_default()
}

fn assert_acceptance_failure(output: &Value, status: i32, id: &str) {
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "acceptance", "{output:#}");
    assert_eq!(output["trace_type"], "acceptance", "{output:#}");
    assert_eq!(output["id"], id, "{output:#}");
}

/// Preservation control: without overrides the false acceptance is rejected.
#[test]
fn unscoped_verify_rejects_the_failing_acceptance() {
    let (output, status) = verify(FAILING_ACCEPTANCE, "3", &[]);
    assert_acceptance_failure(&output, status, "AC-1");
}

/// detector (mutation: skip the replay under `--values`). The override equals
/// the declared range, so nothing about the world changed; the verdict must
/// not either.
#[test]
fn values_override_equal_to_the_declared_range_rejects_the_failing_acceptance() {
    let (output, status) = verify(FAILING_ACCEPTANCE, "3", &["--values", "Limit=0..3"]);
    assert_acceptance_failure(&output, status, "AC-1");
}

/// detector (mutation: skip the replay under a shrinking `--values`). `expect
/// n == 2` references no index, so it is in range for every scope and stays a
/// hard failure.
#[test]
fn shrinking_values_override_rejects_the_in_range_failing_acceptance() {
    let (output, status) = verify(FAILING_ACCEPTANCE, "3", &["--values", "Limit=0..1"]);
    assert_acceptance_failure(&output, status, "AC-1");
}

/// detector (mutation: skip every scenario after the first skipped one, or
/// skip a scenario that is in range). AC-1 is out of range and skipped; AC-3
/// is in range and false, and must still fail the run.
#[test]
fn in_range_scenarios_are_still_replayed_after_a_skip() {
    let (output, status) = verify("mixed_failing.fsl", "3", &["--instances", "Case=1"]);
    assert_acceptance_failure(&output, status, "AC-3");
}

/// Rejecting control for the detector above: the same shape without AC-3 is
/// verified, with exactly one skip naming AC-1 and its out-of-range reference.
#[test]
fn out_of_range_scenario_is_skipped_with_a_warning_and_the_rest_pass() {
    let (output, status) = verify("mixed.fsl", "3", &["--instances", "Case=1"]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    let skipped = skip_warnings(&output, "acceptance_skipped");
    assert_eq!(skipped.len(), 1, "{output:#}");
    assert_eq!(skipped[0]["id"], "AC-1", "{output:#}");
    assert_eq!(
        skipped[0]["reference"], "accept(1): argument 1 for 'c' is outside its domain",
        "{output:#}"
    );
    let message = skipped[0]["message"].as_str().expect("message");
    assert!(message.contains("AC-1"), "{message}");
    assert!(message.contains("Case=1"), "{message}");
    assert!(message.contains("accept(1)"), "{message}");
    assert!(
        skip_warnings(&output, "forbidden_skipped").is_empty(),
        "{output:#}"
    );
    // Without overrides nothing is skipped.
    let (unscoped, unscoped_status) = verify("mixed.fsl", "3", &[]);
    assert_eq!(unscoped_status, 0, "{unscoped:#}");
    assert!(
        skip_warnings(&unscoped, "acceptance_skipped").is_empty(),
        "{unscoped:#}"
    );
}

fn sweep(arguments: &[&str]) -> (Value, i32) {
    let mut full = vec!["sweep"];
    full.extend_from_slice(arguments);
    let cache = std::env::temp_dir().join(format!(
        "fsl-issue-1218-sweep-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    std::fs::create_dir(&cache).expect("sweep cache dir must not already exist");
    let result = run_with_env(&full, &[("FSLC_CACHE_DIR", cache.as_path())]);
    let _ = std::fs::remove_dir_all(&cache);
    result
}

/// detector (mutation: sweep cells skip the replay). Every cell runs with
/// overrides; a spec whose acceptance fails must not be `sweep_passed`.
#[test]
fn sweep_with_a_failing_acceptance_does_not_pass() {
    let (output, status) = sweep(&[
        FAILING_ACCEPTANCE,
        "--values",
        "Limit=0..3",
        "--depth",
        "1..2",
    ]);
    assert_ne!(output["result"], "sweep_passed", "{output:#}");
    assert_acceptance_failure(&output, status, "AC-1");
}

/// Rejecting control for the sweep detector: the sweep cells of `mixed.fsl`
/// pass, and the cell that shrinks `Case` below AC-1's id carries the skip.
#[test]
fn sweep_cells_skip_only_out_of_range_scenarios() {
    let path = fixture("mixed.fsl");
    let (output, status) = sweep(&[&path, "--instances", "Case=1..2", "--depth", "2..2"]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "sweep_passed", "{output:#}");
    let cells = output["sweep"]["results"].as_array().expect("cells");
    let skipped_by_cell = cells
        .iter()
        .map(|cell| {
            (
                cell["scope"]["instances"]["Case"].as_i64(),
                skip_warnings(&cell["verification"], "acceptance_skipped").len(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        skipped_by_cell,
        vec![(Some(1), 1), (Some(2), 0)],
        "{output:#}"
    );
}

// --- Port of tests/test_acceptance_override_skip.py (6 cases) ---

/// `test_liveness_with_instances_override_skips_out_of_fit_acceptance`
#[test]
fn liveness_with_instances_override_skips_out_of_fit_acceptance() {
    let (output, status) = verify(
        "repro.fsl",
        "6",
        &[
            "--instances",
            "Case=1",
            "--property",
            "EveryAcceptedGetsResponse",
        ],
    );
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    let skipped = skip_warnings(&output, "acceptance_skipped");
    assert_eq!(skipped.len(), 1, "{output:#}");
    assert_eq!(skipped[0]["id"], "AC-1", "{output:#}");
    let message = skipped[0]["message"].as_str().expect("message");
    assert!(message.contains("AC-1"), "{message}");
    assert!(message.contains("skipped"), "{message}");
}

/// `test_in_range_acceptance_with_false_expect_still_hard_errors_under_overrides`
#[test]
fn in_range_acceptance_with_false_expect_still_hard_errors_under_overrides() {
    let (output, status) = verify("genuine_failure.fsl", "6", &["--instances", "Case=1"]);
    assert_acceptance_failure(&output, status, "AC-2");
}

/// `test_expect_only_out_of_range_is_skipped_under_overrides`
#[test]
fn expect_only_out_of_range_is_skipped_under_overrides() {
    let (output, status) = verify("expect_out_of_range.fsl", "6", &["--instances", "Case=1"]);
    assert_eq!(status, 0, "{output:#}");
    assert_ne!(output["result"], "error", "{output:#}");
    let skipped = skip_warnings(&output, "acceptance_skipped");
    assert_eq!(skipped.len(), 1, "{output:#}");
    assert_eq!(skipped[0]["id"], "AC-1", "{output:#}");
    assert_eq!(
        skipped[0]["reference"], "expect indexes cases[1] outside its key domain",
        "{output:#}"
    );
}

/// `test_values_override_out_of_range_number_arg_is_skipped`
#[test]
fn values_override_out_of_range_number_arg_is_skipped() {
    let (output, status) = verify("number_override.fsl", "4", &["--values", "Amount=0..1"]);
    assert_eq!(status, 0, "{output:#}");
    assert_ne!(output["result"], "error", "{output:#}");
    let skipped = skip_warnings(&output, "acceptance_skipped");
    assert_eq!(skipped.len(), 1, "{output:#}");
    assert_eq!(skipped[0]["id"], "AC-1", "{output:#}");
    let message = skipped[0]["message"].as_str().expect("message");
    assert!(message.contains("Amount=0..1"), "{message}");
}

/// `test_no_overrides_out_of_range_acceptance_still_hard_errors`
#[test]
fn no_overrides_out_of_range_acceptance_still_hard_errors() {
    let (output, status) = verify("no_override_out_of_range.fsl", "4", &[]);
    assert_acceptance_failure(&output, status, "AC-1");
    assert!(output.get("bounds_overrides").is_none(), "{output:#}");
}

/// `test_forbidden_setup_out_of_range_is_skipped_under_overrides`
#[test]
fn forbidden_setup_out_of_range_is_skipped_under_overrides() {
    let (unscoped, unscoped_status) = verify("forbidden.fsl", "4", &[]);
    assert_eq!(unscoped_status, 0, "{unscoped:#}");
    assert_ne!(unscoped["result"], "error", "{unscoped:#}");
    assert!(
        skip_warnings(&unscoped, "forbidden_skipped").is_empty(),
        "{unscoped:#}"
    );

    let (output, status) = verify("forbidden.fsl", "4", &["--instances", "Case=1"]);
    assert_eq!(status, 0, "{output:#}");
    assert_ne!(output["result"], "error", "{output:#}");
    let skipped = skip_warnings(&output, "forbidden_skipped");
    assert_eq!(skipped.len(), 1, "{output:#}");
    assert_eq!(skipped[0]["id"], "FB-1", "{output:#}");
    let message = skipped[0]["message"].as_str().expect("message");
    assert!(message.contains("FB-1"), "{message}");
}

/// detector (mutation: a cache hit loses, or duplicates, the skip warning).
#[test]
fn a_cached_scoped_run_replays_the_skip_warning_once() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let cache = std::env::temp_dir().join(format!(
        "fsl-issue-1218-cache-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&cache).expect("cache dir must not already exist");
    let path = fixture("mixed.fsl");
    let arguments = [
        "verify",
        path.as_str(),
        "--depth",
        "3",
        "--instances",
        "Case=1",
    ];
    let env = [("FSLC_CACHE_DIR", cache.as_path())];
    let (fresh, fresh_status) = run_with_env(&arguments, &env);
    let (cached, cached_status) = run_with_env(&arguments, &env);
    let _ = std::fs::remove_dir_all(&cache);
    assert_eq!(
        (fresh_status, cached_status),
        (0, 0),
        "{fresh:#}\n{cached:#}"
    );
    assert_eq!(cached["cache"]["hit"], true, "{cached:#}");
    assert_eq!(
        skip_warnings(&fresh, "acceptance_skipped").len(),
        1,
        "{fresh:#}"
    );
    assert_eq!(
        skip_warnings(&cached, "acceptance_skipped"),
        skip_warnings(&fresh, "acceptance_skipped"),
        "{cached:#}"
    );
}

// --- Independent review of 7f1fdf86: a skip must be the override's doing ---

/// detector (mutation: exclude a forbidden scenario's final step from the
/// out-of-scope check). `respond` has no guard, so FB-1 is broken; under
/// `Case=1` its `respond(1)` is "rejected" only because id 1 no longer exists.
/// That tested no guard, so it is skipped and reported, never a silent pass.
#[test]
fn forbidden_final_step_removed_by_the_override_is_skipped_not_passed() {
    let (unscoped, unscoped_status) = verify("forbidden_final_unguarded.fsl", "3", &[]);
    assert_eq!(unscoped_status, 2, "{unscoped:#}");
    assert_eq!(unscoped["kind"], "forbidden", "{unscoped:#}");
    let (in_scope, in_scope_status) = verify(
        "forbidden_final_unguarded.fsl",
        "3",
        &["--instances", "Case=2"],
    );
    assert_eq!(in_scope_status, 2, "{in_scope:#}");
    assert_eq!(in_scope["kind"], "forbidden", "{in_scope:#}");
    assert_eq!(in_scope["trace_type"], "forbidden", "{in_scope:#}");

    let (output, status) = verify(
        "forbidden_final_unguarded.fsl",
        "3",
        &["--instances", "Case=1"],
    );
    assert_eq!(status, 0, "{output:#}");
    let skipped = skip_warnings(&output, "forbidden_skipped");
    assert_eq!(skipped.len(), 1, "{output:#}");
    assert_eq!(skipped[0]["id"], "FB-1", "{output:#}");
    assert_eq!(
        output["requirement_traces"]["skipped"],
        serde_json::json!([{
            "kind": "forbidden",
            "id": "FB-1",
            "reference": "respond(1): argument 1 for 'c' is outside its domain",
        }]),
        "{output:#}"
    );
    assert_eq!(
        output["requirement_traces"]["result"], "not_evaluated",
        "{output:#}"
    );
}

/// detector (mutation: judge "out of scope" against the overridden domain
/// only). `pay(7)` is outside the declared `Amount = 0..3` as well, so no
/// override removed it: a no-op override, an override on another name, and a
/// shrinking one all keep the unscoped hard error.
#[test]
fn argument_outside_the_declared_domain_is_not_excused() {
    let (unscoped, unscoped_status) = verify("argument_outside_declared.fsl", "3", &[]);
    assert_acceptance_failure(&unscoped, unscoped_status, "AC-1");
    for extra in [
        &["--values", "Amount=0..3"][..],
        &["--values", "Other=0..1"][..],
        &["--values", "Amount=0..1"][..],
    ] {
        let (output, status) = verify("argument_outside_declared.fsl", "3", extra);
        assert_acceptance_failure(&output, status, "AC-1");
        assert!(
            skip_warnings(&output, "acceptance_skipped").is_empty(),
            "{output:#}"
        );
    }
}

/// detector (mutation: same as above, for an `expect` index). `cases[7]` is
/// outside the declared `Case = 3`; the unscoped error stays under overrides.
#[test]
fn expect_index_outside_the_declared_domain_is_not_excused() {
    let (unscoped, unscoped_status) = verify("expect_outside_declared.fsl", "3", &[]);
    assert_eq!(unscoped_status, 2, "{unscoped:#}");
    for instances in ["Case=3", "Case=1"] {
        let (output, status) = verify(
            "expect_outside_declared.fsl",
            "3",
            &["--instances", instances],
        );
        assert_eq!(status, 2, "{output:#}");
        assert_eq!(output["result"], "error", "{output:#}");
        assert_eq!(output["kind"], unscoped["kind"], "{output:#}");
        assert_eq!(output["message"], unscoped["message"], "{output:#}");
    }
}

/// detector (mutation: excuse any `expect` error when the expression has an
/// out-of-range index somewhere). The error is a division by zero in every
/// world; `cases[2]` being out of range under `Case=2` did not cause it.
#[test]
fn expect_error_the_override_did_not_cause_is_not_excused() {
    let (unscoped, unscoped_status) = verify("expect_other_error.fsl", "3", &[]);
    assert_eq!(unscoped_status, 2, "{unscoped:#}");
    assert_eq!(unscoped["message"], "division by zero", "{unscoped:#}");
    let (output, status) = verify("expect_other_error.fsl", "3", &["--instances", "Case=2"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], unscoped["kind"], "{output:#}");
    assert_eq!(output["message"], "division by zero", "{output:#}");
}

/// detector (mutation: sweep cells excuse references outside the declared
/// domain). Every cell of a no-op-ish sweep keeps the unscoped error.
#[test]
fn sweep_does_not_pass_a_scenario_outside_the_declared_domain() {
    let path = fixture("argument_outside_declared.fsl");
    let (output, status) = sweep(&[&path, "--values", "Amount=0..3", "--depth", "1..1"]);
    assert_ne!(output["result"], "sweep_passed", "{output:#}");
    assert_acceptance_failure(&output, status, "AC-1");
}

/// The sweep grid names the scenarios its cells skipped (`sweep.not_evaluated
/// .skipped`, the union of the cells' `requirement_traces.skipped`).
#[test]
fn sweep_not_evaluated_lists_the_skipped_scenarios() {
    let path = fixture("mixed.fsl");
    let (output, status) = sweep(&[&path, "--instances", "Case=1..2", "--depth", "2..2"]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(
        output["sweep"]["not_evaluated"]["skipped"],
        serde_json::json!([{"kind": "acceptance", "id": "AC-1"}]),
        "{output:#}"
    );
}
