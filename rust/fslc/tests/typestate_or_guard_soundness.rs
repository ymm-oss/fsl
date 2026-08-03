// SPDX-License-Identifier: Apache-2.0

//! Soundness controls for `fslc typestate`'s `or`-guard handling (issue
//! #521). A state comparison appearing in only one arm of an `or` must not
//! be treated as a sufficient local from-state guard — the checked FSL
//! model can still reach the transition through the other arm, so treating
//! it as `derivable` emits a ghost type that excludes accepted behavior.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn run_typestate(source: &str, extra: &[&str]) -> Vec<u8> {
    let dir = tempfile_dir();
    let path = dir.join("spec.fsl");
    std::fs::write(&path, source).expect("write fixture spec");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("typestate")
        .arg(&path)
        .args(extra)
        .output()
        .expect("run native typestate CLI");
    assert!(
        output.status.success(),
        "typestate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn run_typestate_json(source: &str) -> Value {
    let stdout = run_typestate(source, &[]);
    serde_json::from_slice(&stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={}",
            String::from_utf8_lossy(&stdout)
        )
    })
}

fn tempfile_dir() -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "fslc-typestate-or-guard-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch dir");
    directory
}

const MIXED_OR_SPEC: &str = r"
spec MixedOr {
  enum St { A, B, C }
  struct Item { status: St }
  state { item: Item, bypass: Bool }
  init { item = Item { status: B }  bypass = true }
  action close() {
    requires item.status == A or bypass
    item.status = C
  }
}
";

/// Negative control for #521: `close()` is reachable from `B` through
/// `bypass`, so it must not be reported `derivable`, must not push
/// `applicability` to `full`, and `--ts` must not narrow `self` to
/// `Item<"A">` — a type that would forbid the very call the model permits.
#[test]
fn mixed_or_guard_is_not_derivable_and_not_narrowed_in_the_type() {
    let report = run_typestate_json(MIXED_OR_SPEC);
    let entity = &report["entities"][0];
    assert_eq!(entity["applicability"], "none");
    let action = &entity["actions"][0];
    assert_eq!(action["verdict"], "relational");
    assert_eq!(action["transitions"][0]["from"], Value::Array(vec![]));

    let ts_bytes = run_typestate(MIXED_OR_SPEC, &["--ts"]);
    let ts = String::from_utf8(ts_bytes).expect("utf8 typescript");
    assert!(
        !ts.contains("Item<\"A\">"),
        "typestate must not narrow `close` to Item<\"A\">; the model accepts \
         it from B via `bypass`: {ts}"
    );
    assert!(
        !ts.contains("export function close"),
        "close() has no sound local guard and must stay untyped: {ts}"
    );
}

/// Positive control for #521: every disjunct pinning the *same* state is
/// still a sound local guard and must remain `derivable`.
#[test]
fn or_of_the_same_state_remains_derivable() {
    let source = r"
spec OrSameState {
  enum St { A, B, C }
  struct Item { status: St }
  state { item: Item }
  init { item = Item { status: A } }
  action close() {
    requires item.status == A or item.status == A
    item.status = C
  }
}
";
    let report = run_typestate_json(source);
    let entity = &report["entities"][0];
    assert_eq!(entity["applicability"], "full");
    let action = &entity["actions"][0];
    assert_eq!(action["verdict"], "derivable");
    assert_eq!(action["transitions"][0]["from"], serde_json::json!(["A"]));
}

/// Positive control for #521: when *every* disjunct constrains the entity —
/// even to *different* states — the whole `or` still pins the entity, and
/// the sound from-state set is the union of what each disjunct implies.
/// `status == A or status == B` guarantees `status ∈ {A, B}` for every
/// satisfying trace, so it must stay `derivable` with `from` containing
/// both states (this is what distinguishes a real per-disjunct constraint
/// from `status == A or bypass`, where `bypass` constrains nothing).
#[test]
fn or_of_distinct_states_remains_derivable() {
    let source = r"
spec OrDistinctStates {
  enum St { A, B, C }
  struct Item { status: St }
  state { item: Item }
  init { item = Item { status: A } }
  action close() {
    requires item.status == A or item.status == B
    item.status = C
  }
}
";
    let report = run_typestate_json(source);
    let entity = &report["entities"][0];
    assert_eq!(entity["applicability"], "full");
    let action = &entity["actions"][0];
    assert_eq!(action["verdict"], "derivable");
    assert_eq!(
        action["transitions"][0]["from"],
        serde_json::json!(["A", "B"])
    );
}

/// Positive control for #521: `and` keeps its existing union semantics —
/// one conjunct pinning the state is still a sound guard even when another
/// conjunct adds an unrelated condition.
#[test]
fn and_guard_remains_derivable() {
    let source = r"
spec AndGuard {
  enum St { A, B, C }
  struct Item { status: St }
  state { item: Item, flag: Bool }
  init { item = Item { status: A }  flag = true }
  action close() {
    requires item.status == A and flag
    item.status = C
  }
}
";
    let report = run_typestate_json(source);
    let entity = &report["entities"][0];
    assert_eq!(entity["applicability"], "full");
    let action = &entity["actions"][0];
    assert_eq!(action["verdict"], "derivable");
    assert_eq!(action["transitions"][0]["from"], serde_json::json!(["A"]));
}

/// A locally guarded read/query operation is a real typestate operation even
/// when FSL's frame semantics leave the state field unchanged. It must remain
/// visible as a self-loop instead of disappearing from an otherwise `full`
/// report.
#[test]
fn locally_guarded_state_preserving_action_is_an_explicit_self_loop() {
    let source = r"
spec StatePreservingAction {
  enum St { A, B }
  struct Item { status: St }
  state { item: Item, flag: Bool }
  init { item = Item { status: A } flag = true }
  action go() {
    requires item.status == A
    item.status = B
  }
  action probe() {
    requires item.status == B
  }
  action unrelated() {
    requires flag
  }
}
";
    let report = run_typestate_json(source);
    let entity = &report["entities"][0];
    assert_eq!(entity["applicability"], "full");
    let actions = entity["actions"].as_array().expect("actions");
    assert_eq!(
        actions.len(),
        2,
        "unrelated actions are not entity operations"
    );
    let probe = actions
        .iter()
        .find(|action| action["action"] == "probe")
        .expect("state-preserving probe");
    assert_eq!(probe["verdict"], "derivable");
    assert_eq!(probe["state_preserving"], true);
    assert_eq!(
        probe["transitions"],
        serde_json::json!([{
            "entity": "item",
            "from": ["B"],
            "to": "B",
            "conditional": false
        }])
    );

    let ts = String::from_utf8(run_typestate(source, &["--ts"])).expect("utf8 typescript");
    assert!(
        ts.contains("export function probe<S extends \"B\">(self: Item<S>): Item<S>;"),
        "state-preserving methods must retain the exact phantom state: {ts}"
    );
    assert!(!ts.contains("export function unrelated"));
}

/// When a state-preserving operation is legal in more than one state, its
/// return type must preserve the caller's exact state rather than widen an A
/// handle to A|B.
#[test]
fn multi_state_self_loop_preserves_phantom_identity() {
    let source = r"
spec MultiStatePreservingAction {
  enum St { A, B, C }
  struct Item { status: St }
  state { item: Item }
  init { item = Item { status: A } }
  action inspect() {
    requires item.status == A or item.status == B
  }
}
";
    let report = run_typestate_json(source);
    let action = &report["entities"][0]["actions"][0];
    assert_eq!(action["state_preserving"], true);
    assert_eq!(action["transitions"].as_array().map(Vec::len), Some(2));

    let ts = String::from_utf8(run_typestate(source, &["--ts"])).expect("utf8 typescript");
    assert!(
        ts.contains("export function inspect<S extends \"A\" | \"B\">(self: Item<S>): Item<S>;"),
        "multi-state self-loops must preserve S: {ts}"
    );
    assert!(!ts.contains("): Item<\"A\" | \"B\">;"));
}
