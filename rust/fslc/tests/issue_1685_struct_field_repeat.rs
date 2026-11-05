// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1685: a struct literal that named a field twice kept the last value and
//! dropped the rest, so swapping the two values reversed the verdict. The
//! literal is now refused at the repeated field, in every position and on every
//! route that loads the document. A literal that repeats nothing is unchanged.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const REPEAT: &str = "P { x: 1, x: 0, y: 0 }";
const ONE_EACH: &str = "P { x: 1, y: 0 }";

/// The spec of the issue, with its literal in `init`.
fn spec(literal: &str) -> String {
    format!(
        "spec S {{\n  type K = 0..2\n  struct P {{ x: K, y: K }}\n  state {{ p: P }}\n  \
         init {{ p = {literal} }}\n  action go() {{ p = p }}\n  invariant I {{ p.x == 1 }}\n}}\n"
    )
}

/// A scratch directory of source files.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, files: &[(&str, &str)]) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1685")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        for (name, source) in files {
            std::fs::write(directory.join(name), source).expect("write source");
        }
        Self(directory)
    }

    /// The exit and, when stdout is JSON, the packet of `fslc <args>`.
    fn fslc(&self, args: &[&str]) -> (i32, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env("FSLC_CACHE", "off")
            .env("FSLC_UNSTABLE", "fsl5")
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
        "diagnostic_code": packet["diagnostic_code"],
    })
}

fn refusal(field: &str, line: u32, column: u32) -> Value {
    json!({
        "exit": 2,
        "result": "error",
        "kind": "parse",
        "message": format!("duplicate field '{field}' at {line}:{column}"),
        "loc": {"line": line, "column": column},
        "diagnostic_code": "duplicate_struct_field",
    })
}

/// Every position a literal takes. Each template holds `LIT` once on the line
/// named, or more often when the position needs it; the first is refused.
const POSITIONS: &[(&str, &str, u32)] = &[
    (
        "init",
        "  state { p: P }\n  init { p = LIT }\n  action go() { p = p }\n",
        5,
    ),
    (
        "assignment",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { p = LIT }\n",
        6,
    ),
    (
        "let",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { let v = LIT\n    \
         p = v }\n",
        6,
    ),
    (
        "if statement",
        "  state { p: P, b: Bool }\n  init { p = P { x: 0, y: 0 }\n    b = true }\n  \
         action go() { if b { p = LIT } }\n",
        7,
    ),
    (
        "conditional",
        "  state { p: P, b: Bool }\n  init { p = P { x: 0, y: 0 }\n    b = true }\n  \
         action go() { p = if b then LIT else p }\n",
        7,
    ),
    (
        "requires",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { requires p != LIT\n    \
         p = p }\n",
        6,
    ),
    (
        "ensures",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { p = p\n    \
         ensures p != LIT or true }\n",
        7,
    ),
    (
        "invariant",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { p = p }\n  \
         invariant I { p != LIT or true }\n",
        7,
    ),
    (
        "quantifier",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { p = p }\n  \
         invariant I { forall i: K { p != LIT or true } }\n",
        7,
    ),
    (
        "reachable",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { p = p }\n  \
         reachable R { p == LIT }\n",
        7,
    ),
    (
        "leadsTo",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { p = p }\n  \
         leadsTo L { p == LIT ~> p == p }\n",
        7,
    ),
    (
        "def body",
        "  state { p: P }\n  def d(k: K) = LIT\n  init { p = P { x: 0, y: 0 } }\n  \
         action go() { p = d(0) }\n",
        5,
    ),
    (
        "def argument",
        "  state { p: P }\n  def d(v: P) = v.x\n  init { p = P { x: 0, y: 0 } }\n  \
         action go() { requires d(LIT) >= 0\n    p = p }\n",
        7,
    ),
    (
        "map value",
        "  state { m: Map<K, P> }\n  init { forall i: K { m[i] = P { x: 0, y: 0 } } }\n  \
         action go(i: K) { m[i] = LIT }\n",
        6,
    ),
    (
        "field read",
        "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  action go() { requires (LIT).y == 0\n    \
         p = p }\n",
        6,
    ),
];

fn position_spec(body: &str, literal: &str) -> String {
    format!(
        "spec S {{\n  type K = 0..2\n  struct P {{ x: K, y: K }}\n{}\n}}\n",
        body.replace("LIT", literal)
    )
}

#[test]
fn every_position_refuses_at_the_repeated_field_and_accepts_one_value_per_field() {
    for (label, body, line) in POSITIONS {
        let (exit, packet) = check(label, &position_spec(body, ONE_EACH));
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
        let source = position_spec(body, REPEAT);
        let text = source.lines().nth(*line as usize - 1).expect("the line");
        let repeated = text.find("x: 1, x: 0").expect("the literal") + "x: 1, ".len();
        let column = u32::try_from(repeated + 1).expect("column");
        assert_eq!(
            summary(&check(label, &source)),
            refusal("P.x", *line, column),
            "{label}"
        );
    }
}

/// The issue's table: both orders passed `check` and disagreed on the verdict.
/// The one-value forms show the verdict is live.
#[test]
fn both_orders_are_refused_on_every_engine_and_one_value_keeps_its_verdict() {
    let scratch = Scratch::new(
        "verdicts",
        &[
            ("a.fsl", &spec("P { x: 1, x: 2, y: 0 }")),
            ("b.fsl", &spec("P { x: 2, x: 1, y: 0 }")),
            ("one.fsl", &spec(ONE_EACH)),
            ("two.fsl", &spec("P { x: 2, y: 0 }")),
        ],
    );
    for (engine, one_result) in [
        ("bmc", "verified"),
        ("explicit", "proved"),
        ("induction", "proved"),
    ] {
        let run = |file: &str| scratch.fslc(&["verify", file, "--engine", engine, "--depth", "2"]);
        for file in ["a.fsl", "b.fsl"] {
            assert_eq!(
                summary(&run(file)),
                refusal("P.x", 5, 24),
                "{engine} {file}"
            );
        }
        let (exit, packet) = run("one.fsl");
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!(one_result)),
            "{engine}: {packet:#}"
        );
        let (exit, packet) = run("two.fsl");
        assert_eq!(
            (exit, &packet["result"]),
            (1, &json!("violated")),
            "{engine}: {packet:#}"
        );
    }
}

#[test]
fn a_literal_that_repeats_nothing_is_not_refused() {
    for (label, body) in [
        (
            "other-order",
            "  state { p: P }\n  init { p = P { y: 0, x: 1 } }\n  action go() { p = p }\n",
        ),
        (
            "two-literals",
            "  state { p: P }\n  init { p = P { x: 1, y: 0 } }\n  \
             action go() { requires p != P { x: 1, y: 1 }\n    p = P { x: 1, y: 0 } }\n",
        ),
        (
            "same-field-of-two-structs",
            "  struct Q { x: K, z: K }\n  state { p: P, q: Q }\n  \
             init { p = P { x: 1, y: 0 }\n    q = Q { x: 1, z: 0 } }\n  action go() { p = p }\n",
        ),
        (
            "field-read-in-a-field",
            "  state { p: P }\n  init { p = P { x: 0, y: 0 } }\n  \
             action go() { p = P { x: p.x, y: p.x } }\n",
        ),
    ] {
        let (exit, packet) = check(label, &position_spec(body, ""));
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
    }
}

#[test]
fn the_first_repeat_is_refused_whichever_field_it_is() {
    for (label, literal, field, column) in [
        ("second-field", "P { x: 1, y: 0, y: 1 }", "P.y", 30),
        ("apart", "P { x: 1, y: 0, x: 0 }", "P.x", 30),
        ("first-of-three", "P { x: 1, x: 0, x: 2, y: 0 }", "P.x", 24),
        ("two-repeats", "P { y: 0, y: 1, x: 0, x: 1 }", "P.y", 24),
        ("equal-values", "P { x: 1, x: 1, y: 0 }", "P.x", 24),
    ] {
        assert_eq!(
            summary(&check(label, &spec(literal))),
            refusal(field, 5, column),
            "{label}"
        );
    }
}

/// Every route that loads the document, with the literal both repeated and not.
#[test]
fn every_route_that_loads_the_document_refuses() {
    let map = "refinement M {\n  impl S\n  abs S\n  map p = p\n  action go() -> go()\n}\n";
    let log_map = "refinement LM {\n  impl Log\n  abs S\n  map p = p\n  action ev() -> go()\n}\n";
    let log = "{\"action\":\"ev\",\"params\":{},\"state\":{\"p\":{\"x\":1,\"y\":0}}}\n";
    let scratch = Scratch::new(
        "routes",
        &[
            ("doc.fsl", &spec(REPEAT)),
            ("one.fsl", &spec(ONE_EACH)),
            ("map.fsl", map),
            ("logmap.fsl", log_map),
            ("log.jsonl", log),
            ("trace.json", "{\"events\":[]}"),
        ],
    );
    let routes: &[&[&str]] = &[
        &["check", "F"],
        &["verify", "F", "--depth", "2"],
        &["verify", "F", "--engine", "explicit", "--depth", "2"],
        &["verify", "F", "--engine", "induction"],
        &["kernel", "F", "--kernel-version", "1"],
        &["kernel", "F", "--kernel-version", "2"],
        &["typestate", "F"],
        &["fmt", "F"],
        &["explain", "F"],
        &["analyze", "F"],
        &["conformance", "F"],
        &["scenarios", "F", "--depth", "2"],
        &["testgen", "F", "--depth", "2"],
        &["mutate", "F", "--depth", "2"],
        &["sweep", "F", "--depth", "0..1"],
        &["document", "claims", "F"],
        &["html", "F"],
        &["ledger", "F"],
        &["diff", "F", "one.fsl"],
        &["diff", "one.fsl", "F"],
        &["refine", "F", "one.fsl", "map.fsl"],
        &["refine", "one.fsl", "F", "map.fsl"],
        &["replay", "F", "--trace", "trace.json"],
        &[
            "replay",
            "F",
            "--from-log",
            "log.jsonl",
            "--mapping",
            "logmap.fsl",
        ],
    ];
    for route in routes {
        let with = |file: &'static str| {
            route
                .iter()
                .map(|argument| if *argument == "F" { file } else { argument })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            summary(&scratch.fslc(&with("doc.fsl"))),
            refusal("P.x", 5, 24),
            "{route:?}"
        );
        assert_eq!(scratch.fslc(&with("one.fsl")).0, 0, "{route:?}");
    }
    // `lint` and `migrate` add the file to `loc`, and `fmt --check` reports the
    // formatting of the control, whose result is not the point here.
    for command in ["lint", "migrate"] {
        let (exit, packet) = scratch.fslc(&[command, "doc.fsl"]);
        assert_eq!(
            (exit, &packet["kind"], &packet["loc"]),
            (
                2,
                &json!("parse"),
                &json!({"line": 5, "column": 24, "file": "doc.fsl"})
            ),
            "{command}: {packet:#}"
        );
        assert_eq!(scratch.fslc(&[command, "one.fsl"]).0, 0, "{command}");
    }
    assert_eq!(
        summary(&scratch.fslc(&["fmt", "--check", "doc.fsl"])),
        refusal("P.x", 5, 24)
    );
    assert_eq!(scratch.fslc(&["fmt", "--check", "one.fsl"]).0, 1);
}

const PIECE_HEAD: &str = "spec Piece {\n  type K = 0..1\n  struct P { x: K, y: K }\n  \
    state { p: P }\n  init { p = P { x: 0, y: 0 } }\n";

/// A component is refused whether its action is lifted, synchronized, or
/// internal and unreferenced: the document fails before any of that is decided.
#[test]
fn a_component_is_refused_at_the_use_naming_the_repeated_field() {
    for (label, actions, root, line) in [
        (
            "lifted",
            "  action put() { p = LIT }\n",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n}\n",
            6,
        ),
        (
            "synchronized",
            "  action put() { p = LIT }\n",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n  \
             action step() = part.put() { }\n}\n",
            6,
        ),
        (
            "internal-unreferenced",
            "  action idle() { p = p }\n  action put() { p = LIT }\n",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n  internal part.put\n}\n",
            7,
        ),
    ] {
        let piece = |literal: &str| format!("{PIECE_HEAD}{}}}\n", actions.replace("LIT", literal));
        let scratch = Scratch::new(
            &format!("component-{label}"),
            &[("piece.fsl", &piece(ONE_EACH)), ("root.fsl", root)],
        );
        let (exit, packet) = scratch.fslc(&["check", "root.fsl"]);
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
        std::fs::write(scratch.0.join("piece.fsl"), piece(REPEAT)).expect("write piece");
        assert_eq!(
            summary(&scratch.fslc(&["check", "root.fsl"])),
            json!({
                "exit": 2,
                "result": "error",
                "kind": "semantics",
                "message": format!(
                    "component \"piece.fsl\" failed to parse \
                     (duplicate field 'P.x' at piece.fsl:{line}:32) at root.fsl:2:3"
                ),
                "loc": {"line": 2, "column": 3},
                "diagnostic_code": null,
            }),
            "{label}"
        );
    }
}

#[test]
fn another_file_names_the_repeated_field() {
    let abstraction =
        |literal: &str| format!("{PIECE_HEAD}  action idle() {{ p = {literal} }}\n}}\n");
    let root = "requirements R {\n  implements Piece from \"abs.fsl\" {\n  }\n  number V\n  \
        process E with v: V {\n    stages A, B\n    initial A\n    transition go A -> B by U\n  \
        }\n}\nverify {\n  instances E = 1\n  values V = 0..1\n}\n";
    let scratch = Scratch::new(
        "implements",
        &[("abs.fsl", &abstraction(REPEAT)), ("root.fsl", root)],
    );
    assert_eq!(
        summary(&scratch.fslc(&["check", "root.fsl"])),
        json!({
            "exit": 2,
            "result": "error",
            "kind": "type",
            "message": "implements \"abs.fsl\" failed to parse \
                        (duplicate field 'P.x' at abs.fsl:6:33)",
            "loc": {"line": 2, "column": 3},
            "diagnostic_code": null,
        })
    );
    // With one value per field the same root reaches its next, unrelated error.
    std::fs::write(scratch.0.join("abs.fsl"), abstraction(ONE_EACH)).expect("write abs");
    let (exit, packet) = scratch.fslc(&["check", "root.fsl"]);
    assert_eq!(
        (exit, &packet["message"]),
        (2, &json!("missing map for abstract state variable 'p'")),
        "{packet:#}"
    );

    let mapping = |literal: &str| {
        format!(
            "refinement M {{\n  impl Piece\n  abs Piece\n  map p = {literal}\n  \
             action idle() -> idle()\n}}\n"
        )
    };
    let piece = format!("{PIECE_HEAD}  action idle() {{ p = p }}\n}}\n");
    let scratch = Scratch::new("mapping", &[("a.fsl", &piece), ("m.fsl", &mapping(REPEAT))]);
    let (exit, packet) = scratch.fslc(&["refine", "a.fsl", "a.fsl", "m.fsl"]);
    assert_eq!(
        (exit, &packet["kind"], &packet["message"], &packet["loc"]),
        (
            2,
            &json!("type"),
            &json!("duplicate field 'P.x'"),
            &json!({"line": 4, "column": 21})
        ),
        "{packet:#}"
    );
    std::fs::write(scratch.0.join("m.fsl"), mapping("P { x: 0, y: 0 }")).expect("write mapping");
    let (exit, packet) = scratch.fslc(&["refine", "a.fsl", "a.fsl", "m.fsl"]);
    assert_eq!(
        (exit, &packet["result"]),
        (0, &json!("refines")),
        "{packet:#}"
    );
}

#[test]
fn every_edition_and_the_literate_form_refuse() {
    let body = format!("{PIECE_HEAD}  action idle() {{ p = LIT }}\n}}\n");
    let plain = |literal: &str| body.replace("LIT", literal);
    let next = |literal: &str| format!("fsl 5\n{}", plain(literal));
    let literate = |literal: &str| format!("# t\n\n```fsl\n{}```\n", plain(literal));
    for (label, file, one, repeat, line) in [
        ("plain", "doc.fsl", plain(ONE_EACH), plain(REPEAT), 6),
        ("fsl5", "doc.fsl", next(ONE_EACH), next(REPEAT), 7),
        (
            "literate",
            "doc.md",
            literate(ONE_EACH),
            literate(REPEAT),
            9,
        ),
    ] {
        let scratch = Scratch::new(label, &[(file, &one)]);
        let (exit, packet) = scratch.fslc(&["check", file]);
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
        std::fs::write(scratch.0.join(file), &repeat).expect("write source");
        let (exit, packet) = scratch.fslc(&["check", file]);
        assert_eq!(
            (exit, &packet["kind"], &packet["loc"]),
            (2, &json!("parse"), &json!({"line": line, "column": 33})),
            "{label}: {packet:#}"
        );
    }
}

/// Next-edition and requirements positions, including a step's input, whose
/// reading is speculative and must not lose the location.
#[test]
fn requirements_positions_refuse_at_the_repeated_field() {
    const NEXT: &str = "fsl 5\nrequirements R {\n  actor A\n  struct P { x: V, y: V }\n  \
        number V\n  entity E {\n    v: V = 0\n    lifecycle S -> T\n  }\n";
    const BOUNDS: &str = "}\nverify {\n  instances E = 1\n  values V = 0..1\n}\n";
    let then = format!(
        "{NEXT}  event go = A does E with v S -> T\n  scenario AC \"t\" {{\n    \
         A does e1 with v 1\n    then LIT != P {{ x: 0, y: 0 }}\n  }}\n{BOUNDS}"
    );
    let guard = format!(
        "{NEXT}  event go = A does E with v S -> T\n  rule REQ-1 \"t\" {{\n    \
         allow A does E only when LIT != P {{ x: 0, y: 0 }}\n  }}\n{BOUNDS}"
    );
    let step_value = format!(
        "{NEXT}  event go = A does E with v S -> T\n  scenario AC \"t\" {{\n    \
         A does e1 with v (LIT).x\n    then e1 is T\n  }}\n{BOUNDS}"
    );
    let marked_value = format!(
        "{NEXT}  event go = A が E を does v で S -> T\n  scenario AC \"t\" {{\n    \
         A が e1 を v (LIT).x で does\n    then e1 is T\n  }}\n{BOUNDS}"
    );
    let legacy = "requirements R {\n  struct P { x: V, y: V }\n  number V\n  \
        process E with v: V {\n    stages A, B\n    initial A\n    transition go A -> B by U\n  \
        }\n  acceptance AC-1 \"t\" {\n    go(0)\n    expect LIT != P { x: 0, y: 0 }\n  }\n}\n\
        verify {\n  instances E = 1\n  values V = 0..1\n}\n"
        .to_owned();
    for (label, template) in [
        ("scenario-then", then),
        ("rule-guard", guard),
        ("step-value", step_value),
        ("marked-step-value", marked_value),
        ("legacy-expect", legacy),
    ] {
        let (exit, packet) = check(label, &template.replace("LIT", ONE_EACH));
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
        let source = template.replace("LIT", REPEAT);
        let (index, text) = source
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("x: 1, x: 0"))
            .expect("the literal");
        let repeated = text.chars().count()
            - text.split("x: 1, x: 0").nth(1).unwrap().chars().count()
            - "x: 0".len();
        let line = u32::try_from(index + 1).expect("line");
        let column = u32::try_from(repeated + 1).expect("column");
        assert_eq!(
            summary(&check(label, &source)),
            refusal("P.x", line, column),
            "{label}"
        );
    }
}

/// A domain value object already refused its repeated field, with its own kind,
/// when it is lowered. That stays as it was.
/// A literal that already failed for another reason now reports the repeat, as
/// `parse`, while the same literal with one value per field keeps its failure.
#[test]
fn an_already_failing_literal_reports_the_repeat() {
    let nested = |literal: &str| {
        format!(
            "spec S {{\n  type K = 0..2\n  struct Q {{ a: K }}\n  struct P {{ x: Q, y: K }}\n  \
             state {{ p: P }}\n  init {{ p = P {{ x: {literal}, y: 0 }} }}\n  \
             action go() {{ p = p }}\n}}\n"
        )
    };
    let (exit, packet) = check("nested-one", &nested("Q { a: 1 }"));
    assert_eq!((exit, &packet["kind"]), (2, &json!("type")), "{packet:#}");
    assert_eq!(
        summary(&check("nested-repeat", &nested("Q { a: 1, a: 2 }"))),
        refusal("Q.a", 6, 31)
    );
    let (exit, packet) = check("undefined-one", &spec("Q { a: 1 }"));
    assert_eq!(
        (exit, &packet["kind"]),
        (2, &json!("semantics")),
        "{packet:#}"
    );
    assert_eq!(
        summary(&check("undefined-repeat", &spec("Q { a: 1, a: 2 }"))),
        refusal("Q.a", 5, 24)
    );
}

/// A `domain` keeps `check`'s own refusal, and the routes that build no model
/// refuse in its words. `domain expand` writes nothing.
#[test]
fn a_domain_value_object_is_refused_on_every_domain_route() {
    let domain = |evolve: &str, state: &str, requires: &str, invariant: &str| {
        format!(
            "domain D {{\n  type Q = 0..2\n  value_object C {{ value: Q = 0; }}\n  \
             aggregate A {{\n    state {{ counter: C{state}; }}\n    command T {{}}\n    \
             event E {{}}\n    decide T {{{requires} emits E }}\n    evolve E {{\n      \
             counter = {evolve}\n    }}\n{invariant}  }}\n}}\n"
        )
    };
    let one = "C { value: 1 }";
    let repeat = "C { value: 1, value: 0 }";
    let requires = |literal: &str| format!(" requires counter == {literal}\n");
    let invariant = |literal: &str| format!("    invariant I {{ counter == {literal} }}\n");
    let state = |literal: &str| format!(" = {literal}");
    for (label, with, line, column) in [
        ("evolve", domain(repeat, "", "", ""), 10, 31),
        ("state default", domain(one, &state(repeat), "", ""), 5, 40),
        ("requires", domain(one, "", &requires(repeat), ""), 8, 50),
        ("invariant", domain(one, "", "", &invariant(repeat)), 12, 44),
    ] {
        let scratch = Scratch::new(
            &format!("domain-{label}"),
            &[("doc.fsl", &with), ("one.fsl", &domain(one, "", "", ""))],
        );
        let place = |file: &str| format!("duplicate field 'C.value' at {file}{line}:{column}");
        let expected = |file: &str| {
            (
                2,
                json!("semantics"),
                json!(place(file)),
                json!({"line": line, "column": column}),
            )
        };
        let run = |args: &[&str]| {
            let (exit, packet) = scratch.fslc(args);
            (
                exit,
                packet["kind"].clone(),
                packet["message"].clone(),
                packet["loc"].clone(),
            )
        };
        assert_eq!(run(&["check", "doc.fsl"]), expected("doc.fsl:"), "{label}");
        assert_eq!(
            run(&["domain", "check", "doc.fsl"]),
            expected("doc.fsl:"),
            "{label}"
        );
        assert_eq!(
            run(&["domain", "analyze", "doc.fsl"]),
            expected(""),
            "{label}"
        );
        assert_eq!(
            run(&["domain", "expand", "doc.fsl", "--output", "out.fsl"]),
            expected(""),
            "{label}"
        );
        assert!(!scratch.0.join("out.fsl").exists(), "{label}");
        for command in ["check", "analyze", "expand"] {
            let args = ["domain", command, "one.fsl"];
            assert_eq!(scratch.fslc(&args).0, 0, "{label} {command}");
        }
    }
    // The literal one value per field, in each position, is accepted.
    for (label, source) in [
        ("state default", domain(one, &state(one), "", "")),
        ("requires", domain(one, "", &requires(one), "")),
        ("invariant", domain(one, "", "", &invariant(one))),
    ] {
        let (exit, packet) = check(&format!("domain-one-{label}"), &source);
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
    }
}
