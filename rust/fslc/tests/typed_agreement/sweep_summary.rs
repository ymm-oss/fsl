// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Machine-readable sweep summary: counts of (domain kind, domain size,
//! property kind, operation/context) this run actually exercised, printed
//! at the end of each sweep test so a future #537 C3 slice-2 `expr` axis
//! citation can point at a specific, re-runnable count instead of a prose
//! claim. Slice 1 only aggregates; it does not register an axis with the C3
//! matrix (`assurance_matrix.rs`) -- that is slice 2's job, per
//! `docs/DESIGN-conformance-harness.md`'s C6 section.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Default, Debug)]
pub struct SweepSummary {
    domain_kind_counts: BTreeMap<String, usize>,
    domain_size_counts: BTreeMap<i64, usize>,
    property_kind_counts: BTreeMap<String, usize>,
    state_var_counts: BTreeMap<usize, usize>,
    action_counts: BTreeMap<usize, usize>,
    guarded_count: usize,
    fair_count: usize,
    operation_counts: BTreeMap<String, usize>,
    total_models: usize,
}

impl SweepSummary {
    #[allow(clippy::too_many_arguments)]
    pub fn record_domain_model(
        &mut self,
        domain_kind: &str,
        domain_size: i64,
        property_kind: &str,
        state_vars: usize,
        action_count: usize,
        guarded: bool,
        fair: bool,
    ) {
        *self
            .domain_kind_counts
            .entry(domain_kind.to_owned())
            .or_insert(0) += 1;
        *self.domain_size_counts.entry(domain_size).or_insert(0) += 1;
        *self
            .property_kind_counts
            .entry(property_kind.to_owned())
            .or_insert(0) += 1;
        *self.state_var_counts.entry(state_vars).or_insert(0) += 1;
        *self.action_counts.entry(action_count).or_insert(0) += 1;
        if guarded {
            self.guarded_count += 1;
        }
        if fair {
            self.fair_count += 1;
        }
        self.total_models += 1;
    }

    pub fn record_operation_model(&mut self, operation: &str, context: &str) {
        *self
            .operation_counts
            .entry(format!("{operation}/{context}"))
            .or_insert(0) += 1;
        self.total_models += 1;
    }
}

impl fmt::Display for SweepSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "total_models={} domain_kinds={:?} domain_sizes={:?} property_kinds={:?} \
             state_vars={:?} action_counts={:?} guarded={} fair={} operations={:?}",
            self.total_models,
            self.domain_kind_counts,
            self.domain_size_counts,
            self.property_kind_counts,
            self.state_var_counts,
            self.action_counts,
            self.guarded_count,
            self.fair_count,
            self.operation_counts
        )
    }
}
