// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Command-owned manifest binding every corpus refinement mapping to native
//! `fslc refine` (issue #593, issue #537 C4).
//!
//! `corpus_check_sweep.rs` excludes every `refinement`-dialect file from its
//! `check` sweep for a structural reason: a mapping file has no `state`
//! block, so `fslc check` always answers `semantics`/"spec has no state
//! block" for one whether or not the mapping itself is sound. **A green
//! `check` therefore says nothing at all about `refine`.** Until this
//! manifest, only 6 of the 28 corpus mappings were run through `fslc refine`
//! by any test, and the script that covered those 6
//! (`tools/check_rust_refinement_parity.py`) was wired into no CI workflow
//! and no `tools/check-native-integration.sh` lane. The remaining 22 had
//! never been executed by anything.
//!
//! Three properties, in the order they matter:
//!
//! 1. `every_corpus_refinement_mapping_is_registered` walks `specs/` and
//!    `examples/` and requires every `refinement`-dialect file to appear
//!    either in `CASES` or in `EXCLUSIONS`. The roster is derived from the
//!    corpus, never hard-coded, so adding a mapping fails this test until it
//!    is registered. A hard-coded list is precisely the shape #577 retired
//!    28 stale instances of.
//! 2. `native_refine_matches_the_declared_result_and_exit_for_every_registered_mapping`
//!    runs each live row and compares `result`, `kind`, **and** the process
//!    exit code (#537 C4 requires both; a JSON envelope that disagrees with
//!    the exit status is how #554 and #600 escaped).
//! 3. `every_exclusion_premise_still_holds` re-measures each exclusion's
//!    recorded premise and fails when it no longer holds — the self-retiring
//!    shape #568 introduced for the Worker parity corpus. An exclusion here
//!    cannot outlive its reason.
//!
//! **Every `expected_result`/`expected_kind` is transcribed from a
//! repository declaration, cited in `declared_by`, never from observed
//! output.** Recording what the binary happens to print would pin a defect
//! as the contract the moment one exists. `examples/layers/return_impl_refines.fsl`
//! was that case: it is a live row now, but it entered this manifest as an
//! exclusion whose recorded premise was the defect (#615), not an
//! `expected_result: "error"` that would have made the regression the
//! contract. `depth` is not
//! part of that contract: it is taken from the documented command line where
//! one exists and otherwise chosen to exercise the mapping, because depth
//! bounds the search rather than declaring the expected verdict.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

#[path = "support/mod.rs"]
mod support;
use support::{corpus_files, headers, repo_relative, root, top_level_keyword};

/// One live manifest row: a corpus refinement mapping, the implementation
/// and abstraction it is declared to be run against, and the verdict the
/// repository declares for that run.
struct Case {
    /// Implementation operand, as a repo-relative **file path**. Resolving
    /// it from the mapping's `impl <Name>` declaration instead is not
    /// reliable: `governance_semantic_mapping.fsl`'s `impl After` and
    /// `incident-log-mapping.fsl`'s `impl IncidentProductionLog` do not name
    /// files, and one of them names no `.fsl` declaration at all.
    implementation: &'static str,
    abstraction: &'static str,
    mapping: &'static str,
    depth: u32,
    /// Expected `result` field, transcribed from `declared_by`.
    expected_result: &'static str,
    /// Expected `kind` field, transcribed from `declared_by`. `None` when
    /// the repository declares the result but not the failure class.
    expected_kind: Option<&'static str>,
    /// `path:line` of the repository text that declares the expectation. The
    /// citation is the point of the manifest: a row whose expectation cannot
    /// be traced to a declaration is an observation, and pinning an
    /// observation makes a future defect look like the contract.
    declared_by: &'static str,
    /// Machine-checkable anchor for `declared_by`, checked by
    /// `manifest_rows_agree_with_the_expectations_their_mapping_files_declare`.
    ///
    /// `None` for the four rows whose declaration is a multi-line statement
    /// of the mapping's *purpose* rather than a verdict on one line (the
    /// `agentic_rag` and `multi_agent_system` positive pairs). Those keep the
    /// prose citation only, and are the manifest's residual trust.
    declaration: Option<Declaration>,
}

/// Where a row's `declared_by` citation points, in a form the harness can
/// re-check: the declaring file, plus text identifying the declaring line.
///
/// The check is "some line of `path` contains both `anchor` and this row's
/// `expected_result`" — deliberately not a line number. An unrelated edit
/// above the declaration must not fail this test, but deleting the
/// declaration, or changing the verdict it states, must. That keeps the
/// declaration single-owner: the citation is verified where the declaration
/// already lives instead of being copied next to the row.
struct Declaration {
    path: &'static str,
    anchor: &'static str,
}

const CASES: &[Case] = &[
    Case {
        implementation: "specs/cart_impl.fsl",
        abstraction: "specs/cart_v1.fsl",
        mapping: "specs/cart_refines.fsl",
        declaration: Some(Declaration {
            path: "docs/LANGUAGE.md",
            anchor: "Success: `result:",
        }),
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "docs/LANGUAGE.md:1378-1381 (command line, then \
                      `Success: result: \"refines\" (exit 0)`)",
    },
    Case {
        implementation: "specs/seat_booking_impl.fsl",
        abstraction: "specs/seat_booking.fsl",
        mapping: "specs/seat_refines.fsl",
        declaration: Some(Declaration {
            path: "specs/seat_booking.fsl",
            anchor: "seat_refines.fsl",
        }),
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "specs/seat_booking.fsl:2-4 (`The two-phase implementation \
                      (seat_booking_impl.fsl) refines this`, followed by the command \
                      line). The depth is the manifest's, not the corpus's: nothing \
                      documented one before this row, and `tests/test_refine_oracle.py` \
                      asserted `refines` at depth 4. Depth 6 subsumes that claim -- a \
                      counterexample within 4 steps is also within 6",
    },
    Case {
        implementation: "specs/bank_impl.fsl",
        abstraction: "specs/bank.fsl",
        mapping: "specs/bank_refines.fsl",
        declaration: Some(Declaration {
            path: "specs/bank.fsl",
            anchor: "bank_refines.fsl",
        }),
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "specs/bank.fsl:2-3 (`The detailed two-ledger implementation \
                      (bank_impl.fsl) refines this`, followed by the command line). \
                      Depth as for seat_refines above: undocumented before this row, and \
                      6 subsumes the depth-4 `refines` claim in \
                      tests/test_refine_oracle.py",
    },
    Case {
        implementation: "examples/gallery/errors/refinement_failed_impl.fsl",
        abstraction: "examples/gallery/errors/refinement_failed_abs.fsl",
        mapping: "examples/gallery/errors/refinement_failed_map.fsl",
        declaration: Some(Declaration {
            path: "examples/gallery/errors/refinement_failed_map.fsl",
            anchor: "expected-result",
        }),
        depth: 3,
        expected_result: "refinement_failed",
        expected_kind: Some("abs_requires_failed"),
        declared_by: "examples/gallery/errors/refinement_failed_map.fsl:1-3 \
                      (`expected-command`/`expected-result`/`expected-kind`, including \
                      `--depth 3`), corroborated by examples/gallery/README.md:43. This \
                      row's depth was 4 until the header was consulted; the file's own \
                      declaration wins",
    },
    Case {
        implementation: "examples/gallery/adversarial/refine_mapping_boundary_impl.fsl",
        abstraction: "examples/gallery/adversarial/refine_mapping_boundary_abs.fsl",
        mapping: "examples/gallery/adversarial/refine_mapping_boundary_map.fsl",
        declaration: Some(Declaration {
            path: "examples/gallery/adversarial/refine_mapping_boundary_map.fsl",
            anchor: "expected-result",
        }),
        depth: 2,
        expected_result: "refinement_failed",
        expected_kind: Some("abs_state_mismatch"),
        declared_by: "examples/gallery/adversarial/refine_mapping_boundary_map.fsl:1-3 \
                      (`expected-command`/`expected-result`/`expected-kind`, including \
                      `--depth 2`), corroborated by examples/gallery/README.md:56-60 \
                      (regression example of the full-unfolding deadlock -> vacuous \
                      `refines` bug)",
    },
    Case {
        // The mapping `governance_semantic_dependency.fsl` names in its
        // `checked_by refinement`. The implementation operand is the
        // adversarial fixture whose undefined type *is* the fixture's
        // purpose, so `refine` erroring here is the declared behaviour, not
        // a defect: an error result on a deliberately broken input is the
        // only outcome that would let the governance preservation report the
        // dependency as semantically invalid.
        implementation: "examples/gallery/adversarial/governance_semantic_after.fsl",
        abstraction: "examples/gallery/adversarial/governance_semantic_before.fsl",
        mapping: "examples/gallery/adversarial/governance_semantic_mapping.fsl",
        declaration: Some(Declaration {
            path: "examples/gallery/adversarial/governance_semantic_after.fsl",
            anchor: "expected-result",
        }),
        depth: 6,
        expected_result: "error",
        expected_kind: Some("type"),
        declared_by: "examples/gallery/adversarial/governance_semantic_after.fsl:2 \
                      (`// expected-result: error`) with the failure class named in \
                      corpus_check_sweep.rs GOVERNANCE_FIXTURE_EXCLUSIONS (`its own \
                      undefined-type failure is the fixture's purpose`)",
    },
    Case {
        implementation: "examples/refinement_liveness/design_drops_liveness.fsl",
        abstraction: "examples/refinement_liveness/policy.fsl",
        mapping: "examples/refinement_liveness/design_drops_liveness_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/refinement_liveness/README.md",
            anchor: "design_drops_liveness_refines.fsl",
        }),
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        // Reading `refines` here as a false green inverts the example. The
        // README's headline finding is that this mapping *passes*: safety
        // propagates across refinement and liveness does not, so a design
        // that drops `fair` still refines. The failing counterpart is the
        // `_progress_refines` row below, which opts in to progress
        // preservation.
        declared_by: "examples/refinement_liveness/README.md:33 (`# refines`) and :54 \
                      (`design_drops_liveness` returns `refines` (safety OK))",
    },
    Case {
        implementation: "examples/refinement_liveness/design_keeps_liveness.fsl",
        abstraction: "examples/refinement_liveness/policy.fsl",
        mapping: "examples/refinement_liveness/design_keeps_liveness_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/refinement_liveness/README.md",
            anchor: "design_keeps_liveness_refines.fsl",
        }),
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/refinement_liveness/README.md:38 (`# refines`)",
    },
    Case {
        implementation: "examples/refinement_liveness/design_drops_liveness.fsl",
        abstraction: "examples/refinement_liveness/policy.fsl",
        mapping: "examples/refinement_liveness/design_drops_liveness_progress_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/refinement_liveness/README.md",
            anchor: "design_drops_liveness_progress_refines.fsl",
        }),
        depth: 8,
        expected_result: "refinement_failed",
        expected_kind: Some("progress_lost"),
        declared_by: "examples/refinement_liveness/README.md:43 \
                      (`# refinement_failed / progress_lost`)",
    },
    Case {
        implementation: "examples/refinement_liveness/design_keeps_liveness.fsl",
        abstraction: "examples/refinement_liveness/policy.fsl",
        mapping: "examples/refinement_liveness/design_keeps_liveness_progress_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/refinement_liveness/README.md",
            anchor: "design_keeps_liveness_progress_refines.fsl",
        }),
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/refinement_liveness/README.md:45 (`# refines + progress`)",
    },
    Case {
        implementation: "examples/refinement_liveness/design_bypasses_control.fsl",
        abstraction: "examples/refinement_liveness/policy.fsl",
        mapping: "examples/refinement_liveness/design_bypasses_control_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/refinement_liveness/README.md",
            anchor: "design_bypasses_control_refines.fsl",
        }),
        depth: 8,
        expected_result: "refinement_failed",
        expected_kind: Some("abs_requires_failed"),
        declared_by: "examples/refinement_liveness/README.md:49 \
                      (`# refinement_failed / abs_requires_failed`)",
    },
    Case {
        implementation: "examples/ui_spike/return_ui.fsl",
        abstraction: "examples/ui_spike/return_req_min.fsl",
        mapping: "examples/ui_spike/ui_refines_req.fsl",
        declaration: Some(Declaration {
            path: "examples/ui_spike/README.md",
            anchor: "ui_refines_req.fsl",
        }),
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/ui_spike/README.md:12 (`**refines**`) and :17-18 \
                      (command line with `# refines`)",
    },
    Case {
        implementation: "examples/layers/return_impl.fsl",
        abstraction: "examples/layers/return_system.fsl",
        mapping: "examples/layers/return_impl_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/layers/README.md",
            anchor: "return_impl_refines.fsl",
        }),
        depth: 5,
        expected_result: "refines",
        expected_kind: None,
        // Was an exclusion until issue #615: the mapping used a bare-member
        // if-chain over names `DSt` and `SSt` both declare, and `da003eb`
        // rightly began rejecting that ambiguity. The exclusion went stale the
        // moment the mapping was migrated to `enum abstraction`, and this row
        // is what its failure message said to write.
        declared_by: "examples/layers/README.md:10 \
                      (`| `return_impl_refines.fsl` | design -> requirements mapping | refines |`), \
                      command at :15-16",
    },
    Case {
        implementation: "examples/consulting/tobe_expense.fsl",
        abstraction: "examples/consulting/asis_expense.fsl",
        mapping: "examples/consulting/tobe_refines_asis.fsl",
        declaration: Some(Declaration {
            path: "examples/consulting/README.md",
            anchor: "\"result\":",
        }),
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/consulting/README.md:45-49 \
                      (`# -> {\"result\": \"refines\", ...} = the controls are preserved`)",
    },
    Case {
        implementation: "examples/e2e/3_design.fsl",
        abstraction: "examples/e2e/2_requirements.fsl",
        mapping: "examples/e2e/3_refines_2.fsl",
        declaration: Some(Declaration {
            path: "examples/e2e/README.md",
            anchor: "confirm that the design layer",
        }),
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/e2e/README.md:46-47 (`confirm that the design layer \
                      refines the requirements layer`)",
    },
    Case {
        implementation: "examples/refinement_chain/bot.fsl",
        abstraction: "examples/refinement_chain/mid.fsl",
        mapping: "examples/refinement_chain/bot_refines_mid.fsl",
        declaration: Some(Declaration {
            path: "examples/refinement_chain/README.md",
            anchor: "bot_refines_mid.fsl",
        }),
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/refinement_chain/README.md:30 (`# refines`)",
    },
    Case {
        implementation: "examples/refinement_chain/mid.fsl",
        abstraction: "examples/refinement_chain/top.fsl",
        mapping: "examples/refinement_chain/mid_refines_top.fsl",
        declaration: Some(Declaration {
            path: "examples/refinement_chain/README.md",
            anchor: "mid_refines_top.fsl",
        }),
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/refinement_chain/README.md:31 (`# refines`)",
    },
    Case {
        implementation: "examples/nfr/sla_worker_design.fsl",
        abstraction: "examples/nfr/sla_worker.fsl",
        mapping: "examples/nfr/sla_worker_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/nfr/README.md",
            anchor: "sla_worker_refines.fsl",
        }),
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/nfr/README.md:30-31 (`# => refines`)",
    },
    Case {
        implementation: "examples/validation/order_refund_windowed.fsl",
        abstraction: "examples/validation/order_refund.fsl",
        mapping: "examples/validation/order_refund_windowed_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/validation/README.md",
            anchor: "order_refund_windowed_refines.fsl",
        }),
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/validation/README.md:12 (`**refines**` -- a time limit can \
                      be added without breaking the contract) and :40 (`# refines`)",
    },
    Case {
        implementation: "examples/validation/order_refund_instant.fsl",
        abstraction: "examples/validation/order_refund.fsl",
        mapping: "examples/validation/order_refund_instant_refines.fsl",
        declaration: Some(Declaration {
            path: "examples/validation/README.md",
            anchor: "order_refund_instant_refines.fsl",
        }),
        depth: 8,
        expected_result: "refinement_failed",
        expected_kind: Some("abs_requires_failed"),
        declared_by: "examples/validation/README.md:14 \
                      (`**refinement_failed / abs_requires_failed**`) and :43",
    },
    Case {
        implementation: "examples/agentic_rag/agentic_rag_requirements.fsl",
        abstraction: "examples/agentic_rag/agentic_rag_business.fsl",
        mapping: "examples/agentic_rag/agentic_rag_requirements_refines_business.fsl",
        declaration: None,
        depth: 7,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/agentic_rag/README.md:106-110 (the command's stated purpose \
                      is to confirm the requirements layer does not deviate from the \
                      business layer) and negative/README.md:3, which declares \
                      `negative/` -- not this directory -- the home of designs that must fail",
    },
    Case {
        implementation: "examples/agentic_rag/agentic_rag_design.fsl",
        abstraction: "examples/agentic_rag/agentic_rag_requirements.fsl",
        mapping: "examples/agentic_rag/agentic_rag_design_refines_requirements.fsl",
        declaration: None,
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/agentic_rag/README.md:112-116 (the command's stated purpose \
                      is to confirm the design layer does not deviate from the \
                      requirements layer) and negative/README.md:3",
    },
    Case {
        implementation: "examples/agentic_rag/negative/guard_bypass_design.fsl",
        abstraction: "examples/agentic_rag/agentic_rag_requirements.fsl",
        mapping: "examples/agentic_rag/negative/guard_bypass_refines_requirements.fsl",
        declaration: Some(Declaration {
            path: "examples/agentic_rag/negative/README.md",
            anchor: "guard_bypass_refines_requirements.fsl",
        }),
        depth: 6,
        expected_result: "refinement_failed",
        expected_kind: Some("abs_requires_failed"),
        declared_by: "examples/agentic_rag/negative/README.md:10 \
                      (Expected: `refinement_failed`, `abs_requires_failed`)",
    },
    Case {
        implementation: "examples/agentic_rag/negative/tool_approval_bypass_design.fsl",
        abstraction: "examples/agentic_rag/agentic_rag_requirements.fsl",
        mapping: "examples/agentic_rag/negative/tool_approval_bypass_refines_requirements.fsl",
        declaration: Some(Declaration {
            path: "examples/agentic_rag/negative/README.md",
            anchor: "tool_approval_bypass_refines_requirements.fsl",
        }),
        depth: 6,
        expected_result: "refinement_failed",
        expected_kind: Some("abs_requires_failed"),
        declared_by: "examples/agentic_rag/negative/README.md:11 \
                      (Expected: `refinement_failed`, `abs_requires_failed`)",
    },
    Case {
        implementation: "examples/agentic_rag/negative/liveness_drop_design.fsl",
        abstraction: "examples/agentic_rag/negative/liveness_requirements.fsl",
        mapping: "examples/agentic_rag/negative/liveness_drop_refines_requirements.fsl",
        declaration: Some(Declaration {
            path: "examples/agentic_rag/negative/README.md",
            anchor: "liveness_drop_refines_requirements.fsl",
        }),
        depth: 4,
        expected_result: "refinement_failed",
        expected_kind: Some("progress_lost"),
        declared_by: "examples/agentic_rag/negative/README.md:12 \
                      (Expected: `refinement_failed`, `progress_lost`, `leadsTo`)",
    },
    Case {
        implementation: "examples/multi_agent_system/multi_agent_requirements.fsl",
        abstraction: "examples/multi_agent_system/multi_agent_business.fsl",
        mapping: "examples/multi_agent_system/multi_agent_requirements_refines_business.fsl",
        declaration: None,
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/multi_agent_system/multi_agent_requirements_refines_business.fsl:1-7 \
                      (the mapping's declared role is to confirm the detail does not create \
                      a business shortcut) and README.md:79-82",
    },
    Case {
        implementation: "examples/multi_agent_system/multi_agent_design.fsl",
        abstraction: "examples/multi_agent_system/multi_agent_requirements.fsl",
        mapping: "examples/multi_agent_system/multi_agent_design_refines_requirements.fsl",
        declaration: None,
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
        declared_by: "examples/multi_agent_system/multi_agent_design_refines_requirements.fsl:1-7 \
                      (the mapping's declared role is to fold the design's queues into the \
                      requirements' externally visible state without dropping progress) and \
                      README.md:84-87",
    },
];

/// The measured fact that keeps an exclusion honest. Re-checked by
/// `every_exclusion_premise_still_holds`, so an exclusion cannot outlive its
/// reason: when the premise stops holding the test fails and names the row
/// that must replace it.
/// There was a second variant, `Blocked`, for an exclusion a defect stands in
/// the way of: it recorded the `result`/`kind` `refine` answered while broken,
/// so the exclusion went stale the day that changed. Issue #615 was its only
/// instance, and fixing #615 retired it exactly that way — the premise failed,
/// named the row to write, and the row went live. The variant is gone rather
/// than kept unused: no mapping is currently blocked by a defect, and a type
/// that says otherwise would be describing a state the corpus is not in. Bring
/// it back with its first user, whose shape this paragraph records.
enum Premise {
    /// The mapping is not a `fslc refine` input at all: its `impl` operand
    /// names an artifact that no corpus declaration defines, so there is no
    /// implementation spec to pair it with. The day such a declaration
    /// appears, the exclusion is stale.
    UndeclaredImplOperand { operand: &'static str },
}

struct Exclusion {
    mapping: &'static str,
    /// Why this mapping has no live row, and what has to change for it to
    /// get one.
    reason: &'static str,
    premise: Premise,
}

const EXCLUSIONS: &[Exclusion] = &[Exclusion {
    mapping: "examples/causal/evidence/incident-log-mapping.fsl",
    reason: "not a `fslc refine` input. Its `impl IncidentProductionLog` names a \
                 production observation log (examples/causal/evidence/\
                 incident-observation-log.jsonl), not a spec, so no implementation \
                 `.fsl` exists to pair with the abstraction. The command that consumes \
                 this file is `fslc causal observe-expectations --mapping` (issue #360), \
                 owned by rust/fslc/tests/causal_cli.rs (OBS_MAPPING, 6 call sites). \
                 This is a capability exclusion, not a tolerated difference: no verdict, \
                 location, or exit code is allowlisted for it anywhere",
    premise: Premise::UndeclaredImplOperand {
        operand: "IncidentProductionLog",
    },
}];

/// Every `refinement`-dialect file under `specs/` + `examples/`, repo-relative.
fn corpus_refinement_mappings(root: &Path) -> BTreeSet<String> {
    corpus_files(root)
        .into_iter()
        .filter(|path| {
            let source = std::fs::read_to_string(path).expect("read corpus source");
            top_level_keyword(&source) == Some("refinement")
        })
        .map(|path| repo_relative(root, &path))
        .collect()
}

/// Does any corpus `.fsl` declare `name` at top level? A top-level
/// declaration is unindented and has the declared name as its second token
/// (`spec Foo {`, `design Foo {`, ...).
fn corpus_declares(root: &Path, name: &str) -> bool {
    corpus_files(root).iter().any(|path| {
        let source = std::fs::read_to_string(path).expect("read corpus source");
        source.lines().any(|line| {
            if line.starts_with(char::is_whitespace) || line.trim_start().starts_with("//") {
                return false;
            }
            let mut tokens = line.split_whitespace();
            tokens.next().is_some() && tokens.next() == Some(name)
        })
    })
}

/// Where a mapping declares its own expectation in the gallery header
/// convention, that declaration is the authority and the manifest row must
/// agree with it -- including the `--depth` inside `expected-command`.
///
/// Without this check `declared_by` is a prose claim that can drift from the
/// file it cites, which is the same "the citation looked fine" failure the
/// manifest exists to prevent. It caught one immediately: the
/// `refinement_failed_map.fsl` row carried `depth: 4` while the fixture's own
/// header declares `--depth 3`.
#[test]
fn manifest_rows_agree_with_the_expectations_their_mapping_files_declare() {
    let root = root();
    let mut failures = Vec::new();

    for case in CASES {
        // The cited declaration must still be there, and must still state
        // this row's verdict. Checking the citation is what lets the
        // declaration stay in exactly one place: `governance_semantic_mapping`
        // is declared by the broken *implementation* it maps
        // (`governance_semantic_after.fsl`), which is the file that owns the
        // error, and copying an `expected-result` header onto the mapping
        // would create a second copy to keep in sync.
        if let Some(declaration) = &case.declaration {
            let declaring = std::fs::read_to_string(root.join(declaration.path))
                .expect("read declaration source");
            let states_verdict = declaring.lines().any(|line| {
                line.contains(declaration.anchor) && line.contains(case.expected_result)
            });
            if !states_verdict {
                failures.push(format!(
                    "{}: no line of {} carries both {:?} and {:?}. The cited declaration is \
                     gone or no longer states this verdict, so `declared_by` is stale: \
                     {}",
                    case.mapping,
                    declaration.path,
                    declaration.anchor,
                    case.expected_result,
                    case.declared_by
                ));
            }
        }

        let source = std::fs::read_to_string(root.join(case.mapping)).expect("read mapping");
        let declared = headers(&source);

        if let Some(result) = declared.get("expected-result")
            && result != case.expected_result
        {
            failures.push(format!(
                "{}: header declares expected-result={result:?} but the row says {:?}",
                case.mapping, case.expected_result
            ));
        }
        if let Some(kind) = declared.get("expected-kind")
            && Some(kind.as_str()) != case.expected_kind
        {
            failures.push(format!(
                "{}: header declares expected-kind={kind:?} but the row says {:?}",
                case.mapping, case.expected_kind
            ));
        }
        let Some(command) = declared.get("expected-command") else {
            continue;
        };
        let tokens: Vec<&str> = command.split_whitespace().collect();
        if tokens.first() != Some(&"refine") {
            failures.push(format!(
                "{}: header declares a non-`refine` expected-command {command:?}; this \
                 manifest only owns `refine` expectations",
                case.mapping
            ));
            continue;
        }
        if let Some(depth) = tokens
            .iter()
            .position(|token| *token == "--depth")
            .and_then(|index| tokens.get(index + 1))
            .and_then(|value| value.parse::<u32>().ok())
            && depth != case.depth
        {
            failures.push(format!(
                "{}: header declares `--depth {depth}` but the row runs at depth {}. The \
                 file's own declaration wins; change the row.",
                case.mapping, case.depth
            ));
        }
        // The command's operands are bare file names relative to the fixture
        // directory; compare them against the row's path basenames.
        let basename = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
        for (position, expected) in [(1, case.implementation), (2, case.abstraction)] {
            if let Some(declared_operand) = tokens.get(position)
                && basename(declared_operand) != basename(expected)
            {
                failures.push(format!(
                    "{}: header's expected-command operand {position} is \
                     {declared_operand:?} but the row uses {expected:?}",
                    case.mapping
                ));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn run_refine(implementation: &str, abstraction: &str, mapping: &str, depth: u32) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "refine",
            implementation,
            abstraction,
            mapping,
            "--depth",
            &depth.to_string(),
        ])
        .current_dir(root())
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc refine {implementation} {abstraction} {mapping}`: \
             {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output.status.code().unwrap_or_else(|| {
        panic!("`fslc refine {implementation} {abstraction} {mapping}` terminated by signal")
    });
    (value, status)
}

/// The exit code the CLI contract binds to a `refine` result
/// (`docs/LANGUAGE.md:1381` and `:1394`): `refines` exits 0, `refinement_failed`
/// exits 1, a static `error` exits 2, and `violated` — the impl-self-
/// violation verdict `refine` reaches before it consults the abstraction
/// (#466) — is failure-class, so it exits 1.
///
/// An unregistered result panics rather than falling through to 0. A `_ => 0`
/// arm here would give any future `refine` verdict a passing exit
/// expectation by default, which is the exact shape of #601's `_ => 0` and
/// #554's missing arm: the manifest would keep reporting green for a verdict
/// nobody classified. `fslc_rust::outcome::outcome_class` takes the same
/// position for the same reason.
fn expected_status(result: &str) -> i32 {
    match result {
        "refines" => 0,
        "refinement_failed" | "violated" => 1,
        "error" => 2,
        other => panic!(
            "unregistered `fslc refine` result {other:?}: add its exit code to \
             expected_status. Do not let a new verdict default to 0."
        ),
    }
}

/// Runs `job` over `0..count` on a small worker pool and collects the
/// failure strings. Two `agentic_rag` rows take ~100s and ~126s each in a
/// debug build and dominate the sweep; running the rows concurrently keeps
/// the manifest's CI cost near the single slowest row rather than the sum.
fn for_each_parallel(count: usize, job: impl Fn(usize) -> Option<String> + Sync) -> Vec<String> {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .min(count.max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    if let Some(failure) = job(index) {
                        failures.lock().expect("failure list").push(failure);
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().expect("failure list");
    failures.sort();
    failures
}

/// Every `refinement`-dialect file in the corpus must carry a live manifest
/// row or a reasoned exclusion. The roster is walked from `specs/` +
/// `examples/`, so a mapping added without registration fails here instead
/// of joining the 22 that were exercised by nothing until #593. The reverse
/// direction is checked too: a registered path that no longer exists is a
/// stale entry, not a silent pass.
#[test]
fn every_corpus_refinement_mapping_is_registered() {
    let root = root();
    let corpus = corpus_refinement_mappings(&root);
    assert!(
        corpus.len() >= 28,
        "corpus scan floor: found only {} refinement mappings under specs/+examples/, \
         expected at least the 28 that existed at #593 (the directory walk may be broken)",
        corpus.len()
    );

    let registered: BTreeSet<&str> = CASES
        .iter()
        .map(|case| case.mapping)
        .chain(EXCLUSIONS.iter().map(|exclusion| exclusion.mapping))
        .collect();
    assert_eq!(
        registered.len(),
        CASES.len() + EXCLUSIONS.len(),
        "a mapping is registered twice; each must have exactly one row or one exclusion"
    );

    let mut failures = Vec::new();
    for mapping in &corpus {
        if !registered.contains(mapping.as_str()) {
            failures.push(format!(
                "{mapping}: refinement mapping is in the corpus but in neither CASES nor \
                 EXCLUSIONS. Add a row citing the repository text that declares its \
                 expected `fslc refine` result (never the observed output), or add an \
                 exclusion with a reason and a re-checkable premise."
            ));
        }
    }
    for mapping in &registered {
        if !corpus.contains(*mapping) {
            failures.push(format!(
                "{mapping}: registered here but is not a corpus refinement mapping \
                 (deleted, moved, or no longer `refinement`-dialect). Remove the stale entry."
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Each live row must reproduce the `result`, the `kind`, **and** the exit
/// code its `declared_by` citation states (#537 C4: stdout JSON and process
/// status, both). A mapping or a semantics change that breaks the declared
/// correspondence fails here by name, the way #466, #483, #493, #494, and
/// #512 did not.
#[test]
fn native_refine_matches_the_declared_result_and_exit_for_every_registered_mapping() {
    let failures = for_each_parallel(CASES.len(), |index| {
        let case = &CASES[index];
        let (output, status) = run_refine(
            case.implementation,
            case.abstraction,
            case.mapping,
            case.depth,
        );
        let result = output["result"].as_str().unwrap_or_else(|| {
            panic!(
                "{}: `fslc refine` envelope has no string `result` field: {output}",
                case.mapping
            )
        });

        if result != case.expected_result {
            return Some(format!(
                "{}: declared result={:?} ({}), got result={result:?} ({output})",
                case.mapping, case.expected_result, case.declared_by
            ));
        }
        if let Some(expected_kind) = case.expected_kind {
            let kind = output["kind"].as_str();
            if kind != Some(expected_kind) {
                return Some(format!(
                    "{}: declared kind={expected_kind:?} ({}), got kind={kind:?} ({output})",
                    case.mapping, case.declared_by
                ));
            }
        }
        let expected_exit = expected_status(case.expected_result);
        if status != expected_exit {
            return Some(format!(
                "{}: result={result:?} binds exit={expected_exit}, got exit={status}",
                case.mapping
            ));
        }
        None
    });

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An exclusion may not outlive its reason. Each entry records the fact that
/// blocks a live row; this re-measures it and fails when it no longer holds,
/// so adding the missing declaration forces the excluded mapping into the
/// manifest instead of leaving a permanent hole (#568).
///
/// This has already fired once for real. Fixing #615 made that exclusion's
/// premise false, and the failure named the row to write — depth, expected
/// result, and citation — so the repair and the coverage landed together
/// rather than the fix quietly outrunning the manifest.
#[test]
fn every_exclusion_premise_still_holds() {
    let root = root();
    let failures = for_each_parallel(EXCLUSIONS.len(), |index| {
        let exclusion = &EXCLUSIONS[index];
        match &exclusion.premise {
            Premise::UndeclaredImplOperand { operand } => {
                if !corpus_declares(&root, operand) {
                    return None;
                }
                Some(format!(
                    "{}: the exclusion is STALE. The corpus now declares {operand:?}, so this \
                     mapping has an implementation operand and can take a live manifest row. \
                     Recorded reason: {}",
                    exclusion.mapping, exclusion.reason
                ))
            }
        }
    });

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
