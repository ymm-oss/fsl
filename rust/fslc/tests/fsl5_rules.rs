// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S6, through the native CLI: `rule REQ "text" { … }` with
//! `allow`, `never`, `only after`, `enters … only from`, `always` and
//! `deadline`, in a document headed `fsl 5`, behind the `fsl5` gate.
//!
//! Acceptance 1 and 3 (K, R, M for the rejection example, e2e's REQ-1 and the
//! hand-written `rules` example) live in `fsl5_lowering_contract.rs`. This
//! file holds:
//!
//! - **Acceptance 2**: with its rules, every forbidden trace of the rejection
//!   example is rejected (S4 showed three of them accepted without the
//!   rules), and taking out one rule statement makes exactly the forbidden
//!   traces that probe it fail. The `rules` example is held to the same
//!   table.
//! - The checks of B.6 and B.7: an unsatisfiable guard (decided by
//!   enumeration, and by the solver past the enumeration bound),
//!   `implied_by_lifecycle`, `never_rule_without_forbidden`, phrase and name
//!   errors, and that rules compose in any order.
//! - What the lowering records: the origin's lowering steps and the
//!   requirement annotations, and `deadline` as the requirement block's.

use std::collections::BTreeSet;
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

fn fixture(example: &str, file: &str) -> String {
    std::fs::read_to_string(fixture_dir(example).join(file)).expect("read fixture")
}

/// A scratch directory holding a copy of one fixture example.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, example: Option<&str>) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("fsl5_rules")
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

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, source).expect("write scratch source");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `fslc <command> <file>` in `directory` with the `fsl5` gate set.
fn fslc(command: &str, directory: &Path, file: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([command, file])
        .current_dir(directory)
        .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
        .output()
        .expect("run fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{command} {file}: invalid JSON ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

fn check(directory: &Path, file: &str) -> (Value, i32) {
    fslc("check", directory, file)
}

/// `source` without the one line `line` (matched exactly once, trimmed),
/// and without its `rule` block when that was the block's only statement (a
/// rule has at least one).
fn without_line(source: &str, line: &str) -> String {
    let matching = source.lines().filter(|text| text.trim() == line).count();
    assert_eq!(matching, 1, "{line:?} must be one line of the source");
    let lines = source
        .split_inclusive('\n')
        .filter(|text| text.trim() != line)
        .collect::<Vec<_>>();
    let mut kept = String::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].starts_with("  rule ") && lines.get(index + 1) == Some(&"  }\n") {
            index += 2;
            continue;
        }
        kept.push_str(lines[index]);
        index += 1;
    }
    kept
}

/// `source` keeping, of its `scenario` / `forbidden` blocks, only the one
/// whose id is `keep`. Blocks open at two-space indent and close at `  }`.
fn only_trace(source: &str, keep: &str) -> String {
    let mut kept = String::new();
    let mut skipping = false;
    for line in source.split_inclusive('\n') {
        if ["  scenario ", "  forbidden "]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        {
            skipping = !line.contains(&format!(" {keep} "));
        }
        if !skipping {
            kept.push_str(line);
        }
        if skipping && line == "  }\n" {
            skipping = false;
        }
    }
    assert!(kept.contains(&format!(" {keep} ")), "no trace {keep}");
    kept
}

/// `source` without its `forbidden` block `id`.
fn without_trace(source: &str, id: &str) -> String {
    let mut kept = String::new();
    let mut skipping = false;
    for line in source.split_inclusive('\n') {
        if line.starts_with(&format!("  forbidden {id} ")) {
            skipping = true;
        }
        if !skipping {
            kept.push_str(line);
        }
        if skipping && line == "  }\n" {
            skipping = false;
        }
    }
    kept
}

fn trace_ids(source: &str, keyword: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| line.strip_prefix(&format!("  {keyword} ")))
        .map(|rest| rest.split_whitespace().next().expect("trace id").to_owned())
        .collect()
}

/// The forbidden traces of `source` the model accepts (so `check` fails them
/// with kind `forbidden`), each checked on its own (the other traces
/// removed), after the scenarios are confirmed to pass.
fn accepted_forbidden_traces(scratch: &Scratch, source: &str) -> BTreeSet<String> {
    for id in trace_ids(source, "scenario") {
        scratch.write("trace.fsl", &only_trace(source, &id));
        let (output, exit) = check(&scratch.0, "trace.fsl");
        assert_eq!(
            (output["result"].as_str(), exit),
            (Some("ok"), 0),
            "scenario {id}: {output:#}"
        );
    }
    let mut accepted = BTreeSet::new();
    for id in trace_ids(source, "forbidden") {
        scratch.write("trace.fsl", &only_trace(source, &id));
        let (output, exit) = check(&scratch.0, "trace.fsl");
        match (output["result"].as_str(), output["kind"].as_str(), exit) {
            (Some("ok"), _, 0) => {}
            (Some("error"), Some("forbidden"), 2) => {
                assert_eq!(output["id"], id.as_str(), "{output:#}");
                accepted.insert(id);
            }
            _ => panic!("forbidden {id}: unexpected check result {output:#}"),
        }
    }
    accepted
}

fn ids(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|id| (*id).to_owned()).collect()
}

/// Acceptance 2 of S6 for one example: with all its rules every forbidden
/// trace is rejected, and each row's statement removed leaves exactly the
/// row's traces accepted. Every statement of the rule blocks has a row.
fn assert_rule_table(example: &str, rows: &[(&str, &[&str])]) {
    let source = fixture(example, "fsl5_en.fsl");
    let scratch = Scratch::new(&format!("table-{example}"), Some(example));
    assert_eq!(
        accepted_forbidden_traces(&scratch, &source),
        BTreeSet::new(),
        "{example}: with the rules, every forbidden trace is rejected"
    );
    let statements = rule_statements(&source);
    assert_eq!(
        statements,
        rows.iter()
            .map(|(line, _)| (*line).to_owned())
            .collect::<Vec<_>>(),
        "{example}: one row per rule statement"
    );
    for (line, expected) in rows {
        let weakened = without_line(&source, line);
        assert_eq!(
            accepted_forbidden_traces(&scratch, &weakened),
            ids(expected),
            "{example}: without `{line}`"
        );
    }
}

/// The statements of the `rule` blocks of `source`, one per line.
fn rule_statements(source: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut inside = false;
    for line in source.lines() {
        if line.starts_with("  rule ") {
            inside = true;
        } else if inside && line == "  }" {
            inside = false;
        } else if inside {
            statements.push(line.trim().to_owned());
        }
    }
    statements
}

/// Acceptance 2: the three forbidden traces of the rejection example that
/// S4 accepted without rules are rejected, the fourth (which the lifecycle
/// rejects) stays rejected, and each rule statement is what rejects its own
/// traces: the two `allow` statements no trace probes, and the properties,
/// leave every trace rejected when removed.
#[test]
fn the_rejection_forbidden_traces_hold_with_the_rules_and_each_needs_its_rule() {
    assert_rule_table(
        "rejection",
        &[
            ("allow Agent requests Request only for ToolNeeded", &[]),
            (
                "allow Operator approves Request only for OperatorHeld",
                &["FB-PUBLIC-APPROVE"],
            ),
            ("allow Fault forges Request only for OperatorHeld", &[]),
            (
                "allow Agent executes Request only for ExplicitlyApproved and OperatorHeld",
                &[
                    "FB-EXECUTE-WITH-DOWNGRADED-APPROVAL",
                    "FB-EXECUTE-WITH-PUBLIC-ROLE",
                ],
            ),
            (
                "ToolExecutionApproved: always ActionExecuted Request is ExplicitlyApproved and \
                 OperatorHeld",
                &[],
            ),
            (
                "ExecutionComesFromApprovedStage: Request enters ActionExecuted only from \
                 ToolApproved",
                &[],
            ),
        ],
    );
}

/// The same table for the hand-written example of `only after`, `never …
/// when`, `never … for` and `always … has`.
#[test]
fn the_rules_example_forbidden_traces_each_need_their_rule() {
    assert_rule_table(
        "rules",
        &[
            (
                "Shop ships Order only after User pays Order",
                &["FB-SHIP-UNPAID"],
            ),
            ("never Shop packs Order when qty == 0", &["FB-PACK-EMPTY"]),
            ("never User adds Order for Large", &["FB-ADD-LARGE"]),
            ("always Order in Shipped has qty > 0", &[]),
        ],
    );
}

/// The properties are checked by `verify`: the `rules` example verifies, and
/// without the guard that keeps empty orders from being packed (and the
/// forbidden trace that probes it, which `verify` replays first), `always
/// Order in Shipped has qty > 0` is violated.
#[test]
fn the_rule_properties_are_verified() {
    let scratch = Scratch::new("verify", Some("rules"));
    let (output, exit) = fslc("verify", &scratch.0, "fsl5_en.fsl");
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("verified"), 0),
        "{output:#}"
    );
    let source = fixture("rules", "fsl5_en.fsl");
    scratch.write(
        "weakened.fsl",
        &without_trace(
            &without_line(&source, "never Shop packs Order when qty == 0"),
            "FB-PACK-EMPTY",
        ),
    );
    let (output, exit) = fslc("verify", &scratch.0, "weakened.fsl");
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("violated"), 1),
        "{output:#}"
    );
    assert_eq!(
        (
            output["violation_kind"].as_str(),
            output["invariant"].as_str(),
            output["generated_name"].as_str()
        ),
        (Some("invariant"), Some("REQ-3"), Some("REQ_3_always")),
        "{output:#}"
    );
}

/// B.6: guards are conjunctions, so the order of the statements does not
/// matter. The rejection rule block reversed lowers to the same kernel.
#[test]
fn rule_statements_compose_in_any_order() {
    let source = fixture("rejection", "fsl5_en.fsl");
    let statements = rule_statements(&source);
    let mut reversed = source.clone();
    let indented = |lines: Vec<&String>| {
        lines.into_iter().fold(String::new(), |mut text, line| {
            text.push_str("    ");
            text.push_str(line);
            text.push('\n');
            text
        })
    };
    let block = indented(statements.iter().collect());
    let backwards = indented(statements.iter().rev().collect());
    assert!(reversed.contains(&block), "the rule block is contiguous");
    reversed = reversed.replacen(&block, &backwards, 1);
    let scratch = Scratch::new("order", Some("rejection"));
    let path = scratch.write("reversed.fsl", &reversed);
    kernel_compare::assert_same_kernel(
        &kernel_compare::kernel_v2_fsl5(&path),
        &kernel_compare::kernel_v2(&fixture_dir("rejection").join("lowered.fsl")),
        "rejection with its rule statements reversed",
    );
}

/// The small entity the documents below start from.
fn claim_document(members: &str, verify: &str) -> String {
    format!(
        "fsl 5\nrequirements R {{\n  number Amount\n  const LIMIT = 1\n  actor Employee, Manager\n  \
         entity Claim {{\n    amount: Amount = 0\n    lifecycle Draft -> Submitted -> Approved\n    \
         lifecycle Submitted -> Rejected\n  }}\n  concept Small = Claim where amount <= LIMIT\n  \
         concept Decided = Claim in Approved, Rejected\n  \
         event submit = Employee submits Claim with amount Draft -> Submitted\n  \
         event approve = Manager approves Claim Submitted -> Approved\n  \
         event reject = Manager rejects Claim Submitted -> Rejected\n\
         {members}}}\nverify {{\n  instances Claim = 1\n  {verify}\n}}\n"
    )
}

const SMALL: &str = "values Amount = 0..2";

fn check_source(label: &str, source: &str) -> (Value, i32) {
    let scratch = Scratch::new(label, None);
    scratch.write("doc.fsl", source);
    check(&scratch.0, "doc.fsl")
}

fn assert_error(label: &str, source: &str, code: &str, message: &str) {
    let (output, exit) = check_source(label, source);
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

/// `check` passes; the warnings other than `no_user_invariants` (none of
/// these documents declares an invariant), as `(code, severity)`.
fn diagnostics(label: &str, source: &str) -> Vec<(String, String)> {
    let (output, exit) = check_source(label, source);
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
        .map(|warning| {
            (
                warning["code"].as_str().unwrap_or_default().to_owned(),
                warning["severity"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

/// The negatives below start from documents that check clean.
#[test]
fn the_negative_examples_start_from_a_clean_document() {
    let rule = "  rule REQ-1 \"t\" {\n    allow Employee submits Claim only when input amount > 0\n  \
                }\n  forbidden FB-1 \"t\" {\n    Employee submits c1 with amount 0\n    \
                then rejected\n  }\n";
    assert_eq!(
        diagnostics("clean", &claim_document(rule, SMALL)),
        Vec::<(String, String)>::new()
    );
}

/// B.6 `rule_conflict_unsatisfiable_guard`: the guards of one event cannot
/// hold together over the type domains. The message lists the statements of
/// a minimal conflicting subset, not the third statement that does not take
/// part. A satisfiable pair does not fire.
#[test]
fn guards_that_cannot_hold_together_are_rule_conflict_unsatisfiable_guard() {
    let rules = |third: &str| {
        format!(
            "  rule REQ-1 \"t\" {{\n    allow Employee submits Claim only when input amount > 1\n    \
             never Employee submits Claim when input amount == 0\n  }}\n  \
             rule REQ-2 \"t\" {{\n    allow Employee submits Claim only when {third}\n  }}\n"
        )
    };
    let label = "unsat";
    let (output, exit) = check_source(label, &claim_document(&rules("input amount < 2"), SMALL));
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("error"), 2),
        "{output:#}"
    );
    assert_eq!(
        output["diagnostic_code"], "rule_conflict_unsatisfiable_guard",
        "{output:#}"
    );
    let message = output["message"].as_str().unwrap_or_default();
    for expected in [
        "event 'submit' can never occur",
        "REQ-1 `allow Employee submits Claim only when input amount > 1`",
        "REQ-2 `allow Employee submits Claim only when input amount < 2`",
    ] {
        assert!(message.contains(expected), "{message:?} lacks {expected:?}");
    }
    assert!(
        !message.contains("amount == 0"),
        "{message:?} names a statement outside the conflict"
    );
    let _ = diagnostics(
        "unsat-control",
        &claim_document(
            &format!(
                "{}  forbidden FB-1 \"t\" {{\n    Employee submits c1 with amount 0\n    \
                 then rejected\n  }}\n",
                rules("input amount < 3")
            ),
            SMALL,
        ),
    );
}

/// A concept whose stages exclude the stage the event leaves from empties
/// the guard as well.
#[test]
fn a_concept_off_the_event_stage_is_rule_conflict_unsatisfiable_guard() {
    assert_error(
        "unsat-stage",
        &claim_document(
            "  rule REQ-1 \"t\" {\n    allow Manager approves Claim only for Decided\n  }\n",
            SMALL,
        ),
        "rule_conflict_unsatisfiable_guard",
        "when Claim is Submitted",
    );
}

/// decisions/048 J3: past the enumeration bound the lowering leaves the
/// question to `check`'s solver, which reports the same conflict.
#[test]
fn past_the_enumeration_bound_check_asks_the_solver() {
    let rules = "  rule REQ-1 \"t\" {\n    allow Employee submits Claim only when input amount > 5\n    \
                 allow Employee submits Claim only when input amount < 4\n  }\n";
    let huge = "values Amount = 0..2000000";
    // The lowering alone (`kernel`) does not enumerate this domain, so it
    // reports nothing: the rejection below is the solver's.
    let scratch = Scratch::new("unsat-solver-kernel", None);
    scratch.write("doc.fsl", &claim_document(rules, huge));
    let (output, exit) = fslc("kernel", &scratch.0, "doc.fsl");
    assert_eq!(exit, 0, "{output:#}");
    assert_error(
        "unsat-solver",
        &claim_document(rules, huge),
        "rule_conflict_unsatisfiable_guard",
        "REQ-1 `allow Employee submits Claim only when input amount > 5`; REQ-1 `allow \
         Employee submits Claim only when input amount < 4`",
    );
    // The same domain with a satisfiable pair passes.
    let (output, exit) = check_source(
        "unsat-solver-control",
        &claim_document(
            "  rule REQ-1 \"t\" {\n    allow Employee submits Claim only when input amount > 5\n  \
             }\n  forbidden FB-1 \"t\" {\n    Employee submits c1 with amount 0\n    \
             then rejected\n  }\n",
            huge,
        ),
    );
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{output:#}"
    );
}

/// B.7 `implied_by_lifecycle` (info): an ordering the lifecycle already
/// gives adds no ghost state, and an entry rule the lifecycle already gives
/// is noted (its transition property is still declared).
#[test]
fn rules_the_lifecycle_implies_are_implied_by_lifecycle() {
    let after =
        "  rule REQ-1 \"t\" {\n    Manager approves Claim only after Employee submits Claim\n  }\n";
    assert_eq!(
        diagnostics("implied-after", &claim_document(after, SMALL)),
        vec![("implied_by_lifecycle".to_owned(), "info".to_owned())]
    );
    let scratch = Scratch::new("implied-after-kernel", None);
    let path = scratch.write("doc.fsl", &claim_document(after, SMALL));
    let kernel = kernel_compare::kernel_v2_fsl5(&path);
    let state = kernel["state"]
        .as_array()
        .expect("state")
        .iter()
        .map(|field| field["name"].as_str().expect("state name").to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(state, ids(&["claim_amount", "claim_stage"]));

    let enters = "  rule REQ-1 \"t\" {\n    Claim enters Approved only from Submitted\n  }\n";
    assert_eq!(
        diagnostics("implied-enters", &claim_document(enters, SMALL)),
        vec![("implied_by_lifecycle".to_owned(), "info".to_owned())]
    );
    // An entry rule that narrows the lifecycle is not implied.
    let narrowing = claim_document(
        "  event reopen = Manager reopens Claim Rejected -> Approved\n  \
         rule REQ-1 \"t\" {\n    Claim enters Approved only from Submitted\n  }\n",
        SMALL,
    )
    .replace(
        "lifecycle Submitted -> Rejected\n",
        "lifecycle Submitted -> Rejected -> Approved\n",
    );
    assert_eq!(diagnostics("narrowing-enters", &narrowing), Vec::new());
}

/// B.7 `never_rule_without_forbidden` (warning, #1300): a rejection rule no
/// forbidden trace probes; a trace ending in the guarded event, or naming
/// the requirement, probes it.
#[test]
fn a_rejection_rule_without_a_forbidden_trace_is_a_warning() {
    let rule = "  rule REQ-1 \"t\" {\n    never Manager approves Claim for Small\n  }\n";
    assert_eq!(
        diagnostics("unprobed", &claim_document(rule, SMALL)),
        vec![(
            "never_rule_without_forbidden".to_owned(),
            "warning".to_owned()
        )]
    );
    for probe in [
        "  forbidden FB-1 \"t\" {\n    Employee submits c1 with amount 1\n    \
         Manager approves c1\n    then rejected\n  }\n",
        "  @requirement(\"REQ-1\")\n  forbidden FB-1 \"t\" {\n    Employee submits c1 with amount 0\n    \
         Manager rejects c1\n    Manager approves c1\n    then rejected\n  }\n",
    ] {
        assert_eq!(
            diagnostics("probed", &claim_document(&format!("{rule}{probe}"), SMALL)),
            Vec::new()
        );
    }
}

/// Phrase errors: a verb no event has (with the nearest one), a phrase no
/// event matches, an undeclared actor, an unknown concept.
#[test]
fn rule_phrases_resolve_against_the_events() {
    let rule = |statement: &str| {
        claim_document(
            &format!("  rule REQ-1 \"t\" {{\n    {statement}\n  }}\n"),
            SMALL,
        )
    };
    assert_error(
        "rule-unknown-verb",
        &rule("allow Manager approve Claim only for Small"),
        "unknown_verb",
        "did you mean 'approves'?",
    );
    assert_error(
        "rule-unknown-phrase",
        &rule("allow Employee approves Claim only for Small"),
        "unknown_event_phrase",
        "verbs of 'Employee' on Claim: submits",
    );
    assert_error(
        "rule-actor",
        &rule("allow Auditor approves Claim only for Small"),
        "actor_not_declared",
        "Auditor",
    );
    let (output, exit) = check_source(
        "rule-concept",
        &rule("allow Manager approves Claim only for Smal"),
    );
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("error"), 2),
        "{output:#}"
    );
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("did you mean 'Small'?")),
        "{output:#}"
    );
}

/// decisions/046 Q6 holds in rules: a bare name that is both the event's
/// input and a field is ambiguous.
#[test]
fn a_bare_input_and_field_name_in_a_rule_is_ambiguous_input_field() {
    assert_error(
        "rule-ambiguous",
        &claim_document(
            "  rule REQ-1 \"t\" {\n    allow Employee submits Claim only when amount > 0\n  }\n",
            SMALL,
        ),
        "ambiguous_input_field",
        "input amount",
    );
}

/// decisions/050: what a rule generates goes through the naming owner and
/// may not meet a declared name: the ghost field of `only after`, and the
/// property name of `always`.
#[test]
fn names_a_rule_generates_are_checked_for_collisions() {
    let ghost = claim_document(
        "  rule REQ-1 \"t\" {\n    Manager rejects Claim only after Manager approves Claim\n  }\n",
        SMALL,
    )
    .replace(
        "amount: Amount = 0\n",
        "amount: Amount = 0\n    did_approve: Bool = false\n",
    );
    assert_error(
        "ghost-collision",
        &ghost,
        "generated_name_collision",
        "did_approve",
    );
    let property = claim_document(
        "  rule REQ-1 \"t\" {\n    always Approved Claim is Small\n  }\n  \
         requirement REQ-9 \"t\" {\n    invariant REQ_1_always {\n      true\n    }\n  }\n",
        SMALL,
    );
    assert_error(
        "property-collision",
        &property,
        "generated_name_collision",
        "REQ_1_always",
    );
}

/// Lowering steps (no schema change, plan §0): each guard and property a
/// rule lowers to has an origin whose step kind is its statement's.
#[test]
fn rule_lowerings_carry_their_statement_as_a_lowering_step() {
    let mut kinds = BTreeSet::new();
    for example in ["rejection", "rules"] {
        let kernel = kernel_compare::kernel_v2_fsl5(&fixture_dir(example).join("fsl5_en.fsl"));
        let origins = kernel["provenance"]["origins"].as_array().expect("origins");
        for origin in origins {
            for step in origin["lowering_steps"].as_array().expect("steps") {
                let kind = step["kind"].as_str().expect("step kind");
                if ["allow", "never", "after", "always", "enters"].contains(&kind) {
                    let detail = step["detail"].as_str().expect("step detail");
                    assert!(detail.starts_with("REQ-"), "{detail:?}");
                    kinds.insert(kind.to_owned());
                }
            }
        }
        let bindings = kernel["provenance"]["bindings"]
            .as_array()
            .expect("bindings")
            .iter()
            .map(|binding| binding["target"].as_str().expect("target").to_owned())
            .collect::<BTreeSet<_>>();
        let guarded = if example == "rules" {
            "action:ship:guard:1"
        } else {
            "action:approve_tool:guard:1"
        };
        assert!(bindings.contains(guarded), "{example}: {bindings:?}");
    }
    assert_eq!(
        kinds,
        ids(&["allow", "never", "after", "always", "enters"]),
        "every statement kind but deadline lowers with its step"
    );
}

/// The rule's requirement is annotated on the actions it guards and the
/// properties it declares.
#[test]
fn rules_annotate_their_requirement() {
    let source = fixture("rules", "fsl5_en.fsl");
    let options = fsl_syntax::ParseOptions::with_unstable(
        fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
    );
    let kernel = fsl_core::parse_kernel_source_with_file_with_options(
        &source,
        &fsl_core::FsResolver::new(fixture_dir("rules")),
        "fsl5_en.fsl",
        &options,
    )
    .expect("lower");
    let model = fsl_core::build_model(kernel).expect("model");
    for (target, requirement) in [
        ("action:ship", "REQ-1"),
        ("action:pack", "REQ-2"),
        ("action:add", "REQ-2"),
        ("property:invariant:REQ_3_always", "REQ-3"),
    ] {
        let annotated = model
            .annotations_for(target)
            .source_order()
            .iter()
            .any(|annotation| {
                matches!(annotation, fsl_syntax::Annotation::Requirement { id, .. } if *id == requirement)
            });
        assert!(annotated, "{target} lacks {requirement}");
    }
    assert!(
        !model
            .annotations_for("action:pay")
            .source_order()
            .iter()
            .any(|annotation| matches!(
                annotation,
                fsl_syntax::Annotation::Requirement { id, .. } if id.starts_with("REQ-")
            )),
        "pay is guarded by no rule (it only sets the ghost)"
    );
}

/// `deadline` in a rule is the requirement block's `deadline`, unchanged.
#[test]
fn a_rule_deadline_is_the_requirement_deadline() {
    let document = |deadline: &str| {
        format!(
            "fsl 5\nrequirements R {{\n  const SLA = 2\n  actor Agent\n  entity Case {{\n    \
             lifecycle Open -> Done\n  }}\n  event close = Agent closes Case Open -> Done\n  \
             time {{\n    urgent close\n    age open_age[c: Case] while case_stage[c] == Open\n  \
             }}\n{deadline}}}\nverify {{\n  instances Case = 1\n}}\n"
        )
    };
    let scratch = Scratch::new("deadline", None);
    let rule = scratch.write(
        "rule.fsl",
        &document("  rule REQ-1 \"t\" {\n    deadline open_age <= SLA\n  }\n"),
    );
    let block = scratch.write(
        "block.fsl",
        &document("  requirement REQ-1 \"t\" {\n    deadline open_age <= SLA\n  }\n"),
    );
    kernel_compare::assert_same_kernel(
        &kernel_compare::kernel_v2_fsl5(&rule),
        &kernel_compare::kernel_v2_fsl5(&block),
        "rule deadline against requirement deadline",
    );
    let (output, exit) = fslc("verify", &scratch.0, "rule.fsl");
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("verified"), 0),
        "{output:#}"
    );
}

/// The kernel property names of a `fsl 5` document's invariants and
/// transition properties (the kernel lists them by name).
fn property_names(label: &str, source: &str) -> BTreeSet<String> {
    let scratch = Scratch::new(label, None);
    let path = scratch.write("doc.fsl", source);
    let kernel = kernel_compare::kernel_v2_fsl5(&path);
    ["invariants", "transitions"]
        .iter()
        .flat_map(|kind| {
            kernel["properties"][kind]
                .as_array()
                .expect("property list")
                .iter()
                .map(|property| property["name"].as_str().expect("name").to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A property statement takes the name written before it (`Name: …`); an
/// unnamed one falls back to `<REQ>_<kind>`, numbered among the
/// requirement's unnamed statements of that kind.
#[test]
fn a_written_property_name_is_kept_and_an_omitted_one_is_generated() {
    let source = claim_document(
        "  rule REQ-1 \"t\" {\n    always Approved Claim is Small\n    \
         SmallRejected: always Rejected Claim is Small\n    \
         always Submitted Claim is Small\n    \
         Claim enters Approved only from Submitted\n    \
         ApprovedFromSubmitted: Claim enters Approved only from Submitted\n  }\n",
        SMALL,
    );
    assert_eq!(
        property_names("names", &source),
        ids(&[
            "REQ_1_always",
            "SmallRejected",
            "REQ_1_always_2",
            "REQ_1_enters",
            "ApprovedFromSubmitted",
        ])
    );
}

/// A written name goes through the same collision check as a generated one:
/// it may not meet a declared property or another rule's property.
#[test]
fn a_written_property_name_that_is_taken_is_rule_property_name_collision() {
    assert_error(
        "written-declared",
        &claim_document(
            "  rule REQ-1 \"t\" {\n    Approvals: always Approved Claim is Small\n  }\n  \
             requirement REQ-9 \"t\" {\n    invariant Approvals {\n      true\n    }\n  }\n",
            SMALL,
        ),
        "rule_property_name_collision",
        "'Approvals', which is already declared",
    );
    assert_error(
        "written-twice",
        &claim_document(
            "  rule REQ-1 \"t\" {\n    Approvals: always Approved Claim is Small\n  }\n  \
             rule REQ-2 \"t\" {\n    Approvals: Claim enters Approved only from Submitted\n  }\n",
            SMALL,
        ),
        "rule_property_name_collision",
        "'Approvals'",
    );
    assert_error(
        "written-generated",
        &claim_document(
            "  rule REQ-1 \"t\" {\n    REQ_2_always: always Approved Claim is Small\n  }\n  \
             rule REQ-2 \"t\" {\n    always Rejected Claim is Small\n  }\n",
            SMALL,
        ),
        "generated_name_collision",
        "'REQ_2_always'",
    );
}

/// A name no Kernel property can take, or a name on a statement that lowers
/// to no property, is `invalid_rule_property_name`.
#[test]
fn an_invalid_property_name_is_invalid_rule_property_name() {
    for (label, statement, message) in [
        (
            "separator",
            "Small__Approved: always Approved Claim is Small",
            "contains `__`",
        ),
        (
            "literal",
            "none: always Approved Claim is Small",
            "is a literal",
        ),
        (
            "guard",
            "Gate: allow Manager approves Claim only for Small",
            "only `always` and `enters` statements take a name",
        ),
    ] {
        assert_error(
            label,
            &claim_document(
                &format!("  rule REQ-1 \"t\" {{\n    {statement}\n  }}\n"),
                SMALL,
            ),
            "invalid_rule_property_name",
            message,
        );
    }
}
