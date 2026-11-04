// SPDX-License-Identifier: Apache-2.0
//! Checked admission of integer range binders, against hand-written expectations.
//!
//! Every refusal row is a detector: with `check_range_admission` disabled the
//! form checks `ok`, and the engines report an unlocated refusal or a verdict.

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

const HEAD: &str = "const C = 2
  type T = 0..3
  struct S { f: T }
  state { n: T, y: T, z: Bool, b: Bool, op: Option<T>, q: Seq<T, 4>, st: Set<T>, m: Map<T, T>, iv: Int, s: S }
  init { n = 2 y = 0 z = true b = false op = some(2) q = Seq { 0, 1 } st = Set { 1, 2 } forall i0: T { m[i0] = 2 } iv = 1 s = S { f: 2 } }
  action tick() { requires n < 3 n = n + 1 }
  ";

fn scratch(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fslc-1480-admission-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("scratch directory");
    for (name, text) in files {
        fs::write(dir.join(name), text).expect("scratch file");
    }
    dir
}

fn run(dir: &Path, args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(dir)
        .env("FSLC_CACHE", "off")
        .args(args)
        .output()
        .expect("run fslc");
    let mut value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: stdout is not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    // Envelope identity only: the binary's version stamp is not part of the form.
    value.as_object_mut().expect("object").remove("versions");
    (output.status.code().expect("exit code"), value)
}

fn engine(dir: &Path, engine: &str) -> (i32, Value) {
    run(
        dir,
        &[
            "verify",
            "m.fsl",
            "--engine",
            engine,
            "--depth",
            "3",
            "--no-cache",
        ],
    )
}

fn spec(body: &str) -> String {
    format!("spec M {{\n  {HEAD}{body}\n}}\n")
}

/// `body` starts at line 8 column 3 of [`spec`].
fn assert_refused(body: &str, message: &str, column: usize) {
    let dir = scratch(&[("m.fsl", &spec(body))]);
    let expected = json!({
        "fsl": "1.0",
        "result": "error",
        "kind": "semantics",
        "message": message,
        "loc": {"line": 8, "column": column},
    });
    assert_eq!(
        run(&dir, &["check", "m.fsl"]),
        (2, expected.clone()),
        "check {body}"
    );
    for route in ["bmc", "induction", "explicit"] {
        assert_eq!(engine(&dir, route), (2, expected.clone()), "{route} {body}");
    }
}

const NOT_CONST_N: &str = "'n' is not an integer const";
const STATIC_INTEGER: &str = "binder bound is not a static integer";
const AGGREGATE_AT_3: &str =
    "invalid model expression: public Kernel requires static aggregate range bounds at 8:3";

#[test]
fn class_a_unplanned_expressions_are_refused() {
    assert_refused(
        "invariant P { count(i in 0..m[0]) <= 3 }",
        AGGREGATE_AT_3,
        3,
    );
    assert_refused(
        "invariant P { forall i in 0..m[0] { i >= 0 } }",
        STATIC_INTEGER,
        3,
    );
    assert_refused(
        "invariant P { forall i in 0..s.f { i >= 0 } }",
        STATIC_INTEGER,
        3,
    );
    assert_refused(
        "invariant P { forall i in 0..q.size() { i >= 0 } }",
        STATIC_INTEGER,
        3,
    );
    assert_refused(
        "invariant P { forall i in 0..(if b then 1 else n) { i >= 0 } }",
        STATIC_INTEGER,
        3,
    );
    assert_refused(
        "invariant P { forall i in 0..min(n, 3) { i >= 0 } }",
        STATIC_INTEGER,
        3,
    );
    assert_refused(
        "invariant P { exists i in 0..(2 / y) { i >= 0 } or true }",
        NOT_CONST_Y,
        3,
    );
    assert_refused(
        "invariant P { forall o: Bool { forall i in 0..(if o then 1 else 2) { i >= 0 } } }",
        STATIC_INTEGER,
        3,
    );
}

const NOT_CONST_Y: &str = "'y' is not an integer const";

#[test]
fn class_b_scopes_without_a_certified_read_frame_are_refused() {
    let aggregate = "invalid model expression: 'n' is not an integer const at 8:3";
    for body in [
        "action go() { y = 1 ensures count(i in 0..n) <= 3 }",
        "action go() { y = 1 ensures old(count(i in 0..n)) >= 0 }",
        "terminal { count(i in 0..n) >= 0 }",
        "reachable P { count(i in 0..n) >= 0 }",
        "leadsTo P { count(i in 0..n) >= 0 ~> within 2 z }",
        "leadsTo P { z ~> within 2 count(i in 0..n) >= 0 }",
        "until P { count(i in 0..n) >= 0 until n == 3 }",
        "leadsTo P { n < 3 ~> n == 3 decreases count(i in 0..n) }",
    ] {
        assert_refused(body, aggregate, 3);
    }
    assert_refused(
        "reachable P { forall i in 0..(n + 1) { i >= 0 } }",
        NOT_CONST_N,
        3,
    );
    assert_refused(
        "action go() { y = 1 ensures forall i in 0..n { i >= 0 } }",
        NOT_CONST_N,
        3,
    );
}

#[test]
fn class_c_let_names_are_refused_even_when_literal() {
    assert_refused(
        "action go() { let k = 2 z = count(i in 0..k) == 3 }",
        "invalid model expression: 'k' is not an integer const at 8:27",
        27,
    );
    assert_refused(
        "action go() { let k = 2 requires forall i in 0..k { i >= 0 } y = 1 }",
        "'k' is not an integer const",
        3,
    );
    assert_refused(
        "action go() { let k = C let j = k + 1 z = forall i in 0..j { i >= 0 } }",
        "'j' is not an integer const",
        41,
    );
}

#[test]
fn class_d_names_without_a_finite_fact_are_refused() {
    assert_refused(
        "action go() { requires op is some(v) z = count(i in 0..v) == 3 }",
        "invalid model expression: 'v' is not an integer const at 8:40",
        40,
    );
    assert_refused(
        "invariant P { forall i in 0..iv { i >= 0 } }",
        "'iv' is not an integer const",
        3,
    );
    let bound_x = "'x' is not an integer const";
    for collection in [
        "q",
        "Set { n, 2 }",
        "Seq { n, 1 }",
        "Seq { 1 + 1 }",
        "Seq { -1 }",
    ] {
        assert_refused(
            &format!(
                "invariant P {{ forall x in {collection} {{ forall i in 0..x {{ i >= 0 }} }} }}"
            ),
            bound_x,
            3,
        );
    }
}

#[test]
fn class_e_old_bounds_are_refused() {
    assert_refused(
        "invariant P { count(i in 0..old(n)) >= 0 }",
        AGGREGATE_AT_3,
        3,
    );
    assert_refused("trans P { count(i in 0..old(n)) >= 0 }", AGGREGATE_AT_3, 3);
    assert_refused(
        "invariant P { forall i in 0..old(n) { i >= 0 } }",
        STATIC_INTEGER,
        3,
    );
}

#[test]
fn class_f_aggregate_bounds_are_refused() {
    assert_refused(
        "invariant P { forall i in 0..count(j in 0..1) { i <= 3 } }",
        STATIC_INTEGER,
        3,
    );
}

#[test]
fn unrepresentable_envelopes_are_refused() {
    assert_refused(
        "invariant P { count(i in 0..(n * 9223372036854775807)) >= 0 }",
        AGGREGATE_AT_3,
        3,
    );
    assert_refused(
        "invariant P { forall i in 0..(n * 9223372036854775807) { i >= 0 } }",
        NOT_CONST_N,
        3,
    );
}

#[test]
fn leadsto_wrapper_bounds_use_the_expansion_refusal() {
    assert_refused(
        "leadsTo P { forall i in 0..n { z ~> within 2 i <= 2 } }",
        "'n' is not an integer const",
        3,
    );
}

#[test]
fn breaking_removals_lose_a_verdict_reached_before_the_refused_bound() {
    // Earlier violation: both engines reported `violated` before this rule.
    assert_refused(
        "invariant Q { false } reachable R { forall i in 0..n { i >= 0 } }",
        NOT_CONST_N,
        25,
    );
    assert_refused(
        "invariant Q { n < 3 } action go() { y = 1 ensures forall i in 0..n { i >= 0 } }",
        NOT_CONST_N,
        25,
    );
    // Selection and depth: these flags used to step around the unsupported property.
    let dir = scratch(&[(
        "m.fsl",
        &spec(
            "invariant A { n >= 0 } reachable R { forall i in 0..n { i >= 0 } } action go() { y = 1 ensures forall i in 0..n { i >= 0 } }",
        ),
    )]);
    let refused = json!({
        "fsl": "1.0", "result": "error", "kind": "semantics",
        "message": NOT_CONST_N, "loc": {"line": 8, "column": 70},
    });
    for args in [
        &[
            "verify",
            "m.fsl",
            "--engine",
            "bmc",
            "--depth",
            "3",
            "--property",
            "A",
            "--no-cache",
        ][..],
        &[
            "verify",
            "m.fsl",
            "--engine",
            "bmc",
            "--depth",
            "3",
            "--exclude-property",
            "R",
            "--no-cache",
        ],
        &[
            "verify",
            "m.fsl",
            "--engine",
            "induction",
            "--exclude-property",
            "R",
            "--no-cache",
        ],
        &[
            "verify",
            "m.fsl",
            "--engine",
            "bmc",
            "--depth",
            "0",
            "--no-cache",
        ],
    ] {
        assert_eq!(run(&dir, args), (2, refused.clone()), "{args:?}");
    }
}

/// Literal expected verdicts, independent of the admission implementation.
fn assert_verdicts(body: &str, bmc: (i32, &str), induction: (i32, &str)) {
    let dir = scratch(&[("m.fsl", &spec(body))]);
    let (exit, checked) = run(&dir, &["check", "m.fsl"]);
    assert_eq!(
        (exit, &checked["result"]),
        (0, &json!("ok")),
        "check {body}: {checked}"
    );
    let (exit, produced) = engine(&dir, "bmc");
    assert_eq!(
        (exit, produced["result"].as_str().unwrap()),
        bmc,
        "bmc {body}: {produced}"
    );
    let (exit, produced) = engine(&dir, "induction");
    assert_eq!(
        (exit, produced["result"].as_str().unwrap()),
        induction,
        "induction {body}: {produced}"
    );
}

const VERIFIED: (i32, &str) = (0, "verified");
const PROVED: (i32, &str) = (0, "proved");
const VIOLATED: (i32, &str) = (1, "violated");

#[test]
fn executable_forms_keep_their_verdicts() {
    for (body, bmc, induction) in [
        // Certified read frames.
        ("invariant P { count(i in 0..n) <= 4 }", VERIFIED, PROVED),
        ("invariant P { count(i in 0..n) == 99 }", VIOLATED, VIOLATED),
        ("trans P { old(count(i in 0..n)) >= 0 }", VERIFIED, PROVED),
        (
            "action go() { requires count(i in 0..n) >= 0 y = 1 } invariant Y { y <= 1 }",
            VERIFIED,
            PROVED,
        ),
        (
            "invariant P { count(i in (if C > 1 then 0 else 1)..n) >= 0 }",
            VERIFIED,
            PROVED,
        ),
        // Enumerated values need no certified frame.
        (
            "action go(p: T) { y = 1 ensures count(i in 0..p) <= 4 }",
            VERIFIED,
            PROVED,
        ),
        (
            "action go() { y = 1 ensures forall o: T { count(i in 0..(o + 1)) <= 5 } }",
            VERIFIED,
            PROVED,
        ),
        (
            "reachable P { forall o in 0..2 { count(i in 0..o) >= 1 } }",
            VERIFIED,
            PROVED,
        ),
        (
            "leadsTo P { forall o in 0..2 { forall j in 0..o { z ~> within 2 j <= 2 } } }",
            VERIFIED,
            PROVED,
        ),
        // Literal Seq items stay solver literals, including through a let.
        (
            "reachable P { forall x in Seq { 1, C } { count(i in 0..x) >= 1 } }",
            VERIFIED,
            PROVED,
        ),
        (
            "action go() { let t = Seq { 1, 2 } requires forall x in t { count(i in 0..x) >= 1 } y = 1 }",
            VERIFIED,
            PROVED,
        ),
        (
            "action go() { let t = 2 requires forall x in Seq { t, 1 } { count(i in 0..x) >= 1 } y = 1 }",
            VERIFIED,
            PROVED,
        ),
        // Nothing reaches the body of an empty domain.
        (
            "invariant P { forall o in 2..0 { count(i in 0..m[o]) >= 1 } }",
            VERIFIED,
            PROVED,
        ),
        (
            "reachable P { forall o in 1..0 { count(i in 0..old(n)) >= 1 } }",
            VERIFIED,
            PROVED,
        ),
        // Statement binders in a certified frame.
        (
            "action go() { forall i in 0..n { m[i] = 1 } }",
            VERIFIED,
            PROVED,
        ),
    ] {
        assert_verdicts(body, bmc, induction);
    }
}

#[test]
fn init_reads_of_completed_assignments_keep_running() {
    let dir = scratch(&[(
        "m.fsl",
        "spec M {\n  type T = 0..3\n  state { n: T, y: Int, iv: Int, z: Bool }\n  init { n = 2 iv = 1 y = count(i in 0..n) z = forall i in 0..(iv + n) { i <= 3 } }\n  action tick() {}\n  invariant Y { y == 3 }\n  invariant Z { z }\n}\n",
    )]);
    assert_eq!(run(&dir, &["check", "m.fsl"]).0, 0);
    assert_eq!(engine(&dir, "bmc").0, 0);
    assert_eq!(engine(&dir, "induction").0, 0);
    assert_eq!(engine(&dir, "explicit").0, 0);
    // Reading before the assignment stays the existing located init error.
    let dir = scratch(&[(
        "m.fsl",
        "spec M {\n  type T = 0..3\n  state { n: T, z: Bool }\n  init { z = forall i in 0..n { i >= 0 } n = 0 }\n}\n",
    )]);
    assert_eq!(run(&dir, &["check", "m.fsl"]).0, 2);
    // An unplanned bound stays refused after the assignment, with its statement location.
    let dir = scratch(&[(
        "m.fsl",
        "spec M {\n  type T = 0..3\n  state { n: T, m: Map<T, T>, z: Bool }\n  init { n = 0 forall j: T { m[j] = 1 }\n    z = count(i in 0..m[0]) >= 0 }\n}\n",
    )]);
    let expected = json!({
        "fsl": "1.0", "result": "error", "kind": "semantics",
        "message": "invalid init statement: public Kernel requires static aggregate range bounds at 5:5",
        "loc": {"line": 5, "column": 5},
    });
    assert_eq!(run(&dir, &["check", "m.fsl"]), (2, expected.clone()));
    assert_eq!(engine(&dir, "bmc"), (2, expected));
}

/// One Set-binder form, with what both symbolic engines report on base 5564b25cc's
/// sibling head 5ff48947b (`None`: the form is refused at `check`).
fn assert_set_binder(state: &str, init: &str, item: &str, verdict: Option<(i32, &str, &str)>) {
    let source = format!(
        "spec M {{\n  type K = 0..1\n  state {{ done: Bool, {state} }}\n  init {{ done = false {init} }}\n  {item}\n}}\n"
    );
    let dir = scratch(&[("m.fsl", &source)]);
    let (check, report) = run(&dir, &["check", "m.fsl"]);
    match verdict {
        None => {
            assert_eq!(check, 2, "{source}: {report}");
            assert_eq!(report["kind"], "semantics", "{source}");
            assert_eq!(engine(&dir, "bmc"), (check, report.clone()), "{source}");
            assert_eq!(engine(&dir, "induction"), (check, report), "{source}");
        }
        Some((exit, bmc, induction)) => {
            assert_eq!(check, 0, "{source}: {report}");
            let (code, produced) = engine(&dir, "bmc");
            assert_eq!(
                (code, produced["result"].as_str()),
                (exit, Some(bmc)),
                "{source}"
            );
            let (code, produced) = engine(&dir, "induction");
            assert_eq!(
                (code, produced["result"].as_str()),
                (exit, Some(induction)),
                "{source}"
            );
        }
    }
}

#[test]
fn state_set_binders_follow_the_candidates_the_engines_read_as_literals() {
    const HOLDS: Option<(i32, &str, &str)> = Some((0, "verified", "proved"));
    const FAILS: Option<(i32, &str, &str)> = Some((1, "violated", "violated"));
    const SET: &str = "xs: Set<K>";
    const OVER_XS: &str = "invariant C { forall x in xs { count(y in 0..x) >= 0 } }";
    const IDLE: &str = "action go() {}";
    const IN_REACHABLE: &str = "reachable C { forall x in xs { count(y in 0..x) >= 0 } }";
    // A non-empty init literal is certified on the first state like any other carrier.
    assert_set_binder(SET, "xs = Set { 1 }", &format!("{IDLE} {OVER_XS}"), HOLDS);
    assert_set_binder(
        SET,
        "xs = Set { 1 }",
        "action go() {} invariant C { forall x in xs { count(y in 0..x) == 9 } }",
        FAILS,
    );
    assert_set_binder(
        SET,
        "xs = Set { 1 }",
        "action go() { requires forall x in xs { count(y in 0..x) >= 0 } done = true }",
        HOLDS,
    );
    assert_set_binder(
        SET,
        "if done { xs = Set { 1 } } else { xs = Set { } }",
        &format!("{IDLE} {OVER_XS}"),
        HOLDS,
    );
    // Where no frame is certified the keys decide, and a key read from state has no fact.
    assert_set_binder(
        SET,
        "xs = Set { 1 }",
        &format!("{IDLE} {IN_REACHABLE}"),
        Some((0, "verified", "proved")),
    );
    assert_set_binder(
        "xs: Set<K>, v: K",
        "v = 0 xs = Set { v }",
        &format!("action go() {{ xs = xs.add(v) }} {IN_REACHABLE}"),
        None,
    );
    // An empty or unassigned init keeps the invariant executable and its verdict.
    assert_set_binder(SET, "xs = Set { }", &format!("{IDLE} {OVER_XS}"), HOLDS);
    assert_set_binder(SET, "", &format!("{IDLE} {OVER_XS}"), HOLDS);
    assert_set_binder(
        SET,
        "xs = Set { }",
        "action go() { xs = xs.add(1) } invariant C { forall x in xs { count(y in 0..x) == 9 } }",
        FAILS,
    );
    // Transition properties and postconditions read solver Sets, whatever init holds.
    assert_set_binder(
        SET,
        "xs = Set { 1 }",
        "action go() { xs = xs.add(1) } trans C { forall x in xs { count(y in 0..x) == 9 } }",
        FAILS,
    );
    assert_set_binder(
        SET,
        "xs = Set { 1 }",
        "action go() { done = true ensures forall x in old(xs).add(0) { count(y in 0..x) >= 0 } }",
        HOLDS,
    );
    // Literal candidates stay literal at any length, and a pair keeps its values.
    assert_set_binder(
        SET,
        "xs = Set { }",
        "action go() {} invariant C { forall x in Set { 1 } { count(y in 0..x) >= 0 } }",
        HOLDS,
    );
    assert_set_binder(
        SET,
        "xs = Set { }",
        "action go() {} invariant C { forall x in Set { 0, 1 } { count(y in 0..x) >= 0 } }",
        HOLDS,
    );
    assert_set_binder(
        SET,
        "xs = Set { }",
        "action go() {} invariant C { forall x in Set { 0, 1 } { count(y in 0..x) == 9 } }",
        FAILS,
    );
    // A state item has no literal value; a zero-capacity Seq has no candidate.
    assert_set_binder(
        "xs: Set<K>, v: K",
        "xs = Set { } v = 0",
        "action go() {} invariant C { forall x in xs.add(v) { count(y in 0..x) >= 0 } }",
        None,
    );
    assert_set_binder(
        "q: Seq<K, 0>",
        "q = Seq { }",
        "action go() {} invariant C { forall x in q { count(y in 0..x) >= 0 } }",
        HOLDS,
    );
    assert_set_binder(
        "q: Seq<K, 2>",
        "q = Seq { }",
        "action go() {} invariant C { forall x in q { count(y in 0..x) >= 0 } }",
        None,
    );
}

#[test]
fn slow_large_envelopes_stay_admitted() {
    // No execution cap: 300,001 candidates remain a slow form with a verdict.
    let dir = scratch(&[(
        "m.fsl",
        &spec("invariant P { forall i in 0..(n * 100000) { i >= 0 } }"),
    )]);
    assert_eq!(run(&dir, &["check", "m.fsl"]).0, 0);
}

#[test]
fn component_refusals_keep_the_component_wrapper() {
    let part = "spec B {\n  type T = 0..3\n  state { n: T }\n  init { n = 0 }\n  reachable R { forall i in 0..n { i >= 0 } }\n}\n";
    let system = "compose S {\n  use B as b from \"b.fsl\"\n}\n";
    let dir = scratch(&[("b.fsl", part), ("s.fsl", system)]);
    let (exit, produced) = run(&dir, &["check", "s.fsl"]);
    assert_eq!(exit, 2, "{produced}");
    assert_eq!(produced["kind"], "semantics");
    assert_eq!(produced["loc"], json!({"line": 2, "column": 3}));
    let message = produced["message"].as_str().unwrap();
    assert!(
        message.contains("b.fsl") && message.contains("is not an integer const"),
        "{message}"
    );
}

#[test]
fn bare_core_construction_keeps_its_boundary() {
    let source = spec("reachable P { forall i in 0..m[0] { i >= 0 } }");
    let dir = scratch(&[("m.fsl", &source)]);
    let kernel = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new(&dir))
        .unwrap_or_else(|error| panic!("{dir:?}: {error}"));
    let origins = kernel.origins().clone();
    let mut model = fsl_core::build_model(kernel).expect("bare construction admits the form");
    let refused = fsl_core::check_range_admission(&mut model, &origins).expect_err("admission");
    assert_eq!(refused.message, STATIC_INTEGER);
}

const LEGACY_TRACE: &str = "[]";

/// `replay` loads without checked admission: each expectation is base 5564b25cc output.
#[test]
fn replay_keeps_its_unchecked_load_and_trace_precedence() {
    let init_range = "spec M {\n  type T = 0..3\n  state { n: T, w: Map<T, Bool> }\n  init { n = 1 forall i in 0..n { w[i] = true } }\n  action tick() { requires n < 3 n = n + 1 }\n}\n";
    let init_struct = "spec M {\n  type T = 0..1\n  struct S { f: T }\n  state { n: T, w: Map<T, Bool> }\n  init { n = 1 forall j: T { w[j] = false } forall s: S { w[s.f] = true } }\n  action tick() { requires n < 1 n = n + 1 }\n}\n";
    let refused_elsewhere = "spec M {\n  type T = 0..3\n  state { n: T }\n  init { n = 1 }\n  action tick() { requires n < 3 n = n + 1 }\n  reachable R { forall i in 0..n { i >= 0 } }\n}\n";
    let dir = scratch(&[
        ("range.fsl", init_range),
        ("struct.fsl", init_struct),
        ("elsewhere.fsl", refused_elsewhere),
        ("legacy.json", LEGACY_TRACE),
        ("bad.json", "not json"),
    ]);
    let refusal = |message: &str| {
        (
            2,
            json!({"fsl": "1.0", "result": "error", "kind": "semantics", "message": message}),
        )
    };
    let io = |message: &str| {
        (
            2,
            json!({"fsl": "1.0", "result": "error", "kind": "io", "message": message}),
        )
    };
    let replay = |file: &str, trace: &str| run(&dir, &["replay", file, "--trace", trace]);
    assert_eq!(
        replay("range.fsl", "legacy.json"),
        refusal(
            "init forall range bounds must be compile-time constants; state variable 'n' is not allowed"
        )
    );
    assert_eq!(
        replay("struct.fsl", "legacy.json"),
        refusal("type 'S' is not a finite scalar")
    );
    let malformed = io("invalid JSON: expected ident at line 1 column 2");
    let missing = io("No such file or directory (os error 2)");
    for file in ["range.fsl", "struct.fsl", "elsewhere.fsl"] {
        assert_eq!(replay(file, "bad.json"), malformed, "{file}");
        assert_eq!(replay(file, "missing.json"), missing, "{file}");
    }
    // The new rule refuses this at check, and replay still replays it.
    assert_eq!(run(&dir, &["check", "elsewhere.fsl"]).0, 2);
    let (exit, produced) = replay("elsewhere.fsl", "legacy.json");
    assert_eq!((exit, produced["result"].as_str()), (0, Some("conformant")));
}
