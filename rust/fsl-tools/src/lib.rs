// SPDX-License-Identifier: Apache-2.0

//! Native report generators and specialized FSL dialect engines.

mod ai;
mod analysis;
mod analysis_export;
mod analysis_graph;
mod db;
mod db_import;
mod domain;
mod domain_codegen;
mod html;
mod ledger;
mod mutate;
mod refinement_analysis;
mod testgen;
mod typestate;

pub use ai::{check_ai, replay_ai};
pub use analysis::{analyze_model, build_tsg};
pub use analysis_export::export_analysis_graph;
pub use db::{DbToolError, check_db, observe_db, validate_db};
pub use db_import::{DbImport, import_db};
pub use domain::{
    analyze_domain, check_domain, domain_adapter_files, domain_kernel_source, domain_scaffold,
};
pub use html::render_html_report;
pub use ledger::render_ledger;
pub use mutate::{BuiltinMutant, enumerate_builtin_mutants};
pub use refinement_analysis::analyze_refinement;
pub use testgen::{emit_dart, emit_kotlin, emit_phpunit, emit_swift, emit_vitest};
pub use typestate::analyze_typestate;

/// Complete a deterministic graph envelope from already normalized nodes and edges.
#[must_use]
pub fn complete_analysis_graph(
    projection: &str,
    nodes: &[serde_json::Value],
    edges: &[serde_json::Value],
) -> serde_json::Value {
    analysis_graph::graph_envelope(projection, nodes, edges)
}
