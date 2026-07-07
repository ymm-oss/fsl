# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Lower fsl-ai hard-contract IR into the existing FSL kernel AST."""
from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Dict, List, Optional, Tuple

from .ai_ir import AiComponent, AiTool
from .model import FslError


AI_DIALECT_VERSION = "fsl-ai-hard-mvp.v0"
AI_FINDING_SCHEMA_VERSION = "fsl-ai-finding.v0"

DEFAULT_HARD_RULES = (
    "tool_authority",
    "human_approval_required",
    "forbidden_tool_blocked",
    "tool_schema_declared",
    "tool_precondition_declared",
)
ALLOWED_HARD_RULES = frozenset(DEFAULT_HARD_RULES)


@dataclass(frozen=True)
class AiKernelExpansion:
    ast: tuple
    display_names: Dict[str, str]
    invariant_metadata: Dict[str, dict]
    assumptions: List[dict]


def _err(message, loc=None, hint=None):
    raise FslError(message, kind="semantics", loc=loc, hint=hint)


def _safe(name):
    out = re.sub(r"[^A-Za-z0-9_]", "_", name)
    if not out:
        out = "x"
    if out[0].isdigit():
        out = "_" + out
    return out


def _tool_member(name):
    return "tool_" + _safe(name)


def _num(n):
    return ("num", int(n))


def _bool(value):
    return ("bool", bool(value))


def _var(name):
    return ("var", name)


def _idx(name, key):
    return ("index", ("var", name), key)


def _assign_index(name, key, value, loc=None):
    return ("assign", ("index", name, key), value, loc)


def _assign_var(name, value, loc=None):
    return ("assign", ("var", name), value, loc)


def _bin(op, left, right):
    return ("bin", op, left, right)


def _not(expr):
    return ("not", expr)


def _meta(rule, text):
    return {"id": rule, "text": text}


def _rule_set(component: AiComponent):
    rules = list(component.check.rules or DEFAULT_HARD_RULES)
    for rule in rules:
        if rule not in ALLOWED_HARD_RULES:
            _err(
                f"unknown ai hard-contract rule '{rule}'",
                loc=component.check.loc,
                hint="supported rules: " + ", ".join(sorted(ALLOWED_HARD_RULES)),
            )
    return set(rules)


def _dedupe(values, label, loc=None):
    seen = set()
    out = []
    for value in values:
        if value in seen:
            _err(f"duplicate {label} '{value}'", loc=loc)
        seen.add(value)
        out.append(value)
    return tuple(out)


def validate_ai_component(component: AiComponent):
    if not component.tools:
        _err("ai_component requires at least one tool in the hard-contract MVP", loc=component.loc)

    names = set()
    for tool in component.tools:
        if tool.name in names:
            _err(f"duplicate tool '{tool.name}'", loc=tool.loc)
        names.add(tool.name)
        _dedupe(tool.preconditions, f"precondition on tool '{tool.name}'", tool.loc)

    authority = component.authority
    for label, declared in (
        ("may_suggest", authority.may_suggest),
        ("may_execute", authority.may_execute),
        ("requires_human_approval", authority.requires_human_approval),
        ("forbidden", authority.forbidden),
    ):
        _dedupe(declared, f"authority {label} tool", authority.loc)
        for tool_name in declared:
            if tool_name not in names:
                _err(
                    f"authority {label} references unknown tool '{tool_name}'",
                    loc=authority.loc,
                    hint="declare the tool before referencing it in authority",
                )

    _rule_set(component)


def _tool_members(component: AiComponent):
    return {tool.name: _tool_member(tool.name) for tool in component.tools}


def _action_name(prefix, tool_name):
    return f"{prefix}_{_safe(tool_name)}"


def expand_ai_component(component: AiComponent):
    validate_ai_component(component)
    rules = _rule_set(component)
    members = _tool_members(component)
    generated_names = []

    items = [
        ("__spec_meta", _meta("ai", "AI hard-contract runtime guard")),
        ("enum", "Tool", [members[tool.name] for tool in component.tools]),
        ("state", [
            ("decl", "human_approved", ("map", ("name", "Tool"), ("bool",))),
            ("decl", "tool_executed", ("map", ("name", "Tool"), ("bool",))),
            ("decl", "tool_suggested", ("map", ("name", "Tool"), ("bool",))),
            ("decl", "fallback_required", ("bool",)),
        ]),
    ]

    init = [
        ("forall_stmt", ("binder_typed", "t", "Tool", None), [
            _assign_index("human_approved", _var("t"), _bool(False)),
            _assign_index("tool_executed", _var("t"), _bool(False)),
            _assign_index("tool_suggested", _var("t"), _bool(False)),
        ], None),
        _assign_var("fallback_required", _bool(False), component.loc),
    ]
    items.append(("init", init))

    suggestible = component.suggestible_tools()
    executable = component.executable_tools()
    approval_required = component.approval_required_tools()
    forbidden = set(component.authority.forbidden)

    for tool_name in sorted(suggestible):
        member = _var(members[tool_name])
        name = _action_name("suggest", tool_name)
        generated_names.append(name)
        items.append(("action", name, [], [
            _assign_index("tool_suggested", member, _bool(True)),
        ], component.tool_map()[tool_name].loc, False, _meta(
            "AI-AUTHORITY",
            f"{component.name} may suggest {tool_name}",
        )))

    for tool_name in sorted(approval_required - forbidden):
        member = _var(members[tool_name])
        name = _action_name("approve", tool_name)
        generated_names.append(name)
        items.append(("action", name, [], [
            _assign_index("human_approved", member, _bool(True)),
        ], component.tool_map()[tool_name].loc, False, _meta(
            "AI-HUMAN-APPROVAL",
            f"human approval token is finite state for {tool_name}",
        )))

    for tool_name in sorted(executable - forbidden):
        tool = component.tool_map()[tool_name]
        member = _var(members[tool_name])
        body = []
        if tool_name in approval_required:
            body.append(("requires", _idx("human_approved", member), tool.loc))
        body.append(_assign_index("tool_executed", member, _bool(True), tool.loc))
        name = _action_name("execute", tool_name)
        generated_names.append(name)
        items.append(("action", name, [], body, tool.loc, False, _meta(
            "AI-TOOL-EXECUTE",
            f"{component.name} may execute {tool_name}"
            + (" after human approval" if tool_name in approval_required else ""),
        )))

    if not generated_names:
        generated_names.append("observe_component")
        items.append(("action", "observe_component", [], [
            _assign_var("fallback_required", _var("fallback_required"), component.loc),
        ], component.loc, False, _meta(
            "AI-OBSERVE",
            f"{component.name} has no executable hard-contract transition",
        )))

    invariant_metadata = {}
    if "forbidden_tool_blocked" in rules:
        for tool_name in sorted(forbidden):
            member = _var(members[tool_name])
            name = "ai_forbidden_tool_not_executed__" + _safe(tool_name)
            expr = _not(_idx("tool_executed", member))
            meta = _meta("AI-FORBIDDEN", f"{tool_name} is forbidden and cannot be executed")
            items.append(("invariant", name, expr, component.tool_map()[tool_name].loc, meta))
            generated_names.append(name)
            invariant_metadata[name] = {
                "kind": "ai_hard_contract_violation",
                "violation": "forbidden_tool_call",
                "failed_rule": "forbidden_tool_blocked",
                "tool": tool_name,
                "guarantee_kind": "syntactic_hard",
            }

    if "human_approval_required" in rules:
        for tool_name in sorted(approval_required - forbidden):
            member = _var(members[tool_name])
            name = "ai_approval_before_execute__" + _safe(tool_name)
            expr = _bin("=>", _idx("tool_executed", member), _idx("human_approved", member))
            meta = _meta(
                "AI-HUMAN-APPROVAL",
                f"{tool_name} can execute only after human approval",
            )
            items.append(("invariant", name, expr, component.tool_map()[tool_name].loc, meta))
            generated_names.append(name)
            invariant_metadata[name] = {
                "kind": "ai_hard_contract_violation",
                "violation": "human_approval_required_before_irreversible_tool",
                "failed_rule": "human_approval_required",
                "tool": tool_name,
                "guarantee_kind": "syntactic_hard",
            }

    for fallback in component.fallback:
        name = _action_name("fallback", fallback.reason)
        generated_names.append(name)
        items.append(("action", name, [], [
            _assign_var("fallback_required", _bool(True), fallback.loc),
        ], fallback.loc, False, _meta(
            "AI-FALLBACK",
            f"{fallback.reason} requires {fallback.target}",
        )))

    items.append(("terminal", _bool(False), None))
    items.append(("__generated", generated_names))

    assumptions = [
        {
            "id": "AI-ASSUME-CAPABILITY-DECLARATIONS",
            "text": "tool and authority declarations are complete for the checked AI component boundary",
        },
        {
            "id": "AI-ASSUME-RUNTIME-GUARD",
            "text": "hard contracts are enforced by the runtime guard before external tool side effects occur",
        },
        {
            "id": "AI-ASSUME-NO-PROBABILITY-IN-KERNEL",
            "text": "Phase 1 hard-contract checks add no probability, percentile, or evaluator semantics to the kernel",
        },
    ]

    display_names = {
        "human_approved": "ai.human_approved",
        "tool_executed": "ai.tool_executed",
        "tool_suggested": "ai.tool_suggested",
        "fallback_required": "ai.fallback_required",
    }
    return AiKernelExpansion(
        ast=("spec", component.name, items),
        display_names=display_names,
        invariant_metadata=invariant_metadata,
        assumptions=assumptions,
    )


def static_policy_findings(component: AiComponent, assumptions: Optional[List[dict]] = None):
    validate_ai_component(component)
    assumptions = list(assumptions or [])
    findings = []
    authority = component.authority
    forbidden = set(authority.forbidden)
    executable = component.executable_tools()
    approval_required = set(authority.requires_human_approval)

    for tool_name in sorted(forbidden & executable):
        findings.append(_finding(
            kind="ai_hard_contract_violation",
            result="violated",
            severity="error",
            component=component.name,
            tool=tool_name,
            failed_rule="tool_authority",
            violation="forbidden_tool_declared_executable",
            guarantee_kind="syntactic_hard",
            evidence_kind="static_check",
            witness={
                "tool": tool_name,
                "authority": ["forbidden", "may_execute" if tool_name in authority.may_execute else "requires_human_approval"],
            },
            minimal_conflict_set={"component": component.name, "tool": tool_name},
            repair_candidates=_authority_repairs(tool_name),
            assumptions=assumptions,
        ))

    for tool in component.tools:
        if tool.irreversible and tool.name not in approval_required and tool.name not in forbidden:
            findings.append(_finding(
                kind="ai_hard_contract_violation",
                result="violated",
                severity="error",
                component=component.name,
                tool=tool.name,
                failed_rule="human_approval_required",
                violation="irreversible_tool_without_human_approval_guard",
                guarantee_kind="syntactic_hard",
                evidence_kind="static_check",
                witness={
                    "tool": tool.name,
                    "irreversible": True,
                    "requires_human_approval": False,
                },
                minimal_conflict_set={"component": component.name, "tool": tool.name},
                repair_candidates=_approval_repairs(tool.name),
                assumptions=assumptions,
            ))

    for tool in component.tools:
        if tool.name in executable and not tool.schema:
            findings.append(_finding(
                kind="ai_hard_contract_violation",
                result="violated",
                severity="error",
                component=component.name,
                tool=tool.name,
                failed_rule="tool_schema_declared",
                violation="executable_tool_without_schema",
                guarantee_kind="syntactic_hard",
                evidence_kind="static_check",
                witness={"tool": tool.name, "schema": None},
                minimal_conflict_set={"component": component.name, "tool": tool.name},
                repair_candidates=[{
                    "kind": "schema_declaration",
                    "weakens_spec": False,
                    "description": f"declare the input schema expected for {tool.name}",
                }],
                assumptions=assumptions,
            ))

    return findings


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
        "contract": "hard",
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


def _authority_repairs(tool_name):
    return [
        {
            "kind": "authority_change",
            "weakens_spec": False,
            "description": f"remove {tool_name} from executable authority or from forbidden, after policy review",
        },
        {
            "kind": "runtime_guard",
            "weakens_spec": False,
            "description": f"block {tool_name} before side effects when forbidden authority applies",
        },
    ]


def _approval_repairs(tool_name):
    return [
        {
            "kind": "authority_change",
            "weakens_spec": False,
            "description": f"add {tool_name} to requires_human_approval",
        },
        {
            "kind": "workflow_change",
            "weakens_spec": False,
            "description": f"insert a human approval token before executing {tool_name}",
        },
        {
            "kind": "tool_change",
            "weakens_spec": True,
            "description": f"mark {tool_name} reversible only if the external side effect is actually reversible",
        },
    ]
