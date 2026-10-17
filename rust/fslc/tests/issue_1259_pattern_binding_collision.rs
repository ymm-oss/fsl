// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1259 (item 1 of #1157): `x is some(v)` whose `v` collides with a
//! name already in scope is a located `check` error.
//!
//! Before this, `check` accepted `p is some(exact) or exact <= 2` with a state
//! variable `exact`: the concrete engines read the state variable on the right
//! of `or` when `p` is absent, the symbolic ones read the absent payload, and
//! bmc/induction reported a false `violated` (or a false "undefined") where
//! explicit proved the spec. The reproducers below are the issue's.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn fixture(tag: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fslc-issue-1259-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let path = dir.join("spec.fsl");
    std::fs::write(&path, text).expect("write fixture");
    path
}

fn run(command: &str, path: &PathBuf, extra: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(command)
        .arg(path)
        .args(extra)
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// `check` and every `verify` engine reject `text` with a semantics error
/// naming the pattern variable and what it collides with, at `line`.
fn assert_rejected(tag: &str, text: &str, binding: &str, collides_with: &str, line: u64) {
    let path = fixture(tag, text);
    let mut runs = vec![run("check", &path, &[])];
    for engine in ["explicit", "bmc", "induction"] {
        runs.push(run("verify", &path, &["--no-cache", "--engine", engine]));
    }
    for (report, code) in runs {
        assert_eq!(code, 2, "{tag}: {report:#}");
        assert_eq!(report["result"], "error", "{tag}: {report:#}");
        assert_eq!(report["kind"], "semantics", "{tag}: {report:#}");
        let message = report["message"].as_str().unwrap_or_default();
        assert!(
            message.contains(&format!("pattern binding '{binding}'"))
                && message.contains(collides_with)
                && message.contains("rename the pattern variable"),
            "{tag}: {report:#}"
        );
        assert_eq!(report["loc"]["line"], line, "{tag}: {report:#}");
    }
}

const ABSENT_BIND_FALSE: &str = r"spec AbsentBindFalse {
  type K = 0..2
  state {
    p: Option<K>,
    exact: K
  }
  init {
    p = none
    exact = 0
  }
  action set(k: K) {
    p = some(k)
  }
  action clear() {
    p = none
  }
  invariant Y { p is some(exact) or exact <= 2 }
}
";

const ABSENT_BIND_STATE: &str = r"spec AbsentBindState {
  type K = 0..2
  state {
    p: Option<K>,
    exact: K,
    m: Map<K, Int>
  }
  init {
    p = none
    exact = 0
    forall k: K { m[k] = 0 }
  }
  action set(k: K) {
    p = some(k)
  }
  action clear() {
    p = none
  }
  invariant X { p is some(exact) or m[exact] >= 0 }
}
";

/// The issue's two reproducers: bmc/induction answered `violated` and
/// "undefined" where explicit answered `proved`.
#[test]
fn the_issue_reproducers_are_rejected_at_check() {
    assert_rejected("false", ABSENT_BIND_FALSE, "exact", "a state variable", 17);
    assert_rejected("state", ABSENT_BIND_STATE, "exact", "a state variable", 19);
}

/// The same split in a guard (`requires`) and in a `reachable`.
#[test]
fn a_state_variable_collision_is_rejected_in_guards_and_reachables() {
    let guard = ABSENT_BIND_FALSE
        .replace(
            "  action clear() {\n    p = none",
            "  action clear() {\n    requires p is some(exact) or exact == 2\n    p = none",
        )
        .replace(
            "invariant Y { p is some(exact) or exact <= 2 }",
            "invariant Y { exact <= 2 }",
        );
    assert_rejected("guard", &guard, "exact", "a state variable", 14);
    let reachable = ABSENT_BIND_FALSE.replace(
        "invariant Y { p is some(exact) or exact <= 2 }",
        "reachable R { p is some(exact) or exact == 2 }",
    );
    assert_rejected("reachable", &reachable, "exact", "a state variable", 17);
}

/// The other names #1157 lists, plus constants and enum members (also
/// shadowable by a binding before this change), and binding one new name twice
/// (first-wins natively, last-wins in Python; #1157 notes this makes it moot).
#[test]
fn every_other_name_in_scope_is_protected() {
    let spec = |items: &str| {
        format!(
            "spec Scope {{
  type K = 0..2
  enum Color {{ Red, Green }}
  const LIMIT = 2
  state {{ p: Option<K>, q: Option<K>, c: Option<Color>, n: Int }}
  init {{ p = none  q = none  c = none  n = 0 }}
{items}
}}
"
        )
    };
    let in_scope = "a name already in scope";
    let cases = [
        (
            "const",
            "  invariant I { p is some(LIMIT) => LIMIT <= 2 }",
            "LIMIT",
            "a constant",
        ),
        (
            "enum",
            "  invariant I { c is some(Red) => n == 0 }",
            "Red",
            "an enum member",
        ),
        (
            "param",
            "  action a(k: K) { requires p is some(k)  n = k }",
            "k",
            in_scope,
        ),
        (
            "binder",
            "  invariant I { forall k: K { p is some(k) => k <= 2 } }",
            "k",
            in_scope,
        ),
        (
            "let",
            "  action a() { let k = 0  requires p is some(k)  n = k }",
            "k",
            in_scope,
        ),
        (
            "twice",
            "  invariant I { p is some(v) and q is some(v) => v <= 2 }",
            "v",
            in_scope,
        ),
        (
            "across-requires",
            "  action a() { requires p is some(v)  requires q is some(v)  n = v }",
            "v",
            in_scope,
        ),
    ];
    for (tag, item, binding, collides_with) in cases {
        assert_rejected(tag, &spec(item), binding, collides_with, 7);
    }
}

/// Over-rejection control: a fresh name, the same fresh name in separate
/// properties, and the section 9 guard-to-body idiom still check.
#[test]
fn fresh_pattern_names_still_check() {
    let path = fixture(
        "fresh",
        r"spec Fresh {
  type K = 0..2
  state { p: Option<K>, q: Option<K>, n: Int }
  init { p = none  q = none  n = 0 }
  action a() { requires p is some(v)  n = v }
  action b() { requires q is some(v)  n = v }
  invariant I { p is some(v) => v <= 2 }
  invariant J { q is some(v) and p is some(w) => v + w <= 4 }
}
",
    );
    let (report, code) = run("check", &path, &[]);
    assert_eq!(code, 0, "{report:#}");
    assert_eq!(report["result"], "ok", "{report:#}");
}
