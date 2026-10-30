// SPDX-License-Identifier: Apache-2.0
//! #1561: source-field freshness is checked before spelling or substitution.

use std::{path::Path, process::Command};

use serde_json::Value;

#[path = "support/pattern_field.rs"]
mod pattern_field;

fn document(statement: &str, declarations: &str) -> String {
    format!(
        "fsl 5\nrequirements R {{\n  number Amount\n  actor Manager\n  entity Claim {{\n    amount: Amount = 0\n    lifecycle Draft -> Approved\n  }}\n  event approve = Manager approves Claim Draft -> Approved\n  {declarations}\n  rule REQ-1 \"Eligible\" {{\n    {statement}\n  }}\n}}\nverify {{ instances Claim = 1 values Amount = 0..2 }}\n"
    )
}

fn allow(condition: &str, declarations: &str) -> String {
    document(
        &format!("allow Manager approves Claim only when {condition}"),
        declarations,
    )
}

fn run(label: &str, source: &str, args: &[&str]) -> (i32, Value) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1561_pattern_field_shadow")
        .join(std::process::id().to_string())
        .join(label);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("case.fsl"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(args[0])
        .arg("case.fsl")
        .args(&args[1..])
        .current_dir(&dir)
        .env("FSLC_UNSTABLE", "fsl5")
        .env("FSLC_CACHE", "off")
        .output()
        .unwrap();
    assert!(output.stderr.is_empty(), "{label}: {output:?}");
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

fn refusal(label: &str, source: &str, name: &str, owner: &str) {
    let (line, text) = source
        .lines()
        .enumerate()
        .find(|(_, text)| text.trim_start().starts_with(owner))
        .unwrap();
    let line = u64::try_from(line + 1).unwrap();
    let column = u64::try_from(text.len() - text.trim_start().len() + 1).unwrap();
    let reference = run(label, source, &["check"]);
    pattern_field::assert_refused(reference.0, &reference.1, name, "case.fsl", line, column);
    for engine in ["bmc", "induction", "explicit"] {
        let result = run(
            label,
            source,
            &["verify", "--engine", engine, "--depth", "2", "--no-cache"],
        );
        assert_eq!(result, reference, "{label} {engine}: full refusal packet");
    }
}

fn accepts(label: &str, source: &str) {
    let (exit, report) = run(label, source, &["check"]);
    assert_eq!(
        (exit, report["result"].as_str()),
        (0, Some("ok")),
        "{label}: {report:#}"
    );
}

#[test]
fn detector_all_expression_children_keep_source_field_names() {
    let cases = [
        "some(1) is some(amount) and amount == 1",
        "true and (some(1) is some(amount) and amount == 1)",
        "(some(1) is some(amount)) => amount == 1",
        "(some(1) is some(amount)) or true",
        "not (optional is some(amount))",
        "if true then (some(1) is some(amount)) else false",
        "if (some(1) is some(amount)) then true else false",
        "if false then false else (some(1) is some(amount))",
        "Set { some(1) is some(amount) }.contains(true)",
        "exists x: Amount { some(x) is some(amount) and amount == 1 }",
        "exists x: Amount where some(x) is some(amount) { true }",
        "sum(x in 0..1 of if (some(x) is some(amount)) then 1 else 0) >= 0",
        "count(x: Amount where some(x) is some(amount)) > 0",
    ];
    for (i, condition) in cases.iter().enumerate() {
        let declarations = "state { optional: Option<Amount> } init { optional = none }";
        let source = allow(condition, declarations);
        refusal(
            &format!("expression-{i}"),
            &source,
            "amount",
            "allow Manager",
        );
        // Change only the binding/read spelling, not a field or the condition.
        accepts(
            &format!("expression-{i}-fresh"),
            &allow(&condition.replace("amount", "v"), declarations),
        );
    }
}

#[test]
fn detector_legacy_guard_and_assignment_keep_transition_owner() {
    for clause in [
        "when some(1) is some(amount) and amount == 1",
        "set amount = if (some(1) is some(amount)) then 1 else 0",
    ] {
        let source = format!(
            "requirements R {{\n  number Amount\n  process Claim with amount: Amount = 0 {{\n    stages Draft, Approved\n    initial Draft\n    transition approve Draft -> Approved by Manager {clause}\n  }}\n}}\nverify {{ instances Claim = 1 values Amount = 0..2 }}\n"
        );
        refusal(
            clause.split_whitespace().next().unwrap(),
            &source,
            "amount",
            "transition approve",
        );
        let repaired = source
            .replace("is some(amount)", "is some(v)")
            .replace("and amount == 1", "and v == 1");
        accepts(&format!("legacy-fresh-{}", clause.len()), &repaired);
    }
}

#[test]
fn detector_next_event_property_measure_and_relation_roles() {
    let condition = "some(1) is some(amount) and amount == 1";
    let never = document(
        &format!("never Manager approves Claim when {condition}"),
        "",
    );
    refusal("never", &never, "amount", "never Manager");
    let assignment = allow("true", "").replace("event approve = Manager approves Claim Draft -> Approved\n", "event approve = Manager approves Claim Draft -> Approved set amount = if (some(1) is some(amount)) then 1 else 0\n");
    refusal("event-set", &assignment, "amount", "event approve");
    accepts(
        "event-set-fresh",
        &assignment.replace("is some(amount)", "is some(v)"),
    );
    let concept = format!("concept Eligible = Claim where {condition}");
    let property = document("always Draft Claim is Eligible", &concept);
    refusal("property", &property, "amount", "always Draft");
    accepts(
        "property-fresh",
        &property.replace(condition, "some(1) is some(v) and v == 1"),
    );
    let measure = allow(
        "true",
        &format!("{concept}\n  measure EligibleCount = count Eligible"),
    );
    refusal("measure", &measure, "amount", "measure EligibleCount");
    accepts(
        "measure-fresh",
        &measure.replace(condition, "some(1) is some(v) and v == 1"),
    );
    let role = allow(
        "some(owner) is some(owner) and owner == none",
        "entity Manager\n  Claim belongs to Manager as owner via approve",
    )
    .replace(
        "instances Claim = 1",
        "instances Claim = 1 instances Manager = 1",
    );
    refusal("role", &role, "owner", "allow Manager");
    accepts(
        "role-fresh",
        &role.replace(
            "is some(owner) and owner == none",
            "is some(v) and v == none",
        ),
    );
}

#[test]
fn detector_authored_arguments_are_checked_before_defs_erase_them() {
    for body in ["p", "p and p", "true", "ignore(p)"] {
        let definitions = format!("def ignore(p: Bool) = true\n  def target(p: Bool) = {body}");
        let source = allow(
            "target(some(1) is some(amount) and amount == 1)",
            &definitions,
        );
        refusal(
            &format!("argument-{}", body.len()),
            &source,
            "amount",
            "allow Manager",
        );
        let repaired = source.replace("is some(amount) and amount == 1", "is some(v) and v == 1");
        if body == "p and p" {
            // Expansion duplicates the match: renaming a field does not grant
            // permission to reuse the fresh binding. Preserve the existing error.
            let (exit, report) = run("argument-duplicate-fresh", &repaired, &["check"]);
            assert_eq!(exit, 2, "{report:#}");
            assert_eq!(
                report["message"],
                "invalid model expression: pattern binding 'v' in 'is some(v)' shadows a name already in scope (an action parameter, quantifier binder, let, or earlier pattern binding) of the same name; rename the pattern variable at 9:3",
                "{report:#}"
            );
        } else {
            accepts(&format!("argument-fresh-{}", body.len()), &repaired);
        }
    }
}

#[test]
fn preservation_def_local_patterns_are_not_field_substitutions() {
    // Definition-local names enter only after field substitution. They are not
    // source field aliases; both event and rule-copy expansion must read payload.
    let source = allow(
        "matches(1)",
        "def matches(x: Amount) = some(x) is some(amount) and amount == 1",
    );
    accepts("def-local", &source);
    let (exit, report) = run(
        "def-local",
        &source,
        &["verify", "--depth", "2", "--no-cache"],
    );
    assert_eq!(exit, 0, "{report:#}");
    assert_eq!(report["action_coverage"]["approve"], true, "{report:#}");
}

#[test]
fn preservation_fresh_kernel_binding_and_unrelated_common_action() {
    let source = allow(
        "some(1) is some(v) and v == 1",
        "action unrelated() { requires some(1) is some(amount) and amount == 1 }",
    );
    accepts("fresh", &source);
    let (exit, report) = run("fresh", &source, &["kernel"]);
    assert_eq!(exit, 0, "{report:#}");
    let action = report["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "approve")
        .unwrap();
    assert_eq!(
        action["requires"][1]["right"]["left"]["name"], "v",
        "{action:#}"
    );
    let (exit, report) = run("fresh", &source, &["verify", "--depth", "2", "--no-cache"]);
    assert_eq!(exit, 0, "{report:#}");
    assert_eq!(report["action_coverage"]["approve"], true, "{report:#}");
}
