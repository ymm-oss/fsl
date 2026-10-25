// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1438: auto-mapped fsl 5 events preserve the business transition's actor,
//! including events used by scenario steps, rules and decisions. Requirement
//! annotations on either side must not replace the actor's semantic identity.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const EXPENSE_BUSINESS: &str = include_str!("fixtures/fsl5/e2e/1_business.fsl");

fn fixture(label: &str, source: &str, business: &str) -> PathBuf {
    // Keep the inputs for diagnosis; each process and control has its own path.
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fsl5_actor_refinement")
        .join(std::process::id().to_string())
        .join(label);
    std::fs::create_dir_all(&directory).expect("create fixture directory");
    std::fs::write(directory.join("business.fsl"), business).expect("write business");
    std::fs::write(directory.join("requirements.fsl"), source).expect("write requirements");
    directory
}

fn fslc(directory: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(directory)
        .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
        .output()
        .expect("run fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "fslc {args:?}: invalid JSON ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

fn assert_actor_mismatch(label: &str, source: &str, business: &str, action: &str) {
    let directory = fixture(label, source, business);
    let (output, exit) = fslc(&directory, &["check", "requirements.fsl"]);
    let expected = format!(
        "maps auto actor mismatch for action '{action}': impl actor 'Manager' != abstract actor 'System'"
    );
    assert_eq!(
        exit, 2,
        "{label}: expected error/type with {expected:?}; produced result={} kind={} message={}",
        output["result"], output["kind"], output["message"]
    );
    assert_eq!(output["result"], "error", "{label}: {output:#}");
    assert_eq!(output["kind"], "type", "{label}: {output:#}");
    assert!(
        output["message"]
            .as_str()
            .expect("diagnostic message")
            .contains(&expected),
        "{label}: {output:#} lacks {expected:?}"
    );
}

fn expense_source(actor: &str, covers: &str) -> String {
    format!(
        r#"fsl 5
requirements R5 {{
  implements ExpenseToBe from "business.fsl" {{ }}
  actor Employee, System, Manager, Finance
  entity Claim {{
    lifecycle Draft -> Submitted -> Approved
    lifecycle Submitted -> Rejected
    lifecycle Approved -> Paid
  }}
  event submit       = Employee submits Claim Draft -> Submitted
  event auto_approve = {actor} approves Claim Submitted -> Approved
    {covers}
  event mgr_approve  = Manager approves Claim Submitted -> Approved
  event reject       = Manager rejects Claim Submitted -> Rejected
  event pay          = Finance pays Claim Approved -> Paid
}}
verify {{ instances Claim = 3 }}
"#
    )
}

fn business_source(covers: &str) -> String {
    format!(
        r"business Abs {{
  actor System, Manager
  entity Case
  process Case {{
    stages Open, Done
    initial Open
    transition approve Open -> Done by System
      {covers}
  }}
}}
verify {{ instances Case = 1 }}
"
    )
}

fn case_source(member: &str) -> String {
    format!(
        r#"fsl 5
requirements Impl {{
  implements Abs from "business.fsl" {{ }}
  actor System, Manager
  entity Case {{ lifecycle Open -> Done }}
  event approve = Manager approves Case Open -> Done
  {member}
}}
verify {{ instances Case = 1 }}
"#
    )
}

// A1 detector: the issue's negative control must fail at check time, not refine.
#[test]
fn event_with_wrong_actor_is_a_check_time_error() {
    assert_actor_mismatch(
        "event",
        &expense_source("Manager", ""),
        EXPENSE_BUSINESS,
        "auto_approve",
    );
}

// A2 preservation: changing only the actor makes check and the inline seam pass.
#[test]
fn event_with_correct_actor_checks_and_refines() {
    let directory = fixture(
        "correct_actor",
        &expense_source("System", ""),
        EXPENSE_BUSINESS,
    );
    for (args, expected_result) in [
        (vec!["check", "requirements.fsl"], "ok"),
        (
            vec!["verify", "requirements.fsl", "--depth", "6"],
            "verified",
        ),
    ] {
        let (output, exit) = fslc(&directory, &args);
        assert_eq!(exit, 0, "{args:?}: {output:#}");
        assert_eq!(output["result"], expected_result, "{args:?}: {output:#}");
        assert_eq!(output["implements"]["result"], "refines", "{output:#}");
    }
}

// A3 detectors: these forms consume or guard an event, not a separate action.
#[test]
fn scenario_step_event_with_wrong_actor_is_a_check_time_error() {
    assert_actor_mismatch(
        "step",
        &case_source(
            r#"scenario AC-1 "Approval step" {
    Manager approves case1
    then case1 is Done
  }"#,
        ),
        &business_source(""),
        "approve",
    );
}

#[test]
fn rule_event_with_wrong_actor_is_a_check_time_error() {
    assert_actor_mismatch(
        "rule",
        &case_source(
            r#"rule REQ-1 "Approval authority" {
    allow Manager approves Case only when true
  }"#,
        ),
        &business_source(""),
        "approve",
    );
}

#[test]
fn decide_event_with_wrong_actor_is_a_check_time_error() {
    assert_actor_mismatch(
        "decide",
        &case_source(
            r#"decide REQ-1 "Approval authority" for Open Case {
    otherwise => Manager approves
  }"#,
        ),
        &business_source(""),
        "approve",
    );
}

// A4 preservation: the fsl 4 process form already rejects this mismatch.
#[test]
fn fsl4_process_actor_mismatch_diagnostic_is_preserved() {
    assert_actor_mismatch(
        "fsl4_process",
        r#"requirements Impl {
  implements Abs from "business.fsl" { }
  process Case {
    stages Open, Done
    initial Open
    transition approve Open -> Done by Manager
  }
}
verify { instances Case = 1 }
"#,
        &business_source(""),
        "approve",
    );
}

// A5 detector: an authored requirement is not the implementation actor.
#[test]
fn event_covers_does_not_hide_actor_mismatch() {
    assert_actor_mismatch(
        "event_covers",
        &expense_source("Manager", r#"covers REQ-1 "Approval authority""#),
        EXPENSE_BUSINESS,
        "auto_approve",
    );
}

// A6 detector: the business actor must also come from typed source information.
#[test]
fn business_covers_does_not_hide_fsl5_event_actor_mismatch() {
    assert_actor_mismatch(
        "business_covers",
        &case_source(""),
        &business_source(r#"covers BIZ-1 "Approval authority""#),
        "approve",
    );
}
