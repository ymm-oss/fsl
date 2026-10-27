// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1518: init-forall reads must not depend on iteration order, and an
//! unexecuted body must not supply concrete definite-assignment coverage.
//! Each cell has five separately named CLI tests so one consumer's failure
//! cannot hide another's verdict. Expectations are literals, not values
//! obtained from a production coverage/evaluation helper.
//!
//! Empty-domain exits intentionally differ (LANGUAGE.md, deterministic init):
//! symbolic init leaves unwritten components free, whereas explicit/Monitor
//! require a deterministic initial state and must refuse with exit 2. A free
//! bounded integer admits a `type_bound` counterexample (exit 1); a free Bool
//! has no such bound violation, but can violate b == false (E5, exit 1).
//! A tautological Bool invariant could instead verify symbolically: there is
//! deliberately no blanket assertion that every empty init fails symbolically.
//!
//! Replay of [] calls Monitor initialization and exposes the *whole* initial
//! state, but does not evaluate invariants. Its conformant result is therefore
//! not an invariant proof. Positive invariants independently specify every
//! initial component and are checked by each verification engine.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf, String);

impl Fixture {
    fn new(source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fslc-1518-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create #1518 fixture directory");
        std::fs::write(root.join("model.fsl"), source).expect("write #1518 model");
        std::fs::write(root.join("empty.json"), "[]").expect("write empty Monitor trace");
        Self(root, source.lines().next().expect("spec header").to_owned())
    }

    fn observe(&self, consumer: &str) -> (Value, i32) {
        let args = match consumer {
            "check" => vec!["check", "model.fsl"],
            "Monitor" => vec!["replay", "model.fsl", "--trace", "empty.json"],
            engine => vec![
                "verify",
                "model.fsl",
                "--engine",
                engine,
                "--depth",
                "4",
                "--no-cache",
            ],
        };
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(&args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .output()
            .expect("run native fslc");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{consumer}: invalid JSON: {error}; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        let exit = output.status.code().expect("native exit code");
        eprintln!(
            "OBS cell={} consumer={consumer} exit={exit} output={value}",
            self.1
        );
        assert!(
            output.stderr.is_empty(),
            "{consumer}: unexpected stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (value, exit)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned #1518 fixture");
    }
}

// These are independently specified verdict contracts, not comparisons of
// entire engine envelopes (whose assurance/cost/completeness differ by engine).
// Keep absent violation/action/error fields explicit, rather than ignoring an
// unexpected violation attached to an otherwise successful result.
fn assert_verdict(
    consumer: &str,
    actual: &Value,
    exit: i32,
    expected_exit: i32,
    result: &str,
    violation: Option<(&str, &str)>,
) {
    assert_eq!(exit, expected_exit, "{consumer}: {actual:#}");
    let (kind, name) = violation.map_or((Value::Null, Value::Null), |(kind, name)| {
        (json!(kind), json!(name))
    });
    assert_eq!(
        json!({
            "result": actual["result"],
            "violation_kind": actual["violation_kind"],
            "invariant": actual["invariant"],
            "action": actual["action"],
            "last_action": actual["last_action"],
            "kind": actual["kind"],
            "message": actual["message"],
            "violation": actual["violation"],
        }),
        json!({
            "result": result, "violation_kind": kind, "invariant": name,
            "action": null, "last_action": null,
            "kind": null, "message": null, "violation": null,
        }),
        "{consumer}: verdict/name/error contract: {actual:#}"
    );
}

fn assert_read_refused(source: &str, consumer: &str, line: usize, column: usize) {
    let (actual, exit) = Fixture::new(source).observe(consumer);
    assert_eq!(exit, 2, "{consumer}: {actual:#}");
    assert_eq!(actual["result"], "error", "{consumer}: {actual:#}");
    assert_eq!(actual["kind"], "semantics", "{consumer}: {actual:#}");
    if consumer == "Monitor" {
        // Replay stringifies Monitor errors, so its existing envelope has no loc.
        assert!(actual.get("loc").is_none(), "{consumer}: {actual:#}");
    } else {
        assert_eq!(
            actual["loc"],
            json!({"line": line, "column": column}),
            "{consumer}: locate the statement containing the read: {actual:#}"
        );
    }
    // The implementation chooses the wording, but not the error class or
    // location. Every rejected cell reads the root named m.
    let message = actual["message"].as_str().expect("semantics message");
    assert!(
        message.contains("read")
            && message
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == "m"),
        "{consumer}: expected a read diagnostic naming m: {actual:#}"
    );
    for field in [
        "violation_kind",
        "invariant",
        "action",
        "last_action",
        "violation",
    ] {
        assert!(actual.get(field).is_none(), "{consumer}: {actual:#}");
    }
}

fn assert_clean(source: &str, consumer: &str, result: &str, state: Option<Value>) {
    let (actual, exit) = Fixture::new(source).observe(consumer);
    assert_verdict(consumer, &actual, exit, 0, result, None);
    if let Some(state) = state {
        assert_eq!(actual["final_state"], state, "Monitor: whole initial state");
        assert_eq!(
            actual["steps_checked"], 0,
            "Monitor: no action was replayed"
        );
    }
}

fn assert_symbolic_free(source: &str, consumer: &str, kind: &str, name: &str) {
    let (actual, exit) = Fixture::new(source).observe(consumer);
    assert_verdict(consumer, &actual, exit, 1, "violated", Some((kind, name)));
}

fn assert_unassigned(source: &str, consumer: &str, root: &str, partial: bool) {
    let (actual, exit) = Fixture::new(source).observe(consumer);
    assert_eq!(exit, 2, "{consumer}: {actual:#}");
    assert_eq!(actual["result"], "error", "{consumer}: {actual:#}");
    assert_eq!(actual["kind"], "semantics", "{consumer}: {actual:#}");
    let suffix = if partial {
        " (partial component initialization is rejected by the explicit engine)"
    } else {
        ""
    };
    assert_eq!(
        actual["message"],
        format!("init does not assign state variable(s): {root}{suffix}"),
        "{consumer}: refuse missing assignment, not an invariant violation: {actual:#}"
    );
    for field in [
        "violation_kind",
        "invariant",
        "action",
        "last_action",
        "violation",
        "final_state",
    ] {
        assert!(actual.get(field).is_none(), "{consumer}: {actual:#}");
    }
}

macro_rules! read_cell {
    ($id:ident, $source:expr, $line:expr, $column:expr) => {
        mod $id {
            use super::*;
            const SOURCE: &str = $source;
            #[test]
            fn check() {
                assert_read_refused(SOURCE, "check", $line, $column);
            }
            #[test]
            fn bmc() {
                assert_read_refused(SOURCE, "bmc", $line, $column);
            }
            #[test]
            fn induction() {
                assert_read_refused(SOURCE, "induction", $line, $column);
            }
            #[test]
            fn explicit() {
                assert_read_refused(SOURCE, "explicit", $line, $column);
            }
            #[test]
            fn monitor() {
                assert_read_refused(SOURCE, "Monitor", $line, $column);
            }
        }
    };
}

macro_rules! positive_cell {
    ($id:ident, $source:expr, $state:expr) => {
        mod $id {
            use super::*;
            const SOURCE: &str = $source;
            #[test]
            fn check() {
                assert_clean(SOURCE, "check", "ok", None);
            }
            #[test]
            fn bmc() {
                assert_clean(SOURCE, "bmc", "verified", None);
            }
            #[test]
            fn induction() {
                assert_clean(SOURCE, "induction", "proved", None);
            }
            #[test]
            fn explicit() {
                assert_clean(SOURCE, "explicit", "proved", None);
            }
            #[test]
            fn monitor() {
                assert_clean(SOURCE, "Monitor", "conformant", Some($state));
            }
        }
    };
}

macro_rules! empty_cell {
    ($id:ident, $source:expr, $kind:expr, $name:expr, $root:expr, $partial:expr) => {
        mod $id {
            use super::*;
            const SOURCE: &str = $source;
            #[test]
            fn check() {
                assert_clean(SOURCE, "check", "ok", None);
            }
            #[test]
            fn bmc() {
                assert_symbolic_free(SOURCE, "bmc", $kind, $name);
            }
            #[test]
            fn induction() {
                assert_symbolic_free(SOURCE, "induction", $kind, $name);
            }
            #[test]
            fn explicit() {
                assert_unassigned(SOURCE, "explicit", $root, $partial);
            }
            #[test]
            fn monitor() {
                assert_unassigned(SOURCE, "Monitor", $root, $partial);
            }
        }
    };
}

read_cell!(
    a1_scalar_other_key,
    r"spec A1 {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init {
    forall k: K {
      m[k] = true
      seen[k] = m[1]
    }
  }
  action idle() { }
  invariant Initialized { seen[0] and seen[1] }
}
",
    7,
    7
);

read_cell!(
    a2_whole_entry_other_key,
    r"spec A2 {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell>, seen: Map<K, Bool> }
  init {
    forall k: K {
      m[k] = Cell { flag: true, n: 2 }
      seen[k] = m[1].flag
    }
  }
  action idle() { }
  invariant Initialized { seen[0] and seen[1] }
}
",
    8,
    7
);

read_cell!(
    a3_current_key_before_write,
    r"spec A3 {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init {
    forall k: K {
      seen[k] = m[k]
      m[k] = true
    }
  }
  action idle() { }
  invariant Initialized { seen[0] and seen[1] }
}
",
    6,
    7
);

read_cell!(
    a4_where_reads_written_root,
    r"spec A4 {
  type K = 0..1
  state { m: Map<K, Bool> }
  init {
    forall k: K where m[k] {
      m[k] = true
    }
  }
  action idle() { }
  invariant Initialized { m[0] and m[1] }
}
",
    5,
    5
);

read_cell!(
    a5_if_reads_other_key,
    r"spec A5 {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init {
    forall k: K {
      m[k] = true
      if m[1] {
        seen[k] = true
      } else {
        seen[k] = false
      }
    }
  }
  action idle() { }
  invariant Initialized { seen[0] and seen[1] }
}
",
    7,
    7
);

positive_cell!(
    p1_current_key_after_write,
    r"spec P1 {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init { forall k: K { m[k] = true seen[k] = m[k] } }
  action idle() { }
  invariant Initialized { m[0] and m[1] and seen[0] and seen[1] }
}
",
    json!({"m": {"0": true, "1": true}, "seen": {"0": true, "1": true}})
);

// S9 intentionally refuses concrete init conservatively, not with an incorrect
// verdict: a state-dependent filter is not statically provably nonempty, so it
// contributes no definite coverage even after flag = true (approved option E).
mod s9_state_dependent_nonempty_filter {
    use super::*;
    const SOURCE: &str = r"spec ST {
  type K = 0..1
  state { flag: Bool, x: 0..3 }
  init { flag = true  forall k: K where flag and k == 0 { x = 1 } }
  action idle() { }
  invariant P { x == 1 }
}
";
    #[test]
    fn check() {
        assert_clean(SOURCE, "check", "ok", None);
    }
    #[test]
    fn bmc() {
        assert_clean(SOURCE, "bmc", "verified", None);
    }
    #[test]
    fn induction() {
        assert_clean(SOURCE, "induction", "proved", None);
    }
    #[test]
    fn explicit() {
        assert_unassigned(SOURCE, "explicit", "x", false);
    }
    #[test]
    fn monitor() {
        assert_unassigned(SOURCE, "Monitor", "x", false);
    }
}

// D9 is an intentional conservative removal under the per-root rule, despite
// m[2] being initialized before this forall and disjoint from its writes.
read_cell!(
    d9_preceding_disjoint_entry,
    r"spec DISJ {
  type K = 0..2
  type J = 0..1
  state { m: Map<K, Bool>, seen: Map<J, Bool> }
  init {
    m[2] = false
    forall i in 0..1 { m[i] = true  seen[i] = m[2] }
  }
  action idle() { }
  invariant P { seen[0] == false and seen[1] == false }
}
",
    7,
    37
);

// S10 is an intentional conservative removal under the per-root rule: even a
// singleton's prior scalar write is not the already-written current-key case.
mod s10_singleton_scalar_read {
    use super::*;
    const SOURCE: &str = r"spec SINGLE {
  type K = 0..1
  state { x: 0..3, y: 0..3 }
  init {
    forall k: K where k == 0 { x = 1  y = x + 1 }
  }
  action idle() { }
  invariant P { x == 1 and y == 2 }
}
";

    fn assert_refused(consumer: &str) {
        let (actual, exit) = Fixture::new(SOURCE).observe(consumer);
        assert_eq!(exit, 2, "{consumer}: {actual:#}");
        assert_eq!(actual["result"], "error", "{consumer}: {actual:#}");
        assert_eq!(actual["kind"], "semantics", "{consumer}: {actual:#}");
        if consumer == "Monitor" {
            assert!(actual.get("loc").is_none(), "{consumer}: {actual:#}");
        } else {
            assert_eq!(
                actual["loc"],
                json!({"line": 5, "column": 39}),
                "{consumer}: locate the y assignment containing the x read: {actual:#}"
            );
        }
        let message = actual["message"].as_str().expect("semantics message");
        assert!(
            message.contains("read")
                && message
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .any(|word| word == "x"),
            "{consumer}: expected a read diagnostic naming x: {actual:#}"
        );
        for field in [
            "violation_kind",
            "invariant",
            "action",
            "last_action",
            "violation",
        ] {
            assert!(actual.get(field).is_none(), "{consumer}: {actual:#}");
        }
    }

    #[test]
    fn check() {
        assert_refused("check");
    }
    #[test]
    fn bmc() {
        assert_refused("bmc");
    }
    #[test]
    fn induction() {
        assert_refused("induction");
    }
    #[test]
    fn explicit() {
        assert_refused("explicit");
    }
    #[test]
    fn monitor() {
        assert_refused("Monitor");
    }
}

// RF1 forces the symbolic refinement path with unassigned y; it must still run
// the same init-read gate instead of silently accepting an order-sensitive init.
#[test]
fn rf1_refine_rejects_init_read_order() {
    let fixture = Fixture::new(
        r"spec RImpl {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool>, x: 0..3, y: Bool }
  init { x = 0  forall k: K { m[k] = true  seen[k] = m[1] } }
  action idle() { }
}
",
    );
    std::fs::write(
        fixture.0.join("abs.fsl"),
        r"spec RAbs {
  state { a: Bool }
  init { a = false }
  action idle() { }
}
",
    )
    .expect("write RF1 abstraction");
    std::fs::write(
        fixture.0.join("mapping.fsl"),
        r"refinement RImplRefinesRAbs {
  impl RImpl
  abs RAbs
  map a = seen[0]
  action idle() -> idle()
}
",
    )
    .expect("write RF1 mapping");
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
        .expect("run native RF1 refine");
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "refine: invalid JSON: {error}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let exit = output.status.code().expect("native refine exit code");
    eprintln!("OBS cell=RF1 consumer=refine exit={exit} output={actual}");
    assert!(output.stderr.is_empty(), "refine: {output:?}");
    // Refine stringifies the runtime error; its legacy envelope has no loc.
    assert!(actual.get("loc").is_none(), "refine: {actual:#}");
    assert_eq!(exit, 2, "refine: {actual:#}");
    assert_eq!(actual["result"], "error", "refine: {actual:#}");
    // Refine classifies init errors as type in its legacy envelope, like base ownership errors.
    assert_eq!(actual["kind"], "type", "refine: {actual:#}");
    let message = actual["message"]
        .as_str()
        .expect("refine semantics message");
    assert!(
        message.starts_with("init forall cannot read state variable")
            && message
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == "m"),
        "refine: expected init-read diagnostic naming m: {actual:#}"
    );
    for field in [
        "violation_kind",
        "invariant",
        "action",
        "last_action",
        "violation",
    ] {
        assert!(actual.get(field).is_none(), "refine: {actual:#}");
    }
}

positive_cell!(
    p2_field_after_whole_entry,
    r"spec P2 {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell>, seen: Map<K, Bool> }
  init { forall k: K { m[k] = Cell { flag: true, n: 2 } seen[k] = m[k].flag } }
  action idle() { }
  invariant Initialized {
    m[0].flag and m[0].n == 2 and m[1].flag and m[1].n == 2 and seen[0] and seen[1]
  }
}
",
    json!({"m": {"0": {"flag": true, "n": 2}, "1": {"flag": true, "n": 2}},
          "seen": {"0": true, "1": true}})
);

positive_cell!(
    p3_preceding_statement_and_forall,
    r"spec P3 {
  type K = 0..1
  state { flag: Bool, m: Map<K, Bool>, seen: Map<K, Bool> }
  init {
    flag = true
    forall k: K { m[k] = flag }
    forall k: K { seen[k] = m[k] }
  }
  action idle() { }
  invariant Initialized { flag and m[0] and m[1] and seen[0] and seen[1] }
}
",
    json!({"flag": true, "m": {"0": true, "1": true}, "seen": {"0": true, "1": true}})
);

empty_cell!(
    e1_literal_false_scalar,
    r"spec E1 {
  type K = 0..1
  state { x: 0..2 }
  init { forall k: K where false { x = 1 } }
  action idle() { }
  invariant Assigned { x == 1 }
}
",
    "type_bound",
    "_bounds_x",
    "x",
    false
);

empty_cell!(
    e2_literal_false_whole_struct,
    r"spec E2 {
  type K = 0..1
  struct Pair { a: Bool, b: 0..2 }
  state { s: Pair }
  init { forall k: K where false { s = Pair { a: true, b: 2 } } }
  action idle() { }
  invariant Assigned { s.a and s.b == 2 }
}
",
    "type_bound",
    "_bounds_s",
    "s",
    false
);

empty_cell!(
    e3_const_dependent_filter,
    r"spec E3 {
  type K = 0..1
  const Z = 0
  state { x: 0..2 }
  init { forall k: K where k < Z { x = 1 } }
  action idle() { }
  invariant Assigned { x == 1 }
}
",
    "type_bound",
    "_bounds_x",
    "x",
    false
);

empty_cell!(
    e4_state_dependent_filter,
    r"spec E4 {
  type K = 0..1
  state { flag: Bool, x: 0..2 }
  init { flag = false forall k: K where flag { x = 1 } }
  action idle() { }
  invariant Assigned { not flag and x == 1 }
}
",
    "type_bound",
    "_bounds_x",
    "x",
    false
);

// The default false would satisfy DefaultIsNotAssignment, but the free
// symbolic Bool can also be true. Concrete acceptance here is a false green.
empty_cell!(
    e5_nonliteral_empty_bool,
    r"spec E5 {
  type K = 0..1
  state { b: Bool }
  init { forall k: K where k > 5 { b = true } }
  action idle() { }
  invariant DefaultIsNotAssignment { b == false }
}
",
    "invariant",
    "DefaultIsNotAssignment",
    "b",
    false
);

empty_cell!(
    e6_const_empty_range,
    r"spec E6 {
  const N = 0
  state { x: 0..2 }
  init { forall i in 1..N { x = 1 } }
  action idle() { }
  invariant Assigned { x == 1 }
}
",
    "type_bound",
    "_bounds_x",
    "x",
    false
);

empty_cell!(
    e7_empty_fixed_map_entry,
    r"spec E7 {
  type K = 0..1
  struct Cell { flag: Bool, n: 0..2 }
  state { m: Map<K, Cell> }
  init {
    m[1] = Cell { flag: true, n: 2 }
    forall k: K where k > 5 { m[0] = Cell { flag: true, n: 2 } }
  }
  action idle() { }
  invariant DefaultIsNotAssignment { not m[0].flag and m[0].n == 0 and m[1].flag and m[1].n == 2 }
}
",
    "type_bound",
    "_bounds_m",
    "m",
    true
);

positive_cell!(
    ep_nonempty_singleton,
    r"spec EP {
  type K = 0..1
  state { x: 0..2 }
  init { forall k: K where k == 0 { x = 1 } }
  action idle() { }
  invariant Initialized { x == 1 }
}
",
    json!({"x": 1})
);

// EA uses E1's unconstrained bounded scalar. Auto must not retain the old
// default-filled explicit invariant violation; after deterministic-init
// refusal it falls back to BMC and returns the symbolic type-bound witness.
#[test]
fn ea_auto_falls_back_to_symbolic_free_value_semantics() {
    let source = r"spec EA {
  type K = 0..1
  state { x: 0..2 }
  init { forall k: K where false { x = 1 } }
  action idle() { }
  invariant Assigned { x == 1 }
}
";
    let (actual, exit) = Fixture::new(source).observe("auto");
    assert_verdict(
        "auto",
        &actual,
        exit,
        1,
        "violated",
        Some(("type_bound", "_bounds_x")),
    );
    assert_eq!(actual["engine"], "bmc", "auto: {actual:#}");
    assert_eq!(
        actual["engine_fallback"],
        json!({
            "from": "explicit", "kind": "unsupported",
            "reason": "init does not assign state variable(s): x",
        }),
        "auto: exact deterministic-init fallback: {actual:#}"
    );
}

fn assert_legacy_current_key_refused(source: &str, consumer: &str) {
    let (actual, exit) = Fixture::new(source).observe(consumer);
    assert_eq!(exit, 2, "{consumer}: {actual:#}");
    assert_eq!(actual["result"], "error", "{consumer}: {actual:#}");
    assert_eq!(actual["kind"], "semantics", "{consumer}: {actual:#}");
    assert_eq!(
        actual["message"], "init references state variable 'm' before it is assigned",
        "{consumer}: existing deterministic-init limitation: {actual:#}"
    );
    for field in [
        "loc",
        "violation_kind",
        "invariant",
        "action",
        "last_action",
        "violation",
        "final_state",
    ] {
        assert!(actual.get(field).is_none(), "{consumer}: {actual:#}");
    }
}

macro_rules! legacy_current_key_cell {
    ($id:ident, $source:expr) => {
        mod $id {
            use super::*;
            const SOURCE: &str = $source;
            #[test]
            fn check() {
                assert_clean(SOURCE, "check", "ok", None);
            }
            #[test]
            fn bmc() {
                assert_clean(SOURCE, "bmc", "verified", None);
            }
            #[test]
            fn induction() {
                assert_clean(SOURCE, "induction", "proved", None);
            }
            #[test]
            fn explicit() {
                assert_legacy_current_key_refused(SOURCE, "explicit");
            }
            #[test]
            fn monitor() {
                assert_legacy_current_key_refused(SOURCE, "Monitor");
            }
            // In addition to the five consumers, pin the auto fallback envelope.
            #[test]
            fn auto() {
                let (actual, exit) = Fixture::new(SOURCE).observe("auto");
                assert_verdict("auto", &actual, exit, 0, "verified", None);
                assert_eq!(actual["engine"], "bmc", "auto: {actual:#}");
                assert_eq!(
                    actual["engine_fallback"],
                    json!({
                        "from": "explicit", "kind": "unsupported",
                        "reason": "init references state variable 'm' before it is assigned",
                    }),
                    "auto: exact existing-init fallback: {actual:#}"
                );
            }
        }
    };
}

// X1/X2 record false refusals by explicit/Monitor already present at base,
// not incorrect verdicts. Removing these existing limitations belongs to #1520.
legacy_current_key_cell!(
    x1_filter_current_key,
    r"spec ReviewFilteredCurrentKey {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init { forall k: K where true { m[k] = true seen[k] = m[k] } }
  action idle() { }
  invariant Initialized { m[0] and m[1] and seen[0] and seen[1] }
}
"
);

legacy_current_key_cell!(
    x2_collection_current_key,
    r"spec ReviewCollectionCurrentKey {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init { forall k in Set { 0, 1 } { m[k] = true seen[k] = m[k] } }
  action idle() { }
  invariant Initialized { m[0] and m[1] and seen[0] and seen[1] }
}
"
);

// C1/C2 are order-independent init forms, intentionally removed by the approved
// per-root rule. Even at base, explicit and Monitor refused both forms.
read_cell!(
    c1_previous_disjoint_key,
    r"spec ReviewPreviousDisjointKey {
  type K = 0..1
  state { m: Map<K, Bool> }
  init {
    forall k in 0..0 { m[k] = true }
    forall k in 1..1 { m[k] = m[0] }
  }
  action idle() { }
  invariant Initialized { m[0] and m[1] }
}
",
    6,
    24
);

read_cell!(
    c2_nested_current_key,
    r"spec ReviewNestedCurrentKey {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init {
    forall i in 0..0 {
      forall k: K { m[k] = true seen[k] = m[k] }
    }
  }
  action idle() { }
  invariant Initialized { m[0] and m[1] and seen[0] and seen[1] }
}
",
    6,
    33
);

// G1: a write in only one branch is not definitely assigned after the if.
read_cell!(
    g1_branch_missing_write,
    r"spec ReviewBranchMissingWrite {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init {
    forall k: K {
      if k == 0 { m[k] = true }
      seen[k] = m[k]
    }
  }
  action idle() { }
  invariant Initialized { seen[0] and seen[1] }
}
",
    7,
    7
);

// G2: target-key expressions must be checked, not only assignment RHS values.
read_cell!(
    g2_assignment_key,
    r"spec ReviewAssignmentKey {
  type K = 0..1
  state { m: Map<K, K>, seen: Map<K, Bool> }
  init {
    forall k: K {
      m[k] = k
      seen[m[1]] = true
    }
  }
  action idle() { }
  invariant Initialized { seen[0] and seen[1] }
}
",
    7,
    7
);

// G3: the expression binder shadows k, so m[k] is not the current init key.
read_cell!(
    g3_expression_shadow,
    r"spec ReviewExpressionShadow {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init {
    forall k: K {
      m[k] = true
      seen[k] = forall k: K { m[k] }
    }
  }
  action idle() { }
  invariant Initialized { m[0] and m[1] and seen[0] and seen[1] }
}
",
    7,
    7
);

// G4: a differently named expression binder preserves the outer current key.
// Each m[k] is true before the read; quantifying j leaves that value unchanged.
positive_cell!(
    g4_expression_binder,
    r"spec ReviewExpressionBinder {
  type K = 0..1
  state { m: Map<K, Bool>, seen: Map<K, Bool> }
  init {
    forall k: K {
      m[k] = true
      seen[k] = forall j: K { m[k] }
    }
  }
  action idle() { }
  invariant Initialized { m[0] and m[1] and seen[0] and seen[1] }
}
",
    json!({"m": {"0": true, "1": true}, "seen": {"0": true, "1": true}})
);
