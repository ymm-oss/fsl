# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Bounded semantic witnesses for underspecification review findings."""
from __future__ import annotations

import itertools

import z3

from ..bmc import (
    _build_trace,
    _display_instance,
    _eval_cache_scope,
    _logical_eq_var,
    build_instances,
    eval_expr,
    init_constraints,
    logical_state_values,
    make_state,
    transition,
)
from ..model import FslError


SEMANTIC_ANALYSIS_DEPTH = 4
MAX_INSTANCE_PAIR_QUERIES = 256


def analyze_underspecification(spec, unconstrained_state_names, depth=SEMANTIC_ANALYSIS_DEPTH):
    """Find reachable same-state action choices with observably different successors."""
    try:
        return _analyze(spec, set(unconstrained_state_names), depth)
    except (FslError, KeyError, z3.Z3Exception):
        # AI-review findings are additive review evidence. A semantic probe must
        # never turn structural analysis into an internal error.
        return {"divergent_choices": [], "unconstrained_effects": []}


def _contract_predicates(spec):
    predicates = []
    for inv in spec.get("user_invariants", []) or []:
        predicates.append({
            "kind": "invariant",
            "name": inv["name"],
            "node": f"invariant:{inv['name']}",
            "expr": inv["expr"],
        })
    for acceptance in spec.get("acceptance", []) or []:
        if acceptance.get("expect") is None:
            continue
        predicates.append({
            "kind": "acceptance",
            "name": acceptance["id"],
            "node": f"acceptance:{acceptance['id']}",
            "expr": acceptance["expect"],
        })
    return predicates


def _action_groups(spec):
    groups = {}
    for inst in build_instances(spec):
        if inst["action_def"].get("generated"):
            continue
        groups.setdefault(inst["action"], []).append(inst)
    return {name: groups[name] for name in sorted(groups)}


def _state_invariants(spec, state, cache):
    with _eval_cache_scope(cache, id(state)):
        return [eval_expr(inv["expr"], state, {}, spec) for inv in spec.get("invariants", [])]


def _predicate_terms(predicates, left, right, spec, cache):
    terms = []
    for predicate in predicates:
        with _eval_cache_scope(cache, id(left)):
            left_value = eval_expr(predicate["expr"], left, {}, spec)
        with _eval_cache_scope(cache, id(right)):
            right_value = eval_expr(predicate["expr"], right, {}, spec)
        if z3.is_bool(left_value) and z3.is_bool(right_value):
            terms.append((predicate, left_value, right_value))
    return terms


def _bool_value(model, value):
    return z3.is_true(model.eval(value, model_completion=True))


def _branch_record(model, path_states, path_choices, instances, spec, step,
                   left_inst, right_inst, left_state, right_state, depth):
    left_logical = logical_state_values(model, left_state, spec)
    right_logical = logical_state_values(model, right_state, spec)
    divergent = sorted(
        name for name in spec["state"]
        if left_logical.get(name) != right_logical.get(name)
    )
    trace = _build_trace(model, path_states, path_choices, instances, spec, step)
    return {
        "bounded_evidence": {
            "available": True,
            "depth": depth,
            "reachable_at_step": step,
        },
        "trace": trace,
        "state": trace[-1]["state"],
        "actions": [
            {
                **_display_instance(left_inst, spec),
                "successor": left_logical,
            },
            {
                **_display_instance(right_inst, spec),
                "successor": right_logical,
            },
        ],
        "action_nodes": [
            f"action:{left_inst['action']}",
            f"action:{right_inst['action']}",
        ],
        "divergent_state": divergent,
    }


def _find_property_divergence(solver, common, predicate_terms):
    if not predicate_terms:
        return None
    solver.push()
    solver.add(*common)
    solver.add(z3.Or(*[z3.Xor(left, right) for _item, left, right in predicate_terms]))
    if solver.check() != z3.sat:
        solver.pop()
        return None
    model = solver.model()
    differing = [
        {"kind": item["kind"], "name": item["name"], "node": item["node"]}
        for item, left, right in predicate_terms
        if _bool_value(model, left) != _bool_value(model, right)
    ]
    solver.pop()
    return model, differing


def _find_state_divergence(solver, common, state_name, state_type, left, right, spec):
    solver.push()
    solver.add(*common)
    solver.add(z3.Not(_logical_eq_var(spec, left, right, state_name, state_type)))
    if solver.check() != z3.sat:
        solver.pop()
        return None
    model = solver.model()
    solver.pop()
    return model


def _analyze(spec, unconstrained_state_names, depth):
    predicates = _contract_predicates(spec)
    action_groups = _action_groups(spec)
    instances = build_instances(spec)
    if len(action_groups) < 2 or not instances:
        return {"divergent_choices": [], "unconstrained_effects": []}

    cache = {}
    states = [make_state(spec, "underspec_path_0")]
    choices = []
    solver = z3.Solver()
    with _eval_cache_scope(cache, id(states[0])):
        solver.add(*init_constraints(spec, states[0]))
    solver.add(*_state_invariants(spec, states[0], cache))

    divergent = []
    unconstrained = []
    seen_action_pairs = set()
    seen_unconstrained = set()
    pair_queries = 0

    for step in range(depth + 1):
        if solver.check() != z3.sat:
            break
        for left_name, right_name in itertools.combinations(action_groups, 2):
            pair_key = (left_name, right_name)
            for left_inst in action_groups[left_name]:
                for right_inst in action_groups[right_name]:
                    if pair_queries >= MAX_INSTANCE_PAIR_QUERIES:
                        break
                    pair_queries += 1
                    left_state = make_state(
                        spec, f"underspec_{step}_{pair_queries}_left")
                    right_state = make_state(
                        spec, f"underspec_{step}_{pair_queries}_right")
                    left_index = instances.index(left_inst)
                    right_index = instances.index(right_inst)
                    common = [
                        transition(
                            spec, instances, states[step], left_state,
                            z3.IntVal(left_index), cache),
                        transition(
                            spec, instances, states[step], right_state,
                            z3.IntVal(right_index), cache),
                    ]

                    if pair_key not in seen_action_pairs:
                        predicate_terms = _predicate_terms(
                            predicates, left_state, right_state, spec, cache)
                        property_result = _find_property_divergence(
                            solver, common, predicate_terms)
                        if property_result is not None:
                            model, differing = property_result
                            record = _branch_record(
                                model, states, choices, instances, spec, step,
                                left_inst, right_inst, left_state, right_state, depth)
                            record["kind"] = "reachable_divergent_choice"
                            record["differing_predicates"] = [
                                {"kind": item["kind"], "name": item["name"]}
                                for item in differing
                            ]
                            record["predicate_nodes"] = [item["node"] for item in differing]
                            divergent.append(record)
                            seen_action_pairs.add(pair_key)

                    for state_name in sorted(unconstrained_state_names - seen_unconstrained):
                        state_type = spec["state"].get(state_name)
                        if state_type is None:
                            continue
                        model = _find_state_divergence(
                            solver, common, state_name, state_type,
                            left_state, right_state, spec)
                        if model is None:
                            continue
                        record = _branch_record(
                            model, states, choices, instances, spec, step,
                            left_inst, right_inst, left_state, right_state, depth)
                        record["kind"] = "reachable_unconstrained_effect"
                        record["state_name"] = state_name
                        record["divergent_state"] = [state_name]
                        unconstrained.append(record)
                        seen_unconstrained.add(state_name)
                    if (
                        pair_key in seen_action_pairs
                        and unconstrained_state_names <= seen_unconstrained
                    ):
                        break
                if pair_queries >= MAX_INSTANCE_PAIR_QUERIES:
                    break

        if step >= depth:
            break
        nxt = make_state(spec, f"underspec_path_{step + 1}")
        choice = z3.Int(f"__underspec_choice@{step}")
        solver.add(choice >= 0, choice < len(instances))
        solver.add(transition(spec, instances, states[step], nxt, choice, cache))
        solver.add(*_state_invariants(spec, nxt, cache))
        states.append(nxt)
        choices.append(choice)

    return {
        "divergent_choices": divergent,
        "unconstrained_effects": unconstrained,
    }
