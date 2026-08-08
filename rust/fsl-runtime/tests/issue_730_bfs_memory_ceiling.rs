// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #730: the `bfs` lane's queue used to carry
//! `(Monitor, usize)`, cloning the whole `KernelModel` per queued state
//! (`rust/fsl-runtime/src/lib.rs`'s `bfs`, before this fix). The fix carries
//! `(State, usize)` instead and re-points one scratch `Monitor` at each
//! popped state, the same pattern issue #697's `find_boundary_violation`
//! established.
//!
//! State-count identity between the pre-fix and post-fix `bfs` was measured
//! directly during development (`/usr/bin/time -l fsl-bfs`, release build):
//! 16,290 states for the `LabelCoreRepro` reproducer below at depth 3,
//! identical before and after the fix, with peak memory footprint dropping
//! from ~946 MB (`maximum resident set size`, pre-fix) to ~236 MB
//! (post-fix). This file's own `states_explored` assertion below re-checks
//! that count on whatever binary CI actually runs (a cheap sanity check,
//! not a substitute for the direct before/after comparison above, which
//! this process alone cannot reproduce since it is only ever built from one
//! side of the fix); the file's main purpose is the mechanical trip-wire
//! against a *silent reintroduction* of the per-node model clone, the same
//! shape as `rust/fslc/tests/issue_697_all_properties_memory.rs`'s Control
//! D. Like that control, macOS does not enforce `RLIMIT_AS` (observed
//! directly there: child processes exceed a `setrlimit(RLIMIT_AS, ...)`
//! ceiling without being killed), so this only runs on Linux, where CI
//! actually executes it.
//!
//! The ceiling is calibrated to have discriminating power against this
//! specific regression class, not just a coarse "still bounded" check the
//! way Control D's own >=10x margin is (deliberately -- Control D's margin
//! is wide enough that it would not reliably catch a regression back to
//! this fix's own pre-fix cost on this fixture, which is exactly the gap
//! this file exists to close): 550 MB sits above the measured post-fix
//! baseline's normal variance (~236 MB, ~2.3x headroom) but below what
//! reintroducing the pre-fix per-node `Monitor` clone would cost on this
//! same fixture (~946 MB, ~1.7x margin below it). That said, the
//! measurement this ceiling is calibrated from is `maximum resident set
//! size`/`peak memory footprint` on macOS (`/usr/bin/time -l`), a different
//! metric on a different platform than what `ulimit -v` actually enforces
//! here (`RLIMIT_AS`, i.e. reserved virtual address space, which is `>=`
//! RSS and can differ from it by an allocator- and platform-dependent
//! amount) -- CI has run this test and passed at this ceiling, but no one
//! has measured what the *pre-fix* binary's `ulimit -v` cost actually is on
//! Linux, so the margin's safety rests on an unverified monotonicity
//! assumption (RSS improvement on macOS implies a comparable VSZ
//! improvement on Linux), not a second direct measurement. If this test
//! flakes, widen `CEILING_KB` rather than tightening this comment's claim.
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

/// The same `LabelCoreRepro` reproducer
/// `rust/fslc/tests/issue_697_all_properties_memory.rs` uses, copied rather
/// than shared across crates (that file's own note explains why it is not a
/// `specs`/`examples` corpus fixture): its `audit: Seq<LabelId, 10>` records
/// push order, so the same *set* of pushed values in a different *order* is
/// a different `Seq` value, `visited` dedup never collapses the branching,
/// and the BFS frontier grows like branching^depth.
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
fn bfs_stays_under_a_calibrated_ceiling_for_the_branching_reproducer() {
    // ~236 MB measured post-fix baseline, ~946 MB measured pre-fix, in KiB.
    const CEILING_KB: u64 = 550 * 1024;

    let fixture = Fixture::new("bfs-ceiling", LABEL_CORE_REPRO_SOURCE);
    let command = format!(
        "ulimit -v {CEILING_KB} && exec {:?} {:?} 3",
        env!("CARGO_BIN_EXE_fsl-bfs"),
        fixture.text(),
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .expect("run capped fsl-bfs");
    assert!(
        output.status.success(),
        "expected bfs to finish inside a {CEILING_KB} KiB address-space ceiling \
         (calibrated between the measured post-fix and pre-fix peaks); status={:?} stderr={}",
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
}
