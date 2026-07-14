# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""External evidence checkers for fsl-ai stochastic, migration, and drift jobs."""
from __future__ import annotations

import json
import math
from pathlib import Path
from statistics import NormalDist
from typing import Dict, Iterable, List, Optional, Sequence, Tuple

from .ai_ir import AiComponent
from .ai_project import (
    AiMetricRequirement,
    AiMigration,
    AiObservedProperty,
    AiObservedRequirement,
    AiProject,
    AiRegressionRequirement,
    AiStatisticalProperty,
    is_ai_project_source,
    parse_ai_project,
)
from .ai_parser import parse_ai_component
from .model import FslError


AI_STOCHASTIC_VERSION = "fsl-stochastic.v0"
AI_MIGRATION_VERSION = "fsl-ai-migration.v0"
AI_OBSERVED_VERSION = "fsl-ai-observed.v0"
AI_COMPAT_VERSION = "fsl-ai-compat-profile.v0"
AI_STATISTICAL_RESULT_SCHEMA_VERSION = "fsl-ai-statistical-result.v0"
AI_FINDING_SCHEMA_VERSION = "fsl-ai-finding.v0"


def load_ai_project_file(path) -> AiProject:
    src = Path(path).read_text(encoding="utf-8")
    return parse_ai_project(src, name=Path(path).stem)


def evaluate_ai_project(
        project: AiProject,
        records_path: Optional[str] = None,
        dataset_name: Optional[str] = None,
        slice_name: Optional[str] = None,
        property_name: Optional[str] = None) -> dict:
    prop = _select_statistical_property(project, property_name, dataset_name)
    dataset = dataset_name or prop.dataset
    if not dataset:
        raise FslError("statistical_property requires dataset or --dataset", kind="semantics")
    records_file = _records_path(project, records_path, dataset)
    records = _load_json_records(records_file)
    checks = _evaluate_statistical_requirements(records, prop, dataset, slice_name)
    status = _overall_status(checks)
    primary = _primary_check(checks)
    findings = [
        _statistical_finding(project, prop, check)
        for check in checks
        if check["status"] != "statistically_supported"
    ]
    return {
        "schema_version": AI_STATISTICAL_RESULT_SCHEMA_VERSION,
        "fsl": AI_STOCHASTIC_VERSION,
        "result": status,
        "status": status,
        "formal_result": "not_run",
        "target": prop.target,
        "property": prop.name,
        "dataset": dataset,
        "slice": primary.get("slice", slice_name or "all"),
        "metric": primary.get("metric", "unknown"),
        "n": primary.get("n", 0),
        "estimate": primary.get("estimate", 0.0),
        "interval": primary.get("interval", _empty_interval(prop.confidence)),
        "threshold": primary.get("threshold", {"operator": "none", "value": 0.0}),
        "evaluator": _evaluator_summary(records, prop.evaluator),
        "checks": checks,
        "assumptions": _statistical_assumptions(),
        "findings": findings,
    }


def regress_ai_project(
        project: AiProject,
        migration_name: Optional[str],
        before_records_path: str,
        after_records_path: str,
        dataset_name: Optional[str] = None) -> dict:
    migration = _select_migration(project, migration_name)
    before = _load_json_records(before_records_path)
    after = _load_json_records(after_records_path)
    requirements = list(migration.regression_requirements)
    if not requirements:
        raise FslError(
            f"ai_migration '{migration.name}' has no no_regression metric clauses",
            kind="semantics",
        )
    checks = [
        _evaluate_regression_requirement(before, after, req, dataset_name or req.dataset)
        for req in requirements
    ]
    failed = [check for check in checks if not check["passed"]]
    return {
        "schema_version": "fsl-ai-migration-result.v0",
        "fsl": AI_MIGRATION_VERSION,
        "result": "statistically_unsupported" if failed else "statistically_supported",
        "status": "statistically_unsupported" if failed else "statistically_supported",
        "formal_result": "not_run",
        "migration": migration.name,
        "target": migration.to_endpoint.component if migration.to_endpoint else None,
        "dataset": dataset_name or _first_dataset(requirements),
        "checks": checks,
        "assumptions": _regression_assumptions(),
        "findings": [
            _migration_finding(migration, check)
            for check in failed
        ],
    }


def compare_ai_records(
        before_records_path: str,
        after_records_path: str,
        dataset_name: Optional[str] = None,
        from_label: Optional[str] = None,
        to_label: Optional[str] = None) -> dict:
    before = _load_json_records(before_records_path)
    after = _load_json_records(after_records_path)
    metrics = sorted(_metric_set(before, dataset_name) | _metric_set(after, dataset_name))
    comparisons = []
    for metric in metrics:
        b = _aggregate_metric(before, metric, dataset_name, "all")
        a = _aggregate_metric(after, metric, dataset_name, "all")
        comparisons.append({
            "metric": metric,
            "before": b,
            "after": a,
            "delta": a["estimate"] - b["estimate"],
        })
    return {
        "schema_version": "fsl-ai-comparison-result.v0",
        "fsl": AI_MIGRATION_VERSION,
        "result": "compared",
        "formal_result": "not_run",
        "from": from_label or str(before_records_path),
        "to": to_label or str(after_records_path),
        "dataset": dataset_name,
        "comparisons": comparisons,
        "assumptions": _regression_assumptions(),
        "findings": [],
    }


def drift_ai_project(
        project: AiProject,
        current_logs_path: str,
        baseline_logs_path: Optional[str] = None,
        property_name: Optional[str] = None,
        window: Optional[str] = None,
        baseline_label: Optional[str] = None) -> dict:
    prop = _select_observed_property(project, property_name)
    current = _load_json_records(current_logs_path)
    baseline = _load_json_records(baseline_logs_path) if baseline_logs_path else []
    checks = [
        _evaluate_observed_requirement(current, baseline, req)
        for req in prop.requirements
    ]
    failed = [check for check in checks if not check["passed"]]
    return {
        "schema_version": "fsl-ai-observed-result.v0",
        "fsl": AI_OBSERVED_VERSION,
        "result": "observed_mismatch" if failed else "observed_supported",
        "formal_result": "not_run",
        "target": prop.target,
        "property": prop.name,
        "window": window or prop.window,
        "baseline": baseline_label,
        "checks": checks,
        "assumptions": _observed_assumptions(),
        "findings": [
            _observed_finding(prop, check)
            for check in failed
        ],
    }


def ai_compat_profile_from_file(path, environment: Optional[str] = None) -> dict:
    src = Path(path).read_text(encoding="utf-8")
    from .dialect_registry import dialect_keyword

    if not is_ai_project_source(src) and dialect_keyword(src) == "ai_component":
        components = [parse_ai_component(src)]
    else:
        components = list(parse_ai_project(src, name=Path(path).stem).components)
    profiles = [_component_profile(component) for component in components]
    return {
        "schema_version": "fsl-ai-compat-profile.v0",
        "fsl": AI_COMPAT_VERSION,
        "result": "compat_profile_generated",
        "formal_result": "not_run",
        "environment": environment,
        "profiles": profiles,
        "dbsystem_fragment": _profile_fragment(profiles, environment),
        "assumptions": [{
            "id": "AI-ASSUME-COMPATIBILITY-PROFILE-COMPLETE",
            "text": (
                "generated requires/provides capabilities are finite declarations "
                "for the checked environment window"
            ),
        }],
        "findings": [],
    }


def _select_statistical_property(
        project: AiProject,
        property_name: Optional[str],
        dataset_name: Optional[str]) -> AiStatisticalProperty:
    props = list(project.statistical_properties)
    if property_name:
        for prop in props:
            if prop.name == property_name:
                return prop
        raise FslError(f"unknown statistical_property '{property_name}'", kind="semantics")
    if dataset_name:
        matching = [prop for prop in props if prop.dataset == dataset_name]
        if len(matching) == 1:
            return matching[0]
        if len(matching) > 1:
            raise FslError(
                f"multiple statistical_property declarations use dataset '{dataset_name}'; pass --property",
                kind="semantics",
            )
    if len(props) == 1:
        return props[0]
    if not props:
        raise FslError("no statistical_property declaration found", kind="semantics")
    raise FslError("multiple statistical_property declarations found; pass --property", kind="semantics")


def _select_migration(project: AiProject, migration_name: Optional[str]) -> AiMigration:
    migrations = list(project.migrations)
    if migration_name:
        for migration in migrations:
            if migration.name == migration_name:
                return migration
        raise FslError(f"unknown ai_migration '{migration_name}'", kind="semantics")
    if len(migrations) == 1:
        return migrations[0]
    if not migrations:
        raise FslError("no ai_migration declaration found", kind="semantics")
    raise FslError("multiple ai_migration declarations found; pass --migration", kind="semantics")


def _select_observed_property(project: AiProject, property_name: Optional[str]) -> AiObservedProperty:
    props = list(project.observed_properties)
    if property_name:
        for prop in props:
            if prop.name == property_name:
                return prop
        raise FslError(f"unknown observed_property '{property_name}'", kind="semantics")
    if len(props) == 1:
        return props[0]
    if not props:
        raise FslError("no observed_property declaration found", kind="semantics")
    raise FslError("multiple observed_property declarations found; pass --property", kind="semantics")


def _records_path(project: AiProject, records_path: Optional[str], dataset_name: str) -> str:
    if records_path:
        return records_path
    dataset = project.dataset_map().get(dataset_name)
    if not dataset or not dataset.source:
        raise FslError(f"dataset '{dataset_name}' has no source; pass --records", kind="semantics")
    source = dataset.source
    if "://" in source:
        raise FslError(
            f"dataset '{dataset_name}' source '{source}' is external; pass a local --records JSONL file",
            kind="semantics",
        )
    return source


def _load_json_records(path) -> List[dict]:
    if not path:
        return []
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    stripped = text.lstrip()
    if not stripped:
        return []
    if p.suffix != ".jsonl" and (stripped.startswith("[") or stripped.startswith("{")):
        data = json.loads(text)
        if isinstance(data, list):
            return data
        if isinstance(data, dict) and isinstance(data.get("events"), list):
            return data["events"]
        if isinstance(data, dict) and isinstance(data.get("records"), list):
            return data["records"]
        return [data]
    records = []
    for lineno, line in enumerate(text.splitlines(), start=1):
        raw = line.strip()
        if not raw:
            continue
        try:
            records.append(json.loads(raw))
        except json.JSONDecodeError as exc:
            raise FslError(f"invalid JSONL at line {lineno}: {exc.msg}", kind="parse") from exc
    return records


def _evaluate_statistical_requirements(
        records: Sequence[dict],
        prop: AiStatisticalProperty,
        dataset_name: str,
        slice_filter: Optional[str]) -> List[dict]:
    duplicate = _duplicate_eval_key(records)
    if duplicate:
        return [{
            "status": "dataset_invalid",
            "metric": duplicate["metric"],
            "slice": duplicate["slice"],
            "n": 0,
            "estimate": 0.0,
            "interval": _empty_interval(prop.confidence),
            "threshold": {"operator": "none", "value": 0.0},
            "reason": "duplicate (case_id, slice, metric) record",
            "duplicate": duplicate,
        }]
    requirements = [
        req for req in prop.requirements
        if slice_filter is None or req.slice == slice_filter
    ]
    if not requirements:
        raise FslError("no statistical requirements matched the requested slice/property", kind="semantics")
    checks = []
    for req in requirements:
        if req.kind == "min_samples":
            checks.append(_check_min_samples(records, req, dataset_name))
        elif req.kind in ("ci_lower", "ci_upper"):
            checks.append(_check_ci_requirement(records, req, dataset_name, prop.evaluator))
        elif req.kind == "point_estimate":
            checks.append(_inconclusive_check(req, "point-estimate-only requirement is not accepted"))
        else:
            checks.append(_inconclusive_check(req, "unsupported statistical requirement"))
    return checks


def _check_min_samples(records: Sequence[dict], req: AiMetricRequirement, dataset_name: str) -> dict:
    relevant = [
        record for record in records
        if _record_dataset(record, dataset_name) and _record_slice(record) == req.slice
    ]
    n = len({str(record.get("case_id", idx)) for idx, record in enumerate(relevant)})
    passed = _compare(float(n), req.comparator or ">=", float(req.min_samples or 0))
    return {
        "status": "statistically_supported" if passed else "insufficient_samples",
        "metric": "min_samples",
        "slice": req.slice,
        "n": n,
        "estimate": 0.0,
        "interval": _empty_interval(req.confidence or 0.95),
        "threshold": {"operator": f"min_samples_{req.comparator}", "value": req.min_samples or 0},
        "passed": passed,
        "requirement": req.source,
    }


def _check_ci_requirement(
        records: Sequence[dict],
        req: AiMetricRequirement,
        dataset_name: str,
        evaluator: Optional[str]) -> dict:
    metric = req.metric or "unknown"
    relevant = [
        record for record in records
        if _record_dataset(record, dataset_name)
        and _record_slice(record) == req.slice
        and _record_metric(record) == metric
    ]
    if not relevant:
        return {
            "status": "slice_missing",
            "metric": metric,
            "slice": req.slice,
            "n": 0,
            "estimate": 0.0,
            "interval": _empty_interval(req.confidence or 0.95),
            "threshold": _threshold(req),
            "passed": False,
            "requirement": req.source,
            "reason": "no eval records matched dataset/slice/metric",
        }
    trust = _trust_status(relevant, evaluator)
    if trust != "trusted":
        return {
            "status": "evaluator_untrusted",
            "metric": metric,
            "slice": req.slice,
            "n": len(relevant),
            "estimate": 0.0,
            "interval": _empty_interval(req.confidence or 0.95),
            "threshold": _threshold(req),
            "passed": False,
            "requirement": req.source,
            "evaluator": {"id": evaluator, "trust_status": trust},
        }
    successes = sum(1 for record in relevant if _record_outcome(record))
    n = len(relevant)
    estimate = successes / n if n else 0.0
    interval = _wilson(successes, n, req.confidence or 0.95)
    observed = interval["lower"] if req.kind == "ci_lower" else interval["upper"]
    passed = _compare(observed, req.comparator or ">=", req.threshold or 0.0)
    return {
        "status": "statistically_supported" if passed else "statistically_unsupported",
        "metric": metric,
        "slice": req.slice,
        "n": n,
        "successes": successes,
        "estimate": estimate,
        "interval": interval,
        "threshold": _threshold(req),
        "observed_bound": observed,
        "passed": passed,
        "requirement": req.source,
    }


def _evaluate_regression_requirement(
        before: Sequence[dict],
        after: Sequence[dict],
        req: AiRegressionRequirement,
        dataset_name: Optional[str]) -> dict:
    before_metric = _aggregate_metric(before, req.metric, dataset_name, "all")
    after_metric = _aggregate_metric(after, req.metric, dataset_name, "all")
    delta = before_metric["estimate"] - after_metric["estimate"] if req.direction == "drop" else after_metric["estimate"] - before_metric["estimate"]
    passed = _compare(delta, req.comparator, req.threshold)
    return {
        "metric": req.metric,
        "direction": req.direction,
        "dataset": dataset_name,
        "before": before_metric,
        "after": after_metric,
        "observed_delta": delta,
        "allowed_delta": req.threshold,
        "comparator": req.comparator,
        "passed": passed,
    }


def _evaluate_observed_requirement(
        current: Sequence[dict],
        baseline: Sequence[dict],
        req: AiObservedRequirement) -> dict:
    if req.kind == "observed":
        current_metric = _observed_metric(current, req.metric, req.slice)
        passed = _compare(current_metric["estimate"], req.comparator, req.threshold)
        return {
            "kind": req.kind,
            "metric": req.metric,
            "slice": req.slice,
            "current": current_metric,
            "threshold": req.threshold,
            "comparator": req.comparator,
            "passed": passed,
            "requirement": req.source,
        }
    if req.kind == "drift":
        if _is_distribution_metric(req.metric):
            observed = _distribution_drift(current, baseline, req.metric, req.slice)
        else:
            cur = _observed_metric(current, req.metric, req.slice)
            base = _observed_metric(baseline, req.metric, req.slice)
            observed = {
                "current": cur,
                "baseline": base,
                "drift": abs(cur["estimate"] - base["estimate"]),
            }
        passed = _compare(observed["drift"], req.comparator, req.threshold)
        return {
            "kind": req.kind,
            "metric": req.metric,
            "slice": req.slice,
            "compared_to": req.compared_to,
            "observed": observed,
            "threshold": req.threshold,
            "comparator": req.comparator,
            "passed": passed,
            "requirement": req.source,
        }
    return {
        "kind": "inconclusive",
        "metric": req.metric,
        "slice": req.slice,
        "passed": False,
        "reason": "unsupported observed_property requirement",
        "requirement": req.source,
    }


def _aggregate_metric(records: Sequence[dict], metric: str, dataset_name: Optional[str], slice_name: str) -> dict:
    relevant = [
        record for record in records
        if (dataset_name is None or _record_dataset(record, dataset_name))
        and _record_metric(record) == metric
        and (slice_name == "all" or _record_slice(record) == slice_name)
    ]
    n = len(relevant)
    successes = sum(1 for record in relevant if _record_outcome(record))
    estimate = successes / n if n else 0.0
    return {
        "n": n,
        "successes": successes,
        "estimate": estimate,
        "interval": _wilson(successes, n, 0.95) if n else _empty_interval(0.95),
    }


def _observed_metric(records: Sequence[dict], metric: str, slice_name: str) -> dict:
    values = []
    for record in records:
        if slice_name != "all" and _record_slice(record) != slice_name:
            continue
        value = _metric_value(record, metric)
        if value is not None:
            values.append(value)
    n = len(values)
    if not values:
        return {"n": 0, "estimate": 0.0}
    if all(isinstance(value, bool) for value in values):
        return {"n": n, "estimate": sum(1 for value in values if value) / n}
    numeric = [float(value) for value in values]
    return {"n": n, "estimate": sum(numeric) / n}


def _distribution_drift(current: Sequence[dict], baseline: Sequence[dict], metric: str, slice_name: str) -> dict:
    path = metric[:-len("_distribution")] if metric.endswith("_distribution") else metric
    cur = _distribution(current, path, slice_name)
    base = _distribution(baseline, path, slice_name)
    keys = set(cur) | set(base)
    drift = 0.5 * sum(abs(cur.get(key, 0.0) - base.get(key, 0.0)) for key in keys)
    return {"current": cur, "baseline": base, "drift": drift}


def _distribution(records: Sequence[dict], path: str, slice_name: str) -> Dict[str, float]:
    counts: Dict[str, int] = {}
    total = 0
    for record in records:
        if slice_name != "all" and _record_slice(record) != slice_name:
            continue
        value = _path_value(record, path)
        if value is None:
            continue
        counts[str(value)] = counts.get(str(value), 0) + 1
        total += 1
    if total == 0:
        return {}
    return {key: value / total for key, value in counts.items()}


def _metric_value(record: dict, metric: str):
    if _record_metric(record) == metric and "outcome" in record:
        return _record_outcome(record)
    if isinstance(record.get("metrics"), dict) and metric in record["metrics"]:
        return record["metrics"][metric]
    if isinstance(record.get("evaluator_results"), dict) and metric in record["evaluator_results"]:
        return record["evaluator_results"][metric]
    return _path_value(record, metric)


def _path_value(record: dict, path: str):
    cur = record
    for part in path.split("."):
        if isinstance(cur, dict) and part in cur:
            cur = cur[part]
        else:
            return None
    return cur


def _wilson(successes: int, n: int, confidence: float) -> dict:
    if n == 0:
        return _empty_interval(confidence)
    z = NormalDist().inv_cdf(0.5 + confidence / 2.0)
    phat = successes / n
    denom = 1.0 + z * z / n
    center = (phat + z * z / (2 * n)) / denom
    margin = z / denom * math.sqrt(phat * (1 - phat) / n + z * z / (4 * n * n))
    return {
        "method": "wilson",
        "confidence": confidence,
        "lower": max(0.0, center - margin),
        "upper": min(1.0, center + margin),
    }


def _empty_interval(confidence: float) -> dict:
    return {"method": "wilson", "confidence": confidence, "lower": 0.0, "upper": 1.0}


def _duplicate_eval_key(records: Sequence[dict]) -> Optional[dict]:
    seen = set()
    for record in records:
        key = (record.get("case_id"), _record_slice(record), _record_metric(record))
        if None in key or "" in key:
            return {"case_id": record.get("case_id"), "slice": _record_slice(record), "metric": _record_metric(record)}
        if key in seen:
            return {"case_id": key[0], "slice": key[1], "metric": key[2]}
        seen.add(key)
    return None


def _record_dataset(record: dict, dataset_name: str) -> bool:
    return record.get("dataset") in (None, dataset_name)


def _record_slice(record: dict) -> str:
    return str(record.get("slice") or "all")


def _record_metric(record: dict) -> str:
    return str(record.get("metric") or "")


def _record_outcome(record: dict) -> bool:
    return bool(record.get("outcome"))


def _trust_status(records: Sequence[dict], evaluator: Optional[str]) -> str:
    if not records:
        return "unknown"
    for record in records:
        ev = record.get("evaluator")
        if not isinstance(ev, dict):
            return "unknown"
        if evaluator and ev.get("id") not in (None, evaluator):
            return "unknown"
        if ev.get("calibration_status", "unknown") != "trusted":
            return ev.get("calibration_status", "unknown")
    return "trusted"


def _evaluator_summary(records: Sequence[dict], evaluator: Optional[str]) -> dict:
    relevant = records
    trust = _trust_status(relevant, evaluator)
    return {"id": evaluator or _first_evaluator_id(records), "trust_status": trust}


def _first_evaluator_id(records: Sequence[dict]) -> Optional[str]:
    for record in records:
        ev = record.get("evaluator")
        if isinstance(ev, dict) and ev.get("id"):
            return ev["id"]
    return None


def _threshold(req: AiMetricRequirement) -> dict:
    op = "ci_lower_gte" if req.kind == "ci_lower" else "ci_upper_lte"
    return {"operator": op, "value": req.threshold or 0.0}


def _compare(left: float, comparator: str, right: float) -> bool:
    if comparator == ">=":
        return left >= right
    if comparator == ">":
        return left > right
    if comparator == "<=":
        return left <= right
    if comparator == "<":
        return left < right
    if comparator == "==":
        return abs(left - right) <= 1e-12
    return False


def _overall_status(checks: Sequence[dict]) -> str:
    priority = [
        "dataset_invalid",
        "evaluator_untrusted",
        "slice_missing",
        "insufficient_samples",
        "inconclusive",
        "statistically_unsupported",
        "statistically_supported",
    ]
    statuses = {check["status"] for check in checks}
    for status in priority:
        if status in statuses:
            if status == "statistically_supported" and len(statuses) > 1:
                continue
            return status
    return "inconclusive"


def _primary_check(checks: Sequence[dict]) -> dict:
    status = _overall_status(checks)
    for check in checks:
        if check["status"] == status:
            return check
    return checks[0] if checks else {}


def _inconclusive_check(req: AiMetricRequirement, reason: str) -> dict:
    return {
        "status": "inconclusive",
        "metric": req.metric or "unknown",
        "slice": req.slice,
        "n": 0,
        "estimate": 0.0,
        "interval": _empty_interval(req.confidence or 0.95),
        "threshold": _threshold(req) if req.kind in ("ci_lower", "ci_upper") else {"operator": "none", "value": 0.0},
        "passed": False,
        "requirement": req.source,
        "reason": reason,
    }


def _statistical_finding(project: AiProject, prop: AiStatisticalProperty, check: dict) -> dict:
    return _finding(
        fsl=AI_STOCHASTIC_VERSION,
        result=check["status"],
        kind="statistical_contract_unsupported",
        component=prop.target,
        contract=prop.name,
        failed_rule="statistical_property",
        violation=check["status"],
        guarantee_kind="statistically_unsupported",
        evidence_kind="precomputed_eval_jsonl",
        witness=check,
        minimal_conflict_set={
            "property": prop.name,
            "dataset": prop.dataset,
            "slice": check.get("slice"),
            "metric": check.get("metric"),
        },
        repair_candidates=[
            {
                "kind": "eval_data_or_model_change",
                "weakens_spec": False,
                "description": "add evidence, improve the component, or route the affected slice to fallback/human review",
            }
        ],
        assumptions=_statistical_assumptions(),
    )


def _migration_finding(migration: AiMigration, check: dict) -> dict:
    return _finding(
        fsl=AI_MIGRATION_VERSION,
        result="statistically_unsupported",
        kind="ai_migration_regression",
        component=migration.to_endpoint.component if migration.to_endpoint else None,
        contract=migration.name,
        failed_rule="no_regression",
        violation="ai_migration_regression",
        guarantee_kind="statistically_unsupported",
        evidence_kind="precomputed_eval_jsonl_compare",
        witness=check,
        minimal_conflict_set={"migration": migration.name, "metric": check["metric"]},
        repair_candidates=[
            {
                "kind": "rollout_block",
                "weakens_spec": False,
                "description": "block or narrow rollout for the regressed metric/slice",
            },
            {
                "kind": "artifact_change",
                "weakens_spec": False,
                "description": "repair the prompt/model/retriever/tool schema change and re-run the regression evidence",
            },
        ],
        assumptions=_regression_assumptions(),
    )


def _observed_finding(prop: AiObservedProperty, check: dict) -> dict:
    kind = "ai_observed_drift" if check.get("kind") == "drift" else "ai_observed_threshold_violation"
    return _finding(
        fsl=AI_OBSERVED_VERSION,
        result="observed_mismatch",
        kind=kind,
        component=prop.target,
        contract=prop.name,
        failed_rule="observed_property",
        violation=kind,
        guarantee_kind="runtime_observed",
        evidence_kind="runtime_telemetry",
        witness=check,
        minimal_conflict_set={"property": prop.name, "metric": check.get("metric")},
        repair_candidates=[
            {
                "kind": "operations_response",
                "weakens_spec": False,
                "description": "inspect affected slices, run regression eval, or raise fallback/human-review routing",
            }
        ],
        assumptions=_observed_assumptions(),
    )


def _finding(
        fsl,
        result,
        kind,
        component,
        contract,
        failed_rule,
        violation,
        guarantee_kind,
        evidence_kind,
        witness,
        minimal_conflict_set,
        repair_candidates,
        assumptions):
    return {
        "schema_version": AI_FINDING_SCHEMA_VERSION,
        "fsl": fsl,
        "result": result,
        "kind": kind,
        "severity": "error",
        "component": component,
        "contract": contract,
        "tool": None,
        "failed_rule": failed_rule,
        "violation": violation,
        "guarantee_kind": guarantee_kind,
        "evidence": {"kind": evidence_kind, "formal_proof": False},
        "witness": witness,
        "minimal_conflict_set": minimal_conflict_set,
        "repair_candidates": repair_candidates,
        "assumptions": assumptions,
        "redaction": {
            "policy": "aggregate counts, metric labels, and case identifiers only; prompts, answers, and raw payloads are not required",
        },
    }


def _statistical_assumptions() -> List[dict]:
    return [
        {"id": "AI-ASSUME-PRECOMPUTED-EVAL-JSONL", "text": "eval records are precomputed Bernoulli observations"},
        {"id": "AI-ASSUME-SAMPLE-INDEPENDENCE", "text": "sample independence is dataset construction evidence and is not proved by fslc"},
        {"id": "AI-ASSUME-EVALUATOR-CALIBRATION-EVIDENCE", "text": "evaluator trust is supplied by calibration metadata or external evidence"},
        {"id": "AI-ASSUME-NO-STOCHASTIC-KERNEL-SEMANTICS", "text": "statistical support is external evidence and never a kernel proof"},
    ]


def _regression_assumptions() -> List[dict]:
    return _statistical_assumptions() + [{
        "id": "AI-ASSUME-AGGREGATE-REGRESSION-COMPARISON",
        "text": "migration regression compares aggregate precomputed metrics unless paired case evidence is supplied separately",
    }]


def _observed_assumptions() -> List[dict]:
    return [{
        "id": "AI-ASSUME-OBSERVABILITY-COVERAGE",
        "text": "runtime telemetry coverage is external evidence; absence from logs is not proof of absence",
    }]


def _metric_set(records: Sequence[dict], dataset_name: Optional[str]) -> set:
    return {
        _record_metric(record)
        for record in records
        if _record_metric(record) and (dataset_name is None or _record_dataset(record, dataset_name))
    }


def _first_dataset(requirements: Iterable[AiRegressionRequirement]) -> Optional[str]:
    for req in requirements:
        if req.dataset:
            return req.dataset
    return None


def _is_distribution_metric(metric: str) -> bool:
    return metric.endswith("_distribution")


def _component_profile(component: AiComponent) -> dict:
    requires = []
    provides = []
    if component.model:
        requires.append(f"model.{component.model}")
    if component.prompt:
        requires.append(f"prompt.{component.prompt}")
    if component.retriever:
        requires.append(f"retriever.{component.retriever}")
    for tool in component.tools:
        requires.append(f"tool.{tool.schema or tool.name}")
    if component.output_schema:
        provides.append(f"output.{component.output_schema}")
    return {
        "artifact": _artifact_name(component.name),
        "component": component.name,
        "requires": sorted(set(requires)),
        "provides": sorted(set(provides)),
    }


def _artifact_name(name: str) -> str:
    out = []
    for i, ch in enumerate(name):
        if ch.isupper() and i and (not name[i - 1].isupper()):
            out.append("_")
        out.append(ch.lower())
    return "".join(out).strip("_")


def _profile_fragment(profiles: Sequence[dict], environment: Optional[str]) -> str:
    lines = []
    for profile in profiles:
        lines.append(f"artifact {profile['artifact']} {{")
        if profile["requires"]:
            lines.append("  requires " + ", ".join(profile["requires"]) + ";")
        if profile["provides"]:
            lines.append("  provides " + ", ".join(profile["provides"]) + ";")
        lines.append("}")
        lines.append("")
    if environment:
        names = ", ".join(profile["artifact"] for profile in profiles) or "<artifact>"
        lines.append(f"// Add {names} to environment {environment} active/supported windows as appropriate.")
    return "\n".join(lines).rstrip() + ("\n" if lines else "")
