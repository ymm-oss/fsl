// SPDX-License-Identifier: Apache-2.0

//! #1680: a repeated formal is refused at the repeated name on every route. A
//! composed component used to pass `check` and keep only the last actual, so
//! swapping two arguments flipped the verdict. Each packet is pinned whole. Outside
//! a component the message is the one base gave, and `loc` is what moved.

#[path = "support/issue_1517_scratch.rs"]
mod scratch;

use serde_json::{Value, json};
use std::{fs, process::Command};

/// The repeated `a` is at 1:81, the first one at 1:75.
const PIECE: &str = "spec Piece { type K = 0..1 state { last: K } init { last = 0 } action put(a: K, a: K) { last = a } invariant Zero { last == 0 } }\n";
/// The same action with its formals told apart: the body reads the second.
const DISTINCT: &str = "spec Piece { type K = 0..1 state { last: K } init { last = 0 } action put(a: K, b: K) { last = b } invariant Zero { last == 0 } }\n";
const AT_PIECE: &str = "component \"piece.fsl\" failed to check (part.put: duplicate action parameter 'a' at piece.fsl:1:81) at root.fsl:2:3";

fn internal(reference: &str) -> String {
    format!(
        "compose Root {{\n  use Piece as part from \"piece.fsl\"\n\n  action step() =\n      {reference} {{\n  }}\n\n  internal part.put\n}}\n"
    )
}

struct Run {
    packet: Value,
    exit: i32,
}

fn fslc(id: &str, files: &[(&str, &str)], arguments: &[&str], fsl5: bool) -> Run {
    let scratch = scratch::Scratch::new(
        "FSLC_1680_INPUTS",
        id,
        &format!("1680-{id}-{}", arguments[0]),
    );
    for (name, source) in files {
        fs::write(scratch.path().join(name), source).expect("fixture");
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(arguments)
        .current_dir(scratch.path())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE");
    if fsl5 {
        command.env("FSLC_UNSTABLE", "fsl5");
    }
    let output = command.output().expect("CLI");
    assert!(
        output.stderr.is_empty(),
        "{id}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Run {
        packet: serde_json::from_slice(&output.stdout).expect("JSON packet"),
        exit: output.status.code().expect("exit"),
    }
}

fn versions() -> Value {
    fsl_core::version_metadata(
        "fslc-rust",
        env!("CARGO_PKG_VERSION"),
        "native-z3",
        fsl_solver_z3::version(),
    )
}

fn refused(message: &str, line: u32, column: u32) -> Value {
    json!({"fsl":"1.0","result":"error","kind":"semantics","message":message,
        "loc":{"line":line,"column":column},"versions":versions()})
}

fn assert_refused(id: &str, files: &[(&str, &str)], message: &str, line: u32, column: u32) {
    let run = fslc(id, files, &["check", "root.fsl"], false);
    assert_eq!(
        (run.exit, &run.packet),
        (2, &refused(message, line, column)),
        "{id}"
    );
}

fn assert_checks(id: &str, files: &[(&str, &str)]) {
    let run = fslc(id, files, &["check", "root.fsl"], false);
    assert_eq!(
        (run.exit, &run.packet["result"]),
        (0, &json!("ok")),
        "{id}: {}",
        run.packet
    );
}

#[test]
fn standalone_points_at_the_repeated_name() {
    assert_refused(
        "standalone",
        &[("root.fsl", PIECE)],
        "duplicate action parameter 'a'",
        1,
        81,
    );
}

/// Base took the first token spelled like the name, here the state declaration.
#[test]
fn standalone_does_not_point_at_an_earlier_declaration_of_that_name() {
    assert_refused(
        "standalone_state_name",
        &[(
            "root.fsl",
            "spec P { type K = 0..1 state { v: K } init { v = 0 } action other() { v = 0 }\n  action put(v: K, v: K) { } }\n",
        )],
        "duplicate action parameter 'v'",
        2,
        20,
    );
}

#[test]
fn standalone_finds_a_repeat_that_is_not_adjacent_and_any_formal_form() {
    for (id, action, column) in [
        ("apart", "put(a: K, b: K, a: K)", 89),
        ("range", "put(a in 0..1, a in 0..1)", 88),
        ("mixed", "put(a: K, a in 0..1)", 83),
    ] {
        let source = format!(
            "spec Piece {{ type K = 0..1 state {{ last: Int }} init {{ last = 0 }} action {action} {{ last = a }} }}\n"
        );
        assert_refused(
            id,
            &[("root.fsl", source.as_str())],
            "duplicate action parameter 'a'",
            1,
            column,
        );
    }
}

/// The issue's table: both argument orders passed and disagreed on the verdict.
#[test]
fn composed_internal_action_is_refused_in_either_argument_order() {
    for (id, reference) in [
        ("internal_0_1", "part.put(0, 1)"),
        ("internal_1_0", "part.put(1, 0)"),
    ] {
        let root = internal(reference);
        assert_refused(
            id,
            &[("root.fsl", root.as_str()), ("piece.fsl", PIECE)],
            AT_PIECE,
            2,
            3,
        );
    }
}

#[test]
fn every_verify_engine_refuses_what_check_refuses() {
    let root = internal("part.put(0, 1)");
    for engine in ["bmc", "explicit", "induction"] {
        let run = fslc(
            engine,
            &[("root.fsl", root.as_str()), ("piece.fsl", PIECE)],
            &["verify", "root.fsl", "--engine", engine, "--depth", "2"],
            false,
        );
        assert_eq!(
            (run.exit, &run.packet["result"], &run.packet["message"]),
            (2, &json!("error"), &json!(AT_PIECE)),
            "{engine}: {}",
            run.packet
        );
    }
}

#[test]
fn composed_routes_that_do_not_substitute_are_refused_too() {
    for (id, root) in [
        (
            "lifted_referenced",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n\n  action step() =\n      part.put(0, 1) {\n  }\n}\n",
        ),
        (
            "lifted_unreferenced",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n}\n",
        ),
        (
            "internal_unreferenced",
            "compose Root {\n  use Piece as part from \"piece.fsl\"\n\n  internal part.put\n}\n",
        ),
    ] {
        assert_refused(
            id,
            &[("root.fsl", root), ("piece.fsl", PIECE)],
            AT_PIECE,
            2,
            3,
        );
    }
}

#[test]
fn composed_internal_action_that_is_never_reached_is_refused() {
    let piece = "spec Piece { type K = 0..1 state { last: K } init { last = 0 } action put(a: K, a: K) { last = 0 } action idle() { } }\n";
    assert_refused(
        "internal_never_reached",
        &[
            (
                "root.fsl",
                "compose Root {\n  use Piece as part from \"piece.fsl\"\n\n  action step() =\n      part.idle() {\n  }\n\n  internal part.put\n  internal part.idle\n}\n",
            ),
            ("piece.fsl", piece),
        ],
        AT_PIECE,
        2,
        3,
    );
}

/// A wrong actual no longer decides the report: the formal list is wrong first.
#[test]
fn composed_refusal_comes_before_typing_the_actuals() {
    let root = internal("part.put(true, 0)");
    assert_refused(
        "wrong_actual",
        &[("root.fsl", root.as_str()), ("piece.fsl", PIECE)],
        AT_PIECE,
        2,
        3,
    );
}

/// An arity failure stays ahead of it, as it is ahead of every declaration failure.
#[test]
fn arity_failure_keeps_its_place_and_location() {
    let root = internal("part.put(0)");
    assert_refused(
        "arity",
        &[("root.fsl", root.as_str()), ("piece.fsl", PIECE)],
        "sync reference 'part.put' expects 2 argument(s), got 1 at 4:3",
        4,
        3,
    );
}

#[test]
fn root_formals_point_at_the_root() {
    let root = "compose Root {\n  use Piece as part from \"piece.fsl\"\n\n  action step(a: part.K, a: part.K) =\n      part.put(a, 0) {\n  }\n\n  internal part.put\n}\n";
    assert_refused(
        "root_formals",
        &[("root.fsl", root), ("piece.fsl", DISTINCT)],
        "duplicate action parameter 'a'",
        4,
        26,
    );
}

#[test]
fn glue_action_formals_point_at_the_root() {
    let root = "compose Root {\n  use Piece as part from \"piece.fsl\"\n  state { seen: Bool }\n  init { seen = false }\n\n  action glue(a: Bool, a: Bool) { seen = a }\n}\n";
    assert_refused(
        "glue_formals",
        &[("root.fsl", root), ("piece.fsl", DISTINCT)],
        "duplicate action parameter 'a'",
        6,
        24,
    );
}

#[test]
fn requirements_action_points_at_the_repeated_name() {
    assert_refused(
        "requirements",
        &[(
            "root.fsl",
            "requirements R {\n  type K = 0..1\n  state { last: K }\n  init { last = 0 }\n  requirement REQ-1 \"put\" {\n    action put(a: K, a: K) { last = a }\n  }\n}\n",
        )],
        "duplicate action parameter 'a'",
        6,
        22,
    );
}

#[test]
fn process_transition_input_points_at_the_repeated_name() {
    assert_refused(
        "process",
        &[(
            "root.fsl",
            "requirements R {\n  number Amount\n  process Claim with amount: Amount = 0 {\n    stages Draft, Approved\n    initial Draft\n    transition approve Draft -> Approved by Manager with x: Amount, x: Amount\n  }\n}\nverify { instances Claim = 1 values Amount = 0..2 }\n",
        )],
        "duplicate action parameter 'x'",
        6,
        69,
    );
}

/// The generated process binder has no authored name; the input that meets it does.
#[test]
fn process_transition_input_meeting_the_generated_binder_points_at_the_input() {
    assert_refused(
        "process_binder",
        &[(
            "root.fsl",
            "requirements R {\n  number Amount\n  process Claim with amount: Amount = 0 {\n    stages Draft, Approved\n    initial Draft\n    transition approve Draft -> Approved by Manager with c: Claim\n  }\n}\nverify { instances Claim = 1 values Amount = 0..2 }\n",
        )],
        "duplicate action parameter 'c'",
        6,
        58,
    );
}

#[test]
fn next_edition_event_input_points_at_the_repeated_name() {
    let run = fslc(
        "event",
        &[(
            "root.fsl",
            "fsl 5\nrequirements R {\n  number Amount\n  actor Employee\n  entity Claim { amount: Amount = 0 lifecycle Draft -> Submitted }\n  event submit = Employee submits Claim with x: Amount, x: Amount Draft -> Submitted\n}\nverify { instances Claim = 1 values Amount = 0..1 }\n",
        )],
        &["check", "root.fsl"],
        true,
    );
    assert_eq!(run.exit, 2, "{}", run.packet);
    assert_eq!(run.packet["kind"], "semantics", "{}", run.packet);
    // The event's origin keeps its own place in the message; `loc` is the input.
    assert_eq!(
        run.packet["message"], "duplicate action parameter 'x' at root.fsl:6:3",
        "{}",
        run.packet
    );
    assert_eq!(
        run.packet["loc"],
        json!({"line":6,"column":57}),
        "{}",
        run.packet
    );
}

/// A `def` was already refused on every route, at the definition; it is a
/// different defect (no actual is dropped) and keeps its kind and location.
#[test]
fn def_parameters_keep_their_refusal() {
    let piece = "spec Piece { state { last: Int } init { last = 0 } def f(a: Int, a: Int) = a action put() { last = 0 } }\n";
    assert_refused(
        "def_component",
        &[
            (
                "root.fsl",
                "compose Root {\n  use Piece as part from \"piece.fsl\"\n}\n",
            ),
            ("piece.fsl", piece),
        ],
        "component \"piece.fsl\" failed to lower (duplicate parameter in def 'f' at piece.fsl:1:52) at root.fsl:2:3",
        2,
        3,
    );
}

#[test]
fn distinct_formals_still_check_and_decide_by_the_formal_they_read() {
    for (id, reference, verdict, exit) in [
        ("distinct_0_1", "part.put(0, 1)", "violated", 1),
        ("distinct_1_0", "part.put(1, 0)", "proved", 0),
    ] {
        let root = internal(reference);
        let files = [("root.fsl", root.as_str()), ("piece.fsl", DISTINCT)];
        assert_checks(id, &files);
        let run = fslc(
            id,
            &files,
            &["verify", "root.fsl", "--engine", "explicit", "--depth", "2"],
            false,
        );
        assert_eq!(
            (run.exit, &run.packet["result"]),
            (exit, &json!(verdict)),
            "{id}: {}",
            run.packet
        );
    }
}

#[test]
fn names_differing_in_case_and_across_actions_are_not_repeats() {
    for source in [
        "spec S { type K = 0..1 state { v: K } init { v = 0 } action put(a: K, A: K) { v = a } }\n",
        "spec S { type K = 0..1 state { v: K } init { v = 0 } action put(a: K) { v = a } action set(a: K) { v = a } }\n",
    ] {
        assert_checks("not_repeats", &[("root.fsl", source)]);
    }
}
