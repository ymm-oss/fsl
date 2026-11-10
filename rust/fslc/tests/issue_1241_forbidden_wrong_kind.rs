// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1241: a `forbidden` last step whose argument is not of its
//! parameter's kind — an enum member or a Bool for an `entity` or range
//! parameter, an integer or a member of another enum for an enum parameter —
//! was classified `bad_call` (#1212: "outside the declared type") and so
//! satisfied the forbidden, although no guard was evaluated and the step is a
//! spec typo, not a rejection an implementation makes. It is now a
//! `kind: "forbidden"` error (exit 2) carrying `wrong_kind_argument`, on every
//! command that runs the forbidden gate. A value of the right kind outside a
//! declared range stays `bad_call` (#1212), and one outside a verify scope
//! stays the #1229 error.
//!
//! The main mutation these tests kill is restoring the #1212 classification,
//! which counts every out-of-domain argument that is not an integer outside a
//! verify scope as `bad_call`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("fslc-issue-1241-{name}-{}", std::process::id()));
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

/// Run `fslc` on `source` written to a scratch `spec.fsl`.
fn run_spec(source: &str, name: &str, arguments: &[&str]) -> (Value, i32) {
    let directory = scratch(name);
    std::fs::write(directory.join("spec.fsl"), source).expect("write spec");
    let (command, rest) = arguments.split_first().expect("a command");
    let mut full = vec![*command, "spec.fsl"];
    full.extend_from_slice(rest);
    let outcome = run(&directory, &full);
    let _ = std::fs::remove_dir_all(&directory);
    outcome
}

/// The issue's spec: `respond(c: Case)` has no guard, so only a malformed
/// final step could "reject" it.
fn cases(final_step: &str) -> String {
    format!(
        r#"requirements CaseScope {{
  entity Case
  enum St {{ Waiting, Accepted, Responded }}
  state {{ cases: Map<Case, St> }}
  init {{ forall c: Case {{ cases[c] = Waiting }} }}
  requirement REQ-1 "accept" {{
    action accept(c: Case) {{ requires cases[c] == Waiting  cases[c] = Accepted }}
  }}
  requirement REQ-2 "respond" {{
    action respond(c: Case) {{ cases[c] = Responded }}
  }}
  forbidden FB-1 "cannot respond before accept" {{
    {final_step}
    expect rejected
  }}
}}
verify {{ instances Case = 3 }}
"#
    )
}

/// A range parameter (`withdraw`) and an enum parameter (`paint`), both
/// guarded so that an in-kind value can be refused by a guard.
fn shop(final_step: &str) -> String {
    format!(
        r#"requirements Shop {{
  type Amount = 0..100
  enum Color {{ Red, Blue }}
  enum Size {{ Small, Large }}
  state {{ balance: 0..100, color: Color }}
  init {{ balance = 50  color = Red }}
  requirement REQ-1 "withdraw" {{
    action withdraw(amount: Amount) {{ requires amount <= balance  balance = balance - amount }}
  }}
  requirement REQ-2 "paint" {{
    action paint(c: Color) {{ requires c != color  color = c }}
  }}
  forbidden FB-1 "the call is rejected" {{
    {final_step}
    expect rejected
  }}
}}
"#
    )
}

fn assert_wrong_kind(output: &Value, status: i32, parameter: &str, value: &Value, ty: &str) {
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "forbidden", "{output:#}");
    assert_eq!(output["trace_type"], "forbidden", "{output:#}");
    assert_eq!(output["id"], "FB-1", "{output:#}");
    assert_eq!(output["failed_step"], 0, "{output:#}");
    assert_eq!(
        output["wrong_kind_argument"],
        json!({"parameter": parameter, "value": value, "type": ty}),
        "{output:#}"
    );
    let message = output["message"].as_str().expect("message");
    assert!(
        message.contains(&format!("is not a value of type '{ty}'")),
        "{message}"
    );
    let hint = output["hint"].as_str().expect("hint");
    assert!(hint.contains("malformed rather than rejected"), "{hint}");
    assert!(output.get("accepted_trace").is_none(), "{output:#}");
    assert!(output.get("out_of_scope_argument").is_none(), "{output:#}");
}

/// Issue reproductions and the kinds the acceptance criteria name (another
/// enum's member, a Bool) for an `entity`, a range, and an enum parameter —
/// detector (mutation: restore #1212's "every non-scope out-of-domain
/// argument is `bad_call`"). Each used to make `check` exit 0.
#[test]
fn a_final_step_argument_of_another_kind_is_not_a_rejection() {
    for (source, line, parameter, value, ty, name) in [
        (
            cases("respond(Waiting)"),
            13,
            "c",
            json!("Waiting"),
            "Case",
            "entity-enum",
        ),
        (
            cases("respond(true)"),
            13,
            "c",
            json!(true),
            "Case",
            "entity-bool",
        ),
        (
            shop("withdraw(Red)"),
            14,
            "amount",
            json!("Red"),
            "Amount",
            "range-enum",
        ),
        (
            shop("withdraw(false)"),
            14,
            "amount",
            json!(false),
            "Amount",
            "range-bool",
        ),
        (
            shop("paint(Large)"),
            14,
            "c",
            json!("Large"),
            "Color",
            "enum-other-enum",
        ),
        (
            shop("paint(true)"),
            14,
            "c",
            json!(true),
            "Color",
            "enum-bool",
        ),
        // An integer is not an enum member: step arguments are not
        // ordinals, so `paint(1)` is not `paint(Blue)`.
        (shop("paint(1)"), 14, "c", json!(1), "Color", "enum-int"),
    ] {
        let (output, status) = run_spec(&source, &format!("{name}-check"), &["check"]);
        assert_wrong_kind(&output, status, parameter, &value, ty);
        // Located at its final step.
        assert_eq!(
            output["loc"],
            json!({"line": line, "column": 5}),
            "{name}: {output:#}"
        );
    }
}

/// Every command that runs the forbidden gate agrees with `check`
/// (DESIGN-forbidden.md §2.1) — detector (mutation: classify the wrong kind
/// only on the `check` path, e.g. in `scenarios`' own replay). Before #1241,
/// `scenarios` emitted the forbidden scenario as `rejected_by: "bad_call"`,
/// `testgen` wrote a test asserting `bad_call`, and the rest verified.
#[test]
fn a_wrong_kind_final_step_exits_2_on_every_gate_command() {
    let directory = scratch("gate-commands");
    std::fs::write(directory.join("spec.fsl"), cases("respond(Waiting)")).expect("write spec");
    let wrong = json!("Waiting");
    for args in [
        &["verify", "spec.fsl", "--depth", "2", "--no-cache"][..],
        &["verify", "spec.fsl", "--engine", "induction", "--no-cache"],
        &[
            "verify",
            "spec.fsl",
            "--depth",
            "2",
            "--engine",
            "explicit",
            "--no-cache",
        ],
        &["sweep", "spec.fsl", "--depth", "1..2"],
        &["scenarios", "spec.fsl", "--depth", "2"],
        &[
            "testgen", "spec.fsl", "--depth", "2", "--target", "pytest", "-o", "t.py",
        ],
        &["mutate", "spec.fsl", "--depth", "1"],
        &[
            "counterexample",
            "export",
            "spec.fsl",
            "--depth",
            "2",
            "-o",
            "cx.json",
        ],
    ] {
        let (output, status) = run(&directory, args);
        assert_wrong_kind(&output, status, "c", &wrong, "Case");
    }
    assert!(!directory.join("t.py").exists());
    assert!(!directory.join("cx.json").exists());
    // The reports print the same error and write nothing (#1242, #1243).
    for args in [
        &["html", "spec.fsl", "--depth", "2", "-o", "report.html"][..],
        &["ledger", "spec.fsl", "--depth", "2", "-o", "ledger.md"],
    ] {
        let (output, status) = run(&directory, args);
        assert_wrong_kind(&output, status, "c", &wrong, "Case");
    }
    assert!(!directory.join("report.html").exists());
    assert!(!directory.join("ledger.md").exists());
    std::fs::write(
        directory.join("fsl-project.toml"),
        "[requirements]\nfile = \"spec.fsl\"\ndepth = 2\n",
    )
    .expect("write fsl-project.toml");
    let (output, status) = run(&directory, &["chain", "fsl-project.toml"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["failed"], json!(["requirements"]), "{output:#}");
    assert_wrong_kind(&output["layers"][0]["detail"], status, "c", &wrong, "Case");
    let _ = std::fs::remove_dir_all(&directory);
}

/// Preservation controls: an in-kind integer below a declared range stays
/// `bad_call` (#1212; `withdraw(500)` above it is pinned by
/// `issue_1212_forbidden_bad_call.rs`), and a member of the parameter's own
/// enum that the guard refuses stays `requires_failed` — detector (mutation:
/// treat every out-of-domain argument, or every enum argument, as a wrong
/// kind).
#[test]
fn in_kind_values_keep_their_rejection() {
    for (step, rejected_by) in [
        ("withdraw(-1)", "bad_call"),
        ("paint(Red)", "requires_failed"),
    ] {
        let source = shop(step);
        let (output, status) = run_spec(&source, "in-kind-check", &["check"]);
        assert_eq!(
            (output["result"].as_str(), status),
            (Some("ok"), 0),
            "{step}: {output:#}"
        );
        let (scenarios, status) =
            run_spec(&source, "in-kind-scenarios", &["scenarios", "--depth", "1"]);
        assert_eq!(status, 0, "{step}: {scenarios:#}");
        let forbidden = scenarios["scenarios"]
            .as_array()
            .expect("scenarios")
            .iter()
            .find(|scenario| scenario["kind"] == "forbidden")
            .expect("forbidden scenario");
        assert_eq!(
            forbidden["rejected_by"], rejected_by,
            "{step}: {forbidden:#}"
        );
    }
}

/// A call with one argument outside its range and another of the wrong kind
/// is malformed, not a `bad_call` — detector (mutation: classify by the
/// first out-of-domain argument only, which is the in-kind 500).
#[test]
fn a_wrong_kind_argument_wins_over_an_out_of_range_one() {
    let source = r#"requirements Pay {
  type Amount = 0..100
  state { paid: Amount }
  init { paid = 0 }
  requirement REQ-1 "pay" {
    action pay(a: Amount, urgent: Bool) { paid = a }
  }
  forbidden FB-1 "the call is rejected" {
    pay(500, 3)
    expect rejected
  }
}
"#;
    let (output, status) = run_spec(source, "mixed", &["check"]);
    assert_wrong_kind(&output, status, "urgent", &json!(3), "Bool");
}

/// `fslc diff` reads an OLD wrong-kind final step as no OLD rejection, so
/// the forbidden is `unknown` / `forbidden_replay_failed`, never preserved —
/// detector (mutation: classify the OLD side with the #1212 rule). Before
/// #1241 this spec against itself was `no_semantic_change`.
#[test]
fn diff_does_not_preserve_an_old_wrong_kind_final_step() {
    let directory = scratch("diff");
    let spec = shop("withdraw(Red)");
    std::fs::write(directory.join("old.fsl"), &spec).expect("write OLD");
    std::fs::write(directory.join("new.fsl"), &spec).expect("write NEW");
    let (output, status) = run(
        &directory,
        &[
            "diff", "old.fsl", "new.fsl", "--depth", "2", "--forbid", "unknown",
        ],
    );
    let _ = std::fs::remove_dir_all(&directory);
    let findings = output["findings"]
        .as_array()
        .expect("diff findings")
        .iter()
        .filter(|finding| finding["id"] == "FB-1")
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1, "{output:#}");
    assert_eq!(findings[0]["kind"], "unknown", "{output:#}");
    assert_eq!(findings[0]["reason"], "forbidden_replay_failed");
    let detail = findings[0]["detail"].as_str().expect("detail");
    assert!(detail.starts_with("OLD argument \"Red\""), "{detail}");
    assert_eq!(status, 1, "{output:#}");
}
