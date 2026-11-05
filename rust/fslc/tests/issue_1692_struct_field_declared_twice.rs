// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1692: a `struct` that declared a field twice was accepted, and `check` and the
//! bounds read different copies, so a value the first type admits was reported out
//! of bounds. The declaration is now refused at the repeated field, on every route
//! that loads the document. A struct that repeats nothing is unchanged.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// The issue's spec, with its field list and the literal `init` assigns.
fn spec(fields: &str, literal: &str) -> String {
    format!(
        "spec S {{\n  type K = 0..2\n  struct P {{ {fields} }}\n  state {{ p: P }}\n  \
         init {{ p = {literal} }}\n  action go() {{ p = p }}\n  invariant I {{ p.x == 1 }}\n}}\n"
    )
}

const REPEAT: &str = "x: K, x: Bool";
const LITERAL: &str = "P { x: 1 }";

/// A scratch directory of source files.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, files: &[(&str, &str)]) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1692")
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
    })
}

fn refusal(field: &str, line: u32, column: u32) -> Value {
    json!({
        "exit": 2,
        "result": "error",
        "kind": "name",
        "message": format!("duplicate struct field '{field}'"),
        "loc": {"line": line, "column": column},
    })
}

#[test]
fn a_repeated_field_is_refused_at_the_repeated_name() {
    for (label, fields, literal, field, column) in [
        ("first-type-kept", REPEAT, LITERAL, "P.x", 20),
        ("other-order", "x: Bool, x: K", LITERAL, "P.x", 23),
        ("same-type", "x: K, x: K", LITERAL, "P.x", 20),
        ("apart", "x: K, y: K, x: K", "P { x: 1, y: 0 }", "P.x", 26),
        (
            "first-repeat-of-two",
            "y: K, x: K, y: K, x: Bool",
            "P { x: 1, y: 0 }",
            "P.y",
            26,
        ),
    ] {
        assert_eq!(
            summary(&check(label, &spec(fields, literal))),
            refusal(field, 3, column),
            "{label}"
        );
    }
}

/// A struct no state reads loses nothing yet, and is refused all the same.
#[test]
fn an_unreferenced_struct_is_refused() {
    let source = format!(
        "spec S {{\n  type K = 0..2\n  struct P {{ {REPEAT} }}\n  state {{ n: K }}\n  \
         init {{ n = 0 }}\n  action go() {{ n = n }}\n}}\n"
    );
    assert_eq!(
        summary(&check("unreferenced", &source)),
        refusal("P.x", 3, 20)
    );
}

#[test]
fn a_struct_that_repeats_nothing_is_not_refused() {
    for (label, fields, literal) in [
        ("one", "x: K", LITERAL),
        ("two", "x: K, y: K", "P { x: 1, y: 0 }"),
        ("two-swapped", "y: K, x: K", "P { y: 0, x: 1 }"),
        ("trailing-comma", "x: K, y: K,", "P { x: 1, y: 0 }"),
    ] {
        let (exit, packet) = check(label, &spec(fields, literal));
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
    }
    // The same field name in two structs, and as a state variable, repeats nothing.
    let source = "spec S {\n  type K = 0..2\n  struct P { x: K }\n  struct Q { x: Bool }\n  \
        state { x: P, q: Q }\n  init { x = P { x: 1 }\n q = Q { x: true } }\n  \
        action go() { x = x }\n}\n";
    let (exit, packet) = check("two-structs", source);
    assert_eq!((exit, &packet["result"]), (0, &json!("ok")), "{packet:#}");
}

#[test]
fn every_engine_refuses_and_one_declaration_keeps_its_verdict() {
    let scratch = Scratch::new(
        "verdicts",
        &[
            ("doc.fsl", &spec(REPEAT, LITERAL)),
            ("one.fsl", &spec("x: K", LITERAL)),
        ],
    );
    for (engine, result) in [
        ("bmc", "verified"),
        ("explicit", "proved"),
        ("induction", "proved"),
    ] {
        let run = |file: &str| scratch.fslc(&["verify", file, "--engine", engine, "--depth", "2"]);
        let (exit, packet) = run("one.fsl");
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!(result)),
            "{engine}: {packet:#}"
        );
        assert_eq!(summary(&run("doc.fsl")), refusal("P.x", 3, 20), "{engine}");
    }
}

/// Every route that loads the document, with the field repeated and not.
#[test]
fn every_route_that_loads_the_document_refuses() {
    let scratch = Scratch::new(
        "routes",
        &[
            ("doc.fsl", &spec(REPEAT, LITERAL)),
            ("one.fsl", &spec("x: K", LITERAL)),
            (
                "map.fsl",
                "refinement M {\n  impl S\n  abs S\n  map p = p\n  action go() -> go()\n}\n",
            ),
            (
                "logmap.fsl",
                "refinement LM {\n  impl Log\n  abs S\n  map p = p\n  action ev() -> go()\n}\n",
            ),
            (
                "log.jsonl",
                "{\"action\":\"ev\",\"params\":{},\"state\":{\"p\":{\"x\":1}}}\n",
            ),
            ("trace.json", "{\"events\":[]}"),
        ],
    );
    let routes: &[&[&str]] = &[
        &["check", "F"],
        &["kernel", "F", "--kernel-version", "1"],
        &["kernel", "F", "--kernel-version", "2"],
        &["typestate", "F"],
        &["lint", "F"],
        &["migrate", "F"],
        &["sweep", "F", "--depth", "0..1"],
        &["explain", "F"],
        &["analyze", "F"],
        &["conformance", "F"],
        &["scenarios", "F", "--depth", "2"],
        &["testgen", "F", "--depth", "2"],
        &["mutate", "F", "--depth", "2"],
        &["html", "F"],
        &["ledger", "F"],
        &["document", "claims", "F"],
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
            refusal("P.x", 3, 20),
            "{route:?}"
        );
        assert_eq!(scratch.fslc(&with("one.fsl")).0, 0, "{route:?}");
    }
    // `fmt` reports a model failure as `semantics` with no `loc`, as it does for
    // a repeated enum member.
    for route in [&["fmt", "doc.fsl"][..], &["fmt", "--check", "doc.fsl"]] {
        assert_eq!(
            summary(&scratch.fslc(route)),
            json!({
                "exit": 2,
                "result": "error",
                "kind": "semantics",
                "message": "duplicate struct field 'P.x'",
                "loc": null,
            }),
            "{route:?}"
        );
    }
    assert_eq!(scratch.fslc(&["fmt", "one.fsl"]).0, 0);
}

const PIECE: &str = "spec Piece {\n  type K = 0..1\n  struct P { FIELDS }\n  state { p: P }\n  \
    init { p = P { x: 0 } }\n  action put() { p = p }\n  action hid() { p = p }\n}\n";

/// A struct stays in the composed model whichever way its component's actions are
/// used, so each use refuses, naming the component's file.
#[test]
fn a_component_is_refused_at_the_use_naming_the_repeated_field() {
    for (label, root) in [
        (
            "lifted",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n}\n",
        ),
        (
            "synchronized",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n  \
             action step() = part.put() { }\n}\n",
        ),
        (
            "internal",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n  internal part.hid\n}\n",
        ),
    ] {
        let piece = PIECE.replace("FIELDS", REPEAT);
        let scratch = Scratch::new(label, &[("piece.fsl", &piece), ("root.fsl", root)]);
        assert_eq!(
            summary(&scratch.fslc(&["check", "root.fsl"])),
            json!({
                "exit": 2,
                "result": "error",
                "kind": "semantics",
                "message": "component \"piece.fsl\" failed to check \
                            (duplicate struct field 'P.x' at piece.fsl:3:20) at root.fsl:2:3",
                "loc": {"line": 2, "column": 3},
            }),
            "{label}"
        );
        std::fs::write(scratch.0.join("piece.fsl"), PIECE.replace("FIELDS", "x: K"))
            .expect("write piece");
        let (exit, packet) = scratch.fslc(&["check", "root.fsl"]);
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!("ok")),
            "{label}: {packet:#}"
        );
    }
}

/// `loc` stays on the `implements`, so the message names the other file's place.
#[test]
fn another_file_names_the_repeated_field() {
    let abstraction = |fields: &str| {
        format!(
            "spec Abs {{\n  type K = 0..1\n  struct P {{ {fields} }}\n  state {{ n: K }}\n  \
             init {{ n = 0 }}\n  action idle() {{ n = n }}\n}}\n"
        )
    };
    let root = "requirements R {\n  implements Abs from \"abs.fsl\" {\n  }\n  number V\n  \
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
            "message": "implements \"abs.fsl\" failed to check \
                        (duplicate struct field 'P.x' at abs.fsl:3:20)",
            "loc": {"line": 2, "column": 3},
        })
    );
    // With one declaration the same root reaches its next, unrelated error.
    std::fs::write(scratch.0.join("abs.fsl"), abstraction("x: K")).expect("write abs");
    let (exit, packet) = scratch.fslc(&["check", "root.fsl"]);
    assert_eq!(exit, 2, "{packet:#}");
    assert!(
        !packet["message"]
            .as_str()
            .is_some_and(|message| message.contains("duplicate")),
        "{packet:#}"
    );
}

/// `domain expand` and `domain analyze` build no model, so a value object is
/// refused on the path every domain route shares. `expand` writes nothing.
#[test]
fn a_value_object_is_refused_on_every_domain_route() {
    let domain = |extra: &str| {
        format!(
            "domain D {{\n  type Q = 0..2\n  value_object C {{ value: Q = 0; {extra}}}\n  \
             aggregate A {{\n    state {{ counter: C; }}\n    command T {{}}\n    event E {{}}\n    \
             decide T {{ emits E }}\n    evolve E {{\n      counter = C {{ value: 1 }}\n    }}\n  \
             }}\n}}\n"
        )
    };
    for (label, extra) in [
        ("other-type", "value: Bool = false; "),
        ("same-type", "value: Q = 0; "),
    ] {
        let scratch = Scratch::new(
            &format!("domain-{label}"),
            &[("doc.fsl", &domain(extra)), ("one.fsl", &domain(""))],
        );
        for (args, place) in [
            (&["check", "doc.fsl"][..], "doc.fsl:3:34"),
            (&["domain", "check", "doc.fsl"], "doc.fsl:3:34"),
            (&["domain", "testgen", "doc.fsl"], "doc.fsl:3:34"),
            (&["domain", "generate", "doc.fsl"], "doc.fsl:3:34"),
            (&["domain", "analyze", "doc.fsl"], "3:34"),
            (
                &["domain", "expand", "doc.fsl", "--output", "out.fsl"],
                "3:34",
            ),
        ] {
            assert_eq!(
                summary(&scratch.fslc(args)),
                json!({
                    "exit": 2,
                    "result": "error",
                    "kind": "name",
                    "message": format!("duplicate struct field 'C.value' at {place}"),
                    "loc": {"line": 3, "column": 34},
                }),
                "{label} {args:?}"
            );
        }
        assert!(!scratch.0.join("out.fsl").exists(), "{label}");
        for command in ["check", "analyze", "expand"] {
            let args = ["domain", command, "one.fsl"];
            assert_eq!(scratch.fslc(&args).0, 0, "{label} {command}");
        }
    }
}

#[test]
fn every_edition_and_the_literate_form_refuse() {
    let source = spec(REPEAT, LITERAL);
    assert_eq!(
        summary(&check("next-edition", &format!("fsl 5\n{source}"))),
        refusal("P.x", 4, 20)
    );
    let scratch = Scratch::new(
        "literate",
        &[("doc.md", &format!("# T\n\n```fsl\n{source}```\n"))],
    );
    assert_eq!(
        summary(&scratch.fslc(&["check", "doc.md"])),
        refusal("P.x", 6, 20)
    );
    let requirements = format!(
        "requirements R {{\n  type K = 0..1\n  struct P {{ {REPEAT} }}\n  entity E\n  \
         state {{ n: K }}\n  init {{ n = 0 }}\n  action go() {{ n = n }}\n}}\n\
         verify {{\n  instances E = 1\n}}\n"
    );
    assert_eq!(
        summary(&check("requirements", &requirements)),
        refusal("P.x", 3, 20)
    );
}

/// A declaration that already failed for another reason now reports the repeat,
/// while the same declaration without it keeps its failure.
#[test]
fn an_already_failing_declaration_reports_the_repeat() {
    for (label, one, repeat, column) in [
        ("non-scalar", "x: Set<K>", "x: Set<K>, x: K", 25),
        ("unknown-type", "x: Nope", "x: Nope, x: K", 23),
    ] {
        let (exit, packet) = check(&format!("{label}-one"), &spec(one, LITERAL));
        assert_eq!(
            (exit, &packet["kind"]),
            (2, &json!("type")),
            "{label}: {packet:#}"
        );
        assert_eq!(
            summary(&check(label, &spec(repeat, LITERAL))),
            refusal("P.x", 3, column),
            "{label}"
        );
    }
}
