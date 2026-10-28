// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1552: a rule reads an entity field in every expression form, while event
//! inputs and quantifier/aggregate binders shadow that field. Detectors intentionally
//! fail on the pre-fix implementation; preservation controls are separate.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const LOOKUP: &str = "state { lookup: Map<Amount, Amount> }
  init { forall i: Amount { lookup[i] = i } }";
const BOX: &str = "struct Box { value: Amount }";
const ZERO_SET: &str = "state { zeros: Set<Amount> } init { zeros = Set { 0 } }";
const ZERO_SEQ: &str = "state { zeros: Seq<Amount, 1> } init { zeros = Seq { 0 } }";
const ZERO_BOX: &str = "struct Box { value: Amount }
  state { zero: Box } init { zero = Box { value: 0 } }";
const LINKS: &str = "state { links: relation Amount -> Amount } init { links = Set {} }";
const SETS: &str = "state { positives: Set<Amount>, zeros: Set<Amount> }
  init { positives = Set { 1, 2 } zeros = Set { 0 } }";

fn source(condition: &str, declarations: &str, input: bool) -> String {
    let condition = if input {
        condition.replace("amount", "candidate")
    } else {
        condition.to_owned()
    };
    let input = if input { "with candidate: Amount " } else { "" };
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
  event approve = Manager approves Claim {input}Draft -> Approved
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

/// Collect both commands before asserting, so a check-time rejection cannot
/// hide what verify produced. Retain complete JSON and stderr for diagnosis.
fn run(label: &str, text: &str) -> Vec<(&'static str, i32, Value)> {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1552_rule_guard_forms")
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
            let status = output.status.code().expect("process exit code");
            std::fs::write(
                directory.join(format!("{mode}.json")),
                serde_json::to_vec_pretty(&json!({
                    "exit": status,
                    "output": value,
                    "stderr": String::from_utf8_lossy(&output.stderr),
                }))
                .expect("serialize complete observation"),
            )
            .expect("retain observation");
            assert!(output.stderr.is_empty(), "{label} {mode}: {output:?}");
            (mode, status, value)
        })
        .into_iter()
        .collect()
}

fn assert_accepts(label: &str, condition: &str, declarations: &str, input: bool) {
    let outputs = run(label, &source(condition, declarations, input));
    for (mode, exit, value) in &outputs {
        let expected = if *mode == "check" { "ok" } else { "verified" };
        assert_eq!(
            (*exit, value["result"].as_str()),
            (0, Some(expected)),
            "{label}: expected {mode}={expected}/0; complete observations: {outputs:#?}"
        );
        if *mode == "verify" {
            assert_eq!(value["checked_to_depth"], 2, "{label}: {value:#}");
            assert_eq!(value["completeness"], "bounded", "{label}: {value:#}");
        }
    }
}

// Each reachable form first has a distinct event-input preservation control.
// That proves its operators/types can be checked and verified without fixing
// #1552. Only then does the detector remove the input and require the entity
// field to be spelled.
macro_rules! detector {
    ($name:ident, $condition:expr, $declarations:expr) => {
        #[test]
        fn $name() {
            assert_accepts(
                concat!(stringify!($name), "-input-control"),
                $condition,
                $declarations,
                true,
            );
            assert_accepts(stringify!($name), $condition, $declarations, false);
        }
    };
}

detector!(detector_abs, "abs(amount) > 0", "");
detector!(detector_min_left, "min(amount, 1) > 0", "");
detector!(detector_min_right, "min(1, amount) > 0", "");
detector!(detector_max_left, "max(amount, 1) > 1", "");
detector!(detector_max_right, "max(1, amount) > 1", "");
detector!(detector_index, "lookup[amount] > 0", LOOKUP);
detector!(detector_field, "(Box { value: amount }).value > 0", BOX);
detector!(detector_method_argument, "positives.contains(amount)", SETS);
detector!(
    detector_method_receiver,
    "(if amount > 0 then positives else zeros).contains(1)",
    SETS
);
detector!(detector_some, "some(amount) != some(0)", "");
detector!(detector_set, "Set { amount } != zeros", ZERO_SET);
detector!(detector_seq, "Seq { amount } != zeros", ZERO_SEQ);
detector!(detector_struct, "Box { value: amount } != zero", ZERO_BOX);
detector!(
    detector_is_operand,
    "some(amount) is some(value) and value > 0",
    ""
);
detector!(
    detector_quantified_typed,
    "exists x: Amount { x == amount and x > 0 }",
    ""
);
detector!(
    detector_quantified_range,
    "exists x in 0..2 { x == amount and x > 0 }",
    ""
);
detector!(
    detector_quantified_collection,
    "exists x in Set { amount } { x > 0 }",
    ""
);
detector!(
    detector_aggregate_typed,
    "count(x: Amount where x == amount and x > 0) > 0",
    ""
);
detector!(detector_aggregate_range, "sum(x in 0..2 of amount) > 0", "");
detector!(
    detector_aggregate_collection,
    "sum(x in Set { amount } of x) > 0",
    ""
);
detector!(
    detector_aggregate_unique,
    "unique(x: Amount where x <= amount)",
    ""
);
detector!(
    detector_aggregate_exactly_one,
    "exactlyOne(x: Amount where x == amount and x > 0)",
    ""
);
detector!(
    detector_ternary_named,
    "not reachable(links, amount, 0)",
    LINKS
);

macro_rules! control {
    ($name:ident, $condition:expr) => {
        control!($name, $condition, "");
    };
    ($name:ident, $condition:expr, $declarations:expr) => {
        #[test]
        fn $name() {
            assert_accepts(stringify!($name), $condition, $declarations, false);
        }
    };
}

// Already-recursed forms: Var/Binary (bare), Not, Neg and all three branches
// of Conditional. These must remain check=ok and verify=verified, depth 2.
control!(control_bare_var_binary, "amount > 0");
control!(control_not, "not (amount <= 0)");
control!(control_neg, "-amount < 0");
control!(
    control_conditional,
    "if amount > 0 then amount > 0 else amount > 1"
);

// The outer field is fixed to zero, but each bound amount must be one. If a
// binder is incorrectly spelled Claim.amount, these guards become impossible.
control!(
    control_quantified_shadow_typed,
    "amount == 0 and (exists amount: Amount where amount == 1 { amount == 1 })"
);
control!(
    control_quantified_shadow_range,
    "amount == 0 and (exists amount in 1..1 where amount == 1 { amount == 1 })"
);
control!(
    control_quantified_shadow_collection,
    "amount == 0 and (exists amount in Set { 1 } where amount == 1 { amount == 1 })"
);
control!(
    control_aggregate_shadow_typed,
    "amount == 0 and count(amount: Amount where amount == 1) == 1"
);
control!(
    control_aggregate_shadow_range,
    "amount == 0 and sum(amount in 1..1 of amount where amount == 1) == 1"
);
control!(
    control_aggregate_shadow_collection,
    "amount == 0 and sum(amount in ones of amount) == 1",
    "state { ones: Set<Amount> } init { ones = Set { 1 } }"
);

#[test]
fn control_is_pattern_field_name_matches_lowering() {
    // The guard copy follows the action's lowering: a pattern named `amount`
    // does not hide that entity field in the continuation. Both guards are
    // impossible field constraints and must be rejected, just as on base.
    // The typecheck/lowering discrepancy is tracked separately in #1561.
    let outputs = [
        (
            "control_is_pattern_field_name_matches_lowering-and",
            "amount == 0 and (some(1) is some(amount) and amount == 1)",
        ),
        (
            "control_is_pattern_field_name_matches_lowering-implication",
            "amount == 1 and (some(0) is some(amount) => amount == 0)",
        ),
    ]
    .map(|(label, condition)| run(label, &source(condition, "", false)));
    for (mode, exit, value) in outputs.iter().flatten() {
        assert_eq!(*exit, 2, "{mode}: {outputs:#?}");
        assert_eq!(value["result"], "error", "{mode}: {value:#}");
        assert_eq!(value["kind"], "semantics", "{mode}: {value:#}");
        assert_eq!(
            value["diagnostic_code"], "rule_conflict_unsatisfiable_guard",
            "{mode}: {value:#}"
        );
        assert!(
            value["message"]
                .as_str()
                .expect("message")
                .contains("event 'approve' can never occur"),
            "{mode}: {value:#}"
        );
    }
}

#[test]
fn control_is_pattern_field_name_satisfiable_matches_base() {
    // #1561: lowering still reads the field, which can equal the payload here.
    // Preserve base's accepted result classes; this does not establish that
    // pattern binding and field lookup have the same meaning.
    let outputs = run(
        "control_is_pattern_field_name_satisfiable_matches_base",
        &source("some(1) is some(amount) and amount == 1", "", false),
    );
    for (mode, exit, value) in &outputs {
        let expected = if *mode == "check" { "ok" } else { "verified" };
        assert_eq!(
            (
                *exit,
                value["result"].as_str(),
                value.get("diagnostic_code")
            ),
            (0, Some(expected), None),
            "{mode}: {outputs:#?}"
        );
        if *mode == "verify" {
            assert_eq!(value["checked_to_depth"], 2, "{mode}: {value:#}");
            assert_eq!(value["completeness"], "bounded", "{mode}: {value:#}");
        }
    }
}

#[test]
fn control_is_pattern_in_quantifier_body_matches_lowering() {
    // Base accepted this although `approve` can never occur (a false ok).
    // This is the one intended accepted-to-rejected change: the rule check now
    // reads the field in the quantifier body as the action lowering does (#1561).
    let outputs = run(
        "control_is_pattern_in_quantifier_body_matches_lowering",
        &source(
            "amount == 0 and (exists x: Amount where x == 1 { some(x) is some(amount) and amount == 1 })",
            "",
            false,
        ),
    );
    for (mode, exit, value) in &outputs {
        assert_eq!(*exit, 2, "{mode}: {outputs:#?}");
        assert_eq!(value["result"], "error", "{mode}: {value:#}");
        assert_eq!(value["kind"], "semantics", "{mode}: {value:#}");
        assert_eq!(
            value["diagnostic_code"], "rule_conflict_unsatisfiable_guard",
            "{mode}: {value:#}"
        );
        assert!(
            value["message"]
                .as_str()
                .expect("message")
                .contains("event 'approve' can never occur"),
            "{mode}: {value:#}"
        );
    }
}

#[test]
fn control_unsatisfiable_bare_guard_is_rejected() {
    let outputs = run(
        "control_unsatisfiable_bare_guard",
        &source("amount > 2", "", false),
    );
    for (mode, exit, value) in &outputs {
        assert_eq!(*exit, 2, "{mode}: {outputs:#?}");
        assert_eq!(value["result"], "error", "{mode}: {value:#}");
        assert_eq!(value["kind"], "semantics", "{mode}: {value:#}");
        assert_eq!(
            value["diagnostic_code"], "rule_conflict_unsatisfiable_guard",
            "{mode}: {value:#}"
        );
        assert!(
            value["message"]
                .as_str()
                .expect("message")
                .contains("event 'approve' can never occur"),
            "{mode}: {value:#}"
        );
    }
}

#[test]
fn control_known_separate_defect_unexpanded_call_is_fail_closed() {
    // Not a #1552 detector: RuleGuard.condition keeps the call even though
    // the generated action's expression is def-expanded. Field spelling alone
    // cannot fix that missing expansion. Keep its current rejection explicit
    // until #1556 flips this control; cover Call in unit tests.
    let outputs = run(
        "control_known_separate_defect_unexpanded_call",
        &source("positive(amount)", "def positive(x: Amount) = x > 0", false),
    );
    let expected = "the rule_conflict_unsatisfiable_guard check could not be decided by the solver \
        (enumeration skipped it because its condition uses an expression the enumeration does not evaluate): \
        unexpanded predicate call 'positive'";
    for (mode, exit, value) in &outputs {
        assert_eq!(*exit, 2, "{mode}: {outputs:#?}");
        assert_eq!(value["result"], "error", "{mode}: {value:#}");
        assert_eq!(value["kind"], "semantics", "{mode}: {value:#}");
        assert_eq!(
            value["diagnostic_code"], "rule_conflict_unsatisfiable_guard",
            "{mode}: {value:#}"
        );
        assert_eq!(value["message"], expected, "{mode}: {value:#}");
    }
}

#[test]
fn control_known_separate_defect_unlowered_stage_is_fail_closed() {
    // Not a #1552 detector: the rule obligation keeps surface stage access
    // even though the generated action's expression is lowered. Field spelling
    // cannot fix that missing lowering. Keep its current rejection explicit
    // until #1556 flips this control; cover Stage in unit tests.
    let outputs = run(
        "control_known_separate_defect_unlowered_stage",
        &source("exists amount: Claim { stage(amount) == Draft }", "", false),
    );
    let expected = "the rule_conflict_unsatisfiable_guard check could not be decided by the solver \
        (enumeration skipped it because its condition uses an expression the enumeration does not evaluate): \
        unlowered stage access";
    for (mode, exit, value) in &outputs {
        assert_eq!(*exit, 2, "{mode}: {outputs:#?}");
        assert_eq!(value["result"], "error", "{mode}: {value:#}");
        assert_eq!(value["kind"], "semantics", "{mode}: {value:#}");
        assert_eq!(
            value["diagnostic_code"], "rule_conflict_unsatisfiable_guard",
            "{mode}: {value:#}"
        );
        assert_eq!(value["message"], expected, "{mode}: {value:#}");
    }
}
