// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1229: a `forbidden` last step whose argument lies outside the
//! `verify { instances / values }` scope of an `entity` / `process` / `number`
//! parameter satisfied the forbidden without evaluating any guard (as
//! `requires_failed` before #1212, as `bad_call` since), although an
//! implementation, which has no such bound, may accept the call.
//! It is now a `kind: "forbidden"` error (exit 2). A value outside a declared
//! range type stays `bad_call` (#1212), a guard refusal inside the scope stays
//! `requires_failed`, and a value only the `--instances` / `--values` override
//! removed stays `forbidden_skipped` (#1218).
//!
//! Every detector names the mutation it kills; the main one is restoring the
//! #1212 classification, which treats the verify scope as the parameter type.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("fslc-issue-1229-{name}-{}", std::process::id()));
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

/// `respond` has no guard when `guard` is empty, so a forbidden on it is
/// broken unless the call is rejected for another reason.
fn cases(guard: &str, final_step: &str) -> String {
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
    action respond(c: Case) {{ {guard} cases[c] = Responded }}
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

fn cart(final_step: &str) -> String {
    format!(
        r#"requirements Cart {{
  number Qty
  state {{ total: 0..20 }}
  init {{ total = 0 }}
  requirement REQ-1 "add" {{
    action add(q: Qty) {{ requires q > 0  requires total + q <= 20  total = total + q }}
  }}
  forbidden FB-1 "the add is rejected" {{
    {final_step}
    expect rejected
  }}
}}
verify {{ values Qty = 0..3 }}
"#
    )
}

fn assert_outside_scope(output: &Value, status: i32, value: i64, ty: &str, scope: [i64; 2]) {
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "forbidden", "{output:#}");
    assert_eq!(output["trace_type"], "forbidden", "{output:#}");
    assert_eq!(output["id"], "FB-1", "{output:#}");
    assert_eq!(output["failed_step"], 0, "{output:#}");
    assert_eq!(
        output["out_of_scope_argument"]["value"], value,
        "{output:#}"
    );
    assert_eq!(output["out_of_scope_argument"]["type"], ty, "{output:#}");
    assert_eq!(
        output["out_of_scope_argument"]["scope"],
        json!(scope),
        "{output:#}"
    );
    let message = output["message"].as_str().expect("message");
    assert!(message.contains("outside the verify scope"), "{message}");
    let hint = output["hint"].as_str().expect("hint");
    assert!(
        hint.contains("Widen the scope") && hint.contains("change the step"),
        "{hint}"
    );
    assert!(output.get("accepted_trace").is_none(), "{output:#}");
}

/// Issue reproduction 1 — detector (mutation: classify an `entity` argument
/// outside `instances` as `bad_call`). `respond` has no guard, yet
/// `respond(7)` under `instances Case = 3` used to pass `check` and `verify`.
#[test]
fn an_entity_final_step_outside_the_instances_scope_is_not_a_rejection() {
    let source = cases("", "respond(7)");
    let (output, status) = run_spec(&source, "entity-check", &["check"]);
    assert_outside_scope(&output, status, 7, "Case", [0, 2]);
    assert_eq!(output["out_of_scope_argument"]["parameter"], "c");
    // An error about one step is located at that step.
    assert_eq!(
        output["loc"],
        json!({"line": 13, "column": 5}),
        "{output:#}"
    );
    let (verified, status) = run_spec(
        &source,
        "entity-verify",
        &["verify", "--depth", "3", "--no-cache"],
    );
    assert_outside_scope(&verified, status, 7, "Case", [0, 2]);
}

/// Issue reproduction 2 — detector (same mutation, for a `number`). The
/// guard `q > 0` admits 9, so `scenarios` used to emit a `rejected_by`
/// scenario asking the implementation to reject a call it may accept.
#[test]
fn a_number_final_step_outside_the_values_scope_is_not_a_rejection() {
    let source = cart("add(9)");
    let (output, status) = run_spec(&source, "number-check", &["check"]);
    assert_outside_scope(&output, status, 9, "Qty", [0, 3]);
    let (scenarios, status) = run_spec(&source, "number-scenarios", &["scenarios", "--depth", "2"]);
    assert_outside_scope(&scenarios, status, 9, "Qty", [0, 3]);
    assert!(
        !scenarios.to_string().contains("rejected_by"),
        "{scenarios:#}"
    );
}

/// Migration: `verify` (BMC, induction, explicit, and auto), `sweep`,
/// `counterexample export`,
/// `testgen`, `mutate`, `html`, `ledger`, and the `[requirements]` layer of
/// `chain` stop with exit 2 on this spec, which verifies, so each exited 0
/// before #1229 except `counterexample export` (exit 2, nothing to export) —
/// detector (mutation: classify the scope error only on the `check` path).
#[test]
fn the_gate_stops_verify_and_the_commands_built_on_it_on_an_out_of_scope_final_step() {
    let directory = scratch("gate-commands");
    std::fs::write(directory.join("spec.fsl"), cart("add(9)")).expect("write spec");
    for args in [
        &["verify", "spec.fsl", "--depth", "2"][..],
        &["verify", "spec.fsl", "--engine", "induction"],
        &["verify", "spec.fsl", "--depth", "2", "--engine", "explicit"],
        &["verify", "spec.fsl", "--depth", "2", "--engine", "auto"],
        &["sweep", "spec.fsl", "--depth", "1..2"],
        &[
            "counterexample",
            "export",
            "spec.fsl",
            "--depth",
            "2",
            "-o",
            "cx.json",
        ],
        &[
            "testgen", "spec.fsl", "--depth", "2", "--target", "pytest", "-o", "t.py",
        ],
        &["mutate", "spec.fsl", "--depth", "2"],
    ] {
        let (output, status) = run(&directory, args);
        assert_outside_scope(&output, status, 9, "Qty", [0, 3]);
    }
    assert!(!directory.join("cx.json").exists());
    assert!(!directory.join("t.py").exists());
    // The reports still write their artifact, but fold the gate's verdict
    // into the exit status.
    for args in [
        &["html", "spec.fsl", "--depth", "2", "-o", "report.html"][..],
        &["ledger", "spec.fsl", "--depth", "2", "-o", "ledger.md"],
    ] {
        let (output, status) = run(&directory, args);
        assert_eq!(status, 2, "{args:?}: {output:#}");
    }
    assert!(directory.join("report.html").exists());
    std::fs::write(
        directory.join("fsl-project.toml"),
        "[requirements]\nfile = \"spec.fsl\"\ndepth = 2\n",
    )
    .expect("write fsl-project.toml");
    let (output, status) = run(&directory, &["chain", "fsl-project.toml"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["failed"], json!(["requirements"]), "{output:#}");
    let detail = &output["layers"][0]["detail"];
    assert_outside_scope(detail, status, 9, "Qty", [0, 3]);
    let ledger = std::fs::read_to_string(directory.join("ledger.md")).expect("read ledger");
    let _ = std::fs::remove_dir_all(&directory);
    let entry = ledger
        .split("### FB-1")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap_or_else(|| panic!("no FB-1 finding in ledger:\n{ledger}"));
    // `fslc ledger` names the scope, not an accepted or unresolved step.
    assert!(entry.contains("検証の範囲の外"), "{entry}");
    assert!(!entry.contains("許容されている"), "{entry}");
    assert!(!entry.contains("action を指していない"), "{entry}");
}

/// Boundary — detector (mutation: test only the upper end of the scope).
/// A value below the scope (`-1`) is outside it too.
#[test]
fn a_final_step_below_the_scope_is_not_a_rejection() {
    let (output, status) = run_spec(&cases("", "respond(-1)"), "below", &["check"]);
    assert_outside_scope(&output, status, -1, "Case", [0, 2]);
    let (output, status) = run_spec(&cart("add(-1)"), "below-number", &["check"]);
    assert_outside_scope(&output, status, -1, "Qty", [0, 3]);
}

/// Preservation control: a guard refusal inside the scope, at its bounds,
/// still satisfies the forbidden as `requires_failed`.
#[test]
fn a_guard_refusal_inside_the_scope_stays_requires_failed() {
    for (source, name) in [
        (
            cases("requires cases[c] == Accepted", "respond(2)"),
            "entity-upper",
        ),
        (cart("add(0)"), "number-lower"),
    ] {
        let (output, status) = run_spec(&source, &format!("{name}-check"), &["check"]);
        assert_eq!(
            (output["result"].as_str(), status),
            (Some("ok"), 0),
            "{output:#}"
        );
        let (scenarios, status) = run_spec(
            &source,
            &format!("{name}-scenarios"),
            &["scenarios", "--depth", "2"],
        );
        assert_eq!(status, 0, "{scenarios:#}");
        let forbidden = scenarios["scenarios"]
            .as_array()
            .expect("scenarios")
            .iter()
            .find(|scenario| scenario["kind"] == "forbidden")
            .expect("forbidden scenario");
        assert_eq!(forbidden["rejected_by"], "requires_failed", "{forbidden:#}");
    }
}

fn mixed(final_step: &str) -> String {
    format!(
        r#"requirements Pay {{
  entity Case
  type Amount = 0..100
  type Small = 0..10
  state {{ paid: Map<Case, Amount> }}
  init {{ forall c: Case {{ paid[c] = 0 }} }}
  requirement REQ-1 "pay" {{
    action pay(c: Case, a: Amount) {{ paid[c] = a }}
    action refund(c: Case, a: Small) {{ paid[c] = 0 }}
  }}
  forbidden FB-1 "the call is rejected" {{
    {final_step}
    expect rejected
  }}
}}
verify {{ instances Case = 2 }}
"#
    )
}

/// Preservation control for #1212 and detector (mutation: classify by the
/// first out-of-domain argument only). An argument outside a declared range
/// type is `bad_call` even when another argument is also
/// outside the verify scope; only a call that leaves nothing but the scope is
/// an error.
#[test]
fn an_argument_outside_a_declared_range_type_stays_bad_call() {
    for (step, name) in [
        ("pay(1, 500)", "type"),
        ("pay(7, 500)", "type-and-scope"),
        ("refund(1, 50)", "small"),
        ("refund(7, 50)", "small-and-scope"),
    ] {
        let source = mixed(step);
        let (output, status) = run_spec(&source, &format!("{name}-check"), &["check"]);
        assert_eq!(
            (output["result"].as_str(), status),
            (Some("ok"), 0),
            "{step}: {output:#}"
        );
        let (scenarios, status) = run_spec(
            &source,
            &format!("{name}-scenarios"),
            &["scenarios", "--depth", "1"],
        );
        assert_eq!(status, 0, "{step}: {scenarios:#}");
        let forbidden = scenarios["scenarios"]
            .as_array()
            .expect("scenarios")
            .iter()
            .find(|scenario| scenario["kind"] == "forbidden")
            .expect("forbidden scenario");
        assert_eq!(
            forbidden["rejected_by"], "bad_call",
            "{step}: {forbidden:#}"
        );
    }
    let (output, status) = run_spec(&mixed("pay(7, 5)"), "scope-only", &["check"]);
    assert_outside_scope(&output, status, 7, "Case", [0, 1]);
}

/// #1218 agreement. An argument inside the declared scope that the override
/// removed stays `forbidden_skipped` / `not_evaluated` (preservation
/// control); one outside the declared scope is the unscoped exit 2 under an
/// override that does not widen the scope to include it — detector (mutation: also skip a final step the declared scope
/// excludes, which made `respond(7)` exit 0 under `Case=2`).
#[test]
fn override_skip_and_declared_scope_error_do_not_conflict() {
    let removed = cases("", "respond(2)");
    let (output, status) = run_spec(
        &removed,
        "removed",
        &[
            "verify",
            "--depth",
            "2",
            "--no-cache",
            "--instances",
            "Case=2",
        ],
    );
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(
        output["requirement_traces"]["skipped"],
        json!([{
            "kind": "forbidden",
            "id": "FB-1",
            "reference": "respond(2): argument 2 for 'c' is outside its domain",
        }]),
        "{output:#}"
    );
    assert_eq!(output["requirement_traces"]["result"], "not_evaluated");
    let warnings = output["warnings"].as_array().expect("warnings");
    assert!(
        warnings
            .iter()
            .any(|warning| warning["kind"] == "forbidden_skipped"),
        "{output:#}"
    );

    let outside = cases("", "respond(7)");
    for instances in ["Case=2", "Case=3"] {
        let (output, status) = run_spec(
            &outside,
            &format!("outside-{instances}"),
            &[
                "verify",
                "--depth",
                "2",
                "--no-cache",
                "--instances",
                instances,
            ],
        );
        assert_eq!(status, 2, "{instances}: {output:#}");
        assert_eq!(output["kind"], "forbidden", "{instances}: {output:#}");
        assert_eq!(output["out_of_scope_argument"]["value"], 7, "{output:#}");
        assert!(output.get("requirement_traces").is_none(), "{output:#}");
    }

    let removed_number = cart("add(3)");
    let (output, status) = run_spec(
        &removed_number,
        "removed-number",
        &[
            "verify",
            "--depth",
            "2",
            "--no-cache",
            "--values",
            "Qty=0..2",
        ],
    );
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["requirement_traces"]["result"], "not_evaluated");
    let (output, status) = run_spec(
        &cart("add(9)"),
        "outside-number",
        &[
            "verify",
            "--depth",
            "2",
            "--no-cache",
            "--values",
            "Qty=0..2",
        ],
    );
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["out_of_scope_argument"]["value"], 9, "{output:#}");
}

/// Preservation control: an override that widens the scope to include the
/// argument evaluates the guard, so the unguarded `respond(7)` is accepted.
#[test]
fn a_widening_override_evaluates_the_guard() {
    let (output, status) = run_spec(
        &cases("", "respond(7)"),
        "widened",
        &[
            "verify",
            "--depth",
            "2",
            "--no-cache",
            "--instances",
            "Case=8",
        ],
    );
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["kind"], "forbidden", "{output:#}");
    assert_eq!(output["accepted_step"], 0, "{output:#}");
    assert!(output.get("out_of_scope_argument").is_none(), "{output:#}");
}

fn diff(old: &str, new: &str, name: &str) -> (Value, i32) {
    diff_with(old, new, &[], name)
}

/// `fslc diff old.fsl new.fsl` with `extra` files beside them.
fn diff_with(old: &str, new: &str, extra: &[(&str, &str)], name: &str) -> (Value, i32) {
    let directory = scratch(name);
    std::fs::write(directory.join("old.fsl"), old).expect("write OLD");
    std::fs::write(directory.join("new.fsl"), new).expect("write NEW");
    for (file, source) in extra {
        std::fs::write(directory.join(file), source).expect("write extra fixture");
    }
    let outcome = run(&directory, &["diff", "old.fsl", "new.fsl", "--depth", "2"]);
    let _ = std::fs::remove_dir_all(&directory);
    outcome
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

/// detector (mutation: `fslc diff` keeps reading an OLD final step outside
/// the verify scope as a `bad_call` both sides preserve). OLD never
/// rejected the call, so the forbidden is `unknown`, not preserved.
#[test]
fn diff_does_not_preserve_an_old_final_step_outside_the_verify_scope() {
    let spec = cases("", "respond(7)");
    let (output, status) = diff(&spec, &spec, "diff-outside");
    assert_eq!(status, 0, "{output:#}");
    let findings = output["findings"]
        .as_array()
        .expect("diff findings")
        .iter()
        .filter(|finding| finding["id"] == "FB-1")
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1, "{output:#}");
    assert_eq!(findings[0]["kind"], "unknown", "{output:#}");
    assert_eq!(
        findings[0]["reason"], "forbidden_replay_failed",
        "{output:#}"
    );
    assert_eq!(findings[0]["step"], 0, "{output:#}");

    // Preservation control: OLD and NEW both reject `add(0)` by its guard.
    let spec = cart("add(0)");
    let (output, status) = diff(&spec, &spec, "diff-guarded");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(
        output["summary"],
        json!(["no_semantic_change"]),
        "{output:#}"
    );
}

/// OLD bounds `Amt` by a range type, so `add(9)` is a `bad_call`; the guard
/// `q > 0` admits 9.
const OLD_RANGE_CART: &str = r#"requirements Cart {
  type Amt = 0..3
  state { total: 0..20 }
  init { total = 0 }
  requirement REQ-1 "add" {
    action add(q: Amt) { requires q > 0  requires total + q <= 20  total = total + q }
  }
  forbidden FB-1 "a big add is rejected" {
    add(9)
    expect rejected
  }
}
"#;

/// NEW turns `Amt` into a `number` bounded only by its verify scope. `add(9)`
/// is then outside the scope, which no NEW guard rejects, so the OLD
/// `bad_call` is not preserved — detector (mutation: NEW classified without
/// its verify scope types), for a requirements NEW and for a compose NEW
/// whose component declares the `number` (mutation: take a compose NEW's
/// scope types from its own source only).
#[test]
fn diff_does_not_preserve_a_bad_call_that_new_only_puts_outside_a_verify_scope() {
    let requirements = r#"requirements Cart {
  number Amt
  state { total: 0..20 }
  init { total = 0 }
  requirement REQ-1 "add" {
    action add(q: Amt) { requires q > 0  requires total + q <= 20  total = total + q }
  }
}
verify { values Amt = 0..3 }
"#;
    let component = r"spec Comp {
  number Amt
  action add(q: Amt) { requires q > 0 }
}
verify { values Amt = 0..3 }
";
    let compose = r#"compose Sys {
  use Comp as c from "comp.fsl"
  state { total: 0..20 }
  init { total = 0 }
  action add(q: c.Amt) = c.add(q) { requires total + q <= 20  total = total + q }
  internal c.add
}
"#;
    for (new, extra, name) in [
        (requirements, &[][..], "diff-new-requirements"),
        (compose, &[("comp.fsl", component)][..], "diff-new-compose"),
    ] {
        let (output, status) = diff_with(OLD_RANGE_CART, new, extra, name);
        assert_eq!(status, 0, "{name}: {output:#}");
        let findings = forbidden_findings(&output);
        assert_eq!(findings.len(), 1, "{name}: {output:#}");
        assert_eq!(findings[0]["kind"], "unknown", "{name}: {output:#}");
        assert_eq!(
            findings[0]["reason"], "forbidden_step_unrelatable",
            "{name}: {output:#}"
        );
    }
    // Preservation control: NEW keeps the range type, so both sides reject
    // `add(9)` as `bad_call` and the forbidden is preserved.
    let (output, status) = diff(OLD_RANGE_CART, OLD_RANGE_CART, "diff-both-range");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(
        forbidden_findings(&output),
        Vec::<Value>::new(),
        "{output:#}"
    );
}

/// A requirements `process` is lowered as an entity bounded by `instances`,
/// so it is a verify scope too — detector (mutation: drop `process` names
/// from the scope types).
#[test]
fn a_process_final_step_outside_the_instances_scope_is_not_a_rejection() {
    let source = |final_step: &str| {
        format!(
            r#"requirements Claims {{
  process Claim {{
    stages Draft, Done
    initial Draft
    transition finish Draft -> Done by User
  }}
  forbidden FB-1 "the claim cannot finish" {{
    {final_step}
    expect rejected
  }}
}}
verify {{ instances Claim = 2 }}
"#
        )
    };
    let (output, status) = run_spec(&source("finish(5)"), "process-outside", &["check"]);
    assert_outside_scope(&output, status, 5, "Claim", [0, 1]);
    // Preservation control: the in-scope `finish(1)` is enabled from Draft,
    // so the forbidden is the accepted-step error, not a scope error.
    let (output, status) = run_spec(&source("finish(1)"), "process-inside", &["check"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["accepted_step"], 0, "{output:#}");
}
