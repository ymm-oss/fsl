// SPDX-License-Identifier: Apache-2.0

//! One definition of the success and failure classes over the CLI's `result`
//! vocabulary (issue #537 C2, `docs/DESIGN-rust-component-internals.md`
//! "Outcome classification").
//!
//! The Verdict Conservation Law is that a failure-class `result` must not exit
//! zero and a success-class `result` must not exit non-zero. Before this
//! module the crate had no such definition: each command arm compared `result`
//! against literals at its own return point, so the class was re-derived — and
//! re-forgotten — once per family. #596 recorded the consequence from inside
//! the test suite, which had to carry its own `CHECK_SUCCESS_RESULTS` list
//! because there was no production enumeration to defer to; a conservation
//! check whose class definition lives in the test is the stale-check shape
//! #577 retired 28 instances of.
//!
//! Three properties are load-bearing and must survive future edits:
//!
//! - **It takes the envelope, not the result string.** Five values cannot be
//!   classified from `result` alone — they carry their verdict in a sibling
//!   field (`approval check`'s `status`, `fmt --check`'s `changed`, `lint`'s
//!   `finding_count`, `diff`'s `violations` and `gate.passed`). A `&str`
//!   signature would force a second classifier beside this one, which is the
//!   defect being removed. `docs/LANGUAGE.md`'s exit-code table says the same
//!   thing from the outside: `approval_check`/`approval_diff` are absent from
//!   it "because their exit code is not a function of `result`".
//! - **It is flat, not per-family.** The class does not collide even where
//!   meaning does: `"generated"` names different artifacts in `ledger`,
//!   `testgen`, and `document`, and is success-class in all three. Six or
//!   seven family classifiers would be six or seven places to forget a new
//!   value.
//! - **Unknown values classify as [`OutcomeClass::Failure`].** A result value
//!   nobody registered fails loudly at its first corpus run instead of exiting
//!   zero. Falling through to zero is how #554 arose. **Never add a
//!   `_ => OutcomeClass::Success` arm.**
//!
//! Cacheability is a separate predicate ([`verify_cache_admits`]) and does not
//! collapse into the classifier — see its own comment.
//!
//! This module also owns [`is_definitive_kernel_verdict`] and, since issue
//! #663, [`classify_kernel_key`]/[`project_kernel`]: the sibling question of
//! which nested-kernel keys a dialect check's projected `kernel` object may
//! keep. It lives here rather than a new module because it needs the exact
//! same registry discipline this file already states above (enumerate every
//! member, never fall through to a silent default) and the same two-command
//! motivation as `is_definitive_kernel_verdict`'s doc comment: `run_db_check`
//! and `run_domain_check` must not each carry their own answer.

use serde_json::{Map, Value};

/// The two classes the Verdict Conservation Law is stated over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum OutcomeClass {
    /// Exit zero is permitted.
    Success,
    /// Exit zero is forbidden.
    Failure,
}

impl OutcomeClass {
    /// Whether this class permits a zero exit status.
    #[must_use]
    pub fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }
}

/// Classify a CLI output envelope.
///
/// The class is the one the producing site's exit code already implies, so
/// migrating a call site to this function preserves behavior; the places where
/// it does not are the fixes tracked by #594 (`sweep`'s failure list omitted
/// `unknown_budget`), #600 (`db check` folded only `violated`), and #601
/// (`wrap_specialized`'s `_ => 0` fallthrough).
///
/// The failure arm deliberately enumerates its values even though the `_` arm
/// would classify them identically: the enumeration *is* the registry, and a
/// value that vanishes from it (or is misspelled into it) must be visible here
/// rather than absorbed by the catch-all. Merging the two, as
/// `clippy::match_same_arms` suggests, would delete the registry and leave only
/// the safety net.
// No fn-level `#[must_use]`: `OutcomeClass` carries it on the type, which is
// strictly stronger (issue #858), and both together is `double_must_use`.
#[allow(clippy::match_same_arms)]
pub fn outcome_class(output: &Value) -> OutcomeClass {
    let Some(result) = output.get("result").and_then(Value::as_str) else {
        // No `result` field at all is not a passing verdict. `chain` layer
        // entries and the raw `fmt` source path are the only outputs without
        // one, and neither is classified here.
        return OutcomeClass::Failure;
    };
    match result {
        // ---- Values whose verdict lives in a sibling field ---------------
        //
        // These are why this function takes `&Value`. Each mirrors the exit
        // code its producing site computes today.

        // `main.rs` `run_approval_check`: `i32::from(status ==
        // "signature-invalid")`. `result` is `"approval_check"` for
        // `approved`, `drifted`, and `signature-invalid` alike; only the
        // last exits non-zero. Approval *drift* deliberately exits 0 (#598).
        "approval_check" => {
            class_of(output.get("status").and_then(Value::as_str) != Some("signature-invalid"))
        }
        // `main.rs` `run_fmt_check`: `i32::from(any_changed)`.
        "format_check" => class_of(output.get("changed") != Some(&Value::Bool(true))),
        // `main.rs` `run_lint`: `i32::from(finding_count > 0)`.
        "lint" => class_of(
            output
                .get("finding_count")
                .and_then(Value::as_u64)
                .is_none_or(|count| count == 0),
        ),
        // `main.rs` `run_diff`: `i32::from(!violations.is_empty())`.
        "semantic_diff" => class_of(
            output
                .get("violations")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty),
        ),
        // `main.rs` `run_diff_git` batch: the same gate, aggregated. The
        // envelope publishes the decision as `gate.passed`.
        "semantic_diff_batch" => class_of(
            output
                .get("gate")
                .and_then(|gate| gate.get("passed"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),

        // ---- Success class ----------------------------------------------
        //
        // Every value below exits 0 at its producing site.

        // Core verification and checking verdicts.
        "ok"
        | "verified"
        | "proved"
        | "refines"
        | "conformant"
        // Bounded sweep grid over a clean spec.
        | "sweep_passed"
        // Derived artifacts: `ledger`, `testgen`, `html`, `document`,
        // `domain generate`, `db import`, `approval record`.
        | "generated"
        | "created"
        | "imported"
        | "imported_with_warnings"
        // Analyses and projections that either succeed or error out.
        | "analyzed"
        | "expanded"
        | "explained"
        | "kernel"
        | "typestate"
        | "scenarios"
        | "mutated"
        | "migrated"
        | "compared"
        | "compat_profile_generated"
        // Conformance projections and replay evidence.
        | "conformance"
        | "conformance_coverage"
        | "testgen_trace"
        | "conformance_checked"
        | "document_conformant"
        | "observed_conformant"
        | "replay_conformant"
        // `ai drift`'s success result, deliberately distinct from `db
        // observe`'s `observed_conformant` (#509, `main.rs` `run_ai_drift`).
        | "observed_supported"
        // Specialized dialects: a clean verdict under the declared
        // assumptions (`fsl-tools` `ai.rs`, `domain.rs`, `db.rs`, `agent.rs`).
        | "verified_under_assumptions"
        | "agent_analyzed"
        | "ai_project_analyzed"
        // Causal dialect (`fsl-tools/src/causal_analysis.rs`, `causal.rs`).
        | "causal_analyzed"
        | "causal_model_checked"
        | "causal_diffed"
        | "causal_ledger"
        | "causal_expectations_checked"
        | "causal_expectations_observed"
        // `fsl-ai` statistical evaluation that met its requirement.
        | "statistically_supported"
        // A `chain` layer the manifest did not request. Its own entry
        // declares `exit_code: 0` (`main.rs` `skipped_layer_entry`).
        | "skipped" => OutcomeClass::Success,

        // ---- Failure class ----------------------------------------------
        //
        // Listed explicitly rather than left to the `_` arm so that a typo in
        // one of these names surfaces as an unregistered value rather than
        // silently landing in the same class by accident.

        // Spec/usage/internal errors. The *code* (2 vs 3) is chosen by the
        // caller; the class is the same.
        "error"
        // Kernel verification verdicts that are not a pass.
        | "violated"
        | "reachable_failed"
        | "unknown_cti"
        | "unknown_budget"
        // Refinement, conformance, and sweep failures.
        | "refinement_failed"
        | "nonconformant"
        | "impl_violated"
        | "sweep_failed"
        // Dialect-level failures.
        | "observed_mismatch"
        | "replay_nonconformant"
        | "document_drifted"
        | "migration_refused"
        // `approval diff` only ever publishes this `result` on its
        // signature-invalid path (`main.rs` `run_approval_diff`); a valid
        // signature returns the underlying `semantic_diff` envelope instead.
        | "approval_diff"
        // `fsl-ai` statistical gate statuses (#510): a result that gates
        // before producing a Wilson interval is not a passing evaluation.
        | "statistically_unsupported"
        | "dataset_invalid"
        | "evaluator_untrusted"
        | "slice_missing"
        | "insufficient_samples"
        | "inconclusive"
        // A refinement mapping the auto-mapper could not decide.
        | "unknown" => OutcomeClass::Failure,

        // An unregistered value is an internal inconsistency, never a silent
        // success. Falling through to zero is exactly how #554 arose, and a
        // poisoned verify-cache entry must not be readable as a pass either.
        _ => OutcomeClass::Failure,
    }
}

fn class_of(success: bool) -> OutcomeClass {
    if success {
        OutcomeClass::Success
    } else {
        OutcomeClass::Failure
    }
}

/// Whether a `verify` envelope is a settled verdict worth writing to the
/// on-disk verification cache.
///
/// **This is not [`outcome_class`] and must not be folded into it.**
/// Cacheability asks "is this verdict settled enough to replay", not "did it
/// pass": `violated`, `reachable_failed`, `unknown_cti`, and `unknown_budget`
/// are all failure-class *and* cacheable. The two predicates live in the same
/// file so the vocabulary has one home, and stay distinct so that a new result
/// value forces an explicit decision in both.
#[must_use]
pub fn verify_cache_admits(output: &Value) -> bool {
    matches!(
        output.get("result").and_then(Value::as_str),
        Some(
            "verified"
                | "proved"
                | "violated"
                | "reachable_failed"
                | "unknown_cti"
                | "unknown_budget"
        )
    )
}

/// Whether a kernel `verify` reached a verdict a dialect check may fold into
/// its own.
///
/// Status 0 is `verified`/`proved` and status 1 is
/// `violated`/`reachable_failed`/`unknown_cti`/`unknown_budget`: both are
/// answers about the spec, so a dialect layer may combine them with its own
/// findings. Anything else — 2 for a spec error, 3 for an internal
/// inconsistency — is an answer about the *run*, and the kernel envelope must
/// be returned verbatim rather than absorbed into a `dbsystem` or `domain`
/// verdict that would then contradict its own exit code.
///
/// This lives here because `run_db_check` and `run_domain_check` both need it
/// and #600 exists because they did not share it: `run_domain_check` had folded
/// correctly since #515, `run_db_check` tested only `== 2`, and an internal
/// error read as a `dbsystem` verdict. The rule is one sentence, so the risk
/// was never that it is hard — it is that a third dialect check will be written
/// and its author will not know to look at the other two (#612).
#[must_use]
pub fn is_definitive_kernel_verdict(status: i32) -> bool {
    status == 0 || status == 1
}

/// Fate of one top-level key of a nested `verify` kernel envelope when
/// `run_db_check`/`run_domain_check` project it into their own `kernel`
/// object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelKeyFate {
    /// Survives into the projected `kernel` object.
    Projected,
    /// Deliberately absent from the projected `kernel` object.
    /// [`classify_kernel_key`]'s match arm for the key states the reason.
    Dropped,
}

/// The order [`project_kernel`] emits its surviving keys in. Declaration
/// order is emission order, so this doubles as the field order of the
/// `kernel` object both `run_db_check` and `run_domain_check` produce.
///
/// Grouped verdict identity, then AGENTS.md replayable evidence (issue
/// #515/#641's domain-only fix, now shared), then vacuity/coverage quality
/// signals (#641), then issue #663's additions. `unknown_cti`'s primary
/// evidence is the induction counterexample (`cti`) plus the guidance that
/// goes with every failure-class result (`hint`); `reachable_failed`'s is the
/// list of properties that could not be reached (`unreached`) -- before
/// #663 that verdict class projected to little more than
/// `result`/`spec`/`checked_to_depth`. `trace_type` says how to read `trace`/
/// `cti`; `requirement`/`requirements` are the traceability identifiers a
/// violated property carries when the spec tags one.
///
/// The last group is a `leadsTo` violation's evidence
/// (`bindings`/`pending_since`/`loop_start`/`deadline`/`within`/`stutter`,
/// `verification_output.rs`'s `render_leadsto_failure`) and a ranked-
/// termination counterexample's (`measure`/`rank_failure`/`measure_value`/
/// `measure_before`/`measure_after`/`message`, `verification.rs`'s
/// `render_rank_failure`, `violation_kind: "leadsTo_rank"`). Neither
/// `fsl-core`'s domain lowering (`domain_lowering.rs`, which emits only
/// `SpecItem::{Action, Enum, Type, Struct, State, Init, Terminal, Invariant,
/// Trans, Reachable}`) nor its `db.rs` lowering generates a `LeadsTo` item,
/// so a `db check`/`domain check` kernel cannot reach either shape today --
/// this is measured, not assumed (`issue_663_kernel_projection_owner.rs`'s
/// `every_key_a_curated_verify_corpus_emits_is_classified` corpus includes a
/// plain-kernel `leadsTo` violation and cites the ranked-termination fixture
/// `induction_suggestions.rs` already exercises). They are projected anyway:
/// a future dialect lowering that does emit a `LeadsTo` item must inherit
/// this evidence, not silently lose it the way `db check` lost the rest of
/// this list before #663.
const PROJECTED_KERNEL_KEYS: &[&str] = &[
    "result",
    "spec",
    "depth",
    "checked_to_depth",
    "completeness",
    "invariant",
    "violation_kind",
    "loc",
    "violated_at_step",
    "violating_bindings",
    "blame",
    "last_action",
    "trace",
    "warnings",
    "action_coverage",
    "cti",
    "hint",
    "trace_type",
    "requirement",
    "requirements",
    "unreached",
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

/// Classify one top-level key a `verify` kernel envelope can carry.
///
/// This is the single owner of "which nested-kernel keys survive a
/// projection" (issue #663). Before it, `run_domain_check` held a 16-key
/// allowlist (`stable_kernel_projection`, carrying an AGENTS.md-citing
/// comment for its replayable-evidence keys) and `run_db_check` held an
/// independent 7-key allowlist with no such comment and none of those keys —
/// the same defect shape #600/#612 describe for `is_definitive_kernel_verdict`,
/// applied one level down. Fixes #515 and #641 landed on the domain copy only
/// because there was nowhere else for them to land.
///
/// The `Dropped` arm is listed explicitly, one key per reason, rather than
/// left to a catch-all: the enumeration *is* the registry (same idiom as
/// [`outcome_class`]'s failure arm). A key this function has not seen — a
/// new `verify` output channel nobody registered — returns `None`, and
/// [`project_kernel`] passes it through unchanged rather than dropping or
/// panicking on it (see that function's doc comment for why); the loud gate
/// against an unregistered key living there unnoticed is
/// `issue_663_kernel_projection_owner.rs`'s
/// `every_key_a_curated_verify_corpus_emits_is_classified` census test, run
/// in CI rather than at runtime against production input.
#[must_use]
pub fn classify_kernel_key(key: &str) -> Option<KernelKeyFate> {
    if PROJECTED_KERNEL_KEYS.contains(&key) {
        return Some(KernelKeyFate::Projected);
    }
    #[allow(clippy::match_same_arms)]
    match key {
        // ---- Nested-run bookkeeping, not evidence about the verdict -----
        //
        // The outer `db`/`domain` command's own envelope already carries its
        // own `fsl`; a nested copy from the inner `verify` run would be
        // ambiguous about which run it describes.
        "fsl"
        // The top-level `fslc verify` CLI command's own on-disk
        // verification-cache lookup metadata (`{"hit":true,...}`), inserted
        // by `run_verify_cli`'s cache wrapper -- never by `run_verify`
        // itself. `run_db_check`/`run_domain_check` call `run_verify`
        // directly and so never produce it in a nested kernel, but a census
        // of the `fslc verify` command can still surface it and needs a
        // registered fate.
        | "cache"
        // Tool/solver version metadata for the *inner* `verify` run. The
        // outer command's own envelope is never stamped with this (
        // `with_version_metadata` wraps only the top-level `fslc verify`
        // command), so carrying it here would misattribute the outer
        // command's versions to a run it did not perform.
        | "versions"
        // Solver/timing statistics for the inner run; not evidence of what
        // was found, and the outer command has its own cost profile.
        | "cost"
        // Induction step-index bookkeeping, superseded by `cti.violated_at`
        // once `cti` is projected.
        | "k"
        // Internal generated-symbol provenance for a domain/db-lowered
        // Kernel construct. The *generating* command already knows how it
        // lowered the spec; `loc` (kept) already carries the user-authored
        // location this maps back to.
        | "origin"
        | "generated_name"
        // Echoes the `--engine` flag the caller already supplied.
        | "engine"
        // Induction proof bookkeeping for a *passing* verdict (no violation
        // to interpret): per-invariant step counts and the induction base
        // case, both already summarized by `checked_to_depth`/`completeness`.
        | "k_used"
        | "base_depth"
        // Explicit-engine exploration statistics for a passing or
        // budget-exhausted run; `hint` already carries the actionable
        // guidance for `unknown_budget`.
        | "closure"
        | "states_explored"
        | "max_frontier_width"
        | "depth_reached"
        // Coverage bookkeeping (which named properties this run checked),
        // not evidence of a specific finding. Distinct from `action_coverage`
        // (kept, #641), which qualifies the verdict's own confidence rather
        // than just listing names.
        | "invariants_checked"
        | "transitions_checked"
        // Witnessed-reachable detail and the reachability probe's own
        // deadlock flag: both describe a *passing* verdict, so there is no
        // violation for this evidence to help replay. An actual deadlock is
        // a `violated` result and travels through `trace`/`trace_type`
        // (kept) instead.
        | "reachables"
        | "deadlock"
        // Per-property leadsTo proof/coverage summary on a passing verdict.
        | "leads_to"
        // Free-text summary paired with a passing/closure verdict.
        | "note"
        // Redundant with `violation_kind`/`invariant`, which already say the
        // violated property is a transition postcondition.
        | "trans"
        // Authoring assistance (candidate auxiliary invariant text to add to
        // the spec), not evidence needed to replay the counterexample `cti`
        // already carries.
        | "suggested_invariants"
        // A bundled, independent `implements`/refinement verdict riding on
        // the same envelope (declared via a spec's `implements` contract).
        // Folding it in here would silently promote a second verdict axis
        // into a `dbsystem`/`domain` kernel projection without its own
        // AGENTS.md discipline; out of scope for #663, which is about the
        // `verify` verdict's own replayable evidence.
        | "implements"
        // Advisory "what to do next" guidance that duplicates `hint`'s role
        // on the `reachable_failed`/`partial_op` paths.
        | "faithfulness_class"
        | "recommended_action" => Some(KernelKeyFate::Dropped),
        _ => None,
    }
}

/// Project a nested `verify` kernel envelope for `run_db_check`/
/// `run_domain_check`'s `kernel` object, applying the single
/// [`classify_kernel_key`] registry both commands share (issue #663).
///
/// Non-object input (never produced by `run_verify` in practice) passes
/// through unchanged, matching the pre-#663 `stable_kernel_projection`.
///
/// A key [`classify_kernel_key`] does not recognize passes through
/// unchanged too, appended after the known [`PROJECTED_KERNEL_KEYS`] in the
/// kernel's own key order. Only a `Dropped` classification removes a key.
///
/// A panic here was considered and rejected. AGENTS.md makes the JSON
/// envelope itself an invariant ("Native CLI and Worker output must preserve
/// the JSON envelope, exit codes, locations, and replayable evidence
/// contract"): panicking on an unrecognized key would exit 101 with no
/// envelope at all, discarding a verdict `run_verify` had already computed
/// correctly. That is different from the native-Z3-backend panic at
/// `main.rs`'s "unexpectedly yielded Pending" site, where no correct output
/// exists to discard -- here one does. Passing the key through instead makes
/// an unregistered key harmless *by construction*: evidence is never lost,
/// and the only cost is an unclassified field surfacing in output, which
/// `issue_663_kernel_projection_owner.rs`'s
/// `every_key_a_curated_verify_corpus_emits_is_classified` census exists to
/// catch as a loud CI failure. Do not reinstate a panic/assert here; there is
/// nothing left for a runtime check to protect once the fallback is
/// pass-through rather than silent loss.
#[must_use]
pub fn project_kernel(kernel: Value) -> Value {
    let Value::Object(kernel) = kernel else {
        return kernel;
    };
    let mut projected: Map<String, Value> = PROJECTED_KERNEL_KEYS
        .iter()
        .filter_map(|&key| {
            kernel
                .get(key)
                .cloned()
                .map(|value| (key.to_owned(), value))
        })
        .collect();
    for (key, value) in &kernel {
        if classify_kernel_key(key).is_none() {
            projected.insert(key.clone(), value.clone());
        }
    }
    Value::Object(projected)
}

/// `docs/LANGUAGE.md`'s exit-code table applied to an envelope, as a total
/// function over [`outcome_class`].
///
/// Classification decides whether exit zero is *allowed*; it does not by
/// itself pick the code. `error_status` is the classified spec-error code the
/// caller already holds (2 for parse/type/semantics/io, 3 for internal), so an
/// error envelope is never re-classified here.
///
/// `mutate` re-emits its baseline `verify` envelope verbatim when the baseline
/// does not verify, so the baseline's `result` -- not merely the fact that it
/// is "not verified" -- decides the exit code. Deriving the status from
/// `result == "error"` alone let every other non-success value fall through to
/// 0, so `violated` exited 0 (issue #554): a mutation score is meaningless
/// over a spec that already fails, and a gate reading only the exit code saw a
/// pass. `scenarios` and `testgen` re-emit the same baseline envelope and
/// already exit 1.
#[must_use]
pub fn exit_status(output: &Value, error_status: i32) -> i32 {
    // Row 0 of the table is exactly the success class; that is the part this
    // function no longer restates.
    if outcome_class(output).is_success() {
        return 0;
    }
    match output.get("result").and_then(Value::as_str) {
        Some("error") => error_status,
        // Row 1, restricted to the members a baseline `verify` envelope can
        // actually carry. The row's remaining members (`nonconformant`,
        // `refinement_failed`, `sweep_failed`, `observed_mismatch`) belong to
        // other commands and cannot appear here.
        Some("violated" | "reachable_failed" | "unknown_cti" | "unknown_budget") => 1,
        // A failure-class value outside this command's vocabulary, or one
        // nobody registered at all, is an internal inconsistency -- never a
        // silent success.
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{OutcomeClass, is_definitive_kernel_verdict, outcome_class, verify_cache_admits};

    /// The rule `run_db_check` and `run_domain_check` share, pinned where it
    /// now lives. Both once carried it separately and one of them carried it
    /// wrong (#600, #612).
    ///
    /// Status 3 is the case that matters and the one no `.fsl` input can
    /// currently produce inside `run_db_check` — `check_db` calls `validate_db`
    /// first, so everything `run_verify` would reject with status 2 is rejected
    /// before the kernel runs, and the remaining status-3 exits are internal
    /// inconsistencies (#610). An end-to-end control is therefore impossible;
    /// this is where the rule can still be asserted directly, which is a
    /// by-product of the extraction rather than its reason.
    #[test]
    fn only_a_spec_verdict_may_be_folded_into_a_dialect_verdict() {
        assert!(is_definitive_kernel_verdict(0), "verified/proved");
        assert!(
            is_definitive_kernel_verdict(1),
            "violated/reachable_failed/unknown_*"
        );
        assert!(
            !is_definitive_kernel_verdict(2),
            "spec error is about the run"
        );
        assert!(
            !is_definitive_kernel_verdict(3),
            "internal error is about the run"
        );
    }

    /// Negative control for the `_` arm: a value nobody registered must not
    /// be readable as a pass. This is the property #554 violated.
    #[test]
    fn an_unregistered_result_is_failure_class() {
        assert_eq!(
            outcome_class(&json!({"result": "a_value_nobody_registered"})),
            OutcomeClass::Failure
        );
        assert_eq!(
            outcome_class(&json!({"result": "verified_typo"})),
            OutcomeClass::Failure
        );
        // An envelope with no `result` field at all is likewise not a pass.
        assert_eq!(outcome_class(&json!({})), OutcomeClass::Failure);
        assert_eq!(outcome_class(&json!({"result": 7})), OutcomeClass::Failure);
    }

    /// `verification.rs`'s cache-poisoning fixture value. A poisoned cache
    /// entry must never be read back as a success.
    #[test]
    fn the_cache_poison_fixture_is_failure_class() {
        assert_eq!(
            outcome_class(&json!({"result": "bogus"})),
            OutcomeClass::Failure
        );
    }

    /// The `&Value` signature exists for these: `result` alone cannot
    /// classify them.
    #[test]
    fn sibling_field_verdicts_follow_their_producing_site() {
        // `run_approval_check`: only `signature-invalid` exits non-zero.
        // Drift deliberately exits 0 (#598).
        for (status, expected) in [
            ("approved", OutcomeClass::Success),
            ("drifted", OutcomeClass::Success),
            ("signature-invalid", OutcomeClass::Failure),
        ] {
            assert_eq!(
                outcome_class(&json!({"result": "approval_check", "status": status})),
                expected,
                "approval_check status={status}"
            );
        }

        // `fmt --check`: `i32::from(any_changed)`.
        assert_eq!(
            outcome_class(&json!({"result": "format_check", "changed": false})),
            OutcomeClass::Success
        );
        assert_eq!(
            outcome_class(&json!({"result": "format_check", "changed": true})),
            OutcomeClass::Failure
        );

        // `lint`: `i32::from(finding_count > 0)`.
        assert_eq!(
            outcome_class(&json!({"result": "lint", "finding_count": 0})),
            OutcomeClass::Success
        );
        assert_eq!(
            outcome_class(&json!({"result": "lint", "finding_count": 3})),
            OutcomeClass::Failure
        );

        // `diff`: `i32::from(!violations.is_empty())`.
        assert_eq!(
            outcome_class(&json!({"result": "semantic_diff", "violations": []})),
            OutcomeClass::Success
        );
        assert_eq!(
            outcome_class(&json!({"result": "semantic_diff", "violations": ["R1"]})),
            OutcomeClass::Failure
        );

        // Batch `diff` publishes the same decision as `gate.passed`.
        assert_eq!(
            outcome_class(&json!({
                "result": "semantic_diff_batch",
                "gate": {"violations": [], "passed": true}
            })),
            OutcomeClass::Success
        );
        assert_eq!(
            outcome_class(&json!({
                "result": "semantic_diff_batch",
                "gate": {"violations": ["R1"], "passed": false}
            })),
            OutcomeClass::Failure
        );
        // A batch envelope missing the gate is not a pass.
        assert_eq!(
            outcome_class(&json!({"result": "semantic_diff_batch"})),
            OutcomeClass::Failure
        );
    }

    /// #594: `sweep`'s hand-written failure list omitted `unknown_budget`, so
    /// a grid whose only failing scope reported it collapsed into
    /// `sweep_passed`/exit 0. The classifier cannot have that omission
    /// because the value is registered here.
    ///
    /// This is asserted at the classifier rather than end-to-end because the
    /// CLI cannot currently reach it: `fslc sweep` restricts `--engine` to
    /// `bmc|induction` at its parse site in `main.rs`, `unknown_budget` is
    /// produced only by the explicit engine
    /// (`verification_output.rs` `render_explicit_budget`), the `auto` engine
    /// falls back to BMC before it escapes (`verification.rs`
    /// `auto_fallback_to_bmc`), and the verify-cache key includes the engine
    /// so a BMC sweep cannot read an explicit entry. The omission was latent,
    /// not a live false green.
    #[test]
    fn every_non_passing_kernel_verdict_is_failure_class() {
        for result in [
            "violated",
            "reachable_failed",
            "unknown_cti",
            "unknown_budget",
        ] {
            assert_eq!(
                outcome_class(&json!({"result": result})),
                OutcomeClass::Failure,
                "{result} must not be readable as a pass"
            );
        }
        for result in ["verified", "proved"] {
            assert_eq!(
                outcome_class(&json!({"result": result})),
                OutcomeClass::Success,
                "{result} must not be readable as a failure"
            );
        }
    }

    /// Cacheability is not success. If these two ever collapse into one
    /// predicate, this test fails.
    #[test]
    fn cacheability_is_not_the_success_class() {
        let violated = json!({"result": "violated"});
        assert!(
            verify_cache_admits(&violated),
            "a violation is a settled verdict and stays cacheable"
        );
        assert_eq!(
            outcome_class(&violated),
            OutcomeClass::Failure,
            "and is still failure-class"
        );

        // An error is neither cacheable nor a pass.
        let error = json!({"result": "error", "kind": "parse"});
        assert!(!verify_cache_admits(&error));
        assert_eq!(outcome_class(&error), OutcomeClass::Failure);

        // A success-class result outside `verify`'s vocabulary is not
        // cacheable either.
        let generated = json!({"result": "generated"});
        assert!(!verify_cache_admits(&generated));
        assert_eq!(outcome_class(&generated), OutcomeClass::Success);
    }

    /// The exit-code derivation is separate from the classification: the
    /// class says whether zero is allowed, `error_status` chooses which
    /// non-zero code an error envelope carries.
    #[test]
    fn exit_status_preserves_the_mutate_table() {
        use super::exit_status;

        for result in ["mutated", "verified", "proved"] {
            assert_eq!(exit_status(&json!({"result": result}), 2), 0);
        }
        for result in [
            "violated",
            "reachable_failed",
            "unknown_cti",
            "unknown_budget",
        ] {
            assert_eq!(exit_status(&json!({"result": result}), 2), 1);
        }
        assert_eq!(exit_status(&json!({"result": "error"}), 2), 2);
        assert_eq!(exit_status(&json!({"result": "error"}), 3), 3);
        // An unmapped result is an internal inconsistency, never a silent
        // success -- and keeps the table's distinct code for it.
        assert_eq!(exit_status(&json!({"result": "who_knows"}), 2), 3);
        // A registered failure outside a verify baseline's vocabulary is the
        // same kind of inconsistency when it reaches `mutate`.
        assert_eq!(exit_status(&json!({"result": "nonconformant"}), 2), 3);
    }
}
