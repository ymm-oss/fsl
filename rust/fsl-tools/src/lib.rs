// SPDX-License-Identifier: Apache-2.0

//! Native report generators and specialized FSL dialect engines.

mod ai;
mod analysis;
mod analysis_export;
mod analysis_graph;
mod causal;
mod causal_analysis;
mod causal_evidence;
mod causal_projection;
mod db;
mod db_import;
mod domain;
mod domain_codegen;
mod html;
mod ledger;
mod mutate;
mod public_kernel;
mod refinement_analysis;
mod testgen;
mod typestate;
mod undecided;

pub use ai::{check_ai, replay_ai};
pub use analysis::{analyze_model, build_tsg, review_finding, structural_review_findings};
pub use analysis_export::export_analysis_graph;
pub use causal::{
    Binding, BindingKind, CausalError, CausalModel, CausalWarning, Claim, ClaimStatus, Clock,
    Feedback, ImportedSpec, Interval, Lag, Persistence, Polarity, ScopeVocabulary, Variable,
    VariableRole, active_adjacency, build_causal_model, reachable_from,
    strongly_connected_components,
};
pub use causal_analysis::{
    causal_check_json, causal_review_findings, earliest_from, feedback_loop_classes,
    indicator_classes, latest_bound_from, polarity_reach,
};
pub use causal_evidence::{
    Applicability, EvidenceArtifact, EvidenceError, LifecycleStatus, ScopeApplication,
    SupportOverlay, aggregate_support, artifact_digest, canonical_json, causal_evidence_graph,
    lifecycle_record_digest, parse_artifact, validate_lifecycle_chain,
};
pub use causal_projection::{
    causal_diff_json, causal_dot, causal_graph_projection, causal_mermaid, causal_review_json,
    causal_timeline_projection, causal_traceability_projection,
};
pub use db::{DbToolError, check_db, observe_db, validate_db};
pub use db_import::{DbImport, import_db};
pub use domain::{
    analyze_domain, check_domain, domain_adapter_files, domain_kernel_source, domain_scaffold,
    domain_scaffold_metadata,
};
pub use html::render_html_report;
pub use ledger::{render_ledger, render_ledger_with_approvals};
pub use mutate::{BuiltinMutant, enumerate_builtin_mutants};
pub use refinement_analysis::analyze_refinement;
pub use testgen::{
    TestgenInput, compose_testgen_input, generate_testgen, public_kernel_testgen_input,
};
pub use typestate::analyze_typestate;
pub use undecided::undecided_declarations;

/// Complete a deterministic graph envelope from already normalized nodes and edges.
#[must_use]
pub fn complete_analysis_graph(
    projection: &str,
    nodes: &[serde_json::Value],
    edges: &[serde_json::Value],
) -> serde_json::Value {
    analysis_graph::graph_envelope(projection, nodes, edges)
}
