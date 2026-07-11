# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Bounded semantic comparison of two FSL specifications."""
from __future__ import annotations

from pathlib import Path

import z3
from lark.exceptions import UnexpectedInput

from .acceptance import replay_forbidden
from .bmc import (
    _eval_cache_scope,
    _implicit_inv_constraints,
    eval_expr,
    logical_state_values,
    make_state,
)
from .grammar import Ast, PARSER
from .model import FslError, build_spec
from .parser import parse_refinement, parse_src
from .refine import build_refinement, refine


FINDING_KINDS = (
    "behavior_added",
    "behavior_removed",
    "invariant_weakened",
    "invariant_strengthened",
    "forbidden_relaxed",
    "scope_changed",
    "unknown",
)


def _declared_scope(source):
    """Return source-level verify bounds before dialect desugaring removes them."""
    try:
        tree = PARSER.parse(source)
        ast = Ast().transform(tree)
    except (UnexpectedInput, FslError):
        return {"instances": {}, "values": {}}
    if not isinstance(ast, tuple) or len(ast) < 3:
        return {"instances": {}, "values": {}}
    instances = {}
    values = {}
    for item in ast[2]:
        if item[0] != "verify_bounds":
            continue
        for bound in item[1]:
            if bound[0] == "verify_instances":
                instances[bound[1]] = bound[2]
            elif bound[0] == "verify_values":
                values[bound[1]] = (bound[2], bound[3])
    return {"instances": instances, "values": values}


def _public_scope(scope):
    return {
        "instances": dict(sorted(scope["instances"].items())),
        "values": {
            name: [bounds[0], bounds[1]]
            for name, bounds in sorted(scope["values"].items())
        },
    }


def _load_spec(path, source, bounds_overrides=None):
    ast, display_names = parse_src(
        source,
        str(Path(path).parent),
        bounds_overrides=bounds_overrides,
    )
    return build_spec(ast, display_names)


def _auto_mapping(impl_spec, abs_spec):
    tree = (
        "refinement",
        f"{impl_spec['name']}SemanticDiff{abs_spec['name']}",
        [
            ("impl", impl_spec["name"]),
            ("abs", abs_spec["name"]),
            ("maps_auto", None),
        ],
    )
    return build_refinement(tree, impl_spec, abs_spec)


def _identity_shape_mismatch(impl_spec, abs_spec):
    impl_state = set(impl_spec["state"])
    abs_state = set(abs_spec["state"])
    impl_actions = {action["name"] for action in impl_spec["actions"]}
    abs_actions = {action["name"] for action in abs_spec["actions"]}
    if impl_state == abs_state and impl_actions == abs_actions:
        return None
    return {
        "state": {
            "only_impl": sorted(impl_state - abs_state),
            "only_abs": sorted(abs_state - impl_state),
        },
        "actions": {
            "only_impl": sorted(impl_actions - abs_actions),
            "only_abs": sorted(abs_actions - impl_actions),
        },
    }


def _direction(impl_spec, abs_spec, depth, explicit_ast=None):
    automatic = explicit_ast is None
    if explicit_ast is None:
        mismatch = _identity_shape_mismatch(impl_spec, abs_spec)
        if mismatch:
            return {
                "result": "unknown",
                "reason": "state_or_action_names_differ",
                "mismatch": mismatch,
            }, None
        try:
            mapping = _auto_mapping(impl_spec, abs_spec)
        except FslError as exc:
            return {
                "result": "unknown",
                "reason": "automatic_mapping_failed",
                "message": str(exc),
            }, None
    else:
        mapping = build_refinement(explicit_ast, impl_spec, abs_spec)

    try:
        result = refine(impl_spec, abs_spec, mapping, depth)
    except FslError as exc:
        if not automatic:
            raise
        return {
            "result": "unknown",
            "reason": "automatic_mapping_failed",
            "message": str(exc),
        }, None
    public = {
        "result": result.get("result"),
        "checked_to_depth": depth,
    }
    if result.get("result") == "refinement_failed":
        public["kind"] = result.get("kind")
        public["violated_at_step"] = result.get("violated_at_step")
    elif result.get("result") != "refines":
        public = {
            "result": "unknown",
            "reason": "refinement_input_not_verifiable",
            "detail": result,
        }
    return public, result


def _counterexample_witness(result):
    return {
        "trace_type": "counterexample",
        "trace": result.get("impl_trace", []),
        "violation": {
            key: result[key]
            for key in ("kind", "violated_at_step", "impl_action", "mismatch")
            if key in result
        },
    }


def _same_state_schema(old_spec, new_spec):
    if old_spec["state"] != new_spec["state"]:
        return False
    return {p["phys"] for p in old_spec["phys_vars"]} == {
        p["phys"] for p in new_spec["phys_vars"]
    }


def _invariant_implication(antecedent_spec, consequent_spec, state_spec, label):
    state = make_state(state_spec, f"semantic_diff_{label}")
    solver = z3.Solver()
    cache = {}
    solver.add(*_implicit_inv_constraints(state_spec, state, cache))
    with _eval_cache_scope(cache, id(state)):
        antecedent = [
            eval_expr(inv["expr"], state, {}, antecedent_spec)
            for inv in antecedent_spec.get("user_invariants", [])
        ]
        consequent = [
            eval_expr(inv["expr"], state, {}, consequent_spec)
            for inv in consequent_spec.get("user_invariants", [])
        ]
    solver.add(z3.And(*antecedent) if antecedent else z3.BoolVal(True))
    solver.add(z3.Not(z3.And(*consequent) if consequent else z3.BoolVal(True)))
    status = solver.check()
    if status == z3.unsat:
        return {"implies": True}
    if status == z3.sat:
        model = solver.model()
        return {
            "implies": False,
            "state": logical_state_values(model, state, state_spec),
        }
    return {"implies": None, "reason": solver.reason_unknown()}


def _compare_invariants(old_spec, new_spec):
    if not _same_state_schema(old_spec, new_spec):
        return None, None
    try:
        old_to_new = _invariant_implication(old_spec, new_spec, new_spec, "old_new")
        new_to_old = _invariant_implication(new_spec, old_spec, new_spec, "new_old")
    except (FslError, KeyError, z3.Z3Exception) as exc:
        return None, {"reason": "invariant_implication_failed", "message": str(exc)}
    return (old_to_new, new_to_old), None


def _forbidden_findings(old_spec, new_spec):
    findings = []
    unknown = []
    for forbidden in old_spec.get("forbidden") or []:
        try:
            replay = replay_forbidden(new_spec, forbidden)
        except FslError as exc:
            unknown.append({
                "kind": "unknown",
                "subject": "forbidden",
                "id": forbidden["id"],
                "reason": str(exc),
            })
            continue
        if not replay.get("ok") and replay.get("kind") == "forbidden":
            findings.append({
                "kind": "forbidden_relaxed",
                "id": forbidden["id"],
                "witness": {
                    "trace_type": "counterexample",
                    "trace": replay.get("accepted_trace", []),
                    "accepted_step": replay.get("accepted_step"),
                    "state": replay.get("state"),
                },
            })
    return findings + unknown


def semantic_diff(old_path, new_path, depth=8, mapping_path=None, forbid=None):
    """Compare OLD and NEW under NEW's declared verify scope."""
    old_source = Path(old_path).read_text(encoding="utf-8")
    new_source = Path(new_path).read_text(encoding="utf-8")
    old_scope = _declared_scope(old_source)
    new_scope = _declared_scope(new_source)
    scope_changed = old_scope != new_scope

    overrides = {
        "instances": {
            name: value
            for name, value in new_scope["instances"].items()
            if name in old_scope["instances"]
        },
        "values": {
            name: value
            for name, value in new_scope["values"].items()
            if name in old_scope["values"]
        },
    }
    old_spec = _load_spec(
        old_path,
        old_source,
        bounds_overrides=overrides if scope_changed else None,
    )
    new_spec = _load_spec(new_path, new_source)

    explicit_ast = None
    explicit_direction = None
    if mapping_path:
        explicit_ast = parse_refinement(Path(mapping_path).read_text(encoding="utf-8"))
        items = explicit_ast[2]
        impl = next((item[1] for item in items if item[0] == "impl"), None)
        abs_name = next((item[1] for item in items if item[0] == "abs"), None)
        if (impl, abs_name) == (new_spec["name"], old_spec["name"]):
            explicit_direction = "new_to_old"
        elif (impl, abs_name) == (old_spec["name"], new_spec["name"]):
            explicit_direction = "old_to_new"
        else:
            raise FslError(
                "diff mapping must map NEW to OLD or OLD to NEW",
                kind="type",
            )

    new_public, new_raw = _direction(
        new_spec,
        old_spec,
        depth,
        explicit_ast if explicit_direction == "new_to_old" else None,
    )
    old_public, old_raw = _direction(
        old_spec,
        new_spec,
        depth,
        explicit_ast if explicit_direction == "old_to_new" else None,
    )

    findings = []
    if new_public["result"] == "refinement_failed":
        findings.append({
            "kind": "behavior_added",
            "direction": "new_to_old",
            "witness": _counterexample_witness(new_raw),
        })
    elif new_public["result"] == "unknown":
        findings.append({
            "kind": "unknown",
            "direction": "new_to_old",
            "reason": new_public.get("reason"),
            "detail": (
                new_public.get("mismatch")
                or new_public.get("detail")
                or new_public.get("message")
            ),
        })
    if old_public["result"] == "refinement_failed":
        findings.append({
            "kind": "behavior_removed",
            "direction": "old_to_new",
            "witness": _counterexample_witness(old_raw),
        })
    elif old_public["result"] == "unknown":
        findings.append({
            "kind": "unknown",
            "direction": "old_to_new",
            "reason": old_public.get("reason"),
            "detail": (
                old_public.get("mismatch")
                or old_public.get("detail")
                or old_public.get("message")
            ),
        })

    implication, implication_error = _compare_invariants(old_spec, new_spec)
    if implication is not None:
        old_to_new, new_to_old = implication
        if old_to_new["implies"] is True and new_to_old["implies"] is False:
            findings.append({
                "kind": "invariant_weakened",
                "witness": {
                    "trace_type": "state_counterexample",
                    "state": new_to_old["state"],
                },
            })
        elif new_to_old["implies"] is True and old_to_new["implies"] is False:
            findings.append({
                "kind": "invariant_strengthened",
                "witness": {
                    "trace_type": "state_counterexample",
                    "state": old_to_new["state"],
                },
            })
        elif old_to_new["implies"] is not True or new_to_old["implies"] is not True:
            findings.append({
                "kind": "unknown",
                "subject": "invariants",
                "reason": "invariant_sets_are_incomparable",
                "old_to_new": old_to_new,
                "new_to_old": new_to_old,
            })
    elif implication_error is not None:
        findings.append({"kind": "unknown", "subject": "invariants", **implication_error})

    findings.extend(_forbidden_findings(old_spec, new_spec))
    if scope_changed:
        findings.append({
            "kind": "scope_changed",
            "old": _public_scope(old_scope),
            "new": _public_scope(new_scope),
            "comparison": "new",
        })

    present = {finding["kind"] for finding in findings}
    summary = [kind for kind in FINDING_KINDS if kind in present]
    if not summary:
        summary = ["no_semantic_change"]
    forbidden = sorted(set(forbid or []))
    unknown_forbid = sorted(set(forbidden) - set(FINDING_KINDS))
    if unknown_forbid:
        raise FslError(
            f"unknown --forbid finding kind(s): {', '.join(unknown_forbid)}",
            kind="semantics",
        )
    violations = [kind for kind in forbidden if kind in present]

    return {
        "result": "semantic_diff",
        "old": {"file": str(old_path), "spec": old_spec["name"]},
        "new": {"file": str(new_path), "spec": new_spec["name"]},
        "bounded": {"depth": depth, "completeness": "bounded"},
        "scope": {
            "old": _public_scope(old_scope),
            "new": _public_scope(new_scope),
            "comparison": "new",
            "applied_to_old": _public_scope(overrides),
        },
        "directions": {
            "new_to_old": new_public,
            "old_to_new": old_public,
        },
        "summary": summary,
        "findings": findings,
        "gate": {
            "forbidden": forbidden,
            "violations": violations,
            "passed": not violations,
        },
    }
