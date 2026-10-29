// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1528: declaration binders must not capture definition-body free state names.

#[path = "support/compose_scratch.rs"]
#[allow(dead_code)]
mod compose_scratch;

use compose_scratch::{run, scratch};
use serde_json::{Value, json};

fn source(position: &str, binders: &str) -> String {
    let (definition, body, hints, initial) = match position {
        "before" => ("Low() = c == 0", "Low() ~> level == 5", "", 1),
        "after" => ("High() = c == 1", "true ~> High()", "", 1),
        "decreases" => (
            "Rank() = if c == 1 then level else -1",
            "level > 0 ~> level == 0",
            "helpful down() decreases Rank()",
            1,
        ),
        "within" => (
            "Bound() = c + 1",
            "level > 0 ~> within Bound() level == 0",
            "",
            2,
        ),
        "helpful" => (
            "Arg() = c",
            "level > 0 ~> level == 0",
            "helpful down(Arg()) decreases level",
            1,
        ),
        _ => unreachable!("authored position"),
    };
    let (param, guard) = if position == "helpful" {
        ("x: Case", "x == 1 and ")
    } else {
        ("", "")
    };
    let mut property = body.to_owned();
    for binder in binders.split(',').map(str::trim).rev() {
        property = format!("forall {binder} {{ {property} }}");
    }
    format!(
        "spec P {{\n  type Case = 0..1\n  type One = 1..1\n  state {{ level: Int, c: Case }}\n  def {definition}\n  init {{ level = {initial} c = 0 }}\n  fair action down({param}) {{ requires {guard}level > 0 level = level - 1 }}\n  leadsTo L {{\n    {property}\n    {hints}\n  }}\n}}\n"
    )
}

fn observations(source: &str, component: bool) -> Vec<(Value, i32)> {
    let dir = scratch(
        "issue-1528-capture",
        &[
            ("a.fsl", source),
            (
                "root.fsl",
                "compose C {\n  use P as part from \"a.fsl\"\n}\n",
            ),
        ],
    );
    let file = if component { "root.fsl" } else { "a.fsl" };
    ["check", "bmc", "induction"]
        .into_iter()
        .map(|engine| {
            let args = if engine == "check" {
                vec!["check", file]
            } else {
                vec![
                    "verify",
                    file,
                    "--engine",
                    engine,
                    "--depth",
                    "4",
                    "--no-cache",
                ]
            };
            let actual = run(&dir, &args);
            eprintln!(
                "{engine} component={component} exit={} output={}",
                actual.1, actual.0
            );
            actual
        })
        .collect()
}

fn control(position: &str, binders: &str, component: bool) {
    let expected = match position {
        "before" | "after" => [(0, "ok"), (1, "violated"), (1, "violated")],
        "decreases" => [(0, "ok"), (0, "verified"), (1, "unknown_cti")],
        "helpful" => [(0, "ok"), (0, "verified"), (2, "error")],
        "within" => [(2, "error"); 3],
        _ => unreachable!("authored control"),
    };
    for ((output, code), (exit, result)) in observations(&source(position, binders), component)
        .iter()
        .zip(expected)
    {
        assert_eq!(
            (*code, output["result"].as_str()),
            (exit, Some(result)),
            "{position}: {output}"
        );
        if exit == 2 {
            assert_eq!(output["kind"], "semantics", "{output}");
        }
    }
}

fn detector(position: &str, binders: &str, component: bool, call: &str) {
    // Always calibrate the rename-only control before observing the capture.
    control(position, &binders.replace("c: One", "k: One"), component);
    let source = source(position, binders);
    let prefix = &source[..source.rfind(call).expect("authored call")];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
    let predicate = call.trim_end_matches("()");
    let message = format!(
        "predicate '{predicate}' call would capture variable 'c'; rename the binder at the call site at "
    );
    let direct_message = if position == "within" {
        "predicate 'Bound' call would capture variable 'c'; rename the binder at the call site at 9:41".to_owned()
    } else {
        format!("{message}{line}:{column}")
    };
    let actual = observations(&source, component);
    for (output, exit) in &actual {
        assert_eq!(
            (*exit, &output["result"], &output["kind"]),
            (2, &json!("error"), &json!("semantics")),
            "{position}: {output}"
        );
        if component {
            assert_eq!(output["loc"], json!({"line": 2, "column": 3}));
            assert!(
                output["message"].as_str().unwrap().starts_with(&format!(
                    "component \"a.fsl\" failed to lower ({message}a.fsl:{line}:{column}) at "
                )),
                "{output}"
            );
        } else {
            assert_eq!(output["message"], direct_message);
            assert!(output.get("loc").is_none(), "{output}");
        }
    }
    // No excluded fields: all three routes must return the same whole envelope.
    assert!(actual.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn before_capture_is_refused() {
    detector("before", "c: One", false, "Low()");
}

#[test]
fn after_capture_is_refused() {
    detector("after", "c: One", false, "High()");
}

#[test]
fn decreases_capture_is_refused() {
    detector("decreases", "c: One", false, "Rank()");
}

#[test]
fn helpful_argument_capture_is_refused() {
    detector("helpful", "c: One", false, "Arg()");
}

#[test]
fn component_capture_is_refused_at_component_call() {
    detector("before", "c: One", true, "Low()");
}

#[test]
fn second_declaration_binder_capture_is_refused() {
    detector("before", "a: One, c: One", false, "Low()");
}

#[test]
fn renamed_controls_preserve_verdicts() {
    for position in ["before", "after", "decreases", "within", "helpful"] {
        control(position, "k: One", false);
    }
    control("before", "k: One", true);
    control("before", "a: One, k: One", false);
}

#[test]
fn within_capture_reports_call_instead_of_constant_error() {
    // This is an error-to-error transition, not a false-green detector.
    detector("within", "c: One", false, "Bound()");
}

#[test]
fn definition_without_free_binder_name_still_passes() {
    let source = source("after", "c: One").replace("High() = c == 1", "High() = true");
    for ((output, exit), result) in observations(&source, false)
        .iter()
        .zip(["ok", "verified", "proved"])
    {
        assert_eq!(
            (*exit, output["result"].as_str()),
            (0, Some(result)),
            "{output}"
        );
    }
}
