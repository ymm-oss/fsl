// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1556: rule-conflict obligations must use spelled, stage-lowered and
//! def-expanded guards. Detectors fail on the #1552-only baseline; controls
//! establish the equivalent forms and preserve real main-path errors.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const POSITIVE: &str = "def positive(x: Amount) = x > 0";
const DRAFT: &str = "def draft(amount: Claim) = stage(amount) == Draft";

fn source(condition: &str, declarations: &str) -> String {
    format!(
        r#"fsl 5
requirements R {{
  number Amount
  {declarations}
  actor Manager
  entity Claim {{
    amount: Amount = 0
    lifecycle Draft -> Approved
  }}
  event approve = Manager approves Claim Draft -> Approved
  rule REQ-1 "Only eligible claims may be approved" {{
    allow Manager approves Claim only when {condition}
  }}
}}
verify {{
  instances Claim = 1
  values Amount = 0..2
}}
"#
    )
}

/// Collect both commands before asserting, including on check-time rejection.
/// Retain complete JSON, exits and stderr, not a filtered parity projection.
fn run(label: &str, text: &str) -> Vec<(&'static str, i32, Value)> {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1556_rule_guard_def_stage")
        .join(std::process::id().to_string())
        .join(label);
    std::fs::create_dir_all(&directory).expect("create owned fixture directory");
    std::fs::write(directory.join("rule.fsl"), text).expect("write rule fixture");
    ["check", "verify"]
        .map(|mode| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
            command.args([mode, "rule.fsl"]);
            if mode == "verify" {
                command.args(["--depth", "2", "--no-cache"]);
            }
            let output = command
                .current_dir(&directory)
                .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
                .env("FSLC_CACHE", "off")
                .output()
                .expect("run fslc");
            let value: Value = serde_json::from_slice(&output.stdout)
                .unwrap_or_else(|error| panic!("{label} {mode}: invalid JSON {error}: {output:?}"));
            let exit = output.status.code().expect("process exit code");
            std::fs::write(
                directory.join(format!("{mode}.json")),
                serde_json::to_vec_pretty(&json!({
                    "exit": exit,
                    "output": value,
                    "stderr": String::from_utf8_lossy(&output.stderr),
                }))
                .expect("serialize complete observation"),
            )
            .expect("retain observation");
            assert!(output.stderr.is_empty(), "{label} {mode}: {output:?}");
            (mode, exit, value)
        })
        .into_iter()
        .collect()
}

fn assert_accepts(label: &str, condition: &str, declarations: &str) {
    let outputs = run(label, &source(condition, declarations));
    for (mode, exit, value) in &outputs {
        let expected = if *mode == "check" { "ok" } else { "verified" };
        assert_eq!(
            (
                *exit,
                value["result"].as_str(),
                value.get("diagnostic_code")
            ),
            (0, Some(expected), None),
            "{label}: expected {mode}={expected}/0; complete observations: {outputs:#?}"
        );
        if *mode == "verify" {
            assert_eq!(value["checked_to_depth"], 2, "{label}: {value:#}");
            assert_eq!(value["completeness"], "bounded", "{label}: {value:#}");
        }
    }
}

fn assert_conflict(label: &str, condition: &str, declarations: &str) {
    let outputs = run(label, &source(condition, declarations));
    for (mode, exit, value) in &outputs {
        assert_eq!(*exit, 2, "{label} {mode}: {outputs:#?}");
        assert_eq!(value["result"], "error", "{label} {mode}: {value:#}");
        assert_eq!(value["kind"], "semantics", "{label} {mode}: {value:#}");
        assert_eq!(
            value["diagnostic_code"], "rule_conflict_unsatisfiable_guard",
            "{label} {mode}: {value:#}"
        );
        // The fail-closed 'could not be decided' error has the same code.
        // Require a genuine unsatisfiability finding, not merely exit 2.
        assert!(
            value["message"]
                .as_str()
                .expect("message")
                .starts_with("event 'approve' can never occur: when Claim is Draft,"),
            "{label} {mode}: {value:#}"
        );
    }
}

fn assert_main_error(label: &str, condition: &str, declarations: &str, message: &str) {
    let outputs = run(label, &source(condition, declarations));
    for (mode, exit, value) in &outputs {
        assert_eq!(*exit, 2, "{label} {mode}: {outputs:#?}");
        assert_eq!(value["result"], "error", "{label} {mode}: {value:#}");
        assert_eq!(value["kind"], "semantics", "{label} {mode}: {value:#}");
        // Pin absence, rather than accepting null or a fabricated rule error.
        assert_eq!(value.get("diagnostic_code"), None, "{label}: {value:#}");
        assert_eq!(value["message"], message, "{label} {mode}: {value:#}");
    }
}

macro_rules! accepts {
    ($name:ident, $condition:expr, $declarations:expr) => {
        #[test]
        fn $name() {
            assert_accepts(stringify!($name), $condition, $declarations);
        }
    };
}

accepts!(control_inline_positive, "amount > 0", "");
accepts!(control_explicit_field_positive, "Claim.amount > 0", "");
accepts!(
    control_lowered_stage_binder,
    "exists c: Claim { claim_stage[c] == Draft }",
    ""
);
accepts!(
    control_lowered_stage_field_named_binder,
    "exists amount: Claim { claim_stage[amount] == Draft }",
    ""
);
accepts!(
    control_inline_quantifier,
    "exists amount: Amount where amount == 1 { amount > 0 }",
    ""
);
accepts!(control_lowered_stage_literal, "claim_stage[0] == Draft", "");

accepts!(detector_def_field_argument, "positive(amount)", POSITIVE);
// The generated action also has its implicit typed entity parameter (`c`).
// Main-path stage resolution accepts it even without an explicit event input.
accepts!(
    detector_stage_implicit_entity_parameter,
    "stage(c) == Draft",
    ""
);
accepts!(
    detector_def_explicit_field_argument,
    "positive(Claim.amount)",
    POSITIVE
);
accepts!(
    detector_stage_typed_binder,
    "exists c: Claim { stage(c) == Draft }",
    ""
);
// Main path: dialect.rs substitutes the event guard's fields (~2502), then
// StageResolver (~397) resolves the typed binder, then lib.rs:889 expands defs.
// Spelling must leave the stage operand and field-named binder as entity IDs.
accepts!(
    detector_order_stage_field_named_binder,
    "exists amount: Claim { stage(amount) == Draft }",
    ""
);
// dialect.rs:874-879 resolves stage(amount) in the def's typed environment.
// Its parameter is named like a field but is not an entity-field read.
accepts!(
    detector_def_body_stage_parameter,
    "exists c: Claim { draft(c) }",
    DRAFT
);
// Resolving stages AFTER expansion would see stage(0) and reject its operand.
// The main path first lowers the typed def body to claim_stage[amount], then
// substitutes the literal. This also forbids re-spelling the def parameter.
accepts!(detector_order_stage_before_def_expansion, "draft(0)", DRAFT);
// Call arguments are field-spelled before expansion, not the def's parameters.
accepts!(
    detector_order_def_field_named_parameter,
    "positive(amount)",
    "def positive(amount: Amount) = amount > 0"
);
accepts!(
    detector_nested_def_calls,
    "positive(amount)",
    "def positive(amount: Amount) = above_zero(amount)\n  def above_zero(x: Amount) = x > 0"
);
accepts!(
    detector_def_inside_quantifier_body,
    "exists amount: Amount where amount == 1 { positive(amount) }",
    POSITIVE
);
// Documented SymbolPath spelling; stage[Process](x) is not the syntax.
accepts!(
    detector_qualified_process_stage,
    "exists amount: Claim { R.Claim.stage(amount) == Draft }",
    ""
);

#[test]
fn control_inline_unsatisfiable() {
    assert_conflict("control_inline_unsatisfiable", "amount > 2", "");
}

#[test]
fn detector_def_unsatisfiable_is_a_real_conflict() {
    assert_conflict(
        "detector_def_unsatisfiable_is_a_real_conflict",
        "too_large(amount)",
        "def too_large(x: Amount) = x > 2",
    );
}

#[test]
fn control_lowered_stage_contradicts_event_source() {
    // One instance: the event's Claim is Draft, so no Claim can be Approved.
    assert_conflict(
        "control_lowered_stage_contradicts_event_source",
        "exists c: Claim { claim_stage[c] == Approved }",
        "",
    );
}

#[test]
fn detector_stage_contradicts_event_source() {
    assert_conflict(
        "detector_stage_contradicts_event_source",
        "exists c: Claim { stage(c) == Approved }",
        "",
    );
}

#[test]
fn control_order_free_field_in_def_is_not_respelled() {
    // Main path spells guard arguments, NOT names introduced by expansion.
    // Re-spelling this body would silently accept a different guard.
    assert_main_error(
        "control_order_free_field_in_def_is_not_respelled",
        "positive(1)",
        "def positive(x: Amount) = amount > 0",
        "invalid model expression: public Kernel cannot type identifier 'amount' at 10:3",
    );
}

#[test]
fn control_stage_unknown_process_reports_main_error() {
    assert_main_error(
        "control_stage_unknown_process_reports_main_error",
        "exists x: User { stage(x) == Draft }",
        "type User = 0..0",
        "stage(x) refers to type 'User', which has no process at 12:61",
    );
}

#[test]
fn control_stage_unknown_qualifier_reports_main_error() {
    assert_main_error(
        "control_stage_unknown_qualifier_reports_main_error",
        "exists c: Claim { missing.Claim.stage(c) == Draft }",
        "",
        "process path 'missing.Claim' does not resolve for entity type 'Claim' at 12:62",
    );
}

#[test]
fn control_def_arity_reports_main_error() {
    assert_main_error(
        "control_def_arity_reports_main_error",
        "positive(amount, 1)",
        POSITIVE,
        "predicate 'positive' expects 1 argument(s), got 2 at 12:44",
    );
}

#[test]
fn control_recursive_def_reports_main_error() {
    assert_main_error(
        "control_recursive_def_reports_main_error",
        "positive(amount)",
        "def positive(x: Amount) = positive(x)",
        "recursive predicate definition is not allowed: positive -> positive at 4:3",
    );
}

#[test]
fn control_def_capture_reports_main_error() {
    assert_main_error(
        "control_def_capture_reports_main_error",
        "exists limit: Amount { positive(amount) }",
        "const limit = 1\n  def positive(x: Amount) = x > limit",
        "predicate 'positive' call would capture variable 'limit'; rename the binder at the call site at 13:67",
    );
}

fn round2_assert_enabled(label: &str, text: &str, actions: &[&str]) {
    let outputs = run(label, text);
    for (mode, exit, value) in &outputs {
        let expected = if *mode == "check" { "ok" } else { "verified" };
        assert_eq!(
            (
                *exit,
                value["result"].as_str(),
                value.get("diagnostic_code")
            ),
            (0, Some(expected), None),
            "{label} {mode}: {outputs:#?}"
        );
        if *mode == "verify" {
            assert_eq!(value["checked_to_depth"], 2, "{label}: {value:#}");
            assert_eq!(value["completeness"], "bounded", "{label}: {value:#}");
            for action in actions {
                // A bounded verified verdict alone masks the #1569 blocker.
                // Require the action to execute, not merely an unexercised spec.
                assert_eq!(
                    value["action_coverage"][*action], true,
                    "{label}: action {action} must be covered: {value:#}"
                );
            }
        }
    }
}

fn round2_stage_capture_source(binder: &str) -> String {
    source(
        &format!("exists {binder}: Claim {{ stage({binder}) == Approved and amount == 0 }}"),
        "",
    )
    .replace("instances Claim = 1", "instances Claim = 2")
    .replace(
        "  event approve =",
        "  event seed = Manager seeds Claim Draft -> Approved\n  event approve =",
    )
}

#[test]
fn detector_stage_capture_tracks_renamed_binder() {
    // One claim can be seeded to Approved, leaving another Draft claim to
    // approve at the next step. The source guard is satisfiable. The implicit
    // entity binder is also c, so field substitution must rename the nested c
    // AND its stage operand (#1569). Before #1569, check/verify accept but the
    // action is impossible: Draft(c) and Approved(c) occur in the same guard.
    round2_assert_enabled(
        "detector_stage_capture_tracks_renamed_binder",
        &round2_stage_capture_source("c"),
        &["seed", "approve"],
    );
}

#[test]
fn control_stage_capture_noncolliding_binder() {
    // Same result-class and action-coverage contract without binder collision.
    round2_assert_enabled(
        "control_stage_capture_noncolliding_binder",
        &round2_stage_capture_source("d"),
        &["seed", "approve"],
    );
}

fn round2_explicit_input_source(condition: &str) -> String {
    source(condition, "").replace(
        "event approve = Manager approves Claim Draft -> Approved",
        "event approve = Manager approves Claim with candidate: Claim Draft -> Approved",
    )
}

#[test]
fn detector_stage_explicit_entity_input() {
    // candidate is an explicit Claim input, not the implicit entity binder c.
    // Dropping typed inputs from RuleGuardLowering::condition's environment
    // leaves stage(candidate) unresolved, then the retained Stage fails closed.
    round2_assert_enabled(
        "detector_stage_explicit_entity_input",
        &round2_explicit_input_source("stage(candidate) == Draft"),
        &["approve"],
    );
}

#[test]
fn control_stage_explicit_entity_input_lowered() {
    round2_assert_enabled(
        "control_stage_explicit_entity_input_lowered",
        &round2_explicit_input_source("claim_stage[candidate] == Draft"),
        &["approve"],
    );
}

#[test]
fn detector_order_const_hidden_field_is_not_respelled() {
    // The action can typecheck this def's free amount as a global constant.
    // Re-spelling after expansion in the rule copy would instead impose
    // Claim.amount == 0 and Claim.amount == 1, producing a false conflict.
    assert_accepts(
        "detector_order_const_hidden_field_is_not_respelled",
        "Claim.amount == 0 and positive(1)",
        "const amount = 1\n  def positive(x: Amount) = amount == 1",
    );
}
