// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1483: helpful actuals obey action parameter types, directly and through use.
//! Clause lines are source-derived; columns are recorded, not prescribed.

#[path = "support/compose_scratch.rs"]
#[allow(dead_code)] // This target uses only the shared fresh-fixture allocator.
mod compose_scratch;

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

// The abs(true) issue reproducer is not asserted: abs operand typing is a separate validation gap.
const BINDER: &str = "spec P {
  enum Color { Red, Blue }
  type Case = 0..1
  type Level = 0..2
  state { level: Map<Case, Level> }
  init { forall c: Case { level[c] = 2 } }
  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
";

const COMPOSE: &str = "compose C {\n  use P as part from \"a.fsl\"\n}\n";

fn fixture(params: &str, argument: &str) -> String {
    format!(
        "spec P {{\n  enum Level {{ Low, High }}\n  enum Color {{ Red, Blue }}\n  state {{ done: Bool }}\n  init {{ done = false }}\n  action step({params}) {{ done = true }}\n  leadsTo Responds {{\n    true ~> true\n    helpful step({argument})\n  }}\n}}\n"
    )
}

fn run_check(binary: &Path, directory: &Path, file: &str) -> (Value, i32) {
    let output = Command::new(binary)
        .args(["check", file])
        .current_dir(directory)
        .output()
        .expect("native check");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    (
        serde_json::from_slice(&output.stdout).expect("native JSON"),
        output.status.code().expect("native exit"),
    )
}

fn cases(reject: bool) -> Vec<(&'static str, String)> {
    if reject {
        vec![
            ("bool-to-enum", fixture("l: Level", "true")),
            ("int-to-enum", fixture("l: Level", "1")),
            ("wrong-enum", fixture("l: Level", "Red")),
            (
                "second-argument",
                fixture("l: Level, r: Level", "Low, true"),
            ),
        ]
    } else {
        vec![
            ("enum-label", fixture("l: Level", "Low")),
            (
                "enum-expression",
                fixture("l: Level", "if true then Low else High"),
            ),
            ("property-binder", BINDER.into()),
            ("two-arguments", fixture("l: Level, r: Level", "Low, High")),
        ]
    }
}

fn check_cases(reject: bool) {
    let binary = std::env::var_os("FSL_MATRIX_BINARY")
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_fslc")), PathBuf::from);
    let binary = std::fs::canonicalize(binary).expect("selected native binary");
    let report = compose_scratch::scratch("issue-1483-helpful-report", &[]);
    let mut records = Vec::new();
    let mut type_failures = 0;
    let mut location_failures = 0;
    let mut agreement_failures = 0;
    for (name, source) in cases(reject) {
        // Derived from the authored source, never from a diagnostic/binary.
        let clause_line = source
            .lines()
            .position(|line| line.contains("helpful step("))
            .expect("authored helpful clause")
            + 1;
        let directory = compose_scratch::scratch(
            "issue-1483-helpful-case",
            &[("a.fsl", &source), ("c.fsl", COMPOSE)],
        );
        let direct = run_check(&binary, &directory, "a.fsl");
        let composed = run_check(&binary, &directory, "c.fsl");
        agreement_failures +=
            usize::from(direct.1 != composed.1 || direct.0.get("kind") != composed.0.get("kind"));
        for (route, (output, code)) in [("direct", &direct), ("compose", &composed)] {
            let message = output.get("message").and_then(Value::as_str);
            let type_ok = if reject {
                *code == 2
                    && output["result"] == "error"
                    && output["kind"] == "semantics"
                    && message.is_some_and(|message| {
                        // Assert a type error, not a snapshot of one binary's wording.
                        ["type", "integer", "operand", "enum"]
                            .iter()
                            .any(|word| message.contains(word))
                            && ["Bool", "Int", "Level", "Color", "Case"]
                                .iter()
                                .any(|word| message.contains(word))
                    })
            } else {
                *code == 0 && output["result"] == "ok"
            };
            type_failures += usize::from(!type_ok);
            let location_ok = if reject && *code == 2 {
                // #1463 keeps compose's outer loc at use; the inner location
                // must still identify this component's helpful clause line.
                output["loc"].is_object()
                    && output["loc"]["column"].is_u64()
                    && if route == "direct" {
                        output["loc"]["line"] == clause_line
                    } else {
                        output["loc"]["line"] == 2
                            && message.is_some_and(|m| m.contains(&format!("a.fsl:{clause_line}:")))
                    }
            } else {
                true
            };
            location_failures += usize::from(!location_ok);
            let clause_column = if route == "direct" {
                output["loc"]["column"].as_u64()
            } else {
                message
                    .and_then(|m| m.split_once(&format!("a.fsl:{clause_line}:")))
                    .and_then(|(_, tail)| tail.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|column| column.parse::<u64>().ok())
            };
            records.push(json!({"case":name,"route":route,"expected_rejection":reject,"expected_clause_line":clause_line,"type_ok":type_ok,"location_ok":location_ok,"observed_loc":output.get("loc"),"observed_clause_column":clause_column,"output":output,"exit":code}));
        }
    }
    std::fs::write(
        report.join("report.json"),
        serde_json::to_vec(&records).expect("report JSON"),
    )
    .expect("record complete observations");
    eprintln!(
        "HELPFUL_TYPES reject={reject} observations={} type_failures={type_failures} location_failures={location_failures} agreement_failures={agreement_failures} binary={} report={}",
        records.len(),
        binary.display(),
        report.display()
    );
    assert_eq!(agreement_failures, 0, "direct/compose outcome drift");
    assert_eq!(
        type_failures, 0,
        "helpful actual/action parameter type contract"
    );
    assert_eq!(location_failures, 0, "helpful clause source location");
}

#[test]
fn wrong_kind_helpful_arguments_are_rejected() {
    check_cases(true);
}

#[test]
fn well_typed_helpful_arguments_are_accepted() {
    check_cases(false);
}
