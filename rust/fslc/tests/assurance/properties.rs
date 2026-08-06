// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `properties` axis: the five `KernelModel` property groups (issue #537 C3
//! slice 1). Rows are the kernel schema's own `properties.required` list
//! (`invariants`, `transitions`, `reachables`, `leads_to`, `terminal`) --
//! `kernel_schema_property_groups_match_the_axis_rows` asserts the local
//! list equals the schema's, the same schema-anchored sync discipline
//! `conformance_coverage.rs` uses for `SEMANTICS_FEATURE_KEYS` (a new
//! property group must land in the public-kernel schema to exist as a
//! contract surface, and landing there without an axis row fails that
//! test).
//!
//! Declared columns: `BMC`, `explicit`, `induction`, `replay`.
//!
//! The `replay` column leans on a structural fact of the verification
//! pipeline, verified in source before any cell cited it: every native
//! verify run replays *every* symbolic witness -- violation trace, leadsTo
//! violation trace, each reachable witness trace, deadlock trace -- through
//! the solver-independent Monitor before rendering
//! (`verification_output.rs::replay_bmc_witnesses`, called at
//! `verification.rs`'s BMC seam with exit 3 on failure, and for the
//! explicit engine inside `verification_output.rs`'s explicit renderer). A
//! test that asserts exit 0/1 on a witnessed or violated verdict therefore
//! *is* replay evidence for that row: had the Monitor rejected the trace,
//! the exit would have been 3 and the cited assertion would have failed.
//!
//! Every citation below was confirmed by reading the cited test's function
//! body (or running the probe here), never transcribed from a filename.

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::Value;

use crate::claim::{Axis, Citation, Claim, workspace_root};

/// The five kernel property groups, spelled exactly as the kernel schema's
/// `properties.required` spells them. Kept in sync with
/// `schemas/fslc/kernel/kernel.v1.schema.json` by
/// [`kernel_schema_property_groups_match_the_axis_rows`].
pub(crate) const ROWS: &[&str] = &[
    "invariants",
    "transitions",
    "reachables",
    "leads_to",
    "terminal",
];

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn axis() -> Axis {
    let columns = vec!["BMC", "explicit", "induction", "replay"];
    let mut cells = std::collections::BTreeMap::new();

    cells.insert(
        ("invariants", "BMC"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fsl-verifier/tests/expression_agreement.rs",
                anchor: "fn bounded_verification_rejects_initial_violation_without_action_instances()",
            },
        },
    );
    cells.insert(
        ("invariants", "explicit"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fsl-runtime/tests/explicit_engine.rs",
                anchor: "fn explicit_engine_totalizes_property_context_zero_division()",
            },
        },
    );
    cells.insert(
        ("invariants", "induction"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/induction_suggestions.rs",
                anchor: "fn suggests_a_scalar_bound_without_changing_the_verdict()",
            },
        },
    );
    // The corpus sweep replays every explicit-engine violation trace through
    // `fsl_runtime::replay_trace` directly (`replayed > 0` is asserted), and
    // the corpus's violated top-level specs violate invariants (cart_buggy).
    cells.insert(
        ("invariants", "replay"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/explicit_engine.rs",
                anchor: "fn explicit_and_bmc_agree_on_every_accepted_top_level_corpus_spec()",
            },
        },
    );

    let trans_probe = Citation {
        path: "rust/fslc/tests/assurance/properties.rs",
        anchor: "fn bmc_and_explicit_report_and_replay_a_trans_property_violation()",
    };
    cells.insert(("transitions", "BMC"), Claim::Exercised { by: trans_probe });
    cells.insert(
        ("transitions", "explicit"),
        Claim::Exercised { by: trans_probe },
    );
    cells.insert(
        ("transitions", "induction"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/verification_ownership.rs",
                anchor: "fn selected_transition_induction_matches_the_all_properties_proof()",
            },
        },
    );
    // Exit 1 (not 3) from the probe proves the Monitor replayed the trans
    // violation trace: `replay_bmc_witnesses` gates rendering.
    cells.insert(
        ("transitions", "replay"),
        Claim::Exercised { by: trans_probe },
    );

    let reachable_witness = Citation {
        path: "rust/fslc/tests/explicit_engine.rs",
        anchor: "fn explicit_reachable_witness_step_matches_bmc()",
    };
    cells.insert(
        ("reachables", "BMC"),
        Claim::Exercised {
            by: reachable_witness,
        },
    );
    cells.insert(
        ("reachables", "explicit"),
        Claim::Exercised {
            by: reachable_witness,
        },
    );
    cells.insert(
        ("reachables", "induction"),
        Claim::UnsupportedFailClosed {
            by: Citation {
                path: "rust/fslc/tests/assurance/properties.rs",
                anchor: "fn induction_rejects_a_reachable_property_fail_closed()",
            },
        },
    );
    // Exit 0 on both engines proves every reachable witness trace replayed
    // (`for witness in result.reachables.values().flatten()` in
    // `replay_bmc_witnesses`).
    cells.insert(
        ("reachables", "replay"),
        Claim::Exercised {
            by: reachable_witness,
        },
    );

    cells.insert(
        ("leads_to", "BMC"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/issue_260_leadsto_stagnation.rs",
                anchor: "fn leadsto_deadlock_stagnation_is_detected_beyond_the_deadlock_step_for_plain_kernel_spec()",
            },
        },
    );
    cells.insert(
        ("leads_to", "explicit"),
        Claim::UnsupportedFailClosed {
            by: Citation {
                path: "rust/fslc/tests/explicit_engine.rs",
                anchor: "fn explicit_cli_exit_codes_cover_bounded_proved_violated_budget_and_semantics()",
            },
        },
    );
    cells.insert(
        ("leads_to", "induction"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/induction_suggestions.rs",
                anchor: "fn ranked_leadsto_failures_never_receive_suggestions()",
            },
        },
    );
    cells.insert(
        ("leads_to", "replay"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/replay_trace_contract.rs",
                anchor: "fn overdue_bounded_response_is_liveness_nonconformance_after_safety()",
            },
        },
    );

    cells.insert(
        ("terminal", "BMC"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/assurance/properties.rs",
                anchor: "fn terminal_excludes_the_final_state_from_bmc_deadlock_reporting()",
            },
        },
    );
    cells.insert(
        ("terminal", "explicit"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fsl-runtime/tests/explicit_engine.rs",
                anchor: "fn explicit_bfs_proves_at_state_space_closure()",
            },
        },
    );
    cells.insert(
        ("terminal", "induction"),
        Claim::NotApplicable {
            reason: "terminal's only kernel semantic is excluding matching states from deadlock reporting; the induction engine proves invariants/transitions over arbitrary states and has no deadlock probe (no bounded search), so terminal has nothing to act on there",
            basis: Citation {
                path: "rust/fslc/src/coverage.rs",
                anchor: "excluding matching states from deadlock reporting",
            },
        },
    );
    cells.insert(
        ("terminal", "replay"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fsl-runtime/tests/monitor_regression.rs",
                anchor: "fn action_cover_trace_uses_the_enabling_bool_value_and_may_end_at_terminal()",
            },
        },
    );

    Axis {
        name: "properties",
        rows: ROWS.to_vec(),
        columns,
        cells,
    }
}

fn run_verify(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc {}`: {error}; stderr={}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// The axis's row list must equal the kernel schema's `properties.required`
/// exactly -- the schema is where a new property group must land to become
/// part of the public Kernel contract, so this is the registration a new
/// group cannot skip (mirroring
/// `conformance_coverage.rs::semantics_schema_keys_are_all_registered_as_feature_rows`).
#[test]
fn kernel_schema_property_groups_match_the_axis_rows() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            workspace_root().join("schemas/fslc/kernel/kernel.v1.schema.json"),
        )
        .expect("read Kernel schema"),
    )
    .expect("Kernel schema JSON");
    let required = schema["properties"]["properties"]["required"]
        .as_array()
        .expect("properties.required")
        .iter()
        .map(|value| value.as_str().expect("string key").to_owned())
        .collect::<BTreeSet<_>>();
    let rows = ROWS
        .iter()
        .map(|&row| row.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        required, rows,
        "every kernel.v1.schema.json property group must be a properties-axis row \
         (update ROWS in rust/fslc/tests/assurance/properties.rs)"
    );
}

/// Targeted probe for the `transitions` row's `BMC`, `explicit`, and
/// `replay` cells: no pre-existing test asserted an end-to-end verify run
/// reporting `violation_kind: "trans"` on either engine. Exit 1 (violated),
/// not 3 (internal), additionally proves the Monitor replayed the trans
/// counterexample trace, because `replay_bmc_witnesses` runs before
/// rendering on both engine paths and fails closed to exit 3.
#[test]
fn bmc_and_explicit_report_and_replay_a_trans_property_violation() {
    let fixture = "rust/fslc/tests/fixtures/assurance_trans_violation.fsl";

    let (bmc, status) = run_verify(&[
        "verify",
        fixture,
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1, "{bmc}");
    assert_eq!(bmc["result"], "violated");
    assert_eq!(bmc["violation_kind"], "trans");
    assert_eq!(bmc["trans"], "NeverDecrease");

    let (explicit, status) = run_verify(&[
        "verify",
        fixture,
        "--engine",
        "explicit",
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 1, "{explicit}");
    assert_eq!(explicit["result"], "violated");
    assert_eq!(explicit["violation_kind"], "trans");
    assert_eq!(explicit["engine"], "explicit");
}

/// Targeted probe for the `reachables` row's `induction` cell: the
/// induction engine cannot prove reachability and must reject the property
/// selection closed (a usage error naming the reason), not silently skip or
/// vacuously prove it.
#[test]
fn induction_rejects_a_reachable_property_fail_closed() {
    let (output, status) = run_verify(&[
        "verify",
        "rust/fslc/tests/fixtures/explicit_reachable_witnessed.fsl",
        "--engine",
        "induction",
        "--property",
        "HitTwo",
        "--no-cache",
    ]);
    assert_eq!(status, 2, "{output}");
    assert_eq!(output["result"], "error");
    assert_eq!(output["kind"], "usage");
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("which the induction engine cannot prove")),
        "{output}"
    );
}

/// Targeted probe for the `terminal` row's `BMC` cell, with its negative
/// control: the identical machine minus the `terminal` declaration must
/// deadlock under `--deadlock error`, proving the green verdict on the
/// `terminal` fixture is the exclusion at work, not a deadlock probe that
/// never fires.
#[test]
fn terminal_excludes_the_final_state_from_bmc_deadlock_reporting() {
    let (with_terminal, status) = run_verify(&[
        "verify",
        "rust/fslc/tests/fixtures/assurance_terminal_once.fsl",
        "--depth",
        "4",
        "--deadlock",
        "error",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{with_terminal}");
    assert_eq!(with_terminal["result"], "verified");

    let (without_terminal, status) = run_verify(&[
        "verify",
        "rust/fslc/tests/fixtures/assurance_terminal_once_missing.fsl",
        "--depth",
        "4",
        "--deadlock",
        "error",
        "--no-cache",
    ]);
    assert_eq!(status, 1, "{without_terminal}");
    assert_eq!(without_terminal["result"], "violated");
    assert_eq!(without_terminal["violation_kind"], "deadlock");
}
