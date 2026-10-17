// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1239: `fslc diff` reported an OLD `forbidden` as preserved when a
//! NEW guard disabled one of its *setup* steps. NEW then never runs the final
//! step, so whether NEW still rejects it is unknown; only a disabled final
//! step is evidence of a preserved rejection. Such a forbidden is now
//! `unknown` / `forbidden_replay_failed`, like a NEW setup step that violates
//! (#1213) and an OLD setup step that is not enabled.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("fslc-issue-1239-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn run(directory: &Path, arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// `fslc diff old.fsl new.fsl --forbid <forbid>`.
fn diff(old: &str, new: &str, forbid: &str, name: &str) -> (Value, i32) {
    let directory = scratch(name);
    std::fs::write(directory.join("old.fsl"), old).expect("write OLD");
    std::fs::write(directory.join("new.fsl"), new).expect("write NEW");
    let outcome = run(
        &directory,
        &[
            "diff", "old.fsl", "new.fsl", "--depth", "2", "--forbid", forbid,
        ],
    );
    let _ = std::fs::remove_dir_all(&directory);
    outcome
}

/// A Wallet whose `withdraw` has `guards`, with the forbidden `steps`.
fn wallet(guards: &str, steps: &str) -> String {
    format!(
        r#"requirements Wallet {{
  type Amount = 0..100
  state {{ balance: -100..100 }}
  init {{ balance = 50 }}
  requirement REQ-1 "withdraw" {{
    action withdraw(amount: Amount) {{ {guards} balance = balance - amount }}
  }}
  forbidden FB-1 "overdraw is rejected" {{
    {steps}
    expect rejected
  }}
}}
"#
    )
}

const OLD_GUARD: &str = "requires amount <= balance ";
const STEPS: &str = "withdraw(10)\n    withdraw(60)";

fn forbidden_findings(output: &Value) -> Vec<Value> {
    output["findings"]
        .as_array()
        .expect("diff findings")
        .iter()
        .filter(|finding| finding["id"] == "FB-1")
        .cloned()
        .collect()
}

/// Issue reproduction — detector (mutation: restore `let instance =
/// instance?;`, which returns "preserved" for a disabled NEW step whatever
/// its position). NEW refuses the setup `withdraw(10)` and drops the
/// overdraw guard, so the final `withdraw(60)` is never run in NEW; the
/// forbidden used to be preserved and `--forbid unknown` passed.
#[test]
fn a_new_setup_step_its_guard_disables_is_not_a_preserved_forbidden() {
    let old = wallet(OLD_GUARD, STEPS);
    let new = wallet(
        "requires amount != 10  requires amount <= balance + 20 ",
        STEPS,
    );
    let (output, status) = diff(&old, &new, "unknown", "setup-disabled");
    assert_eq!(
        forbidden_findings(&output),
        vec![json!({
            "kind": "unknown",
            "subject": "forbidden",
            "id": "FB-1",
            "reason": "forbidden_replay_failed",
            "detail": "NEW forbidden setup was not enabled",
            "step": 0,
            "action": "withdraw",
        })],
        "{output:#}"
    );
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["gate"]["passed"], false, "{output:#}");
    // The finding is no relaxation: the final step was never run in NEW.
    let (output, status) = diff(&old, &new, "forbidden_relaxed", "setup-disabled-relaxed");
    assert!(
        output["summary"]
            .as_array()
            .expect("summary")
            .iter()
            .all(|kind| kind != "forbidden_relaxed"),
        "{output:#}"
    );
    assert_eq!(status, 0, "{output:#}");
}

/// Preservation control — detector (mutation: also turn a disabled NEW
/// *final* step into `unknown`). A NEW guard that refuses the final step
/// preserves the forbidden; the stricter NEW guard is no finding for it.
#[test]
fn a_new_final_step_its_guard_disables_stays_preserved() {
    let old = wallet(OLD_GUARD, STEPS);
    let new = wallet("requires amount <= balance - 5 ", STEPS);
    let (output, status) = diff(&old, &new, "forbidden_relaxed,unknown", "final-disabled");
    assert_eq!(
        forbidden_findings(&output),
        Vec::<Value>::new(),
        "{output:#}"
    );
    // Neither `unknown` nor `forbidden_relaxed` anywhere, so the gate passes.
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["gate"]["passed"], true, "{output:#}");
}

/// The issue's second criterion: the OLD side under the same principle. An
/// OLD setup step that OLD's own guard disables never established an OLD
/// rejection, so the forbidden is `unknown` / `forbidden_replay_failed`,
/// whatever NEW does — pinned here so the two sides cannot drift apart.
#[test]
fn an_old_setup_step_its_guard_disables_is_not_a_preserved_forbidden() {
    let steps = "withdraw(70)\n    withdraw(60)";
    let old = wallet(OLD_GUARD, steps);
    let (output, status) = diff(&old, &old, "unknown", "old-setup-disabled");
    let findings = forbidden_findings(&output);
    assert_eq!(findings.len(), 1, "{output:#}");
    assert_eq!(findings[0]["kind"], "unknown", "{output:#}");
    assert_eq!(findings[0]["reason"], "forbidden_replay_failed");
    assert_eq!(findings[0]["detail"], "OLD forbidden setup was not enabled");
    assert_eq!(findings[0]["step"], 0, "{output:#}");
    assert_eq!(status, 1, "{output:#}");
}
