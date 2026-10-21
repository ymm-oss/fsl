// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S4, through the native CLI: `actor`, `entity { … lifecycle }`,
//! `concept`, `event`, `scenario` and `forbidden … then rejected` in a document
//! headed `fsl 5`, behind the `fsl5` gate.
//!
//! Acceptance 1 (K) lives in `fsl5_lowering_contract.rs`. This file holds:
//!
//! - **Acceptance 2**: the scenarios AC-1, AC-2 (e2e) and AC-TOOL (rejection)
//!   pass `check`, and moving a scenario's `then` stage fails it. Without the
//!   rules (a later stage), the rejection example's forbidden traces that only
//!   a rule rejects are accepted, so `check` reports them; the one the
//!   lifecycle alone rejects still holds. The rule stage turns the first group
//!   green, which is its own evidence that the rules take effect.
//! - **Acceptance 3**: one failing document per diagnostic of the stage.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture_dir(example: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5")
        .join(example)
}

/// A scratch copy of one fixture example, so `implements … from` resolves.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, example: Option<&str>) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("fsl5_entity_event")
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

/// `fslc check <file>` in `directory` with the `fsl5` gate set.
fn check(directory: &Path, file: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", file])
        .current_dir(directory)
        .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
        .output()
        .expect("run fslc check");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "check {file}: invalid JSON ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

/// `source` keeping, of its `scenario` / `forbidden` blocks, only the one
/// whose id is `keep`. Blocks open at two-space indent and close at `  }`.
fn only_trace(source: &str, keep: &str) -> String {
    let mut kept = String::new();
    let mut skipping = false;
    for line in source.split_inclusive('\n') {
        let opens = ["  scenario ", "  forbidden "]
            .iter()
            .any(|prefix| line.starts_with(prefix));
        if opens {
            skipping = !line.contains(&format!(" {keep} "));
        }
        if !skipping {
            kept.push_str(line);
        }
        if skipping && line == "  }\n" {
            skipping = false;
        }
    }
    assert!(
        kept.contains(&format!(" {keep} ")),
        "trace {keep} is not in the source"
    );
    kept
}

fn trace_source(example: &str, id: &str) -> String {
    let source = std::fs::read_to_string(fixture_dir(example).join("fsl5_en_norules.fsl"))
        .expect("read fsl5 source");
    only_trace(&source, id)
}

/// Acceptance 2: every scenario passes `check` on its own, and the same
/// scenario with its `then` stage moved to another stage fails.
#[test]
fn scenarios_pass_and_a_moved_then_stage_fails() {
    for (example, id, then, moved) in [
        (
            "e2e",
            "AC-1",
            "then claim1 is Paid",
            "then claim1 is Approved",
        ),
        (
            "e2e",
            "AC-2",
            "then claim1 is Rejected",
            "then claim1 is Submitted",
        ),
        (
            "rejection",
            "AC-TOOL",
            "then req1 is ActionExecuted",
            "then req1 is ToolApproved",
        ),
    ] {
        let scratch = Scratch::new(&format!("scenario-{id}"), Some(example));
        let source = trace_source(example, id);
        let file = scratch.write("scenario.fsl", &source);
        let (output, exit) = check(&scratch.0, &file);
        assert_eq!(
            (output["result"].as_str(), exit),
            (Some("ok"), 0),
            "{id}: {output:#}"
        );

        assert_eq!(source.matches(then).count(), 1, "{id}: one `then`");
        let file = scratch.write("moved.fsl", &source.replacen(then, moved, 1));
        let (output, exit) = check(&scratch.0, &file);
        assert_eq!(
            (output["result"].as_str(), output["kind"].as_str(), exit),
            (Some("error"), Some("acceptance"), 2),
            "{id} with `{moved}`: {output:#}"
        );
        assert_eq!(output["id"], id, "{id}: {output:#}");
    }
}

/// Acceptance 2, forbidden side: without the `rule REQ-5` block the traces
/// only its `allow` lines reject are accepted, and `check` reports each one;
/// the trace the lifecycle already rejects (`execute_tool` leaves
/// `ToolApproved` only) holds without any rule.
#[test]
fn forbidden_traces_need_the_rules_except_where_the_lifecycle_rejects() {
    let scratch = Scratch::new("forbidden", Some("rejection"));
    for id in [
        "FB-PUBLIC-APPROVE",
        "FB-EXECUTE-WITH-DOWNGRADED-APPROVAL",
        "FB-EXECUTE-WITH-PUBLIC-ROLE",
    ] {
        let file = scratch.write(&format!("{id}.fsl"), &trace_source("rejection", id));
        let (output, exit) = check(&scratch.0, &file);
        assert_eq!(
            (output["result"].as_str(), output["kind"].as_str(), exit),
            (Some("error"), Some("forbidden"), 2),
            "{id}: {output:#}"
        );
        assert_eq!(output["id"], id, "{id}: {output:#}");
    }
    let id = "FB-EXECUTE-BEFORE-APPROVAL";
    let file = scratch.write(&format!("{id}.fsl"), &trace_source("rejection", id));
    let (output, exit) = check(&scratch.0, &file);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{id}: {output:#}"
    );
}

/// The small entity most negative documents below start from.
fn claim_document(members: &str, verify: &str) -> String {
    format!(
        "fsl 5\nrequirements R {{\n  number Amount\n  actor Employee, Manager\n  \
         entity Claim {{\n    amount: Amount = 0\n    limit: Amount = 0\n    \
         lifecycle Draft -> Submitted -> Approved\n    lifecycle Submitted -> Rejected\n  }}\n\
         {members}}}\nverify {{\n  {verify}\n  values Amount = 0..2\n}}\n"
    )
}

const EVENTS: &str = "  event submit = Employee submits Claim with amount Draft -> Submitted\n  \
                      event approve = Manager approves Claim Submitted -> Approved\n  \
                      event reject = Manager rejects Claim Submitted -> Rejected\n";

fn assert_error(label: &str, source: &str, code: &str, message: &str) {
    let scratch = Scratch::new(label, None);
    let file = scratch.write("doc.fsl", source);
    let (output, exit) = check(&scratch.0, &file);
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

fn warnings(label: &str, source: &str) -> Vec<Value> {
    let scratch = Scratch::new(label, None);
    let file = scratch.write("doc.fsl", source);
    let (output, exit) = check(&scratch.0, &file);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{label}: {output:#}"
    );
    // None of these small documents declares an invariant, so `check` also
    // reports `no_user_invariants`; it is not this stage's.
    output["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|warning| warning["kind"] != "no_user_invariants")
        .cloned()
        .collect()
}

/// The documents the negatives below change: they check clean, without a
/// warning, so each negative fails for its own line only.
#[test]
fn the_negative_examples_start_from_a_clean_document() {
    let source = claim_document(
        &format!(
            "{EVENTS}  scenario AC \"t\" {{\n    Employee submits c1 with amount 1\n    \
             Manager approves c1\n    then c1 is Approved\n  }}\n"
        ),
        "instances Claim = 1",
    );
    assert_eq!(warnings("clean", &source), Vec::<Value>::new());
}

/// decisions/046 Q2: the process form is not part of a `fsl 5` requirements
/// document.
#[test]
fn a_process_in_fsl5_requirements_is_mixed_requirements_forms() {
    assert_error(
        "mixed-forms",
        "fsl 5\nrequirements R {\n  process Claim {\n    stages Draft, Done\n    initial Draft\n    \
         transition finish Draft -> Done by Employee\n  }\n}\nverify {\n  instances Claim = 1\n}\n",
        "mixed_requirements_forms",
        "fslc migrate --edition 5",
    );
}

/// decisions/046 Q6: a bare name that is both an input of the event and a
/// field of its entity is an error; `input amount` and `Claim.amount` say
/// which is meant.
#[test]
fn a_bare_input_and_field_name_is_ambiguous_input_field() {
    let event = |set: &str| {
        claim_document(
            &format!(
                "  event submit = Employee submits Claim with amount Draft -> Submitted\n    \
                 set limit = {set}\n"
            ),
            "instances Claim = 1",
        )
    };
    assert_error(
        "ambiguous-input",
        &event("amount"),
        "ambiguous_input_field",
        "input amount",
    );
    for explicit in [
        "input amount",
        "Claim.amount",
        "input amount + Claim.amount",
    ] {
        let _ = warnings("explicit-input", &event(explicit));
    }
}

/// decisions/049 D6: every actor of an event or a scenario step is declared.
#[test]
fn an_undeclared_actor_is_actor_not_declared() {
    assert_error(
        "actor-event",
        &claim_document(
            "  event submit = Clerk submits Claim Draft -> Submitted\n",
            "instances Claim = 1",
        ),
        "actor_not_declared",
        "'Clerk'",
    );
    assert_error(
        "actor-step",
        &claim_document(
            &format!(
                "{EVENTS}  scenario AC \"t\" {{\n    Clerk submits c1 with amount 1\n    \
                 then c1 is Submitted\n  }}\n"
            ),
            "instances Claim = 1",
        ),
        "actor_not_declared",
        "'Clerk'",
    );
}

#[test]
fn a_step_with_an_undeclared_verb_is_unknown_verb_with_the_nearest_one() {
    assert_error(
        "unknown-verb",
        &claim_document(
            &format!(
                "{EVENTS}  scenario AC \"t\" {{\n    Employee submit c1 with amount 1\n    \
                 then c1 is Submitted\n  }}\n"
            ),
            "instances Claim = 1",
        ),
        "unknown_verb",
        "did you mean 'submits'",
    );
}

#[test]
fn a_step_no_event_matches_is_unknown_event_phrase() {
    assert_error(
        "unknown-phrase",
        &claim_document(
            &format!(
                "{EVENTS}  scenario AC \"t\" {{\n    Employee submits c1 with amount 1\n    \
                 Employee approves c1\n    then c1 is Approved\n  }}\n"
            ),
            "instances Claim = 1",
        ),
        "unknown_event_phrase",
        "'Employee approves'",
    );
}

/// Two events with the same phrase that both leave the instance's current
/// stage: the step does not say which one it takes.
#[test]
fn a_step_two_events_take_from_the_same_stage_is_ambiguous_step() {
    assert_error(
        "ambiguous-step",
        &claim_document(
            "  event submit = Employee submits Claim with amount Draft -> Submitted\n  \
             event approve = Manager decides Claim Submitted -> Approved\n  \
             event reject = Manager decides Claim Submitted -> Rejected\n  \
             scenario AC \"t\" {\n    Employee submits c1 with amount 1\n    \
             Manager decides c1\n    then c1 is Approved\n  }\n",
            "instances Claim = 1",
        ),
        "ambiguous_step",
        "approve, reject",
    );
}

#[test]
fn a_move_off_the_lifecycle_is_event_not_on_lifecycle() {
    assert_error(
        "off-lifecycle",
        &claim_document(
            "  event submit = Employee submits Claim Draft -> Approved\n",
            "instances Claim = 1",
        ),
        "event_not_on_lifecycle",
        "Draft -> Approved",
    );
}

#[test]
fn a_lifecycle_edge_no_event_moves_along_is_a_warning() {
    let found = warnings(
        "edge-without-event",
        &claim_document(
            "  event submit = Employee submits Claim with amount Draft -> Submitted\n  \
             event approve = Manager approves Claim Submitted -> Approved\n",
            "instances Claim = 1",
        ),
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0]["kind"], "lifecycle_edge_without_event");
    assert!(
        found[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Submitted -> Rejected")),
        "{found:#?}"
    );
}

#[test]
fn more_scenario_instances_than_the_bound_is_scenario_instance_overflow() {
    assert_error(
        "instance-overflow",
        &claim_document(
            &format!(
                "{EVENTS}  scenario AC \"t\" {{\n    Employee submits c1 with amount 1\n    \
                 Employee submits c2 with amount 1\n    then c2 is Submitted\n  }}\n"
            ),
            "instances Claim = 1",
        ),
        "scenario_instance_overflow",
        "'c2'",
    );
}

/// decisions/050: the names an entity lowers to come from `fsl_core::naming`,
/// which also refuses one that is already declared.
#[test]
fn a_declared_name_an_entity_generates_is_generated_name_collision() {
    for (label, declaration, name) in [
        (
            "collision-enum",
            "  enum ClaimStage { Open }\n",
            "'ClaimStage'",
        ),
        (
            "collision-const",
            "  const claim_amount = 1\n",
            "'claim_amount'",
        ),
    ] {
        assert_error(
            label,
            &claim_document(&format!("{declaration}{EVENTS}"), "instances Claim = 1"),
            "generated_name_collision",
            name,
        );
    }
}

#[test]
fn an_actor_spelled_as_an_enum_member_is_a_warning() {
    let found = warnings(
        "actor-member",
        &claim_document(
            &format!("  enum Role {{ Employee, Guest }}\n{EVENTS}"),
            "instances Claim = 1",
        ),
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0]["kind"], "actor_shadows_member");
    assert!(
        found[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("'Employee'") && message.contains("Role")),
        "{found:#?}"
    );
}
