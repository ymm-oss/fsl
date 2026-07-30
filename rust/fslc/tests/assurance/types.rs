// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `types` axis: all 9 `fsl_core::model::TypeRef` variants and all three
//! `TypeDef` variants (issue #537 C3 slice 3).
//!
//! The expression-variant C6 family deliberately carries every type row in
//! each model schema and asserts the source-derived 12-row inventory before
//! running Monitor BFS / explicit / BMC agreement. This axis declares the
//! two expression evaluator columns, `Monitor` and `BMC`; explicit remains a
//! concrete-path control rather than a distinct type implementation.

use std::collections::{BTreeMap, BTreeSet};

use crate::claim::{Axis, Citation, Claim};
use crate::enum_rows::type_rows;

const SWEEP: Citation = Citation {
    path: "rust/fslc/tests/typed_agreement.rs",
    anchor: "fn expression_variant_sweep_agrees_across_all_three_engines_and_covers_all_types()",
};

#[must_use]
pub fn axis() -> Axis {
    let rows = type_rows();
    let columns = vec!["Monitor", "BMC"];
    let mut cells: BTreeMap<(&'static str, &'static str), Claim> = BTreeMap::new();
    for &row in &rows {
        for &column in &columns {
            cells.insert((row, column), Claim::Exercised { by: SWEEP });
        }
    }
    Axis {
        name: "types",
        rows,
        columns,
        cells,
    }
}

#[test]
fn type_rows_are_complete_and_unique() {
    let rows = type_rows();
    assert_eq!(rows.len(), 12, "9 TypeRef rows plus 3 TypeDef rows");
    assert_eq!(
        rows.iter().copied().collect::<BTreeSet<_>>().len(),
        rows.len(),
        "type-axis rows must be unique"
    );
}
