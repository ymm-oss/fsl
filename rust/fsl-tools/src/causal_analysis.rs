// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Review-only analyses over a typed [`CausalModel`] (issue #321).
//!
//! Every finding carries `formal_status: "not_a_violation"` and a
//! `do_not_assume` array. Nothing here proves, refutes, or scores real-world
//! causality; the outputs are deterministic structural and temporal review
//! signals. Feedback SCCs are condensed — no unbounded path enumeration.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::causal::{
    CausalModel, CausalWarning, Claim, Interval, Lag, Persistence, Polarity, Variable,
    VariableRole, active_adjacency, reachable_from, strongly_connected_components,
};

/// Cap for representative path listings; never affects judgments
/// (reachability, earliest times, and cuts come from graph algorithms).
pub const REPRESENTATIVE_PATH_CAP: usize = 16;

const MODEL_DO_NOT_ASSUME: [&str; 3] = [
    "The causal claims are true",
    "The graph is causally complete",
    "Structural or temporal consistency establishes real-world causality",
];

fn finding_do_not_assume() -> Value {
    json!([
        "The claim is false",
        "The finding is a formal violation",
        "Absence of this finding proves the model is sound"
    ])
}

/// `fslc causal check` success JSON (`causal-check.v0`).
#[must_use]
pub fn causal_check_json(model: &CausalModel, warnings: &[CausalWarning]) -> Value {
    let retired = model
        .claims
        .values()
        .filter(|claim| claim.status == crate::causal::ClaimStatus::Retired)
        .count();
    json!({
        "result": "causal_model_checked",
        "schema_version": "causal-check.v0",
        "formal_result": "not_run",
        "model": model.name,
        "variables_checked": model.variables.len(),
        "claims_checked": model.claims.len(),
        "feedbacks_checked": model.feedbacks.len(),
        "evidence_refs_checked": model.evidence_refs.len(),
        "retired_claims": retired,
        "warnings": warnings.iter().map(warning_json).collect::<Vec<_>>(),
        "do_not_assume": MODEL_DO_NOT_ASSUME,
    })
}

fn warning_json(warning: &CausalWarning) -> Value {
    json!({
        "kind": warning.kind,
        "severity": "warning",
        "message": warning.message,
        "loc": {"line": warning.line, "column": warning.column},
        "involved_nodes": warning.involved_nodes,
    })
}

fn interval_json(interval: Interval) -> Value {
    json!({"min": interval.min, "max": interval.max})
}

fn persistence_json(persistence: Persistence) -> Value {
    match persistence {
        Persistence::Known(interval) => interval_json(interval),
        Persistence::Unknown => json!("unknown"),
        Persistence::Unbounded => json!("unbounded"),
    }
}

/// Earliest first response from `start` to every reachable variable over
/// active claims with known lags (Dijkstra on `lag.min`; weights are
/// non-negative, so cycles never shorten a first pass).
#[must_use]
pub fn earliest_from(model: &CausalModel, start: &str) -> BTreeMap<String, u64> {
    let adjacency = active_adjacency(model);
    let mut settled: BTreeMap<String, u64> = BTreeMap::new();
    // (distance, node) frontier; BTreeSet gives deterministic extraction.
    let mut frontier: BTreeSet<(u64, String)> = BTreeSet::from([(0, start.to_owned())]);
    while let Some((distance, node)) = frontier.pop_first() {
        if settled.contains_key(&node) {
            continue;
        }
        settled.insert(node.clone(), distance);
        for claim in adjacency.get(node.as_str()).map_or(&[][..], Vec::as_slice) {
            if let Lag::Known(interval) = claim.lag
                && !settled.contains_key(&claim.target)
            {
                frontier.insert((distance + interval.min, claim.target.clone()));
            }
        }
    }
    settled
}

/// Latest first-pass upper bound from `start` over known-lag active claims,
/// computed on the SCC condensation DAG. Traversing a nontrivial SCC
/// contributes at most the sum of its internal known `lag.max` values (an
/// upper bound for any simple traversal). Returns `(bound, via_feedback)`.
#[must_use]
pub fn latest_bound_from(model: &CausalModel, start: &str) -> BTreeMap<String, (u64, bool)> {
    let components = strongly_connected_components(model);
    let component_of: BTreeMap<&str, usize> = components
        .iter()
        .enumerate()
        .flat_map(|(index, members)| members.iter().map(move |member| (member.as_str(), index)))
        .collect();
    // Internal upper bound per SCC: sum of known lag.max of intra-SCC edges.
    let mut internal: Vec<u64> = vec![0; components.len()];
    let mut nontrivial: Vec<bool> = vec![false; components.len()];
    for claim in model.active_claims() {
        let (source, target) = (
            component_of[claim.source.as_str()],
            component_of[claim.target.as_str()],
        );
        if source == target {
            nontrivial[source] = true;
            if let Lag::Known(interval) = claim.lag {
                internal[source] = internal[source].saturating_add(interval.max);
            }
        }
    }
    // Longest path over the condensation DAG (components from Tarjan are in
    // reverse topological order, so iterate reversed for a forward order).
    let Some(&start_component) = component_of.get(start) else {
        return BTreeMap::new();
    };
    let mut bound: BTreeMap<usize, (u64, bool)> = BTreeMap::new();
    bound.insert(
        start_component,
        (internal[start_component], nontrivial[start_component]),
    );
    let order: Vec<usize> = (0..components.len()).rev().collect();
    for &component in &order {
        let Some(&(component_bound, component_feedback)) = bound.get(&component) else {
            continue;
        };
        for member in &components[component] {
            for claim in model
                .active_claims()
                .filter(|claim| &claim.source == member)
            {
                let Lag::Known(edge_lag) = claim.lag else {
                    continue;
                };
                let target_component = component_of[claim.target.as_str()];
                if target_component == component {
                    continue;
                }
                let candidate = component_bound
                    .saturating_add(edge_lag.max)
                    .saturating_add(internal[target_component]);
                let candidate_feedback = component_feedback || nontrivial[target_component];
                let entry = bound.entry(target_component).or_insert((0, false));
                if candidate > entry.0 {
                    entry.0 = candidate;
                }
                entry.1 = entry.1 || candidate_feedback;
            }
        }
    }
    let mut result = BTreeMap::new();
    for (component, (value, feedback)) in bound {
        for member in &components[component] {
            result.insert(member.clone(), (value, feedback));
        }
    }
    result
}

/// Reachability over every active claim including unknown-lag edges.
fn reachable_any(model: &CausalModel, start: &str) -> BTreeSet<String> {
    reachable_from(&active_adjacency(model), start)
}

/// Polarity sets reachable between every ordered variable pair — a fixpoint
/// over the sign-product transfer, so cycles converge on the finite lattice.
#[must_use]
pub fn polarity_reach(model: &CausalModel) -> BTreeMap<(String, String), BTreeSet<Polarity>> {
    let mut reach: BTreeMap<(String, String), BTreeSet<Polarity>> = BTreeMap::new();
    for claim in model.active_claims() {
        reach
            .entry((claim.source.clone(), claim.target.clone()))
            .or_default()
            .insert(claim.polarity);
    }
    loop {
        let mut additions: Vec<((String, String), Polarity)> = Vec::new();
        for ((source, middle), polarities) in &reach {
            for claim in model
                .active_claims()
                .filter(|claim| &claim.source == middle)
            {
                for polarity in polarities {
                    let composed = polarity.product(claim.polarity);
                    let key = (source.clone(), claim.target.clone());
                    if !reach.get(&key).is_some_and(|set| set.contains(&composed)) {
                        additions.push((key, composed));
                    }
                }
            }
        }
        if additions.is_empty() {
            return reach;
        }
        for (key, polarity) in additions {
            reach.entry(key).or_default().insert(polarity);
        }
    }
}

/// Claims whose removal disconnects `start` from `goal` (cut claims).
#[must_use]
pub fn cut_claims(model: &CausalModel, start: &str, goal: &str) -> Vec<String> {
    let mut cuts = Vec::new();
    for candidate in model.active_claims() {
        let adjacency: BTreeMap<&str, Vec<&Claim>> = {
            let mut adjacency: BTreeMap<&str, Vec<&Claim>> = BTreeMap::new();
            for claim in model.active_claims() {
                if claim.id != candidate.id {
                    adjacency
                        .entry(claim.source.as_str())
                        .or_default()
                        .push(claim);
                }
            }
            adjacency
        };
        if !reachable_from(&adjacency, start).contains(goal) {
            cuts.push(candidate.id.clone());
        }
    }
    cuts
}

/// Mediator variables whose removal disconnects `start` from `goal`.
fn cut_variables(model: &CausalModel, start: &str, goal: &str) -> Vec<String> {
    let mut cuts = Vec::new();
    for candidate in model.variables.values() {
        if candidate.id == start || candidate.id == goal {
            continue;
        }
        let mut adjacency: BTreeMap<&str, Vec<&Claim>> = BTreeMap::new();
        for claim in model.active_claims() {
            if claim.source != candidate.id && claim.target != candidate.id {
                adjacency
                    .entry(claim.source.as_str())
                    .or_default()
                    .push(claim);
            }
        }
        if !reachable_from(&adjacency, start).contains(goal) {
            cuts.push(candidate.id.clone());
        }
    }
    cuts
}

/// Count distinct routes in the condensation DAG from `start` to `goal`,
/// saturating at `cap` (enough to distinguish "one route" from "several").
fn route_count(model: &CausalModel, start: &str, goal: &str, cap: u64) -> u64 {
    let components = strongly_connected_components(model);
    let component_of: BTreeMap<&str, usize> = components
        .iter()
        .enumerate()
        .flat_map(|(index, members)| members.iter().map(move |member| (member.as_str(), index)))
        .collect();
    let (Some(&start_component), Some(&goal_component)) =
        (component_of.get(start), component_of.get(goal))
    else {
        return 0;
    };
    // Distinct condensation edges between component pairs.
    let mut edges: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for claim in model.active_claims() {
        let (source, target) = (
            component_of[claim.source.as_str()],
            component_of[claim.target.as_str()],
        );
        if source != target {
            edges.entry(source).or_default().insert(target);
        }
    }
    let mut counts: BTreeMap<usize, u64> = BTreeMap::new();
    counts.insert(start_component, 1);
    for component in (0..components.len()).rev() {
        let Some(&count) = counts.get(&component) else {
            continue;
        };
        for &target in edges.get(&component).into_iter().flatten() {
            let entry = counts.entry(target).or_insert(0);
            *entry = entry.saturating_add(count).min(cap);
        }
    }
    counts.get(&goal_component).copied().unwrap_or(0)
}

fn variables_with_role(model: &CausalModel, role: VariableRole) -> Vec<&Variable> {
    model
        .variables
        .values()
        .filter(|variable| variable.role == role)
        .collect()
}

struct Finding<'spec> {
    kind: &'spec str,
    analysis: &'spec str,
    confidence: f64,
    involved: Vec<String>,
    witness: Value,
    why: String,
    repairs: &'spec [&'spec str],
}

struct FindingBuilder {
    findings: Vec<Value>,
}

impl FindingBuilder {
    fn push(&mut self, finding: &Finding<'_>) {
        let anchor = finding.involved.first().cloned().unwrap_or_default();
        self.findings.push(json!({
            "finding_id": format!(
                "causal-finding:{}:{}",
                finding.kind,
                anchor.replace(':', "-")
            ),
            "analysis": finding.analysis,
            "finding_type": finding.kind,
            "severity": "review_required",
            "confidence": finding.confidence,
            "formal_status": "not_a_violation",
            "involved_nodes": finding.involved,
            "witness": finding.witness,
            "why_it_matters": finding.why,
            "candidate_repairs": finding.repairs.iter().map(|kind| json!({"kind": kind})).collect::<Vec<_>>(),
            "do_not_assume": finding_do_not_assume(),
        }));
    }
}

/// All `--profile causal-review` findings plus `not_evaluable` records.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn causal_review_findings(model: &CausalModel) -> (Vec<Value>, Vec<Value>) {
    let mut builder = FindingBuilder {
        findings: Vec::new(),
    };
    let mut not_evaluable: Vec<Value> = Vec::new();
    let interventions = variables_with_role(model, VariableRole::Intervention);
    let outcomes = variables_with_role(model, VariableRole::Outcome);
    let earliest: BTreeMap<&str, BTreeMap<String, u64>> = interventions
        .iter()
        .map(|intervention| {
            (
                intervention.id.as_str(),
                earliest_from(model, &intervention.id),
            )
        })
        .collect();
    let reach_any: BTreeMap<&str, BTreeSet<String>> = interventions
        .iter()
        .map(|intervention| {
            (
                intervention.id.as_str(),
                reachable_any(model, &intervention.id),
            )
        })
        .collect();

    // --- structural findings ---
    for outcome in &outcomes {
        let reached = interventions
            .iter()
            .any(|intervention| reach_any[intervention.id.as_str()].contains(&outcome.id));
        if !reached {
            builder.push(&Finding {
                kind: "outcome_without_intervention_path",
                analysis: "causal_structure",
                confidence: 0.95,
                involved: vec![format!("variable:{}", outcome.id)],
                witness: json!({"outcome": outcome.id}),
                why: format!(
                    "no intervention has any causal path to outcome '{}'",
                    outcome.id
                ),
                repairs: &["add_claim_path", "add_intervention", "ask_spec_question"],
            });
        }
    }
    for variable in model.variables.values() {
        let connected = model
            .active_claims()
            .any(|claim| claim.source == variable.id || claim.target == variable.id)
            || variable.binds_action.is_some()
            || variable.observable();
        if !connected {
            builder.push(&Finding {
                kind: "orphan_causal_variable",
                analysis: "causal_structure",
                confidence: 0.95,
                involved: vec![format!("variable:{}", variable.id)],
                witness: json!({"variable": variable.id}),
                why: format!(
                    "variable '{}' connects to no claim, measurement, or binding",
                    variable.id
                ),
                repairs: &["remove_variable", "add_claim_path", "add_measurement"],
            });
        }
    }
    for intervention in &interventions {
        let reachable_outcomes: Vec<&&Variable> = outcomes
            .iter()
            .filter(|outcome| reach_any[intervention.id.as_str()].contains(&outcome.id))
            .collect();
        if reachable_outcomes.is_empty() {
            continue;
        }
        // A claim (or mediator) every outcome path depends on.
        let mut shared_claim_cuts: Option<BTreeSet<String>> = None;
        let mut shared_variable_cuts: Option<BTreeSet<String>> = None;
        for outcome in &reachable_outcomes {
            let claim_cuts: BTreeSet<String> = cut_claims(model, &intervention.id, &outcome.id)
                .into_iter()
                .collect();
            let variable_cuts: BTreeSet<String> =
                cut_variables(model, &intervention.id, &outcome.id)
                    .into_iter()
                    .collect();
            shared_claim_cuts = Some(match shared_claim_cuts {
                None => claim_cuts,
                Some(existing) => existing.intersection(&claim_cuts).cloned().collect(),
            });
            shared_variable_cuts = Some(match shared_variable_cuts {
                None => variable_cuts,
                Some(existing) => existing.intersection(&variable_cuts).cloned().collect(),
            });
        }
        let shared_claim_cuts = shared_claim_cuts.unwrap_or_default();
        let shared_variable_cuts = shared_variable_cuts.unwrap_or_default();
        if !shared_claim_cuts.is_empty() || !shared_variable_cuts.is_empty() {
            let mut involved: Vec<String> = shared_claim_cuts
                .iter()
                .map(|claim| format!("claim:{claim}"))
                .chain(
                    shared_variable_cuts
                        .iter()
                        .map(|variable| format!("variable:{variable}")),
                )
                .collect();
            involved.push(format!("variable:{}", intervention.id));
            builder.push(&Finding {
                kind: "single_hypothesis_bottleneck",
                analysis: "causal_structure",
                confidence: 0.9,
                involved,
                witness: json!({
                    "intervention": intervention.id,
                    "outcomes": reachable_outcomes.iter().map(|outcome| outcome.id.clone()).collect::<Vec<_>>(),
                    "shared_cut_claims": shared_claim_cuts,
                    "shared_cut_variables": shared_variable_cuts,
                }),
                why: format!(
                    "every outcome path from '{}' depends on one shared claim or mediator",
                    intervention.id
                ),
                repairs: &["add_alternative_path", "add_evidence_plan"],
            });
        }
    }
    // high_leverage_untested_claim: an evidence-free claim that is a cut for
    // two or more distinct outcomes.
    for claim in model.active_claims() {
        if !claim.evidence.is_empty() {
            continue;
        }
        let mut dependent_outcomes = BTreeSet::new();
        for intervention in &interventions {
            for outcome in &outcomes {
                if reach_any[intervention.id.as_str()].contains(&outcome.id)
                    && cut_claims(model, &intervention.id, &outcome.id).contains(&claim.id)
                {
                    dependent_outcomes.insert(outcome.id.clone());
                }
            }
        }
        if dependent_outcomes.len() >= 2 {
            builder.push(&Finding {
                kind: "high_leverage_untested_claim",
                analysis: "causal_structure",
                confidence: 0.9,
                involved: vec![
                    format!("claim:{}", claim.id),
                    format!("variable:{}", claim.source),
                    format!("variable:{}", claim.target),
                ],
                witness: json!({
                    "dependent_outcomes": dependent_outcomes.iter().map(|outcome| format!("variable:{outcome}")).collect::<Vec<_>>(),
                    "evidence_count": 0,
                }),
                why: "multiple long-horizon outcomes depend on one unvalidated causal claim".to_owned(),
                repairs: &["add_evidence_plan", "add_alternative_path", "downgrade_strategy_confidence"],
            });
        }
    }
    let polarity = polarity_reach(model);
    for outcome in &outcomes {
        for source in model.variables.values() {
            if source.id == outcome.id {
                continue;
            }
            if let Some(set) = polarity.get(&(source.id.clone(), outcome.id.clone()))
                && set.contains(&Polarity::Positive)
                && set.contains(&Polarity::Negative)
            {
                builder.push(&Finding {
                kind: "opposing_path_polarity",
                analysis: "causal_structure",
                confidence: 0.85,
                involved: vec![
                        format!("variable:{}", source.id),
                        format!("variable:{}", outcome.id),
                    ],
                witness: json!({"source": source.id, "outcome": outcome.id,
                           "polarities": set.iter().map(|polarity| polarity.as_str()).collect::<Vec<_>>()}),
                why: format!(
                        "both a positive and a negative causal path connect '{}' to '{}'",
                        source.id, outcome.id
                    ),
                repairs: &["ask_spec_question", "add_evidence_plan"],
            });
            }
        }
    }
    // potential_common_cause: context C -> A, C -> B directly, plus a directed
    // claim between A and B.
    for context in variables_with_role(model, VariableRole::Context) {
        let direct_targets: Vec<&Claim> = model
            .active_claims()
            .filter(|claim| claim.source == context.id)
            .collect();
        for first in &direct_targets {
            for second in &direct_targets {
                if first.target >= second.target {
                    continue;
                }
                let between = model.active_claims().find(|claim| {
                    (claim.source == first.target && claim.target == second.target)
                        || (claim.source == second.target && claim.target == first.target)
                });
                if let Some(between) = between {
                    builder.push(&Finding {
                kind: "potential_common_cause",
                analysis: "causal_structure",
                confidence: 0.7,
                involved: vec![
                            format!("variable:{}", context.id),
                            format!("claim:{}", between.id),
                            format!("variable:{}", first.target),
                            format!("variable:{}", second.target),
                        ],
                witness: json!({
                            "context": context.id,
                            "fork_claims": [first.id, second.id],
                            "between_claim": between.id,
                        }),
                why: format!(
                            "context '{}' feeds both endpoints of claim '{}' — a declared confounding candidate",
                            context.id, between.id
                        ),
                repairs: &["add_evidence_plan", "ask_spec_question"],
            });
                }
            }
        }
    }
    // Feedback loop findings.
    let loop_classes = feedback_loop_classes(model);
    for (feedback_id, (class, cycle_lag)) in &loop_classes {
        if class == "reinforcing" {
            let feedback = &model.feedbacks[feedback_id];
            let loop_variables: BTreeSet<String> = feedback
                .claims
                .iter()
                .filter_map(|claim_id| model.claims.get(claim_id))
                .flat_map(|claim| [claim.source.clone(), claim.target.clone()])
                .collect();
            let damped = loop_classes.iter().any(|(other_id, (other_class, _))| {
                other_id != feedback_id && other_class == "balancing" && {
                    let other = &model.feedbacks[other_id];
                    other
                        .claims
                        .iter()
                        .filter_map(|claim_id| model.claims.get(claim_id))
                        .flat_map(|claim| [claim.source.clone(), claim.target.clone()])
                        .any(|variable| loop_variables.contains(&variable))
                }
            });
            if !damped {
                builder.push(&Finding {
                kind: "feedback_without_damping_story",
                analysis: "causal_structure",
                confidence: 0.6,
                involved: vec![format!("feedback:{feedback_id}")],
                witness: json!({"loop_class": "reinforcing", "claims": feedback.claims}),
                why: format!(
                        "reinforcing feedback '{feedback_id}' shares no variable with any declared balancing loop"
                    ),
                repairs: &["ask_spec_question", "add_claim_path"],
            });
            }
        }
        if let Some(Interval { min, .. }) = cycle_lag
            && *min > model.horizon
        {
            builder.push(&Finding {
                kind: "feedback_period_exceeds_horizon",
                analysis: "causal_time",
                confidence: 0.85,
                involved: vec![format!("feedback:{feedback_id}")],
                witness: json!({"cycle_min_lag": min, "horizon": model.horizon}),
                why: format!(
                    "feedback '{feedback_id}' cannot complete one lap within the model horizon"
                ),
                repairs: &["extend_horizon", "ask_spec_question"],
            });
        }
    }
    // Undeclared cycles (mirrors the check warning as a review finding).
    let declared_edges: BTreeSet<&str> = model
        .feedbacks
        .values()
        .flat_map(|feedback| feedback.claims.iter().map(String::as_str))
        .collect();
    for component in strongly_connected_components(model) {
        if component.len() < 2 {
            continue;
        }
        let undeclared: Vec<String> = model
            .active_claims()
            .filter(|claim| {
                component.contains(&claim.source)
                    && component.contains(&claim.target)
                    && !declared_edges.contains(claim.id.as_str())
            })
            .map(|claim| claim.id.clone())
            .collect();
        if !undeclared.is_empty() {
            builder.push(&Finding {
                kind: "unacknowledged_feedback_loop",
                analysis: "causal_structure",
                confidence: 0.9,
                involved: component
                    .iter()
                    .map(|variable| format!("variable:{variable}"))
                    .collect(),
                witness: json!({"undeclared_cycle_claims": undeclared}),
                why: "a directed cycle exists that no feedback declaration acknowledges".to_owned(),
                repairs: &["add_feedback_declaration"],
            });
        }
    }
    // redundant_paths_share_same_bottleneck.
    for intervention in &interventions {
        for outcome in &outcomes {
            if !reach_any[intervention.id.as_str()].contains(&outcome.id) {
                continue;
            }
            let routes = route_count(model, &intervention.id, &outcome.id, 8);
            if routes >= 2 {
                let cuts = cut_claims(model, &intervention.id, &outcome.id);
                if let Some(cut) = cuts.first() {
                    builder.push(&Finding {
                        kind: "redundant_paths_share_same_bottleneck",
                        analysis: "causal_structure",
                        confidence: 0.8,
                        involved: vec![
                            format!("claim:{cut}"),
                            format!("variable:{}", intervention.id),
                            format!("variable:{}", outcome.id),
                        ],
                        witness: json!({"routes": routes, "shared_cut_claims": cuts}),
                        why: format!(
                            "'{}' reaches '{}' along {routes} routes, yet all depend on one claim",
                            intervention.id, outcome.id
                        ),
                        repairs: &["add_alternative_path", "add_evidence_plan"],
                    });
                }
            }
        }
    }

    // --- time findings ---
    for outcome in &outcomes {
        let Some(deadline) = outcome.deadline else {
            continue;
        };
        let earliest_reach: Option<u64> = interventions
            .iter()
            .filter_map(|intervention| earliest[intervention.id.as_str()].get(&outcome.id))
            .copied()
            .min();
        if let Some(earliest_reach) = earliest_reach
            && earliest_reach > deadline
        {
            builder.push(&Finding {
                kind: "deadline_before_earliest_effect",
                analysis: "causal_time",
                confidence: 0.9,
                involved: vec![format!("variable:{}", outcome.id)],
                witness: json!({"deadline": deadline, "earliest_effect": earliest_reach}),
                why: format!(
                    "outcome '{}' has a deadline of {deadline} but the earliest causal path arrives at {earliest_reach}",
                    outcome.id
                ),
                repairs: &["extend_deadline", "add_faster_path", "ask_spec_question"],
            });
        }
    }
    for variable in model.variables.values() {
        let Some(window) = variable.window else {
            continue;
        };
        if !variable.observable() {
            continue;
        }
        for claim in model
            .active_claims()
            .filter(|claim| claim.target == variable.id)
        {
            let (Lag::Known(lag), Persistence::Known(persists)) = (claim.lag, claim.persists)
            else {
                continue;
            };
            let response_start = lag.min;
            let response_end = lag.max.saturating_add(persists.max);
            if window.max < response_start || window.min > response_end {
                builder.push(&Finding {
                    kind: "observation_window_misses_effect",
                    analysis: "causal_time",
                    confidence: 0.85,
                    involved: vec![
                        format!("variable:{}", variable.id),
                        format!("claim:{}", claim.id),
                    ],
                    witness: json!({
                        "window": interval_json(window),
                        "response_window": {"min": response_start, "max": response_end},
                    }),
                    why: format!(
                        "the observation window for '{}' cannot overlap the response of claim '{}'",
                        variable.id, claim.id
                    ),
                    repairs: &["shift_window", "ask_spec_question"],
                });
            }
        }
    }
    // measurement_cadence_too_coarse: cadence c > persists.min of an arriving
    // claim can miss the shortest-lived effect entirely.
    for variable in model.variables.values() {
        if !variable.observable() {
            continue;
        }
        for claim in model
            .active_claims()
            .filter(|claim| claim.target == variable.id)
        {
            match (variable.cadence, claim.persists) {
                (Some(cadence), Persistence::Known(persists)) => {
                    if cadence > persists.min {
                        builder.push(&Finding {
                kind: "measurement_cadence_too_coarse",
                analysis: "causal_time",
                confidence: 0.85,
                involved: vec![
                                format!("variable:{}", variable.id),
                                format!("claim:{}", claim.id),
                            ],
                witness: json!({
                                "cadence": cadence,
                                "persists": persistence_json(claim.persists),
                                "comparison": format!("cadence {cadence} > persists.min {}", persists.min),
                            }),
                why: format!(
                                "a cadence of {cadence} can miss the shortest persisting effect ({}) of claim '{}' at any phase",
                                persists.min, claim.id
                            ),
                repairs: &["tighten_cadence", "ask_spec_question"],
            });
                    }
                }
                (None, _) | (_, Persistence::Unknown | Persistence::Unbounded) => {
                    not_evaluable.push(json!({
                        "finding_type": "measurement_cadence_too_coarse",
                        "reason": if variable.cadence.is_none() {
                            "variable declares no cadence"
                        } else {
                            "claim persistence is unknown or unbounded"
                        },
                        "involved_nodes": [
                            format!("variable:{}", variable.id),
                            format!("claim:{}", claim.id),
                        ],
                    }));
                }
            }
        }
    }
    // long_horizon_without_leading_indicator and unknown_lag_blocks_timeline.
    for intervention in &interventions {
        for outcome in &outcomes {
            let reached_any = reach_any[intervention.id.as_str()].contains(&outcome.id);
            if !reached_any || intervention.id == outcome.id {
                continue;
            }
            match earliest[intervention.id.as_str()].get(&outcome.id) {
                None => {
                    builder.push(&Finding {
                kind: "unknown_lag_blocks_timeline",
                analysis: "causal_time",
                confidence: 0.9,
                involved: vec![
                            format!("variable:{}", intervention.id),
                            format!("variable:{}", outcome.id),
                        ],
                witness: json!({"reason": "every path carries at least one unknown lag"}),
                why: format!(
                            "'{}' reaches '{}' but no path has fully known lags, so the timeline stops",
                            intervention.id, outcome.id
                        ),
                repairs: &["estimate_lag", "ask_spec_question"],
            });
                }
                Some(&earliest_reach) if earliest_reach * 2 >= model.horizon => {
                    let has_leading = model.variables.values().any(|candidate| {
                        candidate.observable()
                            && candidate.id != outcome.id
                            && earliest[intervention.id.as_str()]
                                .get(&candidate.id)
                                .is_some_and(|&reach| reach * 4 <= model.horizon)
                            && reachable_any(model, &candidate.id).contains(&outcome.id)
                    });
                    if !has_leading {
                        builder.push(&Finding {
                kind: "long_horizon_without_leading_indicator",
                analysis: "causal_time",
                confidence: 0.7,
                involved: vec![
                                format!("variable:{}", intervention.id),
                                format!("variable:{}", outcome.id),
                            ],
                witness: json!({"earliest_effect": earliest_reach, "horizon": model.horizon,
                                   "rule": "earliest*2 >= horizon and no observable path variable with earliest*4 <= horizon"}),
                why: format!(
                                "the path from '{}' to '{}' has no short-term observable node to steer by",
                                intervention.id, outcome.id
                            ),
                repairs: &["add_measurement", "add_claim_path"],
            });
                    }
                }
                Some(_) => {}
            }
        }
    }
    (builder.findings, not_evaluable)
}

/// `loop_class` and cycle lag range per declared feedback.
#[must_use]
pub fn feedback_loop_classes(model: &CausalModel) -> BTreeMap<String, (String, Option<Interval>)> {
    let mut classes = BTreeMap::new();
    for feedback in model.feedbacks.values() {
        let claims: Vec<&Claim> = feedback
            .claims
            .iter()
            .filter_map(|claim_id| model.claims.get(claim_id))
            .collect();
        let polarity = claims
            .iter()
            .map(|claim| claim.polarity)
            .fold(Polarity::Positive, Polarity::product);
        let class = match polarity {
            Polarity::Positive => "reinforcing",
            Polarity::Negative => "balancing",
            Polarity::Unknown => "unknown",
        };
        let mut lag = Some(Interval { min: 0, max: 0 });
        for claim in &claims {
            lag = match (lag, claim.lag) {
                (Some(total), Lag::Known(interval)) => Some(Interval {
                    min: total.min + interval.min,
                    max: total.max.saturating_add(interval.max),
                }),
                _ => None,
            };
        }
        classes.insert(feedback.id.clone(), (class.to_owned(), lag));
    }
    classes
}

/// Indicator classification for observable variables, relative to the
/// latest known outcome earliest-response `E`: `leading` when
/// `3 * earliest <= E`, `lagging` when `3 * earliest >= 2 * E`, otherwise
/// `intermediate`. Deterministic graph/time metadata only.
#[must_use]
pub fn indicator_classes(model: &CausalModel) -> BTreeMap<String, String> {
    let interventions = variables_with_role(model, VariableRole::Intervention);
    let outcomes = variables_with_role(model, VariableRole::Outcome);
    let mut earliest_of: BTreeMap<String, u64> = BTreeMap::new();
    for intervention in &interventions {
        for (node, &value) in &earliest_from(model, &intervention.id) {
            earliest_of
                .entry(node.clone())
                .and_modify(|existing| *existing = (*existing).min(value))
                .or_insert(value);
        }
    }
    let outcome_earliest = outcomes
        .iter()
        .filter_map(|outcome| earliest_of.get(&outcome.id))
        .copied()
        .max();
    let Some(reference) = outcome_earliest else {
        return BTreeMap::new();
    };
    let mut classes = BTreeMap::new();
    for variable in model.variables.values() {
        if !variable.observable() {
            continue;
        }
        let Some(&earliest) = earliest_of.get(&variable.id) else {
            continue;
        };
        let class = if earliest * 3 <= reference {
            "leading"
        } else if earliest * 3 >= reference * 2 {
            "lagging"
        } else {
            "intermediate"
        };
        classes.insert(variable.id.clone(), class.to_owned());
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal::tests::{VALID_MODEL, build};
    use crate::causal_projection::{
        causal_diff_json, causal_graph_projection, causal_timeline_projection,
    };

    fn finding_types(model_source: &str) -> Vec<String> {
        let (model, _) = build(model_source).expect("valid model");
        let (findings, _) = causal_review_findings(&model);
        findings
            .iter()
            .map(|finding| finding["finding_type"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn earliest_is_the_minkowski_sum_of_min_lags() {
        let (model, _) = build(VALID_MODEL).expect("valid model");
        let earliest = earliest_from(&model, "support");
        assert_eq!(earliest.get("habit"), Some(&7));
        assert_eq!(earliest.get("retention"), Some(&37));
    }

    #[test]
    fn representative_path_carries_polarity_and_first_response() {
        let (model, _) = build(VALID_MODEL).expect("valid model");
        let graph = causal_graph_projection(&model);
        let paths = graph["paths"].as_array().expect("paths");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0]["polarity"], "positive");
        assert_eq!(paths[0]["first_response"], json!({"min": 37, "max": 120}));
        assert_eq!(graph["truncation"]["paths_truncated"], 0);
    }

    #[test]
    fn valid_model_reports_bottleneck_but_not_deadline() {
        let types = finding_types(VALID_MODEL);
        // Single chain: everything depends on each claim -> bottleneck finding.
        assert!(types.contains(&"single_hypothesis_bottleneck".to_owned()));
        // Deadline 180 > earliest 37: no deadline finding.
        assert!(!types.contains(&"deadline_before_earliest_effect".to_owned()));
        assert!(!types.contains(&"unknown_lag_blocks_timeline".to_owned()));
    }

    #[test]
    fn deadline_before_earliest_effect_fires() {
        let source = VALID_MODEL.replace("deadline 180", "deadline 20");
        let types = finding_types(&source);
        assert!(types.contains(&"deadline_before_earliest_effect".to_owned()));
    }

    #[test]
    fn cadence_boundary_rule_is_exact() {
        // persists.min of the arriving claim is 90; cadence 7 -> no finding.
        let types = finding_types(VALID_MODEL);
        assert!(!types.contains(&"measurement_cadence_too_coarse".to_owned()));
        // cadence == persists.min -> still no finding.
        let equal = VALID_MODEL.replace(
            "observes state biz.x\n    cadence 7",
            "observes state biz.x\n    cadence 90",
        );
        assert!(!finding_types(&equal).contains(&"measurement_cadence_too_coarse".to_owned()));
        // cadence > persists.min -> finding.
        let coarse = VALID_MODEL.replace(
            "observes state biz.x\n    cadence 7",
            "observes state biz.x\n    cadence 91",
        );
        assert!(finding_types(&coarse).contains(&"measurement_cadence_too_coarse".to_owned()));
    }

    #[test]
    fn unknown_persistence_is_not_evaluable_not_a_finding() {
        let source = VALID_MODEL.replace("persists 90..365", "persists unknown");
        let (model, _) = build(&source).expect("valid model");
        let (findings, not_evaluable) = causal_review_findings(&model);
        assert!(
            !findings
                .iter()
                .any(|finding| finding["finding_type"] == "measurement_cadence_too_coarse")
        );
        assert!(not_evaluable.iter().any(|entry| {
            entry["finding_type"] == "measurement_cadence_too_coarse"
                && entry["reason"]
                    .as_str()
                    .unwrap()
                    .contains("unknown or unbounded")
        }));
    }

    #[test]
    fn unknown_lag_blocks_timeline_when_no_known_path_exists() {
        let source = VALID_MODEL.replace("lag 30..90", "lag unknown");
        let types = finding_types(&source);
        assert!(types.contains(&"unknown_lag_blocks_timeline".to_owned()));
        let (model, _) = build(&source).expect("valid model");
        let timeline = causal_timeline_projection(&model);
        let entries = timeline["timelines"].as_array().expect("timelines");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["first_pass"], json!("unknown"));
    }

    #[test]
    fn opposing_polarity_is_detected_through_composition() {
        // Add a direct negative claim support -> retention.
        let source = VALID_MODEL.replace(
            "  evidence E1 from \"evidence/e1.causal.json\"",
            "  claim C_Direct support -> retention {\n    version 1\n    status active\n    polarity negative\n    lag 1..5\n    persists 5..10\n    basis hypothesis\n  }\n  evidence E1 from \"evidence/e1.causal.json\"",
        );
        let types = finding_types(&source);
        assert!(types.contains(&"opposing_path_polarity".to_owned()));
        // And the redundant-route bottleneck rule stays quiet: two genuinely
        // independent routes share no cut claim.
        assert!(!types.contains(&"redundant_paths_share_same_bottleneck".to_owned()));
    }

    #[test]
    fn diff_tracks_content_lifecycle_and_support_transition() {
        let (before, _) = build(VALID_MODEL).expect("valid model");
        let changed = VALID_MODEL
            .replace(
                "polarity positive\n    lag 30..90",
                "polarity negative\n    lag 30..90",
            )
            .replace(
                "claim C_HabitRetention habit -> retention {\n    version 1",
                "claim C_HabitRetention habit -> retention {\n    version 2",
            );
        let (after, _) = build(&changed).expect("valid model");
        let diff = causal_diff_json(&before, &after);
        let change_list = diff["changes"].as_array().expect("changes");
        let content = change_list
            .iter()
            .find(|change| change["kind"] == "claim_content_changed")
            .expect("content change");
        assert_eq!(content["id"], "claim:C_HabitRetention");
        assert_eq!(content["before_version"], 1);
        assert_eq!(content["after_version"], 2);
        assert!(
            content["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "polarity")
        );
        assert!(
            change_list
                .iter()
                .all(|change| change["support_transition"] == "not_available")
        );
    }

    #[test]
    fn indicator_classes_use_thirds_of_outcome_earliest() {
        let (model, _) = build(VALID_MODEL).expect("valid model");
        let classes = indicator_classes(&model);
        // Outcome earliest reference E = 37. habit earliest 7: 21 <= 37 -> leading.
        assert_eq!(classes.get("habit").map(String::as_str), Some("leading"));
        // retention earliest 37: 111 >= 74 -> lagging.
        assert_eq!(
            classes.get("retention").map(String::as_str),
            Some("lagging")
        );
    }

    #[test]
    fn feedback_loops_classify_by_sign_product_with_unknown_absorption() {
        let looped = |polarity: &str| {
            VALID_MODEL.trim_end().trim_end_matches('}').to_owned()
                + &format!(
                    "  claim C_Back retention -> support {{\n    version 1\n    status active\n    polarity {polarity}\n    lag 30..60\n    persists unknown\n    basis assumption\n  }}\n  feedback F_Loop {{ claims C_Back, C_SupportHabit, C_HabitRetention }}\n}}\n"
                )
        };
        for (polarity, expected) in [
            ("positive", "reinforcing"),
            ("negative", "balancing"),
            ("unknown", "unknown"),
        ] {
            let (model, _) = build(&looped(polarity)).expect("valid model");
            let classes = feedback_loop_classes(&model);
            let (class, lag) = &classes["F_Loop"];
            assert_eq!(class, expected, "polarity {polarity}");
            assert_eq!(*lag, Some(crate::causal::Interval { min: 67, max: 180 }));
            let (findings, _) = causal_review_findings(&model);
            let damping = findings
                .iter()
                .any(|finding| finding["finding_type"] == "feedback_without_damping_story");
            assert_eq!(damping, expected == "reinforcing", "polarity {polarity}");
        }
        // Repetition upper bound within the horizon: 730 / 67 = 10.
        let (model, _) = build(&looped("positive")).expect("valid model");
        let graph = causal_graph_projection(&model);
        assert_eq!(graph["feedbacks"][0]["loop_class"], "reinforcing");
        assert_eq!(graph["feedbacks"][0]["max_repetitions_within_horizon"], 10);
    }
}
