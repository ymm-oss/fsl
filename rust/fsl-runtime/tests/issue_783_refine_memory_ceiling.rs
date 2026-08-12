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
//! a skipped failure.
//!
//! **Both `CEILING_KB` values below are calibrated from a direct Linux
//! measurement, not a macOS peak-memory estimate.** Using a `rust:1-bookworm`
//! Docker container (`aarch64`, `ulimit -v` confirmed to actually enforce
//! `RLIMIT_AS` there -- unlike macOS), each test's `ulimit`-capped assertion
//! was run under a binary search of `CEILING_KB` values against both the
//! fixed build (this branch) and a per-node-Monitor-clone mutant, until the
//! PASS/FAIL boundary on each side was pinned to within 20-50 MiB and
//! confirmed reproducible by repeating both boundary points. That is the
//! direct observation #776's own review asked for and #783-round2 deferred:
//! each per-test comment below cites the exact boundary window measured on
//! each side, and the mutant side's FAIL is a real `SIGABRT` (Rust's
//! allocator aborting the process, `status=...unix_wait_status(134)`,
//! `stderr="memory allocation of N bytes failed"`) under the `ulimit -v`
//! cap, not a timeout or a different failure mode.
//!
//! Two assumptions still remain, stated precisely so they are not confused
//! with what was actually measured:
//! - The measurement above is `aarch64` Linux (an Apple Silicon Mac's
//!   `colima`/Docker VM); this repository's actual CI runs GitHub Actions'
//!   `ubuntu-latest`, which is `x86_64`. Cross-architecture allocator and
//!   address-space-layout differences could shift the true PASS/FAIL
//!   boundary on `x86_64` from what was measured here -- this has not been
//!   independently checked on `x86_64`, and is the one monotonicity
//!   assumption this file still carries.
//! - The mutant used for each lane is a hand-restored reversion of exactly
//!   that lane's per-node clone (verified below each `#[test]`'s doc comment
//!   by a `monitor.clone()` occurrence count inside the function), not an
//!   independently-authored regression -- it demonstrates this specific
//!   defect class is caught, not every conceivable regression shape.
//!
//! If a `CEILING_KB` below flakes on Linux CI regardless, widen it rather
//! than tightening this comment's claim -- the `x86_64` assumption above is
//! the most likely reason a widening would ever be needed.
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
/// Calibration (`rust:1-bookworm` Docker, `aarch64` Linux, `depth 4`, the
/// shared `support::LABEL_CORE_REPRO_SOURCE` fixture plus `overflow`;
/// `CEILING_KB` swept in binary-search steps against `--exact` runs of this
/// test, both boundary points repeated twice and reproducing identically):
/// the fixed build (`monitor.clone()` count inside `first_self_violation`:
/// 0) passes down to a `CEILING_KB` of 505 MiB and fails at 500 MiB
/// (`status=...unix_wait_status(134)`, `stderr="memory allocation of 5
/// bytes failed"`) -- boundary in `(500, 505]` MiB. A mutant with the
/// pre-#776 per-node clone hand-restored into `first_self_violation` only
/// (`monitor.clone()` count: 1; every other lane, including
/// `check_refinement`, left identical to the fixed build) passes at 2150
/// MiB and fails at 2120 MiB -- boundary in `(2120, 2150]` MiB. `CEILING_KB
/// = 1000 * 1024` (1000 MiB) therefore sits 1.98-2.00x above the fixed
/// boundary and 2.12-2.15x below the mutant boundary (both ratios computed
/// against the boundary's FAIL and PASS endpoints respectively, so the true
/// margin is somewhere in each stated range). See this file's top comment
/// for the two assumptions this measurement still rests on (`x86_64` CI vs.
/// this `aarch64` measurement; this specific mutant vs. every conceivable
/// regression shape).
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
/// Calibration (`rust:1-bookworm` Docker, `aarch64` Linux, `depth 3`, the
/// shared `support::LABEL_CORE_REPRO_SOURCE` fixture; same binary-search
/// sweep methodology as `first_self_violation`'s test above, both boundary
/// points repeated twice and reproducing identically): the post-removal
/// build (this branch; `monitor.clone()` count inside `check_refinement`:
/// 0) passes down to a `CEILING_KB` of 502 MiB and fails at 500 MiB
/// (`status=...unix_wait_status(134)`, `stderr="memory allocation of 6
/// bytes failed"`) -- boundary in `(500, 502]` MiB. The pre-removal commit
/// (`6012c00`, `fsl-refine` present but the per-node clone not yet removed;
/// `monitor.clone()` count inside `check_refinement`: 2) passes at 2020 MiB
/// and fails at 2000 MiB -- boundary in `(2000, 2020]` MiB, consistent with
/// issue #783's own reported ~1.72 GB order of magnitude. `CEILING_KB = 950
/// * 1024` (950 MiB) therefore sits 1.89-1.90x above the post-removal
/// boundary and 2.11-2.13x below the pre-removal boundary (ratios computed
/// against each boundary's FAIL/PASS endpoints, as above). See this file's
/// top comment for the two assumptions this measurement still rests on.
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
