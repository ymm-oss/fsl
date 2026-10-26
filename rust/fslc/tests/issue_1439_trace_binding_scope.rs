// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const BINDING_STEP: &str = r#"requirements E {
  enum Color { Red, Blue }
  state { o: Option<Color>, c: Color }
  init { o = none c = Red }
  requirement REQ-1 "paint" {
    action paint(x: Color) { c = x }
  }
  acceptance AC-1 "conditional binding does not escape" {
    paint(if o is some(z) then z else Red)
    expect c == Red
  }
}
"#;

const REORDERED_PARTICLE: &str = r#"fsl 5
requirements R {
  number Amount
  actor Employee
  entity Claim { amount: Amount = 0 lifecycle Draft -> Submitted }
  event submit = Employee submits Claim with amount Draft -> Submitted
  scenario AC-1 "instance-first diagnostic" {
    claim1 を Employee が amount (if claim_stage[0] != 1 then 1 else 2) で submits
    then claim1 is Submitted
  }
}
verify { instances Claim = 2 values Amount = 0..3 }
"#;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn check(label: &str, source: &str, fsl5: bool) -> Value {
    let scratch = Scratch(
        Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1439_trace_binding_scope")
            .join(label),
    );
    std::fs::create_dir_all(&scratch.0).expect("create owned inline fixture directory");
    std::fs::write(scratch.0.join("trace.fsl"), source).expect("write inline fixture");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(["check", "trace.fsl"])
        .current_dir(&scratch.0)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if fsl5 {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run native check");
    assert!(output.stderr.is_empty(), "{label}: {output:?}");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("native JSON envelope");
    assert_eq!(
        (
            output.status.code(),
            envelope["result"].as_str(),
            envelope["kind"].as_str()
        ),
        (Some(2), Some("error"), Some("semantics")),
        "{label}: {envelope:#}"
    );
    envelope
}

#[test]
fn trace_argument_rejects_pattern_binding_outside_its_logical_continuation() {
    let output = check("binding-step", BINDING_STEP, false);
    assert!(
        output["message"].as_str().is_some_and(|message| {
            message.contains("public Kernel cannot type identifier 'z'")
        })
    );
    assert_eq!(output["loc"], json!({"line": 9, "column": 5}));
}

#[test]
fn reordered_particle_argument_error_starts_at_the_instance_token() {
    let output = check("instance-first", REORDERED_PARTICLE, true);
    assert!(output["message"].as_str().is_some_and(|message| {
        message.contains("expression of type Int is not assignable to Named(\"ClaimStage\")")
    }));
    assert_eq!(output["loc"], json!({"line": 8, "column": 5}));
    assert_eq!(
        REORDERED_PARTICLE
            .lines()
            .nth(7)
            .unwrap()
            .split_whitespace()
            .next(),
        Some("claim1")
    );
    assert_eq!(output["edition"], "next");
    assert_eq!(output["fsl_edition"], 5);
}
