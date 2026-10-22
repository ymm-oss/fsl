// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S7, through the native CLI: the decision table
//! `decide ID "text" for Stage Entity { rows [otherwise => …] } [assuming Q]`
//! in a document headed `fsl 5`, behind the `fsl5` gate.
//!
//! K for the e2e example's table (`fsl5_en_decide.fsl` against
//! `lowered_decide.fsl`) lives in `fsl5_lowering_contract.rs`. This file
//! holds:
//!
//! - **Acceptance 1, R and M**: the table's source and the original refine
//!   each other once REQ-1's guard (the rule stage's) is taken out of the
//!   original; moving the boundary between the table's concepts by one makes
//!   K differ and R fail. Moving only one concept's boundary overlaps the
//!   rows instead.
//! - **Acceptance 2**: one failing document per error of the table, the
//!   values the check reports, and `otherwise` closing the gap (with the
//!   guard it gives its events), plus the empty-row warning.
//! - **Acceptance 3**: `assuming Q` closes a counterexample no reachable state
//!   has, and `verify` proves `Q` — or reports it violated where a state at
//!   the stage breaks it.
//! - **decisions/048 J3**: past the enumeration bound the same checks are
//!   asked of the solver.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "support/kernel_compare.rs"]
mod kernel_compare;

fn fixture_dir(example: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5")
        .join(example)
}

/// A scratch directory, optionally holding a copy of one fixture example.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, example: Option<&str>) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("fsl5_decide")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        if let Some(example) = example {
            for entry in std::fs::read_dir(fixture_dir(example)).expect("read example") {
                let path = entry.expect("example entry").path();
                std::fs::copy(&path, directory.join(path.file_name().expect("file name")))
                    .expect("copy example");
            }
        }
        Self(directory)
    }

    fn write(&self, name: &str, source: &str) -> String {
        std::fs::write(self.0.join(name), source).expect("write scratch source");
        name.to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `fslc <args>` in `directory` with the `fsl5` gate set.
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

fn replace_once(source: &str, from: &str, to: &str) -> String {
    assert_eq!(source.matches(from).count(), 1, "{from:?} occurs once");
    source.replacen(from, to, 1)
}

/// The e2e original without REQ-1's guard, the part of it the rule stage
/// lowers: the table's source has its table and not its rule.
fn original_without_req1(scratch: &Scratch) -> String {
    let original = std::fs::read_to_string(scratch.0.join("original.fsl")).expect("read original");
    scratch.write(
        "original_without_req1.fsl",
        &replace_once(&original, "      when a > 0\n", ""),
    )
}

fn decide_source() -> String {
    std::fs::read_to_string(fixture_dir("e2e").join("fsl5_en_decide.fsl"))
        .expect("read fsl5_en_decide.fsl")
}

fn refine(scratch: &Scratch, implementation: &str, abstraction: &str, mapping: &str) -> String {
    let (output, exit) = fslc(
        &scratch.0,
        &[
            "refine",
            implementation,
            abstraction,
            mapping,
            "--depth",
            "8",
        ],
    );
    format!("{} {exit}", output["result"].as_str().unwrap_or_default())
}

/// Acceptance 1, R: the decision table's source refines the original and
/// back (both without REQ-1, which the rule stage adds).
#[test]
fn r_the_decision_table_and_the_original_refine_each_other() {
    let scratch = Scratch::new("r", Some("e2e"));
    let original = original_without_req1(&scratch);
    assert_eq!(
        refine(
            &scratch,
            "fsl5_en_decide.fsl",
            &original,
            "refines_lowered_to_original.fsl"
        ),
        "refines 0"
    );
    assert_eq!(
        refine(
            &scratch,
            &original,
            "fsl5_en_decide.fsl",
            "refines_original_to_lowered.fsl"
        ),
        "refines 0"
    );
}

/// Acceptance 1, M: the boundary between `SmallClaim` and `LargeClaim` moved
/// by one (the verification.log mutation, made in the source) lowers to a
/// different kernel and no longer refines the original. Moving only
/// `SmallClaim`'s boundary makes the rows overlap at the moved value.
#[test]
fn m_a_boundary_moved_by_one_differs_in_k_and_fails_r() {
    let scratch = Scratch::new("m", Some("e2e"));
    let original = original_without_req1(&scratch);
    let small = (
        "where amount <= AUTO_LIMIT\n",
        "where amount <= AUTO_LIMIT + 1\n",
    );
    let large = (
        "where amount > AUTO_LIMIT\n",
        "where amount > AUTO_LIMIT + 1\n",
    );
    let moved = replace_once(
        &replace_once(&decide_source(), small.0, small.1),
        large.0,
        large.1,
    );
    let file = scratch.write("moved.fsl", &moved);
    assert!(
        kernel_compare::difference(
            &kernel_compare::kernel_v2_fsl5(&scratch.0.join(&file)),
            &kernel_compare::kernel_v2(&fixture_dir("e2e").join("lowered_decide.fsl")),
        )
        .is_some(),
        "K must notice the moved boundary"
    );
    assert_eq!(
        refine(
            &scratch,
            &file,
            &original,
            "refines_lowered_to_original.fsl"
        ),
        "refinement_failed 1"
    );

    let file = scratch.write(
        "small_only.fsl",
        &replace_once(&decide_source(), small.0, small.1),
    );
    let (output, exit) = fslc(&scratch.0, &["check", &file]);
    assert_eq!(
        (output["diagnostic_code"].as_str(), exit),
        (Some("decide_overlap"), 2),
        "{output:#}"
    );
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("both hold at amount = 2, AUTO_LIMIT = 1")),
        "{output:#}"
    );
}

/// A claim document whose table rows are `rows`, with `SmallClaim` read as
/// `amount <small> AUTO_LIMIT` and `values Amount = 0..<top>`.
fn table_document(small: &str, rows: &str, after: &str, top: u32) -> String {
    format!(
        "fsl 5\nrequirements R {{\n  number Amount\n  const AUTO_LIMIT = 1\n  \
         actor Employee, System, Manager\n  entity Claim {{\n    amount: Amount = 0\n    \
         lifecycle Draft -> Submitted -> Approved\n    lifecycle Submitted -> Rejected\n  }}\n  \
         concept SmallClaim = Claim where amount {small} AUTO_LIMIT\n  \
         concept LargeClaim = Claim where amount > AUTO_LIMIT\n  \
         concept Never = Claim where amount > 9\n  \
         event submit = Employee submits Claim with amount Draft -> Submitted\n  \
         event auto_approve = System approves Claim Submitted -> Approved\n  \
         event mgr_approve = Manager approves Claim Submitted -> Approved\n  \
         event reject = Manager rejects Claim Submitted -> Rejected\n  \
         decide T \"who handles a claim\" for Submitted Claim {{\n{rows}  }}\n{after}}}\n\
         verify {{\n  instances Claim = 1\n  values Amount = 0..{top}\n}}\n"
    )
}

const ROWS: &str = "    SmallClaim => System approves\n    \
                    LargeClaim => Manager approves or rejects\n";

fn check_document(label: &str, source: &str) -> (Value, i32) {
    let scratch = Scratch::new(label, None);
    let file = scratch.write("doc.fsl", source);
    fslc(&scratch.0, &["check", &file])
}

fn assert_error(label: &str, source: &str, code: &str, message: &str) {
    let (output, exit) = check_document(label, source);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("error"), 2),
        "{label}: {output:#}"
    );
    assert_eq!(output["diagnostic_code"], code, "{label}: {output:#}");
    let text = output["message"].as_str().unwrap_or_default();
    assert!(
        text.contains(message),
        "{label}: message {text:?} lacks {message:?}"
    );
}

/// The warnings of a document that checks clean, without the
/// `no_user_invariants` warning none of these documents is about.
fn warnings(label: &str, source: &str) -> Vec<Value> {
    let (output, exit) = check_document(label, source);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{label}: {output:#}"
    );
    output["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|warning| warning["kind"] != "no_user_invariants")
        .cloned()
        .collect()
}

/// The document the negatives below change checks clean.
#[test]
fn the_table_documents_start_from_a_clean_table() {
    assert_eq!(
        warnings("clean", &table_document("<=", ROWS, "", 3)),
        Vec::<Value>::new()
    );
}

/// Acceptance 2: a gap at the boundary, reported with its values.
#[test]
fn rows_that_leave_a_value_uncovered_are_decide_not_exhaustive() {
    assert_error(
        "not-exhaustive",
        &table_document("<", ROWS, "", 3),
        "decide_not_exhaustive",
        "no row holds at amount = 1, AUTO_LIMIT = 1",
    );
}

/// Acceptance 2: two rows that hold together, reported with the shared
/// values.
#[test]
fn rows_that_hold_together_are_decide_overlap() {
    assert_error(
        "overlap-boundary",
        &table_document(
            "<=",
            "    SmallClaim => System approves\n    \
             LargeClaim => Manager approves or rejects\n    \
             SmallClaim => Manager rejects\n",
            "",
            3,
        ),
        "decide_overlap",
        "row 1 `SmallClaim => System approves` and row 3 `SmallClaim => Manager rejects` both \
         hold at amount = 0, AUTO_LIMIT = 1",
    );
}

/// Acceptance 2: every event leaving the stage has a row.
#[test]
fn an_event_leaving_the_stage_without_a_row_is_decide_event_not_listed() {
    assert_error(
        "not-listed",
        &table_document("<=", "    SmallClaim => System approves\n", "", 3),
        "decide_event_not_listed",
        "mgr_approve (Manager approves), reject (Manager rejects)",
    );
}

/// A row's verb is matched as declared, against the events leaving the
/// stage.
#[test]
fn a_row_no_event_leaving_the_stage_matches_is_unknown_event_phrase() {
    assert_error(
        "row-phrase",
        &table_document(
            "<=",
            "    SmallClaim => System approves\n    \
             LargeClaim => Manager approve or rejects\n",
            "",
            3,
        ),
        "unknown_event_phrase",
        "'Manager approve'",
    );
    assert_error(
        "row-phrase-stage",
        &table_document(
            "<=",
            "    SmallClaim => System approves\n    \
             LargeClaim => Manager approves or rejects\n    \
             Never => Employee submits\n",
            "",
            3,
        ),
        "unknown_event_phrase",
        "'Employee submits'",
    );
}

/// Acceptance 2, positive: `otherwise` closes the gap, and its events take
/// exactly the values no other row holds for: a manager approves the
/// boundary claim (amount 1) and cannot approve a small one (amount 0).
#[test]
fn otherwise_closes_the_gap_with_the_negation_of_the_other_rows() {
    let rows = "    SmallClaim => System approves\n    \
                otherwise => Manager approves or rejects\n";
    assert_eq!(
        warnings("otherwise", &table_document("<", rows, "", 3)),
        Vec::<Value>::new()
    );
    let scenario = "  scenario AC \"boundary\" {\n    Employee submits c1 with amount 1\n    \
                    Manager approves c1\n    then c1 is Approved\n  }\n";
    assert_eq!(
        warnings(
            "otherwise-scenario",
            &table_document("<", rows, scenario, 3)
        ),
        Vec::<Value>::new()
    );
    let forbidden = "  forbidden FB \"small\" {\n    Employee submits c1 with amount 0\n    \
                     Manager approves c1\n    then rejected\n  }\n";
    assert_eq!(
        warnings(
            "otherwise-forbidden",
            &table_document("<", rows, forbidden, 3)
        ),
        Vec::<Value>::new()
    );
    let (output, exit) = check_document(
        "otherwise-forbidden-boundary",
        &table_document("<", rows, &forbidden.replace("amount 0", "amount 1"), 3),
    );
    assert_eq!(
        (output["kind"].as_str(), exit),
        (Some("forbidden"), 2),
        "{output:#}"
    );
}

/// design B.6: a row that holds for no value is a warning.
#[test]
fn a_row_that_holds_for_no_value_is_decide_row_emptied() {
    let found = warnings(
        "row-emptied",
        &table_document(
            "<=",
            "    SmallClaim => System approves\n    \
             LargeClaim => Manager approves\n    \
             Never => Manager rejects\n",
            "",
            3,
        ),
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0]["kind"], "decide_row_emptied");
    assert!(
        found[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("row 3 `Never => Manager rejects`")),
        "{found:#?}"
    );
}

/// The claim document of acceptance 3: `SmallClaim` starts at 1, so amount
/// 0 is a gap on the type's domain, which no submitted claim reaches when
/// `submit`'s input is `input_type`.
fn assuming_document(input_type: &str, assuming: &str) -> String {
    format!(
        "fsl 5\nrequirements R {{\n  number Amount\n  type Positive = 1..3\n  \
         const AUTO_LIMIT = 1\n  actor Employee, System, Manager\n  entity Claim {{\n    \
         amount: Amount = 0\n    lifecycle Draft -> Submitted -> Approved\n    \
         lifecycle Submitted -> Rejected\n  }}\n  \
         concept SmallClaim = Claim where amount >= 1 and amount <= AUTO_LIMIT\n  \
         concept LargeClaim = Claim where amount > AUTO_LIMIT\n  \
         event submit = Employee submits Claim with amount: {input_type} Draft -> Submitted\n  \
         event auto_approve = System approves Claim Submitted -> Approved\n  \
         event mgr_approve = Manager approves Claim Submitted -> Approved\n  \
         event reject = Manager rejects Claim Submitted -> Rejected\n  \
         decide T \"who handles a claim\" for Submitted Claim {{\n{ROWS}  }}{assuming}\n}}\n\
         verify {{\n  instances Claim = 2\n  values Amount = 0..3\n}}\n"
    )
}

/// Acceptance 3: on the type's domain amount 0 is a gap, though no
/// submitted claim has it. `assuming amount > 0` closes it for `check`, and
/// `verify` proves the assumption; where `submit` takes 0, `verify` reports
/// the assumption violated.
#[test]
fn assuming_closes_an_unreachable_gap_and_verify_proves_it() {
    assert_error(
        "assuming-none",
        &assuming_document("Positive", ""),
        "decide_not_exhaustive",
        "no row holds at amount = 0",
    );
    let scratch = Scratch::new("assuming", None);
    let holds = scratch.write(
        "holds.fsl",
        &assuming_document("Positive", " assuming amount > 0"),
    );
    let (output, exit) = fslc(&scratch.0, &["check", &holds]);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{output:#}"
    );
    let (output, exit) = fslc(&scratch.0, &["verify", &holds]);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("verified"), 0),
        "{output:#}"
    );
    assert!(
        output["invariants_checked"]
            .as_array()
            .is_some_and(|checked| checked.iter().any(|name| name == "_decide_T_assuming")),
        "{output:#}"
    );

    let broken = scratch.write(
        "broken.fsl",
        &assuming_document("Amount", " assuming amount > 0"),
    );
    let (output, exit) = fslc(&scratch.0, &["check", &broken]);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{output:#}"
    );
    let (output, exit) = fslc(&scratch.0, &["verify", &broken]);
    assert_eq!(
        (
            output["result"].as_str(),
            output["generated_name"].as_str(),
            exit
        ),
        (Some("violated"), Some("_decide_T_assuming"), 1),
        "{output:#}"
    );
}

/// decisions/048 J3: a table whose field takes more values than the
/// enumeration bound is checked by the solver in `check`, with the same codes
/// and a counterexample state.
#[test]
fn past_the_enumeration_bound_the_solver_checks_the_table() {
    let top = 1_000_000;
    assert_error(
        "solver-gap",
        &table_document("<", ROWS, "", top),
        "decide_not_exhaustive",
        "(checked by the solver, because its fields take 1000001 values, more than the 1000000 \
         enumerated); counterexample: claim_stage = {\"0\":\"Submitted\"}, \
         claim_amount = {\"0\":1}",
    );
    assert_error(
        "solver-overlap",
        &table_document(
            "<=",
            "    SmallClaim => System approves\n    \
             LargeClaim => Manager approves or rejects\n    \
             SmallClaim => Manager rejects\n",
            "",
            top,
        ),
        "decide_overlap",
        "row 1 `SmallClaim => System approves` and row 3 `SmallClaim => Manager rejects` both \
         hold in a state the solver found",
    );
    assert_eq!(
        warnings("solver-clean", &table_document("<=", ROWS, "", top)),
        Vec::<Value>::new()
    );
}
