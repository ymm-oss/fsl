# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""AI-readable fsl-ai hard-contract checks and event-log replay."""
from __future__ import annotations

import json
from pathlib import Path
from typing import List, Optional

from .ai_expand import (
    AI_DIALECT_VERSION,
    AI_FINDING_SCHEMA_VERSION,
    expand_ai_component,
    static_policy_findings,
)
from .ai_ir import AiComponent, AiTool
from .ai_parser import parse_ai_component
from .bmc import prove, verify
from .model import FslError, build_spec


AI_EVENT_SCHEMA_VERSION = "fsl-ai-event.v0"


def load_ai_component(path):
    return parse_ai_component(Path(path).read_text(encoding="utf-8"))


def check_ai_component(component: AiComponent, depth=8, engine="bmc", deadlock_mode="warn"):
    expansion = expand_ai_component(component)
    static_findings = static_policy_findings(component, expansion.assumptions)
    if static_findings:
        return _ai_result(
            "violated",
            component,
            expansion.assumptions,
            findings=static_findings,
            kernel=None,
            formal_result="not_run",
        )

    spec = build_spec(expansion.ast, expansion.display_names)
    if engine == "induction":
        kernel = prove(spec, 1, depth, deadlock_mode=deadlock_mode)
    else:
        kernel = verify(spec, depth, deadlock_mode=deadlock_mode)

    translated = _translate_kernel_result(kernel, component, expansion)
    if translated:
        return _ai_result(
            "violated",
            component,
            expansion.assumptions,
            findings=translated,
            kernel=kernel,
            formal_result=kernel.get("result"),
        )
    if kernel.get("result") in ("verified", "proved"):
        return _ai_result(
            "verified_under_assumptions",
            component,
            expansion.assumptions,
            findings=[],
            kernel=kernel,
            formal_result=kernel.get("result"),
        )
    return _ai_result(
        kernel.get("result", "error"),
        component,
        expansion.assumptions,
        findings=[],
        kernel=kernel,
        formal_result=kernel.get("result", "error"),
    )


def replay_ai_events(component: AiComponent, logs_path):
    expansion = expand_ai_component(component)
    events = _load_ai_events(logs_path)
    assumptions = list(expansion.assumptions) + [{
        "id": "AI-ASSUME-OBSERVABILITY-COVERAGE",
        "text": (
            "runtime replay is evidence only; absence from logs is not a proof "
            "that a tool or capability is unused"
        ),
    }]

    findings = _replay_findings(component, events, assumptions)
    return {
        "result": "replay_nonconformant" if findings else "replay_conformant",
        "dialect": AI_DIALECT_VERSION,
        "finding_schema_version": AI_FINDING_SCHEMA_VERSION,
        "event_schema_version": AI_EVENT_SCHEMA_VERSION,
        "ai_component": component.name,
        "events_checked": len(events),
        "formal_result": "not_run",
        "evidence": {
            "kind": "runtime_replay",
            "formal_proof": False,
        },
        "assumptions": assumptions,
        "findings": findings,
        "note": "runtime replay is separate from formal proof; statistical and evaluator-backed contracts are out of Phase 1",
    }


def _ai_result(result, component, assumptions, findings, kernel, formal_result):
    out = {
        "result": result,
        "dialect": AI_DIALECT_VERSION,
        "finding_schema_version": AI_FINDING_SCHEMA_VERSION,
        "ai_component": component.name,
        "guarantee_boundary": {
            "proved": "kernel safety facts over the finite hard-contract expansion",
            "evaluator_supported": "out of Phase 1 and never reported as formal proof",
            "statistically_supported": "out of Phase 1 and never reported as formal proof",
            "runtime_replay": "observed evidence, not proof",
        },
        "assumptions": assumptions,
        "findings": findings,
        "formal_result": formal_result,
    }
    if kernel is not None:
        out["kernel"] = {
            key: kernel[key]
            for key in (
                "result",
                "spec",
                "depth",
                "checked_to_depth",
                "completeness",
                "invariant",
                "violation_kind",
            )
            if key in kernel
        }
    return out


def _translate_kernel_result(kernel, component, expansion):
    if kernel.get("result") != "violated" or kernel.get("violation_kind") != "invariant":
        return []
    inv_name = kernel.get("invariant")
    meta = expansion.invariant_metadata.get(inv_name)
    if not meta:
        return []
    return [_finding(
        kind=meta["kind"],
        result="violated",
        severity="error",
        component=component.name,
        tool=meta.get("tool"),
        failed_rule=meta["failed_rule"],
        violation=meta["violation"],
        guarantee_kind=meta.get("guarantee_kind", "syntactic_hard"),
        evidence_kind="induction" if kernel.get("completeness") == "induction" else "bmc",
        witness={
            "invariant": inv_name,
            "trace": kernel.get("trace", []),
        },
        minimal_conflict_set={
            "component": component.name,
            "tool": meta.get("tool"),
            "invariant": inv_name,
        },
        repair_candidates=_repairs_for_violation(meta["violation"], meta.get("tool")),
        assumptions=expansion.assumptions,
    )]


def _load_ai_events(logs_path):
    path = Path(logs_path)
    text = path.read_text(encoding="utf-8")
    stripped = text.lstrip()
    try:
        if path.suffix != ".jsonl" and (stripped.startswith("[") or stripped.startswith("{")):
            data = json.loads(text)
            if isinstance(data, list):
                return data
            if isinstance(data, dict) and isinstance(data.get("events"), list):
                return data["events"]
            raise FslError("AI event JSON must be an array or {\"events\": [...]}",
                           kind="semantics")
        events = []
        for lineno, line in enumerate(text.splitlines(), start=1):
            raw = line.strip()
            if not raw:
                continue
            try:
                events.append(json.loads(raw))
            except json.JSONDecodeError as exc:
                raise FslError(
                    f"invalid AI event JSONL at line {lineno}: {exc.msg}",
                    kind="parse",
                ) from exc
        return events
    except json.JSONDecodeError as exc:
        raise FslError(f"invalid AI event JSON: {exc.msg}", kind="parse") from exc


def _replay_findings(component: AiComponent, events, assumptions):
    findings = []
    approvals = set()
    tools = component.tool_map()

    for index, event in enumerate(events):
        if not isinstance(event, dict):
            findings.append(_observed_finding(
                component,
                None,
                "malformed_event",
                index,
                event,
                assumptions,
                "event must be a JSON object",
            ))
            continue

        if event.get("component") not in (None, component.name):
            findings.append(_observed_finding(
                component,
                event.get("tool"),
                "component_mismatch",
                index,
                event,
                assumptions,
                f"event targets component {event.get('component')}, expected {component.name}",
            ))
            continue

        event_type = event.get("event") or event.get("type")
        if event_type == "human_approval":
            tool_name = event.get("tool")
            if tool_name not in tools:
                findings.append(_observed_finding(
                    component,
                    tool_name,
                    "approval_for_undeclared_tool",
                    index,
                    event,
                    assumptions,
                    "human approval references an undeclared tool",
                ))
                continue
            approvals.add(tool_name)
            continue

        if event_type != "tool_call":
            continue

        tool_name = event.get("tool") or event.get("name")
        mode = event.get("mode") or event.get("phase") or "execute"
        if tool_name not in tools:
            findings.append(_observed_finding(
                component,
                tool_name,
                "undeclared_tool_observed",
                index,
                event,
                assumptions,
                "observed tool call is not declared by the AI component",
            ))
            continue

        tool = tools[tool_name]
        if mode == "suggest":
            if tool_name not in component.suggestible_tools() and tool_name not in component.executable_tools():
                findings.append(_hard_finding(
                    component,
                    tool,
                    "tool_authority",
                    "suggestion_without_authority",
                    index,
                    event,
                    assumptions,
                    "tool suggestion is outside may_suggest/may_execute authority",
                ))
            continue

        if mode != "execute":
            findings.append(_observed_finding(
                component,
                tool_name,
                "unknown_tool_call_mode",
                index,
                event,
                assumptions,
                f"unknown tool_call mode '{mode}'",
            ))
            continue

        if tool_name in component.authority.forbidden:
            findings.append(_hard_finding(
                component,
                tool,
                "forbidden_tool_blocked",
                "forbidden_tool_call",
                index,
                event,
                assumptions,
                "forbidden tool was observed in execute mode",
            ))
            continue

        if tool_name not in component.executable_tools():
            findings.append(_hard_finding(
                component,
                tool,
                "tool_authority",
                "execution_without_authority",
                index,
                event,
                assumptions,
                "tool execution is outside may_execute/requires_human_approval authority",
            ))
            continue

        if tool_name in component.approval_required_tools() and tool_name not in approvals:
            findings.append(_hard_finding(
                component,
                tool,
                "human_approval_required",
                "human_approval_required_before_irreversible_tool",
                index,
                event,
                assumptions,
                "tool execution occurred before a human_approval event for the tool",
            ))

        findings.extend(_schema_findings(component, tool, event, index, assumptions))
        findings.extend(_precondition_findings(component, tool, event, index, assumptions))

    return findings


def _schema_findings(component, tool: AiTool, event, index, assumptions):
    findings = []
    if event.get("schema_valid") is False:
        findings.append(_hard_finding(
            component,
            tool,
            "tool_schema_declared",
            "tool_schema_invalid",
            index,
            event,
            assumptions,
            "runtime marked the tool call arguments as schema-invalid",
        ))
    observed_schema = event.get("tool_schema") or event.get("schema")
    if tool.schema and observed_schema and observed_schema != tool.schema:
        findings.append(_observed_finding(
            component,
            tool.name,
            "tool_schema_mismatch",
            index,
            event,
            assumptions,
            f"observed schema {observed_schema} does not match declared schema {tool.schema}",
        ))
    return findings


def _precondition_findings(component, tool: AiTool, event, index, assumptions):
    preconditions = event.get("preconditions")
    if not tool.preconditions:
        return []
    if not isinstance(preconditions, dict):
        return [_hard_finding(
            component,
            tool,
            "tool_precondition_declared",
            "business_precondition_mismatch",
            index,
            event,
            assumptions,
            "event does not carry the declared business precondition evidence",
            extra={"missing_preconditions": list(tool.preconditions)},
        )]

    findings = []
    for name in tool.preconditions:
        if preconditions.get(name) is not True:
            findings.append(_hard_finding(
                component,
                tool,
                "tool_precondition_declared",
                "business_precondition_mismatch",
                index,
                event,
                assumptions,
                f"business precondition '{name}' was not true",
                extra={"precondition": name, "observed": preconditions.get(name)},
            ))
    return findings


def _hard_finding(
        component,
        tool: Optional[AiTool],
        failed_rule,
        violation,
        event_index,
        event,
        assumptions,
        reason,
        extra=None):
    return _finding(
        kind="ai_hard_contract_violation",
        result="violated",
        severity="error",
        component=component.name,
        tool=tool.name if tool else None,
        failed_rule=failed_rule,
        violation=violation,
        guarantee_kind="syntactic_hard",
        evidence_kind="runtime_replay",
        witness=_event_witness(event_index, event, reason, extra),
        minimal_conflict_set={
            "component": component.name,
            "tool": tool.name if tool else event.get("tool"),
            "event_index": event_index,
        },
        repair_candidates=_repairs_for_violation(violation, tool.name if tool else None),
        assumptions=assumptions,
    )


def _observed_finding(component, tool, violation, event_index, event, assumptions, reason):
    return _finding(
        kind="observed_contract_violation",
        result="observed_mismatch",
        severity="error",
        component=component.name,
        tool=tool,
        failed_rule="runtime_observation",
        violation=violation,
        guarantee_kind="runtime_observed",
        evidence_kind="runtime_replay",
        witness=_event_witness(event_index, event, reason),
        minimal_conflict_set={
            "component": component.name,
            "tool": tool,
            "event_index": event_index,
        },
        repair_candidates=_repairs_for_violation(violation, tool),
        assumptions=assumptions,
    )


def _event_witness(event_index, event, reason, extra=None):
    witness = {
        "event_index": event_index,
        "reason": reason,
    }
    if isinstance(event, dict):
        for key in ("event", "type", "component", "tool", "name", "mode", "tool_schema", "schema", "schema_valid"):
            if key in event:
                witness[key] = event[key]
        if isinstance(event.get("args"), dict):
            witness["arg_keys"] = sorted(str(k) for k in event["args"])
    else:
        witness["raw_event_type"] = type(event).__name__
    if extra:
        witness.update(extra)
    return witness


def _finding(
        kind,
        result,
        severity,
        component,
        tool,
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
        "fsl": AI_DIALECT_VERSION,
        "result": result,
        "kind": kind,
        "severity": severity,
        "component": component,
        "contract": "hard" if guarantee_kind == "syntactic_hard" else "observed",
        "tool": tool,
        "failed_rule": failed_rule,
        "violation": violation,
        "guarantee_kind": guarantee_kind,
        "evidence": {
            "kind": evidence_kind,
            "formal_proof": evidence_kind in ("bmc", "induction"),
        },
        "witness": witness,
        "minimal_conflict_set": minimal_conflict_set,
        "repair_candidates": repair_candidates,
        "assumptions": assumptions,
        "redaction": {
            "policy": "tool names, schema names, and redacted event metadata only; prompts and tool args are not emitted by default",
        },
    }


def _repairs_for_violation(violation, tool):
    label = tool or "the observed tool"
    if violation in ("human_approval_required_before_irreversible_tool", "irreversible_tool_without_human_approval_guard"):
        return [
            {
                "kind": "workflow_change",
                "weakens_spec": False,
                "description": f"insert a human_approval event before executing {label}",
            },
            {
                "kind": "runtime_guard",
                "weakens_spec": False,
                "description": f"block {label} until a valid approval token exists",
            },
        ]
    if violation in ("forbidden_tool_call", "forbidden_tool_declared_executable"):
        return [
            {
                "kind": "runtime_guard",
                "weakens_spec": False,
                "description": f"block {label} before external side effects",
            },
            {
                "kind": "authority_change",
                "weakens_spec": True,
                "description": f"remove {label} from forbidden only if policy explicitly permits it",
            },
        ]
    if violation in ("tool_schema_invalid", "tool_schema_mismatch"):
        return [
            {
                "kind": "schema_change",
                "weakens_spec": False,
                "description": f"align {label}'s runtime schema with the declared tool schema",
            },
            {
                "kind": "adapter_change",
                "weakens_spec": False,
                "description": f"validate {label} arguments before execution and emit schema_valid=false on failure",
            },
        ]
    if violation == "business_precondition_mismatch":
        return [
            {
                "kind": "guard_change",
                "weakens_spec": False,
                "description": f"check {label}'s business preconditions immediately before execution",
            },
            {
                "kind": "workflow_change",
                "weakens_spec": False,
                "description": f"route {label} to fallback or human review when preconditions are not met",
            },
        ]
    return [
        {
            "kind": "declaration_change",
            "weakens_spec": False,
            "description": f"declare {label} only if it is part of the component boundary",
        },
        {
            "kind": "runtime_guard",
            "weakens_spec": False,
            "description": f"block undeclared or unsupported use of {label}",
        },
    ]
