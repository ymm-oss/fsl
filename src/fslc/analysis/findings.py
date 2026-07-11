# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""AI-readable structural review findings."""
from __future__ import annotations

from .graph import representative_cycles
from .invariants import conservation_candidates
from .projections import build_action_dependency_graph, project_tsg
from .schema import FINDINGS_SCHEMA_VERSION
from .tsg import PROPERTY_NODE_KINDS, SCENARIO_NODE_KINDS, build_tsg, expr_reads, node_by_id
from .tag_review import tag_drift_candidates


def analyze(spec, profile="ai-review"):
    if profile != "ai-review":
        raise ValueError(f"unsupported profile: {profile}")
    tsg = build_tsg(spec)
    findings = []
    findings.extend(_disconnected_requirements(tsg))
    findings.extend(_unanchored_properties(tsg))
    findings.extend(_progressless_cycles(spec, tsg))
    findings.extend(_unwritten_state(tsg))
    findings.extend(_unread_state(tsg))
    findings.extend(_unguarded_actions(tsg))
    findings.extend(_tag_drift_findings(spec))
    findings.extend(_conservation_candidate_findings(spec))
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


def _tag_drift_findings(spec):
    findings = []
    for candidate in tag_drift_candidates(spec):
        declaration = candidate["declaration"]
        identifiers = candidate["identifiers"]
        if candidate["finding_type"] == "tag_stale_reference":
            witness_kind = "tag_mentions_unknown_identifier"
            why = "The declaration tag contains a code-shaped identifier that is not present in the current specification, which may be a stale reference after a rename or deletion."
            repair = "Update the tag to the current identifier, or confirm that the token is prose and quote/reword it so it is not presented as an FSL identifier."
            caveat = "The analyzer does not prove that the prose intended to reference an FSL identifier."
        else:
            witness_kind = "tag_identifier_absent_from_formula"
            why = "The tag names a current state variable or constant that the tagged formal definition does not reference, so the human label and checked formula may have drifted apart."
            repair = "Review the tag and formal definition together; update whichever side no longer expresses the intended requirement."
            caveat = "Identifier overlap is not proof that natural-language and formal meanings agree."
        findings.append(_finding(
            candidate["finding_type"],
            [declaration["node_id"]],
            {
                "kind": witness_kind,
                "declaration": {
                    "kind": declaration["kind"],
                    "name": declaration["name"],
                    "tag": declaration["tag"],
                },
                "identifiers": identifiers,
                "formal_identifiers": declaration["formal_identifiers"],
            },
            why,
            [{"kind": "review_tag_formula_pair", "template": repair}],
            [caveat, "This finding is not a verifier violation."],
            confidence=0.82 if candidate["finding_type"] == "tag_stale_reference" else 0.74,
            loc=declaration.get("loc"),
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
    action_nodes, action_edges, bridges = build_action_dependency_graph(projection_nodes, projection_edges)
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


def _unread_state(tsg):
    nodes = node_by_id(tsg)
    relevant = _transitively_relevant_state(tsg)
    writers = _state_writers(tsg)
    findings = []
    for state in sorted((n for n in tsg["nodes"] if n["kind"] == "state"), key=lambda n: n["id"]):
        state_id = state["id"]
        state_writers = sorted(writers.get(state_id, set()))
        if not state_writers or state_id in relevant:
            continue
        if any((nodes.get(writer) or {}).get("meta") for writer in state_writers):
            continue
        findings.append(_finding(
            "unread_state",
            [state_id],
            {
                "kind": "state_influences_no_check",
                "node": state_id,
                "writers": state_writers,
                "relevance_seed_kinds": sorted(_RELEVANCE_SEED_KINDS),
                "message": "No transitive relevance chain reaches a guard, property, ensures clause, or scenario.",
            },
            "The state variable is written, but its value does not transitively influence a guard, property, ensures clause, or acceptance/forbidden scenario in the structural graph.",
            [
                {
                    "kind": "add_property_or_guard",
                    "template": "Add the missing invariant/trans/leadsTo/reachable, scenario expectation, ensures clause, or guard that consumes this state if it is part of the contract.",
                },
                {
                    "kind": "review_state_role",
                    "template": "If this is intentional audit/history/ghost state, tag or document the writing action so reviewers know why the state is externally consumed.",
                },
            ],
            [
                "The state variable is safe to delete.",
                "The value is semantically irrelevant to external tooling, runtime logs, audit requirements, or generated dialect behavior.",
                "A verifier property is violated.",
            ],
            confidence=0.64,
            loc=state.get("loc"),
        ))
    return findings


def _unwritten_state(tsg):
    nodes = node_by_id(tsg)
    written = set()
    read = set()
    for e in tsg["edges"]:
        dst_node = nodes.get(e["to"]) or {}
        if dst_node.get("kind") != "state":
            continue
        if e["kind"] == "writes":
            written.add(e["to"])
        elif e["kind"] in {"reads", "checks"}:
            read.add(e["to"])

    findings = []
    for state in sorted((n for n in tsg["nodes"] if n["kind"] == "state"), key=lambda n: n["id"]):
        if state["id"] in written:
            continue
        findings.append(_finding(
            "unwritten_state",
            [state["id"]],
            {
                "kind": "state_has_no_action_writes",
                "node": state["id"],
                "read_by": sorted(read_for_state(tsg, state["id"])),
            },
            "The state variable is initialized but no action writes it in the structural graph.",
            [
                {
                    "kind": "review_state_role",
                    "template": "Make the value a const/model parameter if it is intentionally fixed, or add the missing action/effect that changes it.",
                }
            ],
            [
                "The state variable is useless.",
                "A verifier property is violated.",
                "The variable is safe to delete without checking generated dialect state.",
            ],
            confidence=0.76 if state["id"] in read else 0.68,
            loc=state.get("loc"),
        ))
    return findings


def _conservation_candidate_findings(spec):
    findings = []
    for candidate in conservation_candidates(spec):
        actions = [item["action"] for item in candidate["actions"]]
        findings.append(_finding(
            "conservation_candidate",
            sorted(set(candidate["states"] + actions)),
            {
                "kind": "weighted_sum_conservation_candidate",
                "expression": candidate["expression"],
                "weights": candidate["weights"],
                "action_net_effects": candidate["actions"],
                "excluded_counters": candidate["excluded_counters"],
            },
            "Counter-like effects structurally preserve this weighted sum, which may indicate an implicit invariant worth declaring and proving.",
            [
                {
                    "kind": "add_invariant_then_verify",
                    "template": (
                        f"Declare `invariant Conservation {{ {candidate['expression']} == <initial value> }}` "
                        "and run `fslc verify` plus `--engine induction` to prove it."
                    ),
                }
            ],
            [
                "The weighted sum is actually invariant.",
                "The absence of a candidate means no conservation law exists.",
                "This finding is a proof; it is only structural evidence and must be checked by verify.",
            ],
            confidence=0.6,
        ))
    return findings


def _unguarded_actions(tsg):
    has_guard = set()
    nodes = node_by_id(tsg)
    for e in tsg["edges"]:
        if e["kind"] == "has_guard":
            has_guard.add(e["from"])

    findings = []
    for action in sorted((n for n in tsg["nodes"] if n["kind"] == "action"), key=lambda n: n["id"]):
        if action.get("generated"):
            continue
        if action["id"] in has_guard:
            continue
        writes = sorted(
            e["to"]
            for e in tsg["edges"]
            if e["from"] == action["id"]
            and e["kind"] == "writes"
            and (nodes.get(e["to"]) or {}).get("kind") == "state"
        )
        findings.append(_finding(
            "unguarded_action",
            [action["id"]],
            {
                "kind": "action_has_no_requires",
                "node": action["id"],
                "writes": writes,
            },
            "The action has no explicit requires clauses, so it is structurally enabled in every state unless generated lowering adds hidden constraints elsewhere.",
            [
                {
                    "kind": "add_or_confirm_guard",
                    "template": "Add a requires clause if the action should be state-dependent, or tag/document why it is intentionally always enabled.",
                }
            ],
            [
                "The action is wrong.",
                "Always-enabled behavior is invalid.",
                "The action is reachable in every semantic state without considering type bounds and invariants.",
            ],
            confidence=0.72,
            loc=action.get("loc"),
        ))
    return findings


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


_RELEVANCE_SEED_KINDS = PROPERTY_NODE_KINDS | SCENARIO_NODE_KINDS | {"guard", "ensures"}


def _transitively_relevant_state(tsg):
    nodes = node_by_id(tsg)
    relevant = set()
    effect_reads = {}
    effect_targets = {}
    for n in tsg["nodes"]:
        if n["kind"] == "effect" and n.get("target"):
            effect_targets[n["id"]] = f"state:{n['target']}"
    for e in tsg["edges"]:
        dst_node = nodes.get(e["to"]) or {}
        if dst_node.get("kind") != "state":
            continue
        src_node = nodes.get(e["from"]) or {}
        if e["kind"] in {"reads", "checks"} and src_node.get("kind") in _RELEVANCE_SEED_KINDS:
            relevant.add(e["to"])
        if e["kind"] == "reads" and src_node.get("kind") == "effect":
            effect_reads.setdefault(e["from"], set()).add(e["to"])

    changed = True
    while changed:
        changed = False
        for effect_id in sorted(effect_targets):
            target = effect_targets[effect_id]
            if target not in relevant:
                continue
            for read_state in sorted(effect_reads.get(effect_id, set())):
                if read_state not in relevant:
                    relevant.add(read_state)
                    changed = True
    return relevant


def _state_writers(tsg):
    nodes = node_by_id(tsg)
    writers = {}
    for e in tsg["edges"]:
        if e["kind"] != "writes":
            continue
        dst_node = nodes.get(e["to"]) or {}
        if dst_node.get("kind") != "state":
            continue
        src_node = nodes.get(e["from"]) or {}
        writer = e["from"] if src_node.get("kind") == "action" else src_node.get("action")
        if writer:
            writers.setdefault(e["to"], set()).add(writer)
    return writers


def _scenario_actions(tsg):
    out = set()
    scenario_ids = {n["id"] for n in tsg["nodes"] if n["kind"] in SCENARIO_NODE_KINDS}
    for e in tsg["edges"]:
        if e["from"] in scenario_ids and e["to"].startswith("action:"):
            out.add(e["to"])
    return out


def read_for_state(tsg, state_id):
    readers = []
    for e in tsg["edges"]:
        if e["to"] == state_id and e["kind"] in {"reads", "checks"}:
            readers.append(e["from"])
    return readers


def _finding(finding_type, involved_nodes, witness, why, repairs, caveats, confidence, loc=None):
    out = {
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
    if loc:
        out["loc"] = loc
    return out


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
