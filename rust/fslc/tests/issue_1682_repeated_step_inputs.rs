// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1682: a scenario or forbidden step that names an input twice kept the last
//! value and dropped the rest, so swapping the two values changed the result.
//! `check` now refuses the step at the repeated name, on every route that
//! loads the document. A step that repeats nothing keeps its result.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const EN: (&str, &str) = ("e2e", "fsl5_en_decide.fsl");
const JA: (&str, &str) = ("e2e", "fsl5_ja_particles.fsl");
const ROLE: (&str, &str) = ("relations", "fsl5_en.fsl");

const EN_STEP: &str = "Employee submits claim1 with amount 1";
const JA_STEP: &str = "従業員 が 申請1 を 申請金額 1 で 提出する";
const ROLE_STEP: &str = "Agent takes t1 with assignee 1";

/// One event with two inputs, and a rule that reads one of them, so a value
/// given to the wrong input fails the scenario.
const TWO_INPUTS: &str = "fsl 5
requirements R {
  number V
  actor A
  entity E {
    v: V = 0
    w: V = 0
    lifecycle S -> T
  }
  event go = A does E with v, w S -> T
  rule REQ-1 \"t\" {
    allow A does E only when input v > 0
  }
  scenario AC \"t\" {
    STEPS
    then e1 is T
  }
}
verify {
  instances E = 2
  values V = 0..1
}
";

fn fixture_dir(example: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5")
        .join(example)
}

fn fixture((example, file): (&str, &str)) -> String {
    std::fs::read_to_string(fixture_dir(example).join(file)).expect("read fixture")
}

/// `source` with `old`, which occurs exactly once, replaced by `new`.
fn edit(source: &str, old: &str, new: &str) -> String {
    assert_eq!(source.matches(old).count(), 1, "{old:?}");
    source.replacen(old, new, 1)
}

/// A scratch directory holding a copy of one fixture example.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, example: Option<&str>) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1682")
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

    fn write(&self, name: &str, source: &str) {
        std::fs::write(self.0.join(name), source).expect("write scratch source");
    }

    /// The exit and, when stdout is JSON, the packet of `fslc <args>`.
    fn fslc(&self, args: &[&str]) -> (i32, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env("FSLC_UNSTABLE", "fsl5")
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run fslc");
        (
            output.status.code().expect("exit code"),
            serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `check` of `source` beside its example's other files.
fn check(label: &str, example: &str, source: &str) -> (i32, Value) {
    let scratch = Scratch::new(label, Some(example));
    scratch.write("doc.fsl", source);
    scratch.fslc(&["check", "doc.fsl"])
}

fn summary((exit, packet): &(i32, Value)) -> Value {
    json!({
        "exit": exit,
        "result": packet["result"],
        "kind": packet["kind"],
        "message": packet["message"],
        "loc": packet["loc"],
        "diagnostic_code": packet["diagnostic_code"],
    })
}

fn refusal(input: &str, line: u32, column: u32) -> Value {
    json!({
        "exit": 2,
        "result": "error",
        "kind": "semantics",
        "message": format!("duplicate step input '{input}' at doc.fsl:{line}:{column}"),
        "loc": {"line": line, "column": column},
        "diagnostic_code": null,
    })
}

#[test]
fn a_repeated_input_is_refused_at_the_repeated_name_whatever_its_values() {
    let source = fixture(EN);
    for (label, step, column) in [
        (
            "en-2-1",
            "Employee submits claim1 with amount 2, amount 1",
            44,
        ),
        (
            "en-1-2",
            "Employee submits claim1 with amount 1, amount 2",
            44,
        ),
        (
            "en-same",
            "Employee submits claim1 with amount 1, amount 1",
            44,
        ),
        (
            "en-instance-first",
            "Employee claim1 submits with amount 2, amount 1",
            44,
        ),
        (
            "en-first-of-three",
            "Employee submits claim1 with amount 2, amount 2, amount 1",
            44,
        ),
        // The dropped value never reached the type check.
        (
            "en-untyped",
            "Employee submits claim1 with amount true, amount 1",
            47,
        ),
    ] {
        let run = check(label, "e2e", &edit(&source, EN_STEP, step));
        assert_eq!(summary(&run), refusal("amount", 43, column), "{label}");
    }
}

#[test]
fn a_repeated_input_is_refused_in_a_forbidden_trace() {
    let source = edit(
        &fixture(EN),
        "  scenario AC-2 ",
        "  forbidden FB-1 \"t\" {\n    Employee submits claim1 with amount 2, amount 1\n    \
         Finance pays claim1\n    then rejected\n  }\n  scenario AC-2 ",
    );
    let line = source
        .lines()
        .position(|line| line.contains("amount 2, amount 1"))
        .expect("the forbidden step");
    let line = u32::try_from(line + 1).expect("line number");
    assert_eq!(
        summary(&check("forbidden", "e2e", &source)),
        refusal("amount", line, 44)
    );
}

#[test]
fn a_repeated_input_is_refused_when_written_with_particles() {
    let source = fixture(JA);
    for (label, step, column) in [
        (
            "ja-list",
            "従業員 が 申請1 を 申請金額 2, 申請金額 1 で 提出する",
            25,
        ),
        (
            "ja-list-swapped",
            "従業員 が 申請1 を 申請金額 1, 申請金額 2 で 提出する",
            25,
        ),
        (
            "ja-groups",
            "従業員 が 申請1 を 申請金額 2 で 申請金額 1 で 提出する",
            26,
        ),
        (
            "ja-around-verb",
            "申請金額 2 で 従業員 が 申請1 を 提出する 申請金額 1 で",
            31,
        ),
        // An alias and its canonical name are one input.
        (
            "ja-alias-and-name",
            "従業員 が 申請1 を 申請金額 2, amount 1 で 提出する",
            25,
        ),
    ] {
        let run = check(label, "e2e", &edit(&source, JA_STEP, step));
        assert_eq!(
            summary(&run),
            refusal("申請金額（amount）", 75, column),
            "{label}"
        );
    }
}

/// The instance a relation records is an input named by the role, read from
/// the same step inputs.
#[test]
fn a_repeated_relation_role_is_refused() {
    let source = fixture(ROLE);
    for (label, step) in [
        ("role-0-1", "Agent takes t1 with assignee 0, assignee 1"),
        ("role-1-0", "Agent takes t1 with assignee 1, assignee 0"),
    ] {
        let run = check(label, "relations", &edit(&source, ROLE_STEP, step));
        assert_eq!(summary(&run), refusal("assignee", 37, 37), "{label}");
    }
}

/// The issue's table, for the two lines that repeat nothing: the second fails
/// on its own, so the scenario is live and the value is read.
#[test]
fn a_step_with_one_value_per_input_keeps_its_result() {
    let source = fixture(EN);
    let (exit, packet) = check("en-1", "e2e", &source);
    assert_eq!((exit, &packet["result"]), (0, &json!("ok")), "{packet:#}");
    let (exit, packet) = check(
        "en-2",
        "e2e",
        &edit(&source, EN_STEP, "Employee submits claim1 with amount 2"),
    );
    assert_eq!(
        (exit, &packet["kind"], &packet["loc"]),
        (2, &json!("acceptance"), &json!({"line": 44, "column": 5})),
        "{packet:#}"
    );
}

/// Inputs bind by name: the order of two different inputs does not matter, and
/// the same input may be named once per step.
#[test]
fn different_inputs_and_different_steps_are_not_repeats() {
    let document = |steps: &str| TWO_INPUTS.replace("STEPS", steps);
    for (label, steps) in [
        ("v-w", "A does e1 with v 1, w 0"),
        ("w-v", "A does e1 with w 0, v 1"),
        ("equal-values", "A does e1 with v 1, w 1"),
        (
            "two-steps",
            "A does e2 with v 1, w 0\n    A does e1 with v 1, w 0",
        ),
    ] {
        let scratch = Scratch::new(label, None);
        scratch.write("doc.fsl", &document(steps));
        let (exit, packet) = scratch.fslc(&["check", "doc.fsl"]);
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
    }
    // The rule reads `v`, so giving its value to `w` fails: these bind by name.
    for (label, steps) in [
        ("rule-v-w", "A does e1 with v 0, w 1"),
        ("rule-w-v", "A does e1 with w 1, v 0"),
    ] {
        let scratch = Scratch::new(label, None);
        scratch.write("doc.fsl", &document(steps));
        let (exit, packet) = scratch.fslc(&["check", "doc.fsl"]);
        assert_eq!(
            (exit, &packet["kind"]),
            (2, &json!("acceptance")),
            "{label}: {packet:#}"
        );
    }
}

/// What a step already got wrong is reported first and unchanged.
#[test]
fn an_earlier_parse_error_keeps_its_report() {
    let source = fixture(EN);
    for (label, step, message, column) in [
        (
            "unknown-input-repeated",
            "Employee submits claim1 with amount 1, nope 2, nope 3",
            "event 'submit' has no input 'nope' at 43:52",
            52,
        ),
        (
            "required-input-missing",
            "Employee submits claim1 with nope 2, nope 1",
            "step 'Employee submits' claim1 gives no value for input 'amount' of event 'submit' \
             (`with amount <value>`) at 43:5",
            5,
        ),
        (
            "two-with",
            "Employee submits claim1 with amount 2 with amount 1",
            "expected identifier at 43:55",
            55,
        ),
    ] {
        let (exit, packet) = check(label, "e2e", &edit(&source, EN_STEP, step));
        assert_eq!(
            (
                exit,
                &packet["kind"],
                &packet["message"],
                &packet["loc"],
                &packet["diagnostic_code"]
            ),
            (
                2,
                &json!("parse"),
                &json!(message),
                &json!({"line": 43, "column": column}),
                &json!("FSL-PARSE")
            ),
            "{label}: {packet:#}"
        );
    }
}

/// `document` reports the refusal with no `loc` and no file in the message.
#[test]
fn document_refuses_without_a_location() {
    let source = fixture(EN);
    let scratch = Scratch::new("document", Some("e2e"));
    scratch.write("one.fsl", &source);
    scratch.write(
        "doc.fsl",
        &edit(
            &source,
            EN_STEP,
            "Employee submits claim1 with amount 2, amount 1",
        ),
    );
    for command in ["claims", "generate"] {
        let (exit, packet) = scratch.fslc(&["document", command, "doc.fsl"]);
        assert_eq!(
            (exit, &packet["kind"], &packet["message"], &packet["loc"]),
            (
                2,
                &json!("semantics"),
                &json!("duplicate step input 'amount' at 43:44"),
                &Value::Null
            ),
            "{command}: {packet:#}"
        );
        assert_eq!(
            scratch.fslc(&["document", command, "one.fsl"]).0,
            0,
            "{command}"
        );
    }
}

/// Every route that loads the document gives the same refusal, and accepts the
/// same document with one value per input.
#[test]
fn every_route_that_loads_the_document_refuses() {
    let source = fixture(EN);
    let scratch = Scratch::new("routes", Some("e2e"));
    scratch.write("one.fsl", &source);
    scratch.write(
        "doc.fsl",
        &edit(
            &source,
            EN_STEP,
            "Employee submits claim1 with amount 2, amount 1",
        ),
    );
    for args in [
        &["check"][..],
        &["verify", "--depth", "4"],
        &["verify", "--engine", "explicit", "--depth", "4"],
        &["verify", "--engine", "induction"],
        &["explain"],
        &["analyze"],
        &["conformance"],
        &["scenarios", "--depth", "3"],
        &["testgen", "--depth", "3"],
        &["mutate", "--depth", "3"],
        &["sweep", "--depth", "0..1"],
        &["html"],
        &["ledger"],
    ] {
        let run = |file: &str| {
            let mut full = vec![args[0], file];
            full.extend_from_slice(&args[1..]);
            scratch.fslc(&full)
        };
        assert_eq!(
            summary(&run("doc.fsl")),
            refusal("amount", 43, 44),
            "{args:?}"
        );
        assert_eq!(run("one.fsl").0, 0, "{args:?}");
    }
    // These name the file differently: `kernel` version 2 by a longer path,
    // which depends on where the scratch directory is, and the others not at all.
    for (args, place) in [
        (
            &["kernel", "doc.fsl", "--kernel-version", "1"][..],
            " 43:44",
        ),
        (
            &["kernel", "doc.fsl", "--kernel-version", "2"],
            "doc.fsl:43:44",
        ),
        (&["typestate", "doc.fsl"], " 43:44"),
    ] {
        let (exit, packet) = scratch.fslc(args);
        let message = packet["message"].as_str().unwrap_or_default();
        assert!(
            message.starts_with("duplicate step input 'amount' at") && message.ends_with(place),
            "{args:?}: {packet:#}"
        );
        assert_eq!(
            (exit, &packet["kind"], &packet["loc"]),
            (2, &json!("semantics"), &json!({"line": 43, "column": 44})),
            "{args:?}: {packet:#}"
        );
        let mut one = args.to_vec();
        one[1] = "one.fsl";
        assert_eq!(scratch.fslc(&one).0, 0, "{args:?}");
    }
    // These report the refusal with no `loc`; the message keeps the place.
    for command in ["lint", "fmt", "migrate"] {
        let (exit, packet) = scratch.fslc(&[command, "doc.fsl"]);
        assert_eq!(
            (exit, &packet["kind"], &packet["message"]),
            (
                2,
                &json!("semantics"),
                &json!("duplicate step input 'amount' at doc.fsl:43:44")
            ),
            "{command}: {packet:#}"
        );
        assert_ne!(scratch.fslc(&[command, "one.fsl"]).0, 2, "{command}");
    }
    let (exit, packet) = scratch.fslc(&["diff", "doc.fsl", "one.fsl"]);
    assert_eq!(
        (exit, &packet["kind"]),
        (2, &json!("semantics")),
        "{packet:#}"
    );
    assert_eq!(scratch.fslc(&["diff", "one.fsl", "one.fsl"]).0, 0);
}

#[test]
fn a_repeat_in_another_file_is_refused_where_that_file_is_loaded() {
    let abstraction = TWO_INPUTS.replace("STEPS", "A does e1 with v 1, w 0, v 0");
    let one_each = TWO_INPUTS.replace("STEPS", "A does e1 with v 1, w 0");
    let implementation = |from: &str| {
        format!(
            "fsl 5
requirements Impl {{
  implements R from \"{from}\" {{
    action go(e, v, w) -> go(e, v, w)
  }}
  number V
  actor A
  entity E {{
    v: V = 0
    w: V = 0
    lifecycle S -> T
  }}
  event go = A does E with v, w S -> T
  rule REQ-1 \"t\" {{
    allow A does E only when input v > 0
  }}
}}
verify {{
  instances E = 2
  values V = 0..1
}}
"
        )
    };
    let scratch = Scratch::new("other-file", None);
    scratch.write("rep.fsl", &abstraction);
    scratch.write("one.fsl", &one_each);
    scratch.write("impl_rep.fsl", &implementation("rep.fsl"));
    scratch.write("impl_one.fsl", &implementation("one.fsl"));
    scratch.write(
        "map.fsl",
        "refinement M {
  impl R
  abs R
  map e_stage[e: E] = e_stage[e]
  map e_v[e: E] = e_v[e]
  map e_w[e: E] = e_w[e]
  action go(e, v, w) -> go(e, v, w)
}
",
    );

    let (exit, packet) = scratch.fslc(&["check", "impl_one.fsl"]);
    assert_eq!((exit, &packet["result"]), (0, &json!("ok")), "{packet:#}");
    assert_eq!(
        summary(&scratch.fslc(&["check", "impl_rep.fsl"])),
        json!({
            "exit": 2,
            "result": "error",
            "kind": "type",
            "message": "implements \"rep.fsl\" failed to check \
                        (duplicate step input 'v' at rep.fsl:15:30)",
            "loc": {"line": 3, "column": 3},
            "diagnostic_code": null,
        })
    );

    let (exit, packet) = scratch.fslc(&["refine", "one.fsl", "one.fsl", "map.fsl"]);
    assert_eq!(
        (exit, &packet["result"]),
        (0, &json!("refines")),
        "{packet:#}"
    );
    for args in [
        ["refine", "rep.fsl", "one.fsl", "map.fsl"],
        ["refine", "one.fsl", "rep.fsl", "map.fsl"],
    ] {
        let (exit, packet) = scratch.fslc(&args);
        assert_eq!(
            (exit, &packet["kind"], &packet["message"], &packet["loc"]),
            (
                2,
                &json!("semantics"),
                &json!("duplicate step input 'v' at rep.fsl:15:30"),
                &json!({"line": 15, "column": 30})
            ),
            "{args:?}: {packet:#}"
        );
    }
}

/// `replay` loads the document before it reads the trace.
#[test]
fn replay_refuses_before_reading_the_trace() {
    let scratch = Scratch::new("replay", None);
    scratch.write(
        "doc.fsl",
        &TWO_INPUTS.replace("STEPS", "A does e1 with v 1, w 0, v 0"),
    );
    scratch.write("trace.json", "{\"steps\":[]}");
    let (exit, packet) = scratch.fslc(&["replay", "doc.fsl", "--trace", "trace.json"]);
    assert_eq!(
        (exit, &packet["kind"], &packet["loc"]),
        (2, &json!("semantics"), &json!({"line": 15, "column": 30})),
        "{packet:#}"
    );
}
