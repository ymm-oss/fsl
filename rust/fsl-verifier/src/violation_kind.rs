// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Single-owner vocabulary for the free-form `kind` strings the symbolic
//! engines (BMC, k-induction, ranked-`leadsTo` liveness) put into
//! `BmcViolation`/`InductionCti`/`RankFailure`, plus the `"deadlock"`
//! literal `fslc` renders for a BMC deadlock trace (issue #646).
//!
//! This is the sibling registry to `fsl_runtime::Monitor`'s `outcome.kind`
//! (`rust/fslc/src/coverage.rs::OUTCOME_FEATURE_KEYS`): before this module,
//! every emission site was a bare string literal with no registry, no
//! bidirectional check, and no per-value exercising evidence. A typo'd or
//! new engine-internal kind shipped silently.
//!
//! Every emission site in `bmc.rs`/`induction.rs` and in `fslc`'s
//! `verification_output.rs` (the two `violation_kind: "leadsTo"`/`"deadlock"`
//! literals that duplicate an already-registered value) references one of
//! these constants instead of a bare string literal, so a typo or a new kind
//! is a compile error at the reference, not a silently unregistered value.
//! `rust/fslc/tests/assurance/violation_kind.rs` asserts the corpus-observed
//! set equals [`ALL`] exactly, mirroring `conformance_coverage.rs`'s
//! `every_outcome_kind_the_corpus_emits_is_registered_and_exercised`.
//!
//! **Scope boundary (Slice 1):** `bmc.rs`'s own `make_violation` call sites
//! for `"invariant"`/`"trans"`/`"ensures"` (the plain, non-induction BMC
//! engine's property violations) are deliberately *not* routed through this
//! module. Issue #646 and `docs/DESIGN-assurance-matrix.md` scope this
//! registry to the induction/liveness kinds plus the two hardcoded ones
//! (`leadsTo`, `deadlock`); the plain-BMC values mirror
//! `fsl_runtime::Monitor`'s own registered `outcome.kind` spelling
//! (`OUTCOME_FEATURE_KEYS` already has `invariant`/`trans`/`ensures`) and are
//! that registry's concern, not this one's. `fslc`'s `main.rs:14741`
//! (`refine`'s progress-check rendering, also `"leadsTo"`) and
//! `verification.rs:537` (`"leadsTo_rank"`, a fixed envelope-shape tag with
//! no corresponding `RankFailure.kind` value) are likewise out of Slice 1's
//! file scope; see `docs/DESIGN-assurance-matrix.md`'s "Slice 1 boundary"
//! section for the reasoned basis these axis N/A cells cite.

/// A bounded BMC search found a state where the `leadsTo` trigger (`P` holds,
/// `Q` false) has been pending past the search depth. `BmcViolation.kind`,
/// `bmc.rs`.
pub const LEADS_TO: &str = "leadsTo";

/// A k-induction step found a state sequence that satisfies every invariant
/// at step `k - 1` but violates one at step `k`. `InductionCti.kind`,
/// `induction.rs`.
pub const INVARIANT: &str = "invariant";

/// A k-induction step found a state sequence that satisfies every invariant
/// but violates a `transition` property. `InductionCti.kind`, `induction.rs`.
pub const TRANS: &str = "trans";

/// A `leadsTo`'s `decreases` measure can be negative in some state where the
/// trigger is pending, so it cannot serve as a ranking function.
/// `RankFailure.kind`, `induction.rs`.
pub const UNBOUNDED_BELOW: &str = "unbounded_below";

/// A `leadsTo`'s `helpful` action instance is not declared `fair`, so
/// `helpful` alone cannot obligate it to eventually fire. `RankFailure.kind`,
/// `induction.rs`.
pub const PROGRESS_ACTION_NOT_FAIR: &str = "progress_action_not_fair";

/// With two or more matching `helpful` instances, one can become enabled
/// while pending and then disabled again without firing, so its `fair`
/// obligation is never triggered. `RankFailure.kind`, `induction.rs`.
pub const HELPFUL_ACTION_ENABLEDNESS_NOT_STICKY: &str = "helpful_action_enabledness_not_sticky";

/// A `leadsTo` obligation can be pending while no matching `helpful` action
/// instance is enabled, so none of them is ever obligated to fire.
/// `RankFailure.kind`, `induction.rs`.
pub const HELPFUL_ACTION_NOT_ENABLED: &str = "helpful_action_not_enabled";

/// No `helpful` clause is declared, and some enabled action can leave the
/// obligation pending without strictly decreasing the measure.
/// `RankFailure.kind`, `induction.rs`.
pub const NON_DECREASING_ACTION: &str = "non_decreasing_action";

/// A `helpful` clause is declared, and a non-`helpful` action can move the
/// state to where neither `P` nor `Q` holds -- outside the region the
/// ranking proof reasons about. `RankFailure.kind`, `induction.rs`.
pub const PENDING_NOT_PRESERVED: &str = "pending_not_preserved";

/// A `helpful` clause is declared, and the matching `helpful` action instance
/// itself can fire while pending without strictly decreasing the measure.
/// `RankFailure.kind`, `induction.rs`.
pub const NON_DECREASING_HELPFUL_ACTION: &str = "non_decreasing_helpful_action";

/// A `helpful` clause is declared, and a non-`helpful` action can increase
/// the measure while the obligation is pending, which could outpace the
/// `helpful` action's guaranteed decrease. `RankFailure.kind`, `induction.rs`.
pub const NON_HELPFUL_ACTION_INCREASES_MEASURE: &str = "non_helpful_action_increases_measure";

/// A bounded search found a state with zero enabled actions (outside any
/// declared `terminal` condition). Not a `Violation`-shaped struct field --
/// `fslc`'s `verification_output.rs` renders this directly from
/// `BmcResult::deadlock_trace`.
pub const DEADLOCK: &str = "deadlock";

/// Every value the constants above declare, in a stable order. Exhaustive by
/// construction only in the sense that this module is the single owner --
/// nothing enforces at compile time that every `pub const` above also
/// appears here, so `rust/fslc/tests/assurance/violation_kind.rs` asserts the
/// two stay in sync (a corpus-observed kind missing from `ALL` fails loudly,
/// the same discipline `OUTCOME_FEATURE_KEYS` uses for `outcome.kind`).
pub const ALL: &[&str] = &[
    LEADS_TO,
    INVARIANT,
    TRANS,
    UNBOUNDED_BELOW,
    PROGRESS_ACTION_NOT_FAIR,
    HELPFUL_ACTION_ENABLEDNESS_NOT_STICKY,
    HELPFUL_ACTION_NOT_ENABLED,
    NON_DECREASING_ACTION,
    PENDING_NOT_PRESERVED,
    NON_DECREASING_HELPFUL_ACTION,
    NON_HELPFUL_ACTION_INCREASES_MEASURE,
    DEADLOCK,
];
