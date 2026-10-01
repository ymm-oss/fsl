// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native contract for #1008: an option that makes `verify` skip a declared
//! check keeps that check's envelope key and says why, instead of dropping it.
//!
//! Before #1008, `--property`, `--exclude-property`, and `--from-state` removed
//! the inline `implements` key with no reason, and `--instances`/`--values`
//! skipped the requirements `acceptance`/`forbidden` replay with no trace in
//! the envelope at all. Since #1218 a scoped run replays the scenarios and
//! skips only the ones its scope puts out of range; those are what
//! `requirement_traces` reports (`issue_1218_acceptance_replay.rs` pins the
//! replay itself). A consumer could not tell "nothing declared" from
//! "declared but not checked", and a broken seam passed a selected run with
//! exit 0 looking exactly like a spec without one.
//!
//! Every detector below names the mutation it kills: restoring the silent
//! skip (the pre-#1008 behavior on `origin/main`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/issue_1008_not_evaluated";

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

fn run(arguments: &[&str]) -> (Value, i32) {
    run_with_env(arguments, &[])
}

fn verify_broken(extra: &[&str]) -> (Value, i32) {
    let path = fixture("impl_broken.fsl");
    let mut arguments = vec!["verify", path.as_str(), "--depth", "3", "--no-cache"];
    arguments.extend_from_slice(extra);
    run(&arguments)
}

fn not_evaluated_implements(reasons: &[&str]) -> Value {
    json!({
        "abs": "CountedFlow",
        "result": "not_evaluated",
        "reason": reasons[0],
        "reasons": reasons,
    })
}

/// Preservation control for the unfiltered path: the full run still evaluates
/// the seam, folds its failure, and carries no `not_evaluated` section. Every
/// detector below depends on this fixture's seam actually being broken.
#[test]
fn unfiltered_verify_still_evaluates_and_folds_the_seam() {
    let (output, status) = verify_broken(&[]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "refinement_failed", "{output:#}");
    assert_eq!(output["implements"]["result"], "refinement_failed");
    assert!(output.get("requirement_traces").is_none(), "{output:#}");
}

/// detector (mutation: restore the silent `--property` skip)
#[test]
fn property_selection_reports_implements_not_evaluated() {
    let (output, status) = verify_broken(&["--property", "Small"]);
    // The verdict still speaks only for the selected property (no change).
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    assert_eq!(
        output["implements"],
        not_evaluated_implements(&["property_selection"]),
        "{output:#}"
    );
}

/// detector (mutation: restore the silent `--exclude-property` skip)
#[test]
fn property_exclusion_reports_implements_not_evaluated() {
    let (output, status) = verify_broken(&["--exclude-property", "Tiny"]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    assert_eq!(
        output["implements"],
        not_evaluated_implements(&["property_exclusion"]),
        "{output:#}"
    );
}

/// detector (mutation: restore the silent `--from-state` skip)
#[test]
fn from_state_reports_implements_not_evaluated() {
    let state = fixture("state.json");
    let (output, status) = verify_broken(&["--from-state", &state]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    assert_eq!(output["initial_state"]["source"], "snapshot", "{output:#}");
    assert_eq!(
        output["implements"],
        not_evaluated_implements(&["from_state"]),
        "{output:#}"
    );
}

/// detector (mutation: report only one cause, or in an unstable order)
#[test]
fn every_suppressor_is_listed_in_a_fixed_order() {
    let state = fixture("state.json");
    let (output, status) = verify_broken(&[
        "--from-state",
        &state,
        // Exclusion wins over a selection of the same name, so this is the
        // one combination where all three suppressors are accepted together.
        "--exclude-property",
        "Small",
        "--property",
        "Small",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(
        output["implements"],
        not_evaluated_implements(&["property_selection", "property_exclusion", "from_state"]),
        "{output:#}"
    );
}

/// detector (mutation: the `--lemma` path, which returns before the default
/// path, restores the silent skip)
#[test]
fn lemma_path_reports_implements_not_evaluated() {
    let (output, status) = verify_broken(&[
        "--engine",
        "induction",
        "--property",
        "Small",
        "--lemma",
        "Tiny",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "proved", "{output:#}");
    assert_eq!(
        output["implements"],
        not_evaluated_implements(&["property_selection"]),
        "{output:#}"
    );
}

/// Rejecting control: a spec that declares no inline `implements` keeps the
/// key absent under a selection, so "not declared" stays distinguishable from
/// "declared but not evaluated" (mutation: attach the section unconditionally).
#[test]
fn a_spec_without_implements_keeps_the_key_absent() {
    let path = fixture("abs.fsl");
    let (output, status) = run(&[
        "verify",
        &path,
        "--depth",
        "3",
        "--no-cache",
        "--property",
        "_bounds_n",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert!(output.get("implements").is_none(), "{output:#}");
}

/// detector (mutation: a cached replay of a selected run loses the section)
#[test]
fn a_cached_selected_run_replays_the_section() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let cache = std::env::temp_dir().join(format!(
        "fsl-issue-1008-cache-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&cache).expect("cache dir must not already exist");
    let path = fixture("impl_broken.fsl");
    let arguments = [
        "verify",
        path.as_str(),
        "--depth",
        "3",
        "--property",
        "Small",
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
        cached["implements"],
        not_evaluated_implements(&["property_selection"]),
        "{cached:#}"
    );
}

/// Preservation control: an unscoped run replays acceptance, so the failing
/// scenario is a hard error. The scoped detector below depends on it.
#[test]
fn unscoped_verify_rejects_the_failing_acceptance() {
    let path = fixture("impl_failing_acceptance.fsl");
    let (output, status) = run(&["verify", &path, "--depth", "3", "--no-cache"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "acceptance", "{output:#}");
}

/// detector (mutation: restore the pre-#1218 skip of the whole replay under
/// `--instances`/`--values`, which reported this run as `verified` with a
/// `not_evaluated` section). `expect n == 2` references no out-of-scope
/// value, so a scoped run replays it and fails exactly like an unscoped one.
#[test]
fn bounds_override_replays_the_failing_acceptance() {
    let path = fixture("impl_failing_acceptance.fsl");
    let (output, status) = run(&[
        "verify",
        &path,
        "--depth",
        "3",
        "--no-cache",
        "--values",
        "Limit=0..2",
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "acceptance", "{output:#}");
    assert_eq!(output["trace_type"], "acceptance", "{output:#}");
    assert!(output.get("requirement_traces").is_none(), "{output:#}");
}

/// detector (mutation: a scenario skipped as out of scope is not reported, or
/// is reported without the skipped id and reference). `pick(2)` is outside
/// `Limit=0..1`, so AC-1 is skipped and the section lists it.
#[test]
fn bounds_override_reports_the_out_of_scope_scenario_not_evaluated() {
    let path = fixture("impl_out_of_scope_acceptance.fsl");
    let (output, status) = run(&[
        "verify",
        &path,
        "--depth",
        "3",
        "--no-cache",
        "--values",
        "Limit=0..1",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    assert_eq!(output["implements"]["result"], "refines", "{output:#}");
    assert_eq!(
        output["requirement_traces"],
        json!({
            "result": "not_evaluated",
            "reason": "bounds_override",
            "reasons": ["bounds_override"],
            "skipped": [{
                "kind": "acceptance",
                "id": "AC-1",
                "reference": "pick(2): argument 2 for 'v' is outside its domain",
            }],
        }),
        "{output:#}"
    );
}

/// Rejecting control (mutation: attach the section whenever scenarios are
/// declared under a scope): a scope that keeps every scenario in range
/// replays them all and has no `requirement_traces` key.
#[test]
fn bounds_override_replaying_every_scenario_adds_no_section() {
    let path = fixture("impl_out_of_scope_acceptance.fsl");
    let (output, status) = run(&[
        "verify",
        &path,
        "--depth",
        "3",
        "--no-cache",
        "--values",
        "Limit=0..2",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    assert!(output.get("requirement_traces").is_none(), "{output:#}");
}

/// detector (mutation: an unextractable trace contract is treated as "no
/// scenarios", or as not evaluated, under a scoped run). The scoped run
/// rejects the duplicate id exactly like the unscoped control.
#[test]
fn bounds_override_rejects_an_unextractable_trace_contract() {
    let path = fixture("impl_duplicate_acceptance.fsl");
    let (unscoped, unscoped_status) = run(&["verify", &path, "--depth", "3", "--no-cache"]);
    assert_eq!(unscoped_status, 2, "{unscoped:#}");
    assert_eq!(unscoped["kind"], "semantics", "{unscoped:#}");
    let (output, status) = run(&[
        "verify",
        &path,
        "--depth",
        "3",
        "--no-cache",
        "--values",
        "Limit=0..2",
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "semantics", "{output:#}");
    assert_eq!(output["message"], unscoped["message"], "{output:#}");
    assert!(output.get("requirement_traces").is_none(), "{output:#}");
}

/// Rejecting control: a scoped run of a spec without acceptance/forbidden
/// scenarios has nothing skipped and no `requirement_traces` key.
#[test]
fn bounds_override_without_scenarios_adds_no_section() {
    let path = fixture("abs.fsl");
    let (output, status) = run(&[
        "verify",
        &path,
        "--depth",
        "3",
        "--no-cache",
        "--values",
        "Limit=0..2",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert!(output.get("requirement_traces").is_none(), "{output:#}");
}

/// detector (mutation: a selected run skips the warning finalization again).
/// Compose-lowering warnings were computed on every run but only attached on
/// a full one, so `--property` hid `fair_not_inherited`.
#[test]
fn property_selection_keeps_compose_warnings() {
    let path = "rust/fslc/tests/fixtures/issue_474_fair_loss.fsl";
    for extra in [&[][..], &["--property", "core.CoreOk"][..]] {
        let mut arguments = vec!["verify", path, "--depth", "3", "--no-cache"];
        arguments.extend_from_slice(extra);
        let (output, status) = run(&arguments);
        assert_eq!(status, 0, "{output:#}");
        let kinds = output["warnings"]
            .as_array()
            .expect("warnings array")
            .iter()
            .filter(|warning| warning["kind"] == "fair_not_inherited")
            .count();
        assert_eq!(kinds, 1, "args={extra:?}: {output:#}");
    }
}

/// Rejecting control for `attach_not_evaluated`'s error-envelope rule: when
/// `--vacuity error` replaces a selected run's envelope with an `error` one,
/// no `not_evaluated` section survives onto it (mutation: carry the section
/// over into the vacuity error envelope). The `warn` run is the control that
/// the fixture is vacuous and that the section was attached before.
#[test]
fn vacuity_error_envelope_drops_the_section() {
    let path = fixture("impl_vacuous.fsl");
    let base = ["verify", path.as_str(), "--depth", "3", "--no-cache"];
    let mut warn = base.to_vec();
    warn.extend_from_slice(&["--property", "Hollow"]);
    let (warned, warned_status) = run(&warn);
    assert_eq!(warned_status, 0, "{warned:#}");
    assert_eq!(
        warned["implements"]["result"], "not_evaluated",
        "{warned:#}"
    );
    let mut error = warn.clone();
    error.extend_from_slice(&["--vacuity", "error"]);
    let (errored, errored_status) = run(&error);
    assert_eq!(errored_status, 2, "{errored:#}");
    assert_eq!(errored["result"], "error", "{errored:#}");
    assert!(errored.get("implements").is_none(), "{errored:#}");
    assert!(errored.get("requirement_traces").is_none(), "{errored:#}");
}

fn sweep(arguments: &[&str]) -> (Value, i32) {
    let mut full = vec!["sweep"];
    full.extend_from_slice(arguments);
    let cache = std::env::temp_dir().join(format!(
        "fsl-issue-1008-sweep-{}-{}",
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

/// detector (mutation: `sweep` aggregates only `result`/`checked_to_depth`
/// and hides the cells' skipped scenarios behind `sweep_passed`). Only the
/// cells whose scope puts `pick(2)` out of range (`Limit=0..0`, `0..1`) skip
/// AC-1; the grid verdict is unchanged and the skip is surfaced.
#[test]
fn sweep_surfaces_skipped_requirement_traces() {
    let path = fixture("impl_out_of_scope_acceptance.fsl");
    let (output, status) = sweep(&[&path, "--depth", "1..2", "--values", "Limit=0..3"]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "sweep_passed", "{output:#}");
    assert_eq!(
        output["sweep"]["not_evaluated"],
        json!({"sections": ["requirement_traces"], "reasons": ["bounds_override"]}),
        "{output:#}"
    );
    let rows = output["sweep"]["results"].as_array().expect("sweep rows");
    assert!(!rows.is_empty(), "{output:#}");
    for row in rows {
        let upper = row["scope"]["values"]["Limit"][1].as_i64().expect("upper");
        if upper < 2 {
            assert_eq!(
                row["summary"]["requirement_traces"], "not_evaluated",
                "{row:#}"
            );
        } else {
            assert!(
                row["summary"].get("requirement_traces").is_none(),
                "{row:#}"
            );
        }
        assert!(row["summary"].get("implements").is_none(), "{row:#}");
    }
}

/// detector (mutation: the sweep union drops `implements` skipped by
/// `--property`, or loses its reason)
#[test]
fn sweep_surfaces_a_seam_skipped_by_property_selection() {
    let path = fixture("impl_out_of_scope_acceptance.fsl");
    let (output, status) = sweep(&[
        &path,
        "--depth",
        "1..2",
        "--values",
        "Limit=0..3",
        "--property",
        "Small",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "sweep_passed", "{output:#}");
    assert_eq!(
        output["sweep"]["not_evaluated"],
        json!({
            "sections": ["implements", "requirement_traces"],
            "reasons": ["property_selection", "bounds_override"],
        }),
        "{output:#}"
    );
    for row in output["sweep"]["results"].as_array().expect("sweep rows") {
        assert_eq!(row["summary"]["implements"], "not_evaluated", "{row:#}");
    }
}

/// detector (mutation: sweep cells skip the scenario replay again). A failing
/// in-range acceptance is an error the sweep returns, not a passing grid.
#[test]
fn sweep_returns_the_failing_acceptance() {
    let path = fixture("impl_failing_acceptance.fsl");
    let (output, status) = sweep(&[&path, "--depth", "1..2", "--values", "Limit=0..3"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "acceptance", "{output:#}");
}

/// Rejecting control: a sweep with nothing skipped has no
/// `sweep.not_evaluated` key and no per-row section entries.
#[test]
fn sweep_without_skipped_checks_adds_no_key() {
    let path = fixture("abs.fsl");
    let (output, status) = sweep(&[&path, "--depth", "1..2", "--values", "Limit=0..3"]);
    assert_eq!(status, 0, "{output:#}");
    assert!(output["sweep"].get("not_evaluated").is_none(), "{output:#}");
    for row in output["sweep"]["results"].as_array().expect("sweep rows") {
        assert!(
            row["summary"].get("requirement_traces").is_none(),
            "{row:#}"
        );
    }
}
