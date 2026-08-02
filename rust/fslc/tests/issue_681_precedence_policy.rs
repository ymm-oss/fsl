// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-681-{name}-{}-{nonce}.fsl",
            std::process::id()
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    fn text(&self) -> &str {
        self.0.to_str().expect("UTF-8 temporary path")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
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

const BYPASS: &str = r#"business ReturnHandling {
  actor Manager
  entity Return

  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition bypass Requested -> Refunded by Manager
    transition refund Approved -> Refunded by Manager
  }

  policy CTRL-APPROVAL "no completion without approval"
    every Return reaching Refunded must have passed through Approved
}
verify {
  instances Return = 1
}
"#;

const POLICY_VARIANT_NEGATIVE_CONTROLS: &str = r#"business PolicyVariantControls {
  actor Worker
  entity Item

  process Item {
    stages Open, Reviewed, Closed
    initial Open
    transition review Open -> Reviewed by Worker
    transition close Reviewed -> Closed by Worker
    transition bypass Open -> Closed by Worker
    transition stayClosed Closed -> Closed by Worker
  }

  policy CTRL-INVARIANT "closed is forbidden" invariant {
    forall item: Item { stage(item) != Closed }
  }
  policy CTRL-RESPONDS "open responds immediately with review" responds {
    forall item: Item {
      stage(item) == Open ~> within 0 stage(item) == Reviewed
    }
  }
  policy CTRL-EVENTUALLY "open items are reviewed"
    every Item in Open must eventually be Reviewed
  policy CTRL-PRECEDENCE "closing requires review"
    every Item reaching Closed must have passed through Reviewed
}
verify { instances Item = 1 }
"#;

#[test]
fn every_business_policy_variant_has_a_native_rejecting_control() {
    let fixture = Fixture::new("policy-variant-controls", POLICY_VARIANT_NEGATIVE_CONTROLS);
    for (property, kind) in [
        ("CTRL-INVARIANT", "invariant"),
        ("CTRL-RESPONDS", "leadsTo"),
        ("CTRL-EVENTUALLY", "leadsTo"),
        ("CTRL-PRECEDENCE", "invariant"),
    ] {
        let (output, status) = run(&[
            "verify",
            fixture.text(),
            "--depth",
            "2",
            "--property",
            property,
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        assert_eq!(status, 1, "{property}: {output:#}");
        assert_eq!(output["result"], "violated", "{property}: {output:#}");
        assert_eq!(output["violation_kind"], kind, "{property}: {output:#}");
        assert_eq!(output["invariant"], property, "{property}: {output:#}");
    }
}

#[test]
fn native_bmc_rejects_a_bypass_with_policy_attribution_and_replay() {
    let fixture = Fixture::new("bypass", BYPASS);
    let (output, status) = run(&[
        "verify",
        fixture.text(),
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);

    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(output["violation_kind"], "invariant", "{output:#}");
    assert_eq!(output["invariant"], "CTRL-APPROVAL", "{output:#}");
    assert_eq!(output["requirement"]["id"], "CTRL-APPROVAL", "{output:#}");
    assert_eq!(output["last_action"]["name"], "bypass", "{output:#}");
    assert_eq!(
        output["trace"].as_array().map(Vec::len),
        Some(2),
        "{output:#}"
    );
}

#[test]
fn native_explicit_engine_is_an_independent_negative_control_for_the_bypass() {
    let fixture = Fixture::new("explicit-bypass", BYPASS);
    let (output, status) = run(&[
        "verify",
        fixture.text(),
        "--depth",
        "2",
        "--engine",
        "explicit",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);

    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(output["invariant"], "CTRL-APPROVAL", "{output:#}");
    assert_eq!(output["last_action"]["name"], "bypass", "{output:#}");
}

#[test]
fn compliant_policy_verifies_with_bmc_and_proves_with_induction() {
    let source = BYPASS.replace(
        "    transition bypass Requested -> Refunded by Manager\n",
        "",
    );
    let fixture = Fixture::new("compliant", &source);
    let (bmc, bmc_status) = run(&[
        "verify",
        fixture.text(),
        "--depth",
        "4",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(bmc_status, 0, "{bmc:#}");
    assert_eq!(bmc["result"], "verified", "{bmc:#}");
    assert!(
        bmc["invariants_checked"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "CTRL-APPROVAL")),
        "{bmc:#}"
    );

    let (induction, induction_status) = run(&[
        "verify",
        fixture.text(),
        "--depth",
        "4",
        "--engine",
        "induction",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(induction_status, 0, "{induction:#}");
    assert_eq!(induction["result"], "proved", "{induction:#}");
    assert_eq!(
        induction["k_used"]["CTRL-APPROVAL_stability"], 1,
        "{induction:#}"
    );
}

#[test]
fn initial_target_violation_and_waypoint_equal_to_target_keep_boundary_semantics() {
    let initial_target = Fixture::new(
        "initial-target",
        r#"business InitialTarget {
  actor Worker
  entity Item
  process Item {
    stages Done, Approved
    initial Done
    transition approve Done -> Approved by Worker
    transition finish Approved -> Done by Worker
  }
  policy CTRL-INITIAL-TARGET "the target starts before approval"
    every Item reaching Done must have passed through Approved
}
verify { instances Item = 1 }
"#,
    );
    let (violated, violated_status) = run(&[
        "verify",
        initial_target.text(),
        "--depth",
        "1",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(violated_status, 1, "{violated:#}");
    assert_eq!(violated["invariant"], "CTRL-INITIAL-TARGET", "{violated:#}");
    assert_eq!(violated["violated_at_step"], 0, "{violated:#}");

    let same_stage = Fixture::new(
        "same-stage",
        r#"business SameStage {
  actor Worker
  entity Item
  process Item {
    stages Requested, Done
    initial Requested
    transition finish Requested -> Done by Worker
  }
  policy CTRL-SAME "landing on the target records the waypoint"
    every Item reaching Done must have passed through Done
}
verify { instances Item = 1 }
"#,
    );
    let (verified, verified_status) = run(&[
        "verify",
        same_stage.text(),
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(verified_status, 0, "{verified:#}");
    assert_eq!(verified["result"], "verified", "{verified:#}");
}

#[test]
fn target_and_waypoint_disjunctions_in_a_cyclic_process_prove_under_induction() {
    let fixture = Fixture::new(
        "disjunctions",
        r#"business Disjunctions {
  actor Worker
  entity Item
  process Item {
    stages Requested, Approved, Rejected, Closed, Archived
    initial Requested
    transition approve Requested -> Approved by Worker
    transition reject Requested -> Rejected by Worker
    transition close Approved -> Closed by Worker
    transition archive Rejected -> Archived by Worker
    transition reopenClosed Closed -> Requested by Worker
    transition reopenArchived Archived -> Requested by Worker
  }
  policy CTRL-DISJUNCTION "both decisions and both outcomes are covered"
    every Item reaching Closed or Archived must have passed through Approved or Rejected
}
verify { instances Item = 1 }
"#,
    );
    let (output, status) = run(&[
        "verify",
        fixture.text(),
        "--depth",
        "4",
        "--engine",
        "induction",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "proved", "{output:#}");
    assert_eq!(output["k_used"]["CTRL-DISJUNCTION_stability"], 1);
}

#[test]
fn unknown_and_ambiguous_references_fail_with_policy_identity_and_location() {
    for (name, process, policy, expected) in [
        (
            "unknown-stage",
            r"process Return {
    stages Requested, Refunded
    initial Requested
    transition refund Requested -> Refunded by Manager
  }",
            "every Return reaching Refunded must have passed through Missing",
            "stage 'Missing' is not declared",
        ),
        (
            "unknown-entity",
            r"process Return {
    stages Requested, Refunded
    initial Requested
    transition refund Requested -> Refunded by Manager
  }",
            "every Invoice reaching Refunded must have passed through Requested",
            "entity 'Invoice' has no process",
        ),
        (
            "ambiguous-entity",
            r"process current.Return {
    stages Requested, Refunded
    initial Requested
    transition currentRefund Requested -> Refunded by Manager
  }
  process legacy.Return {
    stages Requested, Refunded
    initial Requested
    transition legacyRefund Requested -> Refunded by Manager
  }",
            "every Return reaching Refunded must have passed through Requested",
            "multiple processes; precedence policy is ambiguous",
        ),
    ] {
        let source = format!(
            r#"business InvalidPrecedence {{
  actor Manager
  entity Return
  {process}
  policy CTRL-BAD "invalid precedence reference"
    {policy}
}}
verify {{ instances Return = 1 }}
"#
        );
        let fixture = Fixture::new(name, &source);
        let (output, status) = run(&["check", fixture.text()]);
        assert_eq!(status, 2, "{output:#}");
        assert_eq!(output["kind"], "semantics", "{output:#}");
        let message = output["message"].as_str().expect("diagnostic message");
        assert!(message.contains("policy 'CTRL-BAD'"), "{output:#}");
        assert!(message.contains(expected), "{output:#}");
        assert!(output["loc"]["line"].as_u64().is_some(), "{output:#}");
        assert!(output["loc"]["column"].as_u64().is_some(), "{output:#}");
    }
}
