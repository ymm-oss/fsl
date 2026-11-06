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

fn fslc(id: &str, files: &[(&str, &str)], arguments: &[&str]) -> Run {
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
        .env("FSLC_CACHE", "off");
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
    let run = fslc(id, files, &["check", "root.fsl"]);
    assert_eq!(
        (run.exit, &run.packet),
        (2, &refused(message, line, column)),
        "{id}"
    );
}

/// For a packet that carries more than a plain refusal: an edition, warnings or a span.
fn assert_located(run: &Run, kind: &str, message: &str, line: u32, column: u32) {
    assert_eq!(
        (
            run.exit,
            &run.packet["kind"],
            &run.packet["message"],
            &run.packet["loc"]
        ),
        (
            2,
            &json!(kind),
            &json!(message),
            &json!({"line":line,"column":column})
        ),
        "{}",
        run.packet
    );
}

fn assert_checks(id: &str, files: &[(&str, &str)]) {
    let run = fslc(id, files, &["check", "root.fsl"]);
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
    );
    // The event's origin is not the repeated name, so the message does not cite it.
    assert_located(&run, "semantics", "duplicate action parameter 'x'", 6, 57);
}

fn domain(inputs: &str, fields: &str, evolve: &str) -> String {
    format!(
        "domain D {{\n  implementation_profile functional_ddd\n\n  aggregate Order {{\n    id OrderId\n\n    state {{\n      flag: Bool = false;\n    }}\n\n    command Add {{\n      {inputs}\n    }}\n\n    event Added {{\n      {fields}\n    }}\n\n    decide Add {{\n      emits Added\n    }}\n\n    evolve Added {{\n      {evolve}\n    }}\n  }}\n}}\n"
    )
}

#[test]
fn domain_command_input_points_at_the_repeated_name() {
    let form = domain(
        "input amount: Bool\n      input amount: Bool",
        "",
        "flag = true",
    );
    assert_refused(
        "domain-command",
        &[("root.fsl", &form)],
        "duplicate action parameter 'amount'",
        13,
        13,
    );
    let distinct = domain(
        "input amount: Bool\n      input other: Bool",
        "",
        "flag = true",
    );
    assert_checks("domain-distinct", &[("root.fsl", &distinct)]);
}

/// A saga observe action and an effect completion take an event's fields as formals.
/// The event's own refusal comes first (#1700), at the same place.
#[test]
fn domain_event_field_behind_an_action_points_at_the_repeated_name() {
    let saga = |fields: &str| {
        format!(
            "domain D {{\n  implementation_profile functional_ddd\n  type OrderId = 0..1\n  aggregate Order {{\n    id OrderId\n    state {{ flag: Bool = false; }}\n    command Start {{}}\n    command Done {{}}\n    event Started {{}}\n    event Finished {{ {fields} }}\n    decide Start {{ emits Started }}\n    decide Done {{ emits Finished }}\n    evolve Started {{ flag = true }}\n    evolve Finished {{ flag = false }}\n  }}\n  saga Flow {{\n    starts_on Started\n    step Wait {{\n      async\n      requires Started\n      awaits one_of [Finished]\n    }}\n  }}\n}}\n"
        )
    };
    let source = saga("note: Bool  note: Bool");
    let run = fslc(
        "domain-saga",
        &[("root.fsl", &source)],
        &["check", "root.fsl"],
    );
    assert_located(
        &run,
        "name",
        "duplicate event field 'Finished.note' at root.fsl:10:34",
        10,
        34,
    );
    assert_checks("domain-saga-one", &[("root.fsl", &saga("note: Bool"))]);

    let effect = include_str!("fixtures/domain_characterization/effect_saga_valid.fsl");
    let event = "event PaymentCaptured { payment_request_id: PaymentRequestId }";
    assert!(effect.contains(event));
    let with_fields = |fields: &str| {
        effect.replace(
            event,
            &format!("event PaymentCaptured {{ payment_request_id: PaymentRequestId{fields} }}"),
        )
    };
    // This fixture also carries a deprecation warning.
    let refused_at = |id: &str, fields: &str, field: &str, column: u32| {
        let source = with_fields(fields);
        let run = fslc(id, &[("root.fsl", &source)], &["check", "root.fsl"]);
        let message =
            format!("duplicate event field 'PaymentCaptured.{field}' at root.fsl:18:{column}");
        assert_located(&run, "name", &message, 18, column);
    };
    refused_at("domain-effect", "  note: Bool  note: Bool", "note", 79);
    // The name an effect correlates on is its own formal when the event declares it.
    refused_at(
        "domain-effect-correlation",
        "  payment_request_id: PaymentRequestId",
        "payment_request_id",
        67,
    );
    assert_checks("domain-effect-one", &[("root.fsl", &with_fields(""))]);
}

/// `emits` takes no payload, so no action takes this field as a formal. #1680 accepted
/// it because no actual is lost; #1700 refuses it, because the copies still differ in
/// type and their order decides `check`. One field is unchanged.
#[test]
fn domain_event_field_no_action_takes_is_refused_too() {
    let repeated = domain(
        "input amount: Bool",
        "amount: Bool\n      amount: Bool",
        "flag = amount",
    );
    let once = domain("input amount: Bool", "amount: Bool\n", "flag = amount");
    let message = "duplicate event field 'Added.amount' at root.fsl:17:7";
    for arguments in [
        ["check", "root.fsl", "", "", "", ""],
        ["verify", "root.fsl", "--engine", "explicit", "--depth", "2"],
        ["verify", "root.fsl", "--engine", "bmc", "--depth", "2"],
    ] {
        let arguments: Vec<&str> = arguments.into_iter().filter(|a| !a.is_empty()).collect();
        let run = fslc("domain-event-twice", &[("root.fsl", &repeated)], &arguments);
        assert_located(&run, "name", message, 17, 7);
        let run = fslc("domain-event-once", &[("root.fsl", &once)], &arguments);
        assert_eq!(run.exit, 0, "{}", run.packet);
    }
}

const INLINE_IMPL: &str = "requirements Impl {\n  implements AbsI from \"abs.fsl\" {\n    map done = done\n    action step() -> step(false, false)\n  }\n  state { done: Bool }\n  init { done = false }\n  action step() { done = false }\n}\n";
/// The repeated `x` is at 1:77.
const INLINE_ABS: &str = "spec AbsI { state { done: Bool } init { done = false } action step(x: Bool, x: Bool) { done = false } }\n";

/// `loc` stays on the `implements`, so the message names the other file's position.
/// `kind` is `type` because every inline `implements` failure of the other file shares it.
#[test]
fn inline_implements_names_the_repeated_formal() {
    let run = fslc(
        "inline",
        &[("root.fsl", INLINE_IMPL), ("abs.fsl", INLINE_ABS)],
        &["check", "root.fsl"],
    );
    assert_located(
        &run,
        "type",
        "implements \"abs.fsl\" failed to check (step: duplicate action parameter 'x' at abs.fsl:1:77)",
        2,
        3,
    );
    let distinct = INLINE_ABS.replace("step(x: Bool, x: Bool)", "step(x: Bool, y: Bool)");
    assert_checks(
        "inline-distinct",
        &[("root.fsl", INLINE_IMPL), ("abs.fsl", &distinct)],
    );
}

/// Only the repeated formal's refusal is wrapped.
#[test]
fn inline_implements_other_failure_keeps_its_message() {
    let abs = "spec AbsI { state { done: Bool } init { done = false } action step() { done = false } action step() { done = false } }\n";
    let run = fslc(
        "inline-other",
        &[("root.fsl", INLINE_IMPL), ("abs.fsl", abs)],
        &["check", "root.fsl"],
    );
    assert_located(&run, "type", "duplicate action 'step'", 2, 3);
}

/// A log mapping's parameters name fields of a JSON object and bind nothing by
/// position, so `ev(a, a)` is `ev(a)`: one field, no value to lose. Accepted as on base.
#[test]
fn log_mapping_repeated_name_is_one_log_field() {
    const TARGET: &str = "spec Target { type K = 0..1 state { last: K } init { last = 0 } action put(v: K) { last = v } }\n";
    let mapping = |params: &str| {
        format!(
            "refinement LM {{\n  impl Log\n  abs Target\n  map last = last\n  action ev({params}) -> put(a)\n}}\n"
        )
    };
    for (id, log, exit) in [
        (
            "match",
            "{\"action\":\"ev\",\"params\":{\"a\":1},\"state\":{\"last\":1}}\n",
            0,
        ),
        (
            "mismatch",
            "{\"action\":\"ev\",\"params\":{\"a\":0},\"state\":{\"last\":1}}\n",
            1,
        ),
        (
            "extra",
            "{\"action\":\"ev\",\"params\":{\"a\":1,\"b\":0},\"state\":{\"last\":1}}\n",
            1,
        ),
    ] {
        let run = |params: &str, label: &str| {
            let map = mapping(params);
            fslc(
                &format!("log-{id}-{label}"),
                &[("root.fsl", TARGET), ("map.fsl", &map), ("log.jsonl", log)],
                &[
                    "replay",
                    "root.fsl",
                    "--from-log",
                    "log.jsonl",
                    "--mapping",
                    "map.fsl",
                ],
            )
        };
        let (twice, once) = (run("a, a", "twice"), run("a", "once"));
        assert_eq!(twice.exit, exit, "{id}: {}", twice.packet);
        assert_eq!(
            (twice.exit, &twice.packet),
            (once.exit, &once.packet),
            "{id}"
        );
    }
}

/// Base returned a result on these routes for the composed form; see the removal
/// table in the design note. Each control with distinct formals keeps its route's result.
#[test]
fn composed_refusal_reaches_the_routes_besides_check_and_verify() {
    let form = internal("part.put(0, 1)");
    let trace = "[{\"action\":\"step\",\"params\":{}}]";
    let files = [
        ("root.fsl", form.as_str()),
        ("piece.fsl", PIECE),
        ("trace.json", trace),
    ];
    let control_form = internal("part.put(0, 0)");
    let control = [
        ("root.fsl", control_form.as_str()),
        ("piece.fsl", DISTINCT),
        ("trace.json", trace),
    ];
    for (route, arguments, control_result) in [
        ("lint", &["lint", "root.fsl"][..], "lint"),
        ("fmt", &["fmt", "--check", "root.fsl"][..], "format_check"),
        (
            "conformance",
            &["conformance", "root.fsl"][..],
            "conformance",
        ),
        ("explain", &["explain", "root.fsl"][..], "explained"),
        ("analyze", &["analyze", "root.fsl"][..], "analyzed"),
        (
            "replay",
            &["replay", "root.fsl", "--trace", "trace.json"][..],
            "conformant",
        ),
    ] {
        let refused = fslc(&format!("route-{route}"), &files, arguments);
        assert_eq!(refused.exit, 2, "{route}: {}", refused.packet);
        assert_eq!(refused.packet["kind"], "semantics", "{route}");
        assert_eq!(refused.packet["message"], AT_PIECE, "{route}");
        let kept = fslc(&format!("route-{route}-control"), &control, arguments);
        assert_eq!(
            kept.packet["result"], control_result,
            "{route}: {}",
            kept.packet
        );
    }

    let abstraction =
        "spec Abs { state { done: Bool } init { done = false } action step() { done = false } }\n";
    let map = "refinement R {\n  impl Root\n  abs Abs\n  map done = false\n  action step() -> step()\n}\n";
    let arguments = ["refine", "root.fsl", "abs.fsl", "map.fsl", "--depth", "3"];
    let refine = fslc(
        "route-refine",
        &[
            ("root.fsl", form.as_str()),
            ("piece.fsl", PIECE),
            ("abs.fsl", abstraction),
            ("map.fsl", map),
        ],
        &arguments,
    );
    assert_eq!(refine.exit, 2, "{}", refine.packet);
    assert_eq!(refine.packet["message"], AT_PIECE);
    let kept = fslc(
        "route-refine-control",
        &[
            ("root.fsl", control_form.as_str()),
            ("piece.fsl", DISTINCT),
            ("abs.fsl", abstraction),
            ("map.fsl", map),
        ],
        &arguments,
    );
    assert_eq!(
        (kept.exit, &kept.packet["result"]),
        (0, &json!("refines")),
        "{}",
        kept.packet
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
