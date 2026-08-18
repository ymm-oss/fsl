// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! End-to-end coverage for issue #729's `vacuity_probe_truncated` emission
//! arm in `verification_warnings` (`rust/fsl-runtime/src/lib.rs`),
//! independently confirmed missing by review. `rust/fsl-runtime/tests/
//! diagnostics.rs`'s tri-state controls call `expression_reachability`
//! directly and never go through `verification_warnings`.
//! `rust/fsl-tools/tests/issue_729_vacuity_probe_truncated_ledger.rs`
//! hand-constructs the JSON rather than producing it from a real run.
//! `issue_729_vacuity_probe_corpus_budget.rs` asserts truncation does *not*
//! happen. None of them drive a real budget exhaustion through the CLI, so
//! a typo in the `vacuity_probe_truncated` string literal (which would
//! make `is_vacuity_kind` false, letting `--vacuity error` pass a spec
//! whose vacuity was never established: exactly the fail-open this issue
//! exists to close) would leave every existing test green.
//!
//! The model below is the reviewer-provided reproducer. Its `step` action
//! computes `count = (count * 10) + x`, which is injective per BFS level
//! (same trick as `rust/fsl-runtime/tests/boundary_probe_budget.rs`), so
//! the reachable state count grows `10` to the power of the level and is
//! unbounded (`count: Int`, no type bound). It genuinely exhausts
//! `CONCRETE_PROBE_BUDGET` within a `--depth` that would otherwise need
//! `10` to the power of `depth` states to close. `NeverNegative`'s
//! antecedent (`count < 0`) can never become true: `step` only ever
//! appends a non-negative digit. Before this issue's budget existed, a
//! full unbudgeted BFS eventually confirmed it vacuous; now the shared
//! probe truncates instead. Measured on this tree, this fixture takes
//! roughly 1.7 seconds and 290 MB after this fix, versus roughly 18
//! seconds and 6 GB before it (a full, unbounded reachable-set
//! enumeration): a cheap, fast test that also demonstrates this issue's
//! own before/after memory story.

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
            "fsl-issue-729-{name}-{}-{nonce}.fsl",
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

const DIGIT_GROWTH_TRUNC_SOURCE: &str = r"
spec DigitGrowthTrunc {
  state { count: Int, done: Bool }
  init { count = 0 done = false }
  action step(x in 0..9) { count = count * 10 + x }
  invariant NeverNegative { count < 0 => done }
}
";

fn verify(fixture: &Fixture, extra: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture.text(),
            "--depth",
            "6",
            "--deadlock",
            "ignore",
            "--no-cache",
        ])
        .args(extra)
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// The core positive control: a genuine budget exhaustion reaches
/// `--vacuity error`'s fail-closed exit, through the real emission arm this
/// PR added, not a hand-built JSON envelope.
#[test]
fn a_genuine_budget_exhaustion_fails_closed_under_vacuity_error() {
    let fixture = Fixture::new("trunc-error", DIGIT_GROWTH_TRUNC_SOURCE);
    let (output, status) = verify(&fixture, &["--vacuity", "error"]);

    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "vacuity_probe_truncated", "{output:#}");
    assert_eq!(output["trace_type"], "vacuity", "{output:#}");
    let findings = output["findings"].as_array().expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["kind"] == "vacuity_probe_truncated"
                && finding["classification"] == "probe_truncated"
                && finding["faithfulness_class"] == "reachability_unknown"),
        "expected a vacuity_probe_truncated finding with its own classification/faithfulness_class: \
         {output:#}"
    );
}

/// Companion: the same genuine exhaustion is visible (not silently dropped)
/// under the default `--vacuity warn`, with `result` staying `verified` --
/// truncation is reported, not treated as a violation.
#[test]
fn the_same_exhaustion_is_a_warning_not_a_violation_under_vacuity_warn() {
    let fixture = Fixture::new("trunc-warn", DIGIT_GROWTH_TRUNC_SOURCE);
    let (output, status) = verify(&fixture, &["--vacuity", "warn"]);

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    let warnings = output["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|warning| warning["kind"] == "vacuity_probe_truncated"),
        "expected a vacuity_probe_truncated warning: {output:#}"
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning["kind"] == "vacuous_implication"),
        "a truncated probe must not also (or instead) report vacuous_implication -- that would be \
         the exact fail-open this issue closes: {output:#}"
    );
}

/// Companion: `--vacuity ignore` suppresses the finding entirely (skip, not
/// just filter -- `issue_729_vacuity_ignore_skip.rs` covers the general
/// equivalence; this confirms it holds for a genuinely truncated candidate
/// specifically, not only an ordinary vacuous one).
#[test]
fn the_same_exhaustion_is_suppressed_under_vacuity_ignore() {
    let fixture = Fixture::new("trunc-ignore", DIGIT_GROWTH_TRUNC_SOURCE);
    let (output, status) = verify(&fixture, &["--vacuity", "ignore"]);

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    assert!(
        output["warnings"]
            .as_array()
            .expect("warnings array")
            .is_empty(),
        "--vacuity ignore must suppress the truncated-probe finding: {output:#}"
    );
}
