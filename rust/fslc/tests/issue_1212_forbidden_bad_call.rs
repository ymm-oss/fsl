// SPDX-License-Identifier: Apache-2.0

//! Issue #1212: a `forbidden` case whose final step is a call outside the
//! declared parameter type (`bad_call`, DESIGN-bridge.md 1.2) was labelled
//! `rejected_by: "requires_failed"`, so the generated negative test asserted a
//! guard refusal the spec never states. A final step that names no action, or
//! no variant of that arity, was also accepted as a rejection, so a typo made
//! `expect rejected` pass vacuously (the frozen Python reference reports it).
//! `fslc diff` preserves a both-side `bad_call` only outside a declared type,
//! never one decided by an `entity` / `number` verify scope, and `fslc ledger`
//! does not summarize an unknown final action or a broken setup as an
//! accepted final step.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("fslc-issue-1212-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn wallet(final_step: &str) -> String {
    format!(
        r#"requirements Wallet {{
  type Amount = 0..100
  state {{ balance: 0..100 }}
  init {{ balance = 50 }}

  requirement REQ-1 "Withdraw within balance" {{
    action withdraw(amount: Amount) {{ requires amount <= balance  balance = balance - amount }}
  }}

  forbidden FB-1 "The final withdrawal is rejected" {{
    withdraw(10)
    {final_step}
    expect rejected
  }}
}}"#
    )
}

fn write_spec(directory: &Path, final_step: &str) -> PathBuf {
    let path = directory.join("wallet.fsl");
    std::fs::write(&path, wallet(final_step)).expect("write wallet fixture");
    path
}

fn pytest_for(final_step: &str, name: &str) -> String {
    let directory = scratch(name);
    let spec = write_spec(&directory, final_step);
    let output_path = directory.join("test_wallet.py");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("testgen")
        .arg(&spec)
        .args(["--depth", "3", "--target", "pytest", "-o"])
        .arg(&output_path)
        .output()
        .expect("run native testgen");
    assert!(
        output.status.success(),
        "testgen failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let content = std::fs::read_to_string(&output_path).expect("read generated pytest");
    let _ = std::fs::remove_dir_all(&directory);
    content
}

fn check(final_step: &str, name: &str) -> (Value, i32) {
    let directory = scratch(name);
    let spec = write_spec(&directory, final_step);
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("check")
        .arg(&spec)
        .output()
        .expect("run native check");
    let _ = std::fs::remove_dir_all(&directory);
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

#[test]
fn an_out_of_type_final_step_is_rejected_by_bad_call_not_requires_failed() {
    let content = pytest_for("withdraw(500)", "out-of-type");
    assert!(
        content.contains("result = adapter.step('withdraw', {'amount': 500})"),
        "forbidden step call missing from generated pytest:\n{content}"
    );
    assert!(
        content.contains("_assert_rejected(result, 'bad_call')"),
        "an out-of-type forbidden step must be asserted as bad_call:\n{content}"
    );
    assert!(
        !content.contains("'requires_failed'"),
        "an out-of-type forbidden step must not be asserted as a guard refusal:\n{content}"
    );
}

/// Negative control for the classification: a well-typed call the guard
/// refuses (60 > the remaining balance 40) stays `requires_failed`.
#[test]
fn a_guard_refused_final_step_stays_requires_failed() {
    let content = pytest_for("withdraw(60)", "guard-refused");
    assert!(
        content.contains("_assert_rejected(result, 'requires_failed')"),
        "a guard-refused forbidden step must stay requires_failed:\n{content}"
    );
    assert!(!content.contains("'bad_call'"), "{content}");
}

/// An out-of-type final step is still a rejection: `check` stays `ok`.
#[test]
fn an_out_of_type_final_step_still_satisfies_the_forbidden() {
    let (output, status) = check("withdraw(500)", "check-out-of-type");
    assert_eq!(
        (output["result"].as_str(), status),
        (Some("ok"), 0),
        "{output:#}"
    );
}

#[test]
fn a_final_step_naming_no_callable_action_is_not_a_rejection() {
    for (step, message, name) in [
        (
            "withdraw(1, 2)",
            "arity mismatch for action 'withdraw' in forbidden",
            "arity",
        ),
        (
            "withdrew(1)",
            "unknown action 'withdrew' in forbidden",
            "unknown",
        ),
    ] {
        let (output, status) = check(step, name);
        assert_eq!(status, 2, "{step}: {output:#}");
        assert_eq!(output["result"], "error", "{step}: {output:#}");
        assert_eq!(output["kind"], "forbidden", "{step}: {output:#}");
        assert_eq!(output["id"], "FB-1", "{step}: {output:#}");
        assert_eq!(output["failed_step"], 1, "{step}: {output:#}");
        assert_eq!(output["message"], message, "{step}: {output:#}");
        assert_eq!(output["trace_type"], "forbidden", "{step}: {output:#}");
        // This forbidden error carries `failed_step`, so it is located at that
        // step, not at the `forbidden` declaration.
        assert_eq!(
            output["loc"],
            serde_json::json!({"line": 12, "column": 5}),
            "{step}: {output:#}"
        );
    }
}

fn diff(old: &str, new: &str, name: &str) -> (Value, i32) {
    diff_with(old, new, name, &[])
}

fn diff_with(old: &str, new: &str, name: &str, options: &[&str]) -> (Value, i32) {
    let directory = scratch(name);
    let old_path = directory.join("old.fsl");
    let new_path = directory.join("new.fsl");
    std::fs::write(&old_path, old).expect("write OLD fixture");
    std::fs::write(&new_path, new).expect("write NEW fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("diff")
        .arg(&old_path)
        .arg(&new_path)
        .args(["--depth", "2"])
        .args(options)
        .output()
        .expect("run native diff");
    let _ = std::fs::remove_dir_all(&directory);
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn forbidden_findings(output: &Value) -> Vec<Value> {
    output["findings"]
        .as_array()
        .expect("diff findings")
        .iter()
        .filter(|finding| finding["id"] == "FB-1")
        .cloned()
        .collect()
}

/// OLD and NEW both reject the final step as `bad_call` outside the declared
/// type `Amount = 0..100`: the forbidden is preserved, not `unknown` /
/// `forbidden_step_unrelatable`. A `bad_call` decided by an `entity` / `number`
/// verify scope is not preserved (the tests below).
#[test]
fn diff_preserves_a_forbidden_both_sides_reject_as_bad_call_outside_a_declared_type() {
    let spec = wallet("withdraw(500)");
    let (output, status) = diff(&spec, &spec, "diff-both-bad-call");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(
        forbidden_findings(&output),
        Vec::<Value>::new(),
        "{output:#}"
    );
    assert_eq!(
        output["summary"],
        serde_json::json!(["no_semantic_change"]),
        "{output:#}"
    );
}

/// Negative control: NEW widens the parameter type and drops the guard, so
/// the call OLD rejected as `bad_call` is accepted.
#[test]
fn diff_relaxes_a_bad_call_forbidden_that_new_accepts() {
    let old = wallet("withdraw(500)");
    let new = old
        .replace("type Amount = 0..100", "type Amount = 0..1000")
        .replace(
            "requires amount <= balance  balance = balance - amount",
            "balance = balance",
        );
    let (output, status) = diff(&old, &new, "diff-bad-call-relaxed");
    assert_eq!(status, 0, "{output:#}");
    let findings = forbidden_findings(&output);
    assert_eq!(findings.len(), 1, "{output:#}");
    assert_eq!(findings[0]["kind"], "forbidden_relaxed", "{output:#}");
}

/// A requirements spec over `entity Case` (`instances Case = {instances}`).
fn cases(params: &str, guard: &str, final_step: &str, instances: u32) -> String {
    format!(
        r#"requirements Respond {{
  entity Case
  type Count = 0..5
  enum St {{ Waiting, Accepted, Responded }}
  state {{ cases: Map<Case, St> }}
  init {{ forall c: Case {{ cases[c] = Waiting }} }}
  requirement REQ-1 "respond" {{
    action respond({params}) {{ {guard} cases[c] = Responded }}
  }}
  forbidden FB-1 "cannot respond before accepting" {{
    {final_step}
    expect rejected
  }}
}}
verify {{ instances Case = {instances} }}"#
    )
}

fn assert_single_unknown(output: &Value, reason: &str) {
    let findings = forbidden_findings(output);
    assert_eq!(findings.len(), 1, "{output:#}");
    assert_eq!(findings[0]["kind"], "unknown", "{output:#}");
    assert_eq!(findings[0]["reason"], reason, "{output:#}");
}

/// An `entity` argument outside `instances` is decided by the verify scope,
/// not by a type: no guard was tried on either side, so the forbidden is not
/// preserved (C1, review r1 major-1). Since #1229 OLD's `check` rejects that
/// final step, so it is no OLD rejection at all (`forbidden_replay_failed`).
#[test]
fn diff_does_not_preserve_a_bad_call_decided_by_the_entity_scope() {
    let spec = cases("c: Case", "requires cases[c] == Accepted", "respond(7)", 3);
    let (output, status) = diff(&spec, &spec, "diff-entity-scope");
    assert_eq!(status, 0, "{output:#}");
    assert_single_unknown(&output, "forbidden_replay_failed");
}

/// OLD rejects `respond(2)` by its guard under `instances Case = 3`; NEW
/// shrinks the scope to 2 and drops the guard. OLD is replayed under NEW's
/// scope, so neither side tries a guard: the replayed OLD final step is
/// outside the verify scope (#1229).
#[test]
fn diff_does_not_preserve_a_bad_call_the_scope_change_introduced() {
    let old = cases("c: Case", "requires cases[c] == Accepted", "respond(2)", 3);
    let new = cases("c: Case", "", "respond(2)", 2);
    let (output, status) = diff(&old, &new, "diff-entity-scope-change");
    assert_eq!(status, 0, "{output:#}");
    assert_single_unknown(&output, "forbidden_replay_failed");
}

/// A `number` argument outside `values` is likewise decided by the scope.
#[test]
fn diff_does_not_preserve_a_bad_call_decided_by_the_number_scope() {
    let spec = r#"requirements Cart {
  number Qty
  state { total: 0..10 }
  init { total = 0 }
  requirement REQ-1 "add" {
    action add(q: Qty) { requires total + q <= 3  total = total + q }
  }
  forbidden FB-1 "the total never exceeds 3" {
    add(9)
    expect rejected
  }
}
verify { values Qty = 0..3 }"#;
    let (output, status) = diff(spec, spec, "diff-number-scope");
    assert_eq!(status, 0, "{output:#}");
    assert_single_unknown(&output, "forbidden_replay_failed");
}

/// Boundary: an action with an `entity` and a range parameter. An argument
/// outside the range type is a type-contract `bad_call` even when the entity
/// argument is in scope (preserved); an argument outside only the entity
/// scope is not.
#[test]
fn diff_preserves_only_the_type_contract_part_of_a_mixed_bad_call() {
    let preserved = cases(
        "c: Case, n: Count",
        "requires cases[c] == Accepted",
        "respond(1, 9)",
        3,
    );
    let (output, status) = diff(&preserved, &preserved, "diff-mixed-range");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(
        forbidden_findings(&output),
        Vec::<Value>::new(),
        "{output:#}"
    );
    let scoped = cases(
        "c: Case, n: Count",
        "requires cases[c] == Accepted",
        "respond(7, 1)",
        3,
    );
    let (output, status) = diff(&scoped, &scoped, "diff-mixed-entity");
    assert_eq!(status, 0, "{output:#}");
    assert_single_unknown(&output, "forbidden_replay_failed");
}

/// OLD's last step names no action. `check` rejects that case, so `diff`
/// must not read it as an OLD rejection that a NEW guard preserves (C2,
/// review r1 minor-4).
#[test]
fn diff_does_not_read_an_unknown_old_final_action_as_a_rejection() {
    let old = wallet("withdrew(60)");
    let new = old.replace(
        "  forbidden FB-1",
        "  requirement REQ-2 \"typo twin\" {\n    action withdrew(amount: Amount) { requires amount <= balance  balance = balance - amount }\n  }\n\n  forbidden FB-1",
    );
    let (output, status) = diff(&old, &new, "diff-old-unknown-action");
    assert_eq!(status, 0, "{output:#}");
    assert_single_unknown(&output, "forbidden_replay_failed");
    // NEW gives `withdraw` the arity OLD's last step uses, and refuses it.
    let old = wallet("withdraw(1, 2)").replace("    withdraw(10)\n", "");
    let new = old.replace(
        "action withdraw(amount: Amount) { requires amount <= balance ",
        "action withdraw(amount: Amount, fee: Amount) { requires amount + fee <= balance  requires fee == 0 ",
    );
    assert_ne!(old, new);
    let (output, status) = diff(&old, &new, "diff-old-arity");
    assert_eq!(status, 0, "{output:#}");
    assert_single_unknown(&output, "forbidden_replay_failed");
}

/// `cases` with `type Case = 0..2` instead of `entity Case` and no verify
/// scope: the same values, but `respond(7)` is outside a declared type.
fn typed_cases() -> String {
    cases("c: Case", "requires cases[c] == Accepted", "respond(7)", 3)
        .replace("entity Case", "type Case = 0..2")
        .replace("\nverify { instances Case = 3 }", "")
}

const KERNEL_CASES: &str = r"spec Respond {
  type Case = 0..2
  type Count = 0..5
  enum St { Waiting, Accepted, Responded }
  state { cases: Map<Case, St> }
  init { forall c: Case { cases[c] = Waiting } }
  action respond(c: Case) { requires cases[c] == Accepted  cases[c] = Responded }
}";

/// A type is scope-bound when either side declares it as an `entity` /
/// `number` scope. Only one side declares it here, so classifying OLD with
/// NEW's types (first pair) or NEW with OLD's (second pair) would preserve a
/// `bad_call` that one side decided by its verify scope. Since #1229 OLD's own
/// scope makes the first OLD final step no rejection at all.
#[test]
fn diff_treats_a_type_either_side_scopes_as_scope_bound() {
    let entity = cases("c: Case", "requires cases[c] == Accepted", "respond(7)", 3);
    let typed = typed_cases();
    for (old, new, name, reason) in [
        (
            entity.as_str(),
            KERNEL_CASES,
            "diff-union-old-entity",
            "forbidden_replay_failed",
        ),
        (
            typed.as_str(),
            entity.as_str(),
            "diff-union-new-entity",
            "forbidden_step_unrelatable",
        ),
    ] {
        let (output, status) = diff(old, new, name);
        assert_eq!(status, 0, "{name}: {output:#}");
        assert_single_unknown(&output, reason);
    }
    // Controls: with `type Case = 0..2` on both sides the same step is a
    // type-contract `bad_call`, which is preserved.
    for (new, name) in [
        (typed.as_str(), "diff-union-typed"),
        (KERNEL_CASES, "diff-union-typed-kernel"),
    ] {
        let (output, status) = diff(&typed, new, name);
        assert_eq!(status, 0, "{name}: {output:#}");
        assert_eq!(
            forbidden_findings(&output),
            Vec::<Value>::new(),
            "{name}: {output:#}"
        );
    }
}

/// `--forbid unknown` turns these verdicts into the exit status. A
/// type-contract `bad_call` on both sides passes the gate; before #1212 it was
/// `unknown` and this exit was 1. The OLD final step that names no action
/// fails the gate, but that exit is 1 before #1212 too: NEW must declare
/// `withdrew` to refuse it, and that is itself an `unknown` finding
/// (`state_or_action_names_differ`). That half is a preservation control for
/// the exit; `diff_does_not_read_an_unknown_old_final_action_as_a_rejection`
/// is the detector for the verdict.
#[test]
fn diff_forbid_unknown_follows_the_bad_call_and_unknown_action_rules() {
    let spec = wallet("withdraw(500)");
    let (output, status) = diff_with(&spec, &spec, "forbid-bad-call", &["--forbid", "unknown"]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["gate"]["passed"], true, "{output:#}");

    let old = wallet("withdrew(60)");
    let new = old.replace(
        "  forbidden FB-1",
        "  requirement REQ-2 \"typo twin\" {\n    action withdrew(amount: Amount) { requires amount <= balance  balance = balance - amount }\n  }\n\n  forbidden FB-1",
    );
    let (output, status) = diff_with(
        &old,
        &new,
        "forbid-unknown-action",
        &["--forbid", "unknown"],
    );
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(
        output["gate"]["violations"],
        serde_json::json!(["unknown"]),
        "{output:#}"
    );
    // Control: the gate fails only on the kinds it names.
    let (output, status) = diff_with(
        &old,
        &new,
        "forbid-unknown-action-relaxed",
        &["--forbid", "forbidden_relaxed"],
    );
    assert_eq!(status, 0, "{output:#}");
}

fn run_in(directory: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(directory)
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// Migration: on a final step that names no action, `verify` (BMC at
/// `--depth 3`, and `--engine induction`), `counterexample export`,
/// `scenarios`, `testgen`, `mutate`, `sweep`, and the `[requirements]` layer
/// of `chain` stop with the `kind: "forbidden"` error, and `html` and `ledger`
/// exit 2 while still writing their report. `check` is pinned by
/// `a_final_step_naming_no_callable_action_is_not_a_rejection`; `explain` and
/// `approval` are not run here. On this Wallet, which verifies, `verify`, `mutate`,
/// `sweep`, `chain`, `html` and `ledger` used to exit 0; `scenarios` /
/// `testgen` already stopped with exit 2 (`kind: "semantics"`), and
/// `counterexample export` with exit 2 because verification succeeded.
#[test]
fn an_unknown_final_action_exits_2_in_verify_at_depth_3_and_induction_and_eight_listed_commands() {
    let directory = scratch("gate-commands");
    write_spec(&directory, "withdrew(60)");
    for args in [
        &["verify", "wallet.fsl", "--depth", "3"][..],
        &["verify", "wallet.fsl", "--engine", "induction"],
        &[
            "counterexample",
            "export",
            "wallet.fsl",
            "--depth",
            "3",
            "-o",
            "cx.json",
        ],
        &["scenarios", "wallet.fsl", "--depth", "3"],
        &[
            "testgen",
            "wallet.fsl",
            "--depth",
            "3",
            "--target",
            "pytest",
            "-o",
            "t.py",
        ],
        &["mutate", "wallet.fsl", "--depth", "3"],
        &["sweep", "wallet.fsl", "--depth", "1..3"],
    ] {
        let (output, status) = run_in(&directory, args);
        assert_eq!(status, 2, "{args:?}: {output:#}");
        assert_eq!(output["kind"], "forbidden", "{args:?}: {output:#}");
        assert_eq!(
            output["message"], "unknown action 'withdrew' in forbidden",
            "{args:?}: {output:#}"
        );
        assert!(output.get("scenarios").is_none(), "{args:?}: {output:#}");
    }
    for artifact in ["cx.json", "t.py"] {
        assert!(!directory.join(artifact).exists(), "{artifact} written");
    }
    std::fs::write(
        directory.join("fsl-project.toml"),
        "[requirements]\nfile = \"wallet.fsl\"\ndepth = 3\n",
    )
    .expect("write project");
    let (output, status) = run_in(&directory, &["chain", "fsl-project.toml"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(
        output["failed"],
        serde_json::json!(["requirements"]),
        "{output:#}"
    );
    assert_eq!(
        output["layers"][0]["detail"]["kind"], "forbidden",
        "{output:#}"
    );
    // The generated reports still write their artifact, but fold the gate's
    // verdict into the exit status.
    for (args, artifact) in [
        (
            &["html", "wallet.fsl", "--depth", "3", "-o", "report.html"][..],
            "report.html",
        ),
        (
            &["ledger", "wallet.fsl", "--depth", "3", "-o", "ledger.md"],
            "ledger.md",
        ),
    ] {
        let (output, status) = run_in(&directory, args);
        assert_eq!(status, 2, "{args:?}: {output:#}");
        assert!(directory.join(artifact).exists(), "{artifact} not written");
    }
    let _ = std::fs::remove_dir_all(&directory);
}

fn ledger_entry(final_step: &str, name: &str) -> (String, i32) {
    let directory = scratch(name);
    write_spec(&directory, final_step);
    let (_, status) = run_in(
        &directory,
        &["ledger", "wallet.fsl", "--depth", "3", "-o", "ledger.md"],
    );
    let content = std::fs::read_to_string(directory.join("ledger.md")).expect("read ledger");
    let _ = std::fs::remove_dir_all(&directory);
    let entry = content
        .split("### FB-1")
        .nth(1)
        .unwrap_or_else(|| panic!("no FB-1 finding in ledger:\n{content}"))
        .split("```")
        .next()
        .unwrap_or_default()
        .to_owned();
    (entry, status)
}

/// `fslc ledger` summarized every `trace_type: forbidden` error as an accepted
/// final step and advised adding a guard (C5, review r1 minor-5).
#[test]
fn ledger_distinguishes_an_unknown_final_action_from_an_accepted_one() {
    let (entry, status) = ledger_entry("withdrew(60)", "ledger-unknown");
    assert_eq!(status, 2, "{entry}");
    assert!(
        entry.contains("unknown action 'withdrew' in forbidden"),
        "{entry}"
    );
    assert!(!entry.contains("accepted_trace"), "{entry}");
    assert!(!entry.contains("許容"), "{entry}");
    assert!(!entry.contains("ガードを追加"), "{entry}");

    let (entry, status) = ledger_entry("withdraw(500)\n    withdraw(1)", "ledger-setup");
    assert_eq!(status, 2, "{entry}");
    assert!(entry.contains("前提"), "{entry}");
    assert!(!entry.contains("accepted_trace"), "{entry}");
    assert!(!entry.contains("許容"), "{entry}");

    // Control: an accepted final step keeps the accepted-step summary.
    let (entry, status) = ledger_entry("withdraw(30)", "ledger-accepted");
    assert_eq!(status, 2, "{entry}");
    assert!(
        entry.contains("禁止フローが仕様上許容されている（accepted_trace あり）"),
        "{entry}"
    );
}
