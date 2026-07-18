// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The Requirement Claim IR (RCIR) v1 projector (issue #325).
//!
//! Reimplements none of: the parser, name resolver, type checker, expression
//! evaluator, transition semantics, solver encoding, or assurance classifier.
//! It classifies already-checked nodes and serializes them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use fsl_core::{
    ActionDef, ActionGuard, Annotation, Annotations, FsResolver, INIT_TARGET, KernelModel,
    KernelSpec, OriginChain, ParamDef, RequirementLink, RequirementsTraceCase,
    RequirementsTraceContract, RequirementsTraceExpectation, TERMINAL_TARGET, TypeRef,
    action_target, build_model, display_name, parse_kernel_source, property_target,
    requirements_trace_contract,
};
use fsl_syntax::{
    RequirementsItem, SourceFile, Span, SpecItem, SurfaceDocument, VerifyItem, parse_document,
};
use serde_json::{Value, json};

use crate::document::{
    AnalysisScope, AssuranceCounts, Claim, ClaimKind, ClaimProvenance, Completeness, Coverage,
    CoverageCounts, ProvenanceAssurance, ProvenanceSummary, RCIR_SCHEMA_ID, RCIR_SCHEMA_VERSION,
    Requirement, RequirementClaimSet, RequirementStatement, SemanticsInfo, SourceRef, SpecInfo,
    TraceCase, TraceCaseKind, UnsupportedEntry,
};
use crate::document_digest::{
    CLAIM_DIGEST_ALGORITHM, CLAIM_SET_DIGEST_ALGORITHM, SPEC_DIGEST_ALGORITHM, framed_digest,
    normalized_kernel_ast, spec_digest_from_kernel,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentDialect {
    Spec,
    Requirements,
}

impl DocumentDialect {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spec => "spec",
            Self::Requirements => "requirements",
        }
    }
}

pub struct DocumentInput<'a> {
    pub kernel: &'a KernelSpec,
    pub model: &'a KernelModel,
    pub source: &'a str,
    pub source_path: Option<&'a str>,
    pub dialect: DocumentDialect,
    pub implements_names: Vec<String>,
    pub analysis_scope: AnalysisScope,
}

/// `entity`/`number` + `verify { instances ...; values ... }` bounds are fully
/// consumed during `lower_requirements` (folded into concrete bounded types)
/// and leave no trace in the lowered `KernelSpec`/`SurfaceSpec`, so analysis
/// scope must be read from the surface `requirements` tree directly, the same
/// way `implements` names are.
fn requirements_analysis_scope(requirements: &fsl_syntax::SurfaceRequirements) -> AnalysisScope {
    let mut instances = Vec::new();
    let mut values = Vec::new();
    for item in &requirements.items {
        if let RequirementsItem::Common(SpecItem::VerifyBounds { items, .. }) = item {
            for verify_item in items {
                match verify_item {
                    VerifyItem::Instances(name, count, _) => {
                        instances.push(json!({"entity": name, "count": count}));
                    }
                    VerifyItem::Values(name, lo, hi, _) => {
                        values.push(json!({
                            "number": name,
                            "lo": normalize(lo.python_ast()),
                            "hi": normalize(hi.python_ast()),
                        }));
                    }
                }
            }
        }
    }
    AnalysisScope { instances, values }
}

/// Parse, lower, build, and project `source` in one call (test/tooling
/// convenience around [`project_requirement_claims`]).
///
/// # Errors
///
/// Returns an error string for an unsupported dialect, a parse/lowering/model
/// failure, or a projection failure.
pub fn project_requirement_claims_from_source(
    source: &str,
    source_path: Option<&str>,
    resolver_root: &Path,
) -> Result<RequirementClaimSet, String> {
    let parsed = parse_document(SourceFile::new(source)).map_err(|error| error.to_string())?;
    let (dialect, implements_names, analysis_scope) = match &parsed.surface {
        SurfaceDocument::Spec(_) => (DocumentDialect::Spec, Vec::new(), AnalysisScope::default()),
        SurfaceDocument::Requirements(requirements) => {
            let names = requirements
                .items
                .iter()
                .filter_map(|item| match item {
                    RequirementsItem::Implements { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect();
            (
                DocumentDialect::Requirements,
                names,
                requirements_analysis_scope(requirements),
            )
        }
        other => {
            return Err(format!(
                "document projection does not support dialect '{}' in RCIR v1",
                surface_dialect_name(other)
            ));
        }
    };
    let resolver = FsResolver::new(resolver_root);
    let kernel = parse_kernel_source(source, &resolver).map_err(|error| error.to_string())?;
    let model = build_model(kernel.clone()).map_err(|error| error.to_string())?;
    project_requirement_claims(&DocumentInput {
        kernel: &kernel,
        model: &model,
        source,
        source_path,
        dialect,
        implements_names,
        analysis_scope,
    })
}

fn surface_dialect_name(document: &SurfaceDocument) -> &'static str {
    match document {
        SurfaceDocument::Spec(_) => "spec",
        SurfaceDocument::Business(_) => "business",
        SurfaceDocument::Requirements(_) => "requirements",
        SurfaceDocument::Governance(_) => "governance",
        SurfaceDocument::Db(_) => "dbsystem",
        SurfaceDocument::Domain(_) => "domain",
        SurfaceDocument::AiComponent(_) => "ai_component",
        SurfaceDocument::Compose(_) => "compose",
        SurfaceDocument::Refinement(_) => "refinement",
        SurfaceDocument::Agent(_) => "agent",
    }
}

#[derive(Default)]
struct RequirementAgg {
    statements: BTreeSet<(Option<String>, SourceRef)>,
    claim_ids: BTreeSet<String>,
    kinds: BTreeSet<String>,
}

fn source_ref_lc(source_path: Option<&str>, line: u32, column: u32) -> SourceRef {
    SourceRef {
        path: source_path.map(str::to_owned),
        line,
        column,
    }
}

fn source_ref_span(source_path: Option<&str>, span: Span) -> SourceRef {
    source_ref_lc(source_path, span.start.line, span.start.column)
}

#[allow(clippy::needless_pass_by_value)]
fn normalize(value: Value) -> Value {
    normalized_kernel_ast(&value).unwrap_or(Value::Null)
}

fn kind_ids_from(annotations: &Annotations) -> BTreeSet<String> {
    annotations
        .source_order()
        .iter()
        .filter_map(|annotation| match annotation {
            Annotation::Kind { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Record every requirement link a claim carries, returning its sorted,
/// deduplicated requirement ID list for [`Claim::requirements`].
fn record_links(
    links: &[RequirementLink],
    kind_ids: &BTreeSet<String>,
    claim_id: &str,
    source_path: Option<&str>,
    agg: &mut BTreeMap<String, RequirementAgg>,
) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for link in links {
        ids.insert(link.id.clone());
        let entry = agg.entry(link.id.clone()).or_default();
        entry
            .statements
            .insert((link.text.clone(), source_ref_span(source_path, link.span)));
        entry.claim_ids.insert(claim_id.to_owned());
        entry.kinds.extend(kind_ids.iter().cloned());
    }
    ids.into_iter().collect()
}

fn chain_assurance(chain: &OriginChain) -> ProvenanceAssurance {
    let complete_primary = chain
        .primary
        .as_ref()
        .filter(|site| site.source_file.is_some() && site.span.is_some());
    match (complete_primary.is_some(), chain.generated) {
        (true, false) => ProvenanceAssurance::SourceBacked,
        (true, true) => ProvenanceAssurance::GeneratedFromSource,
        (false, true) => ProvenanceAssurance::GeneratedOnly,
        (false, false) => ProvenanceAssurance::Unknown,
    }
}

/// Compute a claim's provenance from the internal origin registry, falling
/// back to the declaration's own checked-model span when the registry has no
/// chain for the target at all.
///
/// The origin registry (`OriginChain`/`OriginSite`) is the richer of the two
/// signals where it is populated (it can distinguish `generated_from_source`
/// from `generated_only`), but it is currently sparse outside the domain
/// dialect (docs/DESIGN-kernel-origin-v2.md): `requirements`/`spec` targets
/// routinely have zero bound origins today. `declared_span` is not a guess —
/// it is the same `Span` the parser attached to the authored declaration
/// (`ActionDef::span`, `PropertyDef::span`, ...) — so when the registry is
/// silent, a real declared span still grounds `source_backed` rather than
/// falling through to `unknown`. Only a target with neither signal (e.g.
/// `terminal`, whose `Expr` carries no span of its own) reports `unknown`.
fn provenance_for(
    model: &KernelModel,
    targets: &[String],
    declared_span: Option<Span>,
    source_path: Option<&str>,
) -> ClaimProvenance {
    let mut origin_ids = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut assurance: Option<ProvenanceAssurance> = None;
    for target in targets {
        for chain in model.origins().origins_for(target) {
            origin_ids.insert(chain.id.0.clone());
            let this_assurance = chain_assurance(chain);
            assurance =
                Some(assurance.map_or(this_assurance, |current| current.max(this_assurance)));
            if let Some(site) = chain
                .primary
                .as_ref()
                .filter(|site| site.source_file.is_some() && site.span.is_some())
            {
                let span = site.span.expect("filtered above");
                sources.insert((span.start.line, span.start.column));
            }
        }
    }
    if assurance.is_none() {
        if let Some(span) = declared_span {
            assurance = Some(ProvenanceAssurance::SourceBacked);
            sources.insert((span.start.line, span.start.column));
        }
    }
    ClaimProvenance {
        assurance: assurance.unwrap_or(ProvenanceAssurance::Unknown),
        sources: sources
            .into_iter()
            .map(|(line, column)| source_ref_lc(source_path, line, column))
            .collect(),
        origin_ids: origin_ids.into_iter().collect(),
    }
}

fn source_from_origin(
    model: &KernelModel,
    target: &str,
    source_path: Option<&str>,
) -> Option<SourceRef> {
    model
        .origins()
        .origins_for(target)
        .iter()
        .find_map(|chain| {
            chain
                .primary
                .as_ref()
                .filter(|site| site.source_file.is_some() && site.span.is_some())
                .map(|site| source_ref_span(source_path, site.span.expect("filtered above")))
        })
}

fn type_ref_json(ty: &TypeRef) -> Value {
    match ty {
        TypeRef::Int => json!({"kind": "int"}),
        TypeRef::Bool => json!({"kind": "bool"}),
        TypeRef::Named(name) => json!({"kind": "named", "name": name}),
        TypeRef::Range(lo, hi) => json!({"kind": "range", "lo": lo, "hi": hi}),
        TypeRef::Map(key, value) => {
            json!({"kind": "map", "key": type_ref_json(key), "value": type_ref_json(value)})
        }
        TypeRef::Relation(left, right) => {
            json!({"kind": "relation", "left": type_ref_json(left), "right": type_ref_json(right)})
        }
        TypeRef::Set(element) => json!({"kind": "set", "element": type_ref_json(element)}),
        TypeRef::Seq(element, bound) => {
            json!({"kind": "seq", "element": type_ref_json(element), "bound": bound})
        }
        TypeRef::Option(element) => json!({"kind": "option", "element": type_ref_json(element)}),
    }
}

fn param_json(param: &ParamDef) -> Value {
    match param {
        ParamDef::Typed { name, ty } => json!({"name": name, "type": type_ref_json(ty)}),
        ParamDef::Range { name, lo, hi } => {
            json!({"name": name, "type": {"kind": "range", "lo": lo, "hi": hi}})
        }
    }
}

/// A claim plus the digest preimage ("core") it was hashed from, kept around
/// so the claim-set digest can reuse the same cores without rebuilding them.
struct BuiltClaim {
    claim: Claim,
    core: Value,
    target: String,
}

fn push_action_claim(
    action: &ActionDef,
    model: &KernelModel,
    dialect: DocumentDialect,
    source_path: Option<&str>,
    agg: &mut BTreeMap<String, RequirementAgg>,
    built: &mut Vec<BuiltClaim>,
) {
    let _ = dialect;
    let target = action_target(&action.name);
    let claim_id = format!("{target}#operation");
    let links = model.requirements_for(&target);
    let kind_ids = kind_ids_from(model.annotations_for(&target));
    let requirement_ids = record_links(&links, &kind_ids, &claim_id, source_path, agg);

    let parameters: Vec<Value> = action.params.iter().map(param_json).collect();
    let expressions: Vec<Value> = action
        .guards
        .iter()
        .map(|guard| match guard {
            ActionGuard::Requires(expr) => {
                json!({"role": "requires", "name": null, "ast": normalize(expr.python_ast())})
            }
            ActionGuard::Let(name, expr) => {
                json!({"role": "let", "name": name, "ast": normalize(expr.python_ast())})
            }
        })
        .collect();
    let statements: Vec<Value> = action
        .statements
        .iter()
        .map(|statement| normalize(statement.python_ast()))
        .collect();
    let postconditions: Vec<Value> = action
        .ensures
        .iter()
        .map(|expr| normalize(expr.python_ast()))
        .collect();
    let fairness = if action.fair { "weak" } else { "none" };

    let subject_core = json!({"action": action.name, "parameters": parameters});
    let enablement = json!({"mode": "all", "expressions": expressions});
    let effects = json!({"commit": "simultaneous", "reads": "pre_state", "statements": statements});
    let postconditions_value = Value::Array(postconditions);

    let core = json!({
        "id": claim_id,
        "kind": ClaimKind::Operation.as_str(),
        "requirements": requirement_ids,
        "subject": subject_core,
        "enablement": &enablement,
        "effects": &effects,
        "postconditions": &postconditions_value,
        "fairness": fairness,
        "semantic_targets": [&target],
    });
    let claim_digest = framed_digest(CLAIM_DIGEST_ALGORITHM, &core);
    let provenance = provenance_for(
        model,
        std::slice::from_ref(&target),
        Some(action.span),
        source_path,
    );

    built.push(BuiltClaim {
        claim: Claim {
            id: claim_id,
            kind: ClaimKind::Operation,
            requirements: requirement_ids,
            subject: json!({
                "action": action.name,
                "display_name": display_name(&action.name),
                "parameters": parameters,
            }),
            enablement: Some(enablement),
            effects: Some(effects),
            postconditions: Some(postconditions_value),
            condition: None,
            progress: None,
            trace: None,
            fairness: Some(fairness.to_owned()),
            semantic_targets: vec![target.clone()],
            source: Some(source_ref_span(source_path, action.span)),
            provenance,
            claim_digest,
        },
        core,
        target,
    });
}

#[allow(clippy::too_many_arguments)]
fn push_property_claim(
    kind_prefix: &str,
    name: &str,
    span: Span,
    expr_ast: Value,
    claim_kind: ClaimKind,
    model: &KernelModel,
    source_path: Option<&str>,
    agg: &mut BTreeMap<String, RequirementAgg>,
    built: &mut Vec<BuiltClaim>,
) {
    let target = property_target(kind_prefix, name);
    let claim_id = format!("{target}#{}", claim_kind.as_str());
    let links = model.requirements_for(&target);
    let kind_ids = kind_ids_from(model.annotations_for(&target));
    let requirement_ids = record_links(&links, &kind_ids, &claim_id, source_path, agg);
    let condition = normalize(expr_ast);

    let core = json!({
        "id": claim_id,
        "kind": claim_kind.as_str(),
        "requirements": requirement_ids,
        "subject": {"property": name},
        "condition": &condition,
        "semantic_targets": [&target],
    });
    let claim_digest = framed_digest(CLAIM_DIGEST_ALGORITHM, &core);
    let provenance = provenance_for(
        model,
        std::slice::from_ref(&target),
        Some(span),
        source_path,
    );

    built.push(BuiltClaim {
        claim: Claim {
            id: claim_id,
            kind: claim_kind,
            requirements: requirement_ids,
            subject: json!({"property": name, "display_name": display_name(name)}),
            enablement: None,
            effects: None,
            postconditions: None,
            condition: Some(condition),
            progress: None,
            trace: None,
            fairness: None,
            semantic_targets: vec![target.clone()],
            source: Some(source_ref_span(source_path, span)),
            provenance,
            claim_digest,
        },
        core,
        target,
    });
}

fn push_terminal_claim(
    expr: &fsl_core::KernelExpr,
    model: &KernelModel,
    source_path: Option<&str>,
    agg: &mut BTreeMap<String, RequirementAgg>,
    built: &mut Vec<BuiltClaim>,
) {
    let target = TERMINAL_TARGET.to_owned();
    let claim_id = format!("{target}#terminal_rule");
    let links = model.requirements_for(&target);
    let kind_ids = kind_ids_from(model.annotations_for(&target));
    let requirement_ids = record_links(&links, &kind_ids, &claim_id, source_path, agg);
    let condition = normalize(expr.python_ast());

    let core = json!({
        "id": claim_id,
        "kind": ClaimKind::TerminalRule.as_str(),
        "requirements": requirement_ids,
        "subject": {"property": "terminal"},
        "condition": &condition,
        "semantic_targets": [&target],
    });
    let claim_digest = framed_digest(CLAIM_DIGEST_ALGORITHM, &core);
    let provenance = provenance_for(model, std::slice::from_ref(&target), None, source_path);
    let source = source_from_origin(model, &target, source_path);

    built.push(BuiltClaim {
        claim: Claim {
            id: claim_id,
            kind: ClaimKind::TerminalRule,
            requirements: requirement_ids,
            subject: json!({"property": "terminal", "display_name": "terminal"}),
            enablement: None,
            effects: None,
            postconditions: None,
            condition: Some(condition),
            progress: None,
            trace: None,
            fairness: None,
            semantic_targets: vec![target.clone()],
            source,
            provenance,
            claim_digest,
        },
        core,
        target,
    });
}

struct BuiltTrace {
    trace_case: TraceCase,
    core: Value,
}

fn push_trace_claims(
    kind: TraceCaseKind,
    cases: &[RequirementsTraceCase],
    source_path: Option<&str>,
    agg: &mut BTreeMap<String, RequirementAgg>,
    built: &mut Vec<BuiltClaim>,
    traces: &mut Vec<BuiltTrace>,
) -> Result<(), String> {
    let (prefix, claim_kind) = match kind {
        TraceCaseKind::Acceptance => ("acceptance", ClaimKind::AcceptanceTrace),
        TraceCaseKind::Forbidden => ("forbidden", ClaimKind::ForbiddenTrace),
    };
    for case in cases {
        let target = format!("{prefix}:{}", case.id);
        let claim_id = format!("{target}#{}", claim_kind.as_str());
        let links = case
            .annotations
            .requirements()
            .map_err(|error| error.to_string())?;
        let kind_ids = kind_ids_from(&case.annotations);
        let requirement_ids = record_links(&links, &kind_ids, &claim_id, source_path, agg);

        let steps_full: Vec<Value> = case
            .steps
            .iter()
            .map(|step| {
                json!({
                    "action": step.name,
                    "args": step.args.iter().map(|arg| normalize(arg.python_ast())).collect::<Vec<_>>(),
                    "source": source_ref_lc(source_path, step.line, step.column),
                })
            })
            .collect();
        let steps_core: Vec<Value> = case
            .steps
            .iter()
            .map(|step| {
                json!({
                    "action": step.name,
                    "args": step.args.iter().map(|arg| normalize(arg.python_ast())).collect::<Vec<_>>(),
                })
            })
            .collect();
        let expectation = case
            .expectation
            .as_ref()
            .map(|expectation| match expectation {
                RequirementsTraceExpectation::Expr(expr) => {
                    json!({"kind": "expr", "ast": normalize(expr.python_ast())})
                }
                RequirementsTraceExpectation::Stage {
                    entity,
                    instance,
                    stage,
                } => {
                    json!({"kind": "stage", "entity": entity, "instance": instance, "stage": stage})
                }
            });

        let trace_core = json!({"id": case.id, "steps": &steps_core, "expectation": &expectation});
        let claim_core = json!({
            "id": claim_id,
            "kind": claim_kind.as_str(),
            "requirements": requirement_ids,
            "subject": {"trace_case": case.id},
            "trace": &trace_core,
            "semantic_targets": [&target],
        });
        let claim_digest = framed_digest(CLAIM_DIGEST_ALGORITHM, &claim_core);
        let provenance = ClaimProvenance {
            assurance: ProvenanceAssurance::SourceBacked,
            sources: vec![source_ref_lc(source_path, case.line, case.column)],
            origin_ids: Vec::new(),
        };

        built.push(BuiltClaim {
            claim: Claim {
                id: claim_id.clone(),
                kind: claim_kind,
                requirements: requirement_ids.clone(),
                subject: json!({"trace_case": case.id, "display_name": case.id}),
                enablement: None,
                effects: None,
                postconditions: None,
                condition: None,
                progress: None,
                trace: Some(json!({"trace_case": case.id})),
                fairness: None,
                semantic_targets: vec![target.clone()],
                source: Some(source_ref_lc(source_path, case.line, case.column)),
                provenance,
                claim_digest,
            },
            core: claim_core,
            target: target.clone(),
        });

        traces.push(BuiltTrace {
            trace_case: TraceCase {
                id: case.id.clone(),
                kind,
                text: case.text.clone(),
                steps: steps_full,
                expectation,
                source: source_ref_lc(source_path, case.line, case.column),
                requirements: requirement_ids,
            },
            core: trace_core,
        });
    }
    Ok(())
}

/// Compile a checked model into the RCIR v1 contract.
///
/// # Errors
///
/// Returns an error string when an authored semantic target cannot be
/// classified into `rendered` / `unattributed` / `unsupported` (a coverage
/// invariant violation), when trace-case annotations fail validation, or when
/// the spec digest cannot be computed.
#[allow(clippy::too_many_lines)]
pub fn project_requirement_claims(
    input: &DocumentInput<'_>,
) -> Result<RequirementClaimSet, String> {
    let model = input.model;
    let source_path = input.source_path;
    let mut universe: Vec<String> = Vec::new();
    let mut unsupported: Vec<UnsupportedEntry> = Vec::new();
    let mut agg: BTreeMap<String, RequirementAgg> = BTreeMap::new();
    let mut built: Vec<BuiltClaim> = Vec::new();
    let mut traces: Vec<BuiltTrace> = Vec::new();

    if !model.init.is_empty() {
        universe.push(INIT_TARGET.to_owned());
        unsupported.push(UnsupportedEntry {
            target: INIT_TARGET.to_owned(),
            reason: "no v1 claim kind projects initial-state definitions".to_owned(),
        });
    }

    for action in &model.actions {
        universe.push(action_target(&action.name));
        push_action_claim(
            action,
            model,
            input.dialect,
            source_path,
            &mut agg,
            &mut built,
        );
    }

    for property in &model.invariants {
        universe.push(property_target("invariant", &property.name));
        let claim_kind = if matches!(input.dialect, DocumentDialect::Requirements)
            && property.name.starts_with("_deadline_")
        {
            ClaimKind::DeadlineRule
        } else {
            ClaimKind::StateRule
        };
        push_property_claim(
            "invariant",
            &property.name,
            property.span,
            property.expr.python_ast(),
            claim_kind,
            model,
            source_path,
            &mut agg,
            &mut built,
        );
    }
    for property in &model.transitions {
        universe.push(property_target("trans", &property.name));
        push_property_claim(
            "trans",
            &property.name,
            property.span,
            property.expr.python_ast(),
            ClaimKind::TransitionRule,
            model,
            source_path,
            &mut agg,
            &mut built,
        );
    }
    for property in &model.reachables {
        universe.push(property_target("reachable", &property.name));
        push_property_claim(
            "reachable",
            &property.name,
            property.span,
            property.expr.python_ast(),
            ClaimKind::ReachabilityGoal,
            model,
            source_path,
            &mut agg,
            &mut built,
        );
    }

    for property in &model.leadstos {
        let target = property_target("leadsTo", &property.name);
        universe.push(target.clone());
        let claim_id = format!("{target}#progress_rule");
        let links = model.requirements_for(&target);
        let kind_ids = kind_ids_from(model.annotations_for(&target));
        let requirement_ids = record_links(&links, &kind_ids, &claim_id, source_path, &mut agg);

        let binders: Vec<Value> = property
            .binders
            .iter()
            .map(|binder| normalize(binder.python_ast()))
            .collect();
        let before = normalize(property.before.python_ast());
        let after = normalize(property.after.python_ast());
        let decreases = property
            .decreases
            .as_ref()
            .map(|expr| normalize(expr.python_ast()));
        let progress = json!({
            "binders": binders,
            "before": &before,
            "after": &after,
            "within": property.within,
            "decreases": &decreases,
        });

        let core = json!({
            "id": claim_id,
            "kind": ClaimKind::ProgressRule.as_str(),
            "requirements": requirement_ids,
            "subject": {"property": property.name},
            "progress": &progress,
            "semantic_targets": [&target],
        });
        let claim_digest = framed_digest(CLAIM_DIGEST_ALGORITHM, &core);
        let provenance = provenance_for(
            model,
            std::slice::from_ref(&target),
            Some(property.span),
            source_path,
        );

        built.push(BuiltClaim {
            claim: Claim {
                id: claim_id,
                kind: ClaimKind::ProgressRule,
                requirements: requirement_ids,
                subject: json!({"property": property.name, "display_name": display_name(&property.name)}),
                enablement: None,
                effects: None,
                postconditions: None,
                condition: None,
                progress: Some(progress),
                trace: None,
                fairness: None,
                semantic_targets: vec![target.clone()],
                source: Some(source_ref_span(source_path, property.span)),
                provenance,
                claim_digest,
            },
            core,
            target,
        });
    }

    if let Some(expr) = &model.terminal {
        universe.push(TERMINAL_TARGET.to_owned());
        push_terminal_claim(expr, model, source_path, &mut agg, &mut built);
    }

    let trace_contract: Option<RequirementsTraceContract> =
        requirements_trace_contract(input.source).map_err(|error| error.to_string())?;
    if let Some(contract) = &trace_contract {
        for case in &contract.acceptance {
            universe.push(format!("acceptance:{}", case.id));
        }
        for case in &contract.forbidden {
            universe.push(format!("forbidden:{}", case.id));
        }
        push_trace_claims(
            TraceCaseKind::Acceptance,
            &contract.acceptance,
            source_path,
            &mut agg,
            &mut built,
            &mut traces,
        )?;
        push_trace_claims(
            TraceCaseKind::Forbidden,
            &contract.forbidden,
            source_path,
            &mut agg,
            &mut built,
            &mut traces,
        )?;
    }

    for projection in &model.projections {
        let target = format!("projection:{}", projection.name);
        universe.push(target.clone());
        unsupported.push(UnsupportedEntry {
            target,
            reason: "KPI projection is a business-layer artifact; not projected in RCIR v1"
                .to_owned(),
        });
    }

    for name in &input.implements_names {
        let target = format!("refinement:{name}");
        universe.push(target.clone());
        unsupported.push(UnsupportedEntry {
            target,
            reason: "the implements correspondence belongs to the refinement contract, not \
                     the requirement claim projection, in v1"
                .to_owned(),
        });
    }

    // Coverage: partition the authored universe into rendered / unattributed
    // / unsupported. Fail closed rather than silently dropping a target.
    let mut rendered: BTreeSet<String> = BTreeSet::new();
    let mut unattributed: BTreeSet<String> = BTreeSet::new();
    for item in &built {
        if item.claim.requirements.is_empty() {
            unattributed.insert(item.target.clone());
        } else {
            rendered.insert(item.target.clone());
        }
    }
    let unsupported_targets: BTreeSet<String> = unsupported
        .iter()
        .map(|entry| entry.target.clone())
        .collect();
    let universe_set: BTreeSet<String> = universe.iter().cloned().collect();
    let classified: BTreeSet<String> = rendered
        .iter()
        .chain(unattributed.iter())
        .chain(unsupported_targets.iter())
        .cloned()
        .collect();
    let overlap = rendered.intersection(&unattributed).next().is_some()
        || rendered.intersection(&unsupported_targets).next().is_some()
        || unattributed
            .intersection(&unsupported_targets)
            .next()
            .is_some();
    if overlap || classified != universe_set {
        return Err(
            "coverage classification did not exactly partition the authored semantic target \
             universe (fail-closed)"
                .to_owned(),
        );
    }

    // Analysis scope was already extracted from the surface `requirements`
    // tree by the caller (see `requirements_analysis_scope`): `entity`/
    // `number` + `verify { ... }` bounds do not survive into the lowered
    // `KernelSpec`, so there is nothing left to scan here.
    let analysis_scope = input.analysis_scope.clone();

    // Undecided metadata (reused from issue #189, never a claim).
    let undecided: Vec<crate::document::UndecidedItem> = crate::undecided::undecided_records(model)
        .into_iter()
        .map(|record| crate::document::UndecidedItem {
            target: record.node,
            declaration: record.declaration,
            reason: record.reason,
            requirement_ids: record.requirement_ids,
            source: Some(source_ref_span(source_path, record.span)),
        })
        .collect();

    // Requirements section, aggregated across every target/trace case above.
    let requirements: Vec<Requirement> = agg
        .into_iter()
        .map(|(id, entry)| Requirement {
            id,
            statements: entry
                .statements
                .into_iter()
                .map(|(text, source)| RequirementStatement {
                    text,
                    source: Some(source),
                })
                .collect(),
            claim_ids: entry.claim_ids.into_iter().collect(),
            kinds: entry.kinds.into_iter().collect(),
        })
        .collect();

    // Sort claims/trace_cases by id for determinism, keeping cores aligned.
    built.sort_by(|left, right| left.claim.id.cmp(&right.claim.id));
    traces.sort_by(|left, right| left.trace_case.id.cmp(&right.trace_case.id));

    let claim_cores: Vec<&Value> = built.iter().map(|item| &item.core).collect();
    let trace_cores: Vec<&Value> = traces.iter().map(|item| &item.core).collect();

    let mut requirement_cores: Vec<Value> = requirements
        .iter()
        .map(|requirement| {
            json!({
                "id": requirement.id,
                "kinds": requirement.kinds,
                "statements": requirement
                    .statements
                    .iter()
                    .map(|statement| statement.text.clone())
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    requirement_cores.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));

    let mut unsupported_sorted = unsupported.clone();
    unsupported_sorted.sort_by(|left, right| left.target.cmp(&right.target));
    let unsupported_core: Vec<Value> = unsupported_sorted
        .iter()
        .map(|entry| json!({"target": entry.target, "reason": entry.reason}))
        .collect();
    let mut undecided_core: Vec<Value> = undecided
        .iter()
        .map(|item| {
            json!({
                "target": item.target,
                "reason": item.reason,
                "requirement_ids": item.requirement_ids,
            })
        })
        .collect();
    undecided_core.sort_by(|left, right| {
        (left["target"].as_str(), left["reason"].as_str())
            .cmp(&(right["target"].as_str(), right["reason"].as_str()))
    });

    let claim_set_core = json!({
        "analysis_scope": &analysis_scope,
        "claims": claim_cores,
        "requirements": requirement_cores,
        "semantics": SemanticsInfo::default(),
        "spec": {"dialect": input.dialect.as_str(), "name": model.name},
        "trace_cases": trace_cores,
        "undecided": undecided_core,
        "unsupported": unsupported_core,
    });
    let claim_set_digest = framed_digest(CLAIM_SET_DIGEST_ALGORITHM, &claim_set_core);
    let spec_digest = spec_digest_from_kernel(input.kernel)?;

    let mut assurance_counts = AssuranceCounts::default();
    for item in &built {
        match item.claim.provenance.assurance {
            ProvenanceAssurance::SourceBacked => assurance_counts.source_backed += 1,
            ProvenanceAssurance::GeneratedFromSource => assurance_counts.generated_from_source += 1,
            ProvenanceAssurance::GeneratedOnly => assurance_counts.generated_only += 1,
            ProvenanceAssurance::Unknown => assurance_counts.unknown += 1,
        }
    }
    let completeness = if built.is_empty() || assurance_counts.unknown == built.len() {
        Completeness::Unknown
    } else if assurance_counts.unknown == 0 {
        Completeness::Complete
    } else {
        Completeness::Partial
    };

    let mut authored_sorted: Vec<String> = universe_set.into_iter().collect();
    authored_sorted.sort();
    let mut rendered_sorted: Vec<String> = rendered.into_iter().collect();
    rendered_sorted.sort();
    let mut unattributed_sorted: Vec<String> = unattributed.into_iter().collect();
    unattributed_sorted.sort();

    let coverage = Coverage {
        counts: CoverageCounts {
            authored: authored_sorted.len(),
            rendered: rendered_sorted.len(),
            unattributed: unattributed_sorted.len(),
            unsupported: unsupported_sorted.len(),
        },
        authored: authored_sorted,
        rendered: rendered_sorted,
        unattributed: unattributed_sorted,
        unsupported: unsupported_sorted,
    };

    Ok(RequirementClaimSet {
        schema: RCIR_SCHEMA_ID.to_owned(),
        schema_version: RCIR_SCHEMA_VERSION.to_owned(),
        result: "requirement_claims".to_owned(),
        spec: SpecInfo {
            name: model.name.clone(),
            dialect: input.dialect.as_str().to_owned(),
            source: source_path.map(str::to_owned),
            spec_digest,
            spec_digest_algorithm: SPEC_DIGEST_ALGORITHM.to_owned(),
            claim_set_digest,
            claim_set_digest_algorithm: CLAIM_SET_DIGEST_ALGORITHM.to_owned(),
            claim_digest_algorithm: CLAIM_DIGEST_ALGORITHM.to_owned(),
        },
        semantics: SemanticsInfo::default(),
        requirements,
        claims: built.into_iter().map(|item| item.claim).collect(),
        trace_cases: traces.into_iter().map(|item| item.trace_case).collect(),
        undecided,
        analysis_scope,
        coverage,
        provenance: ProvenanceSummary {
            completeness,
            identity_stability: "exact_source_revision".to_owned(),
            counts: assurance_counts,
        },
    })
}
