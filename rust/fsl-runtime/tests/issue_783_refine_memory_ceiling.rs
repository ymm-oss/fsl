// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #783: two more lanes that used to carry a
//! whole `Monitor` (a `KernelModel` clone) per queued state, on top of the
//! `bfs`/`find_boundary_violation`/`first_self_violation` lanes issue
//! #730/#697 already fixed and issue #776 documented as bisected apart from
//! this one.
//!
//! Both tests below build on the shared `LabelCoreRepro` reproducer in
//! `support::LABEL_CORE_REPRO_SOURCE` (also used by
//! `issue_730_bfs_memory_ceiling.rs` and `issue_730_explicit_memory_ceiling.rs`):
//! its `audit: Seq<LabelId, 10>` records push order, so the same *set* of
//! pushed values in a different *order* is a different `Seq` value,
//! `visited` dedup never collapses the branching, and a BFS/correspondence-
//! walk frontier grows like branching^depth. An independent review of an
//! earlier version of this file found it had drifted from that shared
//! reproducer -- missing `invariant`/`trans`/`reachable` declarations,
//! which changes the `KernelModel`'s clone size and therefore the pre-fix
//! calibration numbers below -- while claiming to be "the same" copy.
//! Sharing the fixture via `support` makes that specific drift impossible
//! to reintroduce by construction, not just by discipline.
//!
//! `rust/fslc/tests/issue_697_all_properties_memory.rs` carries a fourth
//! copy of this same reproducer, in a different crate that a `tests/`-local
//! module cannot reach. Consolidating that one too is an explicit follow-up
//! (tracked in the #783 pull request), not folded into this fix's scope.
//!
//! Like `issue_730_bfs_memory_ceiling.rs`, macOS does not enforce
//! `RLIMIT_AS` the way `ulimit -v` expects -- observed directly on this
//! repo's own macOS development environment, and more bluntly than that
//! file's own caveat: here `sh -c 'ulimit -v N'` fails outright with
//! "cannot modify limit: Invalid argument" before `fsl-refine` ever runs,
//! rather than merely not being enforced once it does. Both tests below are
//! therefore Linux-only and report zero tests run on macOS by design -- not
//! a skipped failure, and not evidence either test's `ulimit`-capped
//! assertion has ever actually been observed to fail on a mutant binary.
//! Every "measured" figure in this file's calibration comments is a peak
//! *memory* comparison (`/usr/bin/time -l`'s `peak memory footprint`),
//! taken on macOS outside the `ulimit -v` gate entirely -- it is evidence
//! that the two builds' memory use differs by roughly the stated amount,
//! not evidence that the `ulimit`-capped assertion below has been observed
//! to pass on one build and fail on the other. That direct observation
//! (running both the fixed and a per-node-clone-reintroduced mutant on
//! Linux under the actual `ulimit -v` cap and recording which assertion
//! fails, the way #776's own review required) has not been taken and is
//! deliberately deferred -- whether to rely on CI for it or set up a Linux
//! environment for this repository's own development is still an open,
//! human decision as of this file's current revision. As with the existing
//! `issue_730_*` ceiling tests, macOS RSS improving is assumed, but not
//! independently verified, to imply a comparable Linux `RLIMIT_AS` (`ulimit
//! -v`) improvement; if a `CEILING_KB` below flakes on Linux CI, widen it
//! rather than tightening this comment's claim.
#![cfg(target_os = "linux")]

use std::process::Command;

use serde_json::Value;

#[path = "support/mod.rs"]
mod support;
use support::{Fixture, LABEL_CORE_REPRO_SOURCE, insert_before_closing_brace};

/// The extra self-violating action, generated as a diff against
/// [`LABEL_CORE_REPRO_SOURCE`] rather than duplicated inside a second full
/// copy of the spec: `quota` is bounded `0..8`, so `quota + 8` once `quota`
/// has reached its own bound overflows the type. `check_refinement`'s
/// `first_self_violation` precondition finds this before the
/// correspondence walk ever starts, so this fixture isolates
/// `first_self_violation`'s own frontier -- the ceiling PR #776 deferred to
/// this issue -- from the walk the other test below covers.
const OVERFLOW_ACTION: &str =
    "  action overflow() { requires audit.size() >= 3  quota = quota + 8 }\n";

fn label_core_repro_self_violating_source() -> String {
    insert_before_closing_brace(LABEL_CORE_REPRO_SOURCE, OVERFLOW_ACTION)
}

/// An abstraction with the same state declarations as `LabelCoreRepro`, but
/// no actions -- paired below with a mapping that maps every impl state
/// variable to a fixed constant (not the impl variable of the same name).
/// An *identity* map would make every impl action that actually changes
/// state immediately fail `stutter_changed_abs` at step 1 (alpha changes
/// whenever the impl state does), short-circuiting `check_refinement`
/// before it ever explores past depth 0 -- exactly the deep frontier this
/// fixture needs to exercise. Mapping every field to a constant instead
/// keeps `alpha_before == alpha_after` for every action at every depth, so
/// the walk actually runs the full `depth` layers issue #783 measured 1.72
/// GB (peak RSS) at.
const LABEL_CORE_REPRO_ABSTRACTION_SOURCE: &str = r"
spec LabelCoreReproAbs {
  type LabelId = 0..3
  type Qty     = 0..8
  enum Phase { Draft, Review, Approved, Published, Retired }
  struct Label { phase: Phase, weight: Qty, pinned: Bool }
  state {
    labels: Map<LabelId, Label>, audit: Seq<LabelId, 10>,
    reviewed: Set<LabelId>, published: Set<LabelId>,
    draft_count: Qty, total: Int, epoch: 0..16, quota: Qty, frozen: Bool
  }
  init {
    forall l: LabelId { labels[l] = Label { phase: Draft, weight: 0, pinned: false } }
    audit = Seq {} reviewed = Set {} published = Set {}
    draft_count = 4 total = 0 epoch = 0 quota = 8 frozen = false
  }
}
";

/// Maps every `LabelCoreRepro` action to `stutter` against the constant-
/// mapped abstraction above, covering the plain (non-self-violating) five
/// actions only.
const LABEL_CORE_REPRO_ALL_STUTTER_MAPPING_SOURCE: &str = r"
refinement LabelCoreReproStutter {
  impl LabelCoreRepro
  abs  LabelCoreReproAbs

  map labels[l: LabelId] = Label { phase: Draft, weight: 0, pinned: false }
  map audit = Seq {}
  map reviewed = Set {}
  map published = Set {}
  map draft_count = 4
  map total = 0
  map epoch = 0
  map quota = 8
  map frozen = false

  action review(l, w) -> stutter
  action approve(l)   -> stutter
  action publish(l)   -> stutter
  action retire(l)    -> stutter
  action freeze()     -> stutter
}
";

/// The extra `overflow -> stutter` correspondence, generated as a diff
/// against [`LABEL_CORE_REPRO_ALL_STUTTER_MAPPING_SOURCE`] the same way
/// [`label_core_repro_self_violating_source`] generates its spec. Never
/// actually evaluated -- `first_self_violation` returns before the
/// correspondence walk starts -- but `fsl_core::parse_refinement` still
/// requires a correspondence entry for every impl action to parse at all.
const OVERFLOW_STUTTER_CORRESPONDENCE: &str = "  action overflow()   -> stutter\n";

fn label_core_repro_self_violating_stutter_mapping_source() -> String {
    insert_before_closing_brace(
        LABEL_CORE_REPRO_ALL_STUTTER_MAPPING_SOURCE,
        OVERFLOW_STUTTER_CORRESPONDENCE,
    )
}

fn run_capped(ceiling_kb: u64, args: &[&str]) -> std::process::Output {
    let command = format!(
        "ulimit -v {ceiling_kb} && exec {:?} {}",
        env!("CARGO_BIN_EXE_fsl-refine"),
        args.iter()
            .map(|arg| format!("{arg:?}"))
            .collect::<Vec<_>>()
            .join(" "),
    );
    Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .expect("run capped fsl-refine")
}

/// `first_self_violation`'s own frontier, isolated from `check_refinement`'s
/// correspondence walk (which this fixture's `overflow` action never lets
/// run). PR #776 fixed this lane's per-node `(Monitor, Vec<TraceStep>)`
/// clone but deferred adding a ceiling regression test for it to this issue.
///
/// Calibration (release build, `/usr/bin/time -l`'s `peak memory
/// footprint`, this repo's macOS development environment, `depth 4`, the
/// shared `support::LABEL_CORE_REPRO_SOURCE` fixture plus `overflow`):
/// reintroducing the pre-#776 per-node clone (temporarily, for this
/// measurement only) measured ~2,101 MB; the current fix measures ~492 MB.
/// 1000 * 1024 KiB sits ~2.03x above the fixed measurement and ~2.10x below
/// the reintroduced-clone one. This is a peak-memory comparison, not an
/// observation of the `ulimit`-capped assertion below actually failing on
/// Linux against either build -- see this file's top comment.
#[test]
fn first_self_violation_stays_under_a_calibrated_ceiling_for_the_branching_reproducer() {
    const CEILING_KB: u64 = 1000 * 1024;

    let implementation = Fixture::new("fsv-impl", &label_core_repro_self_violating_source());
    let abstraction = Fixture::new("fsv-abs", LABEL_CORE_REPRO_ABSTRACTION_SOURCE);
    let mapping = Fixture::new(
        "fsv-map",
        &label_core_repro_self_violating_stutter_mapping_source(),
    );
    let output = run_capped(
        CEILING_KB,
        &[
            implementation.text(),
            abstraction.text(),
            mapping.text(),
            "4",
        ],
    );
    assert!(
        output.status.success(),
        "expected first_self_violation to finish inside a {CEILING_KB} KiB address-space \
         ceiling; status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON under the capped run: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["verdict"], "impl_violation", "{value:#}");
    assert_eq!(value["kind"], "type_bound", "{value:#}");
}

/// `check_refinement`'s own correspondence walk -- the lane this issue's
/// clone removal actually changes.
///
/// Calibration (release build, `/usr/bin/time -l`'s `peak memory
/// footprint`, this repo's macOS development environment, `depth 3`, the
/// shared `support::LABEL_CORE_REPRO_SOURCE` fixture): the pre-removal
/// commit (`6012c00`, `fsl-refine` present but the per-node clone not yet
/// removed) measured ~1,728 MB, consistent with issue #783's own reported
/// ~1.72 GB; the post-removal commit (this branch) measures ~491 MB. 950 *
/// 1024 KiB sits ~2.03x above the post-removal measurement and ~1.73x below
/// the pre-removal one. As above, this is a peak-memory comparison, not an
/// observation of the `ulimit`-capped assertion below actually failing on
/// Linux against the pre-removal build -- see this file's top comment.
#[test]
fn check_refinement_stays_under_a_calibrated_ceiling_for_the_branching_reproducer() {
    const CEILING_KB: u64 = 950 * 1024;

    let implementation = Fixture::new("cr-impl", LABEL_CORE_REPRO_SOURCE);
    let abstraction = Fixture::new("cr-abs", LABEL_CORE_REPRO_ABSTRACTION_SOURCE);
    let mapping = Fixture::new("cr-map", LABEL_CORE_REPRO_ALL_STUTTER_MAPPING_SOURCE);
    let output = run_capped(
        CEILING_KB,
        &[
            implementation.text(),
            abstraction.text(),
            mapping.text(),
            "3",
        ],
    );
    assert!(
        output.status.success(),
        "expected check_refinement to finish inside a {CEILING_KB} KiB address-space \
         ceiling (calibrated between the measured pre-/post-removal peaks); status={:?} \
         stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON under the capped run: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["verdict"], "refines", "{value:#}");
}
