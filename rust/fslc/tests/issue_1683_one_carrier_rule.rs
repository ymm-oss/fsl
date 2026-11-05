// SPDX-License-Identifier: Apache-2.0

//! #1683: `check` and the symbolic engines answer from one carrier rule, so every
//! form below is either refused with a location or decides on bmc and induction. No
//! form reports `cannot coerce`. Each shape's refusal count is pinned, not derived from
//! the rule, because a test that asks the rule what to expect cannot detect a wrong rule.
//! The positions this does not cover are listed in
//! `docs/design/DESIGN-finite-container-carriers.md`, "Amendment (#1683): one carrier rule".

#[path = "support/issue_1517_scratch.rs"]
mod scratch;

use serde_json::Value;
use std::{collections::BTreeMap, fs, process::Command, sync::Mutex, thread};

const TYPES: &str =
    "type K = 0..1 type A = 0..1 type A2 = 0..1 type B = 0..2 enum E { e0, e1 } enum F { f0, f1 }";

/// A shape's left value is always state `l`. Its right value is state `r`, of the
/// declared carrier, or a literal over `va: A` and `vb: B`.
struct Shape {
    id: &'static str,
    left: &'static str,
    right_state: Option<&'static str>,
    right: &'static str,
    refused: usize,
}

const fn declared(
    id: &'static str,
    left: &'static str,
    right: &'static str,
    refused: usize,
) -> Shape {
    Shape {
        id,
        left,
        right_state: Some(right),
        right: "r",
        refused,
    }
}

const fn literal(
    id: &'static str,
    left: &'static str,
    right: &'static str,
    refused: usize,
) -> Shape {
    Shape {
        id,
        left,
        right_state: None,
        right,
        refused,
    }
}

const SHAPES: [Shape; 28] = [
    declared("relation_alias", "relation A -> B", "relation A2 -> B", 0),
    declared("relation_source", "relation A -> A", "relation B -> A", 25),
    declared("relation_target", "relation A -> A", "relation A -> B", 25),
    declared(
        "relation_alias_target",
        "relation A -> A",
        "relation A -> A2",
        0,
    ),
    declared("enum_same", "E", "E", 0),
    declared("enum_other", "E", "F", 25),
    declared("set_alias", "Set<A>", "Set<A2>", 0),
    declared("set_range", "Set<A>", "Set<B>", 25),
    declared("seq_alias", "Seq<A, 2>", "Seq<A2, 2>", 0),
    declared("seq_range", "Seq<A, 2>", "Seq<B, 2>", 25),
    declared("seq_capacity", "Seq<A, 2>", "Seq<A, 3>", 25),
    declared("seq_int_same", "Seq<Int, 2>", "Seq<Int, 2>", 0),
    declared("seq_int_capacity", "Seq<Int, 2>", "Seq<Int, 3>", 0),
    declared("seq_int_range", "Seq<Int, 2>", "Seq<A, 2>", 0),
    declared("seq_int_range_capacity", "Seq<Int, 2>", "Seq<A, 3>", 0),
    declared("option_alias", "Option<A>", "Option<A2>", 0),
    declared("option_range", "Option<A>", "Option<B>", 0),
    declared("option_int", "Option<Int>", "Option<A>", 0),
    declared("map_alias", "Map<K, A>", "Map<K, A2>", 0),
    declared("map_range", "Map<K, A>", "Map<K, B>", 25),
    declared("map_int", "Map<K, Int>", "Map<K, A>", 25),
    declared(
        "map_option_int",
        "Map<K, Option<Int>>",
        "Map<K, Option<A>>",
        25,
    ),
    literal("set_of_int", "Set<A>", "Set { 0 }", 0),
    literal("set_of_range", "Set<A>", "Set { vb }", 8),
    literal("seq_of_range", "Seq<A, 2>", "Seq { vb }", 8),
    literal("seq_over_capacity", "Seq<A, 2>", "Seq { vb, vb, vb }", 8),
    literal("option_of_range", "Option<A>", "some(vb)", 0),
    literal("option_of_int", "Option<A>", "some(0)", 0),
];

/// `{e}` positions go in a `trans`, which reads `old`. The three statement positions
/// run in the action.
const POSITIONS: [(&str, &str); 25] = [
    ("eq", "{l} == {r}"),
    ("ne", "{l} != {r}"),
    ("arm_then", "{l} == (if c then {r} else {l})"),
    ("arm_else", "{l} == (if c then {l} else {r})"),
    ("arm_on_the_left", "(if c then {r} else {l}) == {l}"),
    ("old", "{l} == old({r})"),
    ("old_ne", "{l} != old({r})"),
    ("old_some_ne", "some({l}) != old(some({r}))"),
    ("old_arm", "{l} == old(if c then {r} else {l})"),
    ("old_arm_ne", "{l} != old(if c then {r} else {l})"),
    ("arm_of_old", "{l} == (if c then old({r}) else {l})"),
    ("arm_of_old_ne", "{l} != (if c then old({r}) else {l})"),
    ("old_some", "some({l}) == old(some({r}))"),
    ("old_set", "Set { {l} } == old(Set { {r} })"),
    ("old_set_ne", "Set { {l} } != old(Set { {r} })"),
    ("some", "some({l}) == some({r})"),
    ("set", "Set { {l} } == Set { {r} }"),
    ("seq", "Seq { {l} } == Seq { {r} }"),
    ("contains", "Set { {l} }.contains({r})"),
    ("add", "Set { {l} }.add({r}) == Set { {l} }"),
    ("head", "Seq { {r} }.head() == {l}"),
    ("is", "some({r}) is some(x) and {l} == x"),
    ("requires", "requires {l} == {r}"),
    ("assign", "{l} = {r}"),
    ("let", "let t = {r}\n    requires {l} == t"),
];

fn initial(ty: &str, name: &str) -> String {
    if ty.starts_with("Set<") || ty.starts_with("relation ") {
        format!("{name} = Set {{}}")
    } else if ty.starts_with("Seq<") {
        format!("{name} = Seq {{}}")
    } else if ty.starts_with("Option<") {
        format!("{name} = none")
    } else if ty == "E" || ty == "F" {
        format!("{name} = {}0", ty.to_lowercase())
    } else if ty.starts_with("Map<K, Option<") {
        format!("forall k: K {{ {name}[k] = none }}")
    } else {
        format!("forall k: K {{ {name}[k] = 0 }}")
    }
}

fn form(shape: &Shape, (position, template): (&str, &str)) -> (String, String) {
    let expression = template.replace("{l}", "l").replace("{r}", shape.right);
    let in_action = matches!(position, "requires" | "assign" | "let");
    let (body, property) = if in_action {
        (expression.as_str(), "trans P { true }".to_owned())
    } else {
        ("", format!("trans P {{ {expression} }}"))
    };
    let mut state = format!("l: {}", shape.left);
    let mut init = initial(shape.left, "l");
    if let Some(right) = shape.right_state {
        state = format!("{state}, r: {right}");
        init = format!("{init} {}", initial(right, "r"));
    }
    (
        format!("{}__{position}", shape.id),
        format!(
            "spec S {{ {TYPES}\n  state {{ {state}, va: A, vb: B, c: Bool }}\n  init {{ {init} va = 0 vb = 0 c = false }}\n  action go() {{\n    {body}\n    c = not c\n  }}\n  {property}\n}}\n"
        ),
    )
}

fn fslc(directory: &std::path::Path, arguments: &[&str]) -> (i32, Value, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(directory)
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("CLI");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    (
        output.status.code().expect("exit"),
        serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("JSON packet: {stdout}")),
        stdout,
    )
}

fn violated(packet: &Value) -> bool {
    packet["result"] == "violated"
}

#[derive(Default)]
struct Tally {
    refused: BTreeMap<&'static str, usize>,
    accepted: usize,
    failures: Vec<String>,
}

/// Check one form, and when it is accepted run bmc, induction and explicit on it.
fn audit(shape: &'static Shape, id: &str, source: &str, tally: &Mutex<Tally>) {
    let scratch = scratch::Scratch::new("FSLC_1683_INPUTS", id, &format!("1683-{id}"));
    fs::write(scratch.path().join("p.fsl"), source).expect("fixture");
    let directory = scratch.path();
    let (exit, packet, text) = fslc(directory, &["check", "p.fsl"]);
    let mut failures = Vec::new();
    if exit == 0 {
        let (bmc_exit, bmc, bmc_text) = fslc(
            directory,
            &["verify", "p.fsl", "--engine", "bmc", "--depth", "2"],
        );
        let (induction_exit, _, induction_text) =
            fslc(directory, &["verify", "p.fsl", "--engine", "induction"]);
        let (_, explicit, _) = fslc(
            directory,
            &["verify", "p.fsl", "--engine", "explicit", "--depth", "2"],
        );
        for (engine, engine_exit, engine_text) in [
            ("bmc", bmc_exit, &bmc_text),
            ("induction", induction_exit, &induction_text),
        ] {
            if !matches!(engine_exit, 0 | 1) {
                failures.push(format!("{id}: {engine} exit {engine_exit}: {engine_text}"));
            }
        }
        if violated(&bmc) != violated(&explicit) {
            failures.push(format!("{id}: bmc {bmc} against explicit {explicit}"));
        }
    } else if exit != 2
        || packet["kind"] != "semantics"
        || packet["loc"]["line"].as_u64().is_none_or(|line| line == 0)
        || text.contains("cannot coerce")
    {
        failures.push(format!("{id}: check exit {exit}: {packet}"));
    }
    let mut tally = tally.lock().expect("tally");
    tally.failures.append(&mut failures);
    if exit == 0 {
        tally.accepted += 1;
    } else {
        *tally.refused.entry(shape.id).or_default() += 1;
    }
}

/// A Map a literal holds under `old` answers to what surrounds it. `infer_type` sees only
/// the literal take the expectation, so evaluation was the first to compare the Map.
#[test]
fn map_in_a_literal_under_old_is_refused_where_it_is_written() {
    const SPEC: &str = "spec S { type K = 0..1 type A = 0..1 type A2 = 0..1 type E = 1..0\n  state { m0: Map<K, Int>, m1: Map<K, A>, m2: Map<K, A2>, c: Bool }\n  init { forall k: K { m0[k] = 0 m1[k] = 0 m2[k] = 0 } c = false }\n  action go() {\n    BODY\n    c = not c\n  }\n  trans P { PROPERTY }\n}\n";
    let spec =
        |body: &str, property: &str| SPEC.replace("BODY", body).replace("PROPERTY", property);
    let scratch = scratch::Scratch::new("FSLC_1683_INPUTS", "literal_under_old", "1683-old");
    let directory = scratch.path();
    let write = |id: &str, source: &str| {
        let form = directory.join(id);
        fs::create_dir_all(&form).expect("form");
        fs::write(form.join("p.fsl"), source).expect("fixture");
        form
    };
    for (id, source, line) in [
        ("some", spec("", "some(m0) == old(some(m1))"), 8),
        ("some_ne", spec("", "some(m0) != old(some(m1))"), 8),
        ("set", spec("", "Set { m0 } == old(Set { m1 })"), 8),
        ("seq", spec("", "Seq { m0 } == old(Seq { m1 })"), 8),
        (
            "set_of_some",
            spec("", "Set { some(m0) } == old(Set { some(m1) })"),
            8,
        ),
        (
            "ensures",
            spec("ensures some(m0) == old(some(m1))", "true"),
            4,
        ),
        // Any depth of literal, and an arm of one or around the `old`.
        (
            "some_of_set",
            spec("", "some(Set { m1 }) == old(some(Set { m0 }))"),
            8,
        ),
        (
            "seq_of_set",
            spec("", "Seq { Set { m1 } } == old(Seq { Set { m0 } })"),
            8,
        ),
        (
            "some_of_some",
            spec("", "some(some(m1)) == old(some(some(m0)))"),
            8,
        ),
        (
            "set_of_arm",
            spec("", "Set { m1 } == old(Set { if c then m0 else m0 })"),
            8,
        ),
        (
            "arm_of_old",
            spec("", "some(m1) == (if c then old(some(m0)) else some(m1))"),
            8,
        ),
        // Never evaluated, so these returned a verdict before: a mismatch stays
        // refused where it is written (#1679).
        (
            "empty_binder",
            spec("", "forall i: E { some(m0) == old(some(m1)) }"),
            8,
        ),
    ] {
        let form = write(id, &source);
        let (exit, packet, _) = fslc(&form, &["check", "p.fsl"]);
        assert_eq!(exit, 2, "{id}: {packet}");
        assert_eq!(packet["kind"], "semantics", "{id}: {packet}");
        assert_eq!(packet["loc"]["line"], line, "{id}: {packet}");
        assert!(
            packet["message"].as_str().is_some_and(|text| {
                text.starts_with("invalid model expression: expression of type Map(")
                    && text.contains(" is not assignable to Map(")
            }),
            "{id}: {packet}"
        );
    }
    // The same forms with one carrier keep deciding: an equal-range alias, and a Map
    // against its own carrier.
    for (id, property, result) in [
        ("alias", "some(m1) == old(some(m2)) or true", "proved"),
        (
            "alias_set",
            "Set { m1 } == old(Set { m2 }) or true",
            "proved",
        ),
        (
            "alias_seq",
            "Seq { m1 } == old(Seq { m2 }) or true",
            "proved",
        ),
        ("own_carrier", "some(m0) == old(some(m0))", "proved"),
    ] {
        let form = write(id, &spec("", property));
        let (exit, packet, _) = fslc(&form, &["verify", "p.fsl", "--engine", "induction"]);
        assert_eq!(
            (exit, packet["result"].as_str()),
            (0, Some(result)),
            "{id}: {packet}"
        );
    }
}

/// A position evaluated concretely asks only for assignability, so the rule's `MapMismatch`
/// passes there and its `NotAssignable` does not. `check` reaches both through the rule.
#[test]
fn a_position_on_the_prior_rule_answers_assignability_from_the_rule() {
    let spec = |left: &str, right: &str| {
        format!(
            "requirements R {{\n  type K = 0..1\n  type A = 0..1\n  type B = 0..2\n  state {{ l: {left}, r: {right}, c: Bool }}\n  init {{ {} {} c = false }}\n  requirement REQ-1 \"go\" {{\n    action go() {{ c = not c }}\n  }}\n  acceptance AC-1 \"same\" {{\n    go()\n    expect l == r\n  }}\n}}\n",
            initial(left, "l"),
            initial(right, "r"),
        )
    };
    let scratch = scratch::Scratch::new("FSLC_1683_INPUTS", "prior_rule", "1683-prior");
    for (id, left, right, accepted) in [
        ("map_int", "Map<K, Int>", "Map<K, A>", true),
        (
            "map_option_int",
            "Map<K, Option<Int>>",
            "Map<K, Option<A>>",
            true,
        ),
        ("map_range", "Map<K, A>", "Map<K, B>", false),
        ("map_key", "Map<K, A>", "Map<B, A>", false),
        ("seq_int_capacity", "Seq<Int, 2>", "Seq<A, 3>", true),
        ("seq_capacity", "Seq<A, 2>", "Seq<A, 3>", false),
        ("set_range", "Set<A>", "Set<B>", false),
    ] {
        let form = scratch.path().join(id);
        fs::create_dir_all(&form).expect("form");
        fs::write(form.join("p.fsl"), spec(left, right)).expect("fixture");
        let (exit, packet, _) = fslc(&form, &["check", "p.fsl"]);
        assert_eq!(exit == 0, accepted, "{id}: {packet}");
    }
}

/// A candidate reaches no old state, so it never ran the operand's walk, and its mismatch
/// under `old` stays where evaluation finds the missing state.
#[test]
fn a_lemma_candidate_under_old_keeps_the_prior_rule() {
    let scratch = scratch::Scratch::new("FSLC_1683_INPUTS", "lemma_under_old", "1683-lemma");
    fs::write(
        scratch.path().join("p.fsl"),
        "spec S { type K = 0..1 type A = 0..1\n  state { m0: Map<K, Int>, m1: Map<K, A>, c: Bool }\n  init { forall k: K { m0[k] = 0 m1[k] = 0 } c = false }\n  action go() { c = not c }\n  invariant P { c or not c }\n}\n",
    )
    .expect("fixture");
    for lemma in [
        "some(m0) == old(some(m1))",
        "Set { 1 } == old(Set { true })",
    ] {
        let (exit, packet, _) = fslc(
            scratch.path(),
            &["verify", "p.fsl", "--engine", "induction", "--lemma", lemma],
        );
        assert_eq!(
            (exit, packet["result"].as_str()),
            (0, Some("proved")),
            "{lemma}: {packet}"
        );
        assert_eq!(
            packet["lemmas"][0]["status"], "rejected",
            "{lemma}: {packet}"
        );
        assert_eq!(
            packet["lemmas"][0]["proof"]["message"], "old() used without old state",
            "{lemma}: {packet}"
        );
    }
}

/// The removal is wider than a position never evaluated: a form that `--exclude-property`
/// leaves out is refused too, and the refusal takes another property's counterexample
/// with it. `some(m0) != old(some(m1))` reports `cannot coerce` on bmc and induction when
/// evaluated, while explicit and `fsl-bfs` answer it. Before #1683 this spec gave `violated`
/// on `I` under `--exclude-property P`. The control keeps that counterexample in sight.
#[test]
fn a_refused_form_takes_another_propertys_counterexample_with_it() {
    const SPEC: &str = "spec S {\n  type K = 0..1\n  type A = 0..1\n  state { m0: Map<K, Int>, m1: Map<K, A>, vb: Int }\n  init { vb = 0 forall k: K { m0[k] = 0 } forall k: K { m1[k] = 0 } }\n  action go() { requires vb < 3 vb = vb + 1 }\n  trans P { PROPERTY }\n  invariant I { vb < 2 }\n}\n";
    let scratch = scratch::Scratch::new("FSLC_1683_INPUTS", "hidden_counterexample", "1683-hidden");
    let write = |id: &str, property: &str| {
        let form = scratch.path().join(id);
        fs::create_dir_all(&form).expect("form");
        fs::write(form.join("p.fsl"), SPEC.replace("PROPERTY", property)).expect("fixture");
        form
    };
    let refused = write("refused", "some(m0) != old(some(m1))");
    let control = write("control", "vb >= 0");
    for engine in ["bmc", "explicit"] {
        let arguments = [
            "verify",
            "p.fsl",
            "--engine",
            engine,
            "--depth",
            "4",
            "--exclude-property",
            "P",
        ];
        let (exit, packet, _) = fslc(&refused, &arguments);
        assert_eq!(exit, 2, "{engine}: {packet}");
        assert_eq!(packet["kind"], "semantics", "{engine}: {packet}");
        let (exit, packet, _) = fslc(&control, &arguments);
        assert_eq!(exit, 1, "{engine}: {packet}");
        assert_eq!(packet["result"], "violated", "{engine}: {packet}");
        assert_eq!(packet["invariant"], "I", "{engine}: {packet}");
    }
    let (exit, packet, _) = fslc(&refused, &["verify", "p.fsl", "--engine", "explicit"]);
    assert_eq!(exit, 2, "explicit: {packet}");
}

/// A negative control for the literal under `old`: `vb` leaves `A`, so a rebuild that
/// dropped an out-of-range element would hold where every engine finds a violation.
#[test]
fn a_literal_under_old_keeps_a_value_outside_the_expected_range() {
    const SPEC: &str = "spec S {\n  type A = 0..1\n  type B = 0..2\n  state { a: Set<A>, vb: B, c: Bool }\n  init { a = Set {} vb = 0 c = false }\n  action go() { requires vb < 2 vb = vb + 1 }\n  action flip() { c = not c }\n  trans P { old(vb) >= 2 => a == old(Set { vb }) }\n}\n";
    let scratch = scratch::Scratch::new("FSLC_1683_INPUTS", "out_of_range", "1683-range");
    fs::write(scratch.path().join("p.fsl"), SPEC).expect("fixture");
    assert_eq!(fslc(scratch.path(), &["check", "p.fsl"]).0, 0);
    for engine in [
        ["--engine", "bmc", "--depth", "4"].as_slice(),
        ["--engine", "explicit", "--depth", "4"].as_slice(),
        ["--engine", "induction"].as_slice(),
    ] {
        let (exit, packet, _) = fslc(scratch.path(), &[&["verify", "p.fsl"], engine].concat());
        assert_eq!((exit, violated(&packet)), (1, true), "{engine:?}: {packet}");
        assert_eq!(packet["invariant"], "P", "{engine:?}: {packet}");
    }
    let model = fsl_core::build_model(
        fsl_core::parse_kernel_source(SPEC, &fsl_core::FsResolver::new(".")).expect("lowers"),
    )
    .expect("model");
    let violation = fsl_runtime::bfs(model, 4)
        .expect("bfs")
        .violation
        .expect("fsl-bfs finds it");
    assert_eq!(violation.step, 3, "{violation:?}");
}

/// What replaced `cannot coerce` is not always a verdict. Here `sb.push` has no bound of
/// its own, so induction's counterexample leaves `sb` longer than its capacity. The same
/// form outside `old` already answered this way before #1683, on the same spec.
#[test]
fn a_counterexample_past_capacity_reports_the_capacity_error_in_and_out_of_old() {
    const SPEC: &str = "spec S {\n  type A = 0..1\n  type B = 0..2\n  state { sa: Seq<A, 2>, sb: Seq<B, 2>, va: A }\n  init { sa = Seq {} sb = Seq {} va = 0 }\n  action go() { requires sa.size() == 0 sa = sa.push(1) sb = sb.push(2) }\n  trans T { PROPERTY }\n}\n";
    let scratch = scratch::Scratch::new("FSLC_1683_INPUTS", "past_capacity", "1683-capacity");
    for (id, property) in [
        ("under_old", "sb != old(Seq { va, va, va })"),
        ("outside_old", "sb != Seq { va, va, va }"),
    ] {
        let form = scratch.path().join(id);
        fs::create_dir_all(&form).expect("form");
        fs::write(form.join("p.fsl"), SPEC.replace("PROPERTY", property)).expect("fixture");
        for (engine, result) in [("bmc", "verified"), ("explicit", "proved")] {
            let (exit, packet, _) = fslc(
                &form,
                &["verify", "p.fsl", "--engine", engine, "--depth", "2"],
            );
            assert_eq!(
                (exit, packet["result"].as_str()),
                (0, Some(result)),
                "{id} {engine}: {packet}"
            );
        }
        let (exit, packet, _) = fslc(&form, &["verify", "p.fsl", "--engine", "induction"]);
        assert_eq!(exit, 2, "{id}: {packet}");
        assert_eq!(
            packet["message"], "model sequence length exceeds capacity",
            "{id}: {packet}"
        );
    }
}

#[test]
fn every_form_is_refused_with_a_location_or_decides() {
    let mut forms = Vec::new();
    for shape in &SHAPES {
        for position in POSITIONS {
            let (id, source) = form(shape, position);
            forms.push((shape, id, source));
        }
    }
    let total = forms.len();
    assert_eq!(total, SHAPES.len() * POSITIONS.len());
    let queue = Mutex::new(forms);
    let tally = Mutex::new(Tally::default());
    thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().expect("queue").pop();
                    let Some((shape, id, source)) = next else {
                        break;
                    };
                    audit(shape, &id, &source, &tally);
                }
            });
        }
    });
    let tally = tally.into_inner().expect("tally");
    assert!(tally.failures.is_empty(), "{}", tally.failures.join("\n"));
    let refused: BTreeMap<_, _> = SHAPES
        .iter()
        .map(|shape| (shape.id, shape.refused))
        .filter(|(_, count)| *count > 0)
        .collect();
    assert_eq!(tally.refused, refused);
    assert_eq!(
        tally.accepted + tally.refused.values().sum::<usize>(),
        total
    );
}
