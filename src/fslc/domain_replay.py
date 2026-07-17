# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Runtime replay for fsl-domain command/event/effect logs."""
from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Tuple

from .domain_expand import DOMAIN_DIALECT_VERSION, DOMAIN_FINDING_SCHEMA_VERSION, expand_domain
from .domain_ir import DomainEffect, DomainSpec
from .model import build_spec
from .runtime import Monitor


def replay_domain_file(domain: DomainSpec, logs_path: str):
    events = _read_events(logs_path)
    return replay_domain_events(domain, events)


def replay_domain_events(domain: DomainSpec, events: Iterable[dict]):
    expansion = expand_domain(domain)
    spec = build_spec(expansion.ast, expansion.display_names)
    monitor = Monitor(spec)
    monitor.reset()

    findings: List[dict] = []
    pending_effects: Dict[Tuple[str, str], dict] = {}
    completed_effects: Dict[Tuple[str, str], dict] = {}
    observed_events = set()
    steps_checked = 0

    event_list = list(events)
    for index, entry in enumerate(event_list):
        kind = entry.get("event") or entry.get("kind")
        if kind == "command":
            result = _step_command(domain, monitor, entry)
            steps_checked += 1
            if not result.get("ok"):
                findings.append(_runtime_finding(
                    "command_rejected_by_model",
                    "error",
                    domain.name,
                    index,
                    failed_rule="runtime_command_must_be_enabled_by_domain_model",
                    witness={"log": entry, "monitor": result},
                    repair=["change the implementation command path or update the FSL decide/evolve model"],
                ))
        elif kind == "domain_event":
            event_name = entry.get("name") or entry.get("domain_event")
            if event_name not in domain.event_map():
                findings.append(_runtime_finding(
                    "unknown_domain_event",
                    "error",
                    domain.name,
                    index,
                    failed_rule="runtime_event_declared_in_domain",
                    witness={"event": event_name},
                    repair=[f"declare event {event_name} in an aggregate or fix the runtime log"],
                ))
            else:
                observed_events.add(event_name)
        elif kind == "effect_request":
            effect = _effect_by_name(domain, entry.get("effect"))
            if effect is None:
                findings.append(_runtime_finding(
                    "unknown_effect",
                    "error",
                    domain.name,
                    index,
                    failed_rule="runtime_effect_declared_in_domain",
                    witness={"effect": entry.get("effect")},
                    repair=["declare the effect or fix the runtime log"],
                ))
                continue
            corr = _correlation_value(entry)
            if corr is None:
                findings.append(_runtime_finding(
                    "uncorrelated_async_completion",
                    "error",
                    domain.name,
                    index,
                    effect=effect.name,
                    failed_rule="effect_request_has_correlation_id",
                    witness={"log": entry},
                    repair=[f"include correlation_id in effect_request for {effect.name}"],
                ))
                continue
            key = (effect.name, corr)
            pending_effects.setdefault(key, entry)
        elif kind == "effect_completion":
            effect = _effect_by_name(domain, entry.get("effect"))
            if effect is None:
                findings.append(_runtime_finding(
                    "unknown_effect",
                    "error",
                    domain.name,
                    index,
                    failed_rule="runtime_effect_declared_in_domain",
                    witness={"effect": entry.get("effect")},
                    repair=["declare the effect or fix the runtime log"],
                ))
                continue
            corr = _correlation_value(entry)
            event_name = entry.get("name") or entry.get("domain_event") or entry.get("outcome")
            if event_name not in effect.outcomes:
                findings.append(_runtime_finding(
                    "effect_completion_event_not_declared",
                    "error",
                    domain.name,
                    index,
                    effect=effect.name,
                    failed_rule="effect_completion_uses_declared_outcome",
                    witness={"effect": effect.name, "event": event_name},
                    repair=[f"add {event_name} to effect {effect.name} outcomes or fix the runtime log"],
                ))
                continue
            if corr is None:
                findings.append(_runtime_finding(
                    "uncorrelated_async_completion",
                    "error",
                    domain.name,
                    index,
                    effect=effect.name,
                    failed_rule="effect_completion_has_correlation_id",
                    witness={"log": entry},
                    repair=[f"include correlation_id in effect_completion for {effect.name}"],
                ))
                continue
            key = (effect.name, corr)
            if key not in pending_effects:
                findings.append(_runtime_finding(
                    "uncorrelated_async_completion",
                    "error",
                    domain.name,
                    index,
                    effect=effect.name,
                    failed_rule="completion_requires_prior_request",
                    witness={"effect": effect.name, "correlation_id": corr},
                    repair=["record effect_request before completion or fix correlation_id mapping"],
                ))
            if effect.irreversible and key in completed_effects:
                findings.append(_runtime_finding(
                    "duplicate_irreversible_effect_commit",
                    "error",
                    domain.name,
                    index,
                    effect=effect.name,
                    failed_rule="irreversible_effect_completes_at_most_once_per_correlation",
                    witness={"effect": effect.name, "correlation_id": corr},
                    repair=["deduplicate completion handling by idempotency_key/correlation_id"],
                ))
            result = _step_effect_completion(effect, event_name, corr, monitor, entry)
            steps_checked += 1
            if not result.get("ok"):
                findings.append(_runtime_finding(
                    "effect_completion_rejected_by_model",
                    "error",
                    domain.name,
                    index,
                    effect=effect.name,
                    failed_rule="effect_completion_matches_pending_lifecycle",
                    witness={"log": entry, "monitor": result},
                    repair=["ensure request and completion ordering matches the fsl-effect lifecycle"],
                ))
            completed_effects[key] = entry
            pending_effects.pop(key, None)
            observed_events.add(event_name)
        else:
            findings.append(_runtime_finding(
                "unknown_runtime_event_kind",
                "error",
                domain.name,
                index,
                failed_rule="runtime_log_event_kind_supported",
                witness={"log": entry},
                repair=["use event kind command, domain_event, effect_request, or effect_completion"],
            ))

    result = "conformance_checked" if not findings else "nonconformant"
    return {
        "result": result,
        "dialect": DOMAIN_DIALECT_VERSION,
        "finding_schema_version": DOMAIN_FINDING_SCHEMA_VERSION,
        "domain": domain.name,
        "guarantee_kind": "runtime_observed",
        "steps_checked": steps_checked,
        "events_observed": sorted(observed_events),
        "pending_effects": [
            {"effect": effect, "correlation_id": corr}
            for effect, corr in sorted(pending_effects)
        ],
        "findings": findings,
        "final_state": monitor.state,
        "assumptions": expansion.assumptions,
    }


def _read_events(logs_path: str) -> List[dict]:
    raw = Path(logs_path).read_text(encoding="utf-8")
    stripped = raw.lstrip()
    if stripped.startswith("[") or stripped.startswith("{"):
        try:
            data = json.loads(raw)
        except json.JSONDecodeError as exc:
            if "Extra data" not in str(exc):
                raise
        else:
            if isinstance(data, list):
                return data
            if isinstance(data, dict) and "events" in data:
                return list(data["events"])
            if isinstance(data, dict) and ("event" in data or "kind" in data):
                return [data]
            raise ValueError("domain replay JSON must be an array or {\"events\": [...]}")
    out = []
    for line in raw.splitlines():
        line = line.strip()
        if line:
            out.append(json.loads(line))
    return out


def _step_command(domain: DomainSpec, monitor: Monitor, entry: dict):
    aggregate = entry.get("aggregate")
    command = entry.get("command")
    params = {key: _coerce_int(value) for key, value in dict(entry.get("params") or {}).items()}
    if aggregate not in domain.aggregate_map():
        return {"ok": False, "kind": "unknown_aggregate", "aggregate": aggregate}
    if command not in domain.aggregate_map()[aggregate].command_map():
        return {"ok": False, "kind": "unknown_command", "aggregate": aggregate, "command": command}
    return monitor.step(f"{_action_name(aggregate)}_{_action_name(command)}", params)


def _step_effect_completion(
        effect: DomainEffect,
        event_name: str,
        corr: str,
        monitor: Monitor,
        entry: dict):
    params = {key: _coerce_int(value) for key, value in dict(entry.get("params") or {}).items()}
    corr_field = _correlation_field(effect) or "correlation_id"
    params.setdefault(corr_field, _coerce_int(corr))
    action = f"{_action_name(effect.name)}_complete_{_action_name(event_name)}"
    return monitor.step(action, params)


def _effect_by_name(domain: DomainSpec, name: Optional[str]) -> Optional[DomainEffect]:
    for effect in domain.effects:
        if effect.name == name:
            return effect
    return None


def _correlation_value(entry: dict) -> Optional[str]:
    value = entry.get("correlation_id")
    if value is None:
        params = entry.get("params") or {}
        value = params.get("correlation_id")
    if value is None:
        return None
    return str(value)


def _correlation_field(effect: DomainEffect) -> Optional[str]:
    if effect.correlation_id and "." in effect.correlation_id:
        return effect.correlation_id.rsplit(".", 1)[1]
    return effect.correlation_id


def _coerce_int(value: str):
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def _action_name(name):
    s = re.sub(r"(.)([A-Z][a-z]+)", r"\1_\2", name)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    return s.lower()


def _runtime_finding(
        kind,
        severity,
        domain,
        step_index,
        failed_rule,
        witness,
        repair,
        effect=None):
    out = {
        "schema_version": DOMAIN_FINDING_SCHEMA_VERSION,
        "fsl": DOMAIN_DIALECT_VERSION,
        "result": "violated",
        "kind": kind,
        "severity": severity,
        "domain": domain,
        "failed_rule": failed_rule,
        "guarantee_kind": "runtime_observed",
        "evidence": {
            "kind": "runtime_replay",
            "formal_proof": False,
            "step_index": step_index,
        },
        "witness": witness,
        "repair_candidates": [
            {"kind": "implementation_or_model_change", "weakens_spec": False, "description": text}
            for text in repair
        ],
        "assumptions": [],
    }
    if effect:
        out["effect"] = effect
    return out
