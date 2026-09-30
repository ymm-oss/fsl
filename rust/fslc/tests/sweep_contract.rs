// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #464: `sweep` must never fold a spec error into the
//! `sweep_passed`/exit-0 verdict. A one-character typo in `--instances`, a
//! parse error, or a missing file are not "no counterexample in this grid" —
//! they are documented exit-2 spec errors (`docs/manual/LANGUAGE.md` exit-code
//! table) that a caller gating on exit code or `result` must be able to
//! distinguish from a genuinely clean sweep.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli(arguments: &[&str]) -> (serde_json::Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn fixture(name: &str) -> String {
    format!("rust/fslc/tests/fixtures/{name}")
}

/// The masking case: a spec with a genuine counterexample, swept under a
/// mistyped `--instances` name. Before the fix this returned
/// `sweep_passed`/exit 0 (a one-character typo silently turned a red gate
/// green); the spec declares no entity/number bounds at all, so any
/// `--instances` name is a documented exit-2 error
/// (`docs/manual/LANGUAGE.md` §"NAME with no matching entity/number declaration").
#[test]
fn sweep_does_not_mask_a_typo_d_instances_name_as_sweep_passed() {
    let path = fixture("sweep_violating.fsl");

    let (baseline, baseline_status) = run_cli(&["sweep", &path, "--depth", "0..3"]);
    assert_eq!(baseline_status, 1, "baseline: {baseline:#}");
    assert_eq!(baseline["result"], "sweep_failed");
    assert!(!baseline["sweep"]["minimal_counterexample"].is_null());

    let (typo, typo_status) = run_cli(&[
        "sweep",
        &path,
        "--instances",
        "Amountt=1..2",
        "--depth",
        "0..3",
    ]);
    assert_eq!(typo_status, 2, "typo'd --instances must not pass: {typo:#}");
    assert_eq!(typo["result"], "error");
    assert_eq!(typo["kind"], "semantics");
    assert!(
        typo["message"]
            .as_str()
            .is_some_and(|message| message.contains("--instances/--values only apply")),
        "unexpected message: {typo:#}"
    );
    // The masking case is specifically that this must not become sweep_passed.
    assert_ne!(typo["result"], "sweep_passed");
}

/// A parse error must surface as-is (`kind`, `message`, `loc`,
/// `diagnostic_code` preserved) instead of being absorbed as a passing grid
/// cell. Reuses the committed gallery corpus fixture so the expected
/// diagnostic is pinned by an existing `expected-kind: parse` contract.
#[test]
fn sweep_propagates_a_parse_error_verbatim() {
    let path = "examples/gallery/errors/parse_missing_expression.fsl";
    let (check, _) = run_cli(&["check", path]);
    assert_eq!(check["kind"], "parse", "fixture drifted: {check:#}");

    let (swept, status) = run_cli(&["sweep", path, "--depth", "0..2"]);
    assert_eq!(status, 2, "swept: {swept:#}");
    assert_eq!(swept["result"], "error");
    assert_eq!(swept["kind"], check["kind"]);
    assert_eq!(swept["message"], check["message"]);
}

/// A missing spec file must surface as an exit-2 spec error, not
/// `sweep_passed`.
#[test]
fn sweep_propagates_a_missing_file_error_instead_of_sweep_passed() {
    let (swept, status) = run_cli(&["sweep", "specs/nope_does_not_exist.fsl", "--depth", "0..1"]);
    assert_eq!(status, 2, "swept: {swept:#}");
    assert_eq!(swept["result"], "error");
    assert_ne!(swept["result"], "sweep_passed");
}

/// Regression control: an ordinary clean sweep (no counterexample, no
/// argument error) must keep returning `sweep_passed`/exit 0, and a sweep
/// over a genuinely violating spec with correct arguments must keep
/// returning `sweep_failed`/exit 1 with `minimal_counterexample` populated.
/// This guards against over-triggering the #464 fix on non-error grid cells.
#[test]
fn sweep_still_passes_clean_specs_and_fails_genuine_counterexamples() {
    let clean = fixture("sweep_clean.fsl");
    let (passed, passed_status) = run_cli(&["sweep", &clean, "--depth", "0..3"]);
    assert_eq!(passed_status, 0, "clean: {passed:#}");
    assert_eq!(passed["result"], "sweep_passed");
    assert!(passed["sweep"]["minimal_counterexample"].is_null());

    let violating = fixture("sweep_violating.fsl");
    let (failed, failed_status) = run_cli(&["sweep", &violating, "--depth", "0..3"]);
    assert_eq!(failed_status, 1, "violating: {failed:#}");
    assert_eq!(failed["result"], "sweep_failed");
    assert!(!failed["sweep"]["minimal_counterexample"].is_null());
}

#[test]
fn sweep_cart_v1_passes_after_insufficient_depth_cells() {
    let (swept, status) = run_cli(&["sweep", "specs/cart_v1.fsl"]);
    assert_eq!(status, 0, "swept: {swept:#}");
    assert_eq!(swept["result"], "sweep_passed");
    assert!(swept["sweep"]["minimal_counterexample"].is_null());
    assert_eq!(swept["sweep"]["inconclusive_scopes"], serde_json::json!([]));
}

#[test]
fn sweep_all_insufficient_depth_cells_are_inconclusive() {
    let (swept, status) = run_cli(&["sweep", "specs/cart_v1.fsl", "--depth", "0..0"]);
    assert_eq!(status, 1, "swept: {swept:#}");
    assert_eq!(swept["result"], "sweep_inconclusive");
    assert!(swept["sweep"]["minimal_counterexample"].is_null());
    assert!(swept["sweep"]["results"].is_array());
    assert!(swept["sweep"]["ranges"].is_object());
    // Without --instances/--values the grid is one scope (#1089).
    assert_eq!(
        swept["sweep"]["inconclusive_scopes"],
        serde_json::json!([{"instances": {}, "values": {}}])
    );
}

#[test]
fn sweep_mixed_reachability_classifications_remain_failed() {
    let path = fixture("issue_634_reachable_classification.fsl");
    let (swept, status) = run_cli(&["sweep", &path, "--depth", "0..0"]);
    assert_eq!(status, 1, "swept: {swept:#}");
    assert_eq!(swept["result"], "sweep_failed");
}

#[test]
fn sweep_cart_controls_keep_genuine_failure_and_clean_success() {
    let (buggy, buggy_status) = run_cli(&["sweep", "specs/cart_buggy.fsl"]);
    assert_eq!(buggy_status, 1, "buggy: {buggy:#}");
    assert_eq!(buggy["result"], "sweep_failed");
    assert!(!buggy["sweep"]["minimal_counterexample"].is_null());

    let (implemented, implemented_status) = run_cli(&["sweep", "specs/cart_impl.fsl"]);
    assert_eq!(implemented_status, 0, "implemented: {implemented:#}");
    assert_eq!(implemented["result"], "sweep_passed");
    assert!(implemented["sweep"]["minimal_counterexample"].is_null());
}

/// Regression for #1080's minimal-selection fix: a grid whose early cells are
/// inconclusive (`reachable_failed` with an `unreached` array containing only
/// `classification:"insufficient_depth"`) and whose later cells contain a
/// genuine bug (`ReachTwo` is reachable at depth 2, and `BoundedBelowFour` is
/// only violated once `x` reaches 4 at depth 4) must select the true-failure
/// cell as `minimal_counterexample`, not the earlier inconclusive one. Before
/// af7046cc, `sweep` chose the first cell whose `verification` was not
/// success-classified, which would have picked the depth-0 inconclusive cell
/// instead.
///
/// Per-cell classification uses `verification` (the full envelope), not
/// `summary`: `summary` deliberately omits `unreached`
/// (`main.rs` `run_sweep`'s summary key list), so it cannot distinguish an
/// inconclusive `reachable_failed` cell from a true one.
#[test]
fn sweep_skips_inconclusive_cell_when_selecting_true_failure_scope() {
    let (swept, status) = run_cli(&[
        "sweep",
        &fixture("sweep_inconclusive_then_failure.fsl"),
        "--depth",
        "0..4",
    ]);
    assert_eq!(status, 1, "swept: {swept:#}");
    assert_eq!(swept["result"], "sweep_failed");
    let results = swept["sweep"]["results"].as_array().expect("sweep results");
    let first_inconclusive = results
        .iter()
        .position(|cell| {
            cell["verification"]["result"] == "reachable_failed"
                && cell["verification"]["unreached"]
                    .as_array()
                    .is_some_and(|unreached| {
                        !unreached.is_empty()
                            && unreached
                                .iter()
                                .all(|item| item["classification"] == "insufficient_depth")
                    })
        })
        .expect("grid has an inconclusive cell");
    let minimal = swept["sweep"]["minimal_counterexample"]
        .as_object()
        .expect("true failure minimal counterexample");
    assert_eq!(
        minimal["summary"]["result"], "violated",
        "minimal must be the genuine invariant violation, not an inconclusive cell: {minimal:#?}"
    );
    let scope = &minimal["scope"];
    let selected = results
        .iter()
        .position(|cell| cell["scope"] == *scope)
        .expect("minimal's scope matches a grid cell");
    assert!(
        selected > first_inconclusive,
        "minimal (index {selected}) must skip the earlier inconclusive cell (index {first_inconclusive}): {swept:#}"
    );
}

/// Per-cell `(values, depth, result)` rows, for readable assertions.
fn cells(swept: &serde_json::Value) -> Vec<(serde_json::Value, u64, String)> {
    swept["sweep"]["results"]
        .as_array()
        .expect("sweep.results")
        .iter()
        .map(|entry| {
            (
                entry["scope"]["values"].clone(),
                entry["scope"]["depth"].as_u64().expect("cell depth"),
                entry["summary"]["result"]
                    .as_str()
                    .expect("cell result")
                    .to_owned(),
            )
        })
        .collect()
}

fn amount_scope(lo: i64, hi: i64) -> serde_json::Value {
    serde_json::json!({"instances": {}, "values": {"Amount": [lo, hi]}})
}

/// #1089: a determinate success in one `--values` scope does not answer the
/// question a depth-limited cell left open in another scope. Before the fix
/// this grid returned `sweep_passed`/exit 0 although `Big` was never
/// witnessed for `Amount` 1..1, 1..2 or 1..3.
#[test]
fn sweep_success_in_one_values_scope_does_not_settle_another() {
    let path = fixture("sweep_values_scope_inconclusive.fsl");
    let (swept, status) = run_cli(&["sweep", &path, "--values", "Amount=1..5", "--depth", "3..3"]);
    let observed = cells(&swept);
    let results = observed
        .iter()
        .map(|(_, _, result)| result.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        results,
        [
            "reachable_failed",
            "reachable_failed",
            "reachable_failed",
            "verified",
            "verified"
        ],
        "grid shape: {swept:#}"
    );
    assert_eq!(swept["result"], "sweep_inconclusive", "{swept:#}");
    assert_eq!(status, 1, "{swept:#}");
    assert!(swept["sweep"]["minimal_counterexample"].is_null());
    assert_eq!(
        swept["sweep"]["inconclusive_scopes"],
        serde_json::json!([amount_scope(1, 1), amount_scope(1, 2), amount_scope(1, 3)]),
        "{swept:#}"
    );
}

/// #1089 keeps #1080's depth-axis rule: within one values scope a success at
/// a larger depth still settles the depth-limited smaller depth.
#[test]
fn sweep_larger_depth_still_settles_the_same_values_scope() {
    let path = fixture("sweep_values_scope_inconclusive.fsl");
    let (swept, status) = run_cli(&["sweep", &path, "--values", "Amount=3..5", "--depth", "3..4"]);
    let observed = cells(&swept);
    assert_eq!(
        observed[0],
        (
            serde_json::json!({"Amount": [3, 3]}),
            3,
            "reachable_failed".to_owned()
        ),
        "{swept:#}"
    );
    assert_eq!(swept["result"], "sweep_passed", "{swept:#}");
    assert_eq!(status, 0, "{swept:#}");
    assert_eq!(swept["sweep"]["inconclusive_scopes"], serde_json::json!([]));
}

/// Every values scope has a determinate success: the grid passes and reports
/// no inconclusive scope.
#[test]
fn sweep_all_values_scopes_successful_passes() {
    let path = fixture("sweep_values_scope_inconclusive.fsl");
    let (swept, status) = run_cli(&["sweep", &path, "--values", "Amount=4..5", "--depth", "3..3"]);
    assert!(
        cells(&swept)
            .iter()
            .all(|(_, _, result)| result == "verified"),
        "{swept:#}"
    );
    assert_eq!(swept["result"], "sweep_passed", "{swept:#}");
    assert_eq!(status, 0, "{swept:#}");
    assert!(swept["sweep"]["minimal_counterexample"].is_null());
    assert_eq!(swept["sweep"]["inconclusive_scopes"], serde_json::json!([]));
}

/// Every values scope is depth-limited only: `sweep_inconclusive` and every
/// scope is reported.
#[test]
fn sweep_all_values_scopes_inconclusive_reports_every_scope() {
    let path = fixture("sweep_values_scope_inconclusive.fsl");
    let (swept, status) = run_cli(&["sweep", &path, "--values", "Amount=1..3", "--depth", "3..3"]);
    assert!(
        cells(&swept)
            .iter()
            .all(|(_, _, result)| result == "reachable_failed"),
        "{swept:#}"
    );
    assert_eq!(swept["result"], "sweep_inconclusive", "{swept:#}");
    assert_eq!(status, 1, "{swept:#}");
    assert!(swept["sweep"]["minimal_counterexample"].is_null());
    assert_eq!(
        swept["sweep"]["inconclusive_scopes"],
        serde_json::json!([amount_scope(1, 1), amount_scope(1, 2), amount_scope(1, 3)]),
        "{swept:#}"
    );
}

/// A true failure dominates inconclusive values scopes: `sweep_failed` with
/// the violating scope as the minimal counterexample, while the inconclusive
/// scopes are still reported.
#[test]
fn sweep_violation_dominates_inconclusive_values_scopes() {
    let path = fixture("sweep_values_scope_violation.fsl");
    let (swept, status) = run_cli(&["sweep", &path, "--values", "Amount=1..3", "--depth", "3..3"]);
    let results = cells(&swept)
        .into_iter()
        .map(|(_, _, result)| result)
        .collect::<Vec<_>>();
    assert_eq!(
        results,
        ["reachable_failed", "reachable_failed", "violated"],
        "{swept:#}"
    );
    assert_eq!(swept["result"], "sweep_failed", "{swept:#}");
    assert_eq!(status, 1, "{swept:#}");
    assert_eq!(
        swept["sweep"]["minimal_counterexample"]["scope"]["values"],
        serde_json::json!({"Amount": [1, 3]}),
        "{swept:#}"
    );
    assert_eq!(
        swept["sweep"]["inconclusive_scopes"],
        serde_json::json!([amount_scope(1, 1), amount_scope(1, 2)]),
        "{swept:#}"
    );
}
