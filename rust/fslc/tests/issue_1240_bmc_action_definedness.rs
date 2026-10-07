// SPDX-License-Identifier: Apache-2.0

//! Issue #1240: BMC must ask whether an action's guard and body are defined
//! (checked i64 overflow, a finite `Map` key outside its domain) whether or
//! not the action contains one of LANGUAGE.md §6's partial operations.
//!
//! A deterministic `init` hides the defect: the concrete pre-pass reports the
//! runtime error first. One unused, unassigned `junk: Bool` makes `init`
//! nondeterministic, skips the pre-pass, and leaves only the symbolic path.
//! `--engine induction` runs the same BMC as its base case.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-1240-{name}-{}-{nonce}.fsl",
            std::process::id()
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    fn text(&self) -> &str {
        self.0.to_str().expect("UTF-8 temporary path")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn verify(fixture: &Fixture, engine: &str, depth: usize) -> (Value, i32) {
    let depth = depth.to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture.text(),
            "--engine",
            engine,
            "--depth",
            &depth,
            "--no-cache",
        ])
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn assert_semantics_error(output: &Value, status: i32, message: &str) {
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "semantics", "{output:#}");
    assert_eq!(output["message"], message, "{output:#}");
}

fn assert_clean(output: &Value, status: i32, engine: &str) {
    assert_eq!(status, 0, "{output:#}");
    let expected = if engine == "induction" {
        "proved"
    } else {
        "verified"
    };
    assert_eq!(output["result"], expected, "{output:#}");
}

const OVERFLOW_BODY_ND: &str = r"
spec OverflowBodyNd {
  state { junk: Bool, x: Int }
  init { x = 0 }
  action inc() { x = x + 4611686018427387904 }
  invariant NonNegative { x >= 0 }
}
";

const MAP_WRITE_BODY_ND: &str = r"
spec MapWriteBodyNd {
  type K = 0..3
  type V = 0..1
  type I = 0..5
  state { junk: Bool, m: Map<K, V>, i: I }
  init {
    forall k: K { m[k] = 0 }
    i = 0
  }
  action step() { requires i < 5  i = i + 1 }
  action write() { m[i] = 1 }
  invariant Small { i <= 5 }
}
";

const MAP_READ_GUARD_ND: &str = r"
spec MapReadGuardNd {
  type K = 0..3
  type V = 0..1
  type I = 0..5
  state { junk: Bool, m: Map<K, V>, i: I, hit: Bool }
  init {
    forall k: K { m[k] = 0 }
    i = 0
    hit = false
  }
  action step() { requires i < 5  i = i + 1 }
  action read() { requires m[i] == 0  hit = true }
  invariant Small { i <= 5 }
}
";

/// Detector: fails on `458040f3`, where all three report `verified` /
/// `proved` with exit 0.
#[test]
fn candidate_free_action_undefinedness_is_reported() {
    let cases = [
        (
            "overflow-body",
            OVERFLOW_BODY_ND,
            "action 'inc' body evaluation has a non-partial failure",
        ),
        (
            "map-write-body",
            MAP_WRITE_BODY_ND,
            "action 'write' body evaluation has a non-partial failure",
        ),
        (
            "map-read-guard",
            MAP_READ_GUARD_ND,
            "action 'read' guard evaluation has a non-partial failure",
        ),
    ];
    for (name, source, message) in cases {
        let fixture = Fixture::new(name, source);
        for engine in ["bmc", "induction"] {
            let (output, status) = verify(&fixture, engine, 8);
            assert_semantics_error(&output, status, message);
        }
    }
}

/// The issue's comparator: with a deterministic `init` the concrete
/// pre-pass and the explicit engine already report the runtime error. A
/// preservation control; it passes before and after the fix.
#[test]
fn deterministic_twins_keep_the_concrete_runtime_error() {
    let cases = [
        (
            "overflow-body-det",
            OVERFLOW_BODY_ND.replace("junk: Bool, ", ""),
            "integer overflow in addition",
        ),
        (
            "map-write-body-det",
            MAP_WRITE_BODY_ND.replace("junk: Bool, ", ""),
            "map assignment index outside key domain",
        ),
        (
            "map-read-guard-det",
            MAP_READ_GUARD_ND.replace("junk: Bool, ", ""),
            "map index outside finite key domain",
        ),
    ];
    for (name, source, message) in cases {
        let fixture = Fixture::new(name, &source);
        for engine in ["bmc", "explicit"] {
            let (output, status) = verify(&fixture, engine, 8);
            assert_eq!(status, 2, "{engine}: {output:#}");
            assert_eq!(output["result"], "error", "{engine}: {output:#}");
            assert_eq!(output["message"], message, "{engine}: {output:#}");
        }
    }
}

/// `x` reaches `3 * 2^61` after three steps; the fourth `inc` overflows, so
/// the error needs depth 4. Depth 3 is a preservation control (no false
/// alarm before the overflow is reachable); depth 4 is a detector.
#[test]
fn overflow_is_reported_exactly_when_it_is_within_depth() {
    let fixture = Fixture::new(
        "overflow-late",
        r"
spec OverflowLateNd {
  state { junk: Bool, x: Int }
  init { x = 0 }
  action inc() { x = x + 2305843009213693952 }
  invariant NonNegative { x >= 0 }
}
",
    );
    let (output, status) = verify(&fixture, "bmc", 3);
    assert_clean(&output, status, "bmc");
    let (output, status) = verify(&fixture, "bmc", 4);
    assert_semantics_error(
        &output,
        status,
        "action 'inc' body evaluation has a non-partial failure",
    );
}

/// Preservation control: a guarded `Map` read and a branch-guarded write stay
/// defined, because the obligation keeps `and`/`if` short-circuiting.
#[test]
fn guarded_map_access_stays_verified() {
    let fixture = Fixture::new(
        "guarded-map",
        r"
spec GuardedMapNd {
  type K = 0..3
  type V = 0..1
  type I = 0..5
  state { junk: Bool, m: Map<K, V>, i: I, hit: Bool }
  init {
    forall k: K { m[k] = 0 }
    i = 0
    hit = false
  }
  action step() { requires i < 5  i = i + 1 }
  action read() { requires i <= 3 and m[i] == 0  hit = true }
  action write() { if i <= 3 { m[i] = 1 } }
  invariant Small { i <= 5 }
}
",
    );
    for engine in ["bmc", "induction"] {
        let (output, status) = verify(&fixture, engine, 8);
        assert_clean(&output, status, engine);
    }
}

/// Preservation controls for the paths #1240 does not change: an action with
/// a typed partial-operation candidate, and a property or `ensures` whose
/// definedness BMC already asked unconditionally.
#[test]
fn partial_candidate_actions_and_properties_are_unchanged() {
    let control = Fixture::new(
        "map-write-control",
        r"
spec MapWriteBodyControlNd {
  type K = 0..3
  type V = 0..1
  type I = 0..5
  type D = 0..2
  state { junk: Bool, m: Map<K, V>, i: I, d: D, q: V }
  init {
    forall k: K { m[k] = 0 }
    i = 0
    d = 1
    q = 0
  }
  action step() { requires i < 5  i = i + 1 }
  action write() {
    m[i] = 1
    q = 1 / d
  }
  invariant Small { i <= 5 }
}
",
    );
    let (output, status) = verify(&control, "bmc", 8);
    assert_semantics_error(
        &output,
        status,
        "action 'write' body evaluation has a non-partial failure",
    );

    let division = Fixture::new(
        "divide-by-zero",
        r"
spec DivZeroNd {
  type D = 0..2
  state { junk: Bool, d: D, q: Int }
  init { d = 1  q = 0 }
  action zero() { d = 0 }
  action div() { q = 1 / d }
}
",
    );
    for engine in ["bmc", "induction"] {
        let (output, status) = verify(&division, engine, 8);
        assert_eq!(status, 1, "{engine}: {output:#}");
        assert_eq!(output["result"], "violated", "{engine}: {output:#}");
        assert_eq!(
            output["violation_kind"], "partial_op",
            "{engine}: {output:#}"
        );
        assert_eq!(output["invariant"], "_partial_div", "{engine}: {output:#}");
    }

    let property = Fixture::new(
        "invariant-overflow",
        r"
spec InvariantOverflowNd {
  state { junk: Bool, x: Int }
  init { x = 0 }
  action inc() { requires x < 3  x = x + 1 }
  invariant Big { x + 9223372036854775807 >= 0 }
}
",
    );
    let (output, status) = verify(&property, "bmc", 8);
    assert_semantics_error(
        &output,
        status,
        "invariant 'Big' is undefined for a reachable state",
    );

    let ensures = Fixture::new(
        "ensures-overflow",
        r"
spec EnsuresOverflowNd {
  state { junk: Bool, x: Int }
  init { x = 0 }
  action inc() {
    requires x < 3
    x = x + 1
    ensures x + 9223372036854775807 > 0
  }
  invariant Small { x <= 3 }
}
",
    );
    let (output, status) = verify(&ensures, "bmc", 8);
    assert_semantics_error(
        &output,
        status,
        "action 'inc' ensures evaluation has a non-partial failure",
    );
}

/// Detector for the definedness pass's `solver.reset()` (#1240 review r2,
/// F1): an action is undefined at step 0 or 1, and the path then ends in a
/// `terminal { done }` state with no successor. Without the reset, the
/// search's later-step transitions (each requires a successor state) stay
/// asserted, every path that dead-ends is removed from the early steps'
/// questions, and both engines report `verified` / `proved` with exit 0.
/// `terminal` keeps the deadlock witness (and its replay) out of the way.
#[test]
fn undefinedness_before_a_terminal_dead_end_is_reported() {
    let cases = [
        (
            "guard-then-deadlock",
            r"
spec GuardThenDeadlock {
  state { junk: Int, x: Int, done: Bool }
  init {
    x = 9223372036854775807
    done = false
  }
  action go() { requires done == false  requires x + 1 > 0  done = true }
  terminal { done }
}
",
            "action 'go' guard evaluation has a non-partial failure",
        ),
        (
            "body-then-deadlock",
            r"
spec BodyThenDeadlock {
  state { junk: Int, x: Int, done: Bool }
  init {
    x = 9223372036854775807
    done = false
  }
  action go() { requires done == false  done = x + 1 > 0 }
  terminal { done }
}
",
            "action 'go' body evaluation has a non-partial failure",
        ),
        (
            "late-then-deadlock",
            r"
spec LateThenDeadlock {
  state { junk: Int, x: Int, n: Int, done: Bool }
  init {
    x = 9223372036854775806
    n = 0
    done = false
  }
  action tick() { requires n == 0  n = 1 }
  action go() { requires n == 1  requires done == false  requires x + 2 > 0  done = true }
  terminal { done }
}
",
            "action 'go' guard evaluation has a non-partial failure",
        ),
        (
            "map-then-deadlock",
            r"
spec MapThenDeadlock {
  type K = 0..3
  type V = 0..1
  type I = 0..5
  state { junk: Int, m: Map<K, V>, i: I, done: Bool }
  init {
    forall k: K { m[k] = 0 }
    i = 5
    done = false
  }
  action write() { requires done == false  m[i] = 1  done = true }
  terminal { done }
}
",
            "action 'write' body evaluation has a non-partial failure",
        ),
    ];
    for (name, source, message) in cases {
        let fixture = Fixture::new(name, source);
        for engine in ["bmc", "induction"] {
            let (output, status) = verify(&fixture, engine, 4);
            assert_semantics_error(&output, status, message);
        }
    }
}

/// Pins the definedness pass's range at the step the search stopped in
/// (#1240 review r2, F2): in one step, `a` overflows with no typed
/// partial-operation candidate and the later `b` has a typed division
/// candidate. The pass must ask that step, so `a`'s non-partial failure is
/// the semantics error, as `c5c5398d` (definedness asked inside the search)
/// reports it; without the step, the replay reports an internal overflow
/// (exit 3).
#[test]
fn same_step_non_partial_failure_precedes_a_later_partial_op() {
    let fixture = Fixture::new(
        "same-step-order",
        r"
spec SameStepOrder {
  state { junk: Int, x: Int, d: Int }
  init {
    x = 9223372036854775807
  }
  action a() { requires x + 1 > 0  junk = 1 }
  action b() { requires 10 / d > 0  junk = 2 }
}
",
    );
    let (output, status) = verify(&fixture, "bmc", 4);
    assert_semantics_error(
        &output,
        status,
        "action 'a' guard evaluation has a non-partial failure",
    );
}
