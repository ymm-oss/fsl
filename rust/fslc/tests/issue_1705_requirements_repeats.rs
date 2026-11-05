// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1705: in this edition's requirements, a repeated `actor` and a second declaration
//! owning one requirement ID passed `check`. For the ID, the ledger kept one row with one
//! title and one status for the two. Each is now refused at the repeat, on every route
//! that loads the document. Two `covers` of one ID with one text are a link and stay
//! accepted.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// The support-desk document of the issue. Line 19 starts `REQ-DESK-001`.
const DESK: &str = "fsl 5
requirements Support {
  actor Agent
  entity Agent
  entity Ticket {
    lifecycle Open -> Assigned -> Closed
    lifecycle Assigned -> Escalated
    lifecycle Open -> Withdrawn
  }
  Ticket belongs to Agent as assignee via take
  resource desk_open: Bool = true

  event take = Agent takes Ticket Open -> Assigned
  event close = Agent closes Ticket Assigned -> Closed
  event escalate = Agent escalates Ticket Assigned -> Escalated
    set desk_open = false
  event withdraw = Agent withdraws Ticket Open -> Withdrawn

  rule REQ-DESK-001 \"A ticket is taken only while the desk is open\" {
    allow Agent takes Ticket only when desk_open
  }
  rule REQ-DESK-002 \"A ticket is withdrawn only after the desk has closed\" {
    allow Agent withdraws Ticket only when not desk_open
  }
  rule REQ-TICKET-001 \"A taken ticket keeps the agent who took it\" {
    always Ticket in Assigned, Closed, Escalated has assignee
  }
  policy POL-TICKET-001 \"Every assigned ticket is eventually closed or escalated\"
    every Ticket in Assigned must eventually be Closed or Escalated
  goal GOAL-TICKET-001 \"A ticket can be closed\" some Ticket can reach Closed
  measure closed_tickets = count Ticket in Closed

  scenario AC-TICKET-001 \"An agent takes and closes a ticket\" {
    Agent takes t1 with assignee 1
    Agent closes t1
    then t1 is Closed
  }
  scenario AC-DESK-001 \"A ticket still open when the desk closes is withdrawn\" {
    Agent takes t1 with assignee 0
    Agent escalates t1
    Agent withdraws t2
    then t2 is Withdrawn
  }
  forbidden FB-DESK-001 \"No ticket is taken once the desk has closed\" {
    Agent takes t1 with assignee 0
    Agent escalates t1
    Agent takes t2 with assignee 0
    then rejected
  }
  forbidden FB-DESK-002 \"No ticket is withdrawn while the desk is open\" {
    Agent withdraws t1
    then rejected
  }
}
verify {
  instances Ticket = 2
  instances Agent = 2
}
";

/// A decision table. Line 18 starts the table, line 20 its first `covers`.
const CLAIMS: &str = "fsl 5
requirements Claims {
  actor System
  actor Manager
  actor Employee
  number Amt
  entity Claim {
    amount: Amt = 0
    lifecycle Draft -> Submitted -> Approved
    lifecycle Submitted -> Rejected
  }
  concept SmallClaim = Claim where amount <= 1
  concept LargeClaim = Claim where amount >= 2
  event submit = Employee submits Claim with amount Draft -> Submitted
  event auto_approve = System approves Claim Submitted -> Approved
  event manual_approve = Manager approves Claim Submitted -> Approved
  event reject = Manager rejects Claim Submitted -> Rejected
  decide REQ-2-3 \"Who handles a submitted claim\" for Submitted Claim {
    SmallClaim => System approves
      covers REQ-2 \"A small claim is auto-approved\"
    LargeClaim => Manager approves or rejects
      covers REQ-3 \"A large claim is approved or rejected by a manager\"
  } assuming amount > 0
}
verify {
  instances Claim = 1
  values Amt = 0..2
}
";

const RULE_A: &str = "  rule REQ-DESK-001 \"A ticket is taken only while the desk is open\" {
    allow Agent takes Ticket only when desk_open
  }
";
const RULE_B: &str =
    "  rule REQ-DESK-001 \"dup\" { allow Agent closes Ticket only when desk_open }\n";
const SECOND_RULE: &str = "  rule REQ-DESK-002";

/// A scratch directory of source files.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, files: &[(&str, &str)]) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1705")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        for (name, source) in files {
            std::fs::write(directory.join(name), source).expect("write source");
        }
        Self(directory)
    }

    /// The exit, the stdout and, when stdout is JSON, the packet of `fslc <args>`.
    fn run(&self, args: &[&str]) -> (i32, String, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env("FSLC_CACHE", "off")
            .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
            .output()
            .expect("run fslc");
        let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
        let packet = serde_json::from_str(&stdout).unwrap_or(Value::Null);
        (output.status.code().expect("exit code"), stdout, packet)
    }

    fn fslc(&self, args: &[&str]) -> (i32, Value) {
        let (exit, _, packet) = self.run(args);
        (exit, packet)
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.0.join(name)).expect("read output")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn check(label: &str, source: &str) -> (i32, Value) {
    Scratch::new(label, &[("doc.fsl", source)]).fslc(&["check", "doc.fsl"])
}

fn summary((exit, packet): &(i32, Value)) -> Value {
    json!({
        "exit": exit,
        "result": packet["result"],
        "kind": packet["kind"],
        "message": packet["message"],
        "loc": packet["loc"],
    })
}

fn refusal(message: &str, line: u32, column: u32) -> Value {
    json!({
        "exit": 2,
        "result": "error",
        "kind": "parse",
        "message": format!("{message} at {line}:{column}"),
        "loc": {"line": line, "column": column},
    })
}

fn owner(id: &str, kind: &str, first: &str, at: (u32, u32)) -> String {
    format!(
        "duplicate requirement id '{id}': this {kind} reuses the ID of the {first} at {}:{}; \
         a requirement ID has one owner",
        at.0, at.1
    )
}

/// `source` with `text` inserted before `anchor`.
fn before(source: &str, anchor: &str, text: &str) -> String {
    assert!(source.contains(anchor), "{anchor}");
    source.replacen(anchor, &format!("{text}{anchor}"), 1)
}

#[test]
fn a_repeated_actor_is_refused_at_the_repeat() {
    let twice = DESK.replacen("  actor Agent\n", "  actor Agent\n  actor Agent\n", 1);
    assert_eq!(
        summary(&check("actor_twice", &twice)),
        refusal("duplicate actor 'Agent'", 4, 9)
    );
    let listed = DESK.replacen("  actor Agent\n", "  actor Agent, Agent\n", 1);
    assert_eq!(
        summary(&check("actor_listed", &listed)),
        refusal("duplicate actor 'Agent'", 3, 16)
    );
    let two = DESK.replacen("  actor Agent\n", "  actor Agent, Lead\n", 1);
    assert_eq!(check("actor_two", &two).0, 0);
}

/// The issue's pair, both orders: the second copy is refused, wherever it is.
#[test]
fn a_second_rule_of_one_id_is_refused_at_its_id() {
    let after = before(DESK, SECOND_RULE, RULE_B);
    assert_eq!(
        summary(&check("rule_after", &after)),
        refusal(&owner("REQ-DESK-001", "rule", "rule", (19, 8)), 22, 8)
    );
    let first = DESK.replacen(RULE_A, &format!("{RULE_B}{RULE_A}"), 1);
    assert_eq!(
        summary(&check("rule_first", &first)),
        refusal(&owner("REQ-DESK-001", "rule", "rule", (19, 8)), 20, 8)
    );
    // `always`, `never` and a second guard on the same event lose a copy the same way.
    for (label, statement) in [
        ("always", "always Ticket in Closed has assignee"),
        ("never", "never Agent closes Ticket when not desk_open"),
        ("same_event", "allow Agent takes Ticket only when desk_open"),
    ] {
        let rule = format!("  rule REQ-DESK-001 \"dup\" {{ {statement} }}\n");
        assert_eq!(
            summary(&check(label, &before(DESK, SECOND_RULE, &rule))),
            refusal(&owner("REQ-DESK-001", "rule", "rule", (19, 8)), 22, 8),
            "{label}"
        );
    }
    assert_eq!(check("desk", DESK).0, 0);
}

/// One owner per ID across `rule`, `policy`, `forbidden`, `scenario`, `goal` and
/// `decide`, at the second owner, naming both kinds.
#[test]
fn a_second_owner_of_another_kind_is_refused_at_its_id() {
    let rule = |id: &str| {
        format!("  rule {id} \"dup\" {{ allow Agent closes Ticket only when desk_open }}\n")
    };
    for (label, id, first, at) in [
        ("policy", "POL-TICKET-001", "policy", (28, 10)),
        ("goal", "GOAL-TICKET-001", "goal", (30, 8)),
        ("scenario", "AC-TICKET-001", "scenario", (33, 12)),
        ("forbidden", "FB-DESK-002", "forbidden", (50, 13)),
    ] {
        let source = before(DESK, "}\nverify {", &rule(id));
        assert_eq!(
            summary(&check(label, &source)),
            refusal(&owner(id, "rule", first, at), 54, 8),
            "{label}"
        );
    }
    let forbidden =
        "  forbidden AC-TICKET-001 \"dup\" {\n    Agent withdraws t1\n    then rejected\n  }\n";
    assert_eq!(
        summary(&check(
            "forbidden_scenario",
            &before(DESK, "}\nverify {", forbidden)
        )),
        refusal(
            &owner("AC-TICKET-001", "forbidden", "scenario", (33, 12)),
            54,
            13
        )
    );
    let decide_rule =
        "  rule REQ-2-3 \"dup\" { allow Manager rejects Claim only when amount >= 2 }\n";
    assert_eq!(
        summary(&check(
            "decide_rule",
            &before(CLAIMS, "}\nverify {", decide_rule)
        )),
        refusal(&owner("REQ-2-3", "rule", "decide", (18, 10)), 24, 8)
    );
    let rule_decide = before(CLAIMS, "  decide REQ-2-3", decide_rule);
    assert_eq!(
        summary(&check("rule_decide", &rule_decide)),
        refusal(&owner("REQ-2-3", "decide", "rule", (18, 8)), 19, 10)
    );
}

#[test]
fn a_second_decide_of_one_id_is_refused_at_its_id() {
    let table = "  decide REQ-2-3 \"dup\" for Submitted Claim {
    SmallClaim => System approves
      covers REQ-8 \"dup small\"
    LargeClaim => Manager approves or rejects
      covers REQ-9 \"dup large\"
  }
";
    assert_eq!(
        summary(&check(
            "decide_twice",
            &before(CLAIMS, "}\nverify {", table)
        )),
        refusal(&owner("REQ-2-3", "decide", "decide", (18, 10)), 24, 10)
    );
}

/// A `covers` is a link: one ID with one text on two rows stays accepted, and the
/// ledger shows it once. With two texts, the second is refused.
#[test]
fn two_covers_of_one_id_need_one_text() {
    let large = "      covers REQ-3 \"A large claim is approved or rejected by a manager\"\n";
    let differing = CLAIMS.replacen(large, "      covers REQ-2 \"dup\"\n", 1);
    assert_eq!(
        summary(&check("covers_differ", &differing)),
        refusal(
            "decision table 'REQ-2-3' covers 'REQ-2' twice with different texts; a covered ID \
             names one requirement",
            22,
            7
        )
    );
    let same = CLAIMS.replacen(
        large,
        "      covers REQ-2 \"A small claim is auto-approved\"\n",
        1,
    );
    let scratch = Scratch::new("covers_same", &[("doc.fsl", &same)]);
    assert_eq!(scratch.fslc(&["check", "doc.fsl"]).0, 0);
    // The document violates its own `assuming`, so the ledger exits 1 either way.
    let (exit, _) = scratch.fslc(&["ledger", "doc.fsl", "--depth", "6", "-o", "ledger.md"]);
    assert_eq!(exit, 1);
    let ledger = scratch.read("ledger.md");
    assert_eq!(
        ledger
            .lines()
            .filter(|line| line.starts_with("| REQ-2 |"))
            .count(),
        1,
        "{ledger}"
    );
}

/// Every route the issue lists, on the issue's pair.
#[test]
fn every_route_refuses_the_second_rule() {
    let scratch = Scratch::new(
        "routes",
        &[
            ("clean.fsl", DESK),
            ("doc.fsl", &before(DESK, SECOND_RULE, RULE_B)),
        ],
    );
    let (exit, _) = scratch.fslc(&["document", "generate", "clean.fsl", "-o", "clean.md"]);
    assert_eq!(exit, 0);
    let expected = refusal(&owner("REQ-DESK-001", "rule", "rule", (19, 8)), 22, 8);
    let routes: [&[&str]; 9] = [
        &["check", "doc.fsl"],
        &["verify", "doc.fsl", "--depth", "6"],
        &["verify", "doc.fsl", "--engine", "explicit", "--depth", "6"],
        &["verify", "doc.fsl", "--engine", "induction"],
        &["ledger", "doc.fsl", "--depth", "6", "-o", "ledger.md"],
        &["document", "generate", "doc.fsl", "-o", "doc.md"],
        &["document", "claims", "doc.fsl"],
        &["document", "check", "doc.fsl", "clean.md"],
        &["kernel", "doc.fsl"],
    ];
    for args in routes {
        let (exit, stdout, packet) = scratch.run(args);
        assert_eq!(exit, 2, "{args:?}: {stdout}");
        assert_eq!(packet["result"], "error", "{args:?}: {stdout}");
        assert_eq!(packet["message"], expected["message"], "{args:?}: {stdout}");
    }
    assert!(!scratch.0.join("ledger.md").exists());
    assert!(!scratch.0.join("doc.md").exists());
}

/// Repeats an existing check refused keep that refusal, and merging repeats stay.
#[test]
fn other_repeats_are_unchanged() {
    let policy =
        "  policy POL-TICKET-001 \"dup\"\n    every Ticket in Assigned must eventually be Closed\n";
    let (exit, packet) = check("policy_twice", &before(DESK, "  goal GOAL", policy));
    assert_eq!(
        (exit, &packet["kind"]),
        (2, &json!("semantics")),
        "{packet:#}"
    );
    assert!(
        packet["message"]
            .as_str()
            .is_some_and(|text| text.starts_with("duplicate property name 'POL-TICKET-001'")),
        "{packet:#}"
    );
    let forbidden =
        "  forbidden FB-DESK-002 \"dup\" {\n    Agent withdraws t1\n    then rejected\n  }\n";
    let (exit, packet) = check("forbidden_twice", &before(DESK, "}\nverify {", forbidden));
    assert_eq!(exit, 2, "{packet:#}");
    assert!(
        packet["message"]
            .as_str()
            .is_some_and(|text| text.starts_with("duplicate forbidden id 'FB-DESK-002'")),
        "{packet:#}"
    );
    let lifecycle = DESK.replacen(
        "    lifecycle Open -> Withdrawn\n",
        "    lifecycle Open -> Withdrawn\n    lifecycle Open -> Withdrawn\n",
        1,
    );
    assert_eq!(check("lifecycle_twice", &lifecycle).0, 0);
}

/// The current edition shares IDs across `requirement`, `acceptance` and `forbidden`
/// and keeps two `requirement` blocks of one ID, with one ledger row. #1705 leaves it.
#[test]
fn the_current_edition_is_unchanged() {
    let source = |extra: &str| {
        format!(
            "requirements R {{\n  type K = 0..1\n  state {{ n: K }}\n  init {{ n = 0 }}\n  \
             requirement REQ-1 \"go sets n\" {{\n    action go() {{\n      requires n == 0\n      \
             n = 1\n    }}\n  }}\n  acceptance AC-1 \"go reaches one\" {{\n    go()\n    \
             expect n == 1\n  }}\n{extra}}}\n"
        )
    };
    for (label, extra) in [
        (
            "requirement_twice",
            "  requirement REQ-1 \"dup\" {\n    invariant small { n <= 1 }\n  }\n",
        ),
        (
            "acceptance_requirement",
            "  acceptance REQ-1 \"dup\" {\n    go()\n    expect n == 1\n  }\n",
        ),
        (
            "requirement_acceptance",
            "  requirement AC-1 \"dup\" {\n    invariant small { n <= 1 }\n  }\n",
        ),
    ] {
        assert_eq!(check(label, &source(extra)).0, 0, "{label}");
    }
}
