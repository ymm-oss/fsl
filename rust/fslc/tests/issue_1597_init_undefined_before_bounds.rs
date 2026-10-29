// SPDX-License-Identifier: Apache-2.0

//! #1597: assigned-target bounds must not hide an undefined init operation.
//! Locations name the failing statement, following #1543. Each engine gets
//! its own cell; the unbounded target and free-input cases preserve behavior.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn verify(source: &str, engine: &str) -> (Value, i32) {
    // Keep fixtures in TMPDIR for inspection, without deleting evidence.
    let path = std::env::temp_dir().join(format!(
        "fsl-1597-{}-{}.fsl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, source).expect("write inline fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("verify")
        .arg(&path)
        .args(["--engine", engine, "--depth", "4", "--no-cache"])
        .env_remove("FSLC_CACHE")
        .output()
        .expect("run working-tree CLI");
    assert!(output.stderr.is_empty(), "{engine}: {output:?}");
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{engine}: invalid JSON: {error}; {output:?}"));
    let exit = output.status.code().expect("native exit status");
    println!(
        "{engine} fixture={} exit={exit} output={value}",
        path.display()
    );
    (value, exit)
}

fn assert_error(value: &Value, exit: i32, message: &str) {
    assert_eq!(exit, 2, "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "semantics", "{value:#}");
    assert_eq!(value["message"], message, "{value:#}");
    for key in ["loc", "violation_kind", "invariant"] {
        assert!(value.get(key).is_none(), "{key}: {value:#}");
    }
}

fn require_error(source: &str, engine: &str, stem: &str, location: &str) {
    let (value, exit) = verify(source, engine);
    assert_error(&value, exit, &format!("{stem} in init at {location}"));
}

macro_rules! failure_cell {
    ($name:ident, $source:expr, $stem:literal, $location:literal) => {
        failure_cell!(
            $name,
            $source,
            $stem,
            $location,
            concat!($stem, " in init at ", $location)
        );
    };
    ($name:ident, $source:expr, $stem:literal, $location:literal, $explicit:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn bmc() {
                require_error($source, "bmc", $stem, $location);
            }
            #[test]
            fn induction() {
                require_error($source, "induction", $stem, $location);
            }
            #[test]
            fn explicit() {
                let (value, exit) = verify($source, "explicit");
                assert_error(&value, exit, $explicit);
            }
            #[test]
            fn auto() {
                require_error($source, "auto", $stem, $location);
            }
        }
    };
}

const POP_BOUNDED: &str = "spec PopBounded {\n  type K = 0..1\n  state { xs: Seq<K, 2>, n: 0..1 }\n  init { xs = Seq {}  n = xs.pop().size() }\n  action step() { }\n  invariant P { true }\n}\n";
const POP_DIRECT: &str = "spec PopDirect {\n  type K = 0..1\n  state { xs: Seq<K, 2>, ys: Seq<K, 2> }\n  init { xs = Seq {}  ys = xs.pop() }\n  action step() { }\n  invariant P { true }\n}\n";
const EXCEPTION: &str = "spec Exception {\n  state { n: 0..1, t: Int }\n  init { n = 5  t = 1 / 0 }\n  action step() { }\n  invariant P { true }\n}\n";
const MAP_BOUNDED: &str = "spec MapBounded {\n  type K = 0..1\n  state { m: Map<K, K>, n: 2..3 }\n  init { m[0] = 0  m[1] = 1  n = m[2] }\n  action step() { }\n  invariant P { true }\n}\n";

// Detector cells: the old symbolic query instead reports a type bound or a
// negative model sequence length. Locations were observed with unbounded n
// (the same failing statement); POP_DIRECT starts ys at the same column as n.
failure_cell!(popbounded, POP_BOUNDED, "pop() on empty sequence", "4:23");
failure_cell!(popdirect, POP_DIRECT, "pop() on empty sequence", "4:23");
failure_cell!(
    out_of_range_and_undefined,
    EXCEPTION,
    "division by zero",
    "3:17"
);
failure_cell!(
    map_bounded,
    MAP_BOUNDED,
    "map index outside finite key domain",
    "4:30"
);

// Review F2 / recursive-walk coverage: the undefined value flows into an
// indexed map entry, a struct field, or an Option payload, not just a scalar
// root. Preserve the exact probe layouts so their observed statement locations
// remain independent of the assignment-root implementation.
const NESTED_POP: &str = "spec NestedPop {\n  type K = 0..1\n  state { xs: Seq<K, 2>, m: Map<K, K> }\n  init {\n    xs = Seq {}\n    if true {\n      forall k: K { m[k] = xs.pop().size() }\n    } else {\n      forall k: K { m[k] = 0 }\n    }\n  }\n  action step() { }\n  invariant P { true }\n}\n";
const NESTED_IF_FORALL: &str = "spec NestedIfForall {\n  type K = 0..1\n  type V = 0..1\n  state { c: Bool, xs: Seq<K, 2>, m: Map<K, V> }\n  init {\n    xs = Seq {}\n    if c { forall k: K { m[k] = xs.pop().size() } } else { forall k: K { m[k] = 0 } }\n  }\n  action step() { }\n  invariant P { true }\n}\n";
const STRUCT_FIELD_BOUNDED: &str = "spec StructFieldBounded {\n  type K = 0..1\n  type V = 2..3\n  struct Pt { a: V, b: V }\n  state { c: Bool, xs: Seq<K, 2>, s: Pt }\n  init { xs = Seq {}  s.a = 2  s.b = xs.pop().size() }\n  action step() { }\n  invariant P { true }\n}\n";
const OPTION_BOUNDED: &str = "spec OptionBounded {\n  type K = 0..1\n  type V = 2..3\n  state { c: Bool, xs: Seq<K, 2>, o: Option<V> }\n  init { xs = Seq {}  if c { o = some(xs.pop().size()) } else { o = none } }\n  action step() { }\n  invariant P { true }\n}\n";

failure_cell!(nested_pop, NESTED_POP, "pop() on empty sequence", "7:21");

// Explicit refuses these nondeterministic inits. Auto falls back and reports
// the symbolic init error; that error envelope carries no fallback metadata.
// These concrete refusals are not evidence of definedness or of detecting pop.
failure_cell!(
    nested_if_forall,
    NESTED_IF_FORALL,
    "pop() on empty sequence",
    "7:26",
    "init references state variable 'c' before it is assigned"
);
failure_cell!(
    struct_field_bounded,
    STRUCT_FIELD_BOUNDED,
    "pop() on empty sequence",
    "6:32",
    "init does not assign state variable(s): c"
);
failure_cell!(
    option_bounded,
    OPTION_BOUNDED,
    "pop() on empty sequence",
    "5:30",
    "init references state variable 'c' before it is assigned"
);

const POP_UNBOUNDED: &str = "spec PopUnbounded {\n  type K = 0..1\n  state { xs: Seq<K, 2>, n: Int }\n  init { xs = Seq {}  n = xs.pop().size() }\n  action step() { }\n  invariant P { true }\n}\n";
const HEAD_BOUNDED: &str = "spec HeadBounded {\n  type K = 0..1\n  state { xs: Seq<K, 2>, n: 2..3 }\n  init { xs = Seq {}  n = xs.head() }\n  action step() { }\n  invariant P { true }\n}\n";
const AT_BOUNDED: &str = "spec AtBounded {\n  type K = 0..1\n  state { xs: Seq<K, 2>, n: 2..3 }\n  init { xs = Seq {}  n = xs.at(0) }\n  action step() { }\n  invariant P { true }\n}\n";

// Preservation cells: bounded head/at and unbounded pop already report the
// undefined operation symbolically. These are not old-query-order detectors.
failure_cell!(
    pop_unbounded_control,
    POP_UNBOUNDED,
    "pop() on empty sequence",
    "4:23"
);
failure_cell!(
    head_bounded,
    HEAD_BOUNDED,
    "head() on empty sequence",
    "4:23"
);
failure_cell!(at_bounded, AT_BOUNDED, "at() index out of range", "4:23");

const BOUND_ONLY: &str = "spec BoundOnly {\n  state { n: 0..1 }\n  init { n = 5 }\n  action step() { }\n  invariant P { true }\n}\n";
const FREE_INPUT: &str = "spec FreeInput {\n  state { k: 0..1, t: Int }\n  init { t = 10 / (k + 1) }\n  action step() { }\n  invariant P { true }\n}\n";

fn assert_bound(value: &Value, exit: i32, property: &str) {
    assert_eq!(exit, 1, "{value:#}");
    assert_eq!(value["result"], "violated", "{value:#}");
    assert_eq!(value["violation_kind"], "type_bound", "{value:#}");
    assert_eq!(value["invariant"], property, "{value:#}");
    assert_eq!(value["violated_at_step"], 0, "{value:#}");
    assert!(value["last_action"].is_null(), "{value:#}");
    for key in ["kind", "message"] {
        assert!(value.get(key).is_none(), "{key}: {value:#}");
    }
}

fn require_bound_only(engine: &str) {
    for _ in 0..2 {
        let (value, exit) = verify(BOUND_ONLY, engine);
        assert_bound(&value, exit, "_bounds_n");
        assert_eq!(value["trace"][0]["state"]["n"], 5, "{value:#}");
    }
}

fn require_free_input(engine: &str) {
    for _ in 0..2 {
        let (value, exit) = verify(FREE_INPUT, engine);
        // Check accepts this symbolic free root. Its type makes division and
        // addition defined; dropping free-input bounds must not invent an init
        // error. Search still finds the existing unconstrained _bounds_k witness.
        // Explicit cannot execute such a read; its existing refusal is NOT a
        // proof of definedness. Auto falls back and preserves the symbolic result.
        let refusal = "init references state variable 'k' before it is assigned";
        if engine == "explicit" {
            assert_error(&value, exit, refusal);
        } else {
            assert_bound(&value, exit, "_bounds_k");
            if engine == "auto" {
                assert_eq!(value["engine"], "bmc", "{value:#}");
                assert_eq!(
                    value["engine_fallback"],
                    serde_json::json!({
                        "from": "explicit", "kind": "unsupported", "reason": refusal
                    }),
                    "{value:#}"
                );
            }
        }
    }
}

macro_rules! control_cells {
    ($name:ident, $require:ident) => {
        mod $name {
            use super::*;
            #[test]
            fn bmc() {
                $require("bmc");
            }
            #[test]
            fn induction() {
                $require("induction");
            }
            #[test]
            fn explicit() {
                $require("explicit");
            }
            #[test]
            fn auto() {
                $require("auto");
            }
        }
    };
}

control_cells!(bound_only_control, require_bound_only);
control_cells!(free_input_control, require_free_input);

// Review F1: over-capacity stored sequences have unrepresented slots. Do not
// diagnose an invented division failure from those slots; refuse without a
// location instead. Keep the exact review probe layouts for fail-first evidence.
const SEQ_CAP_GUARDED2: &str = "spec SeqCapGuarded2 {\n  type K = 0..1\n  state { a: 0..1, k: Bool, xs: Seq<K, 2>, ys: Seq<K, 2>, t: Int }\n  init {\n    xs = Seq { 0, 0 }\n    if k { ys = xs.push(1) } else { ys = xs }\n    t = if ys.size() > 2 then 10 / ys.at(2) else 1\n  }\n  action step() { }\n  invariant P { true }\n}\n";
const SEQ_CAP_AT_ND: &str = "spec SeqCapAtNd {\n  type K = 0..1\n  state { k: K, xs: Seq<K, 2>, ys: Seq<K, 2>, t: Int }\n  init { xs = Seq { 0, 0 }  ys = xs.push(1)  t = 10 / ys.at(2) }\n  action step() { }\n  invariant P { true }\n}\n";
const SEQ_WITHIN_CAPACITY: &str = "spec SeqWithinCapacity {\n  type K = 0..1\n  state { xs: Seq<K, 2>, ys: Seq<K, 2>, t: Int }\n  init {\n    xs = Seq { 0 }\n    ys = xs.push(1)\n    t = 10 / (ys.at(1) - 1)\n  }\n  action step() { }\n  invariant P { true }\n}\n";

fn require_capacity_refusal(source: &str, engine: &str, explicit_refusal: &str) {
    let (value, exit) = verify(source, engine);
    assert_error(
        &value,
        exit,
        if engine == "explicit" {
            explicit_refusal
        } else {
            "model sequence length exceeds capacity"
        },
    );
    if engine == "auto" {
        // Auto falls back to symbolic verification, but an error envelope has
        // neither engine nor fallback metadata. Do not invent a verdict object.
        for key in ["engine", "engine_fallback"] {
            assert!(value.get(key).is_none(), "{key}: {value:#}");
        }
    }
}

fn require_seq_cap_guarded2(engine: &str) {
    require_capacity_refusal(
        SEQ_CAP_GUARDED2,
        engine,
        "init references state variable 'k' before it is assigned",
    );
}

fn require_seq_cap_at_nd(engine: &str) {
    require_capacity_refusal(
        SEQ_CAP_AT_ND,
        engine,
        "init does not assign state variable(s): k",
    );
}

control_cells!(seq_cap_guarded2, require_seq_cap_guarded2);
control_cells!(seq_cap_at_nd, require_seq_cap_at_nd);

// Preservation control: ys has two represented slots and ys.at(1) is exactly
// one, so this division really is undefined and must retain its init location.
failure_cell!(
    seq_within_capacity_control,
    SEQ_WITHIN_CAPACITY,
    "division by zero",
    "7:5"
);
