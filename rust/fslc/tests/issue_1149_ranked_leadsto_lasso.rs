// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1149: a ranked `leadsTo` (one with `decreases`) whose ranking
//! obligations hold no longer pays the cubic bounded fair-lasso search in
//! `verify` (BMC and the induction engine's base case). The verdict,
//! completeness, witnesses, and every other envelope field stay those of the
//! full search; only `cost` changes (a `leadsTo_rank` row appears and the
//! `leadsTo` row drops to the per-step stagnation probes).
//! `docs/design/DESIGN-induction.md` §2.5 carries the soundness argument.
//!
//! Check counts, from `bmc.rs`'s loop structure, for one binding at depth D:
//! stagnation `(D+1)(D+2)/2`, lasso `D(D+1)(2D+1)/6`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// Write `source` to a per-test file and verify it.
fn verify_source(tag: &str, source: &str, extra: &[&str]) -> (Value, i32) {
    let dir = std::env::temp_dir().join(format!("fslc-issue-1149-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(format!("{tag}.fsl"));
    std::fs::write(&path, source).expect("write spec");
    let path = path.to_str().expect("utf-8 path").to_owned();
    let mut args = vec!["verify", path.as_str(), "--no-cache"];
    args.extend_from_slice(extra);
    run(&args)
}

fn checks(output: &Value, kind: &str, name: &str) -> u64 {
    output["cost"]["properties"]
        .as_array()
        .expect("cost.properties")
        .iter()
        .filter(|row| row["kind"] == kind && row["name"] == name)
        .map(|row| row["checks"].as_u64().expect("checks"))
        .sum()
}

fn stagnation(depth: u64) -> u64 {
    (depth + 1) * (depth + 2) / 2
}

fn lasso(depth: u64) -> u64 {
    depth * (depth + 1) * (2 * depth + 1) / 6
}

/// Every envelope field except `cost` (solver counts and timings).
fn without_cost(output: &Value) -> Value {
    let mut output = output.clone();
    output
        .as_object_mut()
        .expect("envelope object")
        .remove("cost");
    output
}

const RANKED: &str = r"
spec IssueRankedReach {
  state { x: 0..5 }
  init { x = 0 }
  action inc() {
    requires x < 5
    x = x + 1
  }
  leadsTo Reach { x < 5 ~> x == 5 decreases 5 - x }
}
";

#[test]
fn a_ranked_leadsto_drops_the_lasso_search_and_keeps_the_envelope() {
    let unranked = RANKED.replace(" decreases 5 - x", "");
    assert_ne!(unranked, RANKED, "fixture edit must apply");
    for depth in [8_u64, 12, 16] {
        let depth_arg = depth.to_string();
        let (ranked, ranked_status) =
            verify_source("ranked", RANKED, &["--depth", depth_arg.as_str()]);
        let (plain, plain_status) =
            verify_source("unranked", &unranked, &["--depth", depth_arg.as_str()]);

        assert_eq!(ranked_status, 0, "{ranked}");
        assert_eq!(ranked["result"], "verified");
        // Still the bounded claim: the fast path does not upgrade BMC to a
        // proof, because the invariants it assumes are only checked to depth.
        assert_eq!(ranked["completeness"], "bounded");
        assert_eq!(ranked["leads_to"]["Reach"]["checked_to_depth"], depth);
        assert!(ranked["leads_to"]["Reach"].get("proved").is_none());

        // Fast path: only the stagnation probes remain, plus a constant
        // number of ranking checks independent of depth.
        assert_eq!(checks(&ranked, "leadsTo", "Reach"), stagnation(depth));
        assert_eq!(checks(&ranked, "leadsTo_rank", "Reach"), 2);
        // Negative control: without `decreases` the full cubic search runs.
        assert_eq!(
            checks(&plain, "leadsTo", "Reach"),
            stagnation(depth) + lasso(depth)
        );
        assert_eq!(checks(&plain, "leadsTo_rank", "Reach"), 0);

        // Same verdict and the same envelope apart from cost.
        assert_eq!(plain_status, ranked_status);
        assert_eq!(without_cost(&plain), without_cost(&ranked));
    }
}

/// A false ranked `leadsTo`: `flip` alternates `x` forever, so `x == 0` never
/// reaches `x == 2`. The ranking fails (`flip` leaves the obligation pending
/// without decreasing `x`), so the full search runs and reports the lasso.
///
/// Calibration: making `ranked_leadsto_lasso_discharges` return every ranked
/// name regardless of the ranking result turns this into `verified` (exit 0)
/// and fails the assertions below.
const FALSE_RANKED: &str = r"
spec IssueRankedFlip {
  state { x: 0..1 }
  init { x = 0 }
  fair action flip() {
    x = 1 - x
  }
  leadsTo Never { x == 0 ~> x == 2 decreases x }
}
";

#[test]
fn a_false_ranked_leadsto_is_still_violated_with_a_lasso_witness() {
    let (output, status) = verify_source("false_ranked", FALSE_RANKED, &["--depth", "8"]);
    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "violated");
    assert_eq!(output["violation_kind"], "leadsTo");
    assert_eq!(output["invariant"], "Never");
    assert_eq!(output["stutter"], false);
    assert_eq!(output["pending_since"], 0);
    assert_eq!(output["loop_start"], 0);
    let trace = output["trace"].as_array().expect("trace");
    let xs = trace
        .iter()
        .map(|step| step["state"]["x"].as_i64().expect("x"))
        .collect::<Vec<_>>();
    assert_eq!(xs, vec![0, 1, 0]);
    // The ranking ran and failed, then the lasso search found the loop.
    assert_eq!(checks(&output, "leadsTo_rank", "Never"), 2);
    assert!(
        checks(&output, "leadsTo", "Never") > stagnation(8),
        "{output}"
    );
}

/// A true ranked `leadsTo` with a wrong measure (`x` increases): the ranking
/// is inconclusive, so the full search runs and still reports `verified`.
#[test]
fn an_inconclusive_ranking_falls_back_to_the_full_search() {
    let wrong = RANKED.replace("decreases 5 - x", "decreases x");
    assert_ne!(wrong, RANKED, "fixture edit must apply");
    let (output, status) = verify_source("wrong_measure", &wrong, &["--depth", "8"]);
    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "verified");
    assert_eq!(output["completeness"], "bounded");
    assert_eq!(checks(&output, "leadsTo_rank", "Reach"), 2);
    assert_eq!(
        checks(&output, "leadsTo", "Reach"),
        stagnation(8) + lasso(8)
    );
}

/// The ranking obligations say nothing about a pending state with no enabled
/// action, so the stagnation probes are kept for a discharged property:
/// `dec` stops at `x == 2`, where `x > 0` is pending forever. The ranking
/// holds (every transition from a pending state decreases `x`), yet the
/// verdict is still the stagnation violation.
const RANKED_STALL: &str = r"
spec IssueRankedStall {
  state { x: 0..5 }
  init { x = 5 }
  action dec() {
    requires x > 2
    x = x - 1
  }
  leadsTo Drain { x > 0 ~> x == 0 decreases x }
}
";

#[test]
fn a_discharged_property_still_reports_a_pending_deadlock() {
    let (output, status) = verify_source("ranked_stall", RANKED_STALL, &["--depth", "8"]);
    assert_eq!(status, 1, "{output}");
    assert_eq!(output["result"], "violated");
    assert_eq!(output["violation_kind"], "leadsTo");
    assert_eq!(output["stutter"], true);
    let trace = output["trace"].as_array().expect("trace");
    assert_eq!(trace.last().expect("last step")["state"]["x"], 2);
}

/// The `helpful` variant (per-binding fair progress action) is discharged
/// too: the fixture's two bindings each skip their lasso search.
#[test]
fn a_ranked_helpful_leadsto_drops_the_lasso_search() {
    let (output, status) = run(&[
        "verify",
        "rust/fslc/tests/fixtures/issue_473_helpful_leadsto.fsl",
        "--depth",
        "8",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{output}");
    assert_eq!(output["result"], "verified");
    assert_eq!(output["completeness"], "bounded");
    assert_eq!(checks(&output, "leadsTo", "Responds"), 2 * stagnation(8));
    assert!(checks(&output, "leadsTo_rank", "Responds") > 0);
}

/// A ranking that fails must leave the fallback witness exactly as the full
/// search reports it without any pre-pass. `pump` raises the measure, so the
/// ranking fails (`non_helpful_action_increases_measure`) and the lasso
/// search finds the `work`/`pump` loop. Stripping `helpful`/`decreases`
/// removes the pre-pass without changing the BMC model (`helpful` is ranking
/// metadata; fairness comes from `fair`), so the two envelopes must agree on
/// everything but `cost`.
///
/// Calibration: running the pre-pass on the BMC thread (a second `Solver` in
/// the same default Z3 context) changes this witness's trace and fails the
/// comparison.
const PUMPED: &str = r"
spec IssueRankedPumped {
  state { x: Int }
  init { x = 5 }
  fair action work() {
    requires x > 0
    x = x - 1
  }
  action pump() {
    requires x > 0
    x = x + 2
  }
  invariant NonNeg { x >= 0 }
  leadsTo Finishes {
    x > 0 ~> x == 0
    helpful work()
    decreases x
  }
}
";

#[test]
fn a_failed_ranking_leaves_the_fallback_witness_unchanged() {
    let stripped = PUMPED
        .replace("    helpful work()\n", "")
        .replace("    decreases x\n", "");
    assert_ne!(stripped, PUMPED, "fixture edit must apply");
    let (ranked, ranked_status) = verify_source("pumped", PUMPED, &["--depth", "8"]);
    let (plain, plain_status) = verify_source("pumped_plain", &stripped, &["--depth", "8"]);
    assert_eq!(ranked_status, 1, "{ranked}");
    assert_eq!(ranked["result"], "violated");
    assert_eq!(ranked["violation_kind"], "leadsTo");
    assert!(ranked["loop_start"].is_u64(), "{ranked}");
    assert!(checks(&ranked, "leadsTo_rank", "Finishes") > 0);
    assert_eq!(checks(&plain, "leadsTo_rank", "Finishes"), 0);
    assert_eq!(plain_status, ranked_status);
    assert_eq!(without_cost(&plain), without_cost(&ranked));
}
