// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Plan (iii): preserve main's ownership refusals, including refine's free-root
//! path. Expected values, mappings and diagnostics are independent literals.

use super::*;

const PI1: &str = r"spec ReviewPartialImpl {
  struct Triple { a: Bool, b: Bool, c: Bool }
  state { s: Triple, free: Bool }
  init { s.a = true s.b = true }
  action idle() { }
  invariant Initialized { not s.c }
}
";

const PARTIAL_ABS: &str = r"spec ReviewPartialAbs {
  state { c: Bool }
  init { c = false }
  action idle() { }
}
";

const PI1_MAPPING: &str = r"refinement ReviewPartialMapping {
  impl ReviewPartialImpl
  abs ReviewPartialAbs
  map c = s.c
  action idle() -> idle()
}
";

const PI2: &str = include_str!("issue_1481_pi2.fsl");
const PI2_MAPPING: &str = include_str!("issue_1481_pi2_mapping.fsl");
const PI3: &str = include_str!("issue_1481_pi3.fsl");
const PI3_MAPPING: &str = include_str!("issue_1481_pi3_mapping.fsl");

fn observe_refine(implementation: &str, abstraction: &str, mapping: &str) -> (Value, i32) {
    let fixture = Fixture::new(implementation);
    std::fs::write(fixture.0.join("abs.fsl"), abstraction).expect("write abstraction");
    std::fs::write(fixture.0.join("mapping.fsl"), mapping).expect("write mapping");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "refine",
            "model.fsl",
            "abs.fsl",
            "mapping.fsl",
            "--depth",
            "4",
        ])
        .current_dir(&fixture.0)
        .env_remove("FSLC_CACHE")
        .output()
        .expect("run working-tree native refine");
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "refine: invalid JSON: {error}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let exit = output.status.code().expect("native refine exit code");
    println!("Refine exit={exit} output={actual}");
    assert!(output.stderr.is_empty(), "refine: {output:?}");
    (actual, exit)
}

fn require_refine_error(implementation: &str, abstraction: &str, mapping: &str, message: &str) {
    let (actual, exit) = observe_refine(implementation, abstraction, mapping);
    assert_eq!(exit, 2, "refine: {actual:#}");
    // Refine stringifies RuntimeError: kind type, no loc or versions. Full
    // equality rejects both a false refines and a false refinement_failed.
    assert_eq!(
        actual,
        json!({"fsl": "1.0", "result": "error", "kind": "type", "message": message}),
        "refine: complete ownership/read rejection envelope"
    );
}

// PI1 detector: the exact review repro must not default-fill s.c=false on the
// free-root refine path. PI2 detects the same defect under a binder; PI3 detects
// counting distinct keys instead of checking each entry's field coverage.
// Use separate module names for the sixth consumer (macro identifiers below).
macro_rules! six_consumer_partial_cell {
    ($name:ident, $source:expr, $abs:expr, $mapping:expr, $message:expr, $line:expr, $column:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn check() {
                require(
                    $source,
                    Consumer::Check,
                    &semantics_at($message, $line, $column),
                    None,
                );
            }
            #[test]
            fn bmc() {
                require(
                    $source,
                    Consumer::Bmc,
                    &semantics_at($message, $line, $column),
                    None,
                );
            }
            #[test]
            fn induction() {
                require(
                    $source,
                    Consumer::Induction,
                    &semantics_at($message, $line, $column),
                    None,
                );
            }
            #[test]
            fn explicit() {
                require(
                    $source,
                    Consumer::Explicit,
                    &semantics_at($message, $line, $column),
                    None,
                );
            }
            #[test]
            fn monitor() {
                require($source, Consumer::Monitor, &semantics($message), None);
            }
            #[test]
            fn refine() {
                require_refine_error($source, $abs, $mapping, $message);
            }
        }
    };
}

six_consumer_partial_cell!(
    pi1_root_two_of_three,
    PI1,
    PARTIAL_ABS,
    PI1_MAPPING,
    "state variable 's' assigned more than once in init",
    4,
    21
);
six_consumer_partial_cell!(
    pi2_forall_two_of_three,
    PI2,
    PARTIAL_ABS,
    PI2_MAPPING,
    "state variable 'm' assigned more than once in init forall",
    5,
    38
);
six_consumer_partial_cell!(
    pi3_distinct_keys_each_partial,
    PI3,
    PARTIAL_ABS,
    PI3_MAPPING,
    "state variable 'm' assigned more than once in init",
    5,
    24
);

// PI4 preservation controls: nondefault field values are fixed in the invariant
// and the complete Monitor state, not inferred from another engine's output.
cell!(
    pi4_full_root,
    A4,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"s": {"a": true, "b": 2}}))
);
cell!(
    pi4_full_map,
    A2,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"0": {"flag": true, "n": 2}, "1": {"flag": true, "n": 2}}}))
);

fn full_with_free_root(source: &str) -> String {
    match source {
        A4 => source.replace("state { s: Pair }", "state { s: Pair, free: Bool }"),
        A2 => source.replace(
            "state { m: Map<K, Cell> }",
            "state { m: Map<K, Cell>, free: Bool }",
        ),
        _ => panic!("only independently specified PI4 fixtures"),
    }
}

const TRUE_ABS: &str = r"spec ReviewPartialAbs {
  state { c: Bool }
  init { c = true }
  action idle() { }
}
";

fn full_mapping(expression: &str) -> String {
    format!(
        "refinement FullMapping {{ impl MapFieldInit abs ReviewPartialAbs \
         map c = {expression} action idle() -> idle() }}\n"
    )
}

fn require_full_refine(source: &str, expression: &str, expected: &str, exit: i32) {
    let (actual, produced_exit) = observe_refine(
        &full_with_free_root(source),
        TRUE_ABS,
        &full_mapping(expression),
    );
    assert_eq!(produced_exit, exit, "refine: {actual:#}");
    assert_eq!(actual["result"], expected, "refine: {actual:#}");
    if exit == 0 {
        assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "impl": "MapFieldInit", "abs": "ReviewPartialAbs",
                "result": "refines", "checked_to_depth": 4, "action_map": {"idle": "idle"}
            }),
            "refine: complete positive envelope, no exclusions"
        );
    } else {
        assert_eq!(actual["kind"], "abs_state_mismatch", "refine: {actual:#}");
        assert_eq!(actual["at"], "init", "refine: {actual:#}");
        assert_eq!(actual["violated_at_step"], 0, "refine: {actual:#}");
        assert_eq!(actual["abs_after_expected"], json!({"c": true}));
        assert_eq!(actual["abs_after_actual"], json!({"c": false}));
        assert_eq!(actual["mismatch"], json!(["c"]));
        let state = &actual["impl_trace"][0]["state"];
        // free is intentionally unconstrained, but every initialized field is
        // fixed independently, including the nonzero fields in the wrong map.
        assert!(state["free"].is_boolean(), "refine: {actual:#}");
        match source {
            A4 => assert_eq!(state["s"], json!({"a": true, "b": 2})),
            A2 => assert_eq!(
                state["m"],
                json!({"0": {"flag": true, "n": 2}, "1": {"flag": true, "n": 2}})
            ),
            _ => unreachable!("only PI4 sources"),
        }
    }
}

// Positive map c=s.a is true; s.b==0 is false because b=2. The negative
// mapping is a rejecting control, not a second implementation-derived oracle.
#[test]
fn pi4_root_free_refines() {
    require_full_refine(A4, "s.a", "refines", 0);
}
#[test]
fn pi4_root_free_wrong_mapping_rejects() {
    require_full_refine(A4, "s.b == 0", "refinement_failed", 1);
}
#[test]
fn pi4_map_free_refines() {
    require_full_refine(A2, "m[0].flag", "refines", 0);
}
#[test]
fn pi4_map_free_wrong_mapping_rejects() {
    require_full_refine(A2, "m[1].n == 0", "refinement_failed", 1);
}

const PI5_ROOT: &str = r"spec OneFieldRoot {
  struct Pair { a: Bool, b: Bool }
  state { s: Pair }
  init { s.a = true }
  action idle() { }
  invariant Initialized { s.a }
}
";
const PI5_MAP: &str = r"spec OneFieldMap {
  type K = 0..1
  struct Pair { a: Bool, b: Bool }
  state { m: Map<K, Pair> }
  init { forall k: K { m[k].a = true } }
  action idle() { }
  invariant Initialized { m[0].a }
}
";

fn require_one_field(source: &str, name: &str, consumer: Consumer, root: &str) {
    let (actual, exit) = observe(source, consumer);
    if matches!(consumer, Consumer::Explicit | Consumer::Monitor) {
        let expected = json!({
            "fsl": "1.0", "result": "error", "kind": "semantics",
            "message": format!("init does not assign state variable(s): {root} (partial component initialization is rejected by the explicit engine)")
        });
        assert_eq!(exit, 2, "{consumer:?}: {actual:#}");
        assert_eq!(without_versions(actual, consumer), expected);
    } else if matches!(consumer, Consumer::Check) {
        assert_eq!(exit, 0, "{consumer:?}: {actual:#}");
        assert_eq!(
            without_versions(actual, consumer),
            json!({"fsl": "1.0", "result": "ok", "spec": name, "warnings": []})
        );
    } else {
        assert_eq!(exit, 0, "{consumer:?}: {actual:#}");
        assert_eq!(
            actual["result"],
            if matches!(consumer, Consumer::Bmc) {
                "verified"
            } else {
                "proved"
            }
        );
        assert_eq!(actual["invariants_checked"], json!(["Initialized"]));
        for field in ["kind", "message", "invariant", "violation_kind", "loc"] {
            assert!(actual.get(field).is_none(), "{consumer:?}: {actual:#}");
        }
    }
}

// PI5 preservation: only ONE field-writing statement, so legacy ownership
// already accepted it. Do not turn #1521's existing false refines into a golden.
macro_rules! one_field_cell {
    ($name:ident, $source:expr, $spec:expr, $root:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn check() {
                require_one_field($source, $spec, Consumer::Check, $root);
            }
            #[test]
            fn bmc() {
                require_one_field($source, $spec, Consumer::Bmc, $root);
            }
            #[test]
            fn induction() {
                require_one_field($source, $spec, Consumer::Induction, $root);
            }
            #[test]
            fn explicit() {
                require_one_field($source, $spec, Consumer::Explicit, $root);
            }
            #[test]
            fn monitor() {
                require_one_field($source, $spec, Consumer::Monitor, $root);
            }
        }
    };
}
one_field_cell!(pi5_one_field_root, PI5_ROOT, "OneFieldRoot", "s");
one_field_cell!(pi5_one_field_map, PI5_MAP, "OneFieldMap", "m");

fn require_free_sibling(source: &str, consumer: Consumer, root: &str, map: bool) {
    let source = if map {
        source.replace(
            "invariant Initialized { m[0].a }",
            "invariant Initialized { not m[1].b }",
        )
    } else {
        source.replace(
            "invariant Initialized { s.a }",
            "invariant Initialized { not s.b }",
        )
    };
    let (actual, exit) = observe(&source, consumer);
    assert_eq!(exit, 1, "{consumer:?}: {actual:#}");
    assert_eq!(actual["result"], "violated", "{consumer:?}: {actual:#}");
    assert_eq!(
        actual["invariant"], "Initialized",
        "{consumer:?}: {actual:#}"
    );
    assert_eq!(actual["violated_at_step"], 0, "{consumer:?}: {actual:#}");
    let state = &actual["trace"][0]["state"];
    assert_eq!(
        if map {
            &state[root]["1"]["b"]
        } else {
            &state[root]["b"]
        },
        true
    );
}

#[test]
fn pi5_root_bmc_sibling_is_free_not_default() {
    require_free_sibling(PI5_ROOT, Consumer::Bmc, "s", false);
}
#[test]
fn pi5_root_induction_sibling_is_free_not_default() {
    require_free_sibling(PI5_ROOT, Consumer::Induction, "s", false);
}
#[test]
fn pi5_map_bmc_sibling_is_free_not_default() {
    require_free_sibling(PI5_MAP, Consumer::Bmc, "m", true);
}
#[test]
fn pi5_map_induction_sibling_is_free_not_default() {
    require_free_sibling(PI5_MAP, Consumer::Induction, "m", true);
}

const Q_IF: &str = r"spec RImpl {
  type V = 0..1
  struct T { a: V, b: V, c: V }
  state { g: Bool, s: T, x: Bool }
  init { g = false if g { s.a = 0 s.b = 0 s.c = 0 } else { s.a = 0 s.b = 0 } }
  action step() { x = not x }
}
";
const Q_EMPTY: &str = r"spec RImpl {
  type V = 0..1
  struct T { a: V, b: V, c: V }
  state { s: T, x: Bool }
  init { forall k in 1..0 { s.c = 0 } s.a = 0 s.b = 0 }
  action step() { x = not x }
}
";
const Q_SUBSET: &str = r"spec RImpl {
  type K = 0..1
  type V = 0..1
  struct C { a: V, c: V }
  state { m: Map<K, C>, x: Bool }
  init { forall k in 0..0 { m[k].a = 0 m[k].c = 0 } }
  action step() { x = not x }
}
";
const Q_MISSING: &str = r"spec RImpl {
  type K = 0..1
  type V = 0..1
  struct C { a: V, b: V, c: V }
  state { m: Map<K, C>, x: Bool }
  init { forall k: K { m[k].a = 0 m[k].b = 0 } }
  action step() { x = not x }
}
";
const Q_ABS: &str = r"spec RAbs {
  type V = 0..1
  state { y: V }
  init { y = 0 }
  action noop() { }
}
";
const Q_MAP_S: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = s.c
  action step() -> noop()
}
";
const Q_MAP_M: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = m[1].c
  action step() -> noop()
}
";

// Path-sensitive detectors: static meet, empty-domain rollback, key-domain
// completeness and per-entry field completeness, respectively. A union of all
// syntactically written field names incorrectly accepts the first two.
six_consumer_partial_cell!(
    q_if_path,
    Q_IF,
    Q_ABS,
    Q_MAP_S,
    "state variable 's' assigned more than once in init",
    5,
    35
);
six_consumer_partial_cell!(
    q_empty_fixed,
    Q_EMPTY,
    Q_ABS,
    Q_MAP_S,
    "state variable 's' assigned more than once in init",
    5,
    39
);
six_consumer_partial_cell!(
    q_subset_keys,
    Q_SUBSET,
    Q_ABS,
    Q_MAP_M,
    "state variable 'm' assigned more than once in init forall",
    6,
    40
);
six_consumer_partial_cell!(
    q_map_missing_field,
    Q_MISSING,
    Q_ABS,
    Q_MAP_M,
    "state variable 'm' assigned more than once in init forall",
    6,
    35
);

const Q2_IMPL: &str = r"spec QImpl {
  type V = 0..1
  state { y: V }
  init { y = 1 }
  action step() { }
}
";
const Q2_ABS: &str = r"spec QAbs {
  type V = 0..1
  struct T { a: V, b: V, c: V }
  state { s: T, z: Bool }
  init { s.a = 0 s.b = 0 }
  action noop() { }
}
";
const Q2_MAP: &str = r"refinement Q {
  impl QImpl
  abs QAbs
  map s = T { a: 0, b: 0, c: y }
  map z = false
  action step() -> noop()
}
";
// Abstract-side detector: c is unconstrained symbolically, so default-filling
// it with zero invents a false abs_state_mismatch against implementation y=1.
ownership_cell!(
    q2_abstract_partial,
    Q2_ABS,
    "state variable 's' assigned more than once in init",
    5,
    18
);
#[test]
fn q2_refine_rejects_partial_abstraction() {
    require_refine_error(
        Q2_IMPL,
        Q2_ABS,
        Q2_MAP,
        "state variable 's' assigned more than once in init",
    );
}

// M1 detector: refine must use gate (a), not merely the new ownership rule.
#[test]
fn m1_refine_rejects_read_before_sibling() {
    require_refine_error(
        R1,
        PARTIAL_ABS,
        "refinement ReadGate { impl MapFieldInit abs ReviewPartialAbs map c = s.b action idle() -> idle() }",
        "init cannot read 's.a' before state variable 's' is fully assigned",
    );
}

fn observe_gate_pair(
    id: &str,
    source: &str,
    abstraction: &str,
    mapping: &str,
) -> (bool, bool, bool, bool, bool) {
    use fsl_core::{FsResolver, build_model, parse_kernel_source};

    let model =
        build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse GS source"))
            .expect("build GS model");
    let ownership = fsl_runtime::check_init_write_ownership(&model);
    let (check, check_exit) = observe(source, Consumer::Check);
    let (refine, refine_exit) = observe_refine(source, abstraction, mapping);
    // Not any error: require the runtime ownership message, type kind and
    // exit 2 to distinguish parse/type/mapping/budget failures from this gate.
    let refine_rejects_ownership = ownership.as_ref().err().is_some_and(|error| {
        refine_exit == 2
            && refine["result"] == "error"
            && refine["kind"] == "type"
            && refine["message"] == error.to_string()
    });
    let check_ok = check_exit == 0 && check["result"] == "ok";
    let check_ownership_error = check_exit == 2
        && check["result"] == "error"
        && check["kind"] == "semantics"
        && ownership
            .as_ref()
            .err()
            .is_some_and(|error| check["message"] == error.to_string());
    let refine_passes_gate = ownership.is_ok()
        && matches!(
            refine["result"].as_str(),
            Some("refines" | "refinement_failed" | "impl_violated")
        );
    let refine_passes_gate = if matches!(id, "PI5/root" | "PI5/map") {
        let root = if id == "PI5/root" { "s" } else { "m" };
        let prefix = format!("init does not assign state variable(s): {root} (partial");
        ownership.is_ok()
            && refine_exit == 2
            && refine["result"] == "error"
            && refine["kind"] == "type"
            && refine["message"].as_str().unwrap().starts_with(&prefix)
    } else {
        refine_passes_gate
    };
    let pair = (
        check_ok,
        check_ownership_error,
        ownership.is_ok(),
        refine_rejects_ownership,
        refine_passes_gate,
    );
    println!("GS {id}: produced={pair:?}; ownership={ownership:?}");
    pair
}

// GS is deliberately a table collected in full before the final assertion:
// one falsely accepted row must not hide the other partial/refine observations.
// It observes the public ownership function called at the start of refine's
// concrete_initial_states and CLI refine. One-field partial init is refused;
// free-value enumeration of partial init belongs to #1579.
#[test]
fn gs_check_and_refine_share_ownership_gate() {
    let full_root = full_with_free_root(A4);
    let full_map = full_with_free_root(A2);
    let root_mapping = full_mapping("s.a");
    let map_mapping = full_mapping("m[0].flag");
    let a1_mapping = "refinement A1 { impl MapFieldInit abs ReviewPartialAbs map c = m[0].flag action go() -> idle() }";
    let n3_mapping = full_mapping("m[0].flag");
    let n5_mapping = full_mapping("s.a");
    let one_root_free = PI5_ROOT.replace("state { s: Pair }", "state { s: Pair, free: Bool }");
    let one_map_free = PI5_MAP.replace(
        "state { m: Map<K, Pair> }",
        "state { m: Map<K, Pair>, free: Bool }",
    );
    let one_root_mapping = "refinement One { impl OneFieldRoot abs ReviewPartialAbs map c = s.b action idle() -> idle() }";
    let one_map_mapping = "refinement One { impl OneFieldMap abs ReviewPartialAbs map c = m[1].b action idle() -> idle() }";
    // Literal rows: (id, source, abstraction, mapping, ownership must reject).
    let rows = [
        ("PI1", PI1, PARTIAL_ABS, PI1_MAPPING, true),
        ("PI2", PI2, PARTIAL_ABS, PI2_MAPPING, true),
        ("PI3", PI3, PARTIAL_ABS, PI3_MAPPING, true),
        (
            "PI4/root",
            full_root.as_str(),
            TRUE_ABS,
            root_mapping.as_str(),
            false,
        ),
        (
            "PI4/map",
            full_map.as_str(),
            TRUE_ABS,
            map_mapping.as_str(),
            false,
        ),
        (
            "PI5/root",
            one_root_free.as_str(),
            PARTIAL_ABS,
            one_root_mapping,
            false,
        ),
        (
            "PI5/map",
            one_map_free.as_str(),
            PARTIAL_ABS,
            one_map_mapping,
            false,
        ),
        ("A1", A1, PARTIAL_ABS, a1_mapping, false),
        ("A2", A2, TRUE_ABS, map_mapping.as_str(), false),
        ("A4", A4, TRUE_ABS, root_mapping.as_str(), false),
        ("N3", N3, PARTIAL_ABS, n3_mapping.as_str(), true),
        ("N5", N5, TRUE_ABS, n5_mapping.as_str(), true),
        (
            "B2",
            B2,
            PARTIAL_ABS,
            include_str!("issue_1481_b2_mapping.fsl"),
            true,
        ),
        ("Q/if", Q_IF, Q_ABS, Q_MAP_S, true),
        ("Q/empty", Q_EMPTY, Q_ABS, Q_MAP_S, true),
        ("Q/subset", Q_SUBSET, Q_ABS, Q_MAP_M, true),
        ("Q/missing", Q_MISSING, Q_ABS, Q_MAP_M, true),
    ];
    assert_eq!(
        B2,
        include_str!("issue_1481_b2.fsl").replace(" // SPDX-License-Identifier: Apache-2.0", "")
    );
    let mut produced = Vec::new();
    let mut expected = Vec::new();
    for (id, source, abstraction, mapping, reject) in rows {
        produced.push((id, observe_gate_pair(id, source, abstraction, mapping)));
        expected.push((id, (!reject, reject, !reject, reject, !reject)));
    }
    assert_eq!(
        produced, expected,
        "GS: check/ownership/refine rows, no short circuit"
    );
}

// m3 detectors: each source isolates a different expression slot in gate (a).
// All field-written roots are fully written by the end. Some other assigned
// roots remain partial, but the approved ordering gives gate (a)'s read error
// priority over final coverage. Exact approved diagnostics remain literals.
const G_DEF: &str = r"spec G1 {
  struct Pair { a: 0..2, b: Bool }
  state { s: Pair }
  def aIsOne() = s.a == 1
  init { s.a = 1 s.b = aIsOne() }
  action idle() { }
  invariant P { s.b == true }
}
";
const G_IF: &str = r"spec G2 {
  struct Pair { a: 0..2, b: Bool }
  state { s: Pair }
  init {
    s.a = 1
    if s.a == 1 { s.b = true } else { s.b = false }
  }
  action idle() { }
  invariant P { s.b == true }
}
";
const G_KEY: &str = r"spec G3b {
  type K = 0..1
  struct Pair { a: K, b: Bool }
  state { s: Pair, w: Map<K, Bool> }
  init {
    s.a = 1
    w[s.a] = true
    s.b = true
  }
  action idle() { }
  invariant P { w[1] == true }
}
";
const G_WHERE: &str = r"spec G4 {
  type K = 0..1
  struct Pair { a: 0..1, b: Bool }
  state { s: Pair, m: Map<K, Bool> }
  init {
    s.a = 1
    forall k: K where k <= s.a { m[k] = true }
    s.b = true
  }
  action idle() { }
  invariant P { m[1] == true }
}
";
const G_QUANT: &str = r"spec G5 {
  type K = 0..1
  struct Pair { a: 0..1, b: Bool }
  state { s: Pair }
  init {
    s.a = 1
    s.b = exists k: K { k == s.a }
  }
  action idle() { }
  invariant P { s.b == true }
}
";
const G_QUANT_WHERE: &str = r"spec G8 {
  type K = 0..1
  struct Pair { a: K, b: Bool }
  state { s: Pair }
  init {
    s.a = 1
    s.b = exists k: K where k == s.a { true }
  }
  action idle() { }
  invariant P { s.b }
}
";
const G_AGG: &str = r"spec G9 {
  type K = 0..1
  type W = 0..4
  struct Pair { a: K, b: W }
  state { s: Pair }
  init {
    s.a = 1
    s.b = count(k: K where k <= s.a)
  }
  action idle() { }
  invariant P { s.b == 2 }
}
";
// All field writes occur inside branches. Dropping If recursion from the root
// collector loses s entirely and lets the RHS read s.a pass the check gate.
const G_BRANCH_ROOT: &str = r"spec BranchRoot {
  struct Pair { a: Bool, b: Bool }
  state { c: Bool, s: Pair }
  init {
    c = true
    if c {
      s.a = true
      s.b = s.a
    } else {
      s.a = false
      s.b = s.a
    }
  }
  action idle() { }
}
";

read_rejection_cell!(
    m3_def_body,
    G_DEF,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    5,
    18
);
read_rejection_cell!(
    m3_if_condition,
    G_IF,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    6,
    5
);
read_rejection_cell!(
    m3_target_key,
    G_KEY,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    7,
    5
);
read_rejection_cell!(
    m3_forall_where,
    G_WHERE,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    7,
    5
);
read_rejection_cell!(
    m3_quantifier_body,
    G_QUANT,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    7,
    5
);
read_rejection_cell!(
    m3_quantifier_where,
    G_QUANT_WHERE,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    7,
    5
);
read_rejection_cell!(
    m3_aggregate_filter,
    G_AGG,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    8,
    5
);
read_rejection_cell!(
    m3_if_field_root_collection,
    G_BRANCH_ROOT,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    8,
    7
);

// MX1–MX5 detectors: completing every field-written root is insufficient.
// Every root assigned on ANY path must be full when legacy ownership fails.
// These exact review repros leave a non-field-written map/scalar root partial;
// refine must not default-fill it and invent a refines verdict.
const MX1: &str = r"spec RImpl {
  type K = 0..1
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, m: Map<K, V>, x: Bool }
  init { s.a = 0 s.b = 0 m[0] = 0 }
  action step() { x = not x }
}
";
const MX2: &str = r"spec RImpl {
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, t: V, x: Bool }
  init { s.a = 0 s.b = 0 if false { t = 0 } }
  action step() { x = not x }
}
";
const MX3: &str = r"spec RImpl {
  type K = 0..1
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, m: Map<K, T>, x: Bool }
  init { s.a = 0 s.b = 0 m[0] = T { a: 0, b: 0 } }
  action step() { x = not x }
}
";
const MX4: &str = r"spec RImpl {
  type K = 0..1
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, m: Map<K, V>, x: Bool }
  init { s.a = 0 s.b = 0 forall k: K where k == 0 { m[k] = 0 } }
  action step() { x = not x }
}
";
const MX5: &str = r"spec RImpl {
  type K = 0..1
  type V = 0..1
  struct T { a: V, b: V }
  state { m: Map<K, T>, n: Map<K, V>, x: Bool }
  init { forall k: K { m[k].a = 0 m[k].b = 0 } n[0] = 0 }
  action step() { x = not x }
}
";
const MX_MAP_M: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = m[1]
  action step() -> noop()
}
";
const MX_MAP_T: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = t
  action step() -> noop()
}
";
const MX_MAP_M1A: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = m[1].a
  action step() -> noop()
}
";
const MX_MAP_N: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = n[1]
  action step() -> noop()
}
";
six_consumer_partial_cell!(
    mx1_mixed_map_partial,
    MX1,
    Q_ABS,
    MX_MAP_M,
    "state variable 's' assigned more than once in init",
    6,
    18
);
six_consumer_partial_cell!(
    mx2_mixed_if_scalar,
    MX2,
    Q_ABS,
    MX_MAP_T,
    "state variable 's' assigned more than once in init",
    5,
    18
);
six_consumer_partial_cell!(
    mx3_mixed_struct_map_entry,
    MX3,
    Q_ABS,
    MX_MAP_M1A,
    "state variable 's' assigned more than once in init",
    6,
    18
);
six_consumer_partial_cell!(
    mx4_mixed_where_forall,
    MX4,
    Q_ABS,
    MX_MAP_M,
    "state variable 's' assigned more than once in init",
    6,
    18
);
six_consumer_partial_cell!(
    mx5_mixed_same_map_other_root,
    MX5,
    Q_ABS,
    MX_MAP_N,
    "state variable 'm' assigned more than once in init forall",
    6,
    35
);

// MX6 is the same missing scalar on the ABSTRACT side: default-filling t=0
// would reject the valid mapped t=1 and invent a refinement_failed verdict.
const MX6: &str = r"spec QAbs {
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, t: V, z: Bool }
  init { s.a = 0 s.b = 0 if false { t = 0 } }
  action noop() { }
}
";
const MX6_MAPPING: &str = r"refinement Q {
  impl QImpl
  abs QAbs
  map s = T { a: 0, b: 0 }
  map t = y
  map z = false
  action step() -> noop()
}
";
ownership_cell!(
    mx6_abstract_mixed,
    MX6,
    "state variable 's' assigned more than once in init",
    5,
    18
);
#[test]
fn mx6_refine_abstract_mixed() {
    require_refine_error(
        Q2_IMPL,
        MX6,
        MX6_MAPPING,
        "state variable 's' assigned more than once in init",
    );
}

// SH1/SH2 detectors: an outer binder shadows the global const. The inner
// filter is false, so it supplies NO coverage. Do not use the global value to
// admit new partial init. This is not a fix/golden for #1515's symbolic domain.
const SH1: &str = r"spec Extra {
  const flag = true
  type K = 0..1
  struct T { a: Bool, b: Bool, c: Bool }
  state { s: T, free: Bool }
  init {
    s.a = true
    s.b = true
    forall flag in Set { false } {
      forall j: K where flag { s.c = false }
    }
  }
  action idle() { }
  invariant Initialized { not s.c }
}
";
const SH2: &str = r"spec Extra {
  const flag = 1
  type K = 0..1
  struct T { a: Bool, b: Bool, c: Bool }
  state { s: T, free: Bool }
  init {
    s.a = true
    s.b = true
    forall flag in 0..0 {
      forall j: K where flag == 1 { s.c = false }
    }
  }
  action idle() { }
  invariant Initialized { not s.c }
}
";
const SH_ABS: &str = r"spec ExtraAbs {
  state { c: Bool }
  init { c = false }
  action idle() { }
}
";
const SH_ROOT_MAPPING: &str = r"refinement ExtraMapping {
  impl Extra
  abs ExtraAbs
  map c = s.c
  action idle() -> idle()
}
";
six_consumer_partial_cell!(
    sh1_shadow_filter_collection,
    SH1,
    SH_ABS,
    SH_ROOT_MAPPING,
    "state variable 's' assigned more than once in init",
    8,
    5
);
six_consumer_partial_cell!(
    sh2_shadow_filter_range,
    SH2,
    SH_ABS,
    SH_ROOT_MAPPING,
    "state variable 's' assigned more than once in init",
    8,
    5
);

// SH3 detects the same phantom coverage on the abstraction side, where the
// consequence is a false mismatch rather than a false refines.
const SH3: &str = r"spec ExtraAbsShadow {
  const flag = true
  type K = 0..1
  struct T { a: Bool, b: Bool, c: Bool }
  state { s: T, free: Bool }
  init {
    s.a = true
    s.b = true
    forall flag in Set { false } {
      forall j: K where flag { s.c = false }
    }
  }
  action idle() { }
  invariant Initialized { s.a and s.b }
}
";
const SH3_IMPL: &str = r"spec ExtraImpl {
  state { c: Bool }
  init { c = true }
  action idle() { }
}
";
const SH3_MAPPING: &str = r"refinement ExtraAbsShadowMapping {
  impl ExtraImpl
  abs ExtraAbsShadow
  map s = T { a: true, b: true, c: c }
  map free = false
  action idle() -> idle()
}
";
ownership_cell!(
    sh3_abstract_shadow_filter,
    SH3,
    "state variable 's' assigned more than once in init",
    8,
    5
);
#[test]
fn sh3_refine_abstract_shadow_filter() {
    require_refine_error(
        SH3_IMPL,
        SH3,
        SH3_MAPPING,
        "state variable 's' assigned more than once in init",
    );
}

// SH4/SH5 pin MAIN's refusals for shadowed range bounds, not a resolution of
// #1515. In SH5 concrete main stops at nested-forall before ownership: that
// distinct error/no-location is deliberately preserved, not allowlisted away.
const SH4: &str = r"spec Extra {
  const limit = 1
  struct T { a: Bool, b: Bool, c: Bool }
  state { s: T, free: Bool }
  init {
    s.a = true
    s.b = true
    forall limit in 0..0 {
      forall j in 1..limit { s.c = false }
    }
  }
  action idle() { }
  invariant Initialized { not s.c }
}
";
const SH5: &str = r"spec Extra {
  const limit = 1
  type K = 0..1
  struct T { a: Bool, b: Bool, c: Bool }
  state { m: Map<K, T>, free: Bool }
  init {
    forall limit in 0..0 {
      forall k in 0..limit {
        m[k].a = true
        m[k].b = true
        m[k].c = false
      }
    }
  }
  action idle() { }
  invariant Initialized { not m[1].c }
}
";
const SH_MAP_MAPPING: &str = r"refinement ExtraMapping {
  impl Extra
  abs ExtraAbs
  map c = m[1].c
  action idle() -> idle()
}
";
six_consumer_partial_cell!(
    sh4_shadow_bound,
    SH4,
    SH_ABS,
    SH_ROOT_MAPPING,
    "state variable 's' assigned more than once in init",
    7,
    5
);
cell!(
    sh5_shadow_keys,
    SH5,
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        10,
        9
    ),
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        10,
        9
    ),
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        10,
        9
    ),
    semantics("nested forall in init is not supported"),
    semantics("nested forall in init is not supported"),
    None
);
#[test]
fn sh5_refine_shadow_keys() {
    require_refine_error(
        SH5,
        SH_ABS,
        SH_MAP_MAPPING,
        "state variable 'm' assigned more than once in init forall",
    );
}

// Positive controls defeat a blanket nested-forall ban and a blanket ban on
// free roots alongside new field-wise init. Values/invariants are independent
// literals; negative mappings must fail at init, not merely return any error.
const PC_NESTED: &str = include_str!("issue_1481_pc_nested.fsl");
const PC_FREE: &str = include_str!("issue_1481_pc_free.fsl");
cell!(
    pc_nested_nonshadow_nonempty,
    PC_NESTED,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    semantics("nested forall in init is not supported"),
    semantics("nested forall in init is not supported"),
    None
);
cell!(
    pc_full_fields_never_assigned_root,
    PC_FREE,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    semantics("init does not assign state variable(s): free"),
    semantics("init does not assign state variable(s): free"),
    None
);

fn require_control_refine(source: &str, expression: &str, negative: bool) {
    let mapping = full_mapping(expression);
    let (actual, exit) = observe_refine(source, TRUE_ABS, &mapping);
    if negative {
        assert_eq!(exit, 1, "refine: {actual:#}");
        assert_eq!(actual["result"], "refinement_failed", "{actual:#}");
        assert_eq!(actual["kind"], "abs_state_mismatch", "{actual:#}");
        assert_eq!(actual["at"], "init", "{actual:#}");
        assert_eq!(actual["violated_at_step"], 0, "{actual:#}");
        assert_eq!(actual["abs_after_expected"], json!({"c": true}));
        assert_eq!(actual["abs_after_actual"], json!({"c": false}));
        assert_eq!(actual["mismatch"], json!(["c"]));
        let state = &actual["impl_trace"][0]["state"];
        assert!(state["free"].is_boolean(), "{actual:#}");
        assert_eq!(
            state["s"],
            if source == PC_NESTED {
                json!({"a": true, "b": true, "c": false})
            } else {
                json!({"a": true, "b": false})
            }
        );
    } else {
        assert_eq!(exit, 0, "refine: {actual:#}");
        assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "impl": "MapFieldInit", "abs": "ReviewPartialAbs",
                "result": "refines", "checked_to_depth": 4, "action_map": {"idle": "idle"}
            })
        );
    }
}
#[test]
fn pc_nested_refines() {
    require_control_refine(PC_NESTED, "not s.c", false);
}
#[test]
fn pc_nested_wrong_mapping_rejects() {
    require_control_refine(PC_NESTED, "s.c", true);
}
#[test]
fn pc_free_refines() {
    require_control_refine(PC_FREE, "s.a", false);
}
#[test]
fn pc_free_wrong_mapping_rejects() {
    require_control_refine(PC_FREE, "s.b", true);
}

// Legacy controls: full.value is a completely field-written root that needs
// only ONE statement, so main's legacy probe already accepts these programs.
// Beside it, preserve single-field partial, whole-root, per-key partial and
// mutually exclusive field writes. A global "all assigned roots full" gate
// would wrongly reject the partial controls. Never golden #1521's verdict.
const LC_SINGLE: &str = include_str!("issue_1481_lc_single.fsl");
const LC_WHOLE: &str = include_str!("issue_1481_lc_whole.fsl");
const LC_KEY: &str = include_str!("issue_1481_lc_key.fsl");
const LC_BRANCH: &str = include_str!("issue_1481_lc_branch.fsl");
cell!(
    lc_single_field_beside_full_root,
    LC_SINGLE,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    semantics(
        "init does not assign state variable(s): s (partial component initialization is rejected by the explicit engine)"
    ),
    semantics(
        "init does not assign state variable(s): s (partial component initialization is rejected by the explicit engine)"
    ),
    None
);
cell!(
    lc_whole_root_beside_full_root,
    LC_WHOLE,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"full": {"value": true}, "s": {"a": true, "b": false}}))
);
cell!(
    lc_per_key_beside_full_root,
    LC_KEY,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    semantics(
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    ),
    semantics(
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    ),
    None
);
cell!(
    lc_branch_split_beside_full_root,
    LC_BRANCH,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    semantics(
        "init does not assign state variable(s): s (partial component initialization is rejected by the explicit engine)"
    ),
    semantics(
        "init does not assign state variable(s): s (partial component initialization is rejected by the explicit engine)"
    ),
    None
);

// Legacy partial refine verdicts are deliberately not pinned (#1521). The
// positive/negative full-init refinement controls above pin correct results.

// RB1–RB4: exact review4 c11–c14 sources. The other assigned root is complete
// at the end, but is read before its assignment. Free x/z forces refine through
// concrete_initial_states' free-root path rather than deterministic init.
const RB1: &str = r"spec RImpl {
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, t: V, x: Bool }
  init {
    s.a = t
    s.b = 0
    t = 1
  }
  action step() { }
}
";
const RB2: &str = r"spec RImpl {
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, t: V, x: Bool }
  init {
    s.a = 0
    if t == 1 { s.b = 1 } else { s.b = 0 }
    t = 1
  }
  action step() { }
  invariant A { s.b == 1 }
}
";
const RB3: &str = r"spec RImpl {
  type V = 0..1
  type K = 0..1
  struct T { a: V, b: V }
  state { s: T, m: Map<K, V>, x: Bool }
  init {
    s.a = m[0]
    s.b = 0
    forall k: K { m[k] = 1 }
  }
  action step() { }
  invariant A { s.a == 1 }
}
";
const RB4: &str = r"spec RAbs {
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, t: V, z: Bool }
  init {
    s.a = t
    s.b = 0
    t = 1
  }
  action noop() { }
  invariant A { s.a == 1 }
}
";
const RB_ABS: &str = r"spec RAbs {
  type V = 0..1
  state { y: V }
  init { y = 0 }
  action noop() { }
}
";
const RB_IMPL: &str = r"spec RImpl {
  type V = 0..1
  state { y: V }
  init { y = 1 }
  action step() { }
}
";
const RB_MAP_SA: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = s.a
  action step() -> noop()
}
";
const RB_MAP_SB: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = s.b
  action step() -> noop()
}
";
const RB_MAP_ABS: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map s = T { a: y, b: 0 }
  map t = 1
  map z = false
  action step() -> noop()
}
";

// Main 11f01225 was measured separately for EACH consumer. Its concrete
// preflight reports the early read without loc, not the ownership error that
// check/symbolic/refine report. Pin both, rather than homogenizing diagnostics.
macro_rules! root_read_before_cell {
    ($name:ident, $source:expr, $root:expr, $line:expr, $column:expr,
     $implementation:expr, $abstraction:expr, $mapping:expr) => {
        mod $name {
            use super::*;
            cell!(
                consumers,
                $source,
                semantics_at(
                    "state variable 's' assigned more than once in init",
                    $line,
                    $column
                ),
                semantics_at(
                    "state variable 's' assigned more than once in init",
                    $line,
                    $column
                ),
                semantics_at(
                    "state variable 's' assigned more than once in init",
                    $line,
                    $column
                ),
                semantics(concat!(
                    "init references state variable '",
                    $root,
                    "' before it is assigned"
                )),
                semantics(concat!(
                    "init references state variable '",
                    $root,
                    "' before it is assigned"
                )),
                None
            );
            #[test]
            fn refine() {
                require_refine_error(
                    $implementation,
                    $abstraction,
                    $mapping,
                    "state variable 's' assigned more than once in init",
                );
            }
        }
    };
}

root_read_before_cell!(rb1_whole_rhs, RB1, "t", 7, 5, RB1, RB_ABS, RB_MAP_SA);
root_read_before_cell!(rb2_if_condition, RB2, "t", 7, 17, RB2, RB_ABS, RB_MAP_SB);
root_read_before_cell!(
    rb3_map_before_forall,
    RB3,
    "m",
    8,
    5,
    RB3,
    RB_ABS,
    RB_MAP_SA
);
root_read_before_cell!(rb4_abstract_rhs, RB4, "t", 7, 5, RB_IMPL, RB4, RB_MAP_ABS);

fn root_read_after(source: &str) -> String {
    // Move the same assignment, without changing its value, domain or mapping.
    // RB1 gains a value-pinning invariant absent from the original review repro.
    match source {
        RB1 => source
            .replace("    t = 1\n", "")
            .replace("  init {\n", "  init {\n    t = 1\n")
            .replace(
                "  action step() { }",
                "  action step() { }\n  invariant A { s.a == 1 and s.b == 0 and t == 1 }",
            ),
        RB2 | RB4 => source
            .replace("    t = 1\n", "")
            .replace("  init {\n", "  init {\n    t = 1\n"),
        RB3 => source
            .replace("    forall k: K { m[k] = 1 }\n", "")
            .replace("  init {\n", "  init {\n    forall k: K { m[k] = 1 }\n"),
        _ => panic!("only RB review sources"),
    }
}

fn require_root_read_after(source: &str, consumer: Consumer, concrete: bool) {
    let mut control = root_read_after(source);
    let abstract_side = source == RB4;
    if concrete {
        // Also exercise full deterministic init and the entire Monitor state.
        // Refine controls below KEEP the free root to pin the vulnerable path.
        control = control.replace(
            "  init {\n",
            if abstract_side {
                "  init {\n    z = false\n"
            } else {
                "  init {\n    x = false\n"
            },
        );
    }
    if !concrete && matches!(consumer, Consumer::Explicit | Consumer::Monitor) {
        require(
            &control,
            consumer,
            &semantics(if abstract_side {
                "init does not assign state variable(s): z"
            } else {
                "init does not assign state variable(s): x"
            }),
            None,
        );
        return;
    }
    let (actual, exit) = observe(&control, consumer);
    assert_eq!(exit, 0, "{consumer:?}: {actual:#}");
    let name = if abstract_side { "RAbs" } else { "RImpl" };
    match consumer {
        Consumer::Check => assert_eq!(
            without_versions(actual, consumer),
            json!({"fsl": "1.0", "result": "ok", "spec": name, "warnings": []}),
        ),
        Consumer::Monitor => {
            let initial = match source {
                RB1 => json!({"s": {"a": 1, "b": 0}, "t": 1, "x": false}),
                RB2 => json!({"s": {"a": 0, "b": 1}, "t": 1, "x": false}),
                RB3 => json!({"s": {"a": 1, "b": 0}, "m": {"0": 1, "1": 1}, "x": false}),
                RB4 => json!({"s": {"a": 1, "b": 0}, "t": 1, "z": false}),
                _ => unreachable!("only RB sources"),
            };
            assert_eq!(
                actual,
                json!({
                    "fsl": "1.0", "result": "conformant", "spec": name,
                    "steps_checked": 0, "final_state": initial,
                    "note": "leadsTo properties are not checked by replay (finite logs only)"
                })
            );
        }
        Consumer::Bmc | Consumer::Induction | Consumer::Explicit => {
            assert_eq!(
                actual["result"],
                if matches!(consumer, Consumer::Bmc) {
                    "verified"
                } else {
                    "proved"
                }
            );
            assert_eq!(
                actual["invariants_checked"],
                if source == RB3 {
                    json!(["_bounds_s", "_bounds_m", "A"])
                } else {
                    json!(["_bounds_s", "_bounds_t", "A"])
                }
            );
            for field in [
                "kind",
                "message",
                "invariant",
                "violation_kind",
                "loc",
                "violation",
                "last_action",
            ] {
                assert!(actual.get(field).is_none(), "{consumer:?}: {actual:#}");
            }
            assert_eq!(
                actual["completeness"],
                if matches!(consumer, Consumer::Bmc) {
                    "bounded"
                } else {
                    "unbounded"
                }
            );
            if matches!(consumer, Consumer::Explicit) {
                assert_eq!(actual["closure"], true, "{actual:#}");
            }
        }
    }
}

fn require_root_read_after_refine(source: &str, negative: bool) {
    let control = root_read_after(source);
    let one_abs = RB_ABS.replace("y = 0", "y = 1");
    // Keep abs invariant A true and violate t=1 instead. This specifically
    // tests init correspondence, not the earlier abstract-invariant check.
    let bad_abs_map = RB_MAP_ABS.replace("map t = 1", "map t = 0");
    let (actual, exit) = if source == RB4 {
        observe_refine(
            RB_IMPL,
            &control,
            if negative { &bad_abs_map } else { RB_MAP_ABS },
        )
    } else {
        observe_refine(
            &control,
            if negative { RB_ABS } else { &one_abs },
            if source == RB2 { RB_MAP_SB } else { RB_MAP_SA },
        )
    };
    if negative {
        assert_eq!(exit, 1, "{actual:#}");
        assert_eq!(actual["result"], "refinement_failed", "{actual:#}");
        assert_eq!(actual["kind"], "abs_state_mismatch", "{actual:#}");
        assert_eq!(actual["at"], "init", "{actual:#}");
        assert_eq!(actual["violated_at_step"], 0, "{actual:#}");
        if source == RB4 {
            assert_eq!(
                actual["abs_after_expected"],
                json!({"s": {"a": 1, "b": 0}, "t": 1, "z": false})
            );
            assert_eq!(
                actual["abs_after_actual"],
                json!({"s": {"a": 1, "b": 0}, "t": 0, "z": false})
            );
            assert_eq!(actual["mismatch"], json!(["t"]));
            assert_eq!(actual["impl_trace"][0]["state"], json!({"y": 1}));
        } else {
            assert_eq!(actual["abs_after_expected"], json!({"y": 0}));
            assert_eq!(actual["abs_after_actual"], json!({"y": 1}));
            assert_eq!(actual["mismatch"], json!(["y"]));
            let state = &actual["impl_trace"][0]["state"];
            assert!(state["x"].is_boolean(), "{actual:#}");
            assert_eq!(
                state["s"],
                if source == RB2 {
                    json!({"a": 0, "b": 1})
                } else {
                    json!({"a": 1, "b": 0})
                }
            );
            if source == RB3 {
                assert_eq!(state["m"], json!({"0": 1, "1": 1}));
            } else {
                assert_eq!(state["t"], 1);
            }
        }
    } else {
        assert_eq!(exit, 0, "{actual:#}");
        assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "impl": "RImpl", "abs": "RAbs", "result": "refines",
                "checked_to_depth": 4, "action_map": {"step": "noop"}
            })
        );
    }
}

macro_rules! root_read_after_cell {
    ($name:ident, $source:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn check() {
                require_root_read_after($source, Consumer::Check, false);
            }
            #[test]
            fn bmc() {
                require_root_read_after($source, Consumer::Bmc, false);
            }
            #[test]
            fn induction() {
                require_root_read_after($source, Consumer::Induction, false);
            }
            #[test]
            fn explicit_free() {
                require_root_read_after($source, Consumer::Explicit, false);
            }
            #[test]
            fn monitor_free() {
                require_root_read_after($source, Consumer::Monitor, false);
            }
            #[test]
            fn explicit_full() {
                require_root_read_after($source, Consumer::Explicit, true);
            }
            #[test]
            fn monitor_full() {
                require_root_read_after($source, Consumer::Monitor, true);
            }
            #[test]
            fn refine() {
                require_root_read_after_refine($source, false);
            }
            #[test]
            fn wrong_mapping_rejects() {
                require_root_read_after_refine($source, true);
            }
        }
    };
}
root_read_after_cell!(rb1_read_after_control, RB1);
root_read_after_cell!(rb2_read_after_control, RB2);
root_read_after_cell!(rb3_read_after_control, RB3);
root_read_after_cell!(rb4_read_after_control, RB4);

// Diagnostic-order controls measured on main, not inferred from head. Early
// reads win in DO1–DO3; DO4's coarse field collision wins over a later read.
const DO1: &str = r"spec RImpl {
  state { t: Bool, x: Bool }
  init { t = x t = true }
  action step() { x = not x }
}
";
const DO2: &str = r"spec RImpl {
  type V = 0..1
  struct T { a: Bool, b: V, c: V }
  state { s: T, x: Bool }
  init { s.a = x s.b = 0 }
  action step() { x = not x }
}
";
const DO3: &str = r"spec RImpl {
  type K = 0..1
  type V = 0..1
  struct T { a: V, b: V, c: V }
  state { s: T, x: Bool }
  def keep() = x
  init { forall k: K where keep() { s.c = 0 } s.a = 0 s.b = 0 }
  action step() { x = not x }
}
";
const DO4: &str = r"spec RImpl {
  type V = 0..1
  struct T { a: V, b: V, c: V }
  state { s: T, t: V, x: V }
  init { s.a = 0 s.b = 0 t = x }
  action step() { }
}
";
macro_rules! diagnostic_order_cell {
    ($name:ident, $source:expr, $check:expr, $explicit:expr, $monitor:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn check() {
                require($source, Consumer::Check, &$check, None);
            }
            #[test]
            fn explicit() {
                require($source, Consumer::Explicit, &$explicit, None);
            }
            #[test]
            fn monitor() {
                require($source, Consumer::Monitor, &$monitor, None);
            }
        }
    };
}
diagnostic_order_cell!(
    do1_scalar_duplicate,
    DO1,
    semantics_at("state variable 't' assigned more than once in init", 3, 16),
    semantics("init references state variable 'x' before it is assigned"),
    semantics("init references state variable 'x' before it is assigned")
);
diagnostic_order_cell!(
    do2_partial_three_fields,
    DO2,
    semantics_at("state variable 's' assigned more than once in init", 5, 18),
    semantics("init references state variable 'x' before it is assigned"),
    semantics("init references state variable 'x' before it is assigned")
);
diagnostic_order_cell!(
    do3_where_def_state,
    DO3,
    semantics_at("state variable 's' assigned more than once in init", 7, 47),
    semantics("init references state variable 'x' before it is assigned"),
    semantics("init references state variable 'x' before it is assigned")
);
diagnostic_order_cell!(
    do4_collision_then_free_read,
    DO4,
    semantics_at("state variable 's' assigned more than once in init", 5, 18),
    semantics_at("state variable 's' assigned more than once in init", 5, 18),
    semantics("state variable 's' assigned more than once in init")
);

#[path = "issue_1481_review5.rs"]
mod review5;
