// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #728: a bounded never-enabled action is a typed vacuity finding. The
//! fixture becomes enabled exactly at K, so K-1 proves the warning's bounded
//! evidence while K is the rejecting control against a permanent-dead-action
//! false positive.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const FIXTURE: &str = "rust/fslc/tests/fixtures/never_enabled_depth_boundary.fsl";
const ORIGIN_FIXTURE: &str = "rust/fslc/tests/fixtures/issue_641_domain_unreachable_decide.fsl";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli(arguments: &[&str]) -> (Value, i32) {
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

fn has_never_enabled_warning(output: &Value) -> bool {
    output["warnings"].as_array().is_some_and(|warnings| {
        warnings.iter().any(|warning| {
            warning["kind"] == "never_enabled_action"
                && warning["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("action 'late' is never enabled"))
        })
    })
}

#[test]
fn never_enabled_action_is_bounded_and_check_does_not_fabricate_coverage() {
    let (checked, check_status) = run_cli(&["check", FIXTURE]);
    assert_eq!(check_status, 0, "{checked:#}");
    assert_eq!(checked["result"], "ok", "{checked:#}");
    assert!(checked.get("action_coverage").is_none(), "{checked:#}");
    assert!(
        !has_never_enabled_warning(&checked),
        "check must not fabricate verification coverage: {checked:#}"
    );

    let (before, before_status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(before_status, 0, "{before:#}");
    assert_eq!(before["result"], "verified", "{before:#}");
    assert_eq!(
        before["action_coverage"]["late"]["covered"], false,
        "{before:#}"
    );
    assert!(has_never_enabled_warning(&before), "{before:#}");
    let warning = before["warnings"]
        .as_array()
        .and_then(|warnings| {
            warnings
                .iter()
                .find(|warning| warning["kind"] == "never_enabled_action")
        })
        .expect("never-enabled warning");
    assert_eq!(warning["name"], "late", "{warning:#}");
    assert_eq!(
        warning["loc"],
        json!({"line": 15, "column": 3}),
        "{warning:#}"
    );
    assert_eq!(warning["requirement"]["id"], "REQ-LATE", "{warning:#}");
    assert_eq!(
        warning["requirements"].as_array().map(Vec::len),
        Some(1),
        "{warning:#}"
    );

    let (at_boundary, boundary_status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(boundary_status, 0, "{at_boundary:#}");
    assert_eq!(
        at_boundary["action_coverage"]["late"], true,
        "{at_boundary:#}"
    );
    assert!(
        !has_never_enabled_warning(&at_boundary),
        "the finding must disappear when late becomes enabled at K: {at_boundary:#}"
    );
}

#[test]
fn never_enabled_action_obeys_warn_error_and_ignore() {
    let (error, error_status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
        "--no-cache",
    ]);
    assert_eq!(error_status, 2, "{error:#}");
    assert_eq!(error["result"], "error", "{error:#}");
    assert_eq!(error["kind"], "never_enabled_action", "{error:#}");
    assert_eq!(error["trace_type"], "vacuity", "{error:#}");
    let finding = error["findings"]
        .as_array()
        .and_then(|findings| findings.first())
        .expect("vacuity finding");
    assert_eq!(finding["name"], "late", "{finding:#}");
    assert_eq!(
        finding["loc"],
        json!({"line": 15, "column": 3}),
        "{finding:#}"
    );
    assert_eq!(finding["requirement"]["id"], "REQ-LATE", "{finding:#}");
    assert_eq!(
        finding["requirements"].as_array().map(Vec::len),
        Some(1),
        "{finding:#}"
    );

    let (ignored, ignored_status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--vacuity",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(ignored_status, 0, "{ignored:#}");
    assert_eq!(ignored["result"], "verified", "{ignored:#}");
    assert!(
        !has_never_enabled_warning(&ignored),
        "ignore must remove the typed finding: {ignored:#}"
    );
}

#[test]
fn never_enabled_lowered_action_uses_its_origin_primary_location() {
    let (output, status) = run_cli(&[
        "verify",
        ORIGIN_FIXTURE,
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{output:#}");
    let warning = output["warnings"]
        .as_array()
        .and_then(|warnings| {
            warnings
                .iter()
                .find(|warning| warning["generated_name"] == "doc_archive_doc")
        })
        .expect("never-enabled lowered ArchiveDoc warning");
    assert_eq!(warning["kind"], "never_enabled_action", "{warning:#}");
    assert_eq!(warning["name"], "ArchiveDoc", "{warning:#}");
    assert_eq!(
        warning["loc"],
        json!({"line": 26, "column": 5}),
        "{warning:#}"
    );
    assert_eq!(
        warning["loc"]["line"], warning["origin"]["primary"]["span"]["start"]["line"],
        "warning location must use origin.primary.span.start.line: {warning:#}"
    );
    assert_eq!(
        warning["loc"]["column"], warning["origin"]["primary"]["span"]["start"]["column"],
        "warning location must use origin.primary.span.start.column: {warning:#}"
    );
}

#[test]
fn never_enabled_action_selects_consistently_for_all_engines_and_sweep() {
    for engine in ["bmc", "explicit", "induction"] {
        let (output, status) = run_cli(&[
            "verify",
            FIXTURE,
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "--vacuity",
            "error",
            "--engine",
            engine,
            "--no-cache",
        ]);
        assert_eq!(status, 2, "{engine}: {output:#}");
        assert_eq!(
            output["kind"], "never_enabled_action",
            "{engine}: {output:#}"
        );

        let (at_boundary, boundary_status) = run_cli(&[
            "verify",
            FIXTURE,
            "--depth",
            "3",
            "--deadlock",
            "ignore",
            "--vacuity",
            "error",
            "--engine",
            engine,
            "--no-cache",
        ]);
        assert_eq!(boundary_status, 0, "{engine}: {at_boundary:#}");
        assert_ne!(at_boundary["result"], "error", "{engine}: {at_boundary:#}");
        assert!(
            !has_never_enabled_warning(&at_boundary),
            "the K-side control must not report a finding for {engine}: {at_boundary:#}"
        );
    }

    let (sweep, sweep_status) = run_cli(&[
        "sweep",
        FIXTURE,
        "--depth",
        "2..2",
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
    ]);
    assert_eq!(sweep_status, 2, "{sweep:#}");
    assert_eq!(sweep["result"], "error", "{sweep:#}");
    assert_eq!(sweep["kind"], "never_enabled_action", "{sweep:#}");

    let (sweep_at_boundary, boundary_sweep_status) = run_cli(&[
        "sweep",
        FIXTURE,
        "--depth",
        "3..3",
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
    ]);
    assert_eq!(boundary_sweep_status, 0, "{sweep_at_boundary:#}");
    assert_eq!(
        sweep_at_boundary["result"], "sweep_passed",
        "{sweep_at_boundary:#}"
    );
    assert!(
        sweep_at_boundary["sweep"]["results"]
            .as_array()
            .is_some_and(|results| results
                .iter()
                .all(|entry| !has_never_enabled_warning(&entry["verification"]))),
        "the K-side sweep control must not report a finding: {sweep_at_boundary:#}"
    );
}
