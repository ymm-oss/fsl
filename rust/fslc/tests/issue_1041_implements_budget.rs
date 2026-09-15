// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #1041: `check_refinement`'s correspondence
//! walk (reached by an inline `requirements ... { implements Abs from
//! "..." { } }` seam) had no state-count budget. P1 (`SIGMA-P1.md`) measured
//! this reaching ~3.8 GB RSS on one corpus-scale domain, with a flat
//! (~7.8 MB) control run against the same domain with the `implements`
//! block removed. The fix (`fsl_runtime::IMPLEMENTS_SEARCH_BUDGET`, wired
//! through both `check` and `verify`) is exercised here through the real
//! `fslc` CLI binary using the actual fixed constant (50,000), not an
//! injected value -- `rust/fsl-runtime/tests/issue_1041_implements_search_budget.rs`
//! separately calibrates the cutoff mechanism itself against a tiny,
//! injected-budget fixture, which does not need a corpus-scale domain.
//!
//! Deliberately **not** a `specs/`/`examples/`/`rust/fslc/tests/fixtures/`
//! corpus fixture, for the same reason `issue_697_all_properties_memory.rs`
//! isn't one (its own top comment): placing a domain this large in the
//! required all-corpus `check` sweep (`corpus_check_sweep.rs`) would make
//! every future `check` run pay this cost. `WideFixtureDir` below writes
//! both files as source strings to a temp directory instead, matching that
//! file's `Fixture` pattern.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// A small temp directory holding two related `.fsl` files, so a
/// requirements-layer `implements Abs from "<relative>"` path resolves the
/// way it would in a real project layout. Removed on drop.
struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "fsl-issue-1041-{name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create fixture directory");
        Self(dir)
    }

    fn write(&self, filename: &str, source: &str) -> PathBuf {
        let path = self.0.join(filename);
        std::fs::write(&path, source).expect("write fixture file");
        path
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Six-valued alphabet, `Seq<V, 7>` push, no dedup collapse (each push
/// order is a distinct `Seq` value, mirroring the branching trick
/// `issue_697`/`issue_783`'s shared `LabelCoreRepro` reproducer uses):
/// cumulative reachable states through depth *k* is `sum_{i=0}^{k} 6^i`,
/// which already exceeds `IMPLEMENTS_SEARCH_BUDGET` (50,000) partway
/// through depth 6 (55,987 at exactly depth 6) -- comfortably inside the
/// `depth = 8` both `check` (fixed) and this file's `verify --depth 8`
/// calls use, and the walk stops the instant it crosses the budget rather
/// than exploring the much larger full tree.
const WIDE_ABS_SOURCE: &str = r"
spec WidePushAbs {
  type V = 0..5
  state { seq: Seq<V, 7> }
  init { seq = Seq {} }
  action push(v: V) {
    requires seq.size() < 7
    seq = seq.push(v)
  }
}
";

fn wide_requirements_source() -> String {
    r#"
requirements WidePushReq {
  implements WidePushAbs from "wide_abs.fsl" { maps auto }

  type V = 0..5
  state { seq: Seq<V, 7> }
  init { seq = Seq {} }
  action push(v: V) maps push(v) {
    requires seq.size() < 7
    seq = seq.push(v)
  }
}
"#
    .to_owned()
}

/// A small, known-correct inline `implements` seam (well under the 50,000
/// budget: 4 reachable states) -- the preservation control for accept
/// criterion 4. A "fix" that hollowed `check_refinement` into always
/// reporting `unknown_budget` (not just when the budget is actually
/// exceeded) would pass the over-budget tests below but fail this one.
const SMALL_ABS_SOURCE: &str = r"
spec SmallCounterAbs {
  type Qty = 0..3
  state { n: Qty }
  init { n = 0 }
  action bump() {
    requires n < 3
    n = n + 1
  }
}
";

fn small_requirements_source() -> String {
    r#"
requirements SmallCounterReq {
  implements SmallCounterAbs from "small_abs.fsl" { maps auto }

  type Qty = 0..3
  state { n: Qty }
  init { n = 0 }
  action bump() maps bump() {
    requires n < 3
    n = n + 1
  }
}
"#
    .to_owned()
}

fn run_check(path: &Path) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check"])
        .arg(path)
        .output()
        .expect("run native CLI check");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn run_verify(path: &Path, depth: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["verify"])
        .arg(path)
        .args(["--depth", depth, "--deadlock", "ignore", "--no-cache"])
        .output()
        .expect("run native CLI verify");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// Accept criterion 1 (via the `check` entry point, criterion 2's first
/// half): `check` fixes depth at 8 (`main.rs`'s `implements_result_from_source(...,
/// 8)`), no user-facing flag. Hitting the real fixed budget must report
/// `unknown_budget` (non-success) and exit 1, directly -- not a ceiling.
#[test]
fn check_reports_unknown_budget_and_exits_one_over_the_real_budget() {
    let fixture = FixtureDir::new("check-over-budget");
    fixture.write("wide_abs.fsl", WIDE_ABS_SOURCE);
    let req_path = fixture.write("wide_req.fsl", &wide_requirements_source());

    let (output, status) = run_check(&req_path);

    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "unknown_budget", "{output:#}");
    assert_eq!(
        output["implements"]["result"], "unknown_budget",
        "{output:#}"
    );
    assert!(
        output["implements"]["states_explored"]
            .as_u64()
            .is_some_and(|n| n >= 50_000),
        "{output:#}"
    );
}

/// Accept criterion 1 via the `verify` entry point (criterion 2's second
/// half): `verify` drives the same `check_refinement` with
/// `options.depth`, a user-chosen value -- a different argument than
/// `check`'s fixed 8, so this is a genuinely separate observation, not the
/// same code path re-run under a different command name.
#[test]
fn verify_reports_unknown_budget_and_exits_one_over_the_real_budget() {
    let fixture = FixtureDir::new("verify-over-budget");
    fixture.write("wide_abs.fsl", WIDE_ABS_SOURCE);
    let req_path = fixture.write("wide_req.fsl", &wide_requirements_source());

    let (output, status) = run_verify(&req_path, "8");

    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "unknown_budget", "{output:#}");
    assert_eq!(
        output["implements"]["result"], "unknown_budget",
        "{output:#}"
    );
    assert!(
        output["implements"]["states_explored"]
            .as_u64()
            .is_some_and(|n| n >= 50_000),
        "{output:#}"
    );
}

/// Accept criterion 4 (preservation control) via `check`: a small,
/// genuinely-correct inline `implements` seam far below the budget must
/// still report `refines` / exit 0, unaffected by the new cutoff.
#[test]
fn check_still_refines_a_small_seam_far_below_the_budget() {
    let fixture = FixtureDir::new("check-small-control");
    fixture.write("small_abs.fsl", SMALL_ABS_SOURCE);
    let req_path = fixture.write("small_req.fsl", &small_requirements_source());

    let (output, status) = run_check(&req_path);

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "ok", "{output:#}");
    assert_eq!(output["implements"]["result"], "refines", "{output:#}");
}

/// Accept criterion 4 (preservation control) via `verify`, the entry
/// point's other half.
#[test]
fn verify_still_refines_a_small_seam_far_below_the_budget() {
    let fixture = FixtureDir::new("verify-small-control");
    fixture.write("small_abs.fsl", SMALL_ABS_SOURCE);
    let req_path = fixture.write("small_req.fsl", &small_requirements_source());

    let (output, status) = run_verify(&req_path, "8");

    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    assert_eq!(output["implements"]["result"], "refines", "{output:#}");
}
