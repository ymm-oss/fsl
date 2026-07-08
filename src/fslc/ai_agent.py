# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Structural analysis for recursive fsl-ai agent composition."""
from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, Iterable, List, Optional, Set, Tuple

from .ai_expand import AI_FINDING_SCHEMA_VERSION
from .ai_ir import AiAgent, AiDelegationEdge, AiFailurePolicy
from .model import FslError


AI_AGENT_DIALECT_VERSION = "fsl-ai-agent-mvp.v0"


@dataclass(frozen=True)
class _AgentInfo:
    agent: AiAgent
    path: Tuple[str, ...]
    parent: Optional[Tuple[str, ...]]
    available_authority: frozenset
    available_context: frozenset


def analyze_ai_agent(agent: AiAgent):
    infos = _validate_agent_tree(agent)
    assumptions = _agent_assumptions()
    findings = _agent_findings(infos, assumptions)
    return {
        "result": "violated" if findings else "agent_analyzed",
        "dialect": AI_AGENT_DIALECT_VERSION,
        "finding_schema_version": AI_FINDING_SCHEMA_VERSION,
        "ai_agent": agent.name,
        "formal_result": "not_run",
        "evidence": {
            "kind": "static_agent_graph",
            "formal_proof": False,
        },
        "guarantee_boundary": {
            "proved": "not claimed for recursive agent composition",
            "agent_structural": (
                "lexical scope, grant subset, delegation, visibility, failure policy, "
                "and tool-reachability structure"
            ),
            "runtime_replay": "not run for agent composition",
            "evaluator_supported": "outside this structural analysis",
            "statistically_supported": "outside this structural analysis",
        },
        "assumptions": assumptions,
        "agent_ir": _agent_ir(agent),
        "graph_summary": _graph_summary(infos),
        "findings": findings,
        "note": (
            "recursive agent analysis is structural only; it does not prove LLM "
            "semantic correctness or statistical/evaluator-backed quality claims"
        ),
    }


def _agent_assumptions():
    return [
        {
            "id": "AI-ASSUME-AGENT-DECLARATIONS",
            "text": "agent authority, context, tool, visibility, and orchestration declarations are complete",
        },
        {
            "id": "AI-ASSUME-NESTING-NOT-DELEGATION",
            "text": "lexical nesting defines namespace and scope only; runtime collaboration comes from orchestration edges",
        },
        {
            "id": "AI-ASSUME-NO-IMPLICIT-INHERITANCE",
            "text": "nested agents do not inherit parent authority or context without explicit grant declarations",
        },
    ]


def _validate_agent_tree(agent: AiAgent) -> Dict[Tuple[str, ...], _AgentInfo]:
    infos: Dict[Tuple[str, ...], _AgentInfo] = {}

    def walk(node: AiAgent, parent_info: Optional[_AgentInfo]) -> None:
        path = (node.name,) if parent_info is None else parent_info.path + (node.name,)

        if parent_info is None:
            if node.grants:
                grant = node.grants[0]
                _err(
                    f"top-level agent '{node.name}' cannot declare grant {grant.kind}",
                    loc=grant.loc,
                    hint="declare root authority/context directly, and grant only inside nested agents",
                )
            available_authority = frozenset(_declared_authority_boundary(node))
            available_context = frozenset(node.context)
        else:
            grant_authority = _granted(node, "authority")
            grant_context = _granted(node, "context")
            extra_authority = grant_authority - set(parent_info.available_authority)
            if extra_authority:
                _err(
                    f"agent '{_path(path)}' grant authority exceeds parent boundary: "
                    + ", ".join(sorted(extra_authority)),
                    loc=_first_grant_loc(node, "authority"),
                    hint="grant only tools/capabilities declared in the immediate parent boundary",
                )
            extra_context = grant_context - set(parent_info.available_context)
            if extra_context:
                _err(
                    f"agent '{_path(path)}' grant context exceeds parent boundary: "
                    + ", ".join(sorted(extra_context)),
                    loc=_first_grant_loc(node, "context"),
                    hint="grant only context symbols declared in the immediate parent boundary",
                )
            available_authority = frozenset(grant_authority)
            available_context = frozenset(grant_context)

        info = _AgentInfo(
            agent=node,
            path=path,
            parent=parent_info.path if parent_info else None,
            available_authority=available_authority,
            available_context=available_context,
        )
        infos[path] = info

        _validate_local_duplicates(node, path)
        child_names = {child.name for child in node.children}
        for edge in node.orchestration:
            _require_child(edge.source, child_names, node, edge.loc, "orchestration source")
            _require_child(edge.target, child_names, node, edge.loc, "orchestration target")
        for gate in node.review_gates:
            _require_child(gate, child_names, node, node.loc, "review_gate")
        for policy in node.failure_policy:
            _require_child(policy.agent, child_names, node, policy.loc, "failure_policy source")

        if node.outputs:
            _validate_output_visibility(node, parent_info, path)

        for child in node.children:
            walk(child, info)

    walk(agent, None)
    return infos


def _validate_local_duplicates(node: AiAgent, path: Tuple[str, ...]) -> None:
    _dedupe([child.name for child in node.children], "child agent", _path(path), node.loc)
    _dedupe([tool.name for tool in node.tools], "tool", _path(path), node.loc)
    _dedupe(list(node.tool_names), "tool", _path(path), node.loc)
    overlap = {tool.name for tool in node.tools} & set(node.tool_names)
    if overlap:
        _err(
            f"agent '{_path(path)}' declares duplicate tool "
            + ", ".join(sorted(overlap)),
            loc=node.loc,
        )
    _dedupe([out.name for out in node.outputs], "output", _path(path), node.loc)


def _validate_output_visibility(
    node: AiAgent,
    parent_info: Optional[_AgentInfo],
    path: Tuple[str, ...],
) -> None:
    parent_children = set()
    if parent_info is not None:
        parent_children = {child.name for child in parent_info.agent.children}
    own_children = {child.name for child in node.children}
    allowed = {"self"} | own_children
    if parent_info is not None:
        allowed.add("parent")
        allowed |= parent_children - {node.name}

    for output in node.outputs:
        unknown = set(output.visibility) - allowed
        if unknown:
            _err(
                f"agent '{_path(path)}' output '{output.name}' has unknown visibility target: "
                + ", ".join(sorted(unknown)),
                loc=output.loc,
                hint="visibility targets must be parent, self, child agents, or sibling agents in the parent scope",
            )


def _agent_findings(infos: Dict[Tuple[str, ...], _AgentInfo], assumptions):
    findings = []
    by_path = {_path(path): info for path, info in infos.items()}

    for path, info in sorted(infos.items(), key=lambda item: _path(item[0])):
        node = info.agent
        component = _path(path)
        if info.parent is not None:
            used_authority = _declared_authority_boundary(node)
            exceeded = used_authority - set(info.available_authority)
            if exceeded:
                findings.append(_finding(
                    component=component,
                    tool=_first_sorted(exceeded),
                    failed_rule="authority_grant_subset",
                    violation="child_authority_exceeds_parent_authority",
                    severity="error",
                    witness={
                        "agent": component,
                        "used_authority": sorted(used_authority),
                        "granted_authority": sorted(info.available_authority),
                        "exceeded": sorted(exceeded),
                    },
                    minimal_conflict_set={
                        "agent": component,
                        "exceeded_authority": sorted(exceeded),
                    },
                    repair_candidates=[{
                        "kind": "grant_change",
                        "weakens_spec": False,
                        "description": "grant only authority inside the parent boundary or remove the child use",
                    }],
                    assumptions=assumptions,
                ))

            context_exceeded = set(node.context) - set(info.available_context)
            if context_exceeded:
                findings.append(_finding(
                    component=component,
                    tool=None,
                    failed_rule="context_grant_subset",
                    violation="child_context_exceeds_parent_context",
                    severity="error",
                    witness={
                        "agent": component,
                        "used_context": sorted(node.context),
                        "granted_context": sorted(info.available_context),
                        "exceeded": sorted(context_exceeded),
                    },
                    minimal_conflict_set={
                        "agent": component,
                        "exceeded_context": sorted(context_exceeded),
                    },
                    repair_candidates=[{
                        "kind": "grant_change",
                        "weakens_spec": False,
                        "description": "grant only context inside the parent boundary or remove the child read",
                    }],
                    assumptions=assumptions,
                ))

        for tool in node.tools:
            if tool.irreversible and tool.name not in node.authority.requires_human_approval:
                findings.append(_finding(
                    component=component,
                    tool=tool.name,
                    failed_rule="human_approval_path",
                    violation="irreversible_operation_without_human_approval_path",
                    severity="error",
                    witness={
                        "agent": component,
                        "tool": tool.name,
                        "irreversible": True,
                        "requires_human_approval": False,
                    },
                    minimal_conflict_set={"agent": component, "tool": tool.name},
                    repair_candidates=[{
                        "kind": "authority_change",
                        "weakens_spec": False,
                        "description": f"add {tool.name} to requires_human_approval or route it to a human review state",
                    }],
                    assumptions=assumptions,
                ))

    for path, info in sorted(infos.items(), key=lambda item: _path(item[0])):
        parent = info.agent
        child_paths = {child.name: path + (child.name,) for child in parent.children}
        if not child_paths:
            continue
        reachability = _reachability(parent.orchestration, child_paths.keys())

        for child in parent.children:
            source_path = child_paths[child.name]
            for output in child.outputs:
                for target in output.visibility:
                    if target in ("parent", "self") or target not in child_paths:
                        continue
                    if target not in reachability.get(child.name, set()):
                        findings.append(_finding(
                            component=_path(source_path),
                            tool=None,
                            failed_rule="visibility_requires_delegation",
                            violation="visibility_leak_across_sibling_agents",
                            severity="error",
                            witness={
                                "output": output.name,
                                "source_agent": _path(source_path),
                                "target_agent": _path(child_paths[target]),
                                "delegation_path_exists": False,
                            },
                            minimal_conflict_set={
                                "source_agent": _path(source_path),
                                "target_agent": _path(child_paths[target]),
                                "output": output.name,
                            },
                            repair_candidates=[{
                                "kind": "orchestration_change",
                                "weakens_spec": False,
                                "description": "add an orchestration path for the declared sibling visibility or remove the visibility target",
                            }],
                            assumptions=assumptions,
                        ))

        for source_name, reachable in sorted(reachability.items()):
            source_info = by_path.get(_path(child_paths[source_name]))
            if source_info is None or source_info.agent.trust != "low":
                continue
            for target_name in sorted(reachable):
                target = child_paths[target_name]
                high_tools = _high_authority_tools(infos[target].agent)
                if high_tools:
                    findings.append(_finding(
                        component=_path(child_paths[source_name]),
                        tool=high_tools[0],
                        failed_rule="tool_reachability_graph",
                        violation="low_trust_agent_path_to_high_authority_tool",
                        severity="error",
                        witness={
                            "source_agent": _path(child_paths[source_name]),
                            "source_trust": "low",
                            "target_agent": _path(target),
                            "high_authority_tools": high_tools,
                        },
                        minimal_conflict_set={
                            "source_agent": _path(child_paths[source_name]),
                            "target_agent": _path(target),
                        },
                        repair_candidates=[{
                            "kind": "orchestration_change",
                            "weakens_spec": False,
                            "description": "route low-trust output through a review gate before it can influence high-authority tools",
                        }],
                        assumptions=assumptions,
                    ))

        if parent.review_gates:
            gates = set(parent.review_gates)
            for source_name, reachable in sorted(reachability.items()):
                if source_name in gates:
                    continue
                for target_name in sorted(reachable):
                    if target_name in gates:
                        continue
                    target = child_paths[target_name]
                    if not _high_authority_tools(infos[target].agent):
                        continue
                    has_review_path = any(
                        gate in reachable
                        and target_name in reachability.get(gate, set())
                        for gate in gates
                    )
                    if not has_review_path:
                        findings.append(_finding(
                            component=_path(path),
                            tool=_high_authority_tools(infos[target].agent)[0],
                            failed_rule="policy_review_gate",
                            violation="policy_review_bypass_in_orchestration",
                            severity="error",
                            witness={
                                "parent_agent": _path(path),
                                "source_agent": _path(child_paths[source_name]),
                                "target_agent": _path(target),
                                "review_gates": sorted(gates),
                            },
                            minimal_conflict_set={
                                "parent_agent": _path(path),
                                "source_agent": _path(child_paths[source_name]),
                                "target_agent": _path(target),
                            },
                            repair_candidates=[{
                                "kind": "orchestration_change",
                                "weakens_spec": False,
                                "description": "insert the declared review gate on paths to high-authority agents",
                            }],
                            assumptions=assumptions,
                        ))

    return findings


def _agent_ir(agent: AiAgent, prefix: Tuple[str, ...] = ()):
    path = prefix + (agent.name,)
    return {
        "path": _path(path),
        "name": agent.name,
        "model": agent.model,
        "prompt": agent.prompt,
        "trust": agent.trust,
        "context": list(agent.context),
        "tools": [_tool_ir(tool) for tool in agent.tools],
        "tool_names": list(agent.tool_names),
        "authority": _authority_ir(agent),
        "grants": [
            {"kind": grant.kind, "names": list(grant.names)}
            for grant in agent.grants
        ],
        "outputs": [
            {"name": output.name, "visibility": list(output.visibility)}
            for output in agent.outputs
        ],
        "review_gates": list(agent.review_gates),
        "orchestration": [
            {
                "source": edge.source,
                "target": edge.target,
                "source_path": _path(path + (edge.source,)),
                "target_path": _path(path + (edge.target,)),
            }
            for edge in agent.orchestration
        ],
        "failure_policy": [_failure_ir(policy, path) for policy in agent.failure_policy],
        "contracts": [
            {"hard_rules": list(contract.hard_rules)}
            for contract in agent.contracts
        ],
        "children": [_agent_ir(child, path) for child in agent.children],
    }


def _graph_summary(infos: Dict[Tuple[str, ...], _AgentInfo]):
    scope_tree = []
    authority_graph = []
    information_flow_graph = []
    delegation_graph = []
    tool_reachability_graph = []
    failure_policies = []

    for path, info in sorted(infos.items(), key=lambda item: _path(item[0])):
        node = info.agent
        child_paths = {child.name: path + (child.name,) for child in node.children}
        scope_tree.append({
            "path": _path(path),
            "parent": _path(info.parent) if info.parent else None,
            "children": [_path(child_paths[child.name]) for child in node.children],
        })
        if info.parent is not None:
            authority_graph.append({
                "agent": _path(path),
                "parent": _path(info.parent),
                "granted_authority": sorted(info.available_authority),
                "granted_context": sorted(info.available_context),
            })
        for edge in node.orchestration:
            delegation_graph.append({
                "parent": _path(path),
                "source": _path(path + (edge.source,)),
                "target": _path(path + (edge.target,)),
            })
        for output in node.outputs:
            for target in output.visibility:
                information_flow_graph.append({
                    "source": f"{_path(path)}.output.{output.name}",
                    "target": _visibility_target_path(path, info.parent, target),
                })
        for tool_name, tool in sorted(node.tool_map().items()):
            tool_reachability_graph.append({
                "agent": _path(path),
                "tool": tool_name,
                "irreversible": tool.irreversible,
                "requires_human_approval": tool_name in node.authority.requires_human_approval,
                "may_execute": tool_name in node.authority.may_execute,
                "may_suggest": tool_name in node.authority.may_suggest,
            })
        for policy in node.failure_policy:
            failure_policies.append(_failure_ir(policy, path))

    return {
        "scope_tree": scope_tree,
        "delegation_graph": delegation_graph,
        "authority_graph": authority_graph,
        "information_flow_graph": information_flow_graph,
        "tool_reachability_graph": tool_reachability_graph,
        "failure_policy": failure_policies,
    }


def _authority_ir(agent: AiAgent):
    return {
        "may_suggest": list(agent.authority.may_suggest),
        "may_execute": list(agent.authority.may_execute),
        "requires_human_approval": list(agent.authority.requires_human_approval),
        "forbidden": list(agent.authority.forbidden),
    }


def _tool_ir(tool):
    return {
        "name": tool.name,
        "schema": tool.schema,
        "irreversible": tool.irreversible,
        "preconditions": list(tool.preconditions),
        "effect": tool.effect,
    }


def _failure_ir(policy: AiFailurePolicy, path: Tuple[str, ...]):
    return {
        "source": _path(path + (policy.agent,)),
        "condition": policy.condition,
        "action": policy.action,
        "target": policy.target,
        "retry_limit": policy.retry_limit,
    }


def _visibility_target_path(path, parent, target):
    if target == "self":
        return _path(path)
    if target == "parent":
        return _path(parent) if parent else None
    if parent:
        return _path(parent + (target,))
    return _path(path + (target,))


def _granted(agent: AiAgent, kind: str) -> Set[str]:
    out: Set[str] = set()
    for grant in agent.grants:
        if grant.kind == kind:
            out.update(grant.names)
    return out


def _first_grant_loc(agent: AiAgent, kind: str):
    for grant in agent.grants:
        if grant.kind == kind:
            return grant.loc
    return agent.loc


def _declared_authority_boundary(agent: AiAgent) -> Set[str]:
    return agent.all_tool_names() | agent.authority_names()


def _high_authority_tools(agent: AiAgent) -> List[str]:
    names = {
        tool.name for tool in agent.tools
        if tool.irreversible or tool.name in agent.authority.requires_human_approval
    }
    names.update(agent.authority.requires_human_approval)
    return sorted(names)


def _reachability(edges: Iterable[AiDelegationEdge], nodes: Iterable[str]):
    graph = {node: set() for node in nodes}
    for edge in edges:
        graph.setdefault(edge.source, set()).add(edge.target)
        graph.setdefault(edge.target, set())

    closure = {node: set(targets) for node, targets in graph.items()}
    changed = True
    while changed:
        changed = False
        for node, targets in list(closure.items()):
            expanded = set(targets)
            for target in targets:
                expanded |= closure.get(target, set())
            if expanded != targets:
                closure[node] = expanded
                changed = True
    return closure


def _require_child(name, child_names, node, loc, label):
    if name not in child_names:
        _err(
            f"{label} '{name}' is not a child agent of '{node.name}'",
            loc=loc,
            hint="orchestration and failure_policy edges are separate from lexical nesting and reference immediate children",
        )


def _dedupe(values, label, path, loc):
    seen = set()
    for value in values:
        if value in seen:
            _err(f"agent '{path}' declares duplicate {label} '{value}'", loc=loc)
        seen.add(value)


def _finding(
    component,
    tool,
    failed_rule,
    violation,
    severity,
    witness,
    minimal_conflict_set,
    repair_candidates,
    assumptions,
):
    return {
        "schema_version": AI_FINDING_SCHEMA_VERSION,
        "fsl": AI_AGENT_DIALECT_VERSION,
        "result": "violated",
        "kind": "agent_structural_violation",
        "severity": severity,
        "component": component,
        "contract": "agent_structure",
        "tool": tool,
        "failed_rule": failed_rule,
        "violation": violation,
        "guarantee_kind": "agent_structural",
        "evidence": {
            "kind": "static_agent_graph",
            "formal_proof": False,
        },
        "witness": witness,
        "minimal_conflict_set": minimal_conflict_set,
        "repair_candidates": repair_candidates,
        "assumptions": assumptions,
        "redaction": {
            "policy": "agent, tool, context, and graph labels only; prompts and tool args are not emitted",
        },
    }


def _path(path: Optional[Tuple[str, ...]]) -> Optional[str]:
    if path is None:
        return None
    return ".".join(path)


def _first_sorted(values):
    return sorted(values)[0] if values else None


def _err(message, loc=None, hint=None):
    raise FslError(message, kind="semantics", loc=loc, hint=hint)
