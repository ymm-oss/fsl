// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `property_selection` axis: the CLI selector-mode dimension the
//! `properties` axis (issue #537 C3 slice 1) never declared (issue #705,
//! the reproduction-prevention half of #701).
//!
//! #701 was possible because `properties::axis()` records
//! `(row, column) = (property group, engine)` capability only for an
//! *unfiltered* run. `transitions x induction` was `Exercised` there while
//! `--engine induction --property <trans>` (a `--property`-selected run of
//! the exact same row/column) was independently rejected by
//! `validate_cli_property_selection` -- a second, uninventoried dimension
//! hid the contradiction. This axis adds that dimension explicitly: rows
//! are `"{property group}x{engine}"`, generated from `properties::ROWS`
//! (the Kernel-schema-synced property-group registry) crossed with
//! `properties::axis()`'s own declared engine columns (excluding
//! `"replay"`, which is not a `--engine` value and has no selection
//! semantics of its own) -- never a hand-copied list of either. A new
//! property group landing in the Kernel schema, or a new engine landing in
//! `properties::axis()`'s columns, therefore expands this axis's rows too,
//! and an unclaimed cell for it fails `every_declared_cell_across_every_axis_has_a_claim`
//! the same way an unclaimed `properties` cell would.
//!
//! Columns are the two selector lanes `--property Name` (`"selected"`) and
//! `--exclude-property Name` (`"excluded"`). The third execution shape
//! named in #705 -- an unfiltered all-properties run -- is `properties.rs`'s
//! own scope and is not repeated here.
//!
//! `terminal` has no name `select_properties` can resolve at all (its
//! available-name chain is state bounds + invariants + transitions +
//! leadsTo + reachables; a bare `terminal { .. }` block contributes none of
//! those), so every `terminal` cell is `NotApplicable`, on every engine, for
//! both lanes -- not silently absent, and not miscoded `UnsupportedFailClosed`
//! (there is no selection attempt to reject; the name never exists to try).

use std::process::Command;

use serde_json::Value;

use crate::claim::{Axis, Citation, Claim, workspace_root};
use crate::properties;

const TERMINAL_NOT_SELECTABLE: Citation = Citation {
    path: "rust/fslc/src/main.rs",
    anchor: "fn select_properties(",
};

fn leak(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

/// `--engine` values `property_selection` cares about: `properties.rs`'s
/// own declared columns minus `"replay"`, which is not an engine a
/// `--property`/`--exclude-property` selection dispatches to.
fn engines() -> Vec<&'static str> {
    properties::axis()
        .columns
        .into_iter()
        .filter(|&column| column != "replay")
        .collect()
}

fn row(kind: &str, engine: &str) -> &'static str {
    leak(format!("{kind}x{engine}"))
}

fn cell(kind: &str, engine: &str) -> Claim {
    match (kind, engine) {
        ("terminal", _) => Claim::NotApplicable {
            reason: "terminal declarations are anonymous: select_properties's available-name \
                     chain (state bounds, invariants, transitions, leadsTo, reachables) never \
                     includes them, so no engine's --property/--exclude-property has a name to \
                     select or exclude in the first place",
            basis: TERMINAL_NOT_SELECTABLE,
        },
        ("invariants", "BMC" | "explicit") => Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/explicit_engine.rs",
                anchor: "fn explicit_property_narrowing_matches_bmc()",
            },
        },
        ("invariants", "induction") => Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/verification_ownership.rs",
                anchor: "fn selected_invariant_induction_keeps_its_existing_isolated_model_semantics()",
            },
        },
        ("transitions", "BMC" | "explicit") => Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/assurance/property_selection.rs",
                anchor: "fn selecting_or_excluding_a_transition_matches_across_bmc_and_explicit_engines()",
            },
        },
        ("transitions", "induction") => Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/verification_ownership.rs",
                anchor: "fn selected_transition_induction_matches_the_all_properties_proof()",
            },
        },
        ("reachables", "BMC" | "explicit") => Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/assurance/property_selection.rs",
                anchor: "fn selecting_or_excluding_a_reachable_matches_across_bmc_and_explicit_engines()",
            },
        },
        ("reachables", "induction") => Claim::UnsupportedFailClosed {
            by: Citation {
                path: "rust/fslc/tests/assurance/properties.rs",
                anchor: "fn induction_rejects_a_reachable_property_fail_closed()",
            },
        },
        ("leads_to", "BMC") => Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/assurance/property_selection.rs",
                anchor: "fn selecting_or_excluding_a_leadsto_matches_on_bmc()",
            },
        },
        ("leads_to", "explicit") => Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/explicit_engine.rs",
                anchor: "fn explicit_excluding_a_leadsto_property_is_accepted_and_verifies_the_rest()",
            },
        },
        ("leads_to", "induction") => Claim::UnsupportedFailClosed {
            by: Citation {
                path: "rust/fslc/tests/verification_ownership.rs",
                anchor: "fn induction_property_selection_keeps_unknown_and_unsupported_kinds_rejected()",
            },
        },
        _ => {
            panic!("property_selection: no capability posture recorded for `{kind}` on `{engine}`")
        }
    }
}

/// `leads_to x explicit x selected` is the one row/engine pair where
/// `"excluded"` and `"selected"` genuinely diverge: excluding the leadsTo
/// property is accepted (cited above), but *selecting* it in isolation
/// still hits the same structural incapability as an unfiltered run
/// (explicit has no leadsTo proof strategy at all, regardless of what else
/// is or is not in the model) and must reject closed too. Recorded as its
/// own override so the shared `cell()` table above is not forced to pretend
/// the two lanes always agree.
fn leads_to_explicit_selected() -> Claim {
    Claim::UnsupportedFailClosed {
        by: Citation {
            path: "rust/fslc/tests/assurance/property_selection.rs",
            anchor: "fn selecting_a_leadsto_under_explicit_is_rejected_closed_regardless_of_isolation()",
        },
    }
}

#[must_use]
pub fn axis() -> Axis {
    let engines = engines();
    let columns = vec!["selected", "excluded"];
    let mut rows = Vec::new();
    let mut cells = std::collections::BTreeMap::new();
    for &kind in properties::ROWS {
        for &engine in &engines {
            let key = row(kind, engine);
            rows.push(key);
            let selected = if kind == "leads_to" && engine == "explicit" {
                leads_to_explicit_selected()
            } else {
                cell(kind, engine)
            };
            cells.insert((key, "selected"), selected);
            cells.insert((key, "excluded"), cell(kind, engine));
        }
    }
    Axis {
        name: "property_selection",
        rows,
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

/// This axis's rows must track `properties::ROWS` x `properties.rs`'s own
/// engine columns exactly -- proving the "no hand-copied list" claim above
/// is actually true and not just asserted in a comment.
#[test]
fn rows_are_generated_from_the_properties_axis_registry_not_copied() {
    let generated = axis().rows;
    let mut expected = Vec::new();
    for &kind in properties::ROWS {
        for engine in engines() {
            expected.push(format!("{kind}x{engine}"));
        }
    }
    assert_eq!(
        generated.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn selecting_or_excluding_a_transition_matches_across_bmc_and_explicit_engines() {
    let fixture = "rust/fslc/tests/fixtures/issue_701_induction_trans_siblings.fsl";
    for engine in ["bmc", "explicit"] {
        let (selected, selected_status) = run_verify(&[
            "verify",
            fixture,
            "--engine",
            engine,
            "--property",
            "StartsZero",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        assert_eq!(selected_status, 0, "{engine}: {selected}");
        assert!(
            matches!(selected["result"].as_str(), Some("proved" | "verified")),
            "{engine}: {selected}"
        );
        assert_eq!(
            selected["transitions_checked"],
            Value::from(vec!["StartsZero"])
        );

        let (excluded, excluded_status) = run_verify(&[
            "verify",
            fixture,
            "--engine",
            engine,
            "--exclude-property",
            "StaysZero",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        assert_eq!(excluded_status, 0, "{engine}: {excluded}");
        assert!(
            matches!(excluded["result"].as_str(), Some("proved" | "verified")),
            "{engine}: {excluded}"
        );
        assert_eq!(
            excluded["transitions_checked"],
            Value::from(vec!["StartsZero"])
        );
        assert_eq!(
            excluded["invariants_checked"],
            Value::from(vec!["_bounds_x", "Zero"]),
            "{engine}: excluding a sibling transition keeps the shared invariant hypothesis: {excluded}"
        );
    }
}

/// The induction-specific sibling-exclusion cell: excluding `StaysZero`
/// leaves `StartsZero` as induction's only transition obligation, still
/// proved with the full invariant hypothesis retained -- the same shape as
/// `verification_ownership.rs`'s `--property`-selected positive control,
/// exercised through `--exclude-property` instead.
#[test]
fn excluding_a_sibling_transition_still_proves_the_remaining_one_by_induction() {
    let (excluded, status) = run_verify(&[
        "verify",
        "rust/fslc/tests/fixtures/issue_701_induction_trans_siblings.fsl",
        "--engine",
        "induction",
        "--exclude-property",
        "StaysZero",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{excluded}");
    assert_eq!(excluded["result"], "proved");
    assert_eq!(
        excluded["transitions_checked"],
        Value::from(vec!["StartsZero"])
    );
    assert_eq!(
        excluded["invariants_checked"],
        Value::from(vec!["_bounds_x", "Zero"])
    );
}

#[test]
fn selecting_or_excluding_a_reachable_matches_across_bmc_and_explicit_engines() {
    let fixture = "rust/fslc/tests/fixtures/explicit_reachable_witnessed.fsl";
    for engine in ["bmc", "explicit"] {
        let (selected, selected_status) = run_verify(&[
            "verify",
            fixture,
            "--engine",
            engine,
            "--property",
            "HitTwo",
            "--depth",
            "4",
            "--no-cache",
        ]);
        assert_eq!(selected_status, 0, "{engine}: {selected}");
        assert!(
            selected["reachables"]["HitTwo"]["witnessed_at_step"].is_number(),
            "{engine}: {selected}"
        );

        let (excluded, excluded_status) = run_verify(&[
            "verify",
            fixture,
            "--engine",
            engine,
            "--exclude-property",
            "HitTwo",
            "--depth",
            "4",
            "--no-cache",
        ]);
        assert_eq!(excluded_status, 0, "{engine}: {excluded}");
        assert!(
            matches!(excluded["result"].as_str(), Some("proved" | "verified")),
            "{engine}: {excluded}"
        );
        assert_eq!(
            excluded["reachables"],
            serde_json::json!({}),
            "{engine}: excluding the only reachable leaves nothing to witness: {excluded}"
        );
    }
}

/// Induction cannot select a `reachable` at all (the rejecting control
/// above), but *excluding* one removes it from the run entirely rather than
/// asking induction to prove it -- so this cell must succeed, not inherit
/// the selected-mode rejection.
#[test]
fn excluding_a_reachable_still_proves_by_induction_with_nothing_left_to_check() {
    let (excluded, status) = run_verify(&[
        "verify",
        "rust/fslc/tests/fixtures/explicit_reachable_witnessed.fsl",
        "--engine",
        "induction",
        "--exclude-property",
        "HitTwo",
        "--depth",
        "4",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{excluded}");
    assert_eq!(excluded["result"], "proved");
    assert_eq!(excluded["reachables"], serde_json::json!({}));
}

#[test]
fn selecting_or_excluding_a_leadsto_matches_on_bmc() {
    let fixture = "rust/fslc/tests/fixtures/explicit_exclude_leadsto.fsl";
    let (selected, selected_status) = run_verify(&[
        "verify",
        fixture,
        "--engine",
        "bmc",
        "--property",
        "FlagLeadsToDone",
        "--depth",
        "4",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(selected_status, 0, "{selected}");
    assert_eq!(selected["result"], "verified");
    assert!(
        selected["leads_to"]["FlagLeadsToDone"].is_object(),
        "{selected}"
    );
    assert_eq!(
        selected["invariants_checked"],
        Value::from(Vec::<String>::new())
    );

    let (excluded, excluded_status) = run_verify(&[
        "verify",
        fixture,
        "--engine",
        "bmc",
        "--exclude-property",
        "FlagLeadsToDone",
        "--depth",
        "4",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(excluded_status, 0, "{excluded}");
    assert_eq!(excluded["result"], "verified");
    assert!(excluded.get("leads_to").is_none(), "{excluded}");
    assert_eq!(
        excluded["invariants_checked"],
        Value::from(vec!["DoneImpliesFlag"])
    );
}

/// `explicit` structurally has no leadsTo proof strategy (the same fact
/// `explicit_cli_exit_codes_cover_bounded_proved_violated_budget_and_semantics`
/// exercises for an unfiltered run): isolating the leadsTo property with
/// `--property` does not change that, so the same `semantics`-kind
/// rejection must still fire -- the `leads_to x explicit x selected`
/// rejecting control this axis's design boundary calls for.
#[test]
fn selecting_a_leadsto_under_explicit_is_rejected_closed_regardless_of_isolation() {
    let (rejected, status) = run_verify(&[
        "verify",
        "rust/fslc/tests/fixtures/explicit_exclude_leadsto.fsl",
        "--engine",
        "explicit",
        "--property",
        "FlagLeadsToDone",
        "--depth",
        "4",
        "--no-cache",
    ]);
    assert_eq!(status, 2, "{rejected}");
    assert_eq!(rejected["result"], "error");
    assert_eq!(rejected["kind"], "semantics");
    assert!(
        rejected["message"]
            .as_str()
            .is_some_and(|message| message.contains("does not support leadsTo")),
        "{rejected}"
    );
}

/// Excluding the leadsTo property leaves only the ordinary invariant, which
/// induction proves the same way it proves any other selected/excluded
/// invariant-only model.
#[test]
fn excluding_a_leadsto_still_proves_the_remaining_invariant_by_induction() {
    let (excluded, status) = run_verify(&[
        "verify",
        "rust/fslc/tests/fixtures/explicit_exclude_leadsto.fsl",
        "--engine",
        "induction",
        "--exclude-property",
        "FlagLeadsToDone",
        "--depth",
        "4",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(status, 0, "{excluded}");
    assert_eq!(excluded["result"], "proved");
    assert_eq!(
        excluded["invariants_checked"],
        Value::from(vec!["DoneImpliesFlag"])
    );
}

/// Excluding an invariant is accepted on every engine and leaves the
/// remaining invariant checked and proved/verified -- the `invariants x *
/// x excluded` column across all three engines.
#[test]
fn excluding_an_invariant_still_verifies_the_remaining_invariant_on_every_engine() {
    let fixture = "rust/fslc/tests/fixtures/explicit_property_narrow.fsl";
    for engine in ["bmc", "explicit", "induction"] {
        let (excluded, status) = run_verify(&[
            "verify",
            fixture,
            "--engine",
            engine,
            "--exclude-property",
            "NeverTrue",
            "--depth",
            "4",
            "--no-cache",
        ]);
        assert_eq!(status, 0, "{engine}: {excluded}");
        assert!(
            matches!(excluded["result"].as_str(), Some("proved" | "verified")),
            "{engine}: {excluded}"
        );
        assert_eq!(
            excluded["invariants_checked"],
            Value::from(vec!["AlwaysBoolean"]),
            "{engine}: {excluded}"
        );
    }
}

/// Mutation control: flip a single cell's classification away from what
/// the CLI actually does and `check_complete`/citation-based intent breaks
/// down at the point that matters -- the positive/rejecting probes above
/// are real subprocess runs, so inverting the codebase's induction-trans
/// capability (the exact regression #701 fixed) makes
/// `selecting_or_excluding_a_transition_matches_across_bmc_and_explicit_engines`-style
/// assertions fail, not this file's bookkeeping. This test pins the one
/// place that assertion is not implicit: the axis's own row/column
/// declaration must still equal 5 property groups x 3 engines, so trimming
/// a row or column out from under a claimed cell (rather than fixing the
/// claim) is caught by `check_complete`'s stale-cell branch.
#[test]
fn axis_declares_every_property_group_across_every_selection_engine() {
    let built = axis();
    assert_eq!(built.rows.len(), properties::ROWS.len() * engines().len());
    assert_eq!(built.columns, vec!["selected", "excluded"]);
    assert!(built.check_complete().is_ok());
}
