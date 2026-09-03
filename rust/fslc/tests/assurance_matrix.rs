// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! C3 Semantic Assurance Matrix aggregator (issue #537 C3 slice 1,
//! `docs/DESIGN-assurance-matrix.md`).
//!
//! No central hand-written table: each semantic-surface module under
//! `tests/assurance/` owns its own rows (derived from an existing registry
//! or enum, never copied) and declared columns, and builds a `Claim` --
//! `Exercised`, `RejectingControl`, `UnsupportedFailClosed`, or
//! `NotApplicable` -- for every `(row, column)` cell in that scope. This
//! file only aggregates and enforces completeness across all axes.
//!
//! Two independent failure modes are enforced, matching issue #537's
//! acceptance criteria ("required Semantic Assurance Matrix blank cells =
//! 0" and every claim's citation must be real, not fabricated):
//!
//! 1. [`every_declared_cell_across_every_axis_has_a_claim`] -- no blank
//!    required cell.
//! 2. [`every_claim_citation_rechecks_against_the_working_tree`] -- no
//!    fabricated or stale citation.
//!
//! [`negative_controls`] demonstrates both checks actually fail on a
//! corrupted citation and an unclaimed cell -- without this, a checker that
//! always returns `Ok` would pass the two tests above vacuously.

#[path = "assurance/claim.rs"]
mod claim;
#[path = "assurance/dialects.rs"]
mod dialects;
#[path = "assurance/enum_rows.rs"]
mod enum_rows;
#[path = "assurance/expr.rs"]
mod expr;
#[path = "assurance/outcome_kind.rs"]
mod outcome_kind;
#[path = "assurance/properties.rs"]
mod properties;
#[path = "assurance/property_selection.rs"]
mod property_selection;
#[path = "support/mod.rs"]
mod support;
#[path = "assurance/types.rs"]
mod types;
#[path = "assurance/violation_kind.rs"]
mod violation_kind;

use claim::Axis;

fn axes() -> Vec<Axis> {
    vec![
        outcome_kind::axis(),
        violation_kind::axis(),
        properties::axis(),
        property_selection::axis(),
        expr::axis(),
        types::axis(),
        dialects::axis(),
    ]
}

#[test]
fn every_declared_cell_across_every_axis_has_a_claim() {
    let errors = axes()
        .iter()
        .filter_map(|axis| axis.check_complete().err())
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "assurance matrix has blank required cells:\n{}",
        errors.join("\n")
    );
}

#[test]
fn every_claim_citation_rechecks_against_the_working_tree() {
    let errors = axes()
        .iter()
        .filter_map(|axis| axis.check_citations().err())
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "assurance matrix has claims with stale or fabricated citations:\n{}",
        errors.join("\n")
    );
}

mod negative_controls {
    use std::collections::BTreeMap;

    use crate::claim::{Axis, Citation, Claim};

    /// A citation whose anchor text does not appear anywhere in the target
    /// file must fail `recheck()`. Without this, a checker that always
    /// returns `Ok` would pass
    /// `every_claim_citation_rechecks_against_the_working_tree` vacuously.
    #[test]
    fn a_citation_with_a_fabricated_anchor_fails_recheck() {
        let broken = Citation {
            path: "rust/Cargo.toml",
            anchor: "this-anchor-text-does-not-exist-in-the-manifest-6f2a1c",
        };
        assert!(
            broken.recheck().is_err(),
            "a fabricated anchor must fail recheck"
        );
    }

    /// A zero-argument function anchor must resolve to an actual item
    /// definition, not merely appear inside a registration string literal.
    #[test]
    fn a_function_anchor_without_a_definition_fails_recheck() {
        let broken = Citation {
            path: "rust/fslc/tests/triangulated/p2_witness_replay.rs",
            anchor: "fn fabricated_p2_rejecting_control_without_definition()",
        };
        assert!(
            broken.recheck().is_err(),
            "a function anchor must resolve to an actual definition"
        );
    }

    /// A zero-argument function anchor must pass when exactly one definition
    /// exists in the cited file.
    #[test]
    fn a_function_anchor_with_a_definition_passes_recheck() {
        let good = Citation {
            path: "rust/fslc/tests/triangulated/p2_witness_replay.rs",
            anchor: "fn corrupting_state_step_kind_or_location_cuts_a_p2_edge()",
        };
        assert!(
            good.recheck().is_ok(),
            "an actual function definition must satisfy recheck"
        );
    }

    /// A citation pointing at a nonexistent file must fail `recheck()`.
    #[test]
    fn a_citation_to_a_nonexistent_file_fails_recheck() {
        let broken = Citation {
            path: "rust/fslc/tests/assurance/this-file-does-not-exist.rs",
            anchor: "anything",
        };
        assert!(
            broken.recheck().is_err(),
            "a citation to a missing file must fail recheck"
        );
    }

    /// A required `(row, column)` cell with no `Claim` at all must fail
    /// `Axis::check_complete`. Without this, a checker that always returns
    /// `Ok` would pass `every_declared_cell_across_every_axis_has_a_claim`
    /// vacuously.
    #[test]
    fn an_unclaimed_required_cell_fails_the_completeness_check() {
        let mut cells = BTreeMap::new();
        cells.insert(
            ("row-a", "col-1"),
            Claim::Exercised {
                by: Citation {
                    path: "rust/Cargo.toml",
                    anchor: "[workspace]",
                },
            },
        );
        // "row-a" x "col-2" is deliberately left unclaimed.
        let toy = Axis {
            name: "negative_control_toy_axis",
            rows: vec!["row-a"],
            columns: vec!["col-1", "col-2"],
            cells,
        };
        assert!(
            toy.check_complete().is_err(),
            "an axis with an unclaimed required cell must fail check_complete"
        );
    }

    /// A cell keyed by a row that was removed/renamed from the axis's
    /// declared `rows` must fail `Axis::check_complete` too -- the reverse
    /// direction from [`an_unclaimed_required_cell_fails_the_completeness_check`].
    /// Without this, narrowing `rows`/`columns` after building `cells` would
    /// leave a stale, unverified `Claim` sitting outside the axis's own
    /// declared scope with nothing to catch it.
    #[test]
    fn a_cell_keyed_by_an_undeclared_row_fails_the_completeness_check() {
        let mut cells = BTreeMap::new();
        cells.insert(
            ("row-a", "col-1"),
            Claim::Exercised {
                by: Citation {
                    path: "rust/Cargo.toml",
                    anchor: "[workspace]",
                },
            },
        );
        // "row-stale" was removed from `rows` below but its cell was not.
        cells.insert(
            ("row-stale", "col-1"),
            Claim::Exercised {
                by: Citation {
                    path: "rust/Cargo.toml",
                    anchor: "[workspace]",
                },
            },
        );
        let toy = Axis {
            name: "negative_control_toy_axis_stale_row",
            rows: vec!["row-a"],
            columns: vec!["col-1"],
            cells,
        };
        assert!(
            toy.check_complete().is_err(),
            "an axis with a cell keyed by an undeclared row must fail check_complete"
        );
    }

    /// A fully-claimed toy axis with real citations must pass both checks --
    /// the positive control for the two negative controls above, proving the
    /// checkers are not simply always-`Err`.
    #[test]
    fn a_fully_claimed_toy_axis_passes_both_checks() {
        let mut cells = BTreeMap::new();
        cells.insert(
            ("row-a", "col-1"),
            Claim::Exercised {
                by: Citation {
                    path: "rust/Cargo.toml",
                    anchor: "[workspace]",
                },
            },
        );
        let toy = Axis {
            name: "positive_control_toy_axis",
            rows: vec!["row-a"],
            columns: vec!["col-1"],
            cells,
        };
        assert!(toy.check_complete().is_ok());
        assert!(toy.check_citations().is_ok());
    }
}
