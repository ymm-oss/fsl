// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

fn repository_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_file(""))
        .output()
        .expect("run native fslc")
}

fn divergent_finding(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("analysis JSON");
    envelope["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["finding_type"] == "divergent_choice")
        .expect("divergent_choice")
        .clone()
}

#[test]
fn undecided_declarations_surface_in_reports_and_acknowledge_without_suppression() {
    let spec = repository_file("tests/fixtures/undecided.fsl");
    let spec = spec.to_str().expect("UTF-8 fixture path");

    let ledger = run(&["ledger", spec]);
    assert!(
        ledger.status.success(),
        "{}",
        String::from_utf8_lossy(&ledger.stderr)
    );
    let ledger = String::from_utf8(ledger.stdout).expect("ledger UTF-8");
    for expected in [
        "## 未決定一覧",
        "`init`",
        "`action approve`",
        "REQ-1",
        "approval versus rejection policy is pending",
    ] {
        assert!(
            ledger.contains(expected),
            "missing {expected:?} from ledger"
        );
    }
    assert!(!ledger.contains("| undecided |"));

    let html = run(&["html", spec]);
    assert!(
        html.status.success(),
        "{}",
        String::from_utf8_lossy(&html.stderr)
    );
    let html = String::from_utf8(html.stdout).expect("HTML UTF-8");
    for expected in [
        "Intentional Undecided Decisions",
        "action approve",
        "REQ-1",
        "approval versus rejection policy is pending",
    ] {
        assert!(html.contains(expected), "missing {expected:?} from HTML");
    }

    let finding = divergent_finding(&run(&["analyze", spec, "--profile", "ai-review"]));
    assert_eq!(finding["formal_status"], "not_a_violation");
    assert_eq!(finding["acknowledged"], true);
    assert_eq!(
        finding["acknowledged_by"],
        json!([{
            "declaration": "action approve",
            "reason": "approval versus rejection policy is pending",
        }])
    );

    let snapshot: Value = serde_json::from_slice(
        &std::fs::read(repository_file("tests/snapshots/undecided_report.json"))
            .expect("read snapshot"),
    )
    .expect("snapshot JSON");
    let ledger_section = ledger
        .split_once("## 未決定一覧")
        .expect("undecided section")
        .1
        .split_once("## リスク一覧")
        .expect("risk section")
        .0;
    assert_eq!(
        format!("## 未決定一覧{}", ledger_section.trim_end()),
        snapshot["ledger_section"]
    );
    assert_eq!(
        json!({
            "finding_type": finding["finding_type"],
            "formal_status": finding["formal_status"],
            "acknowledged": finding["acknowledged"],
            "acknowledged_by": finding["acknowledged_by"],
        }),
        snapshot["finding"]
    );
}

#[test]
fn unmarked_underspecification_remains_unacknowledged() {
    let source = r#"
spec ReviewChoice {
  state { ready: Bool, approved: Bool }
  init { ready = true  approved = false }
  action approve() {
    requires ready
    ready = false
    approved = true
  }
  action reject() {
    requires ready
    ready = false
    approved = false
  }
  invariant ApprovedWhenDone "REQ-1: a completed review is approved" {
    ready or approved
  }
}
"#;
    let path = std::env::temp_dir().join(format!(
        "fsl-issue-189-unacknowledged-{}.fsl",
        std::process::id()
    ));
    std::fs::write(&path, source).expect("write temporary spec");
    let path_text = path.to_str().expect("UTF-8 temporary path");
    let finding = divergent_finding(&run(&["analyze", path_text, "--profile", "ai-review"]));
    std::fs::remove_file(path).expect("remove temporary spec");

    assert!(finding.get("acknowledged").is_none());
    assert!(finding.get("acknowledged_by").is_none());
}
