// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #730's `verify_explicit_selected` lane:
//! its frontier used to be `BTreeMap<State, Monitor>`, cloning the whole
//! `KernelModel` per live frontier state per level
//! (`rust/fsl-runtime/src/explicit.rs`, before this fix). The fix carries a
//! bare `BTreeSet<State>` instead and re-points one scratch `Monitor` at
//! each frontier state.
//!
//! Unlike the `bfs` lane (see `issue_730_bfs_memory_ceiling.rs`), this
//! lane's memory is not dominated by the model clone alone: level-
//! synchronous exploration additionally holds one `Vec<EnabledAction>` per
//! *simultaneously live* frontier state in `enabled_by_state`
//! (`rust/fsl-runtime/src/explicit.rs`), each carrying its own
//! `bindings`/`params` maps -- a cost this fix does not touch and #730
//! never claimed to. Measured directly with `fsl-explicit` (a driver for
//! `fsl_runtime::verify_explicit`, the same shape as `fsl-bfs`, added
//! alongside this test specifically so the measurement is not diluted by
//! `fslc verify`'s own Z3-backed vacuity checks, which run regardless of
//! `--engine`): peak memory footprint on the `LabelCoreRepro` reproducer
//! below (release build, depth 3, 16,290 states, max frontier width
//! 15,424) dropped from ~1,783 MB (pre-fix) to ~954 MB (post-fix) --
//! real and state-count-identical, but a much smaller *relative* drop than
//! `bfs`'s ~4x, because `enabled_by_state`'s cost is unaffected and sets
//! the floor both before and after.
//!
//! Same platform caveat as `issue_730_bfs_memory_ceiling.rs`: macOS does
//! not enforce `RLIMIT_AS`, so this only runs on Linux (where CI actually
//! executes it), and the calibration below crosses from a macOS RSS-style
//! measurement to a Linux `ulimit -v` (`RLIMIT_AS`) ceiling -- a
//! monotonicity assumption, not a second direct measurement on the
//! enforcing platform. Because this lane's post/pre ratio (~0.54) leaves
//! much less headroom than `bfs`'s (~0.25), the ceiling below sits closer
//! to the post-fix baseline than `issue_730_bfs_memory_ceiling.rs`'s does;
//! if it flakes, widen `CEILING_KB` rather than tightening this comment's
//! claim.
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
            "fsl-issue-730-{name}-{}-{nonce}.fsl",
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

/// The same `LabelCoreRepro` reproducer `issue_730_bfs_memory_ceiling.rs`
/// and `rust/fslc/tests/issue_697_all_properties_memory.rs` use.
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
  invariant PublishedWasReviewed {
    forall l: LabelId { published.contains(l) => reviewed.contains(l) }
  }
  invariant TotalConsistent {
    total == sum(l: LabelId of labels[l].weight
                 where labels[l].phase == Approved or labels[l].phase == Published)
  }
  invariant AuditCoversReview {
    forall l: LabelId { labels[l].phase != Draft => audit.contains(l) }
  }
  trans EpochMonotone { epoch >= old(epoch) }
  reachable AllPublished { published.size() >= 2 }
  reachable SomeRetired { exists l: LabelId { labels[l].phase == Retired } }
  reachable QuotaSaturated { total >= 6 }
  reachable AuditFull { audit.size() >= 5 }
}
";

#[test]
fn explicit_stays_under_a_calibrated_ceiling_for_the_branching_reproducer() {
    // ~954 MB measured post-fix baseline, ~1,783 MB measured pre-fix, in KiB.
    const CEILING_KB: u64 = 1_300 * 1024;

    let fixture = Fixture::new("explicit-ceiling", LABEL_CORE_REPRO_SOURCE);
    let command = format!(
        "ulimit -v {CEILING_KB} && exec {:?} {:?} 3",
        env!("CARGO_BIN_EXE_fsl-explicit"),
        fixture.text(),
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .expect("run capped fsl-explicit");
    assert!(
        output.status.success(),
        "expected the explicit engine to finish inside a {CEILING_KB} KiB address-space \
         ceiling (calibrated between the measured post-fix and pre-fix peaks); status={:?} \
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
    assert_eq!(value["states_explored"], 16290, "{value:#}");
    assert_eq!(value["max_frontier_width"], 15424, "{value:#}");
}
