// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Deterministic JSON/Mermaid/DOT projections and `causal diff` over a typed
//! [`CausalModel`] (issue #321). All outputs carry `formal_result: "not_run"`
//! and a `do_not_assume` array; representative path listings are capped with
//! explicit truncation metadata and never drive any judgment.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use crate::causal::{
    CausalModel, Claim, ClaimStatus, Lag, Persistence, Polarity, VariableRole, active_adjacency,
    reachable_from,
};
use crate::causal_analysis::{
    REPRESENTATIVE_PATH_CAP, earliest_from, feedback_loop_classes, indicator_classes,
    latest_bound_from,
};

const PROJECTION_DO_NOT_ASSUME: [&str; 3] = [
    "The causal claims are true",
    "Path existence establishes real-world causality",
    "Timeline bounds are guarantees about the real world",
];

fn interval_json(min: u64, max: u64) -> Value {
    json!({"min": min, "max": max})
}

fn lag_json(lag: Lag) -> Value {
    match lag {
        Lag::Known(interval) => interval_json(interval.min, interval.max),
        Lag::Unknown => json!("unknown"),
    }
}

fn persistence_json(persistence: Persistence) -> Value {
    match persistence {
        Persistence::Known(interval) => interval_json(interval.min, interval.max),
        Persistence::Unknown => json!("unknown"),
        Persistence::Unbounded => json!("unbounded"),
    }
}

fn projection_envelope(model: &CausalModel, projection: &str) -> Map<String, Value> {
    let mut envelope = Map::new();
    envelope.insert("result".to_owned(), json!("causal_analyzed"));
    envelope.insert("schema_version".to_owned(), json!("causal-graph.v0"));
    envelope.insert("formal_result".to_owned(), json!("not_run"));
    envelope.insert("model".to_owned(), json!(model.name));
    envelope.insert("projection".to_owned(), json!(projection));
    envelope.insert("timebase".to_owned(), json!(model.timebase));
    envelope.insert("horizon".to_owned(), json!(model.horizon));
    envelope.insert("do_not_assume".to_owned(), json!(PROJECTION_DO_NOT_ASSUME));
    envelope
}

fn variable_nodes(model: &CausalModel) -> Vec<Value> {
    let indicators = indicator_classes(model);
    model
        .variables
        .values()
        .map(|variable| {
            let mut node = Map::new();
            node.insert("id".to_owned(), json!(format!("variable:{}", variable.id)));
            node.insert("kind".to_owned(), json!("variable"));
            node.insert("role".to_owned(), json!(variable.role.as_str()));
            if variable.latent {
                node.insert("latent".to_owned(), json!(true));
            }
            if let Some(class) = indicators.get(&variable.id) {
                node.insert("indicator_class".to_owned(), json!(class));
            }
            Value::Object(node)
        })
        .collect()
}

fn claim_edges(model: &CausalModel) -> Vec<Value> {
    model
        .claims
        .values()
        .map(|claim| {
            json!({
                "id": format!("claim:{}", claim.id),
                "kind": "claim",
                "source": format!("variable:{}", claim.source),
                "target": format!("variable:{}", claim.target),
                "polarity": claim.polarity.as_str(),
                "lag": lag_json(claim.lag),
                "persists": persistence_json(claim.persists),
                "version": claim.version,
                "status": match claim.status {
                    ClaimStatus::Active => "active",
                    ClaimStatus::Retired => "retired",
                },
                "basis": claim.basis,
            })
        })
        .collect()
}

/// Representative simple paths from each intervention to each outcome over
/// active claims, capped at [`REPRESENTATIVE_PATH_CAP`] with truncation
/// metadata. Deterministic: claims are explored in sorted order.
fn representative_paths(model: &CausalModel) -> (Vec<Value>, usize) {
    let adjacency = active_adjacency(model);
    let interventions: Vec<&str> = model
        .variables
        .values()
        .filter(|variable| variable.role == VariableRole::Intervention)
        .map(|variable| variable.id.as_str())
        .collect();
    let outcomes: BTreeSet<&str> = model
        .variables
        .values()
        .filter(|variable| variable.role == VariableRole::Outcome)
        .map(|variable| variable.id.as_str())
        .collect();
    let mut paths = Vec::new();
    let mut truncated = 0usize;
    for start in interventions {
        let mut stack: Vec<(String, Vec<&Claim>, BTreeSet<String>)> = vec![(
            start.to_owned(),
            Vec::new(),
            BTreeSet::from([start.to_owned()]),
        )];
        while let Some((node, path, visited)) = stack.pop() {
            if outcomes.contains(node.as_str()) && !path.is_empty() {
                if paths.len() < REPRESENTATIVE_PATH_CAP {
                    paths.push(path_json(model, start, &node, &path));
                } else {
                    truncated += 1;
                }
                // An outcome may still lead onward; keep exploring below.
            }
            // Explore in reverse sorted order so the stack pops sorted-first.
            let mut successors: Vec<&Claim> =
                adjacency.get(node.as_str()).cloned().unwrap_or_default();
            successors.sort_by(|left, right| right.id.cmp(&left.id));
            for claim in successors {
                if !visited.contains(&claim.target) {
                    let mut next_path = path.clone();
                    next_path.push(claim);
                    let mut next_visited = visited.clone();
                    next_visited.insert(claim.target.clone());
                    stack.push((claim.target.clone(), next_path, next_visited));
                }
            }
        }
    }
    (paths, truncated)
}

fn path_json(model: &CausalModel, start: &str, end: &str, path: &[&Claim]) -> Value {
    let polarity = path
        .iter()
        .map(|claim| claim.polarity)
        .fold(Polarity::Positive, Polarity::product);
    let first_response = path.iter().try_fold((0u64, 0u64), |(min, max), claim| {
        if let Lag::Known(interval) = claim.lag {
            Some((min + interval.min, max.saturating_add(interval.max)))
        } else {
            None
        }
    });
    let _ = model;
    json!({
        "source": format!("variable:{start}"),
        "target": format!("variable:{end}"),
        "claims": path.iter().map(|claim| format!("claim:{}", claim.id)).collect::<Vec<_>>(),
        "polarity": polarity.as_str(),
        "first_response": first_response
            .map_or(json!("unknown"), |(min, max)| interval_json(min, max)),
    })
}

fn feedback_json(model: &CausalModel) -> Vec<Value> {
    let classes = feedback_loop_classes(model);
    model
        .feedbacks
        .values()
        .map(|feedback| {
            let (class, lag) = &classes[&feedback.id];
            let repetitions = lag.and_then(|interval| {
                (interval.min > 0).then(|| model.horizon / interval.min)
            });
            json!({
                "id": format!("feedback:{}", feedback.id),
                "loop_class": class,
                "claims": feedback.claims.iter().map(|claim| format!("claim:{claim}")).collect::<Vec<_>>(),
                "cycle_lag": lag.map_or(json!("unknown"), |interval| interval_json(interval.min, interval.max)),
                "max_repetitions_within_horizon": repetitions.map_or(json!("unknown"), |value| json!(value)),
                "recurrent": true,
            })
        })
        .collect()
}

/// `--projection causal_graph`.
#[must_use]
pub fn causal_graph_projection(model: &CausalModel) -> Value {
    let mut envelope = projection_envelope(model, "causal_graph");
    envelope.insert("nodes".to_owned(), Value::Array(variable_nodes(model)));
    envelope.insert("edges".to_owned(), Value::Array(claim_edges(model)));
    let (paths, truncated) = representative_paths(model);
    envelope.insert("paths".to_owned(), Value::Array(paths));
    envelope.insert("feedbacks".to_owned(), Value::Array(feedback_json(model)));
    envelope.insert(
        "truncation".to_owned(),
        json!({"paths_cap": REPRESENTATIVE_PATH_CAP, "paths_truncated": truncated}),
    );
    Value::Object(envelope)
}

/// `--projection causal_timeline`: first-pass windows per intervention →
/// outcome. `min` is exact (shortest known-lag path); `max` is an upper
/// bound when the pair is connected through a feedback SCC.
#[must_use]
pub fn causal_timeline_projection(model: &CausalModel) -> Value {
    let mut envelope = projection_envelope(model, "causal_timeline");
    let mut timelines = Vec::new();
    let adjacency = active_adjacency(model);
    for intervention in model
        .variables
        .values()
        .filter(|variable| variable.role == VariableRole::Intervention)
    {
        let earliest = earliest_from(model, &intervention.id);
        let latest = latest_bound_from(model, &intervention.id);
        let reachable = reachable_from(&adjacency, &intervention.id);
        for outcome in model
            .variables
            .values()
            .filter(|variable| variable.role == VariableRole::Outcome)
        {
            if outcome.id == intervention.id || !reachable.contains(&outcome.id) {
                continue;
            }
            let first_pass = match (earliest.get(&outcome.id), latest.get(&outcome.id)) {
                (Some(&min), Some(&(max, _))) => interval_json(min, max),
                _ => json!("unknown"),
            };
            let via_feedback = latest
                .get(&outcome.id)
                .is_some_and(|&(_, feedback)| feedback);
            timelines.push(json!({
                "intervention": format!("variable:{}", intervention.id),
                "outcome": format!("variable:{}", outcome.id),
                "first_pass": first_pass,
                "via_feedback": via_feedback,
            }));
        }
    }
    envelope.insert("nodes".to_owned(), Value::Array(variable_nodes(model)));
    envelope.insert("edges".to_owned(), Value::Array(claim_edges(model)));
    envelope.insert("timelines".to_owned(), Value::Array(timelines));
    Value::Object(envelope)
}

/// `--projection causal_traceability_graph`: bridge to actions, KPIs,
/// states, properties, and requirement IDs.
#[must_use]
pub fn causal_traceability_projection(model: &CausalModel) -> Value {
    let mut envelope = projection_envelope(model, "causal_traceability_graph");
    let mut nodes = variable_nodes(model);
    let mut edges = claim_edges(model);
    let mut bridge_nodes: BTreeSet<(String, String)> = BTreeSet::new();
    let push_edge = |edges: &mut Vec<Value>, kind: &str, source: String, target: String| {
        edges.push(json!({
            "id": format!("edge:{source}:{kind}:{target}"),
            "kind": kind,
            "source": source,
            "target": target,
        }));
    };
    for variable in model.variables.values() {
        let variable_node = format!("variable:{}", variable.id);
        if let Some(binding) = &variable.binds_action {
            bridge_nodes.insert((binding.node_id(), "action".to_owned()));
            push_edge(
                &mut edges,
                "binds",
                variable_node.clone(),
                binding.node_id(),
            );
        }
        if let Some(binding) = &variable.observes {
            bridge_nodes.insert((binding.node_id(), binding.kind.as_str().to_owned()));
            push_edge(
                &mut edges,
                "observes",
                variable_node.clone(),
                binding.node_id(),
            );
        }
        if let Some(binding) = &variable.proxy {
            bridge_nodes.insert((binding.node_id(), binding.kind.as_str().to_owned()));
            push_edge(
                &mut edges,
                "proxy",
                variable_node.clone(),
                binding.node_id(),
            );
        }
        for requirement in &variable.covers {
            let node = format!("requirement:{requirement}");
            bridge_nodes.insert((node.clone(), "requirement".to_owned()));
            push_edge(&mut edges, "covers", variable_node.clone(), node);
        }
    }
    for claim in model.claims.values() {
        for requirement in &claim.covers {
            let node = format!("requirement:{requirement}");
            bridge_nodes.insert((node.clone(), "requirement".to_owned()));
            push_edge(&mut edges, "covers", format!("claim:{}", claim.id), node);
        }
        for evidence in &claim.evidence {
            let node = format!("evidence:{evidence}");
            bridge_nodes.insert((node.clone(), "evidence".to_owned()));
            push_edge(
                &mut edges,
                "evidence_ref",
                format!("claim:{}", claim.id),
                node,
            );
        }
    }
    for (id, kind) in bridge_nodes {
        nodes.push(json!({"id": id, "kind": kind}));
    }
    envelope.insert("nodes".to_owned(), Value::Array(nodes));
    envelope.insert("edges".to_owned(), Value::Array(edges));
    Value::Object(envelope)
}

/// `--profile causal-review` envelope (`causal-findings.v0`).
#[must_use]
pub fn causal_review_json(model: &CausalModel) -> Value {
    let (findings, not_evaluable) = crate::causal_analysis::causal_review_findings(model);
    json!({
        "result": "causal_analyzed",
        "schema_version": "causal-findings.v0",
        "formal_result": "not_run",
        "model": model.name,
        "profile": "causal-review",
        "findings": findings,
        "not_evaluable": not_evaluable,
        "do_not_assume": MODEL_REVIEW_DO_NOT_ASSUME,
    })
}

const MODEL_REVIEW_DO_NOT_ASSUME: [&str; 4] = [
    "The causal claims are true",
    "Findings are formal violations",
    "Absence of findings proves the model is causally sound",
    "High confidence means high causal validity",
];

/// Render a graph projection as Mermaid (deterministic).
#[must_use]
pub fn causal_mermaid(projection: &Value) -> String {
    let mut lines = vec!["graph LR".to_owned()];
    for node in projection["nodes"].as_array().into_iter().flatten() {
        let id = sanitize(node["id"].as_str().unwrap_or_default());
        let label = node["id"].as_str().unwrap_or_default();
        let role = node["role"].as_str().unwrap_or("");
        if role.is_empty() {
            lines.push(format!("  {id}[\"{label}\"]"));
        } else {
            lines.push(format!("  {id}[\"{label} ({role})\"]"));
        }
    }
    for edge in projection["edges"].as_array().into_iter().flatten() {
        let source = sanitize(edge["source"].as_str().unwrap_or_default());
        let target = sanitize(edge["target"].as_str().unwrap_or_default());
        let kind = edge["kind"].as_str().unwrap_or_default();
        let label = if kind == "claim" {
            format!(
                "{} {}",
                edge["id"].as_str().unwrap_or_default(),
                edge["polarity"].as_str().unwrap_or_default()
            )
        } else {
            kind.to_owned()
        };
        lines.push(format!("  {source} -->|\"{label}\"| {target}"));
    }
    lines.join("\n") + "\n"
}

/// Render a graph projection as DOT (deterministic).
#[must_use]
pub fn causal_dot(projection: &Value) -> String {
    let mut lines = vec!["digraph causal {".to_owned()];
    for node in projection["nodes"].as_array().into_iter().flatten() {
        let id = node["id"].as_str().unwrap_or_default();
        let role = node["role"].as_str().unwrap_or("");
        if role.is_empty() {
            lines.push(format!("  \"{id}\";"));
        } else {
            lines.push(format!("  \"{id}\" [label=\"{id}\\n({role})\"];"));
        }
    }
    for edge in projection["edges"].as_array().into_iter().flatten() {
        let source = edge["source"].as_str().unwrap_or_default();
        let target = edge["target"].as_str().unwrap_or_default();
        let kind = edge["kind"].as_str().unwrap_or_default();
        let label = if kind == "claim" {
            edge["id"].as_str().unwrap_or_default().to_owned()
        } else {
            kind.to_owned()
        };
        lines.push(format!(
            "  \"{source}\" -> \"{target}\" [label=\"{label}\"];"
        ));
    }
    lines.push("}".to_owned());
    lines.join("\n") + "\n"
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// `fslc causal diff` (`causal-diff.v0`): claim identity by ID, content by
/// version and typed fields. Support transitions are `not_available` until
/// evidence inputs exist (#322).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn causal_diff_json(before: &CausalModel, after: &CausalModel) -> Value {
    let mut changes: Vec<Value> = Vec::new();
    let mut change = |kind: &str, id: String, extra: Map<String, Value>| {
        let mut object = Map::new();
        object.insert("kind".to_owned(), json!(kind));
        object.insert("id".to_owned(), json!(id));
        object.extend(extra);
        object.insert("support_transition".to_owned(), json!("not_available"));
        changes.push(Value::Object(object));
    };
    if before.timebase != after.timebase {
        change("timebase_changed", "model".to_owned(), Map::new());
    }
    if before.horizon != after.horizon {
        change("horizon_changed", "model".to_owned(), Map::new());
    }
    for (id, claim) in &after.claims {
        match before.claims.get(id) {
            None => {
                change("claim_added", format!("claim:{id}"), Map::new());
                let retired_claims: Vec<String> = before
                    .claims
                    .values()
                    .filter(|previous| {
                        previous.status == ClaimStatus::Retired
                            && previous.source == claim.source
                            && previous.target == claim.target
                            && previous.polarity == claim.polarity
                    })
                    .map(|previous| format!("claim:{}", previous.id))
                    .collect();
                if !retired_claims.is_empty() {
                    let mut extra = Map::new();
                    extra.insert("retired_claims".to_owned(), json!(retired_claims));
                    change(
                        "retired_hypothesis_reproposed",
                        format!("claim:{id}"),
                        extra,
                    );
                }
            }
            Some(previous) => {
                let mut fields = Vec::new();
                if previous.source != claim.source {
                    fields.push("source");
                }
                if previous.target != claim.target {
                    fields.push("target");
                }
                if previous.polarity != claim.polarity {
                    fields.push("polarity");
                }
                if previous.lag != claim.lag {
                    fields.push("lag");
                }
                if previous.persists != claim.persists {
                    fields.push("persists");
                }
                if previous.basis != claim.basis {
                    fields.push("basis");
                }
                if previous.scope != claim.scope {
                    fields.push("scope");
                }
                if !fields.is_empty() || previous.version != claim.version {
                    let mut extra = Map::new();
                    extra.insert("before_version".to_owned(), json!(previous.version));
                    extra.insert("after_version".to_owned(), json!(claim.version));
                    extra.insert("fields".to_owned(), json!(fields));
                    let kind = if !fields.is_empty() && previous.version == claim.version {
                        "content_changed_without_version_bump"
                    } else {
                        "claim_content_changed"
                    };
                    change(kind, format!("claim:{id}"), extra);
                }
                if previous.status != claim.status || previous.superseded_by != claim.superseded_by
                {
                    let mut extra = Map::new();
                    extra.insert(
                        "before_status".to_owned(),
                        json!(status_str(previous.status)),
                    );
                    extra.insert("after_status".to_owned(), json!(status_str(claim.status)));
                    let kind = if previous.status == ClaimStatus::Retired
                        && claim.status == ClaimStatus::Active
                    {
                        "retired_claim_reactivated"
                    } else {
                        "claim_lifecycle_changed"
                    };
                    change(kind, format!("claim:{id}"), extra);
                }
                if previous.evidence != claim.evidence {
                    let added: Vec<&String> = claim
                        .evidence
                        .iter()
                        .filter(|id| !previous.evidence.contains(id))
                        .collect();
                    let removed: Vec<&String> = previous
                        .evidence
                        .iter()
                        .filter(|id| !claim.evidence.contains(id))
                        .collect();
                    for evidence in added {
                        change(
                            "evidence_ref_added",
                            format!("claim:{id}:{evidence}"),
                            Map::new(),
                        );
                    }
                    for evidence in removed {
                        change(
                            "evidence_ref_removed",
                            format!("claim:{id}:{evidence}"),
                            Map::new(),
                        );
                    }
                }
            }
        }
    }
    for id in before.claims.keys() {
        if !after.claims.contains_key(id) {
            change("claim_removed", format!("claim:{id}"), Map::new());
        }
    }
    diff_ids(
        &mut change,
        "variable",
        &before.variables.keys().collect(),
        &after.variables.keys().collect(),
    );
    for (id, variable) in &after.variables {
        if let Some(previous) = before.variables.get(id)
            && previous != variable
        {
            change("variable_changed", format!("variable:{id}"), Map::new());
        }
    }
    diff_ids(
        &mut change,
        "feedback",
        &before.feedbacks.keys().collect(),
        &after.feedbacks.keys().collect(),
    );
    for (id, feedback) in &after.feedbacks {
        if let Some(previous) = before.feedbacks.get(id)
            && previous.claims != feedback.claims
        {
            change("feedback_changed", format!("feedback:{id}"), Map::new());
        }
    }
    if before.scope != after.scope || before.default_scope != after.default_scope {
        change("scope_vocabulary_changed", "model".to_owned(), Map::new());
    }
    if before.clocks != after.clocks {
        change("clock_changed", "model".to_owned(), Map::new());
    }
    json!({
        "result": "causal_diffed",
        "schema_version": "causal-diff.v0",
        "formal_result": "not_run",
        "before": before.name,
        "after": after.name,
        "changes": changes,
        "do_not_assume": [
            "The causal claims are true",
            "A structural diff implies a support change",
            "Removed or retired claims were false",
        ],
    })
}

fn status_str(status: ClaimStatus) -> &'static str {
    match status {
        ClaimStatus::Active => "active",
        ClaimStatus::Retired => "retired",
    }
}

fn diff_ids(
    change: &mut impl FnMut(&str, String, Map<String, Value>),
    kind: &str,
    before: &BTreeSet<&String>,
    after: &BTreeSet<&String>,
) {
    for id in after.difference(before) {
        change(&format!("{kind}_added"), format!("{kind}:{id}"), Map::new());
    }
    for id in before.difference(after) {
        change(
            &format!("{kind}_removed"),
            format!("{kind}:{id}"),
            Map::new(),
        );
    }
}
