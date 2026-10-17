// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native contract for #1254: a sync action's arguments are substituted into
//! the component action with the same scoping rules the expression
//! quantifiers already follow -- a statement `forall`'s where / range is
//! substituted, its binding shadows a same-named parameter, and neither a
//! `forall` binding nor a `let` captures an argument that mentions its name.
//!
//! Before the fix the statement `forall` binder was passed through untouched
//! (exit 2 on `lim`), its body was substituted as if the binding did not
//! exist (exit 2 on write-index distinctness), and -- the false passes -- a
//! sync argument spelled like a component's `forall k` or `let y` was
//! captured, so the compose reported `verified` for an action that violates
//! the component invariant. The semantics asserted here is "the component
//! action with the arguments plugged in": every violating test has a flat or
//! differently-spelled control in the issue that is violated the same way.
//!
//! Every test here fails on `origin/main` (ade95232).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn scratch(files: &[(&str, &str)]) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace")
        .join(format!("target/issue-1254-{}-{id}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    for (name, source) in files {
        std::fs::write(dir.join(name), source).expect("write fixture");
    }
    dir
}

fn fslc(dir: &Path, args: &[&str]) -> (Value, i32) {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run native fslc");
    let json = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (json, output.status.code().expect("exit status"))
}

fn verify(dir: &Path, file: &str) -> (Value, i32) {
    fslc(dir, &["verify", file, "--depth", "3", "--no-cache"])
}

fn assert_violated(json: &Value, code: i32, invariant: &str) {
    assert_eq!(code, 1, "{json}");
    assert_eq!(json["result"], "violated", "{json}");
    assert_eq!(json["invariant"], invariant, "{json}");
}

/// `fill(lim)` with a `where` that reads the parameter: `fill(1)` sets
/// `m[0]`.
const WHERE_STORE: &str = "spec Store {
  type K = 0..2
  state { m: Map<K, Bool> }
  init { forall k: K { m[k] = false } }
  action fill(lim: K) {
    forall k: K where k < lim { m[k] = true }
  }
  invariant ZeroNeverFilled { m[0] == false }
}
";

/// Where: the binder's `where` is substituted, so `lim` no longer reaches
/// the Kernel unbound (was exit 2) and `go(1)` violates the invariant.
#[test]
fn where_clause_of_statement_forall_reads_the_argument() {
    let dir = scratch(&[
        ("store.fsl", WHERE_STORE),
        (
            "sys.fsl",
            "compose Sys {
  use Store as st from \"store.fsl\"
  internal st.fill
  state { n: Int }
  init { n = 0 }
  action go(x: st.K) = st.fill(x) { n = 0 }
}
",
        ),
    ]);
    let (json, code) = verify(&dir, "sys.fsl");
    assert_violated(&json, code, "st.ZeroNeverFilled");
}

/// Where, false-pass shape: the compose declares its own `lim`, which the
/// unsubstituted `where` used to read (always 0, so nothing was filled).
#[test]
fn where_clause_is_not_captured_by_same_named_compose_state() {
    let dir = scratch(&[
        ("store.fsl", WHERE_STORE),
        (
            "sys.fsl",
            "compose Sys {
  use Store as st from \"store.fsl\"
  internal st.fill
  state { lim: Int }
  init { lim = 0 }
  action go(x: st.K) = st.fill(x) { lim = 0 }
}
",
        ),
    ]);
    let (json, code) = verify(&dir, "sys.fsl");
    assert_violated(&json, code, "st.ZeroNeverFilled");
}

/// Range: the binder's range bounds are substituted too. Verifying a
/// non-constant range bound is unsupported for the component on its own, so
/// this is a `check`, which the component passes and the compose now does.
#[test]
fn range_bound_of_statement_forall_reads_the_argument() {
    let dir = scratch(&[
        (
            "store.fsl",
            "spec StoreR {
  type K = 0..2
  state { m: Map<K, Bool> }
  init { forall k: K { m[k] = false } }
  action fill(lim: K) {
    forall k in 0..lim { m[k] = true }
  }
}
",
        ),
        (
            "sys.fsl",
            "compose Sys {
  use StoreR as st from \"store.fsl\"
  state { n: Int }
  init { n = 0 }
  action go(x: st.K) = st.fill(x) { n = n + 1 }
}
",
        ),
    ]);
    let (json, code) = fslc(&dir, &["check", "sys.fsl"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["result"], "ok", "{json}");
}

/// Shadowing: a `forall k` shadows the parameter `k`, so the body still
/// writes every key (was exit 2: `m[k]` became `m[x]` in every iteration).
/// Writing every key makes `m[2]` true, which the invariant forbids.
#[test]
fn statement_forall_binding_shadows_the_parameter() {
    let dir = scratch(&[
        (
            "shadow.fsl",
            "spec Shadow {
  type K = 0..2
  state { m: Map<K, Bool> }
  init { forall k: K { m[k] = false } }
  action mark_all(k: K) {
    requires k == 0
    forall k: K { m[k] = true }
  }
  invariant TwoNeverMarked { m[2] == false }
}
",
        ),
        (
            "sys.fsl",
            "compose Sys {
  use Shadow as sh from \"shadow.fsl\"
  state { n: Int }
  init { n = 0 }
  action go(x: sh.K) = sh.mark_all(x) { n = 0 }
}
",
        ),
    ]);
    let (json, code) = verify(&dir, "sys.fsl");
    assert_violated(&json, code, "sh.TwoNeverMarked");
}

/// Capture by a `forall` binding: the argument is spelled `k`, like the
/// component's binder. Captured, `m[k] = (k < k)` wrote false everywhere and
/// the compose verified; `fill(1)` really sets `m[0]`.
#[test]
fn statement_forall_binding_does_not_capture_the_argument() {
    let dir = scratch(&[
        (
            "store.fsl",
            "spec Store {
  type K = 0..2
  state { m: Map<K, Bool> }
  init { forall k: K { m[k] = false } }
  action fill(lim: K) {
    forall k: K { m[k] = (k < lim) }
  }
  invariant ZeroNeverFilled { m[0] == false }
}
",
        ),
        (
            "sys.fsl",
            "compose Sys {
  use Store as st from \"store.fsl\"
  internal st.fill
  state { n: Int }
  init { n = 0 }
  action go(k: st.K) = st.fill(k) { n = 0 }
}
",
        ),
    ]);
    let (json, code) = verify(&dir, "sys.fsl");
    assert_violated(&json, code, "st.ZeroNeverFilled");
}

/// A component whose `let y` sits between the parameter and its use.
const LET_COMPONENT: &str = "spec LetC {
  type K = 0..2
  state { hit: Bool }
  init { hit = false }
  action poke(lim: K) {
    let y = 0
    hit = (lim > y)
  }
  invariant NeverHit { hit == false }
}
";

/// Capture by a `let`: the argument is spelled `y`. Captured,
/// `hit = (y > y)` never fired; `poke(1)` really sets `hit`.
#[test]
fn let_binding_does_not_capture_the_argument() {
    let dir = scratch(&[
        ("letc.fsl", LET_COMPONENT),
        (
            "sys.fsl",
            "compose Sys {
  use LetC as c from \"letc.fsl\"
  internal c.poke
  state { n: Int }
  init { n = 0 }
  action go(y: c.K) = c.poke(y) { n = 0 }
}
",
        ),
    ]);
    let (json, code) = verify(&dir, "sys.fsl");
    assert_violated(&json, code, "c.NeverHit");
}

/// The component's `let y` is spliced before the compose's own body, which
/// reads the sync parameter `y`: the `let` must not take it over either.
#[test]
fn let_binding_does_not_capture_the_compose_body_parameter() {
    let dir = scratch(&[
        ("letc.fsl", LET_COMPONENT),
        (
            "sys.fsl",
            "compose Sys {
  use LetC as c from \"letc.fsl\"
  internal c.poke
  state { n: Int }
  init { n = 0 }
  action go(y: c.K) = c.poke(0) { n = y }
  invariant NZero { n == 0 }
}
",
        ),
    ]);
    let (json, code) = verify(&dir, "sys.fsl");
    assert_violated(&json, code, "NZero");
}
