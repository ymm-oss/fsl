// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #663: `run_db_check` and `run_domain_check` each projected the
//! nested `verify` kernel envelope into their own `kernel` object through an
//! independent, hand-copied key list. `run_domain_check`'s copy
//! (`stable_kernel_projection`, 16 keys after #641) carried an
//! AGENTS.md-citing comment explaining why replayable evidence must not be
//! dropped; `run_db_check`'s copy (7 keys) carried neither the comment nor
//! any of that evidence. Fixes for #515 and #641 landed on the domain copy
//! only because it was the only place to land them, and neither copy ever
//! carried `unknown_cti`'s primary evidence (the induction counterexample)
//! or `reachable_failed`'s (the list of unreached properties).
//!
//! `fslc_rust::outcome::{classify_kernel_key, project_kernel}` is now the
//! single owner both commands call; `main.rs`'s `stable_kernel_projection`
//! and the inline array inside `run_db_check` are gone. An unrecognized key
//! passes through `project_kernel` rather than panicking or being silently
//! dropped (see that function's doc comment): the census test below is the
//! loud gate against a new, unregistered `verify` output channel, not a
//! runtime assertion over production input. This suite pins:
//!
//! - `db check` now carries the evidence `unknown_cti` needs, which it
//!   dropped entirely before.
//! - `db check` and `domain check` produce the same kernel key set for the
//!   same verdict class -- the shape a re-forked projection would break.
//! - Every top-level key a curated `fslc verify` corpus reaching six verdict
//!   classes (plus a `leadsTo` violation and a ranked-termination
//!   counterexample, neither reachable through `db`/`domain` lowering today
//!   but registered `Projected` for when a future dialect emits one) is
//!   classified by the registry.
//! - A passing verdict does not gain manufactured evidence keys.
//!
//! The pre-existing #600/#515/#641 regression fixtures are exercised by
//! their own suites (`issue_600_db_check_folds_kernel_verdict.rs`,
//! `issue_641_domain_check_kernel_warnings.rs`,
//! `issue_515_domain_check_false_green.rs`), run alongside this file rather
//! than duplicated into it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use fslc_rust::outcome::{classify_kernel_key, project_kernel};
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
            "invalid JSON for `fslc {args:?}`: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output
        .status
        .code()
        .unwrap_or_else(|| panic!("`fslc {args:?}` terminated by signal, no exit code"));
    (value, status)
}

fn key_set(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

const DB_INCONCLUSIVE: &str = "rust/fslc/tests/fixtures/issue_600_db_inconclusive_kernel.fsl";
const DOMAIN_BROKEN: &str = "rust/fslc/tests/fixtures/issue_515_domain_broken_invariant.fsl";
const DOMAIN_CLEAN: &str = "rust/fslc/tests/fixtures/issue_641_domain_clean.fsl";
const REACHABLE_FAILED: &str = "rust/fslc/tests/fixtures/issue_634_reachable_classification.fsl";
const EXPLICIT_BUDGET_FIXTURE: &str = "rust/fslc/tests/fixtures/explicit_finite_toggle.fsl";
const INDUCTION_PROVED: &str = "rust/fslc/tests/fixtures/issue_515_domain_clean_invariant.fsl";
const PLAIN_VERIFIED: &str = "specs/cart_v1.fsl";
/// A plain-kernel `leadsTo` violation (`violation_kind: "leadsTo"`).
/// Measured: emits `bindings`/`loop_start`/`pending_since`/`stutter`/
/// `trace_type`/`hint`/`trace`/`loc`. Neither `db check` nor `domain check`
/// can reach this shape today (`fsl-core`'s domain/db lowering never emits a
/// `LeadsTo` item), but a plain `fslc verify` corpus entry still measures the
/// keys `render_leadsto_failure` (`verification_output.rs`) actually emits,
/// rather than registering them from a source read alone.
const LEADS_TO_VIOLATED: &str = "examples/gallery/errors/violated_leads_to_starvation.fsl";
/// A ranked-termination counterexample (`result: "unknown_cti"`,
/// `violation_kind: "leadsTo_rank"`). Measured via the corpus fixture
/// `induction_suggestions.rs`'s `ranked_leadsto_failures_never_receive_suggestions`
/// already exercises: emits `bindings`/`measure`/`rank_failure`/
/// `measure_before`/`measure_after`/`last_action`/`cti`/`hint`/`message`.
/// `measure_value` and the plain `leadsTo`-only `deadline`/`within` are not
/// reached by either measured fixture and stay registered from
/// `verification.rs`'s `render_rank_failure` (lines ~544-601) and
/// `verification_output.rs`'s `render_leadsto_failure` directly.
const RANKED_LEADS_TO: &str = "tests/fixtures/rust_port/ranked_leadsto_non_decreasing.fsl";

/// Evidence keys no verdict class in this corpus has evidence for on a
/// passing kernel. Used by the over-projection negative control.
const EVIDENCE_KEYS: &[&str] = &[
    "cti",
    "hint",
    "trace_type",
    "requirement",
    "requirements",
    "unreached",
    "loc",
    "violated_at_step",
    "violating_bindings",
    "blame",
    "last_action",
    "trace",
    "violation_kind",
    "invariant",
    "bindings",
    "pending_since",
    "loop_start",
    "deadline",
    "within",
    "stutter",
    "measure",
    "rank_failure",
    "measure_value",
    "measure_before",
    "measure_after",
    "message",
];

// ---------------------------------------------------------------------------
// Positive: db check now carries unknown_cti's replayable evidence.
// ---------------------------------------------------------------------------

#[test]
fn db_check_carries_replayable_evidence_for_an_inconclusive_kernel() {
    let (output, status) = run(&["db", "check", DB_INCONCLUSIVE, "--engine", "induction"]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    let kernel = &output["kernel"];
    assert_eq!(kernel["result"], "unknown_cti", "{output:#}");

    // Before #663 this projected to exactly `{checked_to_depth,
    // completeness, invariant, result, spec}` -- five keys, none of them the
    // counterexample or the guidance that goes with it (the issue's
    // measured ground truth).
    assert!(
        kernel["cti"]["states"].is_array()
            && !kernel["cti"]["states"].as_array().unwrap().is_empty(),
        "missing the induction counterexample trace: {output:#}"
    );
    assert!(
        kernel["cti"]["violated_at"].is_u64(),
        "missing the counterexample's violation step: {output:#}"
    );
    assert!(
        kernel["hint"].as_str().is_some_and(|hint| !hint.is_empty()),
        "missing actionable guidance: {output:#}"
    );
    assert_eq!(
        kernel["trace_type"], "induction_cti",
        "missing the label that says how to read `cti`: {output:#}"
    );
}

// ---------------------------------------------------------------------------
// Symmetry / negative control for the defect: db and domain must agree.
// ---------------------------------------------------------------------------

/// A verified/passing kernel is dialect-independent -- neither
/// `run_db_check` nor `run_domain_check` adds anything of its own to the
/// nested envelope before projecting it -- so this is the one verdict class
/// this suite can compare directly across two different fixtures and two
/// different dialects without the comparison being confounded by which
/// evidence a particular spec happens to trigger. A re-forked projection
/// (someone reintroducing a second hand-copied list in one command but not
/// the other) is exactly what would make these two sets diverge; that is
/// the defect this repository has now paid for twice (#515, #641).
#[test]
fn db_check_and_domain_check_project_the_same_key_set_for_a_passing_kernel() {
    // No dbsystem finding and a clean bounded kernel: the dbsystem layer
    // itself never rewrites `kernel`, so this is a plain `verified` kernel.
    let (db, db_status) = run(&["db", "check", DB_INCONCLUSIVE]);
    assert_eq!(db_status, 0, "{db:#}");
    assert_eq!(db["kernel"]["result"], "verified", "{db:#}");

    let (domain, domain_status) = run(&["domain", "check", DOMAIN_CLEAN, "--depth", "4"]);
    assert_eq!(domain_status, 0, "{domain:#}");
    assert_eq!(domain["kernel"]["result"], "verified", "{domain:#}");

    assert_eq!(
        key_set(&db["kernel"]),
        key_set(&domain["kernel"]),
        "db check and domain check must carry the same verified-kernel key set; \
         db={db:#} domain={domain:#}"
    );
}

/// The mechanical half of the same control: each command's own `kernel`
/// object must equal what the single owner (`project_kernel`) produces from
/// that fixture's raw `verify` envelope -- compared as a set computed from
/// the owner at test time, never against a literal typed into this test.
/// This is what fails if `run_db_check` (or `run_domain_check`) grows a
/// second hand-copied key list instead of calling `project_kernel`.
#[test]
fn db_check_kernel_matches_the_owner_projection() {
    let (db, _status) = run(&["db", "check", DB_INCONCLUSIVE, "--engine", "induction"]);
    let (raw, _raw_status) = run(&[
        "verify",
        DB_INCONCLUSIVE,
        "--engine",
        "induction",
        "--deadlock",
        "warn",
        "--depth",
        "8",
        "--no-cache",
    ]);
    let owner_projected = project_kernel(raw);
    assert_eq!(
        key_set(&db["kernel"]),
        key_set(&owner_projected),
        "db check's kernel must match fslc_rust::outcome::project_kernel exactly; \
         db={db:#} owner={owner_projected:#}"
    );
}

#[test]
fn domain_check_kernel_matches_the_owner_projection() {
    let (domain, _status) = run(&["domain", "check", DOMAIN_BROKEN, "--depth", "4"]);
    let (raw, _raw_status) = run(&[
        "verify",
        DOMAIN_BROKEN,
        "--depth",
        "4",
        "--deadlock",
        "warn",
        "--no-cache",
    ]);
    let owner_projected = project_kernel(raw);
    assert_eq!(
        key_set(&domain["kernel"]),
        key_set(&owner_projected),
        "domain check's kernel must match fslc_rust::outcome::project_kernel exactly; \
         domain={domain:#} owner={owner_projected:#}"
    );
}

// ---------------------------------------------------------------------------
// Census / unregistered-channel gate.
// ---------------------------------------------------------------------------

/// Every top-level key a curated corpus of `fslc verify` runs emits, across
/// six verdict classes, must be classified by
/// `fslc_rust::outcome::classify_kernel_key`. An unclassified key means a
/// source change added a new `verify` output channel without registering
/// its projection fate -- the exact silent-drop failure mode #663 exists to
/// close. `project_kernel` itself no longer fails loudly on this (an
/// unregistered key now passes through instead of panicking, so a real
/// `db`/`domain check` run never loses a verdict it already computed
/// correctly); this census is where the loudness lives instead, at CI time
/// over a known corpus rather than at runtime over production input.
#[test]
#[allow(clippy::too_many_lines)]
fn every_key_a_curated_verify_corpus_emits_is_classified() {
    let corpus: &[&[&str]] = &[
        &["verify", PLAIN_VERIFIED, "--depth", "8", "--no-cache"],
        &["verify", DOMAIN_BROKEN, "--depth", "4", "--no-cache"],
        &[
            "verify",
            DB_INCONCLUSIVE,
            "--engine",
            "induction",
            "--deadlock",
            "ignore",
            "--no-cache",
        ],
        &[
            "verify",
            REACHABLE_FAILED,
            "--depth",
            "0",
            "--deadlock",
            "ignore",
            "--no-cache",
        ],
        &[
            "verify",
            EXPLICIT_BUDGET_FIXTURE,
            "--engine",
            "explicit",
            "--depth",
            "4",
            "--explicit-budget",
            "1",
            "--no-cache",
        ],
        &[
            "verify",
            INDUCTION_PROVED,
            "--engine",
            "induction",
            "--no-cache",
        ],
        &["verify", LEADS_TO_VIOLATED, "--depth", "8", "--no-cache"],
        &[
            "verify",
            RANKED_LEADS_TO,
            "--engine",
            "induction",
            "--depth",
            "8",
            "--k",
            "1",
            "--deadlock",
            "ignore",
            "--no-cache",
        ],
    ];

    let mut reached_classes = BTreeSet::new();
    let mut reached_violation_kinds = BTreeSet::new();
    let mut all_keys = BTreeSet::new();
    for args in corpus {
        let (output, status) = run(args);
        assert!(
            status == 0 || status == 1,
            "corpus entry must be a definitive verify verdict: {args:?} -> {output:#}"
        );
        let result = output["result"]
            .as_str()
            .unwrap_or_else(|| panic!("no `result` string: {args:?} -> {output:#}"))
            .to_owned();
        reached_classes.insert(result);
        if let Some(violation_kind) = output.get("violation_kind").and_then(Value::as_str) {
            reached_violation_kinds.insert(violation_kind.to_owned());
        }
        all_keys.extend(key_set(&output));
    }

    for class in [
        "verified",
        "violated",
        "unknown_cti",
        "reachable_failed",
        "unknown_budget",
        "proved",
    ] {
        assert!(
            reached_classes.contains(class),
            "corpus fixture drifted: no longer reaches {class}; reached {reached_classes:?}"
        );
    }
    // `violated`/`unknown_cti` are reached by more than one corpus entry
    // above (an invariant violation, a `leadsTo` violation, an induction
    // CTI, a ranked-termination CTI); pin the specific `violation_kind`
    // shapes too, or a fixture drifting away from `leadsTo`/`leadsTo_rank`
    // would hide behind the `violated`/`unknown_cti` class check already
    // passing and silently stop measuring the 12 keys in item 2 of #663's
    // review.
    for kind in ["leadsTo", "leadsTo_rank"] {
        assert!(
            reached_violation_kinds.contains(kind),
            "corpus fixture drifted: no longer reaches violation_kind {kind}; reached \
             {reached_violation_kinds:?}"
        );
    }

    let unregistered: Vec<&String> = all_keys
        .iter()
        .filter(|key| classify_kernel_key(key).is_none())
        .collect();
    assert!(
        unregistered.is_empty(),
        "verify emitted key(s) with no registered projection fate: {unregistered:?}; classify \
         them in fslc_rust::outcome::classify_kernel_key as Projected or Dropped, with a stated \
         reason (issue #663)"
    );
}

// ---------------------------------------------------------------------------
// Negative control against over-projection.
// ---------------------------------------------------------------------------

/// A clean/passing spec must not gain evidence keys it has no evidence for.
/// `project_kernel` filters by presence in the raw kernel; a key the raw
/// kernel never set must stay absent from the projection, never appear as a
/// manufactured `null` or empty placeholder.
#[test]
fn a_passing_kernel_does_not_gain_manufactured_evidence_keys() {
    let (db, db_status) = run(&["db", "check", DB_INCONCLUSIVE]);
    assert_eq!(db_status, 0, "{db:#}");
    assert_eq!(db["kernel"]["result"], "verified", "{db:#}");
    let kernel = db["kernel"].as_object().expect("db check kernel object");
    for key in EVIDENCE_KEYS {
        assert!(
            !kernel.contains_key(*key),
            "verified db-check kernel manufactured evidence key {key:?} it has no evidence for: \
             {db:#}"
        );
    }

    let (domain, domain_status) = run(&["domain", "check", DOMAIN_CLEAN, "--depth", "4"]);
    assert_eq!(domain_status, 0, "{domain:#}");
    assert_eq!(domain["kernel"]["result"], "verified", "{domain:#}");
    let kernel = domain["kernel"]
        .as_object()
        .expect("domain check kernel object");
    for key in EVIDENCE_KEYS {
        assert!(
            !kernel.contains_key(*key),
            "verified domain-check kernel manufactured evidence key {key:?} it has no evidence \
             for: {domain:#}"
        );
    }
}
