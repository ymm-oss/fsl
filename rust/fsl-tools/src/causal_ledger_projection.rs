// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Causal portfolio ledger projection (issue #364, Phase 5).
//!
//! Integrates claims, validation plans, evidence, expectations, and
//! observations into a per-claim projection with deterministic attention
//! reasons. The ledger does not prove causality, manage schedules, or own
//! external project state.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::causal::{CausalModel, Claim, ClaimStatus, Lag};
use crate::causal_evidence::{
    EvidenceArtifact, LifecycleStatus, ScopeApplication, SupportOverlay, compare_scope,
};
use crate::causal_plan::PlanArtifact;

const DO_NOT_ASSUME: [&str; 5] = [
    "The causal claim is proved or verified",
    "A completed validation plan establishes causality",
    "No unmodeled common cause exists",
    "Absence of challenging evidence means the claim is true",
    "Portfolio readiness is a formal assurance class",
];

#[must_use]
pub fn build_ledger(
    model: &CausalModel,
    plans: &BTreeMap<String, PlanArtifact>,
    artifacts: &BTreeMap<String, EvidenceArtifact>,
    overlay: &SupportOverlay,
    as_of: Option<&str>,
) -> Value {
    let mut claim_entries: Vec<Value> = Vec::new();

    for claim in model.claims.values() {
        let is_active = claim.status == ClaimStatus::Active;
        let support = overlay
            .support
            .get(&claim.id)
            .cloned()
            .unwrap_or_else(|| "untested".to_owned());

        let (applicable_plans, excluded_plans, plan_external_refs) =
            evaluate_plans(model, claim, plans);
        let (applicable_evidence, excluded_evidence) = evaluate_evidence(claim, artifacts, overlay);

        let mut attention_reasons: Vec<Value> = Vec::new();
        if is_active {
            derive_plan_attention(
                model,
                claim,
                plans,
                &applicable_plans,
                &excluded_plans,
                &mut attention_reasons,
            );
            derive_evidence_attention(claim, &support, artifacts, overlay, &mut attention_reasons);
        }

        claim_entries.push(json!({
            "id": format!("claim:{}", claim.id),
            "version": claim.version,
            "status": if is_active { "active" } else { "retired" },
            "formal_assurance": "not_run",
            "causal_support": support,
            "plans": {
                "applicable": applicable_plans,
                "excluded": excluded_plans,
            },
            "evidence": {
                "applicable": applicable_evidence,
                "excluded": excluded_evidence,
            },
            "external_refs": plan_external_refs,
            "attention_reasons": attention_reasons,
        }));
    }

    json!({
        "result": "causal_ledger",
        "schema_version": "causal-ledger.v0",
        "formal_result": "not_run",
        "model": model.name,
        "as_of": as_of,
        "claims": claim_entries,
        "do_not_assume": DO_NOT_ASSUME,
    })
}

fn evaluate_plans(
    model: &CausalModel,
    claim: &Claim,
    plans: &BTreeMap<String, PlanArtifact>,
) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut applicable = Vec::new();
    let mut excluded = Vec::new();
    let mut external_refs = Vec::new();

    for plan in plans.values() {
        let pins_claim = plan
            .claims
            .iter()
            .any(|(id, _)| strip_prefix(id) == claim.id);
        if !pins_claim {
            continue;
        }

        let version_match = plan
            .claims
            .iter()
            .any(|(id, version)| strip_prefix(id) == claim.id && *version == claim.version);
        let lifecycle_active = plan.lifecycle_status == LifecycleStatus::Active;
        let scope_relation = compare_scope(model, claim, &plan.scope);

        let mut exclusions: Vec<&str> = Vec::new();
        if !version_match {
            exclusions.push("version_mismatch");
        }
        if !lifecycle_active {
            exclusions.push("lifecycle_not_active");
        }
        if scope_relation != ScopeApplication::Subsumes {
            exclusions.push(match scope_relation {
                ScopeApplication::Unassessable => "scope_unassessable",
                ScopeApplication::PartialOverlap => "scope_partial_overlap",
                ScopeApplication::Disjoint => "scope_disjoint",
                ScopeApplication::Subsumes => unreachable!(),
            });
        }

        let entry = json!({
            "plan_id": format!("plan:{}", plan.plan_id),
            "design": plan.design,
            "version_match": version_match,
            "scope_relation": scope_relation.as_str(),
            "lifecycle_status": plan.lifecycle_status.as_str(),
        });

        if exclusions.is_empty() {
            applicable.push(entry);
        } else {
            let mut excluded_entry = entry;
            excluded_entry
                .as_object_mut()
                .expect("object")
                .insert("exclusions".to_owned(), json!(exclusions));
            excluded.push(excluded_entry);
        }

        for ext_ref in &plan.external_refs {
            external_refs.push(ext_ref.clone());
        }
    }

    (applicable, excluded, external_refs)
}

fn evaluate_evidence(
    claim: &Claim,
    artifacts: &BTreeMap<String, EvidenceArtifact>,
    overlay: &SupportOverlay,
) -> (Vec<Value>, Vec<Value>) {
    let mut applicable = Vec::new();
    let mut excluded = Vec::new();

    for artifact in artifacts.values() {
        let pins_claim = artifact
            .claims
            .iter()
            .any(|(id, _)| strip_prefix(id) == claim.id);
        if !pins_claim {
            continue;
        }

        let applicability = overlay
            .applicability
            .iter()
            .find(|entry| entry.claim_id == claim.id && entry.evidence_id == artifact.evidence_id);
        let is_applicable = applicability.is_some_and(|entry| entry.applicable);

        let entry = json!({
            "evidence_id": format!("evidence:{}", artifact.evidence_id),
            "design": artifact.design,
            "support": artifact.support,
            "lifecycle_status": artifact.lifecycle_status.as_str(),
            "is_observation": artifact.observation.is_some(),
        });

        if is_applicable {
            applicable.push(entry);
        } else {
            let mut excluded_entry = entry;
            let exclusions: Vec<&str> = applicability.map_or_else(
                || vec!["no_applicability_record"],
                |entry| entry.exclusions.clone(),
            );
            excluded_entry
                .as_object_mut()
                .expect("object")
                .insert("exclusions".to_owned(), json!(exclusions));
            excluded.push(excluded_entry);
        }
    }

    (applicable, excluded)
}

#[allow(clippy::too_many_lines)]
fn derive_plan_attention(
    model: &CausalModel,
    claim: &Claim,
    plans: &BTreeMap<String, PlanArtifact>,
    applicable_plans: &[Value],
    excluded_plans: &[Value],
    reasons: &mut Vec<Value>,
) {
    if applicable_plans.is_empty() {
        if excluded_plans.is_empty() {
            reasons.push(json!({
                "reason": "validation_plan_missing",
                "witness": { "claim_id": format!("claim:{}", claim.id) }
            }));
        } else {
            for plan_json in excluded_plans {
                let exclusions = plan_json
                    .get("exclusions")
                    .and_then(Value::as_array)
                    .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                    .unwrap_or_default();
                let plan_id = plan_json["plan_id"].clone();
                if exclusions.contains(&"version_mismatch") {
                    reasons.push(json!({
                        "reason": "validation_plan_version_mismatch",
                        "witness": {
                            "claim_id": format!("claim:{}", claim.id),
                            "claim_version": claim.version,
                            "plan_id": plan_id,
                        }
                    }));
                }
                if exclusions.contains(&"scope_unassessable") {
                    reasons.push(json!({
                        "reason": "validation_plan_scope_unassessable",
                        "witness": {
                            "claim_id": format!("claim:{}", claim.id),
                            "plan_id": plan_id,
                        }
                    }));
                }
                if exclusions.contains(&"scope_partial_overlap")
                    || exclusions.contains(&"scope_disjoint")
                {
                    reasons.push(json!({
                        "reason": "validation_plan_scope_inapplicable",
                        "witness": {
                            "claim_id": format!("claim:{}", claim.id),
                            "plan_id": plan_id,
                            "scope_relation": plan_json["scope_relation"],
                        }
                    }));
                }
            }
            if !reasons.iter().any(|reason| {
                reason["reason"]
                    .as_str()
                    .is_some_and(|r| r.starts_with("validation_plan_"))
            }) {
                reasons.push(json!({
                    "reason": "validation_plan_missing",
                    "witness": { "claim_id": format!("claim:{}", claim.id) }
                }));
            }
        }
    }

    // Window and measurement checks for plans pinning this claim.
    for plan in plans.values() {
        let pins_current = plan
            .claims
            .iter()
            .any(|(id, version)| strip_prefix(id) == claim.id && *version == claim.version);
        if !pins_current || plan.lifecycle_status != LifecycleStatus::Active {
            continue;
        }

        if let Some(window) = &plan.observation_window
            && window.timebase == model.timebase
            && let Lag::Known(interval) = &claim.lag
            && window.minimum < interval.min
        {
            reasons.push(json!({
                "reason": "validation_window_shorter_than_lag",
                "witness": {
                    "claim_id": format!("claim:{}", claim.id),
                    "plan_id": format!("plan:{}", plan.plan_id),
                    "window_minimum": window.minimum,
                    "claim_lag_min": interval.min,
                }
            }));
        }

        for measurement_ref in &plan.measurements {
            let var_name = measurement_ref
                .strip_prefix("variable:")
                .unwrap_or(measurement_ref);
            if let Some(variable) = model.variables.get(var_name) {
                if variable.latent && variable.proxy.is_none() {
                    reasons.push(json!({
                        "reason": "required_measurement_unavailable",
                        "witness": {
                            "claim_id": format!("claim:{}", claim.id),
                            "plan_id": format!("plan:{}", plan.plan_id),
                            "measurement": measurement_ref,
                            "detail": "latent variable without proxy",
                        }
                    }));
                }
            } else {
                reasons.push(json!({
                    "reason": "required_measurement_unavailable",
                    "witness": {
                        "claim_id": format!("claim:{}", claim.id),
                        "plan_id": format!("plan:{}", plan.plan_id),
                        "measurement": measurement_ref,
                        "detail": "variable not found in model",
                    }
                }));
            }
        }
    }
}

fn derive_evidence_attention(
    claim: &Claim,
    support: &str,
    artifacts: &BTreeMap<String, EvidenceArtifact>,
    overlay: &SupportOverlay,
    reasons: &mut Vec<Value>,
) {
    let claim_id = &claim.id;

    let has_applicable_evidence = overlay
        .applicability
        .iter()
        .any(|entry| entry.claim_id == *claim_id && entry.applicable);
    let has_any_referencing = artifacts.values().any(|artifact| {
        artifact
            .claims
            .iter()
            .any(|(id, _)| strip_prefix(id) == *claim_id)
    });

    if !has_applicable_evidence {
        if has_any_referencing {
            let all_stale = overlay
                .applicability
                .iter()
                .filter(|entry| entry.claim_id == *claim_id)
                .all(|entry| {
                    entry.exclusions.iter().any(|exclusion| {
                        exclusion.contains("stale") || exclusion.contains("freshness")
                    })
                });
            if all_stale {
                reasons.push(json!({
                    "reason": "evidence_freshness_requires_refresh",
                    "witness": { "claim_id": format!("claim:{claim_id}") }
                }));
            } else {
                reasons.push(json!({
                    "reason": "current_evidence_missing",
                    "witness": { "claim_id": format!("claim:{claim_id}") }
                }));
            }
        } else {
            reasons.push(json!({
                "reason": "current_evidence_missing",
                "witness": { "claim_id": format!("claim:{claim_id}") }
            }));
        }
        return;
    }

    match support {
        "inconclusive" => {
            reasons.push(json!({
                "reason": "current_evidence_inconclusive",
                "witness": { "claim_id": format!("claim:{claim_id}") }
            }));
        }
        "mixed" => {
            reasons.push(json!({
                "reason": "conflicting_evidence_requires_decision",
                "witness": { "claim_id": format!("claim:{claim_id}") }
            }));
        }
        "challenged" => {
            reasons.push(json!({
                "reason": "challenging_evidence_requires_decision",
                "witness": { "claim_id": format!("claim:{claim_id}") }
            }));
        }
        _ => {}
    }

    let applicable_for_claim: Vec<&EvidenceArtifact> = artifacts
        .values()
        .filter(|artifact| {
            artifact
                .claims
                .iter()
                .any(|(id, _)| strip_prefix(id) == *claim_id)
                && overlay.applicability.iter().any(|entry| {
                    entry.claim_id == *claim_id
                        && entry.evidence_id == artifact.evidence_id
                        && entry.applicable
                })
        })
        .collect();
    let all_observations = !applicable_for_claim.is_empty()
        && applicable_for_claim
            .iter()
            .all(|artifact| artifact.observation.is_some());
    if all_observations {
        reasons.push(json!({
            "reason": "observation_not_directional_support",
            "witness": { "claim_id": format!("claim:{claim_id}") }
        }));
    }
}

fn strip_prefix(id: &str) -> &str {
    id.strip_prefix("claim:").unwrap_or(id)
}
