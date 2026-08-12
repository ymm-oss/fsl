// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #783: two more lanes that used to carry a
//! whole `Monitor` (a `KernelModel` clone) per queued state, on top of the
//! `bfs`/`find_boundary_violation`/`first_self_violation` lanes issue
//! #730/#697 already fixed and issue #776 documented as bisected apart from
//! this one.
//!
//! Both tests below use the same `LabelCoreRepro` reproducer
//! `issue_730_bfs_memory_ceiling.rs` uses (copied rather than shared across
//! test binaries, for the same reason that file gives): its
//! `audit: Seq<LabelId, 10>` records push order, so the same *set* of pushed
//! values in a different *order* is a different `Seq` value, `visited` dedup
//! never collapses the branching, and a BFS/correspondence-walk frontier
//! grows like branching^depth.
//!
//! Like that file, macOS does not enforce `RLIMIT_AS` the way `ulimit -v`
//! expects (observed directly on this repo's own macOS development
//! environment: `sh -c 'ulimit -v N'` itself fails with "cannot modify
//! limit: Invalid argument", not merely silently unenforced), so both tests
//! are Linux-only and report zero tests run on macOS -- that is expected,
//! not a skipped failure.
#![cfg(target_os = "linux")]

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
            "fsl-issue-783-{name}-{}-{nonce}.fsl",
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

/// The plain, internally-consistent `LabelCoreRepro` implementation --
/// identical to `issue_730_bfs_memory_ceiling.rs`'s copy, with no self-
/// violating action added, so `check_refinement`'s `first_self_violation`
/// precondition passes and the correspondence walk below actually runs to
/// `depth`.
const LABEL_CORE_REPRO_SOURCE: &str = r"
spec LabelCoreRepro {
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
  action review(l: LabelId, w: Qty) {
    requires audit.size() < 10
    requires not frozen
    requires labels[l].phase == Draft
    requires w > 0 and w <= quota
    labels[l].phase = Review
    labels[l].weight = w
    reviewed = reviewed.add(l)
    draft_count = draft_count - 1
    audit = audit.push(l)
  }
  action approve(l: LabelId) {
    requires audit.size() < 10
    requires labels[l].phase == Review
    labels[l].phase = Approved
    total = total + labels[l].weight
    audit = audit.push(l)
  }
  action publish(l: LabelId) {
    requires audit.size() < 10
    requires labels[l].phase == Approved
    labels[l].phase = Published
    published = published.add(l)
    epoch = epoch + 1
    audit = audit.push(l)
  }
  action retire(l: LabelId) {
    requires audit.size() < 10
    requires labels[l].phase == Published
    labels[l].phase = Retired
    total = total - labels[l].weight
    audit = audit.push(l)
  }
  action freeze() { requires not frozen  frozen = true }
}
";

/// The same reproducer plus one extra action that self-violates: `quota` is
/// bounded `0..8`, so `quota + 8` once `quota` has reached its own bound
/// overflows the type. `check_refinement`'s `first_self_violation`
/// precondition (`rust/fsl-runtime/src/lib.rs:1615-1627`) finds this before
/// the correspondence walk ever starts, so this fixture isolates
/// `first_self_violation`'s own frontier -- the ceiling PR #776 deferred to
/// this issue -- in isolation from the walk the other test below covers.
const LABEL_CORE_REPRO_SELF_VIOLATING_SOURCE: &str = r"
spec LabelCoreReproSelfViolating {
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
  action review(l: LabelId, w: Qty) {
    requires audit.size() < 10
    requires not frozen
    requires labels[l].phase == Draft
    requires w > 0 and w <= quota
    labels[l].phase = Review
    labels[l].weight = w
    reviewed = reviewed.add(l)
    draft_count = draft_count - 1
    audit = audit.push(l)
  }
  action approve(l: LabelId) {
    requires audit.size() < 10
    requires labels[l].phase == Review
    labels[l].phase = Approved
    total = total + labels[l].weight
    audit = audit.push(l)
  }
  action publish(l: LabelId) {
    requires audit.size() < 10
    requires labels[l].phase == Approved
    labels[l].phase = Published
    published = published.add(l)
    epoch = epoch + 1
    audit = audit.push(l)
  }
  action retire(l: LabelId) {
    requires audit.size() < 10
    requires labels[l].phase == Published
    labels[l].phase = Retired
    total = total - labels[l].weight
    audit = audit.push(l)
  }
  action freeze() { requires not frozen  frozen = true }
  action overflow() { requires audit.size() >= 3  quota = quota + 8 }
}
";

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

/// The same all-stutter mapping, covering the self-violating implementation's
/// six actions (the five above plus `overflow`). Never actually evaluated --
/// `first_self_violation` returns before the correspondence walk starts --
/// but `fsl_core::parse_refinement` still requires a correspondence entry for
/// every impl action to parse at all.
const LABEL_CORE_REPRO_SELF_VIOLATING_STUTTER_MAPPING_SOURCE: &str = r"
refinement LabelCoreReproSelfViolatingStutter {
  impl LabelCoreReproSelfViolating
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
  action overflow()   -> stutter
}
";

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
/// Calibration (release build, `/usr/bin/time -l`, this repo's macOS
/// development environment, `depth 4`): reintroducing the pre-#776 per-node
/// clone measured ~1,926 MB peak memory footprint on this exact fixture;
/// the current fix measures ~492 MB. 1000 * 1024 KiB sits about 2x above
/// the fixed measurement and clearly below the reintroduced-clone one.
#[test]
fn first_self_violation_stays_under_a_calibrated_ceiling_for_the_branching_reproducer() {
    const CEILING_KB: u64 = 1000 * 1024;

    let implementation = Fixture::new("fsv-impl", LABEL_CORE_REPRO_SELF_VIOLATING_SOURCE);
    let abstraction = Fixture::new("fsv-abs", LABEL_CORE_REPRO_ABSTRACTION_SOURCE);
    let mapping = Fixture::new(
        "fsv-map",
        LABEL_CORE_REPRO_SELF_VIOLATING_STUTTER_MAPPING_SOURCE,
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
/// Calibration (release build, `/usr/bin/time -l`, this repo's macOS
/// development environment, `depth 3`, this exact fixture): the pre-removal
/// commit (`6012c00`, `fsl-refine` present but the per-node clone not yet
/// removed) measured ~1,557 MB peak memory footprint, consistent with issue
/// #783's own reported ~1.72 GB; the post-removal commit measures ~491 MB.
/// 950 * 1024 KiB sits about 2x above the post-removal measurement (~1.98x)
/// and clearly below the pre-removal one (~1.60x margin).
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
