// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #570: `true`, `false`, and `none` were
//! accepted as declaration names. Every occurrence of those words in an
//! expression resolves to a literal, so the declaration became unreadable and
//! the author's property silently became something else —
//! `invariant AlwaysHolds { true }` over `state { true: Bool }` came back
//! `proved`, with `--engine explicit` agreeing, while `init { true = false }`
//! assigned a variable no expression could read.
//!
//! The reserved set is derived from `fsl_syntax::syntax_expr`'s `atom()`: those
//! three identifiers are matched unconditionally and become literals. `some`,
//! `Set`, `Seq`, `unique`, `exactlyOne`, `forall`, and `exists` are matched
//! only before more syntax, so a bare reference is a loud parse error rather
//! than a silent literal, and `count`/`sum`/`stage`/`in`/`is`/`where`/`old`/
//! `abs`/`and`/`or` were measured to read back correctly. None are reserved.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn scratch_dir() -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "issue-570-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&path).expect("create scratch directory");
    path
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(workspace_root())
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

/// Write `source` to a scratch spec and return its path.
fn spec(name: &str, source: &str) -> String {
    let path = scratch_dir().join(format!("{name}.fsl"));
    std::fs::write(&path, source).expect("write scratch spec");
    path.display().to_string()
}

const SHADOW_TRUE: &str = "spec ShadowTrue {
  state { true: Bool }
  init { true = false }
  action noop() { true = true }
  invariant AlwaysHolds { true }
}
";

// --- negative: the reproduction is rejected, on check *and* on the verdict --

#[test]
fn a_reserved_state_variable_is_a_check_error() {
    let path = spec("shadow_true_check", SHADOW_TRUE);
    let (value, status) = run(&["check", &path]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error", "{value}");
    let message = value["message"].as_str().expect("message");
    assert!(
        message.contains("'true' is a reserved FSL keyword"),
        "{message}"
    );
    assert!(message.contains("state variable"), "{message}");
    // Issue 555 gives the diagnostic a position; without one a repair agent
    // cannot find the declaration.
    assert_eq!(value["loc"]["line"], 2, "{value}");
}

#[test]
fn no_engine_returns_a_verdict_for_a_reserved_declaration() {
    // The load-bearing control. Correct classification from `check` is not
    // enough: if any verdict path still accepts the spec, the false green
    // survives there. Both engines returned `verified`/`proved` before.
    let path = spec("shadow_true_verify", SHADOW_TRUE);
    for engine in ["bmc", "explicit"] {
        let (value, status) = run(&["verify", &path, "--depth", "3", "--engine", engine]);
        assert_eq!(value["result"], "error", "engine {engine}: {value}");
        assert_eq!(status, 2, "engine {engine}: {value}");
        assert!(
            value.get("completeness").is_none(),
            "engine {engine}: {value}"
        );
    }
}

#[test]
fn the_error_gallery_fixture_is_rejected() {
    let (value, status) = run(&[
        "check",
        "examples/gallery/errors/name_reserved_declaration.fsl",
    ]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["result"], "error", "{value}");
}

// --- one control per identifier-introducing position ----------------------

#[test]
fn every_declaration_position_rejects_a_reserved_word() {
    // Measured on the pre-fix binary: every one of these returned `ok` with
    // exit 0 except `action parameter` and `binder`, which errored only
    // incidentally — a type mismatch from the literal, not a name check.
    let cases: &[(&str, &str, &str)] = &[
        (
            "state variable",
            "state variable",
            "spec S { state { true: Bool } init { true = false } invariant I { true } }",
        ),
        (
            "const",
            "const",
            "spec S { const false = 3\n state { x: Int } init { x = 0 } action a() { x = 1 } invariant I { x >= 0 } }",
        ),
        (
            "type",
            "type",
            "spec S { type none = 0..3\n state { x: none } init { x = 0 } action a() { x = 1 } invariant I { x >= 0 } }",
        ),
        (
            "enum",
            "enum",
            "spec S { enum true { A, B } state { x: true } init { x = A } action a() { x = B } invariant I { x == A } }",
        ),
        (
            "enum member",
            "enum member",
            "spec S { enum E { true, B } state { x: E } init { x = B } action a() { x = B } invariant I { x == B } }",
        ),
        (
            "struct",
            "struct",
            "spec S { struct none { f: Bool } state { x: Bool } init { x = false } action a() { x = true } invariant I { x or not x } }",
        ),
        (
            "struct field",
            "struct field",
            "spec S { struct T { true: Bool } state { x: T } init { x = T { true: false } } action a() { x = T { true: true } } invariant I { x.true } }",
        ),
        (
            "action",
            "action",
            "spec S { state { x: Bool } init { x = false } action true() { x = true } invariant I { x or not x } }",
        ),
        (
            "action parameter",
            "parameter",
            "spec S { type K = 0..2\n state { x: K } init { x = 0 } action a(true: K) { x = 1 } invariant I { x >= 0 } }",
        ),
        (
            "property",
            "property",
            "spec S { state { x: Bool } init { x = false } action a() { x = true } invariant true { x or not x } }",
        ),
        (
            "leadsTo",
            "property",
            "spec S { state { x: Bool } init { x = false } action a() { x = true } invariant I { x or not x } leadsTo none { not x ~> x } }",
        ),
        (
            "def",
            "def",
            "spec S { def true() = 1\n state { x: Int } init { x = 0 } action a() { x = 1 } invariant I { x >= 0 } }",
        ),
        (
            "specification",
            "specification",
            "spec true { state { x: Bool } init { x = false } action a() { x = true } invariant I { x or not x } }",
        ),
        (
            "quantifier binder",
            "binder",
            "spec S { type K = 0..2\n state { m: Map<K, Bool> } init { forall k: K { m[k] = false } } action a(k: K) { m[k] = true } invariant I { forall true: K { m[true] } } }",
        ),
    ];
    for (position, expected_position, source) in cases {
        let path = spec(&format!("reserved_{}", position.replace(' ', "_")), source);
        let (value, status) = run(&["check", &path]);
        assert_eq!(status, 2, "{position}: {value}");
        assert_eq!(value["result"], "error", "{position}: {value}");
        let message = value["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("is a reserved FSL keyword"),
            "{position} was not rejected as a reserved name: {value}"
        );
        assert!(
            message.contains(expected_position),
            "{position}: expected the message to name the position, got {message}"
        );
    }
}

// --- positive: words that are only contextual stay usable ------------------

#[test]
fn contextually_keyword_like_words_are_still_valid_names() {
    // Reserving one of these would break a specification that works today,
    // which is worse than the bug for its author. Each was measured to read
    // back correctly: `init` sets the variable false and the invariant asserts
    // it, so a correct reading is `violated`, not `verified`.
    for word in [
        "count", "sum", "stage", "in", "is", "where", "old", "abs", "and", "or",
    ] {
        let source = format!(
            "spec S {{ state {{ {word}: Bool }} init {{ {word} = false }} action a() {{ {word} = true }} invariant I {{ {word} }} }}"
        );
        let path = spec(&format!("contextual_{word}"), &source);
        let (value, status) = run(&["check", &path]);
        assert_eq!(status, 0, "{word} should still check: {value}");
        assert_eq!(value["result"], "ok", "{word}: {value}");

        let (verdict, _) = run(&["verify", &path, "--depth", "2"]);
        assert_eq!(
            verdict["result"], "violated",
            "{word} must still read back as the variable, not a literal: {verdict}"
        );
    }
}
