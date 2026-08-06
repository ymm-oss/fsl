// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #697: verifying **all** properties at once
//! (no `--property`) used to blow past 11.4 GB RSS while each property
//! verified fine in seconds under `--property`, on both `--engine bmc` and
//! `--engine induction`. The root cause was not the solver -- Z3's own
//! `cost.solver.memory_mb` during a blowing-up run was 27.8 MB -- it was an
//! **unbudgeted concrete (solver-free) BFS** in `fsl-runtime`
//! (`find_boundary_violation`) that the default all-properties path runs as
//! a pre-pass and every `--property` run skips (`selected_implicit_bounds`
//! returns `None`, so `prepare_bmc`'s pre-pass condition holds, only when no
//! `--property`/`--exclude-property _bounds_*` narrows the run). The
//! `Monitor` the pre-pass cloned per queued BFS node held the whole model by
//! value, and its trace was cloned per node too, so memory grew with both
//! branching factor and path length on top of state count. This file uses a
//! reproducer whose `audit: Seq<LabelId, 10>` records push order: because
//! the same *set* of pushed values in a different *order* is a different
//! `Seq` value, `visited` dedup never collapses the branching, and the BFS
//! frontier grows like branching^depth.
//!
//! The fix (`rust/fsl-runtime/src/lib.rs`) budgets `find_boundary_violation`
//! at `CONCRETE_PROBE_BUDGET` states and stops cloning the model/trace per
//! node. Exhaustion with no finding falls through to the symbolic engine
//! exactly like today's empty result did -- this pre-pass is an evidence
//! detour, not a verdict authority, so falling through never turns a real
//! violation into a false green. The one outcome class the pre-pass alone
//! covers -- a reachable over-capacity `Seq` successor the bounded symbolic
//! value cannot represent -- still fails closed downstream instead of
//! passing, which Control B below confirms directly.
//!
//! Deliberately **not** a `specs/`/`examples/` corpus fixture:
//! `tests/test_dialect_conformance.py` runs an unbudgeted
//! `bfs_oracle(path, depth=4)` over every kernel file under
//! `SCAN_ROOTS = ("specs", "examples")` (`tests/dialect_registry.py`), and
//! the reproducer below exceeds 6.3 GB in that oracle at depth 4. Placing it
//! in the corpus would move the very OOM this issue reports into the
//! conformance harness instead of fixing it (that harness-side gap is
//! tracked separately as issue #730). The `Fixture` temp-file pattern below
//! matches `rust/fslc/tests/issue_651_bmc_partial_operations.rs`.

use std::collections::BTreeSet;
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
            "fsl-issue-697-{name}-{}-{nonce}.fsl",
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

/// Runs the native CLI's `verify` and returns the parsed JSON envelope with
/// its exit status.
fn verify(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["verify"])
        .args(args)
        .args(["--deadlock", "ignore", "--no-cache"])
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

// ---------------------------------------------------------------------
// Control B: pre-pass evidence preservation (negative control against
// hollowing the budget into an outright skip).
// ---------------------------------------------------------------------

/// A minimal `Seq<Int, 2>` overflow fixture. Concretely, `push` always
/// succeeds (`rust/fsl-runtime/src/lib.rs`'s `push` method never checks
/// capacity) and the overflow is caught afterward as a `type_bound` failure
/// on `xs`'s declared type. Symbolically, `push` past capacity increments
/// `len` without writing any slot (`rust/fsl-verifier/src/eval.rs`), so an
/// invariant that mirrors the declared capacity (`XsWithinBound`) is
/// satisfiably violated, and rendering *that* counterexample projects the
/// resulting `len > slots.len()` `Seq` value -- which fails closed with
/// `rust/fsl-verifier/src/value.rs`'s "model sequence length exceeds
/// capacity" instead of silently producing a wrong trace.
const SEQ_OVERFLOW_SOURCE: &str = r"
spec SeqOverflow {
  state { xs: Seq<Int, 2> }
  init { xs = Seq {} }
  action push(v in 0..5) { xs = xs.push(v) }
  invariant SizeNonNegative { xs.size() >= 0 }
  invariant XsWithinBound { xs.size() <= 2 }
}
";

#[test]
fn default_verify_reports_the_seq_overflow_concretely_while_the_isolated_property_fails_closed() {
    let fixture = Fixture::new("seq-overflow", SEQ_OVERFLOW_SOURCE);

    // The default (all-properties) path runs the budgeted concrete pre-pass,
    // which alone can represent the over-capacity `Seq` successor, and
    // reports it as a `type_bound` violation with a full concrete trace --
    // exactly today's `--property`-absent behavior, unweakened by the budget.
    let (default_output, default_status) = verify(&[fixture.text(), "--depth", "8"]);
    assert_eq!(default_status, 1, "{default_output:#}");
    assert_eq!(default_output["result"], "violated", "{default_output:#}");
    assert_eq!(
        default_output["violation_kind"], "type_bound",
        "{default_output:#}"
    );
    assert_eq!(
        default_output["invariant"], "_bounds_xs",
        "{default_output:#}"
    );
    assert!(
        default_output["trace"]
            .as_array()
            .is_some_and(|trace| trace.len() >= 2),
        "expected a replayable multi-step trace: {default_output:#}"
    );

    // A "fix" that hollowed the budget into skipping the pre-pass wholesale
    // (instead of budgeting it) would push this same fixture down to the
    // symbolic engine, which cannot represent the overflowed `Seq` and fails
    // closed -- turning today's clean `violated`/`type_bound` evidence into
    // an opaque `error`. Isolating to `XsWithinBound` (an invariant that
    // mirrors the declared capacity) reproduces exactly that fail-closed
    // shape on its own, independent of any budget change: `--property`
    // already skips the pre-pass today.
    let (isolated_output, isolated_status) = verify(&[
        fixture.text(),
        "--depth",
        "8",
        "--property",
        "XsWithinBound",
    ]);
    assert_eq!(isolated_status, 2, "{isolated_output:#}");
    assert_eq!(isolated_output["result"], "error", "{isolated_output:#}");
    assert_eq!(isolated_output["kind"], "semantics", "{isolated_output:#}");
    assert_eq!(
        isolated_output["message"], "model sequence length exceeds capacity",
        "{isolated_output:#}"
    );
}

// ---------------------------------------------------------------------
// Control C: verdict and attribution agreement, plus no-false-green.
// ---------------------------------------------------------------------

/// The `audit: Seq<LabelId, 10>` records push order, so the same set of
/// pushed labels reached in a different order is a different concrete
/// state: `find_boundary_violation`'s `visited` dedup never collapses the
/// branching, and its BFS frontier grows like branching^depth. This is the
/// diagnosis's validated reproducer, unmodified.
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

const DECLARED_INVARIANTS: &[&str] = &[
    "PublishedWasReviewed",
    "TotalConsistent",
    "AuditCoversReview",
];
const DECLARED_TRANSITIONS: &[&str] = &["EpochMonotone"];
const DECLARED_REACHABLES: &[&str] =
    &["AllPublished", "SomeRetired", "QuotaSaturated", "AuditFull"];

fn string_set(values: &Value) -> BTreeSet<String> {
    values
        .as_array()
        .unwrap_or_else(|| panic!("expected a JSON array: {values:#}"))
        .iter()
        .map(|value| value.as_str().expect("string array element").to_owned())
        .collect()
}

#[test]
fn all_properties_verdict_agrees_with_the_union_of_isolated_property_runs() {
    let fixture = Fixture::new("label-core-repro", LABEL_CORE_REPRO_SOURCE);

    // The joint, all-properties run: this is exactly the shape issue #697
    // reported blowing past 11.4 GB RSS. With the budgeted, per-node-cheap
    // pre-pass it must now complete `verified`.
    let (joint, joint_status) = verify(&[fixture.text(), "--depth", "8"]);
    assert_eq!(joint_status, 0, "{joint:#}");
    assert_eq!(joint["result"], "verified", "{joint:#}");

    let joint_invariants = string_set(&joint["invariants_checked"])
        .into_iter()
        .filter(|name| !name.starts_with("_bounds_"))
        .collect::<BTreeSet<_>>();
    let joint_transitions = string_set(&joint["transitions_checked"]);
    let joint_witnessed_reachables = joint["reachables"]
        .as_object()
        .expect("reachables object")
        .iter()
        .filter(|(_, witness)| !witness.is_null())
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        joint_invariants,
        DECLARED_INVARIANTS.iter().map(|&s| s.to_owned()).collect(),
        "joint invariants_checked (bounds excluded): {joint:#}"
    );
    assert_eq!(
        joint_transitions,
        DECLARED_TRANSITIONS.iter().map(|&s| s.to_owned()).collect(),
        "joint transitions_checked: {joint:#}"
    );
    assert_eq!(
        joint_witnessed_reachables,
        DECLARED_REACHABLES.iter().map(|&s| s.to_owned()).collect(),
        "joint witnessed reachables: {joint:#}"
    );

    // Every declared property, run alone under `--property`, must
    // independently confirm the same verdict `--property` always gave
    // (issue #697's asymmetry is entirely in the *joint* path; isolated runs
    // were never affected). The union of what each isolated run confirms
    // must equal what the joint run reports above -- no property silently
    // drops out of, or wrongly appears in, the joint attribution.
    for &name in DECLARED_INVARIANTS {
        let (isolated, status) = verify(&[fixture.text(), "--depth", "8", "--property", name]);
        assert_eq!(status, 0, "property {name}: {isolated:#}");
        assert_eq!(
            isolated["result"], "verified",
            "property {name}: {isolated:#}"
        );
        assert_eq!(
            string_set(&isolated["invariants_checked"]),
            BTreeSet::from([name.to_owned()]),
            "property {name}: {isolated:#}"
        );
    }
    for &name in DECLARED_TRANSITIONS {
        let (isolated, status) = verify(&[fixture.text(), "--depth", "8", "--property", name]);
        assert_eq!(status, 0, "property {name}: {isolated:#}");
        assert_eq!(
            isolated["result"], "verified",
            "property {name}: {isolated:#}"
        );
        assert_eq!(
            string_set(&isolated["transitions_checked"]),
            BTreeSet::from([name.to_owned()]),
            "property {name}: {isolated:#}"
        );
    }
    for &name in DECLARED_REACHABLES {
        let (isolated, status) = verify(&[fixture.text(), "--depth", "8", "--property", name]);
        assert_eq!(status, 0, "property {name}: {isolated:#}");
        assert_eq!(
            isolated["result"], "verified",
            "property {name}: {isolated:#}"
        );
        assert!(
            !isolated["reachables"][name].is_null(),
            "property {name} not witnessed in isolation: {isolated:#}"
        );
    }
}

#[test]
fn a_genuinely_broken_invariant_still_reports_violated_after_the_budget_fix() {
    // Same fixture with `review`'s `reviewed = reviewed.add(l)` removed:
    // `PublishedWasReviewed` (`published.contains(l) => reviewed.contains(l)`)
    // now genuinely breaks the first time a label is published, because it
    // can reach `Published` without ever having been recorded as reviewed.
    // A "fix" that hollowed the pre-pass into a wholesale skip, or that
    // widened the budget so far the joint run stopped completing, would
    // both be caught by this: either the violation stops reproducing, or
    // the run stops finishing at all.
    let mutant_source = LABEL_CORE_REPRO_SOURCE.replacen("reviewed = reviewed.add(l)", "", 1);
    let fixture = Fixture::new("label-core-repro-mutant", &mutant_source);

    let (output, status) = verify(&[fixture.text(), "--depth", "8"]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(output["violation_kind"], "invariant", "{output:#}");
    assert_eq!(output["invariant"], "PublishedWasReviewed", "{output:#}");
}

// ---------------------------------------------------------------------
// Control D: the honest resource anchor.
//
// macOS does not enforce RLIMIT_AS (observed directly during development:
// child processes routinely exceed a `setrlimit(RLIMIT_AS, ...)` ceiling
// there without being killed), so this control is compiled only for Linux,
// where CI actually runs it; Controls A-C above carry the
// platform-independent load. The ceiling is derived from this fix's own
// measured baseline with a >=10x margin rather than tuned to the old
// failure, so it is a coarse safety net against a *regression back to
// unbounded growth*, not a tight bound: the measured post-fix peak for the
// joint `LabelCoreRepro` run under the `cargo test` (debug) profile that
// executes this binary was ~2.24 GB RSS; issue #697 reported this same
// shape of run exceeding 11.4 GB and climbing before being killed.
#[cfg(target_os = "linux")]
#[test]
fn joint_run_completes_under_a_generous_address_space_ceiling() {
    // ~2.24 GB observed baseline * 10, in KiB for `ulimit -v`.
    const CEILING_KB: u64 = 2_240 * 10 * 1024;

    let fixture = Fixture::new("label-core-repro-rlimit", LABEL_CORE_REPRO_SOURCE);
    let command = format!(
        "ulimit -v {CEILING_KB} && exec {:?} verify {:?} --depth 8 --deadlock ignore --no-cache",
        env!("CARGO_BIN_EXE_fslc"),
        fixture.text(),
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .expect("run capped native CLI");
    assert!(
        output.status.success(),
        "expected the joint run to finish inside a {CEILING_KB} KiB address-space ceiling \
         (>=10x the measured baseline); status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON under the capped run: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["result"], "verified", "{value:#}");
}
