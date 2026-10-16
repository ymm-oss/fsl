// SPDX-License-Identifier: Apache-2.0

//! Issue #1303: a governance `preservation` whose refinement fails used to
//! leave `fslc check` at `result:"ok"` / exit 0 and `fslc verify` at
//! `verified` / exit 0, so a CI gating on the exit code passed a control the
//! governance says is preserved and is not. A failed preservation now folds
//! into the top-level `result` and exit code through the same owner as an
//! inline `implements` seam
//! (`verification_output::attach_governance` -> `fold_refinement_verdict`),
//! for `check`, `verify`, and every consumer of their envelopes.
//!
//! The reproducer is the issue's own four files, under
//! `tests/fixtures/issue_1303/`. `governance_refines.fsl` is the control: the
//! same contract over an `after` spec that refines must stay `ok` /
//! `verified` / exit 0. The `violated` -> `impl_violated` and
//! `unknown_budget` rows are pinned in `refine_budget_unknown.rs`, next to
//! the fixtures that produce them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicUsize = AtomicUsize::new(0);

const FIXTURES: &[&str] = &[
    "governance.fsl",
    "governance_refines.fsl",
    "abs.fsl",
    "impl.fsl",
    "impl_refines.fsl",
    "map.fsl",
];

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/issue_1303")
}

/// A private copy of the fixtures, with its own verify cache, so a test can
/// rewrite a dependency without touching the checked-in reproducer.
fn scratch(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1303-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    for file in FIXTURES {
        fs::copy(fixture_dir().join(file), dir.join(file)).expect("copy fixture");
    }
    dir
}

fn fslc(dir: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env("FSLC_CACHE_DIR", dir.join("cache"))
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}); stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn assert_failed_preservation_folded(value: &Value, status: i32) {
    assert_eq!(status, 1, "{value:#}");
    assert_eq!(value["result"], "refinement_failed", "{value:#}");
    assert_eq!(
        value["governance"]["preservations"][0]["result"], "refinement_failed",
        "{value:#}"
    );
}

#[test]
fn check_fails_closed_on_a_failed_preservation() {
    let dir = scratch("check");
    let (value, status) = fslc(&dir, &["check", "governance.fsl"]);
    assert_failed_preservation_folded(&value, status);
}

#[test]
fn verify_fails_closed_on_a_failed_preservation() {
    let dir = scratch("verify");
    let (value, status) = fslc(&dir, &["verify", "governance.fsl"]);
    assert_failed_preservation_folded(&value, status);
}

#[test]
fn a_refining_preservation_stays_ok_and_verified() {
    let dir = scratch("control");
    let (checked, check_status) = fslc(&dir, &["check", "governance_refines.fsl"]);
    assert_eq!(check_status, 0, "{checked:#}");
    assert_eq!(checked["result"], "ok", "{checked:#}");
    assert_eq!(
        checked["governance"]["preservations"][0]["result"], "refines",
        "{checked:#}"
    );

    let (verified, verify_status) = fslc(&dir, &["verify", "governance_refines.fsl"]);
    assert_eq!(verify_status, 0, "{verified:#}");
    assert_eq!(verified["result"], "verified", "{verified:#}");
    assert_eq!(
        verified["governance"]["preservations"][0]["result"], "refines",
        "{verified:#}"
    );
}

/// The verify cache keys only the governance document's own kernel; the
/// preservation's files are not in that key. A cached `verified` from a run
/// where the preservation refined must not survive the `after` spec being
/// broken afterwards.
#[test]
fn a_cached_verify_does_not_hide_a_preservation_broken_later() {
    let dir = scratch("cache");
    fs::copy(dir.join("impl_refines.fsl"), dir.join("impl.fsl")).expect("start refining");
    let (first, first_status) = fslc(&dir, &["verify", "governance.fsl"]);
    assert_eq!(first_status, 0, "{first:#}");
    assert_eq!(first["result"], "verified", "{first:#}");

    fs::copy(fixture_dir().join("impl.fsl"), dir.join("impl.fsl")).expect("break the after spec");
    let (second, second_status) = fslc(&dir, &["verify", "governance.fsl"]);
    assert_failed_preservation_folded(&second, second_status);
}

fn chain_layer<'a>(value: &'a Value, layer: &str) -> &'a Value {
    value["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|entry| entry["layer"] == layer)
        .unwrap_or_else(|| panic!("no {layer} layer: {value:#}"))
}

/// A `chain` spec layer reads the folded envelope: without `depth` it runs
/// `check`, with one it runs `verify`, and both must fail the layer
/// (`--keep-going` so the second layer runs after the first fails).
#[test]
fn chain_fails_a_governance_layer_with_a_failed_preservation() {
    let dir = scratch("chain");
    fs::write(
        dir.join("fsl-project.toml"),
        "[business]\nfile = \"governance.fsl\"\n\n\
         [requirements]\nfile = \"governance.fsl\"\ndepth = 2\n",
    )
    .expect("write manifest");

    let (value, status) = fslc(&dir, &["chain", "fsl-project.toml", "--keep-going"]);

    assert_eq!(status, 1, "{value:#}");
    for (layer, kind) in [("business", "check"), ("requirements", "verify")] {
        let entry = chain_layer(&value, layer);
        assert_eq!(entry["kind"], kind, "{value:#}");
        assert_eq!(entry["status"], "failed", "{value:#}");
        assert_eq!(entry["result"], "refinement_failed", "{value:#}");
        assert_eq!(entry["exit_code"], 1, "{value:#}");
    }
}

/// `ledger`, `html`, and `mutate` share the `run_verify` baseline and take
/// their exit from it, so the fold reaches them too. `html` additionally
/// renders the folded verdict as its result badge.
#[test]
fn verification_baseline_consumers_inherit_the_folded_verdict() {
    let dir = scratch("baseline");

    let (ledger, ledger_status) = fslc(&dir, &["ledger", "governance.fsl", "-o", "ledger.md"]);
    assert_eq!(ledger_status, 1, "{ledger:#}");

    let (html, html_status) = fslc(&dir, &["html", "governance.fsl", "-o", "report.html"]);
    assert_eq!(html_status, 1, "{html:#}");
    let report = fs::read_to_string(dir.join("report.html")).expect("read html report");
    assert!(
        report.contains("<span class=\"badge bad\">refinement_failed</span>"),
        "html report must badge the folded verdict"
    );

    let (mutate, mutate_status) = fslc(&dir, &["mutate", "governance.fsl"]);
    assert_eq!(mutate_status, 1, "{mutate:#}");
    assert_eq!(mutate["result"], "refinement_failed", "{mutate:#}");

    // Control: the same consumers still exit 0 over a refining preservation.
    for args in [
        &["ledger", "governance_refines.fsl", "-o", "ledger_ok.md"][..],
        &["html", "governance_refines.fsl", "-o", "report_ok.html"][..],
    ] {
        let (value, status) = fslc(&dir, args);
        assert_eq!(status, 0, "{args:?}: {value:#}");
    }
}

/// Issue #1316: the ledger's exit already carried the folded verdict, but its
/// Markdown body showed neither that verdict nor any preservation result. The
/// body now agrees with the exit: it names the verdict and every preservation
/// with its result, for a passing governance document too.
#[test]
fn ledger_body_shows_the_verdict_and_the_preservations_its_exit_reflects() {
    let dir = scratch("ledger-body");

    let (failing, failing_status) = fslc(&dir, &["ledger", "governance.fsl", "-o", "failing.md"]);
    assert_eq!(failing_status, 1, "{failing:#}");
    let body = fs::read_to_string(dir.join("failing.md")).expect("read ledger");
    assert!(body.contains("- 検証判定: `refinement_failed`\n"), "{body}");
    assert!(
        body.contains("| `P` | `A` → `I` | `CTRL-1` | 🔴 `refinement_failed` |\n"),
        "{body}"
    );

    let (passing, passing_status) = fslc(
        &dir,
        &["ledger", "governance_refines.fsl", "-o", "passing.md"],
    );
    assert_eq!(passing_status, 0, "{passing:#}");
    let body = fs::read_to_string(dir.join("passing.md")).expect("read ledger");
    assert!(body.contains("- 検証判定: `verified`\n"), "{body}");
    assert!(
        body.contains("| `P` | `A` → `I` | `CTRL-1` | 🟢 `refines` |\n"),
        "{body}"
    );
}

/// A preservation `verify` cannot evaluate is the same located `error` (exit
/// 2) `check` reports, not `verified` over the document's trivial kernel.
#[test]
fn verify_rejects_a_preservation_it_cannot_evaluate() {
    let dir = scratch("unevaluable");
    fs::copy(
        fixture_dir().join("../governance_missing_dependency.fsl"),
        dir.join("governance_missing_dependency.fsl"),
    )
    .expect("copy fixture");
    let (value, status) = fslc(&dir, &["verify", "governance_missing_dependency.fsl"]);
    assert_eq!(status, 2, "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "type", "{value:#}");
    assert_eq!(
        value["loc"],
        serde_json::json!({"line": 6, "column": 5}),
        "{value:#}"
    );
}

/// `verify` now evaluates the whole governance contract, not only its
/// preservations: a contract error unrelated to any preservation (here a
/// malformed delegated business file) is the located `error` (exit 2) `check`
/// reports for the same document, not `verified` over the trivial kernel.
#[test]
fn verify_rejects_a_contract_error_check_reports() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = scratch("malformed-dependency");
    for file in [
        "governance_malformed_dependency.fsl",
        "governance_malformed_business.fsl",
    ] {
        fs::copy(
            root.join("examples/gallery/errors").join(file),
            dir.join(file),
        )
        .expect("copy gallery fixture");
    }
    let (checked, check_status) = fslc(&dir, &["check", "governance_malformed_dependency.fsl"]);
    let (verified, verify_status) = fslc(&dir, &["verify", "governance_malformed_dependency.fsl"]);
    assert_eq!(verify_status, 2, "{verified:#}");
    assert_eq!(check_status, 2, "{checked:#}");
    for key in ["result", "kind", "message", "loc"] {
        assert_eq!(verified[key], checked[key], "{key}: {verified:#}");
    }
    assert_eq!(verified["result"], "error", "{verified:#}");
}
