# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""AI-readable structural review findings."""
from __future__ import annotations

from .graph import representative_cycles
from .projections import project_tsg
from .schema import FINDINGS_SCHEMA_VERSION
from .tsg import PROPERTY_NODE_KINDS, SCENARIO_NODE_KINDS, build_tsg, expr_reads, node_by_id


def analyze(spec, profile="ai-review"):
    if profile != "ai-review":
        raise ValueError(f"unsupported profile: {profile}")
    tsg = build_tsg(spec)
    findings = []
    findings.extend(_disconnected_requirements(tsg))
    findings.extend(_unanchored_properties(tsg))
    findings.extend(_progressless_cycles(spec, tsg))
    findings = _renumber(findings)
    return {
        "analysis": "structure",
        "profile": profile,
        "schema_version": FINDINGS_SCHEMA_VERSION,
        "findings": findings,
    }


def _disconnected_requirements(tsg):
    nodes = node_by_id(tsg)
    covers_by_requirement = {}
    for e in tsg["edges"]:
        if e["kind"] == "covers":
            covers_by_requirement.setdefault(e["from"], set()).add(e["to"])
    findings = []
    useful_kinds = PROPERTY_NODE_KINDS | SCENARIO_NODE_KINDS | {"action", "kpi", "control"}
    for req in sorted((n for n in tsg["nodes"] if n["kind"] == "requirement"), key=lambda n: n["id"]):
        covered = covers_by_requirement.get(req["id"], set())
        useful = sorted(t for t in covered if (nodes.get(t) or {}).get("kind") in useful_kinds)
        if useful:
            continue
        findings.append(_finding(
            "disconnected_requirement",
            [req["id"]],
            {
                "kind": "isolated_node",
                "node": req["id"],
            },
            "The requirement is declared but is not connected to an action, property, acceptance scenario, forbidden scenario, governance control, or refinement mapping in the structural graph.",
            [
                {
                    "kind": "add_traceability_anchor",
                    "template": "Attach the requirement id to a relevant action/property or add an acceptance/forbidden scenario.",
                }
            ],
            [
                "The requirement is invalid.",
                "The implementation is missing behavior.",
            ],
            confidence=0.8,
        ))
    return findings


def _unanchored_properties(tsg):
    nodes = node_by_id(tsg)
    property_reads = {}
    action_states = set()
    scenario_nodes = {n["id"] for n in tsg["nodes"] if n["kind"] in SCENARIO_NODE_KINDS}
    scenario_actions = set()
    scenario_states = set()

    for e in tsg["edges"]:
        src_node = nodes.get(e["from"]) or {}
        dst_node = nodes.get(e["to"]) or {}
        if src_node.get("kind") in PROPERTY_NODE_KINDS and e["kind"] in {"reads", "checks"} and dst_node.get("kind") == "state":
            property_reads.setdefault(e["from"], set()).add(e["to"])
        if src_node.get("kind") == "action" and dst_node.get("kind") == "state" and e["kind"] in {"reads", "writes"}:
            action_states.add(e["to"])
        if e["from"] in scenario_nodes and dst_node.get("kind") == "action":
            scenario_actions.add(e["to"])
        if e["from"] in scenario_nodes and dst_node.get("kind") == "state":
            scenario_states.add(e["to"])

    action_related_states = set(action_states) | set(scenario_states)
    findings = []
    for prop in sorted((n for n in tsg["nodes"] if n["kind"] in PROPERTY_NODE_KINDS), key=lambda n: n["id"]):
        if prop.get("meta"):
            continue
        reads = property_reads.get(prop["id"], set())
        if reads and reads.intersection(action_related_states):
            continue
        if scenario_actions and prop["kind"] == "reachable":
            # A reachable can be intentionally scenario-only. Prefer not to flag when
            # scenarios exist but expression-to-scenario linkage is too weak.
            continue
        findings.append(_finding(
            "unanchored_property",
            [prop["id"]],
            {
                "kind": "unanchored_node",
                "node": prop["id"],
                "reads": sorted(reads),
            },
            "The user property is not connected to requirement metadata, scenarios, governance metadata, or an action-state anchor in the structural graph.",
            [
                {
                    "kind": "add_traceability_anchor",
                    "template": "Attach a requirement tag or add a scenario/action-state anchor that explains why this property exists.",
                }
            ],
            [
                "The property is wrong.",
                "The property should be deleted.",
            ],
            confidence=0.7,
        ))
    return findings


def _progressless_cycles(spec, tsg):
    projection_nodes, projection_edges = project_tsg(tsg, "action_state_graph")
    action_nodes, action_edges, bridges = _action_dependency_graph(projection_nodes, projection_edges)
    cycles = representative_cycles(action_nodes, action_edges)
    if not cycles:
        return []

    action_ids = {n["id"] for n in tsg["nodes"] if n["kind"] == "action"}
    action_meta = {f"action:{a['name']}": a.get("meta") for a in spec.get("actions") or []}
    scenario_actions = _scenario_actions(tsg)
    progress_stories = _progress_story_nodes(spec)
    findings = []

    for cycle in cycles:
        cycle_set = set(cycle)
        cycle_actions = sorted(cycle_set.intersection(action_ids))
        if len(cycle_actions) < 2:
            continue
        expanded_cycle, cycle_states = _expand_action_cycle(cycle, bridges)
        tagged = any(action_meta.get(a) for a in cycle_actions)
        scenario_relevant = bool(set(cycle_actions).intersection(scenario_actions))
        if not (tagged or scenario_relevant):
            continue
        attached = _attached_progress(progress_stories, cycle_states, cycle_actions)
        if attached:
            continue
        findings.append(_finding(
            "progressless_cycle",
            sorted(set(expanded_cycle)),
            {
                "kind": "representative_cycle",
                "steps": expanded_cycle,
                "attached_progress": [],
            },
            "This requirement/scenario-linked cycle has no explicit leadsTo, bounded exit, terminal exit, or fairness condition attached.",
            [
                {
                    "kind": "add_property",
                    "template": "Add a leadsTo property that states the cyclic state eventually reaches a terminal state.",
                },
                {
                    "kind": "strengthen_model",
                    "template": "Introduce an explicit bound and terminal state for the cyclic behavior.",
                },
                {
                    "kind": "mark_or_fix_fairness",
                    "template": "Mark the progress-driving action fair, or add a guard/model change that makes progress explicit.",
                },
            ],
            [
                "The cycle is wrong.",
                "The spec violates liveness.",
                "A high cycle count is itself a defect.",
            ],
            confidence=0.68,
        ))
    return findings


def _action_dependency_graph(nodes, edges):
    action_nodes = sorted(n["id"] for n in nodes if n["kind"] == "action")
    state_writers = {}
    state_readers = {}
    for e in edges:
        if e["kind"] == "writes" and e["from"].startswith("action:") and e["to"].startswith("state:"):
            state_writers.setdefault(e["to"], set()).add(e["from"])
        elif e["kind"] == "read_by" and e["from"].startswith("state:") and e["to"].startswith("action:"):
            state_readers.setdefault(e["from"], set()).add(e["to"])
    dep_edges = []
    bridges = {}
    for state in sorted(set(state_writers) & set(state_readers)):
        for writer in sorted(state_writers[state]):
            for reader in sorted(state_readers[state]):
                if writer == reader:
                    continue
                dep_edges.append({"from": writer, "to": reader, "kind": "enables"})
                bridges.setdefault((writer, reader), state)
    return action_nodes, dep_edges, bridges


def _expand_action_cycle(cycle, bridges):
    if not cycle:
        return [], []
    expanded = []
    states = []
    for idx, action in enumerate(cycle[:-1]):
        nxt = cycle[idx + 1]
        expanded.append(action)
        state = bridges.get((action, nxt))
        if state:
            expanded.append(state)
            states.append(state)
    expanded.append(cycle[-1])
    return expanded, sorted(set(states))


def _progress_story_nodes(spec):
    state_names = set(spec.get("state") or {})
    stories = []
    for item in spec.get("leadstos") or []:
        reads = expr_reads(item.get("P"), state_names)
        reads.update(expr_reads(item.get("Q"), state_names))
        stories.append({
            "kind": "leadsTo",
            "id": f"leadsTo:{item['name']}",
            "states": {f"state:{name}" for name in reads},
            "strong": bool(item.get("within") is not None or item.get("decreases") is not None),
        })
    for action in spec.get("actions") or []:
        if action.get("fair"):
            stories.append({
                "kind": "fair_action",
                "id": f"action:{action['name']}",
                "actions": {f"action:{action['name']}"},
                "strong": True,
            })
    if spec.get("terminal") is not None:
        reads = expr_reads(spec.get("terminal"), state_names)
        stories.append({
            "kind": "terminal",
            "id": "terminal",
            "states": {f"state:{name}" for name in reads},
            "strong": True,
        })
    return stories


def _attached_progress(stories, cycle_states, cycle_actions):
    state_set = set(cycle_states)
    action_set = set(cycle_actions)
    attached = []
    for story in stories:
        if story.get("states", set()).intersection(state_set):
            attached.append(story)
        elif story.get("actions", set()).intersection(action_set):
            attached.append(story)
    return attached


def _scenario_actions(tsg):
    out = set()
    scenario_ids = {n["id"] for n in tsg["nodes"] if n["kind"] in SCENARIO_NODE_KINDS}
    for e in tsg["edges"]:
        if e["from"] in scenario_ids and e["to"].startswith("action:"):
            out.add(e["to"])
    return out


def _finding(finding_type, involved_nodes, witness, why, repairs, caveats, confidence):
    return {
        "finding_id": "",
        "analysis": "structure",
        "finding_type": finding_type,
        "severity": "review_required",
        "confidence": confidence,
        "formal_status": "not_a_violation",
        "involved_nodes": involved_nodes,
        "witness": witness,
        "why_it_matters": why,
        "candidate_repairs": repairs,
        "do_not_assume": caveats,
    }


def _renumber(findings):
    counters = {}
    out = []
    for finding in sorted(findings, key=lambda f: (f["finding_type"], f["involved_nodes"])):
        ftype = finding["finding_type"]
        counters[ftype] = counters.get(ftype, 0) + 1
        prefix = ftype.replace("_", "-").upper()
        item = dict(finding)
        item["finding_id"] = f"STRUCT-{prefix}-{counters[ftype]:04d}"
        out.append(item)
    return out
