// SPDX-License-Identifier: Apache-2.0

//! #1543: init evaluation diagnostics name the containing statement, not the
//! enclosing init/forall or the failing subexpression. Each engine has its own
//! cell and source-derived expectation; agreement is not an engine oracle.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn verify(source: &str, engine: &str) -> (Value, i32) {
    // Keep calibration fixtures in TMPDIR for inspection; no manual cleanup.
    let path = std::env::temp_dir().join(format!(
        "fsl-1543-{}-{}.fsl",
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

fn require_error(source: &str, engine: &str, stem: &str, location: &str) {
    let (value, exit) = verify(source, engine);
    assert_eq!(exit, 2, "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "semantics", "{value:#}");
    assert_eq!(
        value["message"],
        format!("{stem} in init at {location}"),
        "{value:#}"
    );
    assert!(
        value.get("loc").is_none(),
        "no structured location: {value:#}"
    );
}

macro_rules! failure_cell {
    ($name:ident, $source:expr, $stem:expr, $location:literal) => {
        failure_cell!($name, $source, $stem, $stem, $location);
    };
    ($name:ident, $source:expr, $symbolic:expr, $concrete:expr, $location:literal) => {
        mod $name {
            use super::*;
            #[test]
            fn bmc() {
                require_error(&$source, "bmc", $symbolic, $location);
            }
            #[test]
            fn induction() {
                require_error(&$source, "induction", $symbolic, $location);
            }
            #[test]
            fn explicit() {
                require_error(&$source, "explicit", $concrete, $location);
            }
            #[test]
            fn auto() {
                require_error(&$source, "auto", $concrete, $location);
            }
        }
    };
}

const MAP_FIELD_WRITE: &str = "spec RImpl {\n  type K = 0..1\n  struct One { v: Bool }\n  state { m: Map<K, One> }\n  init { forall k in 0..2 { m[k].v = true } }\n  action step() { }\n  invariant P { m[0].v and m[1].v }\n}\n";
const MAP_READ: &str = "spec MapRead {\n  type K = 0..1\n  state { m: Map<K, Bool>, b: Bool }\n  init { m[0] = true  m[1] = true  b = m[2] }\n  action step() { }\n  invariant P { true }\n}\n";
const HEAD_EMPTY: &str = "spec HeadEmpty {\n  type K = 0..1\n  state { xs: Seq<K, 2>, n: 0..1 }\n  init { xs = Seq {}  n = xs.head() }\n  action step() { }\n  invariant P { true }\n}\n";

failure_cell!(
    map_field_write,
    MAP_FIELD_WRITE,
    "map assignment index outside key domain",
    "5:29"
);
failure_cell!(
    map_read,
    MAP_READ,
    "map index outside finite key domain",
    "4:36"
);
failure_cell!(head_empty, HEAD_EMPTY, "head() on empty sequence", "4:23");

fn sequence(expression: &str) -> String {
    format!(
        "spec Sequence {{\n  type K = 0..1\n  state {{ xs: Seq<K, 2>, n: 0..1 }}\n  init {{ xs = Seq {{}}  n = {expression} }}\n  action step() {{ }}\n  invariant P {{ true }}\n}}\n"
    )
}

// Int keeps the totalized pop().size() result (-1) within the target's bounds;
// the separate bounded-target definedness discrepancy is recorded for follow-up.
failure_cell!(
    pop_empty,
    "spec PopEmpty {\n  type K = 0..1\n  state { xs: Seq<K, 2>, n: Int }\n  init { xs = Seq {}  n = xs.pop().size() }\n  action step() { }\n  invariant P { true }\n}\n",
    "pop() on empty sequence",
    "4:23"
);
failure_cell!(
    at_past_end,
    sequence("xs.at(0)"),
    "at() index out of range",
    "4:23"
);
failure_cell!(
    at_negative,
    sequence("xs.at(-1)"),
    "at() index out of range",
    "4:23"
);
failure_cell!(
    index_past_end,
    sequence("xs[0]"),
    "sequence index out of range",
    "4:23"
);
failure_cell!(
    index_negative,
    sequence("xs[-1]"),
    "sequence index out of range",
    "4:23"
);

fn arithmetic(a: &str, b: &str, expression: &str) -> String {
    format!(
        "spec Arithmetic {{\n  state {{ a: Int, b: Int, n: Int }}\n  init {{\n    a = {a}\n    b = {b}\n    n = {expression}\n  }}\n  action step() {{ }}\n  invariant P {{ true }}\n}}\n"
    )
}

const MAX: &str = "9223372036854775807";
const MIN: &str = "-9223372036854775807 - 1";
failure_cell!(
    division_zero,
    arithmetic("1", "0", "a / b"),
    "division by zero",
    "6:5"
);
failure_cell!(
    remainder_zero,
    arithmetic("1", "0", "a % b"),
    "remainder by zero",
    "6:5"
);
failure_cell!(
    addition_overflow,
    arithmetic(MAX, "1", "a + b"),
    "integer overflow in addition",
    "6:5"
);
failure_cell!(
    subtraction_overflow,
    arithmetic(MIN, "1", "a - b"),
    "integer overflow in subtraction",
    "6:5"
);
failure_cell!(
    multiplication_overflow,
    arithmetic(MAX, "2", "a * b"),
    "integer overflow in multiplication",
    "6:5"
);
failure_cell!(
    negation_overflow,
    arithmetic(MIN, "0", "-a"),
    "integer overflow in negation",
    "6:5"
);
failure_cell!(
    abs_overflow,
    arithmetic(MIN, "0", "abs(a)"),
    "integer overflow in abs",
    "6:5"
);

// #1607 also aligns symbolic binder diagnostics with the concrete operation;
// #1543's containing-statement locations remain unchanged.
failure_cell!(
    sum_overflow,
    arithmetic(MAX, "0", "sum(k in 0..1 of a)"),
    "integer overflow in sum",
    "integer overflow in sum",
    "6:5"
);
failure_cell!(
    sum_undefined_summand,
    arithmetic("0", "0", "sum(k in 0..1 of 1 / a)"),
    "division by zero",
    "division by zero",
    "6:5"
);
failure_cell!(
    count_undefined_filter,
    arithmetic("0", "0", "count(k in 0..1 where 1 / a > k)"),
    "division by zero",
    "division by zero",
    "6:5"
);
failure_cell!(
    quantified_body,
    "spec Quantified {\n  state { d: Int, ok: Bool }\n  init {\n    d = 0\n    ok = forall k in 0..1 { 1 / d > k }\n  }\n  action step() { }\n  invariant P { true }\n}\n",
    "division by zero",
    "division by zero",
    "5:5"
);

failure_cell!(
    map_whole_write,
    "spec WholeWrite {\n  type K = 0..1\n  state { m: Map<K, Bool> }\n  init {\n    forall k in 0..2 {\n      m[k] = true\n    }\n  }\n  action step() { }\n  invariant P { true }\n}\n",
    "map assignment index outside key domain",
    "6:7"
);
failure_cell!(
    forall_body,
    "spec ForallBody {\n  type K = 0..1\n  state { m: Map<K, Int> }\n  init {\n    forall k: K {\n      m[k] = 1 / k\n    }\n  }\n  action step() { }\n  invariant P { true }\n}\n",
    "division by zero",
    "6:7"
);
failure_cell!(
    second_assignment_same_line,
    "spec Second {\n  state { d: Int, n: Int }\n  init { d = 0  n = 1 / d }\n  action step() { }\n  invariant P { true }\n}\n",
    "division by zero",
    "3:17"
);
failure_cell!(
    if_condition,
    "spec Condition {\n  state { d: Int, n: Int }\n  init {\n    d = 0\n    if 1 / d > 0 { n = 1 } else { n = 0 }\n  }\n  action step() { }\n  invariant P { true }\n}\n",
    "division by zero",
    "5:5"
);
failure_cell!(
    if_branch,
    "spec RemIf {\n  type Small = 0..3\n  state { d: Small, x: Int }\n  init {\n    d = 0\n    if d == 0 {\n      x = 7 % d\n    } else {\n      x = 0\n    }\n  }\n  action tick() { requires d < 3  d = d + 1 }\n  invariant Ok { x >= 0 }\n}\n",
    "remainder by zero",
    "7:7"
);
failure_cell!(
    forall_source,
    "spec Source {\n  state { ready: Bool }\n  init {\n    ready = true\n    forall k in Seq { 1 / count(j in 0..0 where false) } { }\n  }\n  action step() { }\n  invariant P { ready }\n}\n",
    "division by zero",
    "5:5"
);
failure_cell!(
    forall_where,
    "spec Filter {\n  state { ready: Bool }\n  init {\n    ready = true\n    forall k in 0..1 where 1 / k > 0 { }\n  }\n  action step() { }\n  invariant P { ready }\n}\n",
    "division by zero",
    "5:5"
);

const UNSAT: &str = "spec Unsatisfiable {\n  type K = 0..1\n  state { n: Int }\n  init { forall k: K { n = k } }\n  action step() { }\n  invariant P { true }\n}\n";
const SUCCESS: &str = "spec Successful {\n  state { ready: Bool }\n  init { ready = true }\n  action step() { }\n  invariant P { ready }\n}\n";

fn require_controls(engine: &str) {
    let (value, exit) = verify(UNSAT, engine);
    assert_eq!(exit, 2, "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "vacuous", "{value:#}");
    assert_eq!(
        value["message"], "init constraints are unsatisfiable",
        "{value:#}"
    );
    assert!(value.get("loc").is_none(), "{value:#}");

    let (value, exit) = verify(SUCCESS, engine);
    assert_eq!(exit, 0, "{value:#}");
    assert_eq!(
        value["result"],
        if engine == "bmc" {
            "verified"
        } else {
            "proved"
        },
        "{value:#}"
    );
    for key in ["message", "kind", "loc"] {
        assert!(value.get(key).is_none(), "{key}: {value:#}");
    }
}

#[test]
fn controls_bmc() {
    require_controls("bmc");
}
#[test]
fn controls_induction() {
    require_controls("induction");
}
#[test]
fn controls_explicit() {
    require_controls("explicit");
}
#[test]
fn controls_auto() {
    require_controls("auto");
}
