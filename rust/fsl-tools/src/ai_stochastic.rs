// SPDX-License-Identifier: Apache-2.0

//! Executes declared fsl-ai project evidence policy (issues #509/#510):
//! `statistical_property` (via `fslc ai eval`), `ai_migration.no_regression`
//! (via `fslc ai regress`), and `observed_property` (via `fslc ai drift`).
//! Every result is schema-conformant with
//! `schemas/fslc/ai/statistical-result.v0.schema.json` and
//! `docs/DESIGN-stochastic.md`'s status priority; `formal_result` is always
//! `"not_run"` (`docs/DESIGN-stochastic.md`: this layer is external
//! evidence, never a kernel proof).

use fsl_syntax::{
    AiMigration, AiObservedProperty, AiObservedRequirement, AiRegressionRequirement,
    AiStatisticalProperty,
};
use serde_json::{Value, json};

const STATISTICAL_RESULT_SCHEMA_VERSION: &str = "fsl-ai-statistical-result.v0";
const STATUS_PRIORITY: [&str; 7] = [
    "dataset_invalid",
    "evaluator_untrusted",
    "slice_missing",
    "insufficient_samples",
    "inconclusive",
    "statistically_unsupported",
    "statistically_supported",
];

/// Evaluate a selected `statistical_property` against precomputed eval JSONL
/// records, applying every declared slice's `min_samples`/`ci_lower`/
/// `ci_upper` gate (`docs/DESIGN-stochastic.md`'s status priority).
///
/// # Errors
///
/// Returns a message when no declared requirement matches the requested
/// slice/property (a selection error, not a statistical gate failure).
pub fn evaluate_statistical_property(
    prop: &AiStatisticalProperty,
    records: &[Value],
    dataset: &str,
    slice_filter: Option<&str>,
) -> Result<Value, String> {
    let checks = evaluate_statistical_requirements(records, prop, dataset, slice_filter)?;
    let status = overall_status(&checks);
    let primary = primary_check(&checks);
    let findings = checks
        .iter()
        .filter(|check| check["status"] != "statistically_supported")
        .map(|check| statistical_finding(prop, check))
        .collect::<Vec<_>>();
    Ok(json!({
        "schema_version": STATISTICAL_RESULT_SCHEMA_VERSION,
        "fsl": "fsl-stochastic.v0",
        "result": status,
        "status": status,
        "formal_result": "not_run",
        "target": prop.target,
        "property": prop.name,
        "dataset": dataset,
        "slice": primary["slice"],
        "metric": primary["metric"],
        "n": primary["n"],
        "estimate": primary["estimate"],
        "interval": primary["interval"],
        "threshold": primary["threshold"],
        "evaluator": evaluator_summary(records, prop.evaluator.as_deref()),
        "checks": checks,
        "assumptions": statistical_assumptions(),
        "findings": findings,
    }))
}

/// Evaluate a selected `ai_migration`'s declared `no_regression` metric
/// clauses over before/after precomputed eval JSONL.
///
/// # Errors
///
/// Returns a message when the migration declares no `no_regression` metric
/// clause at all.
pub fn evaluate_migration(
    migration: &AiMigration,
    before: &[Value],
    after: &[Value],
    dataset: Option<&str>,
) -> Result<Value, String> {
    if migration.regression_requirements.is_empty() {
        return Err(format!(
            "ai_migration '{}' has no no_regression metric clauses",
            migration.name
        ));
    }
    let checks = migration
        .regression_requirements
        .iter()
        .map(|req| {
            evaluate_regression_requirement(before, after, req, dataset.or(req.dataset.as_deref()))
        })
        .collect::<Vec<_>>();
    let failed = checks
        .iter()
        .filter(|check| check["passed"] == false)
        .collect::<Vec<_>>();
    let status = if failed.is_empty() {
        "statistically_supported"
    } else {
        "statistically_unsupported"
    };
    let findings = failed
        .iter()
        .map(|check| migration_finding(migration, check))
        .collect::<Vec<_>>();
    let resolved_dataset = dataset
        .map(str::to_owned)
        .or_else(|| migration.regression_requirements[0].dataset.clone());
    Ok(json!({
        "schema_version": "fsl-ai-migration-result.v0",
        "fsl": "fsl-ai-migration.v0",
        "result": status,
        "status": status,
        "formal_result": "not_run",
        "migration": migration.name,
        "dataset": resolved_dataset,
        "checks": checks,
        "assumptions": regression_assumptions(),
        "findings": findings,
    }))
}

/// Evaluate a selected `observed_property`'s declared `observed`/`drift`
/// requirements over runtime telemetry JSONL.
#[must_use]
pub fn evaluate_observed_property(
    prop: &AiObservedProperty,
    current: &[Value],
    baseline: &[Value],
    window: Option<&str>,
    baseline_label: Option<&str>,
) -> Value {
    let checks = prop
        .requirements
        .iter()
        .map(|req| evaluate_observed_requirement(current, baseline, req))
        .collect::<Vec<_>>();
    let failed = checks.iter().any(|check| check["passed"] == false);
    let status = if failed {
        "observed_mismatch"
    } else {
        "observed_supported"
    };
    let findings = checks
        .iter()
        .filter(|check| check["passed"] == false)
        .map(|check| observed_finding(prop, check))
        .collect::<Vec<_>>();
    json!({
        "schema_version": "fsl-ai-observed-result.v0",
        "fsl": "fsl-ai-observed.v0",
        "result": status,
        "formal_result": "not_run",
        "target": prop.target,
        "property": prop.name,
        "window": window.map(str::to_owned).or_else(|| prop.window.clone()),
        "baseline": baseline_label,
        "checks": checks,
        "assumptions": observed_assumptions(),
        "findings": findings,
    })
}

// --- record predicates (mirror `_record_*` in the frozen reference) -------

fn record_dataset(record: &Value, dataset_name: &str) -> bool {
    match record.get("dataset") {
        None | Some(Value::Null) => true,
        Some(Value::String(value)) => value == dataset_name,
        Some(_) => false,
    }
}

fn record_slice(record: &Value) -> String {
    record
        .get("slice")
        .and_then(Value::as_str)
        .unwrap_or("all")
        .to_owned()
}

fn record_metric(record: &Value) -> String {
    record
        .get("metric")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn record_outcome(record: &Value) -> bool {
    record
        .get("outcome")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The first `(case_id, slice, metric)` collision, or a record missing one
/// of those required fields; `None` if the dataset is well-formed
/// (`docs/DESIGN-stochastic.md`: "A missing required slice field is
/// `dataset_invalid`. Duplicate `(case_id, slice, metric)` records are
/// `dataset_invalid`.").
fn duplicate_eval_key(records: &[Value]) -> Option<Value> {
    let mut seen = std::collections::BTreeSet::new();
    for record in records {
        let case_id = record
            .get("case_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let slice = record_slice(record);
        let metric = record_metric(record);
        if case_id.is_none() || slice.is_empty() || metric.is_empty() {
            return Some(json!({"case_id": case_id, "slice": slice, "metric": metric}));
        }
        let key = (case_id.unwrap(), slice, metric);
        if !seen.insert(key.clone()) {
            return Some(json!({"case_id": key.0, "slice": key.1, "metric": key.2}));
        }
    }
    None
}

// --- statistical_property evaluation ---------------------------------

fn evaluate_statistical_requirements(
    records: &[Value],
    prop: &AiStatisticalProperty,
    dataset_name: &str,
    slice_filter: Option<&str>,
) -> Result<Vec<Value>, String> {
    if let Some(duplicate) = duplicate_eval_key(records) {
        return Ok(vec![json!({
            "status": "dataset_invalid",
            "metric": duplicate["metric"],
            "slice": duplicate["slice"],
            "n": 0,
            "estimate": 0.0,
            "interval": empty_interval(prop.confidence),
            "threshold": {"operator": "none", "value": 0.0},
            "reason": "duplicate (case_id, slice, metric) record",
            "duplicate": duplicate,
        })]);
    }
    let requirements = prop
        .requirements
        .iter()
        .filter(|req| slice_filter.is_none_or(|slice| req.slice == slice))
        .collect::<Vec<_>>();
    if requirements.is_empty() {
        return Err("no statistical requirements matched the requested slice/property".to_owned());
    }
    Ok(requirements
        .into_iter()
        .map(|req| match req.kind.as_str() {
            "min_samples" => check_min_samples(records, req, dataset_name),
            "ci_lower" | "ci_upper" => {
                check_ci_requirement(records, req, dataset_name, prop.evaluator.as_deref())
            }
            "point_estimate" => {
                inconclusive_check(req, "point-estimate-only requirement is not accepted")
            }
            _ => inconclusive_check(req, "unsupported statistical requirement"),
        })
        .collect())
}

#[allow(clippy::cast_precision_loss)]
fn check_min_samples(
    records: &[Value],
    req: &fsl_syntax::AiMetricRequirement,
    dataset_name: &str,
) -> Value {
    let mut ids = std::collections::BTreeSet::new();
    for (index, record) in records.iter().enumerate() {
        if record_dataset(record, dataset_name) && record_slice(record) == req.slice {
            let id = record
                .get("case_id")
                .and_then(Value::as_str)
                .map_or_else(|| index.to_string(), str::to_owned);
            ids.insert(id);
        }
    }
    let n = ids.len();
    let comparator = req.comparator.as_deref().unwrap_or(">=");
    let threshold = req.min_samples.unwrap_or(0);
    let passed = compare(n as f64, comparator, threshold as f64);
    json!({
        "status": if passed {"statistically_supported"} else {"insufficient_samples"},
        "metric": "min_samples",
        "slice": req.slice,
        "n": n,
        "estimate": 0.0,
        "interval": empty_interval(req.confidence.unwrap_or(0.95)),
        "threshold": {"operator": format!("min_samples_{comparator}"), "value": threshold},
        "passed": passed,
        "requirement": req.source,
    })
}

#[allow(clippy::cast_precision_loss)]
fn check_ci_requirement(
    records: &[Value],
    req: &fsl_syntax::AiMetricRequirement,
    dataset_name: &str,
    evaluator: Option<&str>,
) -> Value {
    let metric = req.metric.clone().unwrap_or_else(|| "unknown".to_owned());
    let confidence = req.confidence.unwrap_or(0.95);
    let relevant = records
        .iter()
        .filter(|record| {
            record_dataset(record, dataset_name)
                && record_slice(record) == req.slice
                && record_metric(record) == metric
        })
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return json!({
            "status": "slice_missing",
            "metric": metric,
            "slice": req.slice,
            "n": 0,
            "estimate": 0.0,
            "interval": empty_interval(confidence),
            "threshold": threshold_json(req),
            "passed": false,
            "requirement": req.source,
            "reason": "no eval records matched dataset/slice/metric",
        });
    }
    let trust = trust_status(&relevant, evaluator);
    if trust != "trusted" {
        return json!({
            "status": "evaluator_untrusted",
            "metric": metric,
            "slice": req.slice,
            "n": relevant.len(),
            "estimate": 0.0,
            "interval": empty_interval(confidence),
            "threshold": threshold_json(req),
            "passed": false,
            "requirement": req.source,
            "evaluator": {"id": evaluator, "trust_status": trust},
        });
    }
    let successes = relevant
        .iter()
        .filter(|record| record_outcome(record))
        .count();
    let n = relevant.len();
    let estimate = successes as f64 / n as f64;
    let interval = wilson(successes, n, confidence);
    let observed = interval[if req.kind == "ci_lower" {
        "lower"
    } else {
        "upper"
    }]
    .as_f64()
    .unwrap_or(0.0);
    let passed = compare(
        observed,
        req.comparator.as_deref().unwrap_or(">="),
        req.threshold.unwrap_or(0.0),
    );
    json!({
        "status": if passed {"statistically_supported"} else {"statistically_unsupported"},
        "metric": metric,
        "slice": req.slice,
        "n": n,
        "successes": successes,
        "estimate": estimate,
        "interval": interval,
        "threshold": threshold_json(req),
        "observed_bound": observed,
        "passed": passed,
        "requirement": req.source,
    })
}

fn trust_status(records: &[&Value], evaluator: Option<&str>) -> String {
    if records.is_empty() {
        return "unknown".to_owned();
    }
    for record in records {
        let Some(evaluator_object) = record.get("evaluator").and_then(Value::as_object) else {
            return "unknown".to_owned();
        };
        if let Some(evaluator) = evaluator {
            match evaluator_object.get("id").and_then(Value::as_str) {
                None => {}
                Some(id) if id == evaluator => {}
                Some(_) => return "unknown".to_owned(),
            }
        }
        let status = evaluator_object
            .get("calibration_status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if status != "trusted" {
            return status.to_owned();
        }
    }
    "trusted".to_owned()
}

fn evaluator_summary(records: &[Value], evaluator: Option<&str>) -> Value {
    let refs = records.iter().collect::<Vec<_>>();
    let trust = trust_status(&refs, evaluator);
    let id = evaluator.map(str::to_owned).or_else(|| {
        records.iter().find_map(|record| {
            record
                .get("evaluator")
                .and_then(Value::as_object)
                .and_then(|evaluator| evaluator.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    });
    json!({"id": id, "trust_status": trust})
}

fn threshold_json(req: &fsl_syntax::AiMetricRequirement) -> Value {
    let operator = if req.kind == "ci_lower" {
        "ci_lower_gte"
    } else {
        "ci_upper_lte"
    };
    json!({"operator": operator, "value": req.threshold.unwrap_or(0.0)})
}

fn inconclusive_check(req: &fsl_syntax::AiMetricRequirement, reason: &str) -> Value {
    json!({
        "status": "inconclusive",
        "metric": req.metric.clone().unwrap_or_else(|| "unknown".to_owned()),
        "slice": req.slice,
        "n": 0,
        "estimate": 0.0,
        "interval": empty_interval(req.confidence.unwrap_or(0.95)),
        "threshold": if matches!(req.kind.as_str(), "ci_lower" | "ci_upper") {
            threshold_json(req)
        } else {
            json!({"operator": "none", "value": 0.0})
        },
        "passed": false,
        "requirement": req.source,
        "reason": reason,
    })
}

fn overall_status(checks: &[Value]) -> &'static str {
    let statuses = checks
        .iter()
        .filter_map(|check| check["status"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for status in STATUS_PRIORITY {
        if statuses.contains(status) {
            if status == "statistically_supported" && statuses.len() > 1 {
                continue;
            }
            return status;
        }
    }
    "inconclusive"
}

fn primary_check(checks: &[Value]) -> &Value {
    let status = overall_status(checks);
    checks
        .iter()
        .find(|check| check["status"] == status)
        .unwrap_or(&checks[0])
}

fn statistical_finding(prop: &AiStatisticalProperty, check: &Value) -> Value {
    json!({
        "schema_version": "fsl-ai-finding.v0",
        "fsl": "fsl-stochastic.v0",
        "result": check["status"],
        "kind": "statistical_contract_unsupported",
        "severity": "error",
        "component": prop.target,
        "contract": prop.name,
        "tool": Value::Null,
        "failed_rule": "statistical_property",
        "violation": check["status"],
        "guarantee_kind": "statistically_unsupported",
        "evidence": {"kind": "precomputed_eval_jsonl", "formal_proof": false},
        "witness": check,
        "minimal_conflict_set": {
            "property": prop.name,
            "dataset": prop.dataset,
            "slice": check["slice"],
            "metric": check["metric"],
        },
        "repair_candidates": [{
            "kind": "eval_data_or_model_change",
            "weakens_spec": false,
            "description": "add evidence, improve the component, or route the affected slice to fallback/human review",
        }],
        "assumptions": statistical_assumptions(),
    })
}

fn statistical_assumptions() -> Value {
    json!([
        {"id": "AI-ASSUME-PRECOMPUTED-EVAL-JSONL", "text": "eval records are precomputed Bernoulli observations"},
        {"id": "AI-ASSUME-SAMPLE-INDEPENDENCE", "text": "sample independence is dataset construction evidence and is not proved by fslc"},
        {"id": "AI-ASSUME-EVALUATOR-CALIBRATION-EVIDENCE", "text": "evaluator trust is supplied by calibration metadata or external evidence"},
        {"id": "AI-ASSUME-NO-STOCHASTIC-KERNEL-SEMANTICS", "text": "statistical support is external evidence and never a kernel proof"},
    ])
}

// --- ai_migration.no_regression evaluation ------------------------------

#[allow(clippy::cast_precision_loss)]
fn aggregate_metric(records: &[Value], metric: &str, dataset: Option<&str>) -> Value {
    let relevant = records
        .iter()
        .filter(|record| {
            dataset.is_none_or(|dataset| record_dataset(record, dataset))
                && record_metric(record) == metric
        })
        .collect::<Vec<_>>();
    let n = relevant.len();
    let successes = relevant
        .iter()
        .filter(|record| record_outcome(record))
        .count();
    let estimate = if n == 0 {
        0.0
    } else {
        successes as f64 / n as f64
    };
    json!({
        "n": n,
        "successes": successes,
        "estimate": estimate,
        "interval": if n > 0 { wilson(successes, n, 0.95) } else { empty_interval(0.95) },
    })
}

fn evaluate_regression_requirement(
    before: &[Value],
    after: &[Value],
    req: &AiRegressionRequirement,
    dataset: Option<&str>,
) -> Value {
    let before_metric = aggregate_metric(before, &req.metric, dataset);
    let after_metric = aggregate_metric(after, &req.metric, dataset);
    let before_estimate = before_metric["estimate"].as_f64().unwrap_or(0.0);
    let after_estimate = after_metric["estimate"].as_f64().unwrap_or(0.0);
    let delta = if req.direction == "drop" {
        before_estimate - after_estimate
    } else {
        after_estimate - before_estimate
    };
    let passed = compare(delta, &req.comparator, req.threshold);
    json!({
        "metric": req.metric,
        "direction": req.direction,
        "dataset": dataset,
        "before": before_metric,
        "after": after_metric,
        "observed_delta": delta,
        "allowed_delta": req.threshold,
        "comparator": req.comparator,
        "passed": passed,
    })
}

fn migration_finding(migration: &AiMigration, check: &Value) -> Value {
    json!({
        "schema_version": "fsl-ai-finding.v0",
        "fsl": "fsl-ai-migration.v0",
        "result": "statistically_unsupported",
        "kind": "ai_migration_regression",
        "severity": "error",
        "component": Value::Null,
        "contract": migration.name,
        "tool": Value::Null,
        "failed_rule": "no_regression",
        "violation": "ai_migration_regression",
        "guarantee_kind": "statistically_unsupported",
        "evidence": {"kind": "precomputed_eval_jsonl_compare", "formal_proof": false},
        "witness": check,
        "minimal_conflict_set": {"migration": migration.name, "metric": check["metric"]},
        "repair_candidates": [
            {"kind": "rollout_block", "weakens_spec": false, "description": "block or narrow rollout for the regressed metric/slice"},
            {"kind": "artifact_change", "weakens_spec": false, "description": "repair the prompt/model/retriever/tool schema change and re-run the regression evidence"},
        ],
        "assumptions": regression_assumptions(),
    })
}

fn regression_assumptions() -> Value {
    let mut assumptions = statistical_assumptions();
    if let Value::Array(items) = &mut assumptions {
        items.push(json!({
            "id": "AI-ASSUME-AGGREGATE-REGRESSION-COMPARISON",
            "text": "migration regression compares aggregate precomputed metrics unless paired case evidence is supplied separately",
        }));
    }
    assumptions
}

// --- observed_property evaluation ---------------------------------------

#[allow(clippy::cast_precision_loss)]
fn observed_metric(records: &[Value], metric: &str, slice_name: &str) -> Value {
    let values = records
        .iter()
        .filter(|record| {
            (slice_name == "all" || record_slice(record) == slice_name)
                && record_metric(record) == metric
                && record.get("outcome").is_some()
        })
        .map(record_outcome)
        .collect::<Vec<_>>();
    let n = values.len();
    if n == 0 {
        return json!({"n": 0, "estimate": 0.0});
    }
    let successes = values.iter().filter(|value| **value).count();
    json!({"n": n, "estimate": successes as f64 / n as f64})
}

fn evaluate_observed_requirement(
    current: &[Value],
    baseline: &[Value],
    req: &AiObservedRequirement,
) -> Value {
    match req.kind.as_str() {
        "observed" => {
            let current_metric = observed_metric(current, &req.metric, &req.slice);
            let passed = compare(
                current_metric["estimate"].as_f64().unwrap_or(0.0),
                &req.comparator,
                req.threshold,
            );
            json!({
                "kind": "observed", "metric": req.metric, "slice": req.slice,
                "current": current_metric, "threshold": req.threshold,
                "comparator": req.comparator, "passed": passed, "requirement": req.source,
            })
        }
        "drift" => {
            let current_metric = observed_metric(current, &req.metric, &req.slice);
            let baseline_metric = observed_metric(baseline, &req.metric, &req.slice);
            let drift = (current_metric["estimate"].as_f64().unwrap_or(0.0)
                - baseline_metric["estimate"].as_f64().unwrap_or(0.0))
            .abs();
            let passed = compare(drift, &req.comparator, req.threshold);
            json!({
                "kind": "drift", "metric": req.metric, "slice": req.slice,
                "compared_to": req.compared_to,
                "observed": {"current": current_metric, "baseline": baseline_metric, "drift": drift},
                "threshold": req.threshold, "comparator": req.comparator,
                "passed": passed, "requirement": req.source,
            })
        }
        _ => json!({
            "kind": "inconclusive", "metric": req.metric, "slice": req.slice, "passed": false,
            "reason": "unsupported observed_property requirement", "requirement": req.source,
        }),
    }
}

fn observed_finding(prop: &AiObservedProperty, check: &Value) -> Value {
    let kind = if check["kind"] == "drift" {
        "ai_observed_drift"
    } else {
        "ai_observed_threshold_violation"
    };
    json!({
        "schema_version": "fsl-ai-finding.v0",
        "fsl": "fsl-ai-observed.v0",
        "result": "observed_mismatch",
        "kind": kind,
        "severity": "error",
        "component": prop.target,
        "contract": prop.name,
        "tool": Value::Null,
        "failed_rule": "observed_property",
        "violation": kind,
        "guarantee_kind": "runtime_observed",
        "evidence": {"kind": "runtime_telemetry", "formal_proof": false},
        "witness": check,
        "minimal_conflict_set": {"property": prop.name, "metric": check["metric"]},
        "repair_candidates": [{
            "kind": "operations_response",
            "weakens_spec": false,
            "description": "inspect affected slices, run regression eval, or raise fallback/human-review routing",
        }],
        "assumptions": observed_assumptions(),
    })
}

fn observed_assumptions() -> Value {
    json!([{
        "id": "AI-ASSUME-OBSERVABILITY-COVERAGE",
        "text": "runtime telemetry coverage is external evidence; absence from logs is not proof of absence",
    }])
}

// --- shared math (Wilson interval over an arbitrary confidence level) ----

fn compare(left: f64, comparator: &str, right: f64) -> bool {
    match comparator {
        ">=" => left >= right,
        ">" => left > right,
        "<=" => left <= right,
        "<" => left < right,
        "==" => (left - right).abs() <= 1e-12,
        _ => false,
    }
}

fn empty_interval(confidence: f64) -> Value {
    json!({"method": "wilson", "confidence": confidence, "lower": 0.0, "upper": 1.0})
}

#[allow(clippy::cast_precision_loss)]
fn wilson(successes: usize, n: usize, confidence: f64) -> Value {
    if n == 0 {
        return empty_interval(confidence);
    }
    let n_f = n as f64;
    let phat = successes as f64 / n_f;
    let z = probit(confidence);
    let denom = 1.0 + z * z / n_f;
    let center = (phat + z * z / (2.0 * n_f)) / denom;
    let margin = z / denom * (phat * (1.0 - phat) / n_f + z * z / (4.0 * n_f * n_f)).sqrt();
    json!({
        "method": "wilson",
        "confidence": confidence,
        "lower": (center - margin).max(0.0),
        "upper": (center + margin).min(1.0),
    })
}

/// The z-quantile for a two-sided confidence level. `0.95` uses the same
/// precise constant as the rest of fslc; any other declared `confidence`
/// uses Peter Acklam's rational approximation of the inverse standard normal
/// CDF (accurate to about 1.15e-9), since `statistical_property` may declare
/// a `confidence` other than the common default.
fn probit(confidence: f64) -> f64 {
    if (confidence - 0.95).abs() < 1e-12 {
        1.959_963_984_540_054
    } else {
        inverse_normal_cdf(0.5 + confidence / 2.0)
    }
}

#[allow(clippy::many_single_char_names)]
fn inverse_normal_cdf(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e+01,
        2.209_460_984_245_205e+02,
        -2.759_285_104_469_687e+02,
        1.383_577_518_672_69e+02,
        -3.066_479_806_614_716e+01,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e+01,
        1.615_858_368_580_409e+02,
        -1.556_989_798_598_866e+02,
        6.680_131_188_771_972e+01,
        -1.328_068_155_288_572e+01,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-03,
        -3.223_964_580_411_365e-01,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-03,
        3.224_671_290_700_398e-01,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const P_LOW: f64 = 0.024_25;
    let p_high = 1.0 - P_LOW;
    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= p_high {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsl_syntax::parse_ai_project;

    #[test]
    fn slice_gate_flags_unsupported_even_when_the_combined_estimate_passes() {
        let source = r"
statistical_property LooseQuality {
  target SupportAnswerAgent
  dataset SupportEvalV3
  evaluator SupportAnswerJudge
  confidence 0.95

  require ci_lower(metric.accuracy, 0.95) >= 0.45

  slice JapaneseRefundTickets {
    require min_samples >= 5
    require ci_lower(metric.accuracy, 0.95) >= 0.35
  }
}
";
        let project = parse_ai_project(source, "P").expect("parse");
        let prop = project
            .select_statistical_property(None, None)
            .expect("select");
        let mut records = Vec::new();
        for i in 0..10 {
            records.push(json!({"case_id": format!("all-{i}"), "dataset":"SupportEvalV3","slice":"all","metric":"accuracy","outcome":true,"evaluator":{"id":"SupportAnswerJudge","calibration_status":"trusted"}}));
        }
        for i in 0..5 {
            records.push(json!({"case_id": format!("jp-{i}"), "dataset":"SupportEvalV3","slice":"JapaneseRefundTickets","metric":"accuracy","outcome":false,"evaluator":{"id":"SupportAnswerJudge","calibration_status":"trusted"}}));
        }
        let result =
            evaluate_statistical_property(prop, &records, "SupportEvalV3", None).expect("evaluate");
        assert_eq!(result["status"], "statistically_unsupported");
        assert_eq!(result["schema_version"], STATISTICAL_RESULT_SCHEMA_VERSION);
    }

    #[test]
    fn duplicate_records_are_dataset_invalid() {
        let source = r"
statistical_property Q {
  target X
  dataset D
  evaluator E
  require ci_lower(metric.accuracy, 0.95) >= 0.5
}
";
        let project = parse_ai_project(source, "P").expect("parse");
        let prop = project
            .select_statistical_property(None, None)
            .expect("select");
        let records = vec![
            json!({"case_id":"dup","dataset":"D","slice":"all","metric":"accuracy","outcome":true,"evaluator":{"id":"E","calibration_status":"trusted"}}),
            json!({"case_id":"dup","dataset":"D","slice":"all","metric":"accuracy","outcome":true,"evaluator":{"id":"E","calibration_status":"trusted"}}),
        ];
        let result = evaluate_statistical_property(prop, &records, "D", None).expect("evaluate");
        assert_eq!(result["status"], "dataset_invalid");
    }
}
