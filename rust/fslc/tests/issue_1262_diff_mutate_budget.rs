// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1262: `check_refinement`'s correspondence walk stops at
//! `fsl_runtime::IMPLEMENTS_SEARCH_BUDGET` (50,000 states). `fslc refine`,
//! `fslc chain` and governance read that cutoff through
//! `RefinementCheck::verdict()` (#1245), but `fslc diff` and the `fslc mutate`
//! implements oracle read the outcome fields themselves and missed it:
//!
//! - `fslc diff` reported the cut-off direction as `refines`, so a behaviour
//!   difference beyond the budget passed the gate as `no_semantic_change`
//!   (exit 0) even under `--forbid behavior_removed` -- a false green. The
//!   same path backs `fslc diff --git` and `fslc approval diff`.
//! - `fslc mutate` counted the cut-off mutant as `survived`.
//!
//! After the fix the cut-off direction is `unknown_budget`, the diff emits an
//! `unknown_budget` finding that fails the gate unconditionally (like
//! `impl_violated`, never opt-in via `--forbid`), and mutate reports the
//! mutant as `inconclusive`: outside `kill_rate`, and a failure of any
//! requested #1237 gate.
//!
//! The fixtures are written to scratch directories instead of the corpus so
//! the all-corpus `check` sweep does not pay for budget-scale domains (the
//! same reason as `refine_budget_unknown.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

const BUDGET: u64 = 50_000;

fn scratch(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1262-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn fslc(dir: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env("FSLC_CACHE_DIR", dir.join("cache"))
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run fslc");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let json = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "fslc {args:?} printed non-JSON ({error}): {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (json, output.status.code().expect("exit code"))
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The issue's reproduction: push-only `Seq<V, 7>`. NEW forbids exactly one
/// seventh push (`[5,5,5,5,5,5]` then `push(5)`). Over `0..5` the reachable
/// set passes 50,000 partway through step 6, so the walk is cut off before it
/// can see the step-7 difference; over `0..2` (3,280 states through depth 7)
/// it is not.
fn issue_pair(values: &str, last: u8) -> (String, String) {
    let old = format!(
        "spec Wide {{\n  type V = {values}\n  state {{ seq: Seq<V, 7> }}\n  \
         init {{ seq = Seq {{}} }}\n  action push(v: V) {{\n    \
         requires seq.size() < 7\n    seq = seq.push(v)\n  }}\n}}\n"
    );
    let guard = format!(
        "    requires not (seq.size() == 6 and v == {last} and seq.at(0) == {last} \
         and seq.at(1) == {last} and seq.at(2) == {last} and seq.at(3) == {last} \
         and seq.at(4) == {last} and seq.at(5) == {last})\n    seq = seq.push(v)"
    );
    let new = old.replace("    seq = seq.push(v)", &guard);
    assert_ne!(old, new, "fixture guard was not inserted");
    (old, new)
}

/// A cheaper cutoff for the `--git` / `approval diff` wiring: `Seq<V, 6>`
/// over six values has 55,987 reachable states, all by depth 6.
fn wide6(requirement: bool) -> String {
    let label = if requirement {
        " \"REQ-1262: push a value\""
    } else {
        ""
    };
    format!(
        "spec Wide {{\n  type V = 0..5\n  state {{ seq: Seq<V, 6> }}\n  \
         init {{ seq = Seq {{}} }}\n  action push(v: V){label} {{\n    \
         requires seq.size() < 6\n    seq = seq.push(v)\n  }}\n}}\n"
    )
}

fn assert_budget_direction(direction: &Value) {
    assert_eq!(direction["result"], "unknown_budget", "{direction}");
    assert_eq!(direction["states_explored"], BUDGET, "{direction}");
    assert!(
        direction.get("checked_to_depth").is_none(),
        "a cut-off walk did not check to the depth: {direction}"
    );
}

/// Asserts one single-comparison `semantic_diff` envelope reports the cutoff
/// in both directions and fails the gate whatever `--forbid` said.
fn assert_budget_diff(output: &Value) {
    assert_eq!(output["result"], "semantic_diff", "{output}");
    assert_budget_direction(&output["directions"]["old_to_new"]);
    assert_budget_direction(&output["directions"]["new_to_old"]);
    let summary = output["summary"].as_array().expect("summary array");
    assert!(
        summary.iter().any(|kind| kind == "unknown_budget"),
        "{output}"
    );
    assert!(
        !summary.iter().any(|kind| kind == "no_semantic_change"),
        "{output}"
    );
    let findings = output["findings"].as_array().expect("findings array");
    for direction in ["old_to_new", "new_to_old"] {
        let finding = findings
            .iter()
            .find(|finding| {
                finding["kind"] == "unknown_budget" && finding["direction"] == direction
            })
            .unwrap_or_else(|| panic!("no unknown_budget finding for {direction}: {output}"));
        assert_eq!(finding["states_explored"], BUDGET, "{finding}");
    }
    assert_eq!(output["gate"]["passed"], false, "{output}");
    assert!(
        output["gate"]["violations"]
            .as_array()
            .expect("violations array")
            .iter()
            .any(|kind| kind == "unknown_budget"),
        "{output}"
    );
}

/// Acceptance: the issue's reproduction fails. No `--forbid` at all: the
/// cutoff is an unconditional gate failure, like `impl_violated`.
#[test]
fn diff_reports_a_budget_cutoff_as_unknown_budget_and_fails_without_forbid() {
    let dir = scratch("diff-cutoff");
    let (old, new) = issue_pair("0..5", 5);
    fs::write(dir.join("old.fsl"), old).expect("write old");
    fs::write(dir.join("new.fsl"), new).expect("write new");

    let (output, code) = fslc(&dir, &["diff", "old.fsl", "new.fsl", "--depth", "7"]);

    assert_budget_diff(&output);
    assert_eq!(
        output["gate"]["forbidden"],
        serde_json::json!([]),
        "{output}"
    );
    assert_eq!(code, 1, "{output}");
}

/// Control on the same shape below the budget: the step-7 difference is
/// still found as `behavior_removed`, and no cutoff is reported.
#[test]
fn diff_below_the_budget_still_reports_behavior_removed() {
    let dir = scratch("diff-control");
    let (old, new) = issue_pair("0..2", 2);
    fs::write(dir.join("old.fsl"), old).expect("write old");
    fs::write(dir.join("new.fsl"), new).expect("write new");

    let (output, code) = fslc(
        &dir,
        &[
            "diff",
            "old.fsl",
            "new.fsl",
            "--depth",
            "7",
            "--forbid",
            "behavior_removed",
        ],
    );

    assert_eq!(
        output["directions"]["old_to_new"]["result"], "refinement_failed",
        "{output}"
    );
    assert_eq!(
        output["directions"]["old_to_new"]["violated_at_step"], 7,
        "{output}"
    );
    assert_eq!(
        output["directions"]["new_to_old"]["result"], "refines",
        "{output}"
    );
    assert_eq!(output["summary"], serde_json::json!(["behavior_removed"]));
    assert_eq!(
        output["gate"]["violations"],
        serde_json::json!(["behavior_removed"])
    );
    assert_eq!(code, 1, "{output}");
}

/// `fslc diff --git` (batch form) goes through the same comparison and must
/// carry the finding into its aggregated gate.
#[test]
fn diff_git_batch_reports_the_cutoff_and_fails_the_gate() {
    let dir = scratch("diff-git");
    fs::write(dir.join("spec.fsl"), wide6(false)).expect("write spec");
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.email", "issue-1262@example.com"]);
    git(&dir, &["config", "user.name", "Issue 1262"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    git(&dir, &["add", "spec.fsl"]);
    git(&dir, &["commit", "-qm", "base"]);
    fs::write(dir.join("spec.fsl"), format!("{}// edited\n", wide6(false))).expect("edit spec");
    git(&dir, &["commit", "-qam", "head"]);

    let (output, code) = fslc(&dir, &["diff", "--git", "HEAD~1..HEAD", "--depth", "6"]);

    assert_eq!(output["result"], "semantic_diff_batch", "{output}");
    let comparisons = output["comparisons"].as_array().expect("comparisons");
    assert_eq!(comparisons.len(), 1, "{output}");
    assert_budget_diff(&comparisons[0]);
    assert_eq!(output["gate"]["passed"], false, "{output}");
    assert_eq!(code, 1, "{output}");
}

/// `fslc approval diff` runs the same comparison with no `--forbid`; the
/// cutoff makes it exit 1 (decision recorded in DESIGN-semantic-diff.md).
#[test]
fn approval_diff_reports_the_cutoff_and_exits_1() {
    let dir = scratch("approval-diff");
    fs::write(dir.join("spec.fsl"), wide6(true)).expect("write spec");
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.email", "issue-1262@example.com"]);
    git(&dir, &["config", "user.name", "Issue 1262"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    git(&dir, &["add", "spec.fsl"]);
    git(&dir, &["commit", "-qm", "approval baseline"]);
    let (ledger, code) = fslc(
        &dir,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "ledger.json"],
    );
    assert_eq!(code, 0, "{ledger}");
    let (record, code) = fslc(
        &dir,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "ledger.json",
            "--approver",
            "tester",
            "--depth",
            "2",
            "-o",
            "record.json",
        ],
    );
    assert_eq!(code, 0, "{record}");
    fs::write(dir.join("spec.fsl"), format!("{}// edited\n", wide6(true))).expect("edit spec");

    let (output, code) = fslc(
        &dir,
        &[
            "approval",
            "diff",
            "spec.fsl",
            "--record",
            "record.json",
            "--depth",
            "6",
        ],
    );

    assert_budget_diff(&output);
    assert!(output.get("approval").is_some(), "{output}");
    assert_eq!(code, 1, "{output}");
}

/// The requirements layer narrows `v` to `0..1` (127 reachable states, well
/// under the budget) while the abstraction allows `0..5` (55,987). The
/// builtin `requires_remove` mutant of `requires v <= 1`, and an external
/// mutant that widens it to `v <= 5`, both make the impl as wide as the
/// abstraction, so their correspondence walk is cut off.
const NARROW_ABS: &str = "spec WideAbs {\n  type V = 0..5\n  state { seq: Seq<V, 6> }\n  \
     init { seq = Seq {} }\n  action push(v: V) {\n    requires seq.size() < 6\n    \
     seq = seq.push(v)\n  }\n}\n";

const NARROW_REQ: &str = "requirements NarrowReq {\n  \
     implements WideAbs from \"abs.fsl\" { maps auto }\n\n  type V = 0..5\n  \
     state { seq: Seq<V, 6> }\n  init { seq = Seq {} }\n  \
     action push(v: V) maps push(v) {\n    requires v <= 1\n    \
     requires seq.size() < 6\n    seq = seq.push(v)\n  }\n}\n";

fn assert_inconclusive(mutant: &Value) {
    assert_eq!(mutant["status"], "inconclusive", "{mutant}");
    assert_eq!(mutant["killed_by"], Value::Null, "{mutant}");
    assert_eq!(
        mutant["inconclusive"]["reason"], "unknown_budget",
        "{mutant}"
    );
    assert_eq!(
        mutant["inconclusive"]["states_explored"], BUDGET,
        "{mutant}"
    );
}

/// Mutate: a cut-off mutant is neither `survived` nor `killed`. It is
/// `inconclusive`, outside `kill_rate`, counted in `summary.inconclusive`,
/// and it fails a requested gate even when the kill rate alone would pass.
#[test]
fn mutate_reports_a_budget_cutoff_as_inconclusive_and_fails_a_requested_gate() {
    let dir = scratch("mutate");
    fs::write(dir.join("abs.fsl"), NARROW_ABS).expect("write abs");
    fs::write(dir.join("req.fsl"), NARROW_REQ).expect("write req");
    fs::write(
        dir.join("external.jsonl"),
        concat!(
            r#"{"id":"widen","target":"requires v <= 1","replacement":"requires v <= 5"}"#,
            "\n",
            r#"{"id":"narrow","target":"requires v <= 1","replacement":"requires v <= 2"}"#,
            "\n",
        ),
    )
    .expect("write external mutants");

    let (output, code) = fslc(
        &dir,
        &[
            "mutate",
            "req.fsl",
            "--depth",
            "6",
            "--from",
            "external.jsonl",
            "--min-kill-rate",
            "0.1",
        ],
    );

    assert_eq!(output["result"], "mutated", "{output}");
    let mutants = output["mutants"].as_array().expect("mutants");
    let builtin = mutants
        .iter()
        .filter(|mutant| mutant["source"] == "builtin" && mutant["status"] == "inconclusive")
        .collect::<Vec<_>>();
    assert_eq!(builtin.len(), 1, "{output}");
    assert_eq!(builtin[0]["op"], "requires_remove", "{output}");
    assert_eq!(builtin[0]["target"], "push requires #1", "{output}");
    assert_inconclusive(builtin[0]);
    let external = |id: &str| {
        mutants
            .iter()
            .find(|mutant| mutant["id"] == id)
            .unwrap_or_else(|| panic!("no external mutant {id}: {output}"))
    };
    assert_inconclusive(external("widen"));
    // Control: a widening that stays under the budget is decided.
    assert_eq!(external("narrow")["status"], "survived", "{output}");

    let summary = &output["summary"];
    assert_eq!(summary["inconclusive"], 2, "{summary}");
    assert_eq!(
        summary["by_source"]["builtin"]["inconclusive"], 1,
        "{summary}"
    );
    assert_eq!(
        summary["by_source"]["external"]["inconclusive"], 1,
        "{summary}"
    );
    // `inconclusive` appears only when non-zero; the issue_848 golden pins
    // the envelope of a run without any cutoff.
    let count = |key: &str| summary[key].as_u64().unwrap_or(0);
    assert_eq!(
        count("total"),
        count("killed")
            + count("survived")
            + count("invalid")
            + count("errored")
            + count("inconclusive"),
        "{summary}"
    );
    // #1251: widening `V` below 0 makes the implements oracle return an
    // error (the mutated parameter domain is outside the abstraction's)
    // instead of a refinement verdict. That error used to be counted as a
    // `refinement` kill; it is an undecided `error` now, kept apart from the
    // budget cutoff and also failing the gate.
    let errors = mutants
        .iter()
        .filter(|mutant| mutant["status"] == "error")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1, "{output}");
    assert_eq!(errors[0]["op"], "type_bound_lo_minus1", "{output}");
    assert_eq!(errors[0]["error"]["stage"], "implements", "{output}");
    assert_eq!(count("errored"), 1, "{summary}");
    assert!(count("killed") > 0, "{summary}");
    #[allow(clippy::cast_precision_loss)]
    let expected_rate = count("killed") as f64 / (count("killed") + count("survived")) as f64;
    let rate = summary["kill_rate"].as_f64().expect("kill_rate");
    assert!((rate - expected_rate).abs() < 1e-4, "{summary}");
    assert!(rate >= 0.1, "the rate alone passes the gate: {summary}");

    let gate = &output["gate"];
    assert_eq!(gate["inconclusive"], 2, "{gate}");
    assert_eq!(gate["errored"], 1, "{gate}");
    assert_eq!(
        gate["violations"],
        serde_json::json!(["oracle_errors", "inconclusive"]),
        "{gate}"
    );
    assert_eq!(gate["passed"], false, "{gate}");
    assert_eq!(code, 1, "{output}");
}
