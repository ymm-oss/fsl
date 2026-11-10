// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1700: a `domain` declaration repeated in a way that loses a copy was accepted. A
//! repeated `evolve` flipped `verify`, and the order of an `event`'s fields decided
//! `check`, while `domain expand` wrote a kernel that keeps no event type. Each is now
//! refused at the repeat, on every route that loads the document. A kind that reads
//! every copy is unchanged.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const ASYNC_EFFECT: &str = include_str!("../../../examples/domain/order_async_effect.fsl");
const SAGA: &str = include_str!("../../../examples/domain/order_fulfillment_saga.fsl");

/// The issue's document: an event, the type of the command input it copies, and the
/// assignment that reads the event's field.
fn domain(event: &str, input: &str, assign: &str) -> String {
    format!(
        "domain D {{\n  type Q = 0..2\n  aggregate A {{\n    id AId\n    \
         state {{ n: Q = 0; flag: Bool = false; }}\n    command T {{ a: {input}; }}\n    \
         event E {{ {event} }}\n    decide T {{ emits E }}\n    evolve E {{\n      {assign}\n    \
         }}\n  }}\n}}\n"
    )
}

/// A scratch directory of source files.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, files: &[(&str, &str)]) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1700")
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
            .output()
            .expect("run fslc");
        (
            output.status.code().expect("exit code"),
            String::from_utf8(output.stdout.clone()).expect("utf-8 stdout"),
            serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
        )
    }

    fn fslc(&self, args: &[&str]) -> (i32, Value) {
        let (exit, _, packet) = self.run(args);
        (exit, packet)
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
        "message": format!("duplicate event field '{field}' at doc.fsl:{line}:{column}"),
        "loc": {"line": line, "column": column},
    })
}

/// The issue's four rows. On the old behavior rows 1 and 4 failed with a type
/// mismatch and rows 2 and 3 passed; all now fail at the repeated field.
#[test]
fn check_refuses_every_order_at_the_repeated_field() {
    for (row, event, input, assign, column) in [
        ("row1", "a: Q; a: Bool;", "Q", "n = a", 21),
        ("row2", "a: Bool; a: Q;", "Q", "n = a", 24),
        ("row3", "a: Q; a: Bool;", "Bool", "flag = a", 21),
        ("row4", "a: Bool; a: Q;", "Bool", "flag = a", 24),
    ] {
        assert_eq!(
            summary(&check(row, &domain(event, input, assign))),
            refusal("E.a", 7, column),
            "{row}"
        );
    }
}

/// Neither an equal type nor an unread field makes the repeat harmless, and the
/// `input` marker changes nothing.
#[test]
fn check_refuses_an_equal_type_an_unread_field_and_an_input_marker() {
    for (label, event, input, assign, column) in [
        ("same_type", "a: Q; a: Q;", "Q", "n = a", 21),
        ("unread", "a: Q; a: Bool;", "Q", "n = 0", 21),
        (
            "input_marker",
            "input a: Q; input a: Bool;",
            "Q",
            "n = a",
            33,
        ),
    ] {
        assert_eq!(
            summary(&check(label, &domain(event, input, assign))),
            refusal("E.a", 7, column),
            "{label}"
        );
    }
}

/// Every route that loads the document. The first copy comes last in row 2, so the
/// old behavior accepted this document on all of them.
#[test]
fn every_route_refuses_at_the_repeated_field() {
    let scratch = Scratch::new(
        "routes",
        &[
            ("doc.fsl", &domain("a: Bool; a: Q;", "Q", "n = a")),
            ("empty.jsonl", ""),
        ],
    );
    let bare = json!({
        "exit": 2,
        "result": "error",
        "kind": "name",
        "message": "duplicate event field 'E.a' at 7:24",
        "loc": {"line": 7, "column": 24},
    });
    // These two format the message and carry no `loc`.
    let unlocated = json!({
        "exit": 2,
        "result": "error",
        "kind": "semantics",
        "message": "duplicate event field 'E.a' at doc.fsl:7:24",
        "loc": null,
    });
    let cases: [(&[&str], Value); 15] = [
        (&["check", "doc.fsl"], refusal("E.a", 7, 24)),
        (&["verify", "doc.fsl"], refusal("E.a", 7, 24)),
        (&["testgen", "doc.fsl"], refusal("E.a", 7, 24)),
        (&["scenarios", "doc.fsl"], refusal("E.a", 7, 24)),
        (&["html", "doc.fsl"], refusal("E.a", 7, 24)),
        (&["ledger", "doc.fsl"], refusal("E.a", 7, 24)),
        (&["domain", "check", "doc.fsl"], refusal("E.a", 7, 24)),
        (&["domain", "testgen", "doc.fsl"], refusal("E.a", 7, 24)),
        (&["domain", "generate", "doc.fsl"], refusal("E.a", 7, 24)),
        (
            &["domain", "replay", "doc.fsl", "--logs", "empty.jsonl"],
            refusal("E.a", 7, 24),
        ),
        (&["domain", "expand", "doc.fsl"], bare.clone()),
        (&["domain", "analyze", "doc.fsl"], bare.clone()),
        (&["kernel", "doc.fsl"], bare),
        (&["fmt", "--check", "doc.fsl"], unlocated.clone()),
        (&["lint", "doc.fsl"], unlocated),
    ];
    for (args, expected) in cases {
        assert_eq!(summary(&scratch.fslc(args)), expected, "{args:?}");
    }
}

/// `domain expand` writes nothing: no expansion on stdout, no output file, and an
/// existing output file keeps its bytes.
#[test]
fn domain_expand_writes_nothing() {
    let scratch = Scratch::new(
        "expand",
        &[
            ("doc.fsl", &domain("a: Q; a: Bool;", "Q", "n = a")),
            ("kept.fsl", "kept\n"),
        ],
    );
    let (exit, stdout, packet) = scratch.run(&["domain", "expand", "doc.fsl"]);
    assert_eq!(exit, 2, "{stdout}");
    assert_eq!(packet["result"], "error", "{stdout}");
    assert!(!stdout.contains("spec "), "{stdout}");

    let (exit, packet) = scratch.fslc(&["domain", "expand", "doc.fsl", "-o", "new.fsl"]);
    assert_eq!(exit, 2, "{packet:#}");
    assert!(!scratch.0.join("new.fsl").exists(), "{packet:#}");

    let (exit, packet) = scratch.fslc(&["domain", "expand", "doc.fsl", "-o", "kept.fsl"]);
    assert_eq!(exit, 2, "{packet:#}");
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("kept.fsl")).expect("read kept file"),
        "kept\n"
    );
}

/// The issue left these unmeasured. A repeat of the field `correlation_id` reads
/// passed or failed by the order of the copies, and a saga's event was accepted.
#[test]
fn an_event_an_effect_or_a_saga_reads_is_refused_too() {
    let field = "    event PaymentCaptureRequested {\n      payment_request_id: PaymentRequestId\n";
    let before = ASYNC_EFFECT
        .split(field)
        .next()
        .expect("text before the event");
    let line = 1 + u32::try_from(before.lines().count()).expect("a small line number");
    assert!(ASYNC_EFFECT.contains(field));
    let correlated = format!("{field}      payment_request_id: Bool\n");
    assert_eq!(
        summary(&check(
            "correlation_first",
            &ASYNC_EFFECT.replace(field, &correlated)
        )),
        refusal("PaymentCaptureRequested.payment_request_id", line + 2, 7),
    );
    let reversed = "    event PaymentCaptureRequested {\n      payment_request_id: Bool\n      \
                    payment_request_id: PaymentRequestId\n";
    assert_eq!(
        summary(&check(
            "correlation_last",
            &ASYNC_EFFECT.replace(field, reversed)
        )),
        refusal("PaymentCaptureRequested.payment_request_id", line + 2, 7),
    );

    let one = "event PaymentCaptured { payment_request_id: PaymentRequestId }";
    let two = "event PaymentCaptured { payment_request_id: PaymentRequestId; \
               payment_request_id: Bool }";
    assert!(SAGA.contains(one));
    let (exit, packet) = check("saga", &SAGA.replace(one, two));
    assert_eq!(exit, 2, "{packet:#}");
    assert_eq!(packet["kind"], "name", "{packet:#}");
    assert!(
        packet["message"].as_str().is_some_and(|text| text
            .starts_with("duplicate event field 'PaymentCaptured.payment_request_id' at")),
        "{packet:#}"
    );

    for (label, source) in [("effect_clean", ASYNC_EFFECT), ("saga_clean", SAGA)] {
        assert_eq!(check(label, source).0, 0, "{label}");
    }
}

/// The refusal looks at every aggregate and only within one event.
#[test]
fn a_later_aggregate_is_read_and_a_shared_name_is_not_a_repeat() {
    let doc = |second: &str| {
        format!(
            "domain D {{\n  type Q = 0..2\n  aggregate A {{\n    state {{ n: Q = 0; }}\n    \
             command T {{ a: Q; b: Q; }}\n    event E {{ a: Q; b: Q; }}\n    \
             decide T {{ emits E }}\n    evolve E {{\n      n = a\n    }}\n  }}\n  \
             aggregate B {{\n    state {{ m: Q = 0; }}\n    command U {{ a: Q; }}\n    \
             event F {{ {second} }}\n    decide U {{ emits F }}\n    evolve F {{\n      \
             m = a\n    }}\n  }}\n}}\n"
        )
    };
    assert_eq!(
        summary(&check("later", &doc("a: Q; a: Q;"))),
        refusal("F.a", 15, 21)
    );
    assert_eq!(summary(&check("shared", &doc("a: Q;")))["exit"], 0);
}

/// `loc` stays on the `implements`, so the message names the other file's place.
#[test]
fn another_file_names_the_repeated_field() {
    let root = "requirements R {\n  implements D from \"abs.fsl\" {\n  }\n  number V\n  \
        process E with v: V {\n    stages A, B\n    initial A\n    transition go A -> B by U\n  \
        }\n}\nverify {\n  instances E = 1\n  values V = 0..1\n}\n";
    let scratch = Scratch::new(
        "implements",
        &[
            ("abs.fsl", &domain("a: Q; a: Bool;", "Q", "n = 0")),
            ("root.fsl", root),
        ],
    );
    assert_eq!(
        summary(&scratch.fslc(&["check", "root.fsl"])),
        json!({
            "exit": 2,
            "result": "error",
            "kind": "type",
            "message": "implements \"abs.fsl\" failed to check \
                        (duplicate event field 'E.a' at abs.fsl:7:21)",
            "loc": {"line": 2, "column": 3},
        })
    );
    // With one declaration the same root reaches its next, unrelated error.
    std::fs::write(scratch.0.join("abs.fsl"), domain("a: Q;", "Q", "n = 0")).expect("write abs");
    let (exit, packet) = scratch.fslc(&["check", "root.fsl"]);
    assert_eq!(exit, 2, "{packet:#}");
    assert!(
        !packet["message"]
            .as_str()
            .is_some_and(|message| message.contains("duplicate")),
        "{packet:#}"
    );
}

/// An event that repeats nothing is unchanged on every route, and a type mismatch
/// with no repeat keeps its own error.
#[test]
fn an_event_that_repeats_nothing_is_unchanged() {
    let scratch = Scratch::new("clean", &[("doc.fsl", &domain("a: Q;", "Q", "n = a"))]);
    for args in [
        ["check", "doc.fsl", ""],
        ["domain", "check", "doc.fsl"],
        ["domain", "expand", "doc.fsl"],
        ["domain", "analyze", "doc.fsl"],
        ["domain", "generate", "doc.fsl"],
    ] {
        let args: Vec<&str> = args.into_iter().filter(|arg| !arg.is_empty()).collect();
        let (exit, stdout, _) = scratch.run(&args);
        assert_eq!(exit, 0, "{args:?}: {stdout}");
    }
    assert_eq!(
        summary(&check("mismatch", &domain("a: Bool;", "Q", "n = a"))),
        json!({
            "exit": 2,
            "result": "error",
            "kind": "semantics",
            "message": "domain expression type mismatch at doc.fsl:10:11",
            "loc": {"line": 10, "column": 11},
        })
    );
}

/// A document with one slot for each kind the small tables repeat.
const SMALL: &str = "domain D {\n  implementation_profile functional_ddd\n  type Q = 0..2\n  \
    aggregate A {\n    id AId\n    state { n: Q = 0; flag: Bool = false; }\n    command T {}\n    \
    event E {}\n    decide T { emits E }\n    evolve E {\n      n = 1\n    }\n    \
    invariant small { n <= 1 }\n  }\n  projection P {\n    from A\n    fields [n]\n  }\n}\n";

const EVOLVE_ONE: &str = "    evolve E {\n      n = 1\n    }\n";
const EVOLVE_TWO: &str = "    evolve E {\n      n = 2\n    }\n";

/// `source` with `slot` replaced by `first` then `second`, and the 1-based position of
/// the first non-blank character of `second` there.
fn repeated(source: &str, slot: &str, first: &str, second: &str) -> (String, u32, u32) {
    let at = source.find(slot).expect("the slot is in the document");
    let text = format!(
        "{}{first}{second}{}",
        &source[..at],
        &source[at + slot.len()..]
    );
    let blank = second.len() - second.trim_start().len();
    let before = &text[..at + first.len() + blank];
    let line = before.matches('\n').count() + 1;
    let column = before.len() - before.rfind('\n').map_or(0, |index| index + 1) + 1;
    (
        text,
        u32::try_from(line).expect("a small line"),
        u32::try_from(column).expect("a small column"),
    )
}

/// The verdict flip: each copy alone is honest, and together the first decided, so
/// the violating pair verified. The refusal removed, the first row reads `verified`.
#[test]
fn a_repeated_evolve_no_longer_flips_the_verdict() {
    let single = |evolve: &str| SMALL.replace(EVOLVE_ONE, evolve);
    let scratch = Scratch::new(
        "evolve_flip",
        &[
            (
                "hides.fsl",
                &repeated(SMALL, EVOLVE_ONE, EVOLVE_ONE, EVOLVE_TWO).0,
            ),
            (
                "shows.fsl",
                &repeated(SMALL, EVOLVE_ONE, EVOLVE_TWO, EVOLVE_ONE).0,
            ),
            ("one.fsl", &single(EVOLVE_ONE)),
            ("two.fsl", &single(EVOLVE_TWO)),
        ],
    );
    for route in [
        &["verify", "--depth", "3"][..],
        &["verify", "--engine", "explicit", "--depth", "3"][..],
        &["domain", "check"][..],
    ] {
        for document in ["hides.fsl", "shows.fsl"] {
            let args: Vec<&str> = route.iter().copied().chain([document]).collect();
            assert_eq!(
                summary(&scratch.fslc(&args)),
                json!({
                    "exit": 2,
                    "result": "error",
                    "kind": "name",
                    "message": format!(
                        "duplicate evolve 'E' in aggregate 'A' at {document}:13:5"
                    ),
                    "loc": {"line": 13, "column": 5},
                }),
                "{args:?}"
            );
        }
        let args: Vec<&str> = route.iter().copied().chain(["one.fsl"]).collect();
        assert_eq!(scratch.fslc(&args).0, 0, "{args:?}");
        let args: Vec<&str> = route.iter().copied().chain(["two.fsl"]).collect();
        let (exit, packet) = scratch.fslc(&args);
        assert_eq!(
            (exit, &packet["result"]),
            (1, &json!("violated")),
            "{args:?}"
        );
    }
}

/// A second `command` of one name was lost to lowering, so its inputs were not.
#[test]
fn a_repeated_command_is_refused_at_the_repeat() {
    let slot = "    command T {}\n";
    let (source, line, column) = repeated(SMALL, slot, slot, "    command T { a: Q; }\n");
    let (exit, packet) = check("command", &source);
    assert_eq!(exit, 2, "{packet:#}");
    assert_eq!(packet["kind"], "name", "{packet:#}");
    assert_eq!(
        packet["message"],
        format!("duplicate command 'T' in aggregate 'A' at doc.fsl:{line}:{column}"),
    );
    assert_eq!(packet["loc"], json!({"line": line, "column": column}));
}

/// Both orders of a clause that holds one value are refused at the repeated keyword:
/// `slot` is in `base`, and `first` and `second` are the copies that replace it.
fn assert_clause_refused(
    label: &str,
    base: &str,
    slot: &str,
    (first, second): (&str, &str),
    (clause, owner): (&str, &str),
) {
    for (order, one, other) in [("in", first, second), ("out", second, first)] {
        let (source, line, column) = repeated(base, slot, one, other);
        let (exit, packet) = check(&format!("{label}_{order}"), &source);
        assert_eq!(exit, 2, "{label} {order}: {packet:#}");
        assert_eq!(packet["kind"], "parse", "{label} {order}: {packet:#}");
        assert_eq!(
            packet["message"],
            format!("duplicate domain clause '{clause}' in {owner} at {line}:{column}"),
            "{label} {order}"
        );
        assert_eq!(
            packet["loc"],
            json!({"line": line, "column": column}),
            "{label} {order}"
        );
    }
}

#[test]
fn a_domain_aggregate_or_projection_clause_is_refused_at_the_repeat() {
    for (label, slot, second, clause, owner) in [
        (
            "profile",
            "  implementation_profile functional_ddd\n",
            "  implementation_profile other\n",
            "implementation_profile",
            "domain 'D'",
        ),
        (
            "id",
            "    id AId\n",
            "    id OtherId\n",
            "id",
            "aggregate 'A'",
        ),
        (
            "from",
            "    from A\n",
            "    from Z\n",
            "from",
            "projection 'P'",
        ),
    ] {
        assert_clause_refused(label, SMALL, slot, (slot, second), (clause, owner));
    }
}

#[test]
fn an_effect_clause_the_example_has_is_refused_at_the_repeat() {
    let effect = "effect 'CapturePayment'";
    for (label, slot, second, clause, owner) in [
        (
            "correlation",
            "    correlation_id PaymentCaptureRequested.payment_request_id\n",
            "    correlation_id PaymentCaptured.payment_request_id\n",
            "correlation_id",
            effect,
        ),
        (
            "idempotency",
            "    idempotency_key Order.id\n",
            "    idempotency_key PaymentCaptureRequested.payment_request_id\n",
            "idempotency_key",
            effect,
        ),
        (
            "handles",
            "    handles PaymentCaptureRequested\n",
            "    handles OrderApproved\n",
            "handles",
            effect,
        ),
        (
            "irreversible",
            "    irreversible\n",
            "    irreversible false\n",
            "irreversible",
            effect,
        ),
        (
            "retry",
            "    retry {\n      max_attempts 3\n    }\n",
            "    retry {\n      max_attempts 9\n    }\n",
            "retry",
            effect,
        ),
        (
            "attempts",
            "      max_attempts 3\n",
            "      max_attempts 9\n",
            "max_attempts",
            "retry",
        ),
        (
            "timeout",
            "    timeout after 10m emits PaymentCaptureTimedOut\n",
            "    timeout after 1m emits PaymentCaptureTimedOut\n",
            "timeout",
            effect,
        ),
    ] {
        assert_clause_refused(label, ASYNC_EFFECT, slot, (slot, second), (clause, owner));
    }
}

/// The example has none of these, so both copies go in beside a clause it keeps.
#[test]
fn an_effect_clause_the_example_lacks_is_refused_at_the_repeat() {
    let slot = "    handles PaymentCaptureRequested\n";
    let effect = "effect 'CapturePayment'";
    for (label, first, second, clause) in [
        (
            "reliable",
            "    reliable\n",
            "    reliable false\n",
            "reliable",
        ),
        ("outbox", "    outbox O1\n", "    outbox O2\n", "outbox"),
        ("inbox", "    inbox I1\n", "    inbox I2\n", "inbox"),
        (
            "request",
            "    request_event PaymentCaptureRequested\n",
            "    request_event OrderApproved\n",
            "request_event",
        ),
        (
            "success",
            "    success_event PaymentCaptured\n",
            "    success_event PaymentFailed\n",
            "success_event",
        ),
        (
            "failure",
            "    failure_event PaymentFailed\n",
            "    failure_event PaymentCaptured\n",
            "failure_event",
        ),
        (
            "timed_out",
            "    timeout_event PaymentCaptureTimedOut\n",
            "    timeout_event PaymentFailed\n",
            "timeout_event",
        ),
    ] {
        let base = ASYNC_EFFECT.replace(slot, &format!("{slot}@@"));
        assert_clause_refused(label, &base, "@@", (first, second), (clause, effect));
    }
    let attempts = "      max_attempts 3\n";
    let base = ASYNC_EFFECT.replace(attempts, &format!("{attempts}@@"));
    assert_clause_refused(
        "backoff",
        &base,
        "@@",
        ("      backoff fixed\n", "      backoff linear\n"),
        ("backoff", "retry"),
    );
}

#[test]
fn a_saga_clause_is_refused_at_the_repeat() {
    let step = "saga step 'ReserveInventory'";
    for (label, slot, second, clause, owner) in [
        (
            "starts",
            "    starts_on OrderApproved\n",
            "    starts_on InventoryReserved\n",
            "starts_on",
            "saga 'OrderFulfillment'",
        ),
        (
            "awaits",
            "      awaits one_of [InventoryReserved, InventoryReservationFailed]\n",
            "      awaits one_of [InventoryReserved]\n",
            "awaits",
            step,
        ),
        (
            "step_timeout",
            "      timeout after 5m emits InventoryReservationFailed\n",
            "      timeout after 1m emits InventoryReserved\n",
            "timeout",
            step,
        ),
    ] {
        assert_clause_refused(label, SAGA, slot, (slot, second), (clause, owner));
    }
}

/// A field of an `effect` is read by nothing, one copy or two, so the second is lost.
#[test]
fn a_repeated_effect_field_is_refused_at_the_repeat() {
    let slot = "    handles PaymentCaptureRequested\n";
    let (source, line, _) = repeated(
        ASYNC_EFFECT,
        slot,
        &format!("{slot}    a: Bool\n"),
        "    a: PaymentStatus\n",
    );
    let (exit, packet) = check("effect_field", &source);
    assert_eq!(exit, 2, "{packet:#}");
    assert_eq!(packet["kind"], "parse", "{packet:#}");
    assert_eq!(
        packet["message"],
        format!("duplicate domain clause 'a' in effect 'CapturePayment' at {line}:5"),
    );
    assert_eq!(packet["loc"], json!({"line": line, "column": 5}));
    assert_eq!(
        check(
            "effect_one_field",
            &ASYNC_EFFECT.replace(slot, &format!("{slot}    a: Bool\n"))
        )
        .0,
        0
    );
}

/// The other routes read the same refusals. `domain expand` writes nothing.
#[test]
fn every_route_refuses_each_refused_kind() {
    let evolve = repeated(SMALL, EVOLVE_ONE, EVOLVE_ONE, EVOLVE_TWO).0;
    let command = repeated(
        SMALL,
        "    command T {}\n",
        "    command T {}\n",
        "    command T {}\n",
    )
    .0;
    let clause = repeated(SMALL, "    id AId\n", "    id AId\n", "    id OtherId\n").0;
    let scratch = Scratch::new(
        "kinds_routes",
        &[
            ("evolve.fsl", &evolve),
            ("command.fsl", &command),
            ("clause.fsl", &clause),
            ("empty.jsonl", ""),
            ("kept.fsl", "kept\n"),
        ],
    );
    for document in ["evolve.fsl", "command.fsl", "clause.fsl"] {
        let routes: [&[&str]; 9] = [
            &["check", document],
            &["verify", document],
            &["domain", "check", document],
            &["domain", "analyze", document],
            &["domain", "testgen", document],
            &["domain", "generate", document],
            &["domain", "replay", document, "--logs", "empty.jsonl"],
            &["domain", "expand", document],
            &["domain", "expand", document, "-o", "kept.fsl"],
        ];
        for args in routes {
            let (exit, stdout, packet) = scratch.run(args);
            assert_eq!(
                (exit, &packet["result"]),
                (2, &json!("error")),
                "{args:?}: {stdout}"
            );
            assert!(
                packet["message"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("duplicate ")),
                "{args:?}: {stdout}"
            );
            assert!(packet["loc"]["line"].is_u64(), "{args:?}: {stdout}");
            assert!(!stdout.contains("spec "), "{args:?}: {stdout}");
        }
        assert_eq!(
            std::fs::read_to_string(scratch.0.join("kept.fsl")).expect("read kept file"),
            "kept\n",
            "{document}"
        );
    }
}

/// `loc` stays on the `implements`, for a refusal from the lowering or the parse.
#[test]
fn another_file_names_each_refused_kind() {
    let root = "requirements R {\n  implements D from \"abs.fsl\" {\n  }\n  number V\n  \
        process E with v: V {\n    stages A, B\n    initial A\n    transition go A -> B by U\n  \
        }\n}\nverify {\n  instances E = 1\n  values V = 0..1\n}\n";
    let evolve = repeated(SMALL, EVOLVE_ONE, EVOLVE_ONE, EVOLVE_TWO).0;
    let scratch = Scratch::new(
        "implements_kinds",
        &[("abs.fsl", &evolve), ("root.fsl", root)],
    );
    assert_eq!(
        summary(&scratch.fslc(&["check", "root.fsl"])),
        json!({
            "exit": 2,
            "result": "error",
            "kind": "type",
            "message": "implements \"abs.fsl\" failed to check \
                        (duplicate evolve 'E' in aggregate 'A' at abs.fsl:13:5)",
            "loc": {"line": 2, "column": 3},
        })
    );
    let clause = repeated(SMALL, "    id AId\n", "    id AId\n", "    id OtherId\n").0;
    std::fs::write(scratch.0.join("abs.fsl"), clause).expect("write abs");
    assert_eq!(
        summary(&scratch.fslc(&["check", "root.fsl"])),
        json!({
            "exit": 2,
            "result": "error",
            "kind": "type",
            "message": "implements \"abs.fsl\" failed to check \
                        (duplicate domain clause 'id' in aggregate 'A' at abs.fsl:6:5)",
            "loc": {"line": 2, "column": 3},
        })
    );
}

/// The kinds that read every copy stay accepted. The note records why for each.
#[test]
fn a_kind_that_reads_every_copy_is_unchanged() {
    let saga_outbox = "    outbox OrderOutbox\n";
    let saga_inbox = "    inbox FulfillmentInbox\n";
    let step_async = "      async\n      emits InventoryReservationRequested\n";
    let effect_async = "    async\n    irreversible\n";
    let compensation = "    compensation {\n      emits PaymentFailed\n    }\n";
    let ship = "    step ShipOrder {";
    let other_saga = "  saga Other {\n    starts_on OrderApproved\n    step Other {\n      \
                      emits ShipmentRequested\n    }\n  }\n";
    let saga_head = "  saga OrderFulfillment {";
    let same_saga = other_saga.replace("saga Other", "saga OrderFulfillment");
    let cases: [(&str, String); 9] = [
        (
            "saga_outbox",
            SAGA.replace(saga_outbox, &format!("{saga_outbox}    outbox Other\n")),
        ),
        (
            "saga_inbox",
            SAGA.replace(saga_inbox, &format!("{saga_inbox}    inbox Other\n")),
        ),
        (
            "saga_by_name",
            SAGA.replace(saga_head, &format!("{same_saga}{saga_head}")),
        ),
        (
            "step_async",
            SAGA.replace(
                step_async,
                "      async\n      async\n      emits InventoryReservationRequested\n",
            ),
        ),
        (
            "effect_async",
            ASYNC_EFFECT.replace(effect_async, "    async\n    async\n    irreversible\n"),
        ),
        (
            "effect_compensation",
            ASYNC_EFFECT.replace(
                compensation,
                &format!(
                    "{compensation}    compensation {{\n      emits PaymentCaptured\n    }}\n"
                ),
            ),
        ),
        (
            "error",
            SMALL.replace(
                "    event E {}\n",
                "    event E {}\n    error X\n    error X\n",
            ),
        ),
        (
            "state_block",
            SMALL.replace(
                "    command T {}\n",
                "    state { m: Q = 0; }\n    command T {}\n",
            ),
        ),
        (
            "projection_by_name",
            SMALL.replace(
                "  projection P {",
                "  projection P {\n    from A\n    fields [flag]\n  }\n  projection P {",
            ),
        ),
    ];
    assert!(SAGA.contains(ship));
    for (label, source) in cases {
        let (exit, packet) = check(label, &source);
        assert_eq!(exit, 0, "{label}: {packet:#}");
    }
}
