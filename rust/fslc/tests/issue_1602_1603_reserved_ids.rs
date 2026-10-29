// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1602/#1603: user IDs and glossary aliases obey the generated-name
//! reservation in both editions. Invalid written rule-property names retain
//! their diagnostic code/message, but become located name errors.

use serde_json::{Value, json};
use std::process::Command;

// Keep the position vocabulary together for the declared-name walk contract.
const GOAL: &str = "goal";
const POLICY: &str = "policy";
const REQUIREMENT: &str = "requirement";
const SCENARIO: &str = "scenario";
const FORBIDDEN: &str = "forbidden";
const DECISION: &str = "decision";
const GLOSSARY_ALIAS: &str = "glossary alias";

const TICKET: &str = include_str!("fixtures/fsl5/relations/fsl5_en.fsl");
const BUSINESS: &str = include_str!("../../../examples/layers/return_policy.fsl");
const CLAIM: &str = "fsl 5\nrequirements R {\n  actor User\n  entity Claim {\n    lifecycle Draft -> Approved\n  }\n  event approve = User approves Claim Draft -> Approved\n  MEMBER\n}\nverify {\n  instances Claim = 1\n}\n";
const LEGACY: &str = "requirements R {\n  state { x: 0..1 }\n  init { x = 0 }\n  requirement REQ-1 \"advance\" {\n    action go() { requires x == 0 x = 1 }\n  }\n  acceptance AC-1 \"advance succeeds\" { go() expect x == 1 }\n  forbidden FB-1 \"cannot advance twice\" { go() go() expect rejected }\n}\n";
const ROLE: &str = "fsl 5\nrequirements R {\n  actor User\n  entity User\n  entity Claim {\n    lifecycle Draft -> Approved\n  }\n  Claim belongs to User as NAME via approve\n  event approve = User approves Claim Draft -> Approved\n}\nverify {\n  instances User = 1\n  instances Claim = 1\n}\n";
const RESOURCE: &str = "fsl 5\nrequirements R {\n  number Amount\n  resource NAME: Amount = 0\n  actor User\n  entity Claim {\n    lifecycle Draft -> Approved\n  }\n  event approve = User approves Claim Draft -> Approved\n}\nverify {\n  instances Claim = 1\n  values Amount = 0..1\n}\n";
const MEASURE: &str = "fsl 5\nrequirements R {\n  actor User\n  entity Claim {\n    lifecycle Draft -> Approved\n  }\n  event approve = User approves Claim Draft -> Approved\n  measure NAME = count Claim in Approved\n}\nverify {\n  instances Claim = 1\n}\n";
const RULE_DEADLINE: &str = "fsl 5\nrequirements R {\n  const SLA = 2\n  actor Agent\n  entity Case {\n    lifecycle Open -> Done\n  }\n  event close = Agent closes Case Open -> Done\n  time {\n    urgent close\n    age open_age[c: Case] while case_stage[c] == Open\n  }\n  rule NAME \"Close within SLA\" {\n    deadline open_age <= SLA\n  }\n}\nverify {\n  instances Case = 1\n}\n";
const RULE_IMPLIED: &str = "fsl 5\nrequirements R {\n  actor User\n  entity Claim {\n    lifecycle Draft -> Submitted -> Approved\n  }\n  event submit = User submits Claim Draft -> Submitted\n  event approve = User approves Claim Submitted -> Approved\n  rule NAME \"Submit before approval\" {\n    User approves Claim only after User submits Claim\n  }\n}\nverify {\n  instances Claim = 1\n}\n";
const RULE_COVERED: &str = "fsl 5\nrequirements R {\n  actor User\n  entity Claim {\n    lifecycle Draft -> Approved\n  }\n  event approve = User approves Claim Draft -> Approved\n    covers NAME \"Approve\"\n  rule NAME \"Approve\" {\n    allow User approves Claim only when true\n  }\n}\nverify {\n  instances Claim = 1\n}\n";

fn replace_once(source: &str, from: &str, to: &str) -> String {
    assert_eq!(
        source.matches(from).count(),
        1,
        "unique source anchor {from:?}"
    );
    source.replacen(from, to, 1)
}

fn location(source: &str, needle: &str) -> Value {
    let offset = source.find(needle).expect("location anchor in source");
    location_at(source, offset)
}

fn location_at(source: &str, offset: usize) -> Value {
    let before = &source[..offset];
    json!({
        "line": before.bytes().filter(|byte| *byte == b'\n').count() + 1,
        "column": before.rsplit('\n').next().unwrap().chars().count() + 1,
    })
}

fn check(label: &str, source: &str) -> (Value, i32, std::path::PathBuf) {
    // Match issue_1600's worktree-local scratch convention; retain evidence.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!(
            "issue-1602-1603-{}-{label}.fsl",
            std::process::id()
        ));
    std::fs::write(&path, source).expect("write fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("check")
        .arg(&path)
        .env("FSLC_UNSTABLE", "fsl5")
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: invalid CLI JSON ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("CLI exit code"), path)
}

fn name_case(label: &str, template: &str, position: &str, name: &str) {
    name_case_in_declaration(label, template, position, name, "");
}

fn name_case_in_declaration(label: &str, template: &str, position: &str, name: &str, prefix: &str) {
    assert!(template.contains("NAME"), "name placeholder in {label}");
    let source = template.replace("NAME", name);
    let (value, exit, _) = check(label, &source);
    if name.contains("__") {
        assert_eq!(exit, 2, "{label}: {value:#}");
        assert_eq!(value["result"], "error", "{label}: {value:#}");
        assert_eq!(value["kind"], "name", "{label}: {value:#}");
        let message_prefix = format!("{position} name '{name}' contains '__'");
        assert!(
            value["message"]
                .as_str()
                .unwrap()
                .starts_with(&message_prefix),
            "{label}: {value:#}"
        );
        let start = source
            .find(&format!("{prefix}{name}"))
            .expect("declaration anchor in source")
            + prefix.len();
        assert_eq!(
            value["loc"],
            location_at(&source, start),
            "{label}: {value:#}"
        );
    } else {
        assert_eq!(exit, 0, "{label}: {value:#}");
        assert_eq!(value["result"], "ok", "{label}: {value:#}");
    }
}

macro_rules! pair {
    ($reject:ident, $control:ident, $source:expr, $position:expr, $bad:literal, $good:literal) => {
        #[test]
        fn $reject() {
            name_case(stringify!($reject), &$source, $position, $bad);
        }
        #[test]
        fn $control() {
            name_case(stringify!($control), &$source, $position, $good);
        }
    };
}

// Detectors: skipping each newly walked position must turn its rejection red.
pair!(
    next_goal,
    next_goal_control,
    replace_once(
        TICKET,
        "goal Closable \"A ticket can be closed\" some Ticket can reach Closed",
        "goal NAME \"A ticket can be escalated\" some Ticket can reach Escalated"
    ),
    GOAL,
    "Clo__sable",
    "Closable"
);
pair!(
    next_policy,
    next_policy_control,
    replace_once(TICKET, "policy Resolved", "policy NAME"),
    POLICY,
    "Res__olved",
    "Resolved"
);
pair!(
    minimal_goal,
    minimal_goal_control,
    replace_once(
        CLAIM,
        "MEMBER",
        "goal NAME \"Reach approval\" some Claim can reach Approved"
    ),
    GOAL,
    "__g",
    "g"
);
pair!(
    minimal_policy,
    minimal_policy_control,
    replace_once(
        CLAIM,
        "MEMBER",
        "policy NAME \"Eventually approve\" every Claim in Draft must eventually be Approved"
    ),
    POLICY,
    "__p",
    "p"
);
pair!(
    legacy_goal,
    legacy_goal_control,
    replace_once(BUSINESS, "goal AllSettled", "goal NAME"),
    GOAL,
    "G__x",
    "Gx"
);
pair!(
    legacy_policy,
    legacy_policy_control,
    replace_once(BUSINESS, "policy PAY-2", "policy NAME"),
    POLICY,
    "P__x",
    "Px"
);
pair!(
    next_decide,
    next_decide_control,
    replace_once(
        TICKET,
        "  rule REQ-1",
        "  concept AssignedTicket = Ticket in Assigned\n  decide NAME \"Who escalates\" for Assigned Ticket {\n    AssignedTicket => Agent closes or escalates\n  }\n  rule REQ-1"
    ),
    DECISION,
    "DEC__1",
    "DEC1"
);
// All three written headers must be authoritative for ID checking, regardless
// of the deadline, guard, or property effects retained after desugaring.
pair!(
    next_rule_deadline,
    next_rule_deadline_control,
    RULE_DEADLINE,
    REQUIREMENT,
    "REQ__1",
    "REQ1"
);
pair!(
    next_rule_guard_only,
    next_rule_guard_only_control,
    replace_once(TICKET, "rule REQ-1", "rule NAME"),
    REQUIREMENT,
    "REQ__1",
    "REQ1"
);
pair!(
    next_rule_property_only,
    next_rule_property_only_control,
    replace_once(TICKET, "rule REQ-2", "rule NAME"),
    REQUIREMENT,
    "REQ__1",
    "REQ1"
);

// Review findings 1/2: the written rule header is authoritative even when
// desugaring elides the rule's effect or earlier covers clauses carry its ID.
macro_rules! rule_header_pair {
    ($reject:ident, $control:ident, $source:expr) => {
        #[test]
        fn $reject() {
            name_case_in_declaration(
                stringify!($reject),
                &$source,
                REQUIREMENT,
                "REQ__1",
                "rule ",
            );
        }
        #[test]
        fn $control() {
            name_case_in_declaration(stringify!($control), &$source, REQUIREMENT, "REQ1", "rule ");
        }
    };
}

rule_header_pair!(
    review_rule_implied_only_after,
    review_rule_implied_only_after_control,
    RULE_IMPLIED
);
rule_header_pair!(
    review_rule_one_cover,
    review_rule_one_cover_control,
    RULE_COVERED
);
rule_header_pair!(
    review_rule_two_covers,
    review_rule_two_covers_control,
    replace_once(
        RULE_COVERED,
        "    covers NAME \"Approve\"",
        "    covers OTHER \"Other\"\n    covers NAME \"Approve\""
    )
);

fn deadline_location_case(label: &str, header: &str, name: &str, line: u32, column: u32) {
    let source = replace_once(RULE_DEADLINE, "NAME", header);
    let (value, exit, _) = check(label, &source);
    if name.contains("__") {
        assert_eq!(exit, 2, "{label}: {value:#}");
        assert_eq!(value["result"], "error", "{label}: {value:#}");
        assert_eq!(value["kind"], "name", "{label}: {value:#}");
        let message_prefix = format!("{REQUIREMENT} name '{name}' contains '__'");
        assert!(
            value["message"]
                .as_str()
                .unwrap()
                .starts_with(&message_prefix),
            "{label}: {value:#}"
        );
        // Explicit token coordinates distinguish the written header from its
        // normalized ID, an earlier comment, and the deadline requirement copy.
        assert_eq!(
            value["loc"],
            json!({"line": line, "column": column}),
            "{label}: {value:#}"
        );
    } else {
        assert_eq!(exit, 0, "{label}: {value:#}");
        assert_eq!(value["result"], "ok", "{label}: {value:#}");
    }
}

#[test]
fn review_deadline_split_id() {
    deadline_location_case("review-deadline-split-id", "REQ - __1", "REQ-__1", 13, 8);
}

#[test]
fn review_deadline_split_id_control() {
    deadline_location_case(
        "review-deadline-split-id-control",
        "REQ - 1",
        "REQ-1",
        13,
        8,
    );
}

#[test]
fn review_deadline_comment_before_id() {
    deadline_location_case(
        "review-deadline-comment-before-id",
        "// REQ__1\n    REQ__1",
        "REQ__1",
        14,
        5,
    );
}

#[test]
fn review_deadline_comment_before_id_control() {
    deadline_location_case(
        "review-deadline-comment-before-id-control",
        "// REQ1\n    REQ1",
        "REQ1",
        14,
        5,
    );
}

// Explicit requirement blocks are accepted inside fsl 5 documents too. They
// are declarations, unlike the requirement copies generated from rule headers.
pair!(
    review_explicit_requirement_deadline,
    review_explicit_requirement_deadline_control,
    replace_once(RULE_DEADLINE, "rule NAME", "requirement NAME"),
    REQUIREMENT,
    "REQ__1",
    "REQ-1"
);
pair!(
    review_explicit_requirement_without_deadline,
    review_explicit_requirement_without_deadline_control,
    replace_once(
        &format!("fsl 5\n{LEGACY}"),
        "requirement REQ-1",
        "requirement NAME"
    ),
    REQUIREMENT,
    "REQ__1",
    "REQ-1"
);

pair!(
    next_scenario,
    next_scenario_control,
    replace_once(TICKET, "scenario AC-1", "scenario NAME"),
    SCENARIO,
    "AC__1",
    "AC1"
);
pair!(
    next_forbidden,
    next_forbidden_control,
    replace_once(TICKET, "forbidden FB-DESK-CLOSED", "forbidden NAME"),
    FORBIDDEN,
    "FB__1",
    "FB1"
);
pair!(
    next_glossary_alias,
    next_glossary_alias_control,
    replace_once(
        TICKET,
        "requirements Support {",
        "requirements Support {\n  glossary { Ticket = NAME }"
    ),
    GLOSSARY_ALIAS,
    "Kip__pu",
    "Kippu"
);
pair!(
    legacy_requirement,
    legacy_requirement_control,
    replace_once(LEGACY, "requirement REQ-1", "requirement NAME"),
    REQUIREMENT,
    "REQ__1",
    "REQ1"
);
pair!(
    legacy_acceptance,
    legacy_acceptance_control,
    replace_once(LEGACY, "acceptance AC-1", "acceptance NAME"),
    SCENARIO,
    "AC__1",
    "AC1"
);
pair!(
    legacy_forbidden,
    legacy_forbidden_control,
    replace_once(LEGACY, "forbidden FB-1", "forbidden NAME"),
    FORBIDDEN,
    "FB__1",
    "FB1"
);

fn rule_property(label: &str, name: &str, reason: Option<&str>) {
    let statement = "always Ticket in Assigned, Closed, Escalated has assignee";
    let source = replace_once(TICKET, statement, &format!("{name}: {statement}"));
    let (value, exit, path) = check(label, &source);
    if let Some(reason) = reason {
        assert_eq!(exit, 2, "{label}: {value:#}");
        assert_eq!(value["result"], "error", "{label}: {value:#}");
        assert_eq!(value["kind"], "name", "{label}: {value:#}");
        // RuleProperty.span is the statement span, not the written label's.
        // Preserve the diagnostic wording and pin the located name error's
        // source-stamped suffix to this exact fixture, not an arbitrary path.
        let loc = location(&source, statement);
        assert_eq!(value["loc"], loc, "{label}: {value:#}");
        assert_eq!(value["diagnostic_code"], "invalid_rule_property_name");
        assert_eq!(
            value["message"],
            format!(
                "rule REQ-2 names its property '{name}', which {reason} at {}:{}:{}",
                path.display(),
                loc["line"],
                loc["column"]
            ),
            "{label}: {value:#}"
        );
    } else {
        assert_eq!(exit, 0, "{label}: {value:#}");
        assert_eq!(value["result"], "ok", "{label}: {value:#}");
    }
}

#[test]
fn rule_property_separator() {
    rule_property(
        "rule-property-separator",
        "Ke__eps",
        Some("contains `__`, which a published name reads as a separator"),
    );
}

#[test]
fn rule_property_literal() {
    rule_property("rule-property-literal", "none", Some("is a literal"));
}

#[test]
fn rule_property_separator_control() {
    rule_property("rule-property-separator-control", "Keeps", None);
}

#[test]
fn rule_property_literal_control() {
    rule_property("rule-property-literal-control", "NoneValue", None);
}

// Preservation controls: existing #1256/#1600 rejections, not detectors for
// the new walk. These must already pass on the unchanged production baseline.
macro_rules! preserved {
    ($test:ident, $source:expr, $position:literal, $name:literal) => {
        #[test]
        fn $test() {
            name_case(stringify!($test), &$source, $position, $name);
        }
    };
}

preserved!(
    existing_action,
    "spec S {\n  state { x: Int }\n  init { x = 0 }\n  action NAME() { x = 1 }\n}\n",
    "action",
    "foo__bar"
);
preserved!(
    existing_state,
    "spec S {\n  state { NAME: Bool }\n  init { a__b = false }\n}\n",
    "state variable",
    "a__b"
);
preserved!(
    existing_parameter,
    "spec S {\n  type N = 0..3\n  state { x: Int }\n  init { x = 0 }\n  action go(NAME: N) { x = n__m }\n}\n",
    "parameter",
    "n__m"
);
preserved!(
    existing_legacy_property,
    "requirements R {\n  state { x: 0..2 }\n  init { x = 0 }\n  requirement REQ-1 \"bounded\" {\n    invariant NAME { x <= 2 }\n  }\n}\n",
    "property",
    "Small__Inv"
);
preserved!(
    existing_actor,
    replace_once(CLAIM, "MEMBER", "").replace("User", "NAME"),
    "actor",
    "Us__er"
);
preserved!(existing_relation_role, ROLE, "relation role", "owner__x");
preserved!(existing_resource, RESOURCE, "resource", "__r");
preserved!(existing_measure, MEASURE, "measure", "__m");
