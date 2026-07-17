// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! External-evidence ingestion for the review-only causal profile
//! (issue #322, `docs/DESIGN-causal.md` §1 Evidence Plane).
//!
//! FSL validates artifact schema, references, scope, period, digests, and
//! lifecycle chains, and aggregates a deterministic per-claim
//! `causal_support`. It never re-analyzes raw data, estimates effect sizes,
//! or converts support into formal assurance: `formal_assurance` stays
//! `not_run` on every claim regardless of evidence.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::causal::{CausalModel, Claim, Lag};

pub const EVIDENCE_SCHEMA_VERSION: &str = "fsl-causal-evidence.v0";
pub const LIFECYCLE_SCHEMA_VERSION: &str = "fsl-causal-evidence-lifecycle.v0";
pub const DESIGN_VOCABULARY: &[&str] = &[
    "randomized_experiment",
    "quasi_experiment",
    "observational",
    "expert_judgment",
];
pub const SUPPORT_VOCABULARY: &[&str] = &["supports", "challenges", "inconclusive"];

/// Fatal evidence-plane error: analysis does not start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceError {
    pub kind: &'static str,
    pub message: String,
}

fn error(kind: &'static str, message: impl Into<String>) -> EvidenceError {
    EvidenceError {
        kind,
        message: message.into(),
    }
}

/// Canonical JSON: recursively sorted object keys, compact separators.
/// `preserve_order` is enabled workspace-wide, so sorting must be explicit.
#[must_use]
pub fn canonical_json(value: &Value) -> String {
    fn sort(value: &Value) -> Value {
        match value {
            Value::Object(entries) => {
                let mut sorted: Vec<(&String, &Value)> = entries.iter().collect();
                sorted.sort_by_key(|(key, _)| key.as_str());
                let mut object = Map::new();
                for (key, entry) in sorted {
                    object.insert(key.clone(), sort(entry));
                }
                Value::Object(object)
            }
            Value::Array(items) => Value::Array(items.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    sort(value).to_string()
}

fn sha256_of(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// The digest an artifact must declare: canonical JSON of the payload with
/// the `artifact_digest` field removed.
#[must_use]
pub fn artifact_digest(artifact: &Value) -> String {
    let mut payload = artifact.clone();
    if let Some(object) = payload.as_object_mut() {
        object.remove("artifact_digest");
    }
    sha256_of(&canonical_json(&payload))
}

/// The digest a lifecycle record must declare: canonical JSON of the record
/// with `record_digest` removed, plus the chain's `evidence_id` and
/// `artifact_digest`.
#[must_use]
pub fn lifecycle_record_digest(
    evidence_id: &str,
    chain_artifact_digest: &str,
    record: &Value,
) -> String {
    let mut payload = record.clone();
    if let Some(object) = payload.as_object_mut() {
        object.remove("record_digest");
        object.insert("evidence_id".to_owned(), json!(evidence_id));
        object.insert("artifact_digest".to_owned(), json!(chain_artifact_digest));
    }
    sha256_of(&canonical_json(&payload))
}

/// Days since the civil epoch for a `YYYY-MM-DD` date (Gregorian).
fn civil_days(date: &str) -> Option<i64> {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = date.get(0..4)?.parse().ok()?;
    let month: i64 = date.get(5..7)?.parse().ok()?;
    let day: i64 = date.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let adjusted_year = if month <= 2 { year - 1 } else { year };
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeApplication {
    Subsumes,
    PartialOverlap,
    Disjoint,
    Unassessable,
}

impl ScopeApplication {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subsumes => "subsumes",
            Self::PartialOverlap => "partial_overlap",
            Self::Disjoint => "disjoint",
            Self::Unassessable => "unassessable",
        }
    }
}

/// One validated evidence artifact plus its lifecycle status.
#[derive(Clone, Debug)]
pub struct EvidenceArtifact {
    pub evidence_id: String,
    pub design: String,
    pub support: String,
    pub claims: Vec<(String, u64)>,
    pub scope: BTreeMap<String, Vec<String>>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub valid_until: Option<String>,
    pub source_study_id: Option<String>,
    pub derived_from: Vec<String>,
    pub observation: Option<Value>,
    pub declared_digest: String,
    pub lifecycle_status: LifecycleStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleStatus {
    Active,
    Retracted,
    Superseded,
    Unknown,
}

impl LifecycleStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Retracted => "retracted",
            Self::Superseded => "superseded",
            Self::Unknown => "unknown",
        }
    }
}

fn required_str<'json>(
    value: &'json Value,
    field: &str,
    context: &str,
) -> Result<&'json str, EvidenceError> {
    value.get(field).and_then(Value::as_str).ok_or_else(|| {
        error(
            "causal_evidence_schema_mismatch",
            format!("{context}: missing or non-string required field '{field}'"),
        )
    })
}

/// Parse and structurally validate one artifact JSON (schema version, closed
/// enums, digest, observation rules). Fatal errors here stop the analysis.
///
/// # Errors
///
/// `causal_evidence_schema_mismatch` for shape/enum violations and
/// `causal_evidence_digest_mismatch` when the declared digest does not match
/// the canonical payload.
#[allow(clippy::too_many_lines)]
pub fn parse_artifact(artifact: &Value) -> Result<EvidenceArtifact, EvidenceError> {
    let evidence_id = required_str(artifact, "evidence_id", "evidence artifact")?.to_owned();
    let context = format!("evidence '{evidence_id}'");
    let schema_version = required_str(artifact, "schema_version", &context)?;
    if schema_version != EVIDENCE_SCHEMA_VERSION {
        return Err(error(
            "causal_evidence_schema_mismatch",
            format!("{context}: unsupported schema_version '{schema_version}'"),
        ));
    }
    let design = required_str(artifact, "design", &context)?.to_owned();
    if !DESIGN_VOCABULARY.contains(&design.as_str()) {
        return Err(error(
            "causal_evidence_schema_mismatch",
            format!("{context}: unknown design '{design}' (closed vocabulary)"),
        ));
    }
    let support = required_str(artifact, "support", &context)?.to_owned();
    if !SUPPORT_VOCABULARY.contains(&support.as_str()) {
        return Err(error(
            "causal_evidence_schema_mismatch",
            format!("{context}: unknown support '{support}' (closed vocabulary)"),
        ));
    }
    let formal_result = required_str(artifact, "formal_result", &context)?;
    if formal_result != "not_run" {
        return Err(error(
            "causal_evidence_schema_mismatch",
            format!("{context}: formal_result must be \"not_run\""),
        ));
    }
    let mut claims = Vec::new();
    for entry in artifact
        .get("claims")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "causal_evidence_schema_mismatch",
                format!("{context}: missing claims array"),
            )
        })?
    {
        let id = required_str(entry, "id", &context)?;
        let id = id.strip_prefix("claim:").unwrap_or(id).to_owned();
        let version = entry
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                error(
                    "causal_evidence_schema_mismatch",
                    format!("{context}: claim reference requires a positive integer version"),
                )
            })?;
        claims.push((id, version));
    }
    let mut scope = BTreeMap::new();
    if let Some(object) = artifact.get("scope").and_then(Value::as_object) {
        for (dimension, tokens) in object {
            let tokens = tokens.as_array().ok_or_else(|| {
                error(
                    "causal_evidence_schema_mismatch",
                    format!("{context}: scope dimension '{dimension}' must be an array"),
                )
            })?;
            scope.insert(
                dimension.clone(),
                tokens
                    .iter()
                    .map(|token| token.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        error(
                            "causal_evidence_schema_mismatch",
                            format!("{context}: scope tokens must be strings"),
                        )
                    })?,
            );
        }
    }
    let period = artifact.get("period");
    let period_field = |field: &str| {
        period
            .and_then(|period| period.get(field))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let observation = artifact.get("observation").filter(|value| !value.is_null());
    if let Some(observation) = observation {
        validate_observation(observation, &design, &support, &context)?;
    }
    let declared_digest = required_str(artifact, "artifact_digest", &context)?.to_owned();
    let computed = artifact_digest(artifact);
    if declared_digest != computed {
        return Err(error(
            "causal_evidence_digest_mismatch",
            format!("{context}: declared artifact_digest does not match the canonical payload"),
        ));
    }
    Ok(EvidenceArtifact {
        evidence_id,
        design,
        support,
        claims,
        scope,
        period_start: period_field("start"),
        period_end: period_field("end"),
        valid_until: period_field("valid_until"),
        source_study_id: artifact
            .get("source_study_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        derived_from: artifact
            .get("derived_from")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        observation: observation.cloned(),
        declared_digest,
        lifecycle_status: LifecycleStatus::Unknown,
    })
}

/// Reserved Phase-4 observation object: closed shape, forces
/// `design: observational` and `support: inconclusive` (issue #322/#360).
fn validate_observation(
    observation: &Value,
    design: &str,
    support: &str,
    context: &str,
) -> Result<(), EvidenceError> {
    let allowed = [
        "kind",
        "expectation_id",
        "expectation_digest",
        "verdict",
        "assurance",
        "event_counts",
        "digests",
    ];
    let object = observation.as_object().ok_or_else(|| {
        error(
            "causal_evidence_schema_mismatch",
            format!("{context}: observation must be an object or null"),
        )
    })?;
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(error(
                "causal_evidence_schema_mismatch",
                format!("{context}: unknown observation field '{key}'"),
            ));
        }
    }
    if object.get("kind").and_then(Value::as_str) != Some("expectation_replay") {
        return Err(error(
            "causal_evidence_schema_mismatch",
            format!("{context}: observation.kind must be \"expectation_replay\""),
        ));
    }
    if object.get("assurance").and_then(Value::as_str) != Some("replay-observed") {
        return Err(error(
            "causal_evidence_schema_mismatch",
            format!("{context}: observation.assurance must be \"replay-observed\""),
        ));
    }
    if let Some(counts) = object.get("event_counts").and_then(Value::as_object) {
        for (name, count) in counts {
            if count.as_u64().is_none() {
                return Err(error(
                    "causal_evidence_schema_mismatch",
                    format!(
                        "{context}: observation.event_counts.{name} must be a non-negative integer"
                    ),
                ));
            }
        }
    }
    if design != "observational" {
        return Err(error(
            "causal_evidence_schema_mismatch",
            format!(
                "{context}: an artifact with an observation must declare design \"observational\""
            ),
        ));
    }
    if support != "inconclusive" {
        return Err(error(
            "causal_evidence_schema_mismatch",
            format!(
                "{context}: an artifact with an observation must declare support \"inconclusive\" (a study protocol digest never unlocks directed support)"
            ),
        ));
    }
    Ok(())
}

/// Validate one lifecycle chain and return the current status it establishes.
///
/// # Errors
///
/// `causal_evidence_lifecycle_mismatch` for structural chain violations.
#[allow(clippy::too_many_lines)]
pub fn validate_lifecycle_chain(
    chain: &Value,
    artifacts: &BTreeMap<String, EvidenceArtifact>,
) -> Result<(String, LifecycleStatus), EvidenceError> {
    let evidence_id = required_str(chain, "evidence_id", "lifecycle chain")?.to_owned();
    let context = format!("lifecycle chain for '{evidence_id}'");
    let schema_version = required_str(chain, "schema_version", &context)?;
    if schema_version != LIFECYCLE_SCHEMA_VERSION {
        return Err(error(
            "causal_evidence_lifecycle_mismatch",
            format!("{context}: unsupported schema_version '{schema_version}'"),
        ));
    }
    let chain_digest = required_str(chain, "artifact_digest", &context)?;
    if let Some(artifact) = artifacts.get(&evidence_id)
        && artifact.declared_digest != chain_digest
    {
        return Err(error(
            "causal_evidence_lifecycle_mismatch",
            format!("{context}: chain artifact_digest does not match the artifact"),
        ));
    }
    let records = chain
        .get("records")
        .and_then(Value::as_array)
        .filter(|records| !records.is_empty())
        .ok_or_else(|| {
            error(
                "causal_evidence_lifecycle_mismatch",
                format!("{context}: requires a non-empty records array"),
            )
        })?;
    let mut previous_digest: Option<String> = None;
    let mut status = LifecycleStatus::Unknown;
    let mut terminal = false;
    for (index, record) in records.iter().enumerate() {
        let sequence = record
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                error(
                    "causal_evidence_lifecycle_mismatch",
                    format!("{context}: record {index} missing sequence"),
                )
            })?;
        if sequence != (index as u64) + 1 {
            return Err(error(
                "causal_evidence_lifecycle_mismatch",
                format!("{context}: sequence gap or fork at record {index} (sequence {sequence})"),
            ));
        }
        if terminal {
            return Err(error(
                "causal_evidence_lifecycle_mismatch",
                format!("{context}: record after a terminal status"),
            ));
        }
        let recorded_previous = record.get("previous_record_digest").and_then(Value::as_str);
        if recorded_previous.map(str::to_owned) != previous_digest {
            return Err(error(
                "causal_evidence_lifecycle_mismatch",
                format!("{context}: previous_record_digest chain broken at sequence {sequence}"),
            ));
        }
        let declared = required_str(record, "record_digest", &context)?;
        let computed = lifecycle_record_digest(&evidence_id, chain_digest, record);
        if declared != computed {
            return Err(error(
                "causal_evidence_lifecycle_mismatch",
                format!("{context}: record_digest mismatch at sequence {sequence}"),
            ));
        }
        let record_status = required_str(record, "status", &context)?;
        status = match record_status {
            "active" => LifecycleStatus::Active,
            "retracted" => {
                terminal = true;
                LifecycleStatus::Retracted
            }
            "superseded" => {
                let successor = record.get("superseded_by").and_then(Value::as_str);
                let Some(successor) = successor else {
                    return Err(error(
                        "causal_evidence_lifecycle_mismatch",
                        format!("{context}: superseded record requires superseded_by"),
                    ));
                };
                if !artifacts.contains_key(successor) {
                    return Err(error(
                        "causal_evidence_lifecycle_mismatch",
                        format!(
                            "{context}: superseded_by '{successor}' does not resolve to a supplied artifact"
                        ),
                    ));
                }
                terminal = true;
                LifecycleStatus::Superseded
            }
            other => {
                return Err(error(
                    "causal_evidence_lifecycle_mismatch",
                    format!("{context}: unknown lifecycle status '{other}'"),
                ));
            }
        };
        previous_digest = Some(declared.to_owned());
    }
    Ok((evidence_id, status))
}

/// Per-artifact, per-claim applicability outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Applicability {
    pub evidence_id: String,
    pub claim_id: String,
    pub applicable: bool,
    /// Warning finding types explaining an exclusion (empty when applicable).
    pub exclusions: Vec<&'static str>,
    pub scope: ScopeApplication,
    /// Timebase-converted observation window, when convertible.
    pub window: Option<u64>,
    pub not_evaluable: Vec<Value>,
}

/// Deterministic per-claim support aggregation input and result.
#[derive(Clone, Debug)]
pub struct SupportOverlay {
    /// Claim id -> `causal_support` value.
    pub support: BTreeMap<String, String>,
    pub applicability: Vec<Applicability>,
    /// Findings contributed by the evidence plane (all `not_a_violation`).
    pub findings: Vec<Value>,
    pub not_evaluable: Vec<Value>,
}

fn scope_dimension_relation(
    model: &CausalModel,
    dimension: &str,
    claim_token: &str,
    evidence_tokens: &[String],
) -> ScopeApplication {
    let empty_tokens = BTreeSet::new();
    let declared = model.scope.tokens.get(dimension).unwrap_or(&empty_tokens);
    if !declared.contains(claim_token)
        || evidence_tokens
            .iter()
            .any(|token| !declared.contains(token))
    {
        return ScopeApplication::Unassessable;
    }
    let empty_subsets = BTreeMap::new();
    let subsets = model
        .scope
        .subset_closure
        .get(dimension)
        .unwrap_or(&empty_subsets);
    let empty_pairs = BTreeSet::new();
    let overlaps = model.scope.overlaps.get(dimension).unwrap_or(&empty_pairs);
    let disjoint = model.scope.disjoint.get(dimension).unwrap_or(&empty_pairs);
    let pair = |left: &str, right: &str| {
        if left <= right {
            (left.to_owned(), right.to_owned())
        } else {
            (right.to_owned(), left.to_owned())
        }
    };
    let contains = |ancestor: &str, descendant: &str| {
        ancestor == descendant
            || subsets
                .get(descendant)
                .is_some_and(|parents| parents.contains(ancestor))
    };
    // Subsumes: some evidence token contains the claim token.
    if evidence_tokens
        .iter()
        .any(|token| contains(token, claim_token))
    {
        return ScopeApplication::Subsumes;
    }
    // Partial overlap: declared overlaps, or the evidence token sits below
    // the claim token (evidence narrower than claim).
    let some_overlap = evidence_tokens
        .iter()
        .any(|token| contains(claim_token, token) || overlaps.contains(&pair(token, claim_token)));
    if some_overlap {
        return ScopeApplication::PartialOverlap;
    }
    // Disjoint: every evidence token is disjoint from the claim token.
    let all_disjoint = !evidence_tokens.is_empty()
        && evidence_tokens
            .iter()
            .all(|token| disjoint.contains(&pair(token, claim_token)));
    if all_disjoint {
        return ScopeApplication::Disjoint;
    }
    ScopeApplication::Unassessable
}

pub(crate) fn compare_scope(
    model: &CausalModel,
    claim: &Claim,
    other_scope: &BTreeMap<String, Vec<String>>,
) -> ScopeApplication {
    let claim_scope = model.claim_scope(claim);
    if claim_scope.is_empty() || other_scope.is_empty() {
        return ScopeApplication::Unassessable;
    }
    let mut composed = ScopeApplication::Subsumes;
    for (dimension, claim_token) in claim_scope {
        let Some(tokens) = other_scope.get(dimension) else {
            composed = ScopeApplication::Unassessable;
            continue;
        };
        match scope_dimension_relation(model, dimension, claim_token, tokens) {
            ScopeApplication::Disjoint => return ScopeApplication::Disjoint,
            ScopeApplication::Unassessable => composed = ScopeApplication::Unassessable,
            ScopeApplication::PartialOverlap => {
                if composed == ScopeApplication::Subsumes {
                    composed = ScopeApplication::PartialOverlap;
                }
            }
            ScopeApplication::Subsumes => {}
        }
    }
    composed
}

fn scope_application(
    model: &CausalModel,
    claim: &Claim,
    artifact: &EvidenceArtifact,
) -> ScopeApplication {
    compare_scope(model, claim, &artifact.scope)
}

/// Convert an artifact's observation window to model timebase units.
/// `Ok(None)` means not evaluable (fractional/tick/missing).
fn window_in_timebase(model: &CausalModel, artifact: &EvidenceArtifact) -> Option<u64> {
    let (start, end) = (
        artifact.period_start.as_deref()?,
        artifact.period_end.as_deref()?,
    );
    let days = civil_days(end)? - civil_days(start)?;
    if days < 0 {
        return None;
    }
    #[allow(clippy::cast_sign_loss)]
    let days = days as u64;
    match model.timebase.as_str() {
        "day" => Some(days),
        "hour" => Some(days * 24),
        "week" => days.is_multiple_of(7).then_some(days / 7),
        _ => None,
    }
}

/// Root of an artifact's source lineage: the transitive `derived_from` root
/// when present, else the `source_study_id`, else the artifact's own id.
fn lineage_root(
    artifact: &EvidenceArtifact,
    artifacts: &BTreeMap<String, EvidenceArtifact>,
) -> Result<String, EvidenceError> {
    let mut current = artifact.evidence_id.clone();
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current.clone()) {
            return Err(error(
                "causal_evidence_lifecycle_mismatch",
                format!("derived_from cycle involving evidence '{current}'"),
            ));
        }
        let Some(entry) = artifacts.get(&current) else {
            return Err(error(
                "causal_evidence_schema_mismatch",
                format!("derived_from references unknown evidence '{current}'"),
            ));
        };
        if let Some(parent) = entry.derived_from.first() {
            current = parent.clone();
        } else {
            return Ok(entry
                .source_study_id
                .clone()
                .unwrap_or_else(|| format!("evidence:{current}")));
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn evidence_finding(
    finding_type: &str,
    involved: Vec<String>,
    witness: Value,
    why: String,
) -> Value {
    let anchor = involved.first().cloned().unwrap_or_default();
    json!({
        "finding_id": format!("causal-finding:{finding_type}:{}", anchor.replace(':', "-")),
        "analysis": "causal_evidence",
        "finding_type": finding_type,
        "severity": "review_required",
        "formal_status": "not_a_violation",
        "involved_nodes": involved,
        "witness": witness,
        "why_it_matters": why,
        "do_not_assume": [
            "The claim is false",
            "The evidence is worthless",
            "Support or challenge is universal causal proof"
        ],
    })
}

/// Aggregate deterministic per-claim `causal_support` from validated
/// artifacts plus applicability findings. Nothing here touches
/// `formal_assurance`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn aggregate_support(
    model: &CausalModel,
    artifacts: &BTreeMap<String, EvidenceArtifact>,
    as_of: Option<&str>,
) -> SupportOverlay {
    let mut findings: Vec<Value> = Vec::new();
    let mut not_evaluable: Vec<Value> = Vec::new();
    let mut applicability: Vec<Applicability> = Vec::new();
    // claim id -> lineage root -> applicable support values.
    let mut votes: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    let mut referenced: BTreeMap<String, bool> = BTreeMap::new();
    let mut lineage_members: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for artifact in artifacts.values() {
        if let Ok(root) = lineage_root(artifact, artifacts) {
            lineage_members
                .entry(root)
                .or_default()
                .push(artifact.evidence_id.clone());
        }
    }
    for (root, members) in &lineage_members {
        if members.len() > 1 {
            findings.push(evidence_finding(
                "duplicate_evidence_source",
                members
                    .iter()
                    .map(|member| format!("evidence:{member}"))
                    .collect(),
                json!({"lineage_root": root, "artifacts": members}),
                "multiple artifacts share one source lineage and do not count as independent evidence".to_owned(),
            ));
        }
    }
    for artifact in artifacts.values() {
        let evidence_node = format!("evidence:{}", artifact.evidence_id);
        if artifact.lifecycle_status == LifecycleStatus::Unknown {
            findings.push(evidence_finding(
                "unknown_lifecycle",
                vec![evidence_node.clone()],
                json!({"reason": "no matching lifecycle chain"}),
                "without a lifecycle chain the artifact's current status cannot be established"
                    .to_owned(),
            ));
        }
        if matches!(
            artifact.lifecycle_status,
            LifecycleStatus::Retracted | LifecycleStatus::Superseded
        ) {
            findings.push(evidence_finding(
                "retracted_or_superseded_evidence",
                vec![evidence_node.clone()],
                json!({"lifecycle_status": artifact.lifecycle_status.as_str()}),
                "the artifact remains in history but is not current".to_owned(),
            ));
        }
        if artifact.valid_until.is_none() {
            findings.push(evidence_finding(
                "unknown_freshness",
                vec![evidence_node.clone()],
                json!({"reason": "no valid_until"}),
                "missing valid_until is not indefinite validity; the artifact is excluded from current support".to_owned(),
            ));
        }
        let stale = match (artifact.valid_until.as_deref(), as_of) {
            (Some(valid_until), Some(as_of)) => {
                match (civil_days(valid_until), civil_days(as_of)) {
                    (Some(until), Some(now)) => until < now,
                    _ => false,
                }
            }
            _ => false,
        };
        if stale {
            findings.push(evidence_finding(
                "stale_evidence",
                vec![evidence_node.clone()],
                json!({"valid_until": artifact.valid_until, "as_of": as_of}),
                "the artifact's validity window ended before the as-of date".to_owned(),
            ));
        }
        for (claim_id, pinned_version) in &artifact.claims {
            let claim_node = format!("claim:{claim_id}");
            let Some(claim) = model.claims.get(claim_id) else {
                findings.push(evidence_finding(
                    "evidence_claim_mismatch",
                    vec![evidence_node.clone(), claim_node],
                    json!({"pinned": claim_id}),
                    "the artifact references a claim that does not exist in this model".to_owned(),
                ));
                continue;
            };
            referenced.insert(claim_id.clone(), true);
            let mut exclusions: Vec<&'static str> = Vec::new();
            if artifact.lifecycle_status == LifecycleStatus::Unknown {
                exclusions.push("unknown_lifecycle");
            }
            if matches!(
                artifact.lifecycle_status,
                LifecycleStatus::Retracted | LifecycleStatus::Superseded
            ) {
                exclusions.push("retracted_or_superseded_evidence");
            }
            if artifact.valid_until.is_none() {
                exclusions.push("unknown_freshness");
            }
            if stale {
                exclusions.push("stale_evidence");
            }
            if *pinned_version != claim.version {
                exclusions.push("evidence_claim_version_mismatch");
                findings.push(evidence_finding(
                    "evidence_claim_version_mismatch",
                    vec![evidence_node.clone(), format!("claim:{claim_id}")],
                    json!({"pinned_version": pinned_version, "current_version": claim.version}),
                    "the artifact is historical evidence for an older claim version".to_owned(),
                ));
            }
            let scope = scope_application(model, claim, artifact);
            match scope {
                ScopeApplication::Subsumes => {}
                ScopeApplication::PartialOverlap => {
                    exclusions.push("evidence_scope_partial_overlap");
                    findings.push(evidence_finding(
                        "evidence_scope_partial_overlap",
                        vec![evidence_node.clone(), format!("claim:{claim_id}")],
                        json!({"relation": "partial_overlap"}),
                        "the evidence scope only partially overlaps the claim scope; transportability review is required".to_owned(),
                    ));
                }
                ScopeApplication::Disjoint => {
                    exclusions.push("evidence_scope_mismatch");
                    findings.push(evidence_finding(
                        "evidence_scope_mismatch",
                        vec![evidence_node.clone(), format!("claim:{claim_id}")],
                        json!({"relation": "disjoint"}),
                        "the evidence scope is disjoint from the claim scope".to_owned(),
                    ));
                }
                ScopeApplication::Unassessable => {
                    exclusions.push("evidence_scope_unassessable");
                    findings.push(evidence_finding(
                        "evidence_scope_unassessable",
                        vec![evidence_node.clone(), format!("claim:{claim_id}")],
                        json!({"relation": "unassessable"}),
                        "the scope relation cannot be determined from declared tokens".to_owned(),
                    ));
                }
            }
            // Observation window vs minimum lag.
            let window = window_in_timebase(model, artifact);
            match (window, claim.lag) {
                (Some(window_units), Lag::Known(lag)) => {
                    if window_units < lag.min {
                        exclusions.push("evidence_window_shorter_than_lag");
                        findings.push(evidence_finding(
                            "evidence_window_shorter_than_lag",
                            vec![evidence_node.clone(), format!("claim:{claim_id}")],
                            json!({
                                "period": {"start": artifact.period_start, "end": artifact.period_end},
                                "conversion": format!("1 {} timebase units", model.timebase),
                                "window": window_units,
                                "lag": {"min": lag.min, "max": lag.max},
                                "claim": {"id": claim_id, "version": claim.version},
                            }),
                            "the observation period is shorter than the claim's minimum lag, so the effect could not have been observed".to_owned(),
                        ));
                    }
                }
                (None, _) | (_, Lag::Unknown) => {
                    not_evaluable.push(json!({
                        "finding_type": "evidence_window_shorter_than_lag",
                        "reason": if window.is_none() {
                            "period missing or not convertible to the model timebase"
                        } else {
                            "claim lag is unknown"
                        },
                        "involved_nodes": [evidence_node.clone(), format!("claim:{claim_id}")],
                    }));
                }
            }
            let applicable = exclusions.is_empty();
            if applicable {
                let root = lineage_root(artifact, artifacts)
                    .unwrap_or_else(|_| format!("evidence:{}", artifact.evidence_id));
                votes
                    .entry(claim_id.clone())
                    .or_default()
                    .entry(root)
                    .or_default()
                    .insert(artifact.support.clone());
            }
            applicability.push(Applicability {
                evidence_id: artifact.evidence_id.clone(),
                claim_id: claim_id.clone(),
                applicable,
                exclusions,
                scope,
                window,
                not_evaluable: Vec::new(),
            });
        }
    }
    // Aggregate per claim.
    let mut support = BTreeMap::new();
    for (claim_id, claim) in &model.claims {
        let lineages = votes.get(claim_id);
        let mut collapsed: BTreeSet<&str> = BTreeSet::new();
        if let Some(lineages) = lineages {
            for supports in lineages.values() {
                if let [only] = supports.iter().collect::<Vec<_>>().as_slice() {
                    collapsed.insert(only.as_str());
                } else {
                    // Contradictory support inside one lineage.
                    collapsed.insert("inconclusive");
                    findings.push(evidence_finding(
                        "conflicting_evidence",
                        vec![format!("claim:{claim_id}")],
                        json!({"reason": "one source lineage carries contradictory support values"}),
                        "a single source lineage both supports and challenges the claim; review the lineage".to_owned(),
                    ));
                }
            }
        }
        let value = if collapsed.is_empty() {
            if referenced.contains_key(claim_id) || !claim.evidence.is_empty() {
                "unsupported_by_current_evidence"
            } else {
                "untested"
            }
        } else {
            let supports = collapsed.contains("supports");
            let challenges = collapsed.contains("challenges");
            match (supports, challenges) {
                (true, true) => "mixed",
                (true, false) => "supported",
                (false, true) => "challenged",
                (false, false) => "inconclusive",
            }
        };
        if value == "mixed" {
            findings.push(evidence_finding(
                "conflicting_evidence",
                vec![format!("claim:{claim_id}")],
                json!({"support_values": collapsed.iter().collect::<Vec<_>>()}),
                "supporting and challenging evidence coexist for this claim".to_owned(),
            ));
        }
        if value == "supported" {
            let all_observational = artifacts.values().all(|artifact| {
                !artifact
                    .claims
                    .iter()
                    .any(|(id, _)| id == claim_id && artifact.support == "supports")
                    || artifact.design == "observational"
            });
            let any_supporting = artifacts.values().any(|artifact| {
                artifact.support == "supports"
                    && artifact.claims.iter().any(|(id, _)| id == claim_id)
            });
            if any_supporting && all_observational {
                findings.push(evidence_finding(
                    "observational_only_support",
                    vec![format!("claim:{claim_id}")],
                    json!({"design": "observational"}),
                    "support rests on observational studies only; additional assumptions are required".to_owned(),
                ));
            }
        }
        support.insert(claim_id.clone(), value.to_owned());
    }
    SupportOverlay {
        support,
        applicability,
        findings,
        not_evaluable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamped(mut artifact: Value) -> Value {
        let digest = artifact_digest(&artifact);
        artifact
            .as_object_mut()
            .expect("object")
            .insert("artifact_digest".to_owned(), json!(digest));
        artifact
    }

    fn base_artifact() -> Value {
        stamped(json!({
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "evidence_id": "E1",
            "claims": [{"id": "claim:C1", "version": 1}],
            "design": "randomized_experiment",
            "support": "supports",
            "scope": {"population": ["all_users"]},
            "period": {"start": "2026-01-01", "end": "2026-03-31", "valid_until": "2027-03-31"},
            "observation": null,
            "formal_result": "not_run"
        }))
    }

    #[test]
    fn artifact_round_trips_and_digest_is_checked() {
        let artifact = parse_artifact(&base_artifact()).expect("valid artifact");
        assert_eq!(artifact.evidence_id, "E1");
        assert_eq!(artifact.claims, vec![("C1".to_owned(), 1)]);
        let mut tampered = base_artifact();
        tampered["support"] = json!("challenges");
        let error = parse_artifact(&tampered).expect_err("digest must fail");
        assert_eq!(error.kind, "causal_evidence_digest_mismatch");
    }

    #[test]
    fn unknown_design_and_schema_version_are_rejected() {
        let mut artifact = base_artifact();
        artifact["design"] = json!("vibes");
        let error = parse_artifact(&stamped(artifact)).expect_err("closed enum");
        assert_eq!(error.kind, "causal_evidence_schema_mismatch");
        let mut artifact = base_artifact();
        artifact["schema_version"] = json!("fsl-causal-evidence.v9");
        let error = parse_artifact(&stamped(artifact)).expect_err("schema version");
        assert_eq!(error.kind, "causal_evidence_schema_mismatch");
    }

    #[test]
    fn observation_forces_observational_inconclusive() {
        let observation = json!({
            "kind": "expectation_replay",
            "expectation_id": "expectation:E_Obs",
            "expectation_digest": "sha256:00",
            "verdict": "pass",
            "assurance": "replay-observed",
            "event_counts": {"observed": 10, "unmapped": 0, "missing_required": 0},
            "digests": {"model": "sha256:00", "log": "sha256:00", "mapping": "sha256:00", "study_protocol": null}
        });
        let mut artifact = base_artifact();
        artifact["observation"] = observation.clone();
        let error = parse_artifact(&stamped(artifact)).expect_err("design must be observational");
        assert!(error.message.contains("observational"));
        let mut artifact = base_artifact();
        artifact["observation"] = observation.clone();
        artifact["design"] = json!("observational");
        let error = parse_artifact(&stamped(artifact)).expect_err("directed support rejected");
        assert!(error.message.contains("inconclusive"));
        let mut artifact = base_artifact();
        artifact["observation"] = observation;
        artifact["design"] = json!("observational");
        artifact["support"] = json!("inconclusive");
        parse_artifact(&stamped(artifact)).expect("observational inconclusive accepted");
        // Unknown observation field is a schema error.
        let mut artifact = base_artifact();
        artifact["observation"] =
            json!({"kind": "expectation_replay", "assurance": "replay-observed", "extra": 1});
        artifact["design"] = json!("observational");
        artifact["support"] = json!("inconclusive");
        let error = parse_artifact(&stamped(artifact)).expect_err("closed observation");
        assert!(error.message.contains("unknown observation field"));
    }

    fn lifecycle_chain(
        evidence_id: &str,
        digest: &str,
        statuses: &[(&str, Option<&str>)],
    ) -> Value {
        let mut records = Vec::new();
        let mut previous: Option<String> = None;
        for (index, (status, superseded_by)) in statuses.iter().enumerate() {
            let mut record = json!({
                "sequence": index + 1,
                "status": status,
                "superseded_by": superseded_by.map(str::to_owned),
                "recorded_at": format!("2026-0{}-01T00:00:00Z", index + 1),
                "previous_record_digest": previous,
            });
            let record_digest = lifecycle_record_digest(evidence_id, digest, &record);
            record
                .as_object_mut()
                .expect("record")
                .insert("record_digest".to_owned(), json!(record_digest.clone()));
            previous = Some(record_digest);
            records.push(record);
        }
        json!({
            "schema_version": LIFECYCLE_SCHEMA_VERSION,
            "evidence_id": evidence_id,
            "artifact_digest": digest,
            "records": records,
        })
    }

    #[test]
    fn lifecycle_chain_validates_and_rejects_gaps_and_tampering() {
        let artifact = parse_artifact(&base_artifact()).expect("artifact");
        let digest = artifact.declared_digest.clone();
        let mut artifacts = BTreeMap::from([("E1".to_owned(), artifact)]);
        let chain = lifecycle_chain("E1", &digest, &[("active", None)]);
        let (id, status) = validate_lifecycle_chain(&chain, &artifacts).expect("valid chain");
        assert_eq!(id, "E1");
        assert_eq!(status, LifecycleStatus::Active);
        // Retraction chain.
        let chain = lifecycle_chain("E1", &digest, &[("active", None), ("retracted", None)]);
        let (_, status) = validate_lifecycle_chain(&chain, &artifacts).expect("valid chain");
        assert_eq!(status, LifecycleStatus::Retracted);
        // Sequence gap.
        let mut broken = lifecycle_chain("E1", &digest, &[("active", None), ("retracted", None)]);
        broken["records"][1]["sequence"] = json!(3);
        let error = validate_lifecycle_chain(&broken, &artifacts).expect_err("gap");
        assert_eq!(error.kind, "causal_evidence_lifecycle_mismatch");
        // Digest tampering.
        let mut tampered = lifecycle_chain("E1", &digest, &[("active", None)]);
        tampered["records"][0]["status"] = json!("retracted");
        let error = validate_lifecycle_chain(&tampered, &artifacts).expect_err("tamper");
        assert_eq!(error.kind, "causal_evidence_lifecycle_mismatch");
        // superseded_by must resolve.
        let chain = lifecycle_chain("E1", &digest, &[("superseded", Some("E9"))]);
        let error = validate_lifecycle_chain(&chain, &artifacts).expect_err("unresolved");
        assert!(error.message.contains("E9"));
        // Record after terminal is rejected.
        let chain = lifecycle_chain("E1", &digest, &[("retracted", None), ("active", None)]);
        let error = validate_lifecycle_chain(&chain, &artifacts).expect_err("terminal");
        assert!(error.message.contains("terminal"));
        // Supersession resolving to a supplied artifact passes.
        let mut second = base_artifact();
        second["evidence_id"] = json!("E2");
        let second = parse_artifact(&stamped({
            let mut copy = second;
            copy.as_object_mut()
                .expect("object")
                .remove("artifact_digest");
            copy
        }))
        .expect("second artifact");
        artifacts.insert("E2".to_owned(), second);
        let chain = lifecycle_chain("E1", &digest, &[("superseded", Some("E2"))]);
        let (_, status) = validate_lifecycle_chain(&chain, &artifacts).expect("supersession");
        assert_eq!(status, LifecycleStatus::Superseded);
    }

    #[test]
    fn civil_days_handles_gregorian_dates() {
        assert_eq!(civil_days("1970-01-01"), Some(0));
        assert_eq!(civil_days("1970-01-02"), Some(1));
        assert_eq!(
            civil_days("2026-03-31").unwrap() - civil_days("2026-01-01").unwrap(),
            89
        );
        assert_eq!(civil_days("2026-13-01"), None);
        assert_eq!(civil_days("garbage"), None);
    }
}

/// `--projection causal_evidence_graph` (`causal-evidence-graph.v0`): the
/// claim/evidence relation graph with the per-claim support overlay. Claims
/// always carry `formal_assurance: "not_run"` — the two axes never merge.
#[must_use]
pub fn causal_evidence_graph(
    model: &CausalModel,
    artifacts: &BTreeMap<String, EvidenceArtifact>,
    overlay: &SupportOverlay,
) -> Value {
    let claims: Vec<Value> = model
        .claims
        .values()
        .map(|claim| {
            json!({
                "id": format!("claim:{}", claim.id),
                "version": claim.version,
                "status": if claim.status == crate::causal::ClaimStatus::Active { "active" } else { "retired" },
                "formal_assurance": "not_run",
                "causal_support": overlay.support.get(&claim.id).cloned().unwrap_or_else(|| "untested".to_owned()),
            })
        })
        .collect();
    let evidence: Vec<Value> = artifacts
        .values()
        .map(|artifact| {
            json!({
                "id": format!("evidence:{}", artifact.evidence_id),
                "design": artifact.design,
                "support": artifact.support,
                "lifecycle_status": artifact.lifecycle_status.as_str(),
                "source_study_id": artifact.source_study_id,
                "derived_from": artifact.derived_from,
                "observation": artifact.observation.as_ref().map(|observation| observation.get("kind").cloned().unwrap_or(Value::Null)),
            })
        })
        .collect();
    let edges: Vec<Value> = overlay
        .applicability
        .iter()
        .map(|entry| {
            let artifact = &artifacts[&entry.evidence_id];
            json!({
                "source": format!("evidence:{}", entry.evidence_id),
                "target": format!("claim:{}", entry.claim_id),
                "kind": artifact.support,
                "applicable": entry.applicable,
                "scope_relation": entry.scope.as_str(),
                "exclusions": entry.exclusions,
            })
        })
        .collect();
    json!({
        "result": "causal_analyzed",
        "schema_version": "causal-evidence-graph.v0",
        "formal_result": "not_run",
        "model": model.name,
        "projection": "causal_evidence_graph",
        "claims": claims,
        "evidence": evidence,
        "edges": edges,
        "findings": overlay.findings,
        "not_evaluable": overlay.not_evaluable,
        "do_not_assume": [
            "The causal claims are true",
            "causal_support is a formal assurance class",
            "Supported means proved; challenged means refuted",
            "Evidence applicability establishes study quality"
        ],
    })
}

#[cfg(test)]
mod aggregation_tests {
    use super::*;
    use crate::causal::tests::{VALID_MODEL, build};

    fn stamped(mut artifact: Value) -> Value {
        let digest = artifact_digest(&artifact);
        artifact
            .as_object_mut()
            .expect("object")
            .insert("artifact_digest".to_owned(), json!(digest));
        artifact
    }

    fn artifact(id: &str, claim: &str, version: u64, support: &str) -> EvidenceArtifact {
        let mut parsed = parse_artifact(&stamped(json!({
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "evidence_id": id,
            "claims": [{"id": claim, "version": version}],
            "design": "randomized_experiment",
            "support": support,
            "scope": {"population": ["all_users"]},
            "period": {"start": "2026-01-01", "end": "2026-03-31", "valid_until": "2027-03-31"},
            "observation": null,
            "formal_result": "not_run"
        })))
        .expect("artifact");
        parsed.lifecycle_status = LifecycleStatus::Active;
        parsed
    }

    fn overlay_for(artifacts: Vec<EvidenceArtifact>, as_of: Option<&str>) -> SupportOverlay {
        let (model, _) = build(VALID_MODEL).expect("model");
        let map: BTreeMap<String, EvidenceArtifact> = artifacts
            .into_iter()
            .map(|artifact| (artifact.evidence_id.clone(), artifact))
            .collect();
        aggregate_support(&model, &map, as_of)
    }

    #[test]
    fn support_aggregation_follows_the_table() {
        // supports only -> supported.
        let overlay = overlay_for(vec![artifact("E1", "C_SupportHabit", 1, "supports")], None);
        assert_eq!(overlay.support["C_SupportHabit"], "supported");
        // Unreferenced claim with no evidence declaration -> untested.
        assert_eq!(overlay.support["C_HabitRetention"], "untested");
        // challenges only -> challenged.
        let overlay = overlay_for(
            vec![artifact("E1", "C_SupportHabit", 1, "challenges")],
            None,
        );
        assert_eq!(overlay.support["C_SupportHabit"], "challenged");
        // inconclusive only -> inconclusive.
        let overlay = overlay_for(
            vec![artifact("E1", "C_SupportHabit", 1, "inconclusive")],
            None,
        );
        assert_eq!(overlay.support["C_SupportHabit"], "inconclusive");
        // supports + challenges from independent lineages -> mixed + finding.
        let mut supporting = artifact("E1", "C_SupportHabit", 1, "supports");
        supporting.source_study_id = Some("S1".to_owned());
        let mut challenging = artifact("E2", "C_SupportHabit", 1, "challenges");
        challenging.source_study_id = Some("S2".to_owned());
        let overlay = overlay_for(vec![supporting, challenging], None);
        assert_eq!(overlay.support["C_SupportHabit"], "mixed");
        assert!(
            overlay
                .findings
                .iter()
                .any(|finding| finding["finding_type"] == "conflicting_evidence")
        );
    }

    #[test]
    fn inapplicable_only_evidence_is_unsupported_by_current_evidence() {
        // Version mismatch: artifact pins v2, current claim is v1.
        let overlay = overlay_for(vec![artifact("E1", "C_SupportHabit", 2, "supports")], None);
        assert_eq!(
            overlay.support["C_SupportHabit"],
            "unsupported_by_current_evidence"
        );
        assert!(
            overlay
                .findings
                .iter()
                .any(|finding| finding["finding_type"] == "evidence_claim_version_mismatch")
        );
        // The declared-but-unmatched evidence reference on the claim also
        // yields unsupported (VALID_MODEL's C_SupportHabit declares E1).
        let overlay = overlay_for(Vec::new(), None);
        assert_eq!(
            overlay.support["C_SupportHabit"],
            "unsupported_by_current_evidence"
        );
    }

    #[test]
    fn staleness_requires_the_explicit_as_of_date() {
        let fresh = overlay_for(vec![artifact("E1", "C_SupportHabit", 1, "supports")], None);
        assert_eq!(fresh.support["C_SupportHabit"], "supported");
        let stale = overlay_for(
            vec![artifact("E1", "C_SupportHabit", 1, "supports")],
            Some("2028-01-01"),
        );
        assert_eq!(
            stale.support["C_SupportHabit"],
            "unsupported_by_current_evidence"
        );
        assert!(
            stale
                .findings
                .iter()
                .any(|finding| finding["finding_type"] == "stale_evidence")
        );
        // Still valid at the as-of date -> supported.
        let valid = overlay_for(
            vec![artifact("E1", "C_SupportHabit", 1, "supports")],
            Some("2027-03-31"),
        );
        assert_eq!(valid.support["C_SupportHabit"], "supported");
    }

    #[test]
    fn missing_freshness_and_unknown_lifecycle_exclude_the_artifact() {
        let mut no_freshness = parse_artifact(&stamped(json!({
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "evidence_id": "E1",
            "claims": [{"id": "C_SupportHabit", "version": 1}],
            "design": "randomized_experiment",
            "support": "supports",
            "scope": {"population": ["all_users"]},
            "period": {"start": "2026-01-01", "end": "2026-03-31"},
            "observation": null,
            "formal_result": "not_run"
        })))
        .expect("artifact");
        no_freshness.lifecycle_status = LifecycleStatus::Active;
        let overlay = overlay_for(vec![no_freshness], None);
        assert_eq!(
            overlay.support["C_SupportHabit"],
            "unsupported_by_current_evidence"
        );
        assert!(
            overlay
                .findings
                .iter()
                .any(|finding| finding["finding_type"] == "unknown_freshness")
        );
        // Unknown lifecycle (no chain supplied).
        let unknown = artifact("E1", "C_SupportHabit", 1, "supports");
        let mut unknown = unknown;
        unknown.lifecycle_status = LifecycleStatus::Unknown;
        let overlay = overlay_for(vec![unknown], None);
        assert_eq!(
            overlay.support["C_SupportHabit"],
            "unsupported_by_current_evidence"
        );
        assert!(
            overlay
                .findings
                .iter()
                .any(|finding| finding["finding_type"] == "unknown_lifecycle")
        );
    }

    #[test]
    fn one_lineage_collapses_to_one_vote() {
        let mut first = artifact("E1", "C_SupportHabit", 1, "supports");
        first.source_study_id = Some("STUDY".to_owned());
        let mut second = artifact("E2", "C_SupportHabit", 1, "supports");
        second.source_study_id = Some("STUDY".to_owned());
        let overlay = overlay_for(vec![first, second], None);
        assert_eq!(overlay.support["C_SupportHabit"], "supported");
        assert!(
            overlay
                .findings
                .iter()
                .any(|finding| finding["finding_type"] == "duplicate_evidence_source")
        );
        // Contradiction inside one lineage -> inconclusive, not mixed.
        let mut first = artifact("E1", "C_SupportHabit", 1, "supports");
        first.source_study_id = Some("STUDY".to_owned());
        let mut second = artifact("E2", "C_SupportHabit", 1, "challenges");
        second.source_study_id = Some("STUDY".to_owned());
        let overlay = overlay_for(vec![first, second], None);
        assert_eq!(overlay.support["C_SupportHabit"], "inconclusive");
        assert!(
            overlay
                .findings
                .iter()
                .any(|finding| finding["finding_type"] == "conflicting_evidence")
        );
    }

    #[test]
    fn observation_window_shorter_than_lag_is_excluded_with_boundaries() {
        // C_SupportHabit lag 7..30 (days). Window of 6 days < 7 -> excluded.
        let mut short = parse_artifact(&stamped(json!({
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "evidence_id": "E1",
            "claims": [{"id": "C_SupportHabit", "version": 1}],
            "design": "randomized_experiment",
            "support": "supports",
            "scope": {"population": ["all_users"]},
            "period": {"start": "2026-01-01", "end": "2026-01-07", "valid_until": "2027-03-31"},
            "observation": null,
            "formal_result": "not_run"
        })))
        .expect("artifact");
        short.lifecycle_status = LifecycleStatus::Active;
        let overlay = overlay_for(vec![short], None);
        assert_eq!(
            overlay.support["C_SupportHabit"],
            "unsupported_by_current_evidence"
        );
        let finding = overlay
            .findings
            .iter()
            .find(|finding| finding["finding_type"] == "evidence_window_shorter_than_lag")
            .expect("window finding");
        assert_eq!(finding["witness"]["window"], 6);
        assert_eq!(finding["witness"]["lag"]["min"], 7);
        // Window == lag.min (7 days) -> no finding.
        let mut exact = parse_artifact(&stamped(json!({
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "evidence_id": "E1",
            "claims": [{"id": "C_SupportHabit", "version": 1}],
            "design": "randomized_experiment",
            "support": "supports",
            "scope": {"population": ["all_users"]},
            "period": {"start": "2026-01-01", "end": "2026-01-08", "valid_until": "2027-03-31"},
            "observation": null,
            "formal_result": "not_run"
        })))
        .expect("artifact");
        exact.lifecycle_status = LifecycleStatus::Active;
        let overlay = overlay_for(vec![exact], None);
        assert_eq!(overlay.support["C_SupportHabit"], "supported");
    }

    #[test]
    fn observational_only_support_is_flagged_but_still_supported() {
        let value = stamped(json!({
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "evidence_id": "E1",
            "claims": [{"id": "C_SupportHabit", "version": 1}],
            "design": "observational",
            "support": "supports",
            "scope": {"population": ["all_users"]},
            "period": {"start": "2026-01-01", "end": "2026-03-31", "valid_until": "2027-03-31"},
            "observation": null,
            "formal_result": "not_run"
        }));
        let mut parsed = parse_artifact(&value).expect("artifact");
        parsed.lifecycle_status = LifecycleStatus::Active;
        let overlay = overlay_for(vec![parsed], None);
        assert_eq!(overlay.support["C_SupportHabit"], "supported");
        assert!(
            overlay
                .findings
                .iter()
                .any(|finding| finding["finding_type"] == "observational_only_support")
        );
    }
}
