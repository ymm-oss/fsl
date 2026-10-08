// SPDX-License-Identifier: Apache-2.0

//! Issue #1213: a bare `expect rejected` forbidden counted as satisfied when its
//! last step was *enabled* and then stopped with a runtime violation
//! (`invariant` / `trans` / `ensures` / `type_bound` / `partial_op`). The guard the
//! forbidden states was missing, yet the case passed. Now only a
//! not-enabled last step (`requires_failed` / `bad_call`) satisfies it; a
//! violation is a `kind:"forbidden"` error that names the violation.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("fslc-issue-1213-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

/// A wallet whose `withdraw` body and extra declarations vary per case; the
/// forbidden always withdraws 10 and then tries to overdraw with 60.
fn wallet(action_body: &str, extra: &str) -> String {
    format!(
        r#"requirements Wallet {{
  type Amount = 0..100
  state {{ balance: Int }}
  init {{ balance = 50 }}

  requirement REQ-1 "Withdraw within balance" {{
    action withdraw(amount: Amount) {{ {action_body} }}
  }}
{extra}
  forbidden FB-1 "An overdraft is rejected" {{
    withdraw(10)
    withdraw(60)
    expect rejected
  }}
}}"#
    )
}

fn run(args: &[&std::ffi::OsStr]) -> (Value, i32) {
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
    (value, output.status.code().expect("exit status"))
}

fn write(directory: &Path, name: &str, source: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, source).expect("write fixture");
    path
}

fn check(source: &str, name: &str) -> (Value, i32) {
    let directory = scratch(name);
    let spec = write(&directory, "wallet.fsl", source);
    let result = run(&["check".as_ref(), spec.as_os_str()]);
    let _ = std::fs::remove_dir_all(&directory);
    result
}

#[test]
fn a_final_step_that_stops_with_a_violation_does_not_satisfy_the_forbidden() {
    let cases = [
        (
            "invariant",
            "invariant",
            wallet(
                "balance = balance - amount",
                "  invariant NonNegative { balance >= 0 }\n",
            ),
            "NonNegative",
        ),
        (
            "ensures",
            "ensures",
            wallet("balance = balance - amount  ensures balance >= 0", ""),
            "withdraw",
        ),
        (
            "type_bound",
            "type_bound",
            wallet("balance = balance - amount", "")
                .replace("state { balance: Int }", "state { balance: 0..100 }"),
            "_bounds_balance",
        ),
        (
            "partial_op",
            "partial_op",
            wallet(
                "balance = balance - amount + (amount - 10) / (amount - 60)",
                "",
            ),
            "_partial_withdraw",
        ),
        (
            "trans",
            "trans",
            wallet(
                "balance = balance - amount",
                "  trans NoBigDrop { balance >= old(balance) - 50 }\n",
            ),
            "NoBigDrop",
        ),
    ];
    for (name, kind, source, violation_name) in cases {
        let (output, status) = check(&source, name);
        assert_eq!(status, 2, "{name}: {output:#}");
        assert_eq!(output["result"], "error", "{name}: {output:#}");
        assert_eq!(output["kind"], "forbidden", "{name}: {output:#}");
        assert_eq!(output["trace_type"], "forbidden", "{name}: {output:#}");
        assert_eq!(output["id"], "FB-1", "{name}: {output:#}");
        assert_eq!(output["failed_step"], 1, "{name}: {output:#}");
        assert_eq!(output["violation"]["kind"], kind, "{name}: {output:#}");
        assert_eq!(
            output["violation"]["name"], violation_name,
            "{name}: {output:#}"
        );
        assert_eq!(
            output["violation"]["action"], "withdraw",
            "{name}: {output:#}"
        );
        assert_eq!(
            output["violation"]["params"],
            serde_json::json!({"amount": 60}),
            "{name}: {output:#}"
        );
        assert_eq!(
            output["accepted_trace"],
            serde_json::json!([{"action": "withdraw", "params": {"amount": 10}}]),
            "{name}: {output:#}"
        );
        assert_eq!(output["state"]["balance"], 40, "{name}: {output:#}");
        // The violating step is reported in `violation`, not as a step result.
        assert_eq!(
            output["step_results"],
            serde_json::json!([]),
            "{name}: {output:#}"
        );
        // This forbidden error carries `failed_step`, so it is located at that
        // step, not at the `forbidden` declaration (#1212).
        let final_line = source
            .lines()
            .position(|line| line.trim() == "withdraw(60)")
            .expect("final step line")
            + 1;
        assert_eq!(
            output["loc"],
            serde_json::json!({"line": final_line, "column": 5}),
            "{name}: {output:#}"
        );
        // The hint separates the two remedies: reject the call, or (when the
        // call should be allowed) fix the violation and revise the forbidden.
        let hint = output["hint"].as_str().unwrap_or_default();
        assert!(
            hint.contains("If this call must be rejected, add a requires")
                && hint.contains("If the call should be allowed"),
            "{name}: {output:#}"
        );
        assert!(
            output.get("accepted_step").is_none(),
            "a violated step is not an accepted step: {output:#}"
        );
    }
}

/// The forbidden gate runs before BMC, so `verify` on a spec whose forbidden
/// last step violates stops at the gate (exit 2) instead of reporting the
/// invariant counterexample it reported before #1213 (`violated`, exit 1).
#[test]
fn verify_stops_at_the_forbidden_gate_before_bmc() {
    let directory = scratch("verify-gate");
    let spec = write(
        &directory,
        "wallet.fsl",
        &wallet(
            "balance = balance - amount",
            "  invariant NonNegative { balance >= 0 }\n",
        ),
    );
    let (output, status) = run(&[
        "verify".as_ref(),
        spec.as_os_str(),
        "--depth".as_ref(),
        "3".as_ref(),
    ]);
    let _ = std::fs::remove_dir_all(&directory);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "forbidden", "{output:#}");
    assert_eq!(output["violation"]["name"], "NonNegative", "{output:#}");
    assert!(output.get("trace").is_none(), "{output:#}");
}

/// Negative controls: a rejection still satisfies the forbidden.
#[test]
fn a_not_enabled_final_step_still_satisfies_the_forbidden() {
    for (name, source) in [
        (
            "guard",
            wallet("requires amount <= balance  balance = balance - amount", ""),
        ),
        (
            "guard-with-invariant",
            wallet(
                "requires amount <= balance  balance = balance - amount",
                "  invariant NonNegative { balance >= 0 }\n",
            ),
        ),
        (
            "bad-call",
            wallet("requires amount <= balance  balance = balance - amount", "")
                .replace("withdraw(60)", "withdraw(500)"),
        ),
    ] {
        let (output, status) = check(&source, name);
        assert_eq!(
            (output["result"].as_str(), status),
            (Some("ok"), 0),
            "{name}: {output:#}"
        );
    }
}

/// `fslc diff` replays OLD forbidden cases against NEW with the same rule: a NEW
/// final step that is enabled and then violates relaxes the OLD rejection. The
/// Monitor rolls that step back, so the witness `trace` ends at the last setup
/// step and its `state` is the state the final step started from.
#[test]
fn diff_reports_a_new_final_step_that_violates_as_forbidden_relaxed() {
    let directory = scratch("diff");
    let old = write(
        &directory,
        "old.fsl",
        &wallet(
            "requires amount <= balance  balance = balance - amount",
            "  invariant NonNegative { balance >= 0 }\n",
        ),
    );
    let new = write(
        &directory,
        "new.fsl",
        r"spec Wallet {
  type Amount = 0..100
  state { balance: Int }
  init { balance = 50 }
  action withdraw(amount: Amount) { balance = balance - amount }
  invariant NonNegative { balance >= 0 }
}",
    );
    let (output, _) = run(&[
        "diff".as_ref(),
        old.as_os_str(),
        new.as_os_str(),
        "--depth".as_ref(),
        "2".as_ref(),
    ]);
    let _ = std::fs::remove_dir_all(&directory);
    let relaxed = output["findings"]
        .as_array()
        .unwrap_or_else(|| panic!("findings missing: {output:#}"))
        .iter()
        .find(|finding| finding["kind"] == "forbidden_relaxed")
        .unwrap_or_else(|| panic!("forbidden_relaxed missing: {output:#}"));
    assert_eq!(relaxed["id"], "FB-1", "{output:#}");
    assert_eq!(
        relaxed["witness"]["violation"],
        serde_json::json!({"kind": "invariant", "name": "NonNegative"}),
        "{output:#}"
    );
    assert_eq!(
        relaxed["witness"]["accepted_step"], "withdraw",
        "{output:#}"
    );
    assert_eq!(
        relaxed["witness"]["trace"],
        serde_json::json!([
            {"step": 0, "state": {"balance": 50}},
            {
                "step": 1,
                "state": {"balance": 40},
                "action": {"name": "withdraw", "params": {"amount": 10}}
            }
        ]),
        "{output:#}"
    );
    assert_eq!(
        relaxed["witness"]["state"],
        serde_json::json!({"balance": 40}),
        "{output:#}"
    );
}

fn guarded_wallet() -> String {
    wallet("requires amount <= balance  balance = balance - amount", "")
}

fn diff_forbidden_findings(old: &str, new: &str, name: &str) -> (Vec<Value>, Value) {
    let directory = scratch(name);
    let old = write(&directory, "old.fsl", old);
    let new = write(&directory, "new.fsl", new);
    let (output, _) = run(&[
        "diff".as_ref(),
        old.as_os_str(),
        new.as_os_str(),
        "--depth".as_ref(),
        "2".as_ref(),
    ]);
    let _ = std::fs::remove_dir_all(&directory);
    let findings = output["findings"]
        .as_array()
        .unwrap_or_else(|| panic!("findings missing: {output:#}"))
        .iter()
        .filter(|finding| finding["id"] == "FB-1")
        .cloned()
        .collect();
    (findings, output)
}

fn assert_replay_failed(findings: &[Value], step: u64, output: &Value) {
    assert_eq!(findings.len(), 1, "{output:#}");
    let finding = &findings[0];
    assert_eq!(finding["kind"], "unknown", "{output:#}");
    assert_eq!(finding["subject"], "forbidden", "{output:#}");
    assert_eq!(finding["reason"], "forbidden_replay_failed", "{output:#}");
    assert_eq!(finding["step"], step, "{output:#}");
    assert_eq!(finding["action"], "withdraw", "{output:#}");
}

/// A NEW setup step that is enabled and then violates never reaches the final
/// step: the OLD rejection is neither preserved nor relaxed, so the diff
/// fails closed with `unknown` instead of silently preserving it.
#[test]
fn diff_reports_a_new_setup_step_that_violates_as_replay_failed() {
    let new = r"spec Wallet {
  type Amount = 0..100
  state { balance: Int }
  init { balance = 50 }
  action withdraw(amount: Amount) { requires amount <= balance  balance = balance - amount }
  invariant Floor { balance >= 45 }
}";
    let (findings, output) =
        diff_forbidden_findings(&guarded_wallet(), new, "diff-new-setup-violates");
    assert_replay_failed(&findings, 0, &output);
    assert!(
        findings[0]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("invariant 'Floor'")),
        "{output:#}"
    );
}

/// An OLD final step that is enabled and then violates was never a rejection
/// (#1213), so there is nothing for NEW to preserve or relax.
#[test]
fn diff_reports_an_old_final_step_that_violates_as_replay_failed() {
    let old = wallet(
        "balance = balance - amount",
        "  invariant NonNegative { balance >= 0 }\n",
    );
    let (findings, output) =
        diff_forbidden_findings(&old, &guarded_wallet(), "diff-old-final-violates");
    assert_replay_failed(&findings, 1, &output);
}

/// Migration: on these specs each of the three replay rules fails a
/// `diff --forbid` gate that passed before #1213, when the forbidden was
/// preserved (DESIGN-forbidden §2.1); the other kind's gate and the control
/// pass. `--depth 0` keeps refinement findings out of the gate.
#[test]
fn diff_forbid_exits_follow_the_violation_rules() {
    let guardless = wallet(
        "balance = balance - amount",
        "  invariant NonNegative { balance >= 0 }\n",
    );
    let guarded_with_invariant = wallet(
        "requires amount <= balance  balance = balance - amount",
        "  invariant NonNegative { balance >= 0 }\n",
    );
    let kernel_guardless = r"spec Wallet {
  type Amount = 0..100
  state { balance: Int }
  init { balance = 50 }
  action withdraw(amount: Amount) { balance = balance - amount }
  invariant NonNegative { balance >= 0 }
}";
    let kernel_setup_violates = r"spec Wallet {
  type Amount = 0..100
  state { balance: Int }
  init { balance = 50 }
  action withdraw(amount: Amount) { requires amount <= balance  balance = balance - amount }
  invariant Floor { balance >= 45 }
}";
    let guarded = guarded_wallet();
    // (name, OLD, NEW, exit with --forbid forbidden_relaxed, with --forbid unknown)
    let cases = [
        (
            "new-final",
            guarded_with_invariant.as_str(),
            kernel_guardless,
            1,
            0,
        ),
        ("new-setup", guarded.as_str(), kernel_setup_violates, 0, 1),
        ("old-final", guardless.as_str(), guarded.as_str(), 0, 1),
        ("control", guarded.as_str(), guarded.as_str(), 0, 0),
    ];
    for (name, old, new, relaxed_exit, unknown_exit) in cases {
        let directory = scratch(&format!("diff-forbid-{name}"));
        write(&directory, "old.fsl", old);
        write(&directory, "new.fsl", new);
        for (kind, expected) in [
            ("forbidden_relaxed", relaxed_exit),
            ("unknown", unknown_exit),
        ] {
            let (output, status) = run_in(
                &directory,
                &[
                    "diff", "old.fsl", "new.fsl", "--depth", "0", "--forbid", kind,
                ],
            );
            assert_eq!(status, expected, "{name} --forbid {kind}: {output:#}");
            let violations = if expected == 1 { vec![kind] } else { vec![] };
            assert_eq!(
                output["gate"]["violations"],
                serde_json::json!(violations),
                "{name} --forbid {kind}: {output:#}"
            );
        }
        let _ = std::fs::remove_dir_all(&directory);
    }
}

/// A guard that divides by `balance - {pole}`: undefined once `balance`
/// reaches `pole`, and otherwise refusing an overdraft.
fn wallet_with_partial_guard(pole: u32) -> String {
    wallet(
        &format!(
            "requires 100 / (balance - {pole}) > 0 and amount <= balance  balance = balance - amount"
        ),
        "",
    )
}

/// The last step's guard is undefined in the state the setup reaches
/// (division by zero once `balance` is 40). That is not a rejection: the
/// guard was never decided, so the forbidden must not be satisfied. Today it
/// is a replay `error` (#1191), which this test deliberately does not pin: a
/// #1191 fix may report it in any non-`ok` shape, but must not turn it into a
/// `requires_failed` that satisfies the forbidden. The control differs only in
/// the pole, so a failure here is this final step's.
#[test]
fn a_final_step_whose_guard_is_undefined_does_not_satisfy_the_forbidden() {
    // Control: with the pole away from the replayed balances the same guard
    // refuses the overdraft and the forbidden is satisfied.
    let (output, status) = check(&wallet_with_partial_guard(30), "partial-guard-control");
    assert_eq!(
        (output["result"].as_str(), status),
        (Some("ok"), 0),
        "{output:#}"
    );
    let source = wallet_with_partial_guard(40);
    let (output, status) = check(&source, "partial-guard");
    assert_ne!(status, 0, "{output:#}");
    assert_ne!(output["result"], "ok", "{output:#}");
    let directory = scratch("partial-guard-scenarios");
    let spec = write(&directory, "wallet.fsl", &source);
    let (scenarios, status) = run(&[
        "scenarios".as_ref(),
        spec.as_os_str(),
        "--depth".as_ref(),
        "2".as_ref(),
    ]);
    let _ = std::fs::remove_dir_all(&directory);
    assert_ne!(status, 0, "{scenarios:#}");
    assert!(
        !scenarios.to_string().contains("requires_failed"),
        "an undefined guard must not be asserted as a guard refusal: {scenarios:#}"
    );
}

/// `fslc diff` replays the same final step against NEW. A NEW guard that is
/// undefined there decided nothing, so the OLD rejection must not be
/// preserved; this must survive a #1191 fix that reads an undefined guard as
/// disabled.
#[test]
fn diff_does_not_preserve_a_forbidden_whose_new_guard_is_undefined() {
    let (findings, output) = diff_forbidden_findings(
        &guarded_wallet(),
        &wallet_with_partial_guard(40),
        "diff-new-guard-undefined",
    );
    assert_eq!(findings.len(), 1, "{output:#}");
    assert!(
        matches!(
            findings[0]["kind"].as_str(),
            Some("unknown" | "forbidden_relaxed")
        ),
        "{output:#}"
    );
    // Control: a defined NEW guard that refuses the call preserves it.
    let (findings, output) = diff_forbidden_findings(
        &guarded_wallet(),
        &wallet_with_partial_guard(30),
        "diff-new-guard-defined",
    );
    assert_eq!(findings, Vec::<Value>::new(), "{output:#}");
}

/// A guardless wallet whose overdraft needs both forbidden steps: no single
/// withdrawal (at most 30 from 50) breaks `NonNegative`, so BMC at depth 1
/// finds nothing and `verify --depth 1` was `verified` / exit 0 before #1213.
fn two_step_overdraft(action_body: &str) -> String {
    wallet(action_body, "  invariant NonNegative { balance >= 0 }\n")
        .replace("type Amount = 0..100", "type Amount = 0..30")
        .replace("withdraw(10)", "withdraw(30)")
        .replace("withdraw(60)", "withdraw(30)")
}

fn run_in(directory: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(directory)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// Migration: on the two-step overdraft, `verify` at `--depth` 1 and 2 and
/// with `--engine induction`, and six commands — `sweep --depth 1..2`,
/// `counterexample export`, `scenarios`, `testgen`, `html` (each at the depths
/// below), and the `[requirements]` layer of `chain` — exit 2, where they
/// exited 0 or 1 before #1213. `counterexample export` and `testgen` write no
/// file, the JSON of `verify`, `sweep`, `counterexample export`, `scenarios`,
/// and `testgen` is the `kind: "forbidden"` error and carries no `trace` or
/// `scenarios`, and `html` still writes its report. `ledger` exits 2 in
/// `ledger_reports_a_violating_final_step_as_a_violation` (a one-step spec);
/// the `mutate` baseline, the `explicit` and `auto` engines, `explain`, and
/// `approval` are not run in this file.
#[test]
fn a_violating_final_step_exits_2_in_verify_at_depth_1_2_and_induction_and_six_listed_commands() {
    let directory = scratch("migration");
    write(
        &directory,
        "wallet.fsl",
        &two_step_overdraft("balance = balance - amount"),
    );
    for args in [
        &["verify", "wallet.fsl", "--depth", "1"][..],
        &["verify", "wallet.fsl", "--depth", "2"],
        &["verify", "wallet.fsl", "--engine", "induction"],
        &["sweep", "wallet.fsl", "--depth", "1..2"],
        &[
            "counterexample",
            "export",
            "wallet.fsl",
            "--depth",
            "2",
            "-o",
            "cx.json",
        ],
        &["scenarios", "wallet.fsl", "--depth", "1"],
        &[
            "testgen",
            "wallet.fsl",
            "--depth",
            "1",
            "--target",
            "pytest",
            "-o",
            "test_wallet.py",
        ],
    ] {
        let (output, status) = run_in(&directory, args);
        assert_eq!(status, 2, "{args:?}: {output:#}");
        assert_eq!(output["kind"], "forbidden", "{args:?}: {output:#}");
        assert_eq!(
            output["violation"]["name"], "NonNegative",
            "{args:?}: {output:#}"
        );
        assert!(output.get("trace").is_none(), "{args:?}: {output:#}");
        assert!(output.get("scenarios").is_none(), "{args:?}: {output:#}");
    }
    assert!(
        !directory.join("cx.json").exists(),
        "no reproducer is exported past the gate"
    );
    assert!(
        !directory.join("test_wallet.py").exists(),
        "no test is generated past the gate"
    );
    for depth in ["1", "2"] {
        let _ = std::fs::remove_file(directory.join("report.html"));
        let args = ["html", "wallet.fsl", "--depth", depth, "-o", "report.html"];
        let (output, status) = run_in(&directory, &args);
        assert_eq!(status, 2, "{args:?}: {output:#}");
        assert!(
            directory.join("report.html").exists(),
            "{args:?}: report.html not written"
        );
    }
    std::fs::write(
        directory.join("fsl-project.toml"),
        "[requirements]\nfile = \"wallet.fsl\"\ndepth = 1\n",
    )
    .expect("write fsl-project.toml");
    let (output, status) = run_in(&directory, &["chain", "fsl-project.toml"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(
        output["failed"],
        serde_json::json!(["requirements"]),
        "{output:#}"
    );
    let detail = &output["layers"][0]["detail"];
    assert_eq!(detail["kind"], "forbidden", "{output:#}");
    assert_eq!(detail["violation"]["name"], "NonNegative", "{output:#}");
    let _ = std::fs::remove_dir_all(&directory);
}

/// Migration: a mutant that drops the guard leaves BMC at depth 1 clean, and
/// its forbidden last step now violates instead of being rejected, so it is
/// killed by the forbidden where it survived before #1213.
#[test]
fn mutate_kills_a_guard_dropping_mutant_with_the_forbidden() {
    let directory = scratch("mutate");
    write(
        &directory,
        "wallet.fsl",
        &two_step_overdraft("requires amount <= balance  balance = balance - amount"),
    );
    let (output, status) = run_in(&directory, &["mutate", "wallet.fsl", "--depth", "1"]);
    let _ = std::fs::remove_dir_all(&directory);
    assert_eq!(status, 0, "{output:#}");
    let mutants = output["mutants"]
        .as_array()
        .unwrap_or_else(|| panic!("mutants missing: {output:#}"));
    let dropped = mutants
        .iter()
        .find(|mutant| mutant["op"] == "requires_remove")
        .unwrap_or_else(|| panic!("no requires_remove mutant: {output:#}"));
    assert_eq!(dropped["status"], "killed", "{dropped:#}");
    assert_eq!(dropped["killed_by"], "forbidden", "{dropped:#}");
}

/// `fslc ledger` reports the violation error as a violation reached by an
/// enabled step, not as an accepted final step.
#[test]
fn ledger_reports_a_violating_final_step_as_a_violation() {
    let directory = scratch("ledger");
    write(
        &directory,
        "wallet.fsl",
        &wallet(
            "balance = balance - amount",
            "  invariant NonNegative { balance >= 0 }\n",
        ),
    );
    let (_, status) = run_in(
        &directory,
        &["ledger", "wallet.fsl", "--depth", "3", "-o", "ledger.md"],
    );
    let content = std::fs::read_to_string(directory.join("ledger.md")).expect("read ledger");
    let _ = std::fs::remove_dir_all(&directory);
    assert_eq!(status, 2, "{content}");
    let entry = content
        .split("### FB-1")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap_or_else(|| panic!("no FB-1 finding in ledger:\n{content}"));
    assert!(entry.contains("invariant 'NonNegative'"), "{entry}");
    assert!(entry.contains("実行時違反"), "{entry}");
    assert!(!entry.contains("accepted_trace あり"), "{entry}");
    assert!(!entry.contains("許容されている"), "{entry}");
}
