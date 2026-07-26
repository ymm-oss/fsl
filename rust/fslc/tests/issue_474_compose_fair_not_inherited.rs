// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #474: native `fslc check`/`verify` never
//! emitted the documented `fair_not_inherited` compose warning
//! (`docs/LANGUAGE.md`, `docs/DESIGN-compose.md`) because
//! `rust/fsl-core/src/compose.rs` had no warnings channel at all -- a
//! synchronized action's constituent `fair` markers were silently discarded
//! during lowering (`sync_action`, formerly `resolve_alias_action`) with
//! nothing recording that the composite failed to inherit them, and the
//! checked `KernelModel` cannot reconstruct this after the fact (only the
//! composite's own single `fair` marker survives expansion).
//!
//! Three-lane matrix (matches the frozen Python reference's
//! `src/fslc/compose.py` behavior exactly, including the warning message and
//! `loc`):
//! - `fair_loss`: a non-fair synchronized action references a fair
//!   constituent -> exactly one `fair_not_inherited` warning.
//! - `fair_kept`: the same synchronized action is declared `fair` itself ->
//!   no warning (fairness is intentionally not inherited; the author opted
//!   in explicitly).
//! - `no_fair_loss`: the synchronized action references only non-fair
//!   constituents -> no warning (there is nothing lost).

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .display()
        .to_string()
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

fn fair_not_inherited_warnings(value: &Value) -> Vec<&Value> {
    value["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .filter(|warning| warning["kind"] == "fair_not_inherited")
        .collect()
}

#[test]
fn check_reports_fair_not_inherited_for_a_non_fair_synchronized_action() {
    let path = fixture("issue_474_fair_loss.fsl");
    let (value, status) = run(&["check", &path]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ok");

    let warnings = fair_not_inherited_warnings(&value);
    assert_eq!(warnings.len(), 1, "{value}");
    assert_eq!(
        warnings[0]["message"],
        "synchronized action 'decay_fresh' is not fair; fair constituent action(s) \
         core.decay_fresh will not contribute fairness unless the composite action \
         is declared fair"
    );
    assert_eq!(
        warnings[0]["loc"],
        serde_json::json!({"line": 6, "column": 3})
    );
}

#[test]
fn check_is_silent_when_the_synchronized_action_is_itself_declared_fair() {
    // Negative control: fairness is intentionally not inherited (the composite
    // keeps only its own `fair` marker), so declaring the sync action fair
    // itself must suppress the warning -- it is not a blanket "any fair
    // constituent" trigger.
    let path = fixture("issue_474_fair_kept.fsl");
    let (value, status) = run(&["check", &path]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ok");
    assert!(fair_not_inherited_warnings(&value).is_empty(), "{value}");
}

#[test]
fn check_is_silent_when_no_referenced_constituent_is_fair() {
    // Negative control: no fair constituent is referenced at all, so there is
    // nothing to warn about.
    let path = fixture("issue_474_no_fair_loss.fsl");
    let (value, status) = run(&["check", &path]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ok");
    assert!(fair_not_inherited_warnings(&value).is_empty(), "{value}");
}

#[test]
fn verify_reports_the_same_warning_as_check_and_still_verifies() {
    let path = fixture("issue_474_fair_loss.fsl");
    let (value, status) = run(&["verify", &path, "--depth", "3"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "verified", "{value}");

    let warnings = fair_not_inherited_warnings(&value);
    assert_eq!(warnings.len(), 1, "{value}");
    assert_eq!(
        warnings[0]["message"],
        "synchronized action 'decay_fresh' is not fair; fair constituent action(s) \
         core.decay_fresh will not contribute fairness unless the composite action \
         is declared fair"
    );
}

#[test]
fn verify_is_silent_on_the_fair_kept_and_no_fair_loss_controls() {
    for name in ["issue_474_fair_kept.fsl", "issue_474_no_fair_loss.fsl"] {
        let path = fixture(name);
        let (value, status) = run(&["verify", &path, "--depth", "3"]);
        assert_eq!(status, 0, "{name}: {value}");
        assert_eq!(value["result"], "verified", "{name}: {value}");
        assert!(
            fair_not_inherited_warnings(&value).is_empty(),
            "{name}: {value}"
        );
    }
}

#[test]
fn existing_compose_corpus_is_unaffected() {
    // No fair constituents are referenced by any non-fair synchronized action
    // in the shipped corpus, so this must stay a no-op change for it.
    for path in ["specs/order_system.fsl", "specs/bank_system.fsl"] {
        let (value, status) = run(&["check", path]);
        assert_eq!(status, 0, "{path}: {value}");
        assert_eq!(value["result"], "ok", "{path}: {value}");
        assert!(
            fair_not_inherited_warnings(&value).is_empty(),
            "{path}: {value}"
        );
    }
}
