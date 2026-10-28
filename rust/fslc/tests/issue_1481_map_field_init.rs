// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1481 cell 1: field-wise init must agree across symbolic and concrete engines.
//!
//! Each consumer has its own test and literal expectation: a check failure must
//! not hide a concrete failure, nor may two equally wrong engines be an oracle.
//! A1 keeps the original issue body. A2–A5 additionally pin initialized
//! values in invariants (symbolic) and the entire replay state (Monitor).
//! N1–N6 preserve incomplete-init and overlapping-write refusals.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
enum Consumer {
    Check,
    Bmc,
    Induction,
    Explicit,
    Monitor,
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fslc-1481-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("create test directory");
        std::fs::write(root.join("model.fsl"), source).expect("write inline spec");
        // No observed initial state: replay must construct Monitor::new rather
        // than use #519's fallback to a supplied concrete state.
        std::fs::write(root.join("trace.json"), "[]").expect("write legacy trace");
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove own test directory");
    }
}

fn observe(source: &str, consumer: Consumer) -> (Value, i32) {
    let fixture = Fixture::new(source);
    let args: &[&str] = match consumer {
        Consumer::Check => &["check", "model.fsl"],
        Consumer::Bmc => &[
            "verify",
            "model.fsl",
            "--engine",
            "bmc",
            "--depth",
            "4",
            "--no-cache",
        ],
        Consumer::Induction => &[
            "verify",
            "model.fsl",
            "--engine",
            "induction",
            "--depth",
            "4",
            "--no-cache",
        ],
        Consumer::Explicit => &[
            "verify",
            "model.fsl",
            "--engine",
            "explicit",
            "--depth",
            "4",
            "--no-cache",
        ],
        Consumer::Monitor => &["replay", "model.fsl", "--trace", "trace.json"],
    };
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(&fixture.0)
        .env_remove("FSLC_CACHE")
        .output()
        .expect("run working-tree native fslc");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{consumer:?}: invalid JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(
        output.stderr.is_empty(),
        "{consumer:?}: stderr={}; output={value:#}",
        String::from_utf8_lossy(&output.stderr)
    );
    let exit = output.status.code().expect("native exit code");
    // Preserve complete raw observations in --nocapture calibration logs.
    println!("{consumer:?} exit={exit} output={value}");
    (value, exit)
}

fn clean(result: &str) -> Value {
    json!({
        "exit": 0, "result": result, "kind": null, "message": null,
        "violation_kind": null, "invariant": null, "action": null,
        "violation": null, "last_action": null, "loc": null
    })
}

fn bounds() -> Value {
    json!({
        "exit": 1, "result": "violated", "kind": null, "message": null,
        "violation_kind": "type_bound", "invariant": "_bounds_m", "action": null,
        "violation": null, "last_action": null, "loc": null
    })
}

fn semantics(message: &str) -> Value {
    json!({
        "exit": 2, "result": "error", "kind": "semantics", "message": message,
        "violation_kind": null, "invariant": null, "action": null,
        "violation": null, "last_action": null, "loc": null
    })
}

fn semantics_at(message: &str, line: u64, column: u64) -> Value {
    let mut expected = semantics(message);
    expected["loc"] = json!({"line": line, "column": column});
    expected
}

fn without_versions(mut actual: Value, consumer: Consumer) -> Value {
    // Observed check/verify envelopes include versions, but legacy replay
    // does not. Z3's version is installation-dependent, not an init result.
    // A live shape check prevents silently dropping a missing provenance key.
    if !matches!(consumer, Consumer::Monitor) {
        let versions = actual
            .as_object_mut()
            .unwrap()
            .remove("versions")
            .expect("versions");
        assert_eq!(
            versions["verifier"],
            json!({"name": "fslc-rust", "version": env!("CARGO_PKG_VERSION")})
        );
        assert_eq!(
            versions["core"],
            json!({"name": "fsl-core", "version": env!("CARGO_PKG_VERSION")})
        );
        assert_eq!(versions["solver"]["name"], "z3");
        assert_eq!(versions["solver"]["backend"], "native-z3");
        assert!(
            versions["solver"]["version"]
                .as_str()
                .is_some_and(|version| !version.is_empty())
        );
    }
    actual
}

fn require(source: &str, consumer: Consumer, expected: &Value, initial: Option<Value>) {
    let (actual, exit) = observe(source, consumer);
    // This is the verdict contract, not equality of complete engine reports:
    // proof/closure, cost, coverage and witness metadata have distinct engine
    // meanings. Keep every requested verdict/name/error observable, including
    // absence of a contradictory violation on a purportedly clean result.
    let verdict = json!({
        "exit": exit, "result": actual["result"], "kind": actual["kind"],
        "message": actual["message"], "violation_kind": actual["violation_kind"],
        "invariant": actual["invariant"], "action": actual["action"],
        "violation": actual["violation"], "last_action": actual["last_action"],
        "loc": actual["loc"]
    });
    assert_eq!(verdict, *expected, "{consumer:?}: full output={actual:#}");
    if expected["result"] == "error" {
        let mut envelope = json!({
            "fsl": "1.0", "result": "error", "kind": "semantics",
            "message": expected["message"]
        });
        if !expected["loc"].is_null() {
            envelope["loc"] = expected["loc"].clone();
        }
        assert_eq!(
            without_versions(actual.clone(), consumer),
            envelope,
            "{consumer:?}: complete error envelope"
        );
    } else if matches!(consumer, Consumer::Check) {
        assert_eq!(
            without_versions(actual.clone(), consumer),
            json!({"fsl": "1.0", "result": "ok", "spec": "MapFieldInit", "warnings": []}),
            "{consumer:?}: complete check envelope"
        );
    } else if expected["exit"] == 0 && !matches!(consumer, Consumer::Monitor) {
        let invariant = if source == A1 { "P" } else { "Initialized" };
        assert_eq!(
            actual["invariants_checked"],
            match source {
                A2 | A5 | P2 | P4 | B1_TRUE | B1_FALSE | B2 | K1 | K2 | K3 | O2 => {
                    json!(["_bounds_m", invariant])
                }
                A4 | P1 | O1 => json!(["_bounds_s", invariant]),
                _ => json!([invariant]),
            },
            "{consumer:?}: {actual:#}"
        );
        let completeness = if matches!(consumer, Consumer::Bmc) {
            "bounded"
        } else {
            "unbounded"
        };
        assert_eq!(
            actual["completeness"], completeness,
            "{consumer:?}: {actual:#}"
        );
        if matches!(consumer, Consumer::Explicit) {
            assert_eq!(actual["closure"], true, "{consumer:?}: {actual:#}");
        }
    }
    if let Some(initial) = initial {
        assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "result": "conformant", "spec": "MapFieldInit",
                "steps_checked": 0, "final_state": initial,
                "note": "leadsTo properties are not checked by replay (finite logs only)"
            }),
            "{consumer:?}: complete Monitor init envelope, no exclusions"
        );
    }
}

// Five independently reported tests per cell. In particular, base calibration
// still runs BMC/induction/explicit/Monitor when the cell's check test is red.
macro_rules! cell {
    ($name:ident, $source:expr, $check:expr, $bmc:expr, $induction:expr,
     $explicit:expr, $monitor:expr, $initial:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn check() {
                require($source, Consumer::Check, &$check, None);
            }
            #[test]
            fn bmc() {
                require($source, Consumer::Bmc, &$bmc, None);
            }
            #[test]
            fn induction() {
                require($source, Consumer::Induction, &$induction, None);
            }
            #[test]
            fn explicit() {
                require($source, Consumer::Explicit, &$explicit, None);
            }
            #[test]
            fn monitor() {
                require($source, Consumer::Monitor, &$monitor, $initial);
            }
        }
    };
}

const A1: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool }
  state { m: Map<K, Cell>, done: Bool }
  init { forall k: K { m[k].flag = false } done = m[0].flag }
  action go() { done = true }
  invariant P { done == false or done == true }
}
";

cell!(
    a1_issue_single_field_forall,
    A1,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"0": {"flag": false}, "1": {"flag": false}}, "done": false}))
);

const A2: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell> }
  init { forall k: K { m[k].flag = true m[k].n = 2 } }
  action idle() { }
  invariant Initialized { forall k: K { m[k].flag == true and m[k].n == 2 } }
}
";

cell!(
    a2_two_fields_forall,
    A2,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"0": {"flag": true, "n": 2}, "1": {"flag": true, "n": 2}}}))
);

const A3: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool }
  state { m: Map<K, Cell> }
  init { m[0].flag = false m[1].flag = true }
  action idle() { }
  invariant Initialized { m[0].flag == false and m[1].flag == true }
}
";

cell!(
    a3_distinct_concrete_keys,
    A3,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"0": {"flag": false}, "1": {"flag": true}}}))
);

const A4: &str = r"spec MapFieldInit {
  struct Pair { a: Bool, b: 0..2 }
  state { s: Pair }
  init { s.a = true s.b = 2 }
  action idle() { }
  invariant Initialized { s.a == true and s.b == 2 }
}
";

cell!(
    a4_struct_root_fields,
    A4,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"s": {"a": true, "b": 2}}))
);

const A5: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell>, done: Bool }
  init {
    forall k: K { m[k].flag = true m[k].n = 1 }
    done = m[0].flag
  }
  action idle() { }
  invariant Initialized {
    done == true and m[0].flag == true and m[1].flag == true and
    m[0].n == 1 and m[1].n == 1
  }
}
";

cell!(
    a5_read_after_field_writes,
    A5,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"0": {"flag": true, "n": 1}, "1": {"flag": true, "n": 1}}, "done": true}))
);

const N1: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell>, done: Bool }
  init { forall k: K { m[k].flag = false } done = false }
  action go() { done = true }
  invariant P { done == false or done == true }
}
";

cell!(
    n1_partial_coverage_without_read,
    N1,
    clean("ok"),
    bounds(),
    bounds(),
    // Field writes count as coverage, so the existing partial-component diagnostic applies.
    semantics(
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    ),
    semantics(
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    ),
    None
);

const N2: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell>, done: Bool }
  init { forall k: K { m[k].flag = false } done = m[0].n == 0 }
  action go() { done = true }
  invariant P { done == false or done == true }
}
";

const N3: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool }
  state { m: Map<K, Cell> }
  init { forall k: K { m[k].flag = false m[k].flag = true } }
  action idle() { }
}
";

cell!(
    n3_duplicate_key_field,
    N3,
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        5,
        42
    ),
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        5,
        42
    ),
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        5,
        42
    ),
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        5,
        42
    ),
    semantics("state variable 'm' assigned more than once in init forall"),
    None
);

const N4: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool }
  state { m: Map<K, Cell> }
  init { forall k: K { m[k] = Cell { flag: false } m[k].flag = true } }
  action idle() { }
}
";

cell!(
    n4_entry_and_field_overlap,
    N4,
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        5,
        52
    ),
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        5,
        52
    ),
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        5,
        52
    ),
    semantics_at(
        "state variable 'm' assigned more than once in init forall",
        5,
        52
    ),
    semantics("state variable 'm' assigned more than once in init forall"),
    None
);

const N5: &str = r"spec MapFieldInit {
  struct Pair { a: Bool, b: 0..2 }
  state { s: Pair }
  init { s = Pair { a: false, b: 0 } s.a = true }
  action idle() { }
}
";

cell!(
    n5_root_and_field_overlap,
    N5,
    semantics_at("state variable 's' assigned more than once in init", 4, 38),
    semantics_at("state variable 's' assigned more than once in init", 4, 38),
    semantics_at("state variable 's' assigned more than once in init", 4, 38),
    semantics_at("state variable 's' assigned more than once in init", 4, 38),
    semantics("state variable 's' assigned more than once in init"),
    None
);

// Even though i was assigned 0, ownership must not constant-propagate a state
// key. An unresolved field write owns the entire map and overlaps the later
// concrete write, including a different field on a different apparent key.
const N6: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { i: K, m: Map<K, Cell> }
  init { i = 0 m[i].flag = true m[1].n = 2 }
  action idle() { }
}
";

cell!(
    n6_dynamic_key_fails_closed,
    N6,
    semantics_at("state variable 'm' assigned more than once in init", 5, 33),
    semantics_at("state variable 'm' assigned more than once in init", 5, 33),
    semantics_at("state variable 'm' assigned more than once in init", 5, 33),
    semantics_at("state variable 'm' assigned more than once in init", 5, 33),
    semantics("state variable 'm' assigned more than once in init"),
    None
);

// Approval (a): a read of a field-initialized root requires the WHOLE root,
// including every map key, to be covered. Keep both sides of each boundary.
const P1: &str = r"spec MapFieldInit {
  struct Pair { a: 0..2, b: 0..2 }
  state { s: Pair, done: Bool }
  init {
    s.a = 1
    s.b = 2
    done = (s.a == 1)
  }
  action idle() { }
  invariant Initialized { s.a == 1 and s.b == 2 and done }
}
";

cell!(
    p1_root_field_read_after_full_coverage,
    P1,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"s": {"a": 1, "b": 2}, "done": true}))
);

const R1: &str = r"spec MapFieldInit {
  struct Pair { a: 0..2, b: Bool }
  state { s: Pair }
  init {
    s.a = 1
    s.b = (s.a == 1)
  }
  action idle() { }
}
";

// T1 detector: the approved diagnostic is an independent literal, never copied
// from the observation. This also pins whole-entry/root markers and punctuation.
fn require_read_rejection(source: &str, consumer: Consumer, message: &str, line: u64, column: u64) {
    let (actual, exit) = observe(source, consumer);
    let mut expected = json!({
        "fsl": "1.0", "result": "error", "kind": "semantics", "message": message
    });
    if !matches!(consumer, Consumer::Monitor) {
        expected["loc"] = json!({"line": line, "column": column});
    }
    assert_eq!(exit, 2, "{consumer:?}: {actual:#}");
    assert_eq!(
        without_versions(actual, consumer),
        expected,
        "{consumer:?}: complete read rejection envelope, statement span"
    );
}

macro_rules! read_rejection_cell {
    ($name:ident, $source:expr, $message:expr, $line:expr, $column:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn check() {
                require_read_rejection($source, Consumer::Check, $message, $line, $column);
            }
            #[test]
            fn bmc() {
                require_read_rejection($source, Consumer::Bmc, $message, $line, $column);
            }
            #[test]
            fn induction() {
                require_read_rejection($source, Consumer::Induction, $message, $line, $column);
            }
            #[test]
            fn explicit() {
                require_read_rejection($source, Consumer::Explicit, $message, $line, $column);
            }
            #[test]
            fn monitor() {
                require_read_rejection($source, Consumer::Monitor, $message, $line, $column);
            }
        }
    };
}

// #1451: a free component of a may-assigned root still requires full-root coverage.
read_rejection_cell!(
    n2_read_uncovered_field,
    N2,
    "init references state variable 'm' before it is assigned",
    5,
    44
);

read_rejection_cell!(
    r1_root_field_read_before_sibling,
    R1,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    6,
    5
);

const P2: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { f: 0..2, g: 0..2 }
  state { m: Map<K, Cell>, done: Bool }
  init {
    forall k: K { m[k].f = 1 m[k].g = 2 }
    done = (m[0].f == 1)
  }
  action idle() { }
  invariant Initialized { done and forall k: K { m[k].f == 1 and m[k].g == 2 } }
}
";

cell!(
    p2_map_field_read_outside_completed_forall,
    P2,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"0": {"f": 1, "g": 2}, "1": {"f": 1, "g": 2}}, "done": true}))
);

const R2: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { f: 0..2, g: 0..2 }
  state { m: Map<K, Cell>, seen: Map<K, K> }
  init {
    forall k: K {
      m[k].f = 1
      seen[k] = m[k].f
      m[k].g = 2
    }
  }
  action idle() { }
}
";

read_rejection_cell!(
    r2_binder_field_read_before_sibling,
    R2,
    "init cannot read 'm.f' before state variable 'm' is fully assigned",
    8,
    7
);

const P4: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { f: 0..2, g: 0..2 }
  state { m: Map<K, Cell>, seen: Map<K, Bool> }
  init {
    forall k: K {
      m[k].f = 1
      m[k].g = 2
      seen[k] = (m[k].f == 1)
    }
  }
  action idle() { }
  invariant Initialized { forall k: K { seen[k] and m[k].f == 1 and m[k].g == 2 } }
}
";

cell!(
    p4_current_binder_read_after_both_fields,
    P4,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(
        json!({"m": {"0": {"f": 1, "g": 2}, "1": {"f": 1, "g": 2}}, "seen": {"0": true, "1": true}})
    )
);

const R4: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { f: 0..2, g: 0..2 }
  state { m: Map<K, Cell>, x: 0..2 }
  init {
    m[0].f = 1
    m[0].g = 2
    x = m[0].f
    m[1].f = 1
    m[1].g = 2
  }
  action idle() { }
}
";

read_rejection_cell!(
    r4_complete_entry_is_not_complete_root,
    R4,
    "init cannot read 'm.f' before state variable 'm' is fully assigned",
    8,
    5
);

const P3: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { a: Bool, b: Bool }
  state { m: Map<K, Cell>, e: Cell }
  init {
    m[0].a = true
    m[0].b = false
    m[1].a = false
    m[1].b = true
    e = m[0]
  }
  action idle() { }
  invariant Initialized {
    m[0] == Cell { a: true, b: false } and
    m[1] == Cell { a: false, b: true } and e == Cell { a: true, b: false }
  }
}
";

cell!(
    p3_whole_entry_read_after_all_keys_fields,
    P3,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(
        json!({"m": {"0": {"a": true, "b": false}, "1": {"a": false, "b": true}}, "e": {"a": true, "b": false}})
    )
);

const R3: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { a: Bool, b: Bool }
  state { m: Map<K, Cell>, e: Cell }
  init {
    m[0].a = true
    m[0].b = false
    e = m[0]
    m[1].a = false
    m[1].b = true
  }
  action idle() { }
}
";

read_rejection_cell!(
    r3_whole_entry_read_before_other_key,
    R3,
    "init cannot read 'm.<whole entry>' before state variable 'm' is fully assigned",
    8,
    5
);

const P5: &str = r"spec MapFieldInit {
  struct Pair { a: Bool, b: Bool }
  state { s: Pair, t: Pair }
  init {
    s.a = true
    s.b = false
    t = s
  }
  action idle() { }
  invariant Initialized { s == Pair { a: true, b: false } and t == Pair { a: true, b: false } }
}
";

cell!(
    p5_whole_root_read_after_both_fields,
    P5,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"s": {"a": true, "b": false}, "t": {"a": true, "b": false}}))
);

const R5: &str = r"spec MapFieldInit {
  struct Pair { a: Bool, b: Bool }
  state { s: Pair, t: Pair }
  init {
    s.a = true
    t = s
    s.b = false
  }
  action idle() { }
}
";

read_rejection_cell!(
    r5_whole_root_read_before_sibling,
    R5,
    "init cannot read 's.<whole root>' before state variable 's' is fully assigned",
    6,
    5
);

const R6: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RangeFieldRead {
  type K = 0..1
  struct Pair { a: K, b: Bool }
  state { s: Pair, m: Map<K, Bool> }
  init {
    s.a = 1
    forall k in 0..s.a { m[k] = false }
    s.b = true
  }
  action idle() { }
}
";

// The field-read gate also checks expressions in forall range bounds.
read_rejection_cell!(
    r6_range_bound_field_read_before_sibling,
    R6,
    "init cannot read 's.a' before state variable 's' is fully assigned",
    8,
    5
);

// Both concrete branch outcomes are pinned, not just the taken true branch.
const B1_TRUE: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { c: Bool, m: Map<K, Cell>, done: Bool }
  init {
    c = true
    if c { m[0].flag = true m[1].n = 2 } else { m[0].flag = false m[1].n = 1 }
    m[0].n = 1
    m[1].flag = false
    done = m[0].flag
  }
  action idle() { }
  invariant Initialized {
    c and done and m[0].flag and m[0].n == 1 and not m[1].flag and m[1].n == 2
  }
}
";

cell!(
    b1_true_branch_nested_key_meet,
    B1_TRUE,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(
        json!({"c": true, "m": {"0": {"flag": true, "n": 1}, "1": {"flag": false, "n": 2}}, "done": true})
    )
);

const B1_FALSE: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { c: Bool, m: Map<K, Cell>, done: Bool }
  init {
    c = false
    if c { m[0].flag = true m[1].n = 2 } else { m[0].flag = false m[1].n = 1 }
    m[0].n = 1
    m[1].flag = false
    done = m[0].flag
  }
  action idle() { }
  invariant Initialized {
    not c and not done and not m[0].flag and m[0].n == 1 and not m[1].flag and m[1].n == 1
  }
}
";

cell!(
    b1_false_branch_nested_key_meet,
    B1_FALSE,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(
        json!({"c": false, "m": {"0": {"flag": false, "n": 1}, "1": {"flag": false, "n": 1}}, "done": false})
    )
);

// Plan (iii) detector: choosing the complete then branch must not make the
// static meet full. Newly admitted multi-field partial init stays rejected by
// main's ownership diagnostic, including its original write-statement span.
const B2: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { c: Bool, m: Map<K, Cell> }
  init {
    c = true
    if c { m[0].flag = true m[0].n = 2 } else { m[0].flag = false }
    m[1] = Cell { flag: false, n: 1 }
  }
  action idle() { }
  invariant Initialized { c and m[0].flag and m[0].n == 2 and not m[1].flag and m[1].n == 1 }
}
";

cell!(
    b2_partial_else_must_not_inherit_then_coverage,
    B2,
    semantics_at("state variable 'm' assigned more than once in init", 7, 29),
    semantics_at("state variable 'm' assigned more than once in init", 7, 29),
    semantics_at("state variable 'm' assigned more than once in init", 7, 29),
    semantics_at("state variable 'm' assigned more than once in init", 7, 29),
    semantics("state variable 'm' assigned more than once in init"),
    None
);

// Uniform ownership refusals still have independently executed consumer tests.
macro_rules! ownership_cell {
    ($name:ident, $source:expr, $message:expr, $line:expr, $column:expr) => {
        cell!(
            $name,
            $source,
            semantics_at($message, $line, $column),
            semantics_at($message, $line, $column),
            semantics_at($message, $line, $column),
            semantics_at($message, $line, $column),
            semantics($message),
            None
        );
    };
}

const D1: &str = r"spec MapFieldInit {
  struct Pair { a: Bool, b: Bool }
  state { s: Pair }
  init {
    s.a = true
    s = Pair { a: true, b: false }
  }
  action idle() { }
}
";
ownership_cell!(
    d1_field_then_whole_root,
    D1,
    "state variable 's' assigned more than once in init",
    6,
    5
);

const D2: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool }
  state { m: Map<K, Cell> }
  init {
    forall k: K {
      m[k].flag = true
      m[k] = Cell { flag: true }
    }
  }
  action idle() { }
}
";
ownership_cell!(
    d2_field_then_whole_entry,
    D2,
    "state variable 'm' assigned more than once in init forall",
    8,
    7
);

const D3: &str = r"spec MapFieldInit {
  struct Pair { a: Bool }
  state { s: Pair }
  init {
    s.a = true
    s.a = true
  }
  action idle() { }
}
";
ownership_cell!(
    d3_duplicate_root_field_even_same_value,
    D3,
    "state variable 's' assigned more than once in init",
    6,
    5
);

const D4: &str = r"spec MapFieldInit {
  const Z = 0
  type K = 0..1
  struct Cell { flag: Bool }
  state { m: Map<K, Cell> }
  init {
    m[Z].flag = true
    m[0].flag = true
    m[1].flag = false
  }
  action idle() { }
}
";
ownership_cell!(
    d4_const_key_alias_duplicate,
    D4,
    "state variable 'm' assigned more than once in init",
    8,
    5
);

const D5: &str = r"spec MapFieldInit {
  enum Color { Red, Green }
  struct Cell { flag: Bool }
  state { m: Map<Color, Cell> }
  init {
    forall c: Color { m[c].flag = true }
    m[Red].flag = true
  }
  action idle() { }
}
";
ownership_cell!(
    d5_enum_forall_concrete_key_overlap,
    D5,
    "state variable 'm' assigned more than once in init",
    7,
    5
);

const D6: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { c: Bool, m: Map<K, Cell> }
  init {
    c = true
    if c { m[0].flag = true } else { m[0].n = 1 }
    m[0].n = 2
    m[1] = Cell { flag: false, n: 0 }
  }
  action idle() { }
}
";
ownership_cell!(
    d6_branch_may_write_union_overlaps_later_field,
    D6,
    "state variable 'm' assigned more than once in init",
    8,
    5
);

const K1: &str = r"spec MapFieldInit {
  enum Color { Red, Green }
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<Color, Cell> }
  init { m[Red].flag = true m[Green].flag = false forall c: Color { m[c].n = 2 } }
  action idle() { }
  invariant Initialized { m[Red].flag and not m[Green].flag and m[Red].n == 2 and m[Green].n == 2 }
}
";
cell!(
    k1_enum_keys_and_forall_fields,
    K1,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"Red": {"flag": true, "n": 2}, "Green": {"flag": false, "n": 2}}}))
);

const K2: &str = r"spec MapFieldInit {
  const Z = 0
  const O = 1
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell> }
  init { m[Z].flag = true m[O].flag = false m[0].n = 1 m[O].n = 2 }
  action idle() { }
  invariant Initialized { m[0].flag and not m[1].flag and m[0].n == 1 and m[1].n == 2 }
}
";
cell!(
    k2_const_keys_and_literal_aliases,
    K2,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"0": {"flag": true, "n": 1}, "1": {"flag": false, "n": 2}}}))
);

const K3: &str = r"spec MapFieldInit {
  type K = 0..2
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell> }
  init { forall k in 0..1 { m[k].flag = true m[k].n = k } m[2] = Cell { flag: false, n: 2 } }
  action idle() { }
  invariant Initialized {
    m[0].n == 0 and m[1].n == 1 and m[2].n == 2 and m[0].flag and m[1].flag and not m[2].flag
  }
}
";
cell!(
    k3_subset_forall_plus_remaining_whole_entry,
    K3,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(
        json!({"m": {"0": {"flag": true, "n": 0}, "1": {"flag": true, "n": 1}, "2": {"flag": false, "n": 2}}})
    )
);

// F4: optional scalar fields were already legal types. Both some and none
// participate in field-wise init; none is not a substitute for an assignment.
const O1: &str = r"spec MapFieldInit {
  type K = 0..2
  struct Pair { a: Option<K>, b: Option<K> }
  state { s: Pair }
  init { s.a = some(2) s.b = none }
  action idle() { }
  invariant Initialized { s.a == some(2) and s.b == none }
}
";
cell!(
    o1_option_root_fields_some_and_none,
    O1,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"s": {"a": 2, "b": null}}))
);

const O2: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { a: Option<K>, b: Option<K> }
  state { m: Map<K, Cell> }
  init { forall k: K { m[k].a = some(k) m[k].b = none } }
  action idle() { }
  invariant Initialized { forall k: K { m[k].a == some(k) and m[k].b == none } }
}
";
cell!(
    o2_option_map_fields_some_and_none,
    O2,
    clean("ok"),
    clean("verified"),
    clean("proved"),
    clean("proved"),
    clean("conformant"),
    Some(json!({"m": {"0": {"a": 0, "b": null}, "1": {"a": 1, "b": null}}}))
);

// #1518 preservation control: unlike R2's already-written binder key, a fixed
// key read inside this forall was rejected on the new base, before cell 1.
const F1518: &str = r"spec MapFieldInit {
  type K = 0..1
  struct Cell { f: 0..2, g: 0..2 }
  state { m: Map<K, Cell>, seen: Map<K, K> }
  init {
    forall k: K {
      m[k].f = 1
      seen[k] = m[0].f
      m[k].g = 2
    }
  }
  action idle() { }
}
";
ownership_cell!(
    f1518_fixed_key_read_remains_rejected,
    F1518,
    "init forall cannot read state variable 'm' except at an already assigned current binder key",
    8,
    7
);

// Reuse the exact #1463 inventory sources, including the rejecting go() action,
// rather than accidentally changing whole-entry init to field-wise init. Other
// inventory fields/assertion helpers are consumed by its own integration test.
#[allow(dead_code)]
#[path = "support/issue_1463_forms.rs"]
mod issue_1463_forms;

fn require_1463(consumer: Consumer, negative: bool) {
    let id = format!("R2/map-struct-field/{negative}");
    let case = issue_1463_forms::cases()
        .into_iter()
        .find(|case| case.id == id)
        .expect("original #1463 whole-entry preservation source");
    let (actual, exit) = observe(&case.source, consumer);
    if matches!(consumer, Consumer::Check) {
        assert_eq!(exit, 0, "{id} {consumer:?}: {actual:#}");
        assert_eq!(
            without_versions(actual, consumer),
            json!({"fsl": "1.0", "result": "ok", "spec": "Support", "warnings": []}),
            "{id} {consumer:?}: complete check envelope"
        );
    } else if matches!(consumer, Consumer::Monitor) {
        assert_eq!(exit, 0, "{id} {consumer:?}: {actual:#}");
        assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "result": "conformant", "spec": "Support",
                "steps_checked": 0,
                "final_state": {"m": {"0": {"flag": false}, "1": {"flag": false}}, "done": false},
                "note": "leadsTo properties are not checked by replay (finite logs only)"
            }),
            "{id} {consumer:?}: Monitor initial state, not the go() transition"
        );
    } else {
        let expected = match (consumer, negative) {
            (Consumer::Bmc, false) => {
                json!({"exit": 0, "result": "verified", "kind": null, "invariant": null})
            }
            (Consumer::Induction | Consumer::Explicit, false) => {
                json!({"exit": 0, "result": "proved", "kind": null, "invariant": null})
            }
            (Consumer::Bmc | Consumer::Induction | Consumer::Explicit, true) => {
                json!({"exit": 1, "result": "violated", "kind": null, "invariant": "Capture"})
            }
            _ => unreachable!("check and Monitor have their own envelope checks"),
        };
        assert_eq!(
            json!({"exit": exit, "result": actual["result"], "kind": actual["kind"], "invariant": actual["invariant"]}),
            expected,
            "{id} {consumer:?}: {actual:#}"
        );
        if negative {
            assert_eq!(
                actual["loc"],
                json!({"line": 9, "column": 3}),
                "{id} {consumer:?}: {actual:#}"
            );
            assert_eq!(
                actual["violated_at_step"], 1,
                "{id} {consumer:?}: {actual:#}"
            );
            assert_eq!(
                actual["trace"]
                    .as_array()
                    .expect("counterexample trace")
                    .last()
                    .unwrap()["state"],
                json!({"m": {"0": {"flag": false}, "1": {"flag": false}}, "done": true}),
                "{id} {consumer:?}: independent rejecting-state literal"
            );
        } else {
            assert_eq!(
                actual["invariants_checked"],
                json!(["Capture"]),
                "{id} {consumer:?}: {actual:#}"
            );
            assert_eq!(
                actual["completeness"],
                if matches!(consumer, Consumer::Bmc) {
                    "bounded"
                } else {
                    "unbounded"
                },
                "{id} {consumer:?}: {actual:#}"
            );
            if matches!(consumer, Consumer::Explicit) {
                assert_eq!(actual["closure"], true, "{id} {consumer:?}: {actual:#}");
            }
        }
    }
}

macro_rules! whole_entry_preservation_cell {
    ($name:ident, $negative:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn check() {
                require_1463(Consumer::Check, $negative);
            }
            #[test]
            fn bmc() {
                require_1463(Consumer::Bmc, $negative);
            }
            #[test]
            fn induction() {
                require_1463(Consumer::Induction, $negative);
            }
            #[test]
            fn explicit() {
                require_1463(Consumer::Explicit, $negative);
            }
            #[test]
            fn monitor() {
                require_1463(Consumer::Monitor, $negative);
            }
        }
    };
}

whole_entry_preservation_cell!(r2_1463_whole_entry_positive, false);
whole_entry_preservation_cell!(r2_1463_whole_entry_negative, true);

#[path = "support/issue_1481_partial_init.rs"]
mod partial_init;

#[path = "support/issue_1481_legacy_concrete.rs"]
mod legacy_concrete;
