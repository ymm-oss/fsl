// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Oracle-attribution controls for issue #848.
//!
//! Default `fslc mutate` output must remain byte-identical to the pre-change
//! contract; `--oracle-attribution` is the only surface that exposes full killer
//! sets and per-oracle sole/shared counts keyed by oracle display names.

use std::process::Command;

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn run_cli(args: &[&str]) -> (serde_json::Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn run_cli_stdout(args: &[&str]) -> (Vec<u8>, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    (
        output.stdout,
        output.status.code().expect("native exit status"),
    )
}

const BANK_HEALTHY: &str = "rust/fslc/tests/fixtures/issue_848_bank_healthy.fsl";
const BANK_DROP: &str = "rust/fslc/tests/fixtures/issue_848_bank_drop_ensures.fsl";

fn deposit_assignment_mutant(output: &serde_json::Value) -> &serde_json::Value {
    output["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| {
            mutant["op"] == "assignment_remove"
                && mutant["target"]
                    .as_str()
                    .is_some_and(|target| target.contains("deposit assignment"))
        })
        .expect("deposit assignment_remove mutant")
}

/// Calibration: default stdout must not gain attribution fields (opt-in only).
#[test]
fn mutate_default_stdout_is_byte_identical_to_baseline_fixture() {
    let (stdout, status) = run_cli_stdout(&["mutate", BANK_HEALTHY, "--depth", "8"]);
    assert_eq!(status, 0);
    let golden = include_bytes!("fixtures/issue_848_bank_healthy_default.stdout.json");
    assert_eq!(
        stdout.as_slice(),
        golden.as_slice(),
        "default mutate stdout changed; opt-in fields must not alter the default envelope"
    );
}

/// Negative control (pre-attribution semantics): without the flag, redundancy is invisible.
#[test]
fn mutate_without_oracle_attribution_omits_killers_and_by_obligation() {
    let (output, status) = run_cli(&["mutate", BANK_HEALTHY, "--depth", "8"]);
    assert_eq!(status, 0, "{output}");
    assert!(output.get("by_obligation").is_none(), "{output}");
    assert!(output.get("attribution").is_none(), "{output}");
    let mutant = deposit_assignment_mutant(&output);
    assert_eq!(mutant["status"], "killed", "{mutant}");
    assert_eq!(mutant["killed_by"], "deposit", "{mutant}");
    assert!(mutant.get("killers").is_none(), "{mutant}");
}

/// Positive control: redundant killers are visible when the flag is set.
#[test]
fn mutate_oracle_attribution_reports_shared_killers_for_deposit_assignment() {
    let (healthy, status) = run_cli(&[
        "mutate",
        BANK_HEALTHY,
        "--depth",
        "8",
        "--oracle-attribution",
    ]);
    assert_eq!(status, 0, "{healthy}");
    let mutant = deposit_assignment_mutant(&healthy);
    assert_eq!(mutant["killed_by"], "deposit", "{mutant}");
    let killers = mutant["killers"].as_array().expect("killers");
    assert_eq!(killers.len(), 2, "{mutant}");
    assert!(killers.iter().any(|killer| killer == "deposit"), "{mutant}");
    assert!(killers.iter().any(|killer| killer == "Funded"), "{mutant}");
    let deposit = &healthy["by_obligation"]["deposit"];
    assert_eq!(deposit["kills_any"], 1, "{healthy}");
    assert_eq!(deposit["sole_kills"], 0, "{healthy}");
    assert_eq!(deposit["shared_kills"], 1, "{healthy}");
    assert_eq!(
        healthy["attribution"],
        serde_json::json!({"mode": "all_killers", "order_independent": true})
    );
}

/// Redundancy phenomenon on the implementation version: union kill-rate is preserved while
/// first-killer attribution shifts and the dropped ensures oracle disappears from killers.
#[test]
fn mutate_oracle_attribution_detects_ensures_redundancy_across_healthy_and_drop_specs() {
    let (healthy, healthy_status) = run_cli(&[
        "mutate",
        BANK_HEALTHY,
        "--depth",
        "8",
        "--oracle-attribution",
    ]);
    let (dropped, dropped_status) =
        run_cli(&["mutate", BANK_DROP, "--depth", "8", "--oracle-attribution"]);
    assert_eq!(healthy_status, 0, "{healthy}");
    assert_eq!(dropped_status, 0, "{dropped}");
    assert_eq!(
        healthy["summary"]["kill_rate"], dropped["summary"]["kill_rate"],
        "union kill-rate must stay equal across healthy and drop_ensures"
    );
    assert_eq!(
        healthy["summary"]["killed"], dropped["summary"]["killed"],
        "{healthy} vs {dropped}"
    );
    assert_eq!(
        healthy["summary"]["total"], dropped["summary"]["total"],
        "{healthy} vs {dropped}"
    );
    let healthy_mutant = deposit_assignment_mutant(&healthy);
    let dropped_mutant = deposit_assignment_mutant(&dropped);
    assert_eq!(healthy_mutant["status"], "killed", "{healthy_mutant}");
    assert_eq!(dropped_mutant["status"], "killed", "{dropped_mutant}");
    assert_eq!(healthy_mutant["killed_by"], "deposit", "{healthy_mutant}");
    assert_eq!(dropped_mutant["killed_by"], "Funded", "{dropped_mutant}");
    let healthy_killers = healthy_mutant["killers"]
        .as_array()
        .expect("healthy killers");
    let dropped_killers = dropped_mutant["killers"]
        .as_array()
        .expect("dropped killers");
    assert!(
        healthy_killers.iter().any(|killer| killer == "deposit"),
        "{healthy_mutant}"
    );
    assert!(
        !dropped_killers.iter().any(|killer| killer == "deposit"),
        "{dropped_mutant}"
    );
    assert!(
        dropped_killers.iter().any(|killer| killer == "Funded"),
        "{dropped_mutant}"
    );
    assert!(
        dropped["by_obligation"].get("deposit").is_none(),
        "dropped ensures must remove the deposit oracle from aggregation: {dropped}"
    );
}

/// Sole-killer control: when only one oracle rejects a mutant, `shared_kills` stay zero.
#[test]
fn mutate_oracle_attribution_counts_sole_kills_without_shared_kills() {
    let (dropped, status) = run_cli(&["mutate", BANK_DROP, "--depth", "8", "--oracle-attribution"]);
    assert_eq!(status, 0, "{dropped}");
    let funded = &dropped["by_obligation"]["Funded"];
    assert!(
        funded["sole_kills"].as_u64().unwrap_or(0) >= 1,
        "Funded must have at least one sole kill after ensures removal: {dropped}"
    );
    assert_eq!(funded["shared_kills"], 0, "{dropped}");
}
