// SPDX-License-Identifier: Apache-2.0

//! #1481 cell 2: reached zero-capacity reads are violations, not evaluation
//! errors or successful reads. Expectations come from LANGUAGE.md §6 and
//! DESIGN-seq-partial-operations.md, not another engine's classification.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::{Value, json};

struct Fixture(PathBuf);

impl Fixture {
    fn new(cell: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-1481-{cell}-{}-{nonce}.fsl",
            std::process::id()
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    fn run(&self, command: &str, options: &[&str]) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .env_remove("FSLC_CACHE")
            .arg(command)
            .arg(&self.0)
            .args(options)
            .output()
            .expect("run working-tree CLI");
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON: {error}; stdout={}; stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (value, output.status.code().expect("native exit status"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// This is a verdict-contract test, not an equality claim about entire engine
// envelopes (proof/completeness/cost/coverage metadata have different meanings).
// Preserve the full envelope in failure messages. Include message even for
// non-error results so a raw semantics error cannot masquerade as partial_op.
fn verdict(output: &Value, exit: i32) -> Value {
    json!({
        "exit": exit,
        "result": output["result"],
        "violation_kind": output["violation_kind"],
        "invariant": output["invariant"],
        "last_action": output["last_action"]["name"],
        "violated_at_step": output["violated_at_step"],
        "message": output["message"],
    })
}

fn check(cell: &str, source: &str) {
    let fixture = Fixture::new(cell, source);
    let (output, exit) = fixture.run("check", &[]);
    let actual = verdict(&output, exit);
    eprintln!("{cell}/check: {actual}");
    assert_eq!(
        actual,
        json!({
            "exit": 0, "result": "ok", "violation_kind": null,
            "invariant": null, "last_action": null,
            "violated_at_step": null, "message": null,
        }),
        "{cell}/check: {output:#}"
    );
}

fn verify(cell: &str, source: &str, engine: &str, expected: &Value) {
    let fixture = Fixture::new(cell, source);
    let (output, exit) = fixture.run(
        "verify",
        &[
            "--engine",
            engine,
            "--depth",
            "4",
            "--no-cache",
            "--deadlock",
            "ignore",
        ],
    );
    let actual = verdict(&output, exit);
    eprintln!("{cell}/{engine}: {actual}");
    assert_eq!(&actual, expected, "{cell}/{engine}: {output:#}");
}

fn partial(name: &str, action: Option<&str>, step: usize) -> Value {
    json!({
        "exit": 1, "result": "violated", "violation_kind": "partial_op",
        "invariant": name, "last_action": action,
        "violated_at_step": step, "message": null,
    })
}

fn pass(result: &str) -> Value {
    json!({
        "exit": 0, "result": result, "violation_kind": null,
        "invariant": null, "last_action": null,
        "violated_at_step": null, "message": null,
    })
}

fn property_source(capacity: &str, declaration: &str, expression: &str) -> String {
    format!(
        "spec ZeroCapacityProperty {{
  {declaration}
  type K = 0..1
  state {{ xs: Seq<K, {capacity}> }}
  init {{ xs = Seq {{}} }}
  action idle() {{ }}
  invariant Capture {{ {expression} }}
}}"
    )
}

fn action_source(capacity: &str, statement: &str) -> String {
    format!(
        "spec ZeroCapacityAction {{
  type K = 0..1
  state {{ xs: Seq<K, {capacity}>, x: K }}
  init {{ xs = Seq {{}}  x = 0 }}
  action take() {{ {statement} }}
}}"
    )
}

fn unreached_source(capacity: &str, guard: &str, read: &str, property: &str) -> String {
    format!(
        "spec UnreachedPartialRead {{
  type K = 0..1
  state {{ x: K, xs: Seq<K, {capacity}> }}
  init {{ x = 0  xs = Seq {{}} }}
  action take() {{
    requires {guard}
    x = {read}
  }}
  action idle() {{ }}
  invariant Capture {{ {property} }}
}}"
    )
}

fn monitor(cell: &str, source: &str, action: &str, name: Option<&str>) {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse fixture");
    let model = build_model(kernel).expect("build fixture");
    let mut monitor = fsl_runtime::Monitor::new(model).expect("deterministic init");
    let initial = monitor.state.clone();
    let result = monitor
        .attempt(action, &BTreeMap::new())
        .unwrap_or_else(|error| panic!("{cell}/Monitor/{action}: {error:?}"));
    // Monitor has no process exit code or result string. Check its raw public
    // violation instead of fabricating a CLI verdict. Unlike CLI's initial
    // property check at step 0, idle consults Capture at step 1.
    eprintln!("{cell}/Monitor/{action}: {result:?}");
    assert_eq!(
        result,
        fsl_runtime::StepResult {
            action: action.to_owned(),
            params: BTreeMap::new(),
            state: initial.clone(),
            attempted_state: (action == "idle" && name.is_some()).then(|| initial.clone()),
            violation: name.map(|name| fsl_runtime::Violation {
                kind: "partial_op".to_owned(),
                name: name.to_owned(),
                step: 1,
            }),
        },
        "{cell}/Monitor: reached reads must not pass or return a raw error"
    );
    assert_eq!(monitor.state, initial, "{cell}/Monitor retained state");

    if name.is_none() {
        // A disabled call is not a violation of the spec. Nevertheless, directly
        // attempt it so we detect eager evaluation of its unreachable body too.
        let enabled = monitor.enabled().expect("enumerate enabled actions");
        assert_eq!(enabled.len(), 1, "{cell}/Monitor: {enabled:?}");
        assert_eq!(enabled[0].action, "idle", "{cell}/Monitor: {enabled:?}");
        let rejected = monitor
            .attempt("take", &BTreeMap::new())
            .expect("do not evaluate a disabled action body");
        eprintln!("{cell}/Monitor/take: {rejected:?}");
        assert_eq!(
            rejected,
            fsl_runtime::StepResult {
                action: "take".to_owned(),
                params: BTreeMap::new(),
                state: initial.clone(),
                attempted_state: None,
                violation: Some(fsl_runtime::Violation {
                    kind: "requires_failed".to_owned(),
                    name: "_requires_failed_take".to_owned(),
                    step: 2,
                }),
            },
            "{cell}/Monitor disabled action"
        );
        assert_eq!(monitor.state, initial, "{cell}/Monitor disabled state");
    }
}

// Separate tests (and literal expected verdicts) for each engine keep a failure
// in symbolic evaluation from hiding the explicit or direct Monitor result.
macro_rules! partial_cell {
    ($cell:ident, $source:expr, $name:literal, $action:literal, $last:expr, $step:literal) => {
        mod $cell {
            use super::*;

            #[test]
            fn check() {
                super::check(stringify!($cell), &$source);
            }

            #[test]
            fn bmc() {
                verify(
                    stringify!($cell),
                    &$source,
                    "bmc",
                    &partial($name, $last, $step),
                );
            }

            #[test]
            fn induction() {
                verify(
                    stringify!($cell),
                    &$source,
                    "induction",
                    &partial($name, $last, $step),
                );
            }

            #[test]
            fn explicit() {
                verify(
                    stringify!($cell),
                    &$source,
                    "explicit",
                    &partial($name, $last, $step),
                );
            }

            #[test]
            fn monitor() {
                super::monitor(stringify!($cell), &$source, $action, Some($name));
            }
        }
    };
}

partial_cell!(
    b1_property_head,
    property_source("0", "", "xs.head() == 1"),
    "_partial_property_Capture",
    "idle",
    None,
    0
);
partial_cell!(
    b2_property_at,
    property_source("0", "", "xs.at(0) == 1"),
    "_partial_property_Capture",
    "idle",
    None,
    0
);
partial_cell!(
    b3_property_index,
    property_source("0", "", "xs[0] == 1"),
    "_partial_property_Capture",
    "idle",
    None,
    0
);
partial_cell!(
    b4_action_head,
    action_source("0", "x = xs.head()"),
    "_partial_take",
    "take",
    Some("take"),
    1
);
partial_cell!(
    b5_action_at,
    action_source("0", "x = xs.at(0)"),
    "_partial_take",
    "take",
    Some("take"),
    1
);
partial_cell!(
    b6_action_index,
    action_source("0", "x = xs[0]"),
    "_partial_take",
    "take",
    Some("take"),
    1
);
partial_cell!(
    b7_const_head,
    property_source("CAP", "const CAP = 0", "xs.head() == 1"),
    "_partial_property_Capture",
    "idle",
    None,
    0
);

// Preservation controls: none of these require the #1481 production fix.
partial_cell!(
    n1_property_pop,
    property_source("0", "", "xs.pop().size() == 0"),
    "_partial_property_Capture",
    "idle",
    None,
    0
);
partial_cell!(
    n1_action_pop,
    action_source("0", "xs = xs.pop()"),
    "_partial_take",
    "take",
    Some("take"),
    1
);
partial_cell!(
    n2_property_head,
    property_source("1", "", "xs.head() == 1"),
    "_partial_property_Capture",
    "idle",
    None,
    0
);
partial_cell!(
    n2_property_at,
    property_source("1", "", "xs.at(0) == 1"),
    "_partial_property_Capture",
    "idle",
    None,
    0
);
partial_cell!(
    n2_action_head,
    action_source("1", "x = xs.head()"),
    "_partial_take",
    "take",
    Some("take"),
    1
);
partial_cell!(
    n2_action_at,
    action_source("1", "x = xs.at(0)"),
    "_partial_take",
    "take",
    Some("take"),
    1
);

macro_rules! unreached_cell {
    ($cell:ident, $source:expr) => {
        mod $cell {
            use super::*;

            #[test]
            fn check() {
                super::check(stringify!($cell), &$source);
            }

            #[test]
            fn bmc() {
                verify(stringify!($cell), &$source, "bmc", &pass("verified"));
            }

            #[test]
            fn induction() {
                verify(stringify!($cell), &$source, "induction", &pass("proved"));
            }

            #[test]
            fn explicit() {
                verify(stringify!($cell), &$source, "explicit", &pass("proved"));
            }

            #[test]
            fn monitor() {
                super::monitor(stringify!($cell), &$source, "idle", None);
            }
        }
    };
}

// Zero-capacity unreachable reads are also detectors: base errors while the
// accepted path-sensitive contract requires a pass, not partial_op.
unreached_cell!(
    n3_disabled_head,
    unreached_source("0", "false", "xs.head()", "x == 0")
);
unreached_cell!(
    n3_guarded_at,
    unreached_source(
        "0",
        "xs.size() > 0",
        "xs.at(0)",
        "xs.size() > 0 => xs.at(0) == 1"
    )
);

// The identical capacity-one forms are preservation controls, green on base.
unreached_cell!(
    n4_disabled_head,
    unreached_source("1", "false", "xs.head()", "x == 0")
);
unreached_cell!(
    n4_guarded_at,
    unreached_source(
        "1",
        "xs.size() > 0",
        "xs.at(0)",
        "xs.size() > 0 => xs.at(0) == 1"
    )
);

// L1 compares the complete public envelope after separating only two observed
// non-evidence objects. In base measurements cost varies in elapsed time,
// checks and memory, including zero solver checks for explicit; versions names
// the build and installed solver, not the state. Both exclusions must be live.
fn stable_evidence(mut output: Value) -> Value {
    let object = output.as_object_mut().expect("CLI JSON object");
    for key in ["cost", "versions"] {
        let metadata = object
            .remove(key)
            .unwrap_or_else(|| panic!("missing {key}"));
        assert!(metadata.is_object(), "{key} must be a live metadata object");
    }
    output
}

fn expected_evidence(action: bool, engine: &str) -> Value {
    let action_location = json!({"line": 5, "column": 3});
    let call = json!({"name": "take", "params": {}, "loc": action_location});
    let state = if action {
        json!({"x": 0, "xs": []})
    } else {
        json!({"xs": []})
    };
    let trace = if action {
        json!([
            {"step": 0, "state": state},
            {
                "step": 1, "state": state, "action": call,
                "changes": {}, "blame": {"guards": [], "effects": []},
            },
        ])
    } else {
        json!([{"step": 0, "state": state}])
    };
    let mut expected = json!({
        "fsl": "1.0",
        "result": "violated",
        "spec": if action { "ZeroCapacityAction" } else { "ZeroCapacityProperty" },
        "violation_kind": "partial_op",
        "invariant": if action { "_partial_take" } else { "_partial_property_Capture" },
        "loc": if action { json!({"line": 5, "column": 19}) } else { json!({"line": 7, "column": 3}) },
        "hint": if action {
            "guard the action with requires q.size() > 0 (or bound the index)"
        } else {
            "guard the property expression with q.size() > 0 (or bound the index)"
        },
        "violated_at_step": usize::from(action),
        "violating_bindings": null,
        "last_action": if action { call } else { Value::Null },
        "trace": trace,
        "checked_to_depth": usize::from(action),
        "completeness": "bounded",
        "faithfulness_class": "partial_op_unguarded",
        "recommended_action": "add the missing guard / run bounded Monitor (replay)",
        "trace_type": "partial_op",
    });
    if engine == "explicit" {
        let object = expected.as_object_mut().expect("expected JSON object");
        object.insert("engine".to_owned(), json!("explicit"));
        object.insert("closure".to_owned(), json!(false));
        object.insert("states_explored".to_owned(), json!(1));
        object.insert("max_frontier_width".to_owned(), json!(1));
        object.insert("depth_reached".to_owned(), json!(usize::from(action)));
        object.insert(
            "action_profile".to_owned(),
            if action {
                json!({"take": {"enabled": 1, "fired": 0, "no_op": 0}})
            } else {
                json!({"idle": {"enabled": 0, "fired": 0, "no_op": 0}})
            },
        );
    }
    expected
}

fn evidence(cell: &str, action: bool, engine: &str) {
    let source = if action {
        action_source("0", "x = xs.head()")
    } else {
        property_source("0", "", "xs.head() == 1")
    };
    let fixture = Fixture::new(cell, &source);
    let (output, exit) = fixture.run(
        "verify",
        &[
            "--engine",
            engine,
            "--depth",
            "4",
            "--no-cache",
            "--deadlock",
            "ignore",
        ],
    );
    eprintln!("{cell}/{engine}: exit={exit}, envelope={output}");
    assert_eq!(exit, 1, "{cell}/{engine}: {output:#}");
    assert_eq!(
        stable_evidence(output),
        expected_evidence(action, engine),
        "{cell}/{engine}: no slot, invented state value or witness may escape"
    );
}

macro_rules! evidence_cell {
    ($cell:ident, $action:literal) => {
        mod $cell {
            use super::*;

            #[test]
            fn bmc() {
                evidence(stringify!($cell), $action, "bmc");
            }

            #[test]
            fn induction() {
                evidence(stringify!($cell), $action, "induction");
            }

            #[test]
            fn explicit() {
                evidence(stringify!($cell), $action, "explicit");
            }

            #[test]
            fn monitor() {
                if $action {
                    super::monitor(
                        stringify!($cell),
                        &action_source("0", "x = xs.head()"),
                        "take",
                        Some("_partial_take"),
                    );
                } else {
                    super::monitor(
                        stringify!($cell),
                        &property_source("0", "", "xs.head() == 1"),
                        "idle",
                        Some("_partial_property_Capture"),
                    );
                }
            }
        }
    };
}

evidence_cell!(l1_property_head, false);
evidence_cell!(l1_action_head, true);

// DM: ANS-mini-1135 accepts a narrow exception for decreases measures: reached
// empty capacity-zero reads must retain the base semantics error, not generate
// a rank value from a fresh read slot. Multiplication by zero is not a guard.
fn rank_source(capacity: &str, measure: &str) -> String {
    format!(
        "spec RankRead {{
  type K = 0..1
  state {{ xs: Seq<K, {capacity}>, n: 0..2 }}
  init {{ xs = Seq {{}}  n = 0 }}
  action tick() {{ requires n < 2  n = n + 1 }}
  action idle() {{ requires n == 2 }}
  leadsTo L {{ n < 2 ~> n == 2 decreases {measure} }}
}}"
    )
}

fn run_rank_or_frame(cell: &str, source: &str, engine: &str, extra: &[&str]) -> (Value, i32) {
    let fixture = Fixture::new(cell, source);
    let mut options = vec![
        "--engine",
        engine,
        "--depth",
        "4",
        "--no-cache",
        "--deadlock",
        "ignore",
    ];
    options.extend_from_slice(extra);
    let (output, exit) = fixture.run("verify", &options);
    eprintln!("{cell}/{engine}/{extra:?}: exit={exit}, envelope={output}");
    (output, exit)
}

fn assert_semantics_envelope(mut output: Value, exit: i32, message: &str) {
    // Errors have no cost object. Only versions is excluded: its verifier/solver
    // build identity is ambient (observed on base and head), not rank evidence.
    // Exact comparison of every remaining key rejects measure_value, CTI,
    // rank_failure, proof or any other new carrier of an unconstrained slot.
    let versions = output
        .as_object_mut()
        .expect("error object")
        .remove("versions")
        .expect("live versions metadata");
    assert!(versions.is_object(), "versions: {versions}");
    assert_eq!(exit, 2, "expected fail closed: {output:#}");
    assert_eq!(
        output,
        json!({
            "fsl": "1.0", "result": "error", "kind": "semantics",
            "message": message,
        }),
        "no rank value or success evidence may escape"
    );
}

fn expected_rank_envelope(engine: &str, decreases: &str) -> Value {
    let mut expected = json!({
        "fsl": "1.0", "spec": "RankRead", "result": "verified",
        "depth": 4, "checked_to_depth": 4, "completeness": "bounded",
        "invariants_checked": ["_bounds_xs", "_bounds_n"],
        "transitions_checked": [], "reachables": {},
        "action_coverage": {"idle": true, "tick": true},
        "deadlock": {"found": false},
        "leads_to": {"L": {"checked_to_depth": 4}},
        "warnings": [{
            "kind": "no_user_invariants",
            "message": "spec declares no user invariants (only implicit type bounds are checked)",
        }],
        "note": "bounded verification: no violation within depth 4",
    });
    if engine == "induction" {
        let object = expected.as_object_mut().expect("expected object");
        object.remove("depth");
        object.remove("deadlock");
        object.insert("result".to_owned(), json!("proved"));
        object.insert("engine".to_owned(), json!("induction"));
        object.insert("completeness".to_owned(), json!("unbounded"));
        object.insert("not_covered".to_owned(), json!(["int_overflow"]));
        object.insert(
            "k_used".to_owned(),
            json!({"_bounds_n": 1, "_bounds_xs": 1}),
        );
        object.insert("base_depth".to_owned(), json!(4));
        object.insert(
            "leads_to".to_owned(),
            json!({"L": {
                "checked_to_depth": 4, "proved": true,
                "completeness": "unbounded", "proof": "ranking",
                "decreases": decreases,
            }}),
        );
        object.insert(
            "note".to_owned(),
            json!("invariants and ranked leadsTo proved for all depths"),
        );
    }
    expected
}

fn rank_cell(
    cell: &str,
    capacity: &str,
    measure: &str,
    engine: &str,
    induction_error: Option<&str>,
    displayed_measure: &str,
) {
    let (output, exit) = run_rank_or_frame(cell, &rank_source(capacity, measure), engine, &[]);
    if engine == "explicit" {
        assert_semantics_envelope(
            output,
            exit,
            "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property",
        );
    } else if let Some(message) = induction_error.filter(|_| engine == "induction") {
        assert_semantics_envelope(output, exit, message);
    } else {
        assert_eq!(exit, 0, "{cell}/{engine}: {output:#}");
        // Reuse L1's live cost/versions checks. Observed timings, solver counts
        // and memory vary, but all stable fields, including rank evidence, match.
        assert_eq!(
            stable_evidence(output),
            expected_rank_envelope(engine, displayed_measure),
            "{cell}/{engine}: complete stable rank envelope"
        );
    }
}

macro_rules! decreases_cell {
    ($cell:ident, $capacity:literal, $measure:literal, $error:expr, $display:literal) => {
        mod $cell {
            use super::*;

            #[test]
            fn bmc() {
                rank_cell(
                    stringify!($cell),
                    $capacity,
                    $measure,
                    "bmc",
                    $error,
                    $display,
                );
            }

            #[test]
            fn induction() {
                rank_cell(
                    stringify!($cell),
                    $capacity,
                    $measure,
                    "induction",
                    $error,
                    $display,
                );
            }

            #[test]
            fn explicit() {
                rank_cell(
                    stringify!($cell),
                    $capacity,
                    $measure,
                    "explicit",
                    $error,
                    $display,
                );
            }
        }
    };
}

decreases_cell!(
    dm1_direct_head,
    "0",
    "xs.head()",
    Some("head() on zero-capacity sequence"),
    "xs.head()"
);
decreases_cell!(
    dm2_masked_head,
    "0",
    "2 - n + 0 * xs.head()",
    Some("head() on zero-capacity sequence"),
    "(2 - n + 0 * xs.head())"
);
decreases_cell!(
    dm3_direct_at,
    "0",
    "xs.at(0)",
    Some("cannot select from an empty finite domain"),
    "xs.at(0)"
);
decreases_cell!(
    dm4_direct_index,
    "0",
    "xs[0]",
    Some("cannot select from an empty finite domain"),
    "xs[0]"
);
// A push into an empty capacity-one Seq is inside capacity: its temporary has
// the real head 0, so 2-n+head is nonnegative and decreases on every pending tick.
decreases_cell!(
    dm5_within_capacity_push,
    "1",
    "2 - n + xs.push(0).head()",
    None,
    "(2 - n + xs.push(0).head())"
);
// Short-circuiting, unlike 0*read, makes this capacity-zero read unreachable.
decreases_cell!(
    dm6_unreached_head,
    "0",
    "if xs.size() > 0 then xs.head() else 2 - n",
    None,
    "if xs.size() > 0 then xs.head() else 2 - n"
);

// CF: capacity-zero frames with len>0 must reach #1519's F/missing-slot gates.
// If those gates disappear, read definedness can be true and a fresh slot can
// decide a property/rank verdict. Standalone selection must still fail closed;
// induction first reports any reachable bound its base now checks (#1598).
const CF1_EXCLUDED_BOUNDS: &str = "spec F04 {
  type K = 0..1
  state { xs: Seq<K, 0> }
  init { xs = Seq {} }
  action grow() { requires xs.size() == 0  xs = xs.push(1) }
  action idle() { }
  reachable R { xs.size() > 0 and xs.head() == 0 }
  invariant I { xs.size() > 0 => xs.head() == 1 }
}";

const CF2_STEP_SUCCESSOR: &str = "spec S04 {
  type K = 0..1
  state { xs: Seq<K, 0>, go: Bool }
  init { xs = Seq { }  go = false }
  action take() { requires go  xs = xs.push(1) }
  action idle() { requires not go }
  invariant P { xs.size() == 0 or xs.head() == 1 }
}";

const CF3_RANK_SUCCESSOR: &str = "spec R01 {
  type K = 0..1
  state { xs: Seq<K, 0>, go: Bool }
  init { xs = Seq {} go = false }
  action take() { requires go xs = xs.push(1) go = false }
  action idle() { requires not go }
  leadsTo L {
    go and xs.size() == 0 ~> xs.size() == 1 and xs.head() == 1
    decreases 0
  }
}";

const CF4_PROPERTY_OVERFLOW: &str = "spec F01 {
  type K = 0..1
  state { xs: Seq<K, 0>, x: K }
  init { xs = Seq {}  x = 0 }
  action grow() { xs = xs.push(1) }
  invariant I { xs.size() > 0 => xs.head() == 0 }
  invariant J { x == 0 }
}";

fn assert_frame_closed(cell: &str, source: &str, engine: &str, options: &[&str]) {
    let (output, exit) = run_rank_or_frame(cell, source, engine, options);
    assert_semantics_envelope(output, exit, "model sequence length exceeds capacity");
}

#[test]
fn cf1_excluded_bounds_reachable_bmc() {
    // Concrete grow stores 1, never 0. Without F, a free head could falsely
    // satisfy R and yield successful verified instead of rejecting the frame.
    assert_frame_closed("cf1_r", CF1_EXCLUDED_BOUNDS, "bmc", &["--property", "R"]);
}

#[test]
fn cf1_excluded_bounds_invariant_bmc() {
    assert_frame_closed("cf1_i", CF1_EXCLUDED_BOUNDS, "bmc", &["--property", "I"]);
}

#[test]
fn cf2_induction_step_successor() {
    // go is unreachable, but the induction step may choose it. Its successor
    // has len=1 and no state slots; a free head cannot be a genuine P CTI.
    assert_frame_closed("cf2", CF2_STEP_SUCCESSOR, "induction", &[]);
}

#[test]
fn cf2_selected_induction_step_successor() {
    // The reachable base is safe; selected induction must still reject the
    // arbitrary step's overflowing frame rather than fabricating a head value.
    for _ in 0..2 {
        assert_frame_closed(
            "cf2_selected",
            CF2_STEP_SUCCESSOR,
            "induction",
            &["--property", "P"],
        );
    }
}

#[test]
fn cf3_ranked_leadsto_successor_k2() {
    // k=2 reaches the ranked obligation rather than the k=1 bound CTI. Missing
    // successor storage must not turn a free head into ranked proof/failure.
    assert_frame_closed("cf3", CF3_RANK_SUCCESSOR, "induction", &["--k", "2"]);
}

#[test]
fn cf4_property_overflow_bmc() {
    assert_frame_closed("cf4", CF4_PROPERTY_OVERFLOW, "bmc", &["--property", "I"]);
}

#[test]
fn cf4_property_overflow_induction() {
    // #1598: the base rejects the reachable bound before evaluating I there.
    let (value, exit) = run_rank_or_frame(
        "cf4",
        CF4_PROPERTY_OVERFLOW,
        "induction",
        &["--property", "I"],
    );
    assert_eq!(
        (
            exit,
            &value["result"],
            &value["violation_kind"],
            &value["invariant"],
            &value["violated_at_step"]
        ),
        (
            1,
            &json!("violated"),
            &json!("type_bound"),
            &json!("_bounds_xs"),
            &json!(1)
        ),
        "{value}"
    );
    assert_eq!(value["completeness"], "bounded", "{value}");
}

// DM7/DM8 exercise the reached direction of conditional/or path selection.
// DM9/DM10 isolate successor/deadlock measure evaluation: neither read can be
// reached from init, but induction must still reject it in its arbitrary frame.
const DM9_STATE1_ONLY: &str = "spec S1 {
  type K = 0..1
  state { xs: Seq<K, 0>, n: 0..2, go: Bool, done: Bool }
  init { xs = Seq {}  n = 0  go = false  done = false }
  action tick() { requires n < 2 and not go  n = n + 1 }
  action idle() { requires n == 2 and not go }
  action take() { requires go  done = true  go = false }
  invariant ND { not done }
  leadsTo L { n < 2 ~> n == 2 decreases if done then xs.head() else 2 - n }
}";

const DM10_DEADLOCK_ONLY: &str = "spec D1 {
  type K = 0..1
  state { xs: Seq<K, 0>, n: 0..2, dead: Bool }
  init { xs = Seq {}  n = 0  dead = false }
  action tick() { requires n < 2 and not dead  n = n + 1 }
  action idle() { requires n == 2 and not dead }
  leadsTo L { n < 2 ~> n == 2 decreases if dead then xs.head() else 2 - n }
}";

fn expected_reached_rank_bmc(spec: &str) -> Value {
    let mut expected = expected_rank_envelope("bmc", "");
    expected["spec"] = json!(spec);
    if spec == "S1" {
        // go remains false from init, so take is never enabled and ND holds.
        // Bool type bounds do not appear in invariants_checked.
        expected["invariants_checked"] = json!(["_bounds_xs", "_bounds_n", "ND"]);
        let hint = "these requires clauses are unsatisfiable at every step up to depth 4; weaken one of them, add an action that establishes them, or increase --depth";
        expected["action_coverage"]["take"] = json!({
            "covered": false, "blocking_requires": [], "hint": hint,
            "faithfulness_class": "intent_unexercised",
            "recommended_action": "add a single-shot reachable for the action / raise --depth",
        });
        expected["warnings"] = json!([{
            "kind": "never_enabled_action", "name": "take",
            "loc": {"line": 7, "column": 3},
            "message": "action 'take' is never enabled within depth 4 — the spec may be vacuous (check its requires clauses)",
            "hint": hint, "blocking_requires": [],
        }]);
    }
    expected
}

macro_rules! reached_decreases_cell {
    ($cell:ident, $source:expr, $spec:literal, $induction_options:expr) => {
        mod $cell {
            use super::*;

            #[test]
            fn check() {
                let source = $source;
                super::check(stringify!($cell), source.as_ref());
            }

            #[test]
            fn bmc() {
                let source = $source;
                let (output, exit) =
                    run_rank_or_frame(stringify!($cell), source.as_ref(), "bmc", &[]);
                assert_eq!(exit, 0, "{}: {output:#}", stringify!($cell));
                assert_eq!(
                    stable_evidence(output),
                    expected_reached_rank_bmc($spec),
                    "{}: complete stable BMC envelope",
                    stringify!($cell),
                );
            }

            #[test]
            fn induction() {
                let source = $source;
                let (output, exit) = run_rank_or_frame(
                    stringify!($cell),
                    source.as_ref(),
                    "induction",
                    $induction_options,
                );
                // Full error comparison rejects a proof, CTI before/after,
                // deadlock measure_value, or any other leaked fresh slot value.
                assert_semantics_envelope(output, exit, "head() on zero-capacity sequence");
            }

            #[test]
            fn explicit() {
                let source = $source;
                let (output, exit) =
                    run_rank_or_frame(stringify!($cell), source.as_ref(), "explicit", &[]);
                assert_semantics_envelope(
                    output,
                    exit,
                    "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property",
                );
            }
        }
    };
}

// size>0 is false: the else read is reached even though multiplied by zero.
reached_decreases_cell!(
    dm7_else_reached,
    rank_source(
        "0",
        "if xs.size() > 0 then 2 - n else 2 - n + 0 * xs.head()"
    ),
    "RankRead",
    &[]
);
// A false left operand of or must evaluate its right operand, not skip it.
reached_decreases_cell!(
    dm8_or_reached,
    rank_source("0", "if xs.size() > 0 or xs.head() == 1 then 2 - n else 0"),
    "RankRead",
    &[]
);
// k=2 proves ND before ranking. In the rank step, ND excludes done at state0,
// but go=true permits take to set done at state1 and reach the head there only.
reached_decreases_cell!(dm9_state1_only, DM9_STATE1_ONLY, "S1", &["--k", "2"]);
// Enabled steps force not dead at both ends; only an arbitrary pending deadlock
// frame can have dead=true and reach this measure's head.
reached_decreases_cell!(dm10_deadlock_only, DM10_DEADLOCK_ONLY, "D1", &[]);
