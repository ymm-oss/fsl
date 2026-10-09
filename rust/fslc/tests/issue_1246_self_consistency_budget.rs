// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #1246: `check_refinement`'s self-consistency pre-pass
//! (`first_self_violation`) ran without the state budget #1060 gave the
//! correspondence walk, so it held the implementation's whole reachable set
//! before the walk's budget could apply (#1041's reproducer still exceeds
//! 6 GB there). It now stops at `fsl_runtime::IMPLEMENTS_SEARCH_BUDGET` and
//! reports `unknown_budget` through the same entry points as a cut-off walk
//! (`refine_budget_unknown.rs`, #1245).
//!
//! The fixture's implementation pushes onto a `Seq<V, 7>` over six values and
//! breaks its own invariant only at size 7. Every state through depth 6 is
//! in bounds: `sum_{i=0}^{6} 6^i = 55,987` of them, more than the 50,000
//! budget, and the BFS inserts all of them before it generates a depth-7
//! child. An unbudgeted pre-pass therefore walks past the budget and reports
//! the depth-7 self-violation; a budgeted one stops at 50,000 first. The
//! files are written to a scratch directory, not the corpus, so the
//! all-corpus `check` sweep does not pay for them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

const BUDGET: u64 = 50_000;

const ABS: &str = "spec WideAbs {\n  type AV = 0..5\n  state { seq: Seq<AV, 7> }\n  \
     init { seq = Seq {} }\n  action push(v: AV) {\n    requires seq.size() < 7\n    \
     seq = seq.push(v)\n  }\n}\n";

/// The body shared by the standalone impl spec and the inline-`implements`
/// requirements document: the same pushes as `WideAbs`, plus an invariant
/// the seventh push breaks.
const IMPL_BODY: &str = "  type IV = 0..5\n  state { seq: Seq<IV, 7> }\n  \
     init { seq = Seq {} }\n  action push(v: IV) {\n    requires seq.size() < 7\n    \
     seq = seq.push(v)\n  }\n  invariant ShorterThanSeven { seq.size() < 7 }\n";

/// The same pushes without the invariant: self-consistent, 55,987 states
/// within depth 6.
const CONSISTENT_IMPL: &str = "spec WideConsistent {\n  type IV = 0..5\n  \
     state { seq: Seq<IV, 7> }\n  init { seq = Seq {} }\n  action push(v: IV) {\n    \
     requires seq.size() < 7\n    seq = seq.push(v)\n  }\n}\n";

/// An abstraction that refuses to push 5, so `WideConsistent`'s first
/// `push(5)` is an `abs_requires_failed` mismatch at step 1.
const NARROW_ABS: &str = "spec NarrowAbs {\n  type AV = 0..5\n  state { seq: Seq<AV, 7> }\n  \
     init { seq = Seq {} }\n  action push(v: AV) {\n    requires v <= 4\n    \
     requires seq.size() < 7\n    seq = seq.push(v)\n  }\n}\n";

fn fixture(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1246-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    for (file, source) in [
        ("abs.fsl", ABS.to_owned()),
        ("impl.fsl", format!("spec WideBroken {{\n{IMPL_BODY}}}\n")),
        (
            "impl_refines_abs.fsl",
            "refinement BrokenRefinesAbs { impl WideBroken abs WideAbs maps auto }\n".to_owned(),
        ),
        (
            "req.fsl",
            format!(
                "requirements WideBrokenReq {{\n  implements WideAbs from \"abs.fsl\" \
                 {{ maps auto }}\n{IMPL_BODY}}}\n"
            ),
        ),
        (
            "governance.fsl",
            "governance WideControls {\n  control CTRL-WIDE \"The sequence is preserved\"\n\n  \
             preservation WidePreserved {\n    before WideAbs from \"abs.fsl\"\n    \
             after WideBroken from \"impl.fsl\"\n    preserve CTRL-WIDE\n    \
             checked_by refinement \"impl_refines_abs.fsl\"\n  }\n}\n"
                .to_owned(),
        ),
        ("narrow_abs.fsl", NARROW_ABS.to_owned()),
        ("consistent_impl.fsl", CONSISTENT_IMPL.to_owned()),
        (
            "consistent_refines_narrow.fsl",
            "refinement ConsistentRefinesNarrow { impl WideConsistent abs NarrowAbs maps auto }\n"
                .to_owned(),
        ),
    ] {
        fs::write(dir.join(file), source).expect("write fixture file");
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

/// `fslc refine` at depth 7: the pre-pass stops at the budget, so the result
/// is the cutoff, with the pre-pass's state count -- not the depth-7
/// self-violation an unbudgeted pre-pass reaches after 55,987 states.
#[test]
fn refine_reports_unknown_budget_when_the_self_consistency_pre_pass_is_cut_off() {
    let dir = fixture("refine");

    let (value, status) = fslc(
        &dir,
        &[
            "refine",
            "impl.fsl",
            "abs.fsl",
            "impl_refines_abs.fsl",
            "--depth",
            "7",
        ],
    );

    assert_eq!(value["result"], "unknown_budget", "{value:#}");
    assert_eq!(status, 1, "{value:#}");
    assert_eq!(value["states_explored"], BUDGET, "{value:#}");
    assert!(value.get("checked_to_depth").is_none(), "{value:#}");
}

/// Control on the same files below the budget (depth 5: 9,331 states, the
/// self-violation out of reach): the pre-pass completes and the impl
/// refines. Fails if the pre-pass reports a cutoff when there is none.
#[test]
fn refine_still_refines_the_same_files_below_the_budget() {
    let dir = fixture("refine-control");

    let (value, status) = fslc(
        &dir,
        &[
            "refine",
            "impl.fsl",
            "abs.fsl",
            "impl_refines_abs.fsl",
            "--depth",
            "5",
        ],
    );

    assert_eq!(status, 0, "{value:#}");
    assert_eq!(value["result"], "refines", "{value:#}");
    assert_eq!(value["checked_to_depth"], 5, "{value:#}");
}

/// Inline `implements` (`fslc check`, fixed depth 8) reports the pre-pass
/// cutoff as `unknown_budget` / exit 1, as it does a cut-off walk.
#[test]
fn check_reports_unknown_budget_for_an_inline_implements_pre_pass_cutoff() {
    let dir = fixture("check");

    let (value, status) = fslc(&dir, &["check", "req.fsl"]);

    assert_eq!(value["implements"]["result"], "unknown_budget", "{value:#}");
    assert_eq!(value["implements"]["states_explored"], BUDGET, "{value:#}");
    assert_eq!(value["result"], "unknown_budget", "{value:#}");
    assert_eq!(status, 1, "{value:#}");
}

/// Governance (depth 8) reports the preservation as `unknown_budget`, not
/// the `violated` an unbudgeted pre-pass reaches past the budget.
#[test]
fn governance_reports_unknown_budget_for_a_pre_pass_cutoff() {
    let dir = fixture("governance");

    let (value, _status) = fslc(&dir, &["check", "governance.fsl"]);

    assert_eq!(
        value["governance"]["preservations"][0]["result"], "unknown_budget",
        "{value:#}"
    );
}

/// The order this issue fixes, under the real budget: the pre-pass reaches
/// 50,000 states before the walk runs, so the run is `unknown_budget` even
/// though the walk would find a mismatch at step 1. The depth-1 control on
/// the same files shows the mismatch is real. Running the walk after a
/// pre-pass cutoff is a deferred option; adopting it must change this test.
#[test]
fn refine_reports_the_pre_pass_cutoff_before_an_early_correspondence_failure() {
    let dir = fixture("order");
    let refine = |depth: &str| {
        fslc(
            &dir,
            &[
                "refine",
                "consistent_impl.fsl",
                "narrow_abs.fsl",
                "consistent_refines_narrow.fsl",
                "--depth",
                depth,
            ],
        )
    };

    let (shallow, shallow_status) = refine("1");
    assert_eq!(shallow["result"], "refinement_failed", "{shallow:#}");
    assert_eq!(shallow["kind"], "abs_requires_failed", "{shallow:#}");
    assert_eq!(shallow["violated_at_step"], 1, "{shallow:#}");
    assert_eq!(shallow_status, 1, "{shallow:#}");

    let (deep, deep_status) = refine("6");
    assert_eq!(deep["result"], "unknown_budget", "{deep:#}");
    assert_eq!(deep["states_explored"], BUDGET, "{deep:#}");
    assert_eq!(deep_status, 1, "{deep:#}");
}
