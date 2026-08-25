// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Core `Claim`/`Citation`/`Axis` types for the C3 Semantic Assurance Matrix
//! (issue #537 C3, `docs/DESIGN-assurance-matrix.md`).
//!
//! A `Citation` is a machine-rechecked pointer: `path` + `anchor`, where
//! `anchor` must appear on some line of `path`. `recheck()` re-reads the
//! file from the working tree every time it runs -- nothing here is a cached
//! table, mirroring `tests/refine_corpus_parity.rs`'s
//! `declaration.recheck()` discipline (`#537 C4`, `#616`). A `Claim` names
//! one of four evidence classes for a single `(row, column)` cell and always
//! carries exactly one `Citation`. `Axis::check_complete` is the "no blank
//! required cells" enforcement; `Axis::check_citations` is the "no
//! fabricated citation" enforcement. Both return `Err` naming every failure
//! instead of stopping at the first one, matching
//! `coverage.rs::coverage_matrix`'s discipline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Repository root, derived from `CARGO_MANIFEST_DIR` (`rust/fslc`) rather
/// than a hardcoded absolute path, matching
/// `tests/conformance_coverage.rs::workspace_root`.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// A machine-rechecked pointer to a declaration: a repository-relative
/// `path` and an `anchor` substring that some line of that file must
/// contain. Never a line number -- an unrelated edit above the anchor must
/// not fail `recheck()`, but deleting or renaming the anchored declaration
/// must (same shape as `tests/refine_corpus_parity.rs::Declaration`).
#[derive(Clone, Copy, Debug)]
pub struct Citation {
    pub path: &'static str,
    pub anchor: &'static str,
}

impl Citation {
    /// Re-read `path` from the working tree and confirm some line contains
    /// `anchor`. Never trusts a prior run's result.
    ///
    /// # Errors
    ///
    /// Returns `Err` when `path` cannot be read, or when no line of it
    /// contains `anchor`.
    pub fn recheck(&self) -> Result<(), String> {
        let full = workspace_root().join(self.path);
        let content = std::fs::read_to_string(&full)
            .map_err(|error| format!("{}: cannot read citation target: {error}", self.path))?;
        if content.lines().any(|line| line.contains(self.anchor)) {
            Ok(())
        } else {
            Err(format!(
                "{}: anchor `{}` not found on any line (citation is stale or fabricated)",
                self.path, self.anchor
            ))
        }
    }
}

/// One assurance-matrix cell's evidence class, matching
/// `docs/DESIGN-assurance-matrix.md`'s cell vocabulary. Every variant
/// carries exactly one [`Citation`] -- there is no variant without one,
/// so a cell can never be "claimed" without something to recheck.
///
/// `RejectingControl` and `UnsupportedFailClosed` are unused by every
/// Slice 1 axis (`outcome_kind`, `violation_kind`, `properties`): none of
/// their cells are honestly a rejecting-control or fail-closed-unsupported
/// case rather than `Exercised`/`NotApplicable`, and forcing one to avoid a
/// dead-code warning would misclassify a real cell. Both variants are part
/// of the accepted `docs/DESIGN-assurance-matrix.md` cell vocabulary for
/// later slices (e.g. an `expr`/`types` axis construct a given engine
/// explicitly refuses).
#[allow(dead_code)]
pub enum Claim {
    /// A real, currently-passing test demonstrates the row is actually
    /// produced/checked on this column's surface.
    Exercised { by: Citation },
    /// A real, currently-passing test demonstrates this column's surface
    /// correctly *rejects* a contract-violating case for this row.
    RejectingControl { by: Citation },
    /// This column's surface does not support the row at all, and rejects
    /// it closed (an explicit error/refusal, not silent success).
    UnsupportedFailClosed { by: Citation },
    /// This `(row, column)` combination is structurally out of scope for a
    /// reason recorded in `reason`, with `basis` citing where that
    /// structural fact is codified (source or design doc).
    NotApplicable {
        reason: &'static str,
        basis: Citation,
    },
}

impl Claim {
    #[must_use]
    pub fn citation(&self) -> &Citation {
        match self {
            Self::Exercised { by }
            | Self::RejectingControl { by }
            | Self::UnsupportedFailClosed { by } => by,
            Self::NotApplicable { basis, .. } => basis,
        }
    }

    /// # Errors
    ///
    /// Returns `Err` when this claim's citation fails [`Citation::recheck`].
    pub fn recheck(&self) -> Result<(), String> {
        self.citation().recheck()
    }
}

/// One semantic-surface axis of the assurance matrix: a set of rows, a set
/// of *declared* columns (the axis's own required scope -- see the
/// `docs/DESIGN-assurance-matrix.md` "Cell vocabulary"), and the
/// `Claim` for every `(row, column)` pair the axis declares as required.
pub struct Axis {
    pub name: &'static str,
    pub rows: Vec<&'static str>,
    pub columns: Vec<&'static str>,
    pub cells: BTreeMap<(&'static str, &'static str), Claim>,
}

impl Axis {
    /// Every `(row, column)` pair over this axis's declared rows/columns
    /// must have a `Claim` (a blank cell is a hard failure, never a silent
    /// N/A -- N/A is itself a `Claim` variant that must cite its basis), and
    /// conversely `cells` must not carry a key outside `rows`x`columns` --
    /// a row or column removed/renamed from the axis's declared scope must
    /// take its cells with it, or the stale cell is dead, unverified
    /// registry state hiding behind a scope that no longer claims it. This
    /// is the same both-directions discipline as the corpus/refinement
    /// manifests (`refine_corpus_parity.rs`'s registered-vs-declared check,
    /// `coverage.rs`'s unrecognized-outcome-kind rejection): unregistered
    /// fails, and stale-registered fails too.
    ///
    /// # Errors
    ///
    /// Returns `Err` naming every missing cell and every stale cell.
    pub fn check_complete(&self) -> Result<(), String> {
        let rows = self
            .rows
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let columns = self
            .columns
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let missing = self
            .rows
            .iter()
            .flat_map(|row| self.columns.iter().map(move |column| (*row, *column)))
            .filter(|key| !self.cells.contains_key(key))
            .map(|(row, column)| format!("{}::{row}x{column} (missing)", self.name));
        let stale = self
            .cells
            .keys()
            .filter(|&&(row, column)| !rows.contains(row) || !columns.contains(column))
            .map(|(row, column)| {
                format!(
                    "{}::{row}x{column} (stale, outside declared rows/columns)",
                    self.name
                )
            });
        let errors = missing.chain(stale).collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("cell errors: {}", errors.join(", ")))
        }
    }

    /// Every claimed cell's citation must recheck against the current
    /// working tree.
    ///
    /// # Errors
    ///
    /// Returns `Err` naming every cell whose citation failed to recheck.
    pub fn check_citations(&self) -> Result<(), String> {
        let errors = self
            .cells
            .iter()
            .filter_map(|((row, column), claim)| {
                claim
                    .recheck()
                    .err()
                    .map(|error| format!("{}::{row}x{column}: {error}", self.name))
            })
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}
