// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1438/#1441: auto-mapped fsl 4 processes and fsl 5 events preserve source
//! actors across editions, standalone mappings and compose, including events
//! used by scenario steps, rules and decisions. Requirement
//! annotations on either side must not replace the actor's semantic identity.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const EXPENSE_BUSINESS: &str = include_str!("fixtures/fsl5/e2e/1_business.fsl");

fn fixture(label: &str, source: &str, business: &str) -> PathBuf {
    // Keep the inputs for diagnosis; each process and control has its own path.
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fsl5_actor_refinement")
        .join(std::process::id().to_string())
        .join(label);
    std::fs::create_dir_all(&directory).expect("create fixture directory");
    std::fs::write(directory.join("business.fsl"), business).expect("write business");
    std::fs::write(directory.join("requirements.fsl"), source).expect("write requirements");
    directory
}

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

fn assert_actor_mismatch(label: &str, source: &str, business: &str, action: &str) {
    let directory = fixture(label, source, business);
    let (output, exit) = fslc(&directory, &["check", "requirements.fsl"]);
    let expected = format!(
        "maps auto actor mismatch for action '{action}': impl actor 'Manager' != abstract actor 'System'"
    );
    assert_eq!(
        exit, 2,
        "{label}: expected error/type with {expected:?}; produced result={} kind={} message={}",
        output["result"], output["kind"], output["message"]
    );
    assert_eq!(output["result"], "error", "{label}: {output:#}");
    assert_eq!(output["kind"], "type", "{label}: {output:#}");
    assert!(
        output["message"]
            .as_str()
            .expect("diagnostic message")
            .contains(&expected),
        "{label}: {output:#} lacks {expected:?}"
    );
}

fn expense_source(actor: &str, covers: &str) -> String {
    format!(
        r#"fsl 5
requirements R5 {{
  implements ExpenseToBe from "business.fsl" {{ }}
  actor Employee, System, Manager, Finance
  entity Claim {{
    lifecycle Draft -> Submitted -> Approved
    lifecycle Submitted -> Rejected
    lifecycle Approved -> Paid
  }}
  event submit       = Employee submits Claim Draft -> Submitted
  event auto_approve = {actor} approves Claim Submitted -> Approved
    {covers}
  event mgr_approve  = Manager approves Claim Submitted -> Approved
  event reject       = Manager rejects Claim Submitted -> Rejected
  event pay          = Finance pays Claim Approved -> Paid
}}
verify {{ instances Claim = 3 }}
"#
    )
}

fn business_source(covers: &str) -> String {
    format!(
        r"business Abs {{
  actor System, Manager
  entity Case
  process Case {{
    stages Open, Done
    initial Open
    transition approve Open -> Done by System
      {covers}
  }}
}}
verify {{ instances Case = 1 }}
"
    )
}

fn case_source(member: &str) -> String {
    format!(
        r#"fsl 5
requirements Impl {{
  implements Abs from "business.fsl" {{ }}
  actor System, Manager
  entity Case {{ lifecycle Open -> Done }}
  event approve = Manager approves Case Open -> Done
  {member}
}}
verify {{ instances Case = 1 }}
"#
    )
}

// A1 detector: the issue's negative control must fail at check time, not refine.
#[test]
fn event_with_wrong_actor_is_a_check_time_error() {
    assert_actor_mismatch(
        "event",
        &expense_source("Manager", ""),
        EXPENSE_BUSINESS,
        "auto_approve",
    );
}

// A2 preservation: changing only the actor makes check and the inline seam pass.
#[test]
fn event_with_correct_actor_checks_and_refines() {
    let directory = fixture(
        "correct_actor",
        &expense_source("System", ""),
        EXPENSE_BUSINESS,
    );
    for (args, expected_result) in [
        (vec!["check", "requirements.fsl"], "ok"),
        (
            vec!["verify", "requirements.fsl", "--depth", "6"],
            "verified",
        ),
    ] {
        let (output, exit) = fslc(&directory, &args);
        assert_eq!(exit, 0, "{args:?}: {output:#}");
        assert_eq!(output["result"], expected_result, "{args:?}: {output:#}");
        assert_eq!(output["implements"]["result"], "refines", "{output:#}");
    }
}

// A3 detectors: these forms consume or guard an event, not a separate action.
#[test]
fn scenario_step_event_with_wrong_actor_is_a_check_time_error() {
    assert_actor_mismatch(
        "step",
        &case_source(
            r#"scenario AC-1 "Approval step" {
    Manager approves case1
    then case1 is Done
  }"#,
        ),
        &business_source(""),
        "approve",
    );
}

#[test]
fn rule_event_with_wrong_actor_is_a_check_time_error() {
    assert_actor_mismatch(
        "rule",
        &case_source(
            r#"rule REQ-1 "Approval authority" {
    allow Manager approves Case only when true
  }"#,
        ),
        &business_source(""),
        "approve",
    );
}

#[test]
fn decide_event_with_wrong_actor_is_a_check_time_error() {
    assert_actor_mismatch(
        "decide",
        &case_source(
            r#"decide REQ-1 "Approval authority" for Open Case {
    otherwise => Manager approves
  }"#,
        ),
        &business_source(""),
        "approve",
    );
}

// A4 preservation: the fsl 4 process form already rejects this mismatch.
#[test]
fn fsl4_process_actor_mismatch_diagnostic_is_preserved() {
    assert_actor_mismatch(
        "fsl4_process",
        r#"requirements Impl {
  implements Abs from "business.fsl" { }
  process Case {
    stages Open, Done
    initial Open
    transition approve Open -> Done by Manager
  }
}
verify { instances Case = 1 }
"#,
        &business_source(""),
        "approve",
    );
}

// A5 detector: an authored requirement is not the implementation actor.
#[test]
fn event_covers_does_not_hide_actor_mismatch() {
    assert_actor_mismatch(
        "event_covers",
        &expense_source("Manager", r#"covers REQ-1 "Approval authority""#),
        EXPENSE_BUSINESS,
        "auto_approve",
    );
}

// A6 detector: the business actor must also come from typed source information.
#[test]
fn business_covers_does_not_hide_fsl5_event_actor_mismatch() {
    assert_actor_mismatch(
        "business_covers",
        &case_source(""),
        &business_source(r#"covers BIZ-1 "Approval authority""#),
        "approve",
    );
}

// Keep the original controls/helpers above unchanged. The additional command
// routes share an exact diagnostic assertion, not just a message substring.
fn assert_exact_actor_mismatch(directory: &Path, args: &[&str], action: &str) {
    let (output, exit) = fslc(directory, args);
    let expected = format!(
        "maps auto actor mismatch for action '{action}': impl actor 'Manager' != abstract actor 'System'"
    );
    assert_eq!(
        (
            exit,
            output["result"].as_str(),
            output["kind"].as_str(),
            output["message"].as_str(),
        ),
        (2, Some("error"), Some("type"), Some(expected.as_str())),
        "{args:?}: {output:#}"
    );
    assert!(output.get("diagnostic_code").is_none(), "{output:#}");
}

#[test]
fn fault_event_with_wrong_actor_is_a_check_time_error() {
    let source = case_source("").replace("event approve =", "fault event approve =");
    let directory = fixture("fault_event", &source, &business_source(""));
    assert_exact_actor_mismatch(&directory, &["check", "requirements.fsl"], "approve");
}

#[test]
fn in_stage_event_with_wrong_actor_is_a_check_time_error() {
    let source = case_source("").replace("Case Open -> Done", "Case in Open");
    let directory = fixture("in_stage_event", &source, &business_source(""));
    assert_exact_actor_mismatch(&directory, &["check", "requirements.fsl"], "approve");
}

#[test]
fn explicit_maps_auto_checks_event_actor() {
    let source = case_source("").replace(
        r#"implements Abs from "business.fsl" { }"#,
        r#"implements Abs from "business.fsl" { maps auto }"#,
    );
    let directory = fixture("explicit_maps_auto", &source, &business_source(""));
    assert_exact_actor_mismatch(&directory, &["check", "requirements.fsl"], "approve");
}

#[test]
fn standalone_refinement_maps_auto_checks_event_actor() {
    let directory = standalone_fixture("standalone_refinement", "Manager", &business_source(""));
    assert_exact_actor_mismatch(&directory, STANDALONE_REFINE, "approve");
}

#[test]
fn glossary_actor_mismatch_names_the_canonical_actor() {
    let source = case_source("")
        .replace(
            "requirements Impl {",
            "requirements Impl {\n  glossary { Manager = 上長 }",
        )
        .replace("= Manager approves", "= 上長 approves");
    let directory = fixture("glossary_actor", &source, &business_source(""));
    assert_exact_actor_mismatch(&directory, &["check", "requirements.fsl"], "approve");
}

#[test]
fn verify_rejects_the_issue_actor_mismatch_before_proof() {
    let directory = fixture(
        "verify_wrong_actor",
        &expense_source("Manager", ""),
        EXPENSE_BUSINESS,
    );
    assert_exact_actor_mismatch(
        &directory,
        &["verify", "requirements.fsl", "--depth", "6"],
        "auto_approve",
    );
}

#[test]
fn event_covers_actor_text_cannot_override_the_declared_actor() {
    let directory = fixture(
        "event_covers_actor_text",
        &expense_source("Manager", r#"covers REQ-1 "by System""#),
        EXPENSE_BUSINESS,
    );
    assert_exact_actor_mismatch(&directory, &["check", "requirements.fsl"], "auto_approve");
}

#[test]
fn business_covers_actor_text_cannot_override_the_declared_actor() {
    let directory = fixture(
        "business_covers_actor_text",
        &case_source(""),
        &business_source(r#"covers BIZ-1 "by Manager""#),
    );
    assert_exact_actor_mismatch(&directory, &["check", "requirements.fsl"], "approve");
}

#[test]
fn explicit_action_correspondence_does_not_check_event_actor() {
    let source = case_source("").replace(
        r#"implements Abs from "business.fsl" { }"#,
        r#"implements Abs from "business.fsl" { action approve(c) -> approve(c) }"#,
    );
    let directory = fixture("explicit_action", &source, &business_source(""));
    let (output, exit) = fslc(&directory, &["check", "requirements.fsl"]);
    assert_eq!(exit, 0, "{output:#}");
    assert_eq!(output["result"], "ok", "{output:#}");
    assert_eq!(output["implements"]["result"], "refines", "{output:#}");
}

const STANDALONE_REFINE: &[&str] = &[
    "refine",
    "requirements.fsl",
    "business.fsl",
    "mapping.fsl",
    "--depth",
    "6",
];

fn standalone_source(name: &str, actor: &str) -> String {
    case_source("")
        .replace("  implements Abs from \"business.fsl\" { }\n", "")
        .replace("requirements Impl", &format!("requirements {name}"))
        .replace("= Manager approves", &format!("= {actor} approves"))
}

fn standalone_fixture(label: &str, actor: &str, abstraction: &str) -> PathBuf {
    let directory = fixture(label, &standalone_source("Impl", actor), abstraction);
    std::fs::write(
        directory.join("mapping.fsl"),
        "refinement Mapping { impl Impl abs Abs maps auto }\n",
    )
    .expect("write standalone refinement");
    directory
}

#[test]
fn event_abstraction_with_wrong_actor_is_a_refinement_type_error() {
    let directory = standalone_fixture(
        "event_abstraction_wrong_actor",
        "Manager",
        &standalone_source("Abs", "System"),
    );
    assert_exact_actor_mismatch(&directory, STANDALONE_REFINE, "approve");
}

#[test]
fn event_abstraction_with_correct_actor_refines() {
    let directory = standalone_fixture(
        "event_abstraction_correct_actor",
        "System",
        &standalone_source("Abs", "System"),
    );
    let (output, exit) = fslc(&directory, STANDALONE_REFINE);
    assert_eq!(exit, 0, "{output:#}");
    assert_eq!(output["result"], "refines", "{output:#}");
}

// #1441 acceptance IDs: covers, cross-edition and mixed-carrier actor checks.
const PROCESS_1441: &str = include_str!("fixtures/actor_refinement/process.fsl");
const BUSINESS_1441: &str = include_str!("fixtures/actor_refinement/business.fsl");
const EVENT_1441: &str = include_str!("fixtures/actor_refinement/event.fsl");
const EXPENSE_1441: &str = include_str!("fixtures/actor_refinement/expense.fsl");
const AUTO_1441: &str = include_str!("fixtures/actor_refinement/auto.fsl");
const INLINE_1441: &str = "  implements Abs from \"business.fsl\" { }\n";
const VERIFY_1441: &[&str] = &["verify", "requirements.fsl", "--depth", "6", "--no-cache"];

fn process_1441(actor: &str, covers: &str) -> String {
    PROCESS_1441.replace("by Manager", &format!("by {actor} {covers}"))
}

fn abstract_process_1441(edition: u8) -> String {
    if edition == 5 {
        return standalone_source("Abs", "System");
    }
    PROCESS_1441
        .replace(INLINE_1441, "")
        .replace("Impl", "Abs")
        .replace("by Manager", "by System")
}

fn inline_event_1441() -> String {
    EVENT_1441.replace(
        "requirements Impl {\n",
        &format!("requirements Impl {{\n{INLINE_1441}"),
    )
}

fn mapping_fixture_1441(label: &str, source: &str, abstraction: &str, mapping: &str) -> PathBuf {
    // Extend #1438's standalone setup for process/compose inputs and located maps.
    let directory = standalone_fixture(label, "Manager", abstraction);
    std::fs::write(directory.join("requirements.fsl"), source).expect("write standalone source");
    std::fs::write(directory.join("mapping.fsl"), mapping).expect("write mapping");
    std::fs::write(
        directory.join("compose-sync-types.fsl"),
        include_str!("fixtures/actor_refinement/compose-sync-types.fsl"),
    )
    .expect("write compose dependency");
    directory
}

fn assert_location_1441(output: &Value, source: &str, marker: &str) {
    let offset = source.find(marker).expect("diagnostic marker");
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().expect("line prefix").len() + 1;
    assert_eq!(
        output["loc"],
        serde_json::json!({"line": line, "column": column}),
        "{output:#}"
    );
    assert_eq!(output["span"]["start"]["offset"], offset, "{output:#}");
    assert_eq!(output["span"]["start"]["line"], line, "{output:#}");
    assert_eq!(output["span"]["start"]["column"], column, "{output:#}");
}

fn mismatch_1441(directory: &Path, args: &[&str], action: &str, source: &str, marker: &str) {
    // Reuse #1438's exact assertion unchanged, then check the located envelope.
    assert_exact_actor_mismatch(directory, args, action);
    let (output, _) = fslc(directory, args);
    assert_location_1441(&output, source, marker);
}

fn success_1441(directory: &Path, args: &[&str], result: &str, inline: bool) {
    let (output, exit) = fslc(directory, args);
    assert_eq!(
        (exit, output["result"].as_str()),
        (0, Some(result)),
        "{args:?}: {output:#}"
    );
    if inline {
        assert_eq!(output["implements"]["result"], "refines", "{output:#}");
    }
}

fn check_and_verify_1441(label: &str, source: &str, business: &str, action: &str, wrong: bool) {
    let directory = fixture(label, source, business);
    for (args, result) in [
        (&["check", "requirements.fsl"][..], "ok"),
        (VERIFY_1441, "verified"),
    ] {
        if wrong {
            mismatch_1441(&directory, args, action, source, "implements");
        } else {
            success_1441(&directory, args, result, true);
        }
    }
}

#[test] // A1 detector: expense process, implementation covers; check and unscoped verify.
fn issue_1441_a1_expense_covers_wrong_actor() {
    check_and_verify_1441(
        "1441_a1",
        EXPENSE_1441,
        EXPENSE_BUSINESS,
        "auto_approve",
        true,
    );
}

#[test] // A2 preservation: only the auto_approve actor differs from A1.
fn issue_1441_a2_expense_covers_correct_actor() {
    let source = EXPENSE_1441.replace(
        "Approved by Manager\n      covers",
        "Approved by System\n      covers",
    );
    check_and_verify_1441("1441_a2", &source, EXPENSE_BUSINESS, "auto_approve", false);
}

fn business_covers_1441(text: &str) -> String {
    BUSINESS_1441.replace("by System", &format!("by System covers BIZ-1 \"{text}\""))
}

#[test] // A3 detector: covers on the abstraction, not the implementation.
fn issue_1441_a3_business_covers_wrong_actor() {
    check_and_verify_1441(
        "1441_a3",
        PROCESS_1441,
        &business_covers_1441("Approval authority"),
        "approve",
        true,
    );
}

#[test] // A4 preservation.
fn issue_1441_a4_business_covers_correct_actor() {
    check_and_verify_1441(
        "1441_a4",
        &process_1441("System", ""),
        &business_covers_1441("Approval authority"),
        "approve",
        false,
    );
}

#[test] // A5 detector plus matching-actor preservation, both sides annotated.
fn issue_1441_a5_both_covers() {
    for actor in ["System", "Manager"] {
        check_and_verify_1441(
            &format!("1441_a5_{actor}"),
            &process_1441(actor, "covers REQ-1 \"Approval authority\""),
            &business_covers_1441("Approval authority"),
            "approve",
            actor == "Manager",
        );
    }
}

#[test] // B1 detector: authored text must not spoof the implementation actor.
fn issue_1441_b1_impl_spoof() {
    check_and_verify_1441(
        "1441_b1",
        &process_1441("Manager", "covers REQ-1 \"by System\""),
        BUSINESS_1441,
        "approve",
        true,
    );
}

#[test] // B2 detector: authored text must not spoof the business actor.
fn issue_1441_b2_business_spoof() {
    check_and_verify_1441(
        "1441_b2",
        PROCESS_1441,
        &business_covers_1441("by Manager"),
        "approve",
        true,
    );
}

#[test] // B3 detector: the old metadata lookup falsely rejects matching actors.
fn issue_1441_b3_impl_text_preserves_matching_actors() {
    check_and_verify_1441(
        "1441_b3",
        &process_1441("System", "covers REQ-1 \"by Manager\""),
        BUSINESS_1441,
        "approve",
        false,
    );
}

#[test] // B4 detector: likewise on the business side.
fn issue_1441_b4_business_text_preserves_matching_actors() {
    check_and_verify_1441(
        "1441_b4",
        &process_1441("System", ""),
        &business_covers_1441("by Manager"),
        "approve",
        false,
    );
}

#[test] // B5 preservation: explicit action correspondence is not Auto.
fn issue_1441_b5_explicit_correspondence() {
    let source = process_1441("Manager", "covers REQ-1 \"Approval authority\"")
        .replace("{ }", "{ action approve(c) -> approve(c) }");
    check_and_verify_1441("1441_b5_inline", &source, BUSINESS_1441, "approve", false);
    let mapping = AUTO_1441.replace("maps auto", "maps auto\n  action approve(c) -> approve(c)");
    let standalone =
        process_1441("Manager", "covers REQ-1 \"Approval authority\"").replace(INLINE_1441, "");
    let directory =
        mapping_fixture_1441("1441_b5_standalone", &standalone, BUSINESS_1441, &mapping);
    success_1441(&directory, STANDALONE_REFINE, "refines", false);
}

fn standalone_covers_1441(business_covers: bool) {
    for actor in ["System", "Manager"] {
        let source = process_1441(
            actor,
            if business_covers {
                ""
            } else {
                "covers REQ-1 \"Approval authority\""
            },
        )
        .replace(INLINE_1441, "");
        let business = if business_covers {
            business_covers_1441("Approval authority")
        } else {
            BUSINESS_1441.to_owned()
        };
        let directory = mapping_fixture_1441(
            &format!("1441_b6_{business_covers}_{actor}"),
            &source,
            &business,
            AUTO_1441,
        );
        if actor == "Manager" {
            mismatch_1441(
                &directory,
                STANDALONE_REFINE,
                "approve",
                AUTO_1441,
                "maps auto",
            );
        } else {
            success_1441(&directory, STANDALONE_REFINE, "refines", false);
        }
    }
}

#[test] // B6 detector with standalone positive control, implementation covers.
fn issue_1441_b6_standalone_impl_covers() {
    standalone_covers_1441(false);
}

#[test] // B6 detector with standalone positive control, business covers.
fn issue_1441_b6_standalone_business_covers() {
    standalone_covers_1441(true);
}

#[test] // B7 detector: every A/B negative also goes through unscoped verify.
fn issue_1441_b7_verify_rejects_before_proof() {
    let source = process_1441("Manager", "covers REQ-1 \"Approval authority\"");
    let directory = fixture("1441_b7", &source, BUSINESS_1441);
    mismatch_1441(&directory, VERIFY_1441, "approve", &source, "implements");
}

#[test] // C1 detector: inline 5 -> 4, both check and unscoped verify.
fn issue_1441_c1_inline_cross_edition() {
    check_and_verify_1441(
        "1441_c1",
        &inline_event_1441(),
        &abstract_process_1441(4),
        "approve",
        true,
    );
}

fn cross_edition_1441(label: &str, source: &str, edition: u8, wrong: bool) {
    let directory = mapping_fixture_1441(label, source, &abstract_process_1441(edition), AUTO_1441);
    if wrong {
        mismatch_1441(
            &directory,
            STANDALONE_REFINE,
            "approve",
            AUTO_1441,
            "maps auto",
        );
    } else {
        success_1441(&directory, STANDALONE_REFINE, "refines", false);
    }
}

#[test] // C2 detector: standalone 5 -> 4.
fn issue_1441_c2_event_to_process() {
    cross_edition_1441("1441_c2", EVENT_1441, 4, true);
}

#[test] // C3 detector: standalone 4 -> 5.
fn issue_1441_c3_process_to_event() {
    cross_edition_1441("1441_c3", &PROCESS_1441.replace(INLINE_1441, ""), 5, true);
}

#[test] // C4 preservation: matching actors in both directions.
fn issue_1441_c4_cross_edition_matching_actors() {
    cross_edition_1441(
        "1441_c4_event",
        &EVENT_1441.replace("Manager approves", "System approves"),
        4,
        false,
    );
    cross_edition_1441(
        "1441_c4_process",
        &process_1441("System", "").replace(INLINE_1441, ""),
        5,
        false,
    );
}

fn compose_1441(label: &str, reverse: bool, actor: &str, event: bool, wrong: bool) {
    let (source, abstraction, mapping) = if reverse {
        (
            include_str!("fixtures/actor_refinement/compose-impl.fsl")
                .replace("by Manager", &format!("by {actor}")),
            abstract_process_1441(5),
            include_str!("fixtures/actor_refinement/compose-reverse-map.fsl"),
        )
    } else {
        let source = if event {
            EVENT_1441.replace("Manager approves", &format!("{actor} approves"))
        } else {
            process_1441(actor, "").replace(INLINE_1441, "")
        };
        (
            source,
            include_str!("fixtures/actor_refinement/compose-abs.fsl").to_owned(),
            include_str!("fixtures/actor_refinement/compose-map.fsl"),
        )
    };
    let directory = mapping_fixture_1441(label, &source, &abstraction, mapping);
    if wrong {
        mismatch_1441(
            &directory,
            STANDALONE_REFINE,
            "approve",
            mapping,
            "maps auto",
        );
    } else {
        success_1441(&directory, STANDALONE_REFINE, "refines", false);
    }
}

#[test] // D1 detector: typed event -> compose legacy metadata.
fn issue_1441_d1_event_to_compose_wrong() {
    compose_1441("1441_d1", false, "Manager", true, true);
}

#[test] // D2 preservation.
fn issue_1441_d2_event_to_compose_correct() {
    compose_1441("1441_d2", false, "System", true, false);
}

#[test] // D3 preservation: fsl 4 metadata already rejects this input.
fn issue_1441_d3_process_to_compose_wrong() {
    compose_1441("1441_d3", false, "Manager", false, true);
}

#[test] // D4 detector: compose legacy metadata -> typed event.
fn issue_1441_d4_compose_to_event_wrong() {
    compose_1441("1441_d4", true, "Manager", true, true);
}

#[test] // D5 preservation.
fn issue_1441_d5_compose_to_event_correct() {
    compose_1441("1441_d5", true, "System", true, false);
}

#[test] // E1 preservation: fsl 4 no-covers process/business actor comparison.
fn issue_1441_e1_no_covers() {
    check_and_verify_1441(
        "1441_e1_correct",
        &process_1441("System", ""),
        BUSINESS_1441,
        "approve",
        false,
    );
    check_and_verify_1441(
        "1441_e1_wrong",
        PROCESS_1441,
        BUSINESS_1441,
        "approve",
        true,
    );
    cross_edition_1441(
        "1441_e1_process_pair",
        &PROCESS_1441.replace(INLINE_1441, ""),
        4,
        true,
    );
    cross_edition_1441(
        "1441_e1_process_pair_correct",
        &process_1441("System", "").replace(INLINE_1441, ""),
        4,
        false,
    );
}

#[test] // E2 preservation: raw legacy mismatch, matching actor, actorless on either side.
fn issue_1441_e2_raw_metadata() {
    let source = include_str!("fixtures/actor_refinement/raw-impl.fsl");
    let business = include_str!("fixtures/actor_refinement/raw-abs.fsl");
    check_and_verify_1441("1441_e2_wrong", source, business, "approve", true);
    for (label, source, business) in [
        (
            "same",
            source.replace("by Manager", "by System"),
            business.to_owned(),
        ),
        (
            "impl_actorless",
            source.replace(" \"REQ-1: by Manager\"", ""),
            business.to_owned(),
        ),
        (
            "abs_actorless",
            source.to_owned(),
            business.replace(" \"ABS-1: by System\"", ""),
        ),
    ] {
        check_and_verify_1441(
            &format!("1441_e2_{label}"),
            &source,
            &business,
            "approve",
            false,
        );
    }
}

#[test] // C2 checkpoint addition: diff synthesizes auto maps; report unknown, NOT type error.
fn issue_1441_diff_covers_actor_mismatch_is_automatic_mapping_failed() {
    let source = EXPENSE_1441.replace("  implements ExpenseToBe from \"business.fsl\" { }\n", "");
    let correct = source.replace(
        "Approved by Manager\n      covers",
        "Approved by System\n      covers",
    );
    let directory = fixture("1441_diff", &source, &correct);
    let (output, exit) = fslc(
        &directory,
        &["diff", "requirements.fsl", "business.fsl", "--depth", "6"],
    );
    assert_eq!(exit, 0, "{output:#}");
    assert_eq!(output["result"], "semantic_diff", "{output:#}");
    assert_eq!(
        output["directions"],
        serde_json::json!({
            "old_to_new": {
                "result": "unknown", "reason": "automatic_mapping_failed",
                "message": "maps auto actor mismatch for action 'auto_approve': impl actor 'Manager' != abstract actor 'System'"
            },
            "new_to_old": {
                "result": "unknown", "reason": "automatic_mapping_failed",
                "message": "maps auto actor mismatch for action 'auto_approve': impl actor 'System' != abstract actor 'Manager'"
            }
        }),
        "{output:#}"
    );
    assert!(
        output.get("kind").is_none(),
        "diff is not a type-error envelope: {output:#}"
    );
}

const RULE_ACTOR_TEXT_1441: &str = r#"fsl 5
requirements Abs {
  actor System, Manager
  entity Case { lifecycle Open -> Done }
  event approve = System approves Case Open -> Done
  rule REQ-9 "by Manager only" {
    allow System approves Case only when true
  }
}
verify { instances Case = 1 }
"#;

#[test] // F1 detector: rule requirement text is not the abstract event's actor.
fn issue_1441_f1_rule_text_preserves_matching_actors() {
    let source = process_1441("System", "");
    check_and_verify_1441(
        "1441_f1_inline",
        &source,
        RULE_ACTOR_TEXT_1441,
        "approve",
        false,
    );
    let directory = mapping_fixture_1441(
        "1441_f1_standalone",
        &source.replace(INLINE_1441, ""),
        RULE_ACTOR_TEXT_1441,
        AUTO_1441,
    );
    success_1441(&directory, STANDALONE_REFINE, "refines", false);
}

const REQUIREMENT_1456: &str = include_str!("fixtures/actor_refinement/requirement-action.fsl");
const RAW_ABS_1456: &str = include_str!("fixtures/actor_refinement/raw-abs.fsl");
const BRANCHES_1456: &str = "{ branches { when not done { done = true } maps approve()\n when done { done = true } maps approve() } }";

// Run every table cell even on the pre-fix implementation, retaining each
// assertion's diagnostic. A red first cell must not hide uncalibrated siblings.
fn all_1456<T>(cases: impl IntoIterator<Item = T>, mut run: impl FnMut(T)) {
    let mut failed = 0;
    for case in cases {
        failed += usize::from(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(case))).is_err(),
        );
    }
    assert_eq!(failed, 0, "{failed} acceptance cells failed");
}

fn requirement_routes_1456(cases: Vec<(&str, String, String)>, wrong: bool) {
    all_1456(cases, |(label, source, abstraction)| {
        let inline = source.contains("implements");
        let directory = mapping_fixture_1441(label, &source, &abstraction, AUTO_1441);
        let mut commands = vec![
            (&["check", "requirements.fsl"][..], "ok"),
            (VERIFY_1441, "verified"),
        ];
        if !inline {
            commands.push((STANDALONE_REFINE, "refines"));
        }
        all_1456(commands, |(args, result)| {
            eprintln!("1456 cell {label} {args:?}");
            if wrong && (inline || result == "refines") {
                let (located, marker) = if inline {
                    (source.as_str(), "implements")
                } else {
                    (AUTO_1441, "maps auto")
                };
                let action = if source.contains("branches") {
                    "approve__b1"
                } else {
                    "approve"
                };
                mismatch_1441(&directory, args, action, located, marker);
            } else {
                success_1441(&directory, args, result, inline);
            }
        });
    });
}

fn abstract_requirement_1456() -> String {
    REQUIREMENT_1456
        .replace(INLINE_1441, "")
        .replace("Impl", "Abs")
        .replace("REQ-1 \"by System\"", "ABS-REQ \"by Manager\"")
        .replace("IMPL-1: by Manager", "ABS-1: by System")
}

#[test] // A detectors: block, ordinary text, abstract side, standalone and branches.
fn issue_1456_a_requirement_own_actor_mismatch() {
    let raw = include_str!("fixtures/actor_refinement/raw-impl.fsl");
    let branches = REQUIREMENT_1456.replace("{ done = true }", BRANCHES_1456);
    let branch_abs = branches
        .replace(INLINE_1441, "")
        .replace("Impl", "Abs")
        .replace("IMPL-1: by Manager", "ABS-1: by System")
        .replace("maps approve()", "maps stutter");
    requirement_routes_1456(
        vec![
            ("1456_a_issue", REQUIREMENT_1456.into(), RAW_ABS_1456.into()),
            (
                "1456_a_ordinary",
                REQUIREMENT_1456.replace("\"by System\"", "\"Approval policy\""),
                RAW_ABS_1456.into(),
            ),
            (
                "1456_a_reverse_inline",
                raw.into(),
                abstract_requirement_1456(),
            ),
            (
                "1456_a_explicit_auto",
                REQUIREMENT_1456.replace("{ }", "{ maps auto }"),
                RAW_ABS_1456.into(),
            ),
            (
                "1456_a_standalone",
                REQUIREMENT_1456.replace(INLINE_1441, ""),
                RAW_ABS_1456.into(),
            ),
            (
                "1456_a_reverse",
                raw.replace(INLINE_1441, ""),
                abstract_requirement_1456(),
            ),
            (
                "1456_a_branch_auto",
                branches.replace(INLINE_1441, ""),
                branch_abs,
            ),
        ],
        true,
    );
}

#[test] // B detectors: matching own actors cannot be rejected by either block's text.
fn issue_1456_b_requirement_text_is_not_actor() {
    let spoof = REQUIREMENT_1456
        .replace("\"by System\"", "\"by Manager\"")
        .replace("IMPL-1: by Manager", "IMPL-1: by System");
    requirement_routes_1456(
        vec![
            ("1456_b_impl", spoof.clone(), RAW_ABS_1456.into()),
            (
                "1456_b_both",
                spoof,
                abstract_requirement_1456().replace("by Manager", "by Finance"),
            ),
            (
                "1456_b_abs",
                REQUIREMENT_1456.replace("IMPL-1: by Manager", "IMPL-1: by System"),
                abstract_requirement_1456(),
            ),
        ],
        false,
    );
}

#[test] // C detectors (D1): registered absence must never descend into block text.
fn issue_1456_c_requirement_actor_absence() {
    let mut cases = Vec::new();
    for (label, edition, annotation) in [
        ("none", "", ""),
        ("nonby", "", " \"IMPL-1: Approve request\""),
        ("empty", "", " \"IMPL-1: by   \""),
        ("fsl5_none", "fsl 5\n", ""),
        ("fsl5_typed", "fsl 5\n", ""),
        ("fsl5_typed_spoof", "fsl 5\n", ""),
    ] {
        for text in ["by System", "by Manager", "Approval policy"] {
            let mut source = format!("{edition}{REQUIREMENT_1456}")
                .replace(" \"IMPL-1: by Manager\"", annotation)
                .replace("\"by System\"", &format!("\"{text}\""));
            if label.starts_with("fsl5_typed") {
                source = source.replace(
                    "action approve",
                    &format!(
                        "@requirement(\"IMPL-1\", \"by {}\")\n    action approve",
                        if label == "fsl5_typed" {
                            "System"
                        } else {
                            "Manager"
                        }
                    ),
                );
            }
            cases.push((
                format!("1456_c_{label}_{text}"),
                source,
                RAW_ABS_1456.to_owned(),
            ));
        }
    }
    cases.push((
        "1456_c_abs_none".into(),
        REQUIREMENT_1456.replace("IMPL-1: by Manager", "IMPL-1: by System"),
        abstract_requirement_1456().replace(" \"ABS-1: by System\"", ""),
    ));
    requirement_routes_1456(
        cases
            .iter()
            .map(|(label, source, abs)| (label.as_str(), source.clone(), abs.clone()))
            .collect(),
        false,
    );
}

#[test] // D preservation, not detectors: outside-block, explicit, branches, stutter and edition gate.
fn issue_1456_d_preserved_boundaries() {
    requirement_routes_1456(
        vec![(
            "1456_d_outside",
            include_str!("fixtures/actor_refinement/raw-impl.fsl").into(),
            RAW_ABS_1456.into(),
        )],
        true,
    );
    requirement_routes_1456(
        vec![
            (
                "1456_d_same",
                REQUIREMENT_1456.replace("IMPL-1: by Manager", "IMPL-1: by System"),
                RAW_ABS_1456.into(),
            ),
            (
                "1456_d_explicit",
                REQUIREMENT_1456.replace("{ }", "{ action approve() -> approve() }"),
                RAW_ABS_1456.into(),
            ),
            (
                "1456_d_branches",
                REQUIREMENT_1456.replace("{ done = true }", BRANCHES_1456),
                RAW_ABS_1456.into(),
            ),
            (
                "1456_d_stutter",
                REQUIREMENT_1456
                    .replace("{ }", "{ action approve() -> stutter }")
                    .replace("{ done = true }", "{ done = done }"),
                RAW_ABS_1456.into(),
            ),
        ],
        false,
    );
    let legacy = format!("fsl 5\n{REQUIREMENT_1456}");
    let directory = fixture("1456_d_legacy", &legacy, RAW_ABS_1456);
    for args in [&["check", "requirements.fsl"][..], VERIFY_1441] {
        let (output, exit) = fslc(&directory, args);
        assert_eq!(
            (exit, output["result"].as_str(), output["kind"].as_str()),
            (2, Some("error"), Some("unsupported_in_edition")),
            "{output:#}"
        );
        assert!(
            output["findings"]
                .as_array()
                .expect("edition findings")
                .iter()
                .any(|finding| finding["code"] == "legacy_string_metadata"),
            "{output:#}"
        );
    }
}

fn diff_1456(wrong: bool) {
    let source = REQUIREMENT_1456.replace(INLINE_1441, "");
    let correct = source.replace("IMPL-1: by Manager", "IMPL-1: by System");
    let directory = fixture(
        if wrong { "1456_g_wrong" } else { "1456_g_same" },
        if wrong { &source } else { &correct },
        RAW_ABS_1456,
    );
    let (output, exit) = fslc(
        &directory,
        &["diff", "requirements.fsl", "business.fsl", "--depth", "3"],
    );
    assert_eq!(
        (exit, output["result"].as_str()),
        (0, Some("semantic_diff")),
        "{output:#}"
    );
    assert!(output.get("kind").is_none(), "{output:#}");
    all_1456(
        [
            ("old_to_new", "Manager", "System"),
            ("new_to_old", "System", "Manager"),
        ],
        |(direction, actor, abstract_actor)| {
            if wrong {
                assert_eq!(
                    output["directions"][direction],
                    serde_json::json!({
                        "result": "unknown", "reason": "automatic_mapping_failed",
                        "message": format!("maps auto actor mismatch for action 'approve': impl actor '{actor}' != abstract actor '{abstract_actor}'")
                    }),
                    "{output:#}"
                );
            } else {
                assert_eq!(
                    output["directions"][direction]["result"], "refines",
                    "{output:#}"
                );
            }
        },
    );
    if !wrong {
        assert_eq!(
            output["summary"],
            serde_json::json!(["no_semantic_change"]),
            "{output:#}"
        );
    }
}

#[test] // G detector: directional unknown is not a top-level type error.
fn issue_1456_g_diff_mismatch() {
    diff_1456(true);
}

#[test] // G preservation: same actors still have no semantic change.
fn issue_1456_g_diff_same_actor() {
    diff_1456(false);
}
