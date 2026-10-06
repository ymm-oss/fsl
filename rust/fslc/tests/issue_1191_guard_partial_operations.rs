// SPDX-License-Identifier: Apache-2.0

//! Issue #1191: a partial operation reached in an action's `requires` or `let`
//! is `violated` / `partial_op` (LANGUAGE.md §6; DESIGN-kernel-contract.md
//! "guards (`let` and `requires`) in source order ... a reached partial
//! expression is `partial_op`"), in every engine. The concrete action
//! enumeration (explicit, and the BMC boundary pre-pass) used to stop with a
//! raw `result: error, kind: semantics` instead.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use fsl_core::{FsResolver, KernelModel, build_model, parse_kernel_source};
use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-1191-{name}-{}-{nonce}.fsl",
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

fn verify(fixture: &Fixture, engine: &str) -> (Value, i32) {
    verify_with(fixture, engine, "3", "ignore")
}

fn verify_with(fixture: &Fixture, engine: &str, depth: &str, deadlock: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture.text(),
            "--engine",
            engine,
            "--depth",
            depth,
            "--deadlock",
            deadlock,
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

fn build(source: &str) -> KernelModel {
    let resolver = FsResolver::new(".");
    let kernel = parse_kernel_source(source, &resolver).expect("fixture parses");
    build_model(kernel).expect("fixture typechecks")
}

/// The three reproductions in the issue body, verbatim in shape.
const REQUIRES_HEAD: &str = r"
spec RequiresHead {
  type Small = 0..3
  state { s: Seq<Small, 3>, n: Small }
  init { s = Seq {}  n = 0 }
  action a() {
    requires s.head() == 0
    n = 1
  }
}
";

const LET_HEAD: &str = r"
spec LetHead {
  type Small = 0..3
  state { s: Seq<Small, 3>, n: Small }
  init { s = Seq {}  n = 0 }
  action a() {
    let h = s.head()
    n = h
  }
}
";

const REQUIRES_DIVIDE: &str = r"
spec RequiresDivide {
  type Small = 0..3
  state { x: Small, n: Small }
  init { x = 0  n = 0 }
  action a() {
    requires 2 / x == 0
    n = 1
  }
}
";

/// The verdict fields both engines must agree on for an action-context
/// `partial_op` (the symbolic engine already produced exactly these).
fn verdict(output: &Value) -> Value {
    serde_json::json!({
        "result": output["result"],
        "violation_kind": output["violation_kind"],
        "invariant": output["invariant"],
        "loc": output["loc"],
        "violated_at_step": output["violated_at_step"],
        "last_action": output["last_action"]["name"],
        "trace_type": output["trace_type"],
        "faithfulness_class": output["faithfulness_class"],
        "trace": output["trace"],
    })
}

/// The `{line, column}` of the Public Kernel's only `partial_operations` site
/// of action `a` (`fslc kernel`), the position every engine must report for a
/// guard `partial_op` (#1191).
fn kernel_partial_location(fixture: &Fixture) -> Value {
    let sites = kernel_partial_locations(fixture);
    assert_eq!(sites.len(), 1, "one guard site: {sites:#?}");
    sites[0].clone()
}

/// Every `partial_operations` site of action `a`, in Kernel order.
fn kernel_partial_locations(fixture: &Fixture) -> Vec<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["kernel", fixture.text()])
        .output()
        .expect("run native CLI");
    let kernel: Value = serde_json::from_slice(&output.stdout).expect("kernel JSON");
    let action = kernel["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .find(|action| action["name"] == "a")
        .expect("action a");
    action["partial_operations"]
        .as_array()
        .expect("partial_operations")
        .iter()
        .map(|site| {
            let span = &site["span"];
            serde_json::json!({"line": span["line"], "column": span["column"]})
        })
        .collect()
}

fn assert_guard_partial(name: &str, source: &str, step: u64) {
    let fixture = Fixture::new(name, source);
    let kernel_location = kernel_partial_location(&fixture);
    let mut verdicts = Vec::new();
    for engine in ["bmc", "explicit"] {
        let (output, status) = verify(&fixture, engine);
        assert_eq!(status, 1, "{name}/{engine}: {output:#}");
        assert_eq!(output["result"], "violated", "{name}/{engine}: {output:#}");
        assert_eq!(
            output["violation_kind"], "partial_op",
            "{name}/{engine}: {output:#}"
        );
        assert_eq!(
            output["invariant"], "_partial_a",
            "{name}/{engine}: {output:#}"
        );
        assert_eq!(
            output["loc"], kernel_location,
            "{name}/{engine}: a guard partial_op is located at its Public Kernel \
             partial_operations span: {output:#}"
        );
        assert_eq!(
            output["violated_at_step"], step,
            "{name}/{engine}: {output:#}"
        );
        assert_eq!(
            output["last_action"]["name"], "a",
            "{name}/{engine}: {output:#}"
        );
        let trace = output["trace"].as_array().expect("trace");
        let last = trace.last().expect("non-empty trace");
        let before = &trace[trace.len() - 2];
        assert_eq!(
            last["action"]["name"], "a",
            "{name}/{engine}: the failing attempt ends the trace: {output:#}"
        );
        assert_eq!(
            last["state"], before["state"],
            "{name}/{engine}: a guard failure has no successor state: {output:#}"
        );
        verdicts.push(verdict(&output));
    }
    assert_eq!(
        verdicts[0], verdicts[1],
        "{name}: bmc and explicit disagree on the guard partial_op"
    );
}

#[test]
fn requires_head_on_empty_sequence_is_partial_op_in_bmc_and_explicit() {
    assert_guard_partial("requires-head", REQUIRES_HEAD, 1);
}

#[test]
fn let_head_on_empty_sequence_is_partial_op_in_bmc_and_explicit() {
    assert_guard_partial("let-head", LET_HEAD, 1);
}

#[test]
fn requires_division_by_zero_is_partial_op_in_bmc_and_explicit() {
    assert_guard_partial("requires-divide", REQUIRES_DIVIDE, 1);
}

#[test]
fn guard_partial_op_reached_after_steps_keeps_its_step_and_trace() {
    assert_guard_partial(
        "later-head",
        r"
spec LaterHead {
  type Small = 0..3
  state { s: Seq<Small, 3>, n: Small }
  init { s = Seq { 0 }  n = 0 }
  action drop() {
    requires s.size() > 0
    s = s.pop()
  }
  action a() {
    requires s.head() == 0
    n = 1
  }
}
",
        2,
    );
}

/// An action without a body has no statement to point at: the guard's Kernel
/// span is its only location, in both engines (it used to be `null`).
#[test]
fn bodyless_action_guard_partial_op_is_located() {
    assert_guard_partial(
        "bodyless",
        r"
spec Bodyless {
  type Small = 0..3
  state { x: Small }
  init { x = 0 }
  action a() {
    requires 2 / x == 0
  }
}
",
        1,
    );
}

/// A nondeterministic init keeps BMC on the symbolic engine (no concrete
/// boundary pre-pass); its guard `partial_op` is located at the same Kernel
/// span.
#[test]
fn symbolic_bmc_guard_partial_op_is_located_at_the_kernel_span() {
    let fixture = Fixture::new(
        "symbolic-head",
        r"
spec SymbolicHead {
  type Small = 0..3
  state { s: Seq<Small, 3>, n: Small, b: Bool }
  init { s = Seq {}  n = 0 }
  action a() {
    requires s.head() == 0
    n = 1
  }
}
",
    );
    let (output, status) = verify(&fixture, "bmc");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
    assert_eq!(
        output["loc"],
        kernel_partial_location(&fixture),
        "{output:#}"
    );
}

/// With several guard sites, the location is the site of the guard that
/// actually failed, not the first one listed.
#[test]
fn guard_partial_op_is_located_at_the_failing_guard_site() {
    let fixture = Fixture::new(
        "second-guard",
        r"
spec SecondGuard {
  type Small = 0..3
  state { x: Small, n: Small }
  init { x = 0  n = 0 }
  action a() {
    requires 2 / (x + 1) == 2
    requires 2 / x == 0
    n = 1
  }
}
",
    );
    let sites = kernel_partial_locations(&fixture);
    assert_eq!(sites.len(), 2, "{sites:#?}");
    assert_ne!(sites[0], sites[1], "{sites:#?}");
    for engine in ["bmc", "explicit"] {
        let (output, status) = verify(&fixture, engine);
        assert_eq!(status, 1, "{engine}: {output:#}");
        assert_eq!(
            output["violation_kind"], "partial_op",
            "{engine}: {output:#}"
        );
        assert_eq!(output["loc"], sites[1], "{engine}: {output:#}");
    }
}

/// A partial operation in the body keeps its existing location (the body's
/// first statement), even when the action also has a guard: only a guard
/// failure moves to the guard's Kernel span.
#[test]
fn body_partial_op_location_is_unchanged() {
    let fixture = Fixture::new(
        "body-head",
        r"
spec BodyHead {
  type Small = 0..3
  state { s: Seq<Small, 3>, n: Small }
  init { s = Seq {}  n = 0 }
  action a() {
    requires n == 0
    n = s.head()
  }
}
",
    );
    for engine in ["bmc", "explicit"] {
        let (output, status) = verify(&fixture, engine);
        assert_eq!(status, 1, "{engine}: {output:#}");
        assert_eq!(
            output["violation_kind"], "partial_op",
            "{engine}: {output:#}"
        );
        assert_eq!(
            output["loc"],
            serde_json::json!({"line": 8, "column": 5}),
            "{engine}: {output:#}"
        );
    }
}

/// Negative controls: a guard that keeps the partial operation unreached
/// (short-circuit `and`, an earlier `requires`, or an ordered `let` after a
/// `requires`) is not a `partial_op` in any engine.
#[test]
fn guarded_partial_operations_in_requires_and_let_stay_clean() {
    let cases = [
        (
            "guard-and",
            r"
spec GuardAnd {
  type Small = 0..3
  state { s: Seq<Small, 3>, n: Small }
  init { s = Seq {}  n = 0 }
  action a() {
    requires s.size() > 0 and s.head() == 0
    n = 1
  }
}
",
        ),
        (
            "guard-ordered-requires",
            r"
spec GuardOrderedRequires {
  type Small = 0..3
  state { x: Small, n: Small }
  init { x = 0  n = 0 }
  action a() {
    requires x != 0
    requires 2 / x == 0
    n = 1
  }
}
",
        ),
        (
            "guard-ordered-let",
            r"
spec GuardOrderedLet {
  type Small = 0..3
  state { s: Seq<Small, 3>, n: Small }
  init { s = Seq {}  n = 0 }
  action a() {
    requires s.size() > 0
    let h = s.head()
    n = h
  }
}
",
        ),
    ];
    for (name, source) in cases {
        let fixture = Fixture::new(name, source);
        for engine in ["bmc", "explicit"] {
            let (output, status) = verify(&fixture, engine);
            assert_eq!(status, 0, "{name}/{engine}: {output:#}");
            assert!(
                output.get("violation_kind").is_none(),
                "{name}/{engine}: {output:#}"
            );
        }
    }
}

/// At the depth bound the guard `partial_op` lies one step beyond the search,
/// and the instance is still not *disabled* (its guard did not evaluate to
/// `false`), so the state is not a deadlock. BMC agrees here: its bound-step
/// enabledness reads the totalized `2 / 0 == 0`, which holds.
#[test]
fn guard_partial_op_at_the_depth_bound_is_not_a_deadlock() {
    let fixture = Fixture::new("bound-divide", REQUIRES_DIVIDE);
    for engine in ["bmc", "explicit"] {
        let (output, status) = verify_with(&fixture, engine, "0", "error");
        assert_eq!(status, 0, "{engine}: {output:#}");
        assert_eq!(output["result"], "verified", "{engine}: {output:#}");
        assert_eq!(output["deadlock"]["found"], false, "{engine}: {output:#}");
    }
}

/// An instance whose guard reaches a partial operation is not enabled, so a
/// concrete walk that only needs enabled instances skips it instead of
/// aborting the whole walk.
const PEEK_AFTER_PUSH: &str = r"
spec PeekAfterPush {
  type Small = 0..1
  state { s: Seq<Small, 1>, n: Small }
  init { s = Seq {}  n = 0 }
  action push() {
    requires s.size() < 1
    s = s.push(0)
  }
  action peek() {
    requires s.head() == 0
    n = 1
  }
}
";

#[test]
fn replay_is_not_aborted_by_an_unrelated_partial_guard() {
    let model = build(PEEK_AFTER_PUSH);
    let monitor = fsl_runtime::Monitor::new(model.clone()).expect("deterministic init");
    let initial = monitor.state.clone();
    let mut pushed = initial.clone();
    pushed.insert(
        "s".to_owned(),
        fsl_core::FslValue::Seq(vec![fsl_core::FslValue::Int(0)]),
    );
    let trace = vec![
        fsl_core::TraceStep {
            step: 0,
            state: initial.clone(),
            action: None,
            changes: std::collections::BTreeMap::new(),
        },
        fsl_core::TraceStep {
            step: 1,
            state: pushed.clone(),
            action: Some(fsl_core::TraceAction {
                name: "push".to_owned(),
                params: std::collections::BTreeMap::new(),
            }),
            changes: std::collections::BTreeMap::from([(
                "s".to_owned(),
                fsl_core::TraceChange {
                    from: initial["s"].clone(),
                    to: pushed["s"].clone(),
                },
            )]),
        },
    ];
    fsl_runtime::replay_trace(model, &trace)
        .expect("push is enabled; peek's partial guard is not part of this trace");
}

#[test]
fn action_cover_traces_skip_a_partial_guard_instead_of_erroring() {
    let model = build(PEEK_AFTER_PUSH);
    let covers = fsl_runtime::action_cover_traces(model, 2).expect("cover traces");
    assert_eq!(covers.keys().collect::<Vec<_>>(), ["peek", "push"]);
    assert_eq!(
        covers["peek"].len(),
        3,
        "peek is first covered after push: {covers:?}"
    );
}
