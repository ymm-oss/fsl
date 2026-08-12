// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Fixture helpers shared by `fsl-runtime`'s memory-ceiling regression
//! tests (issue #730, #783). Before this module, `issue_730_bfs_memory_ceiling.rs`
//! and `issue_730_explicit_memory_ceiling.rs` each carried an identical copy
//! of `Fixture` and the `LabelCoreRepro` reproducer; `issue_783_refine_memory_ceiling.rs`
//! adding a *third*, non-identical copy (missing `invariants`/`trans`/
//! `reachables`, which changes the `KernelModel` clone size these ceilings
//! are calibrated against) is exactly the "same fixture in 3+ places, and
//! drifting" duplication an independent review of #783 flagged. One
//! canonical copy here, reused by all three.
//!
//! Placed at `tests/support/mod.rs` (not `tests/support.rs`) so Cargo does
//! not compile it as its own top-level integration-test binary; each
//! consumer pulls it in with `#[path = "support/mod.rs"] mod support;`.
//! Every item is `pub` because a given consumer only needs a subset, and
//! `#[allow(dead_code)]` is applied per item because each integration test
//! is compiled as its own binary crate, where an unused `pub` item still
//! triggers the lint (the same rationale `rust/fslc/tests/support/mod.rs`
//! documents).
//!
//! `rust/fslc/tests/issue_697_all_properties_memory.rs` carries a fourth
//! copy of this same reproducer, in a different crate (`fslc-rust`, not
//! `fsl-runtime`) that this test-only module cannot reach without a shared
//! dev-dependency crate. Consolidating that one too is left as an explicit
//! follow-up rather than folded into #783's own scope.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A temporary `.fsl` file, removed on drop.
pub struct Fixture(PathBuf);

impl Fixture {
    #[allow(dead_code)]
    pub fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-fixture-{name}-{}-{nonce}.fsl",
            std::process::id()
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    #[allow(dead_code)]
    pub fn text(&self) -> &str {
        self.0.to_str().expect("UTF-8 temporary path")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A branching reproducer whose `audit: Seq<LabelId, 10>` records push
/// order, so the same *set* of pushed values in a different *order* is a
/// different `Seq` value: `visited` dedup never collapses the branching,
/// and a BFS/correspondence-walk frontier grows like branching^depth. This
/// is the shared calibration fixture for every `fsl-runtime` memory-ceiling
/// regression test.
#[allow(dead_code)]
pub const LABEL_CORE_REPRO_SOURCE: &str = r"
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

/// Insert a new top-level item just before `source`'s closing `spec { .. }`
/// brace -- the last `}` in the string -- so a variant fixture (e.g. one
/// extra self-violating action) can be generated as a diff against
/// [`LABEL_CORE_REPRO_SOURCE`] instead of a full duplicate copy.
#[allow(dead_code)]
pub fn insert_before_closing_brace(source: &str, item: &str) -> String {
    let insert_at = source.rfind('}').expect("spec has a closing brace");
    let mut generated = source[..insert_at].to_owned();
    generated.push_str(item);
    generated.push_str(&source[insert_at..]);
    generated
}
