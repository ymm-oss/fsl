"""Acceptance scenario replay for the requirements dialect."""
from __future__ import annotations

from .model import FslError, resolve_action_name
from .runtime import Monitor, _as_bool, eval_concrete


def _err(message, loc=None):
    raise FslError(message, kind="acceptance", loc=loc)


def _literal_value(expr):
    tag = expr[0]
    if tag == "num":
        return expr[1]
    if tag == "bool":
        return expr[1]
    if tag == "none":
        return None
    if tag == "var":
        return expr[1]
    _err("acceptance action arguments must be literals")


def _action_def(spec, name):
    internal = resolve_action_name(name, spec)
    for act in spec["actions"]:
        if act["name"] == internal:
            return act
    return None


def _params_for(spec, action_name, args, loc):
    act = _action_def(spec, action_name)
    if act is None:
        _err(f"unknown action '{action_name}' in acceptance", loc=loc)
    if len(args) != len(act["params"]):
        _err(f"arity mismatch for action '{action_name}' in acceptance", loc=loc)
    return {
        p[0]: _literal_value(args[i])
        for i, p in enumerate(act["params"])
    }


def replay_acceptance(spec, ac):
    mon = Monitor(spec)
    initial = mon.reset()
    steps_out = []
    expected_states = []
    aliases = spec.get("action_aliases") or {}

    for idx, step in enumerate(ac["steps"]):
        _, name, args, loc = step
        candidates = aliases.get(name, [name])
        failures = []
        for candidate in candidates:
            params = _params_for(spec, candidate, args, loc)
            result = mon.step(candidate, params)
            if result.get("ok"):
                steps_out.append({"action": result["action"], "params": params})
                expected_states.append(result["state"])
                break
            failures.append(result)
        else:
            return {
                "ok": False,
                "kind": "acceptance",
                "id": ac["id"],
                "text": ac["text"],
                "failed_step": idx,
                "step": {"action": name, "args": [_literal_value(a) for a in args]},
                "step_results": failures,
                "loc": loc,
            }

    expect_ok = _as_bool(eval_concrete(ac["expect"], mon._phys, {}, spec))
    if not expect_ok:
        return {
            "ok": False,
            "kind": "acceptance",
            "id": ac["id"],
            "text": ac["text"],
            "failed_step": len(ac["steps"]),
            "expect": ac["expect"],
            "state": mon.state,
            "loc": ac.get("loc"),
        }

    return {
        "ok": True,
        "scenario": {
            "name": f"acceptance_{ac['id']}",
            "kind": "acceptance",
            "acceptance": ac["id"],
            "requirement": {"id": ac["id"], "text": ac["text"]},
            "steps": steps_out,
            "initial_state": initial,
            "expected_states": expected_states,
        },
    }


def validate_acceptance(spec):
    scenarios = []
    for ac in spec.get("acceptance") or []:
        result = replay_acceptance(spec, ac)
        if not result.get("ok"):
            return result
        scenarios.append(result["scenario"])
    return {"ok": True, "scenarios": scenarios}


def replay_forbidden(spec, fb):
    """Replay a must-forbid scenario: the setup steps must all be accepted, and
    the final step must be rejected (not enabled, or executing it violates an
    invariant / type_bound / partial_op / ensures). Mirror of acceptance, but
    the last step's outcome must be a rejection rather than a state predicate."""
    steps = fb.get("steps") or []
    if not steps:
        raise FslError(
            f"forbidden '{fb['id']}' must have at least one step",
            kind="forbidden", loc=fb.get("loc"),
        )

    mon = Monitor(spec)
    initial = mon.reset()
    aliases = spec.get("action_aliases") or {}
    steps_out = []
    expected_states = []

    # Setup: every step before the last must be accepted (ok).
    for idx in range(len(steps) - 1):
        _, name, args, loc = steps[idx]
        candidates = aliases.get(name, [name])
        failures = []
        for candidate in candidates:
            params = _params_for(spec, candidate, args, loc)
            result = mon.step(candidate, params)
            if result.get("ok"):
                steps_out.append({"action": result["action"], "params": params})
                expected_states.append(result["state"])
                break
            failures.append(result)
        else:
            return {
                "ok": False,
                "kind": "forbidden_setup",
                "id": fb["id"],
                "text": fb["text"],
                "failed_step": idx,
                "step": {"action": name, "args": [_literal_value(a) for a in args]},
                "step_results": failures,
                "loc": loc,
                "hint": "forbidden の前提ステップは enabled で ok でなければならない(トレースが壊れている)。",
            }

    # Final step: must be rejected. If any candidate is accepted, the spec
    # failed to forbid the operation.
    _, name, args, loc = steps[-1]
    candidates = aliases.get(name, [name])
    attempts = []
    for candidate in candidates:
        params = _params_for(spec, candidate, args, loc)
        result = mon.step(candidate, params)
        attempts.append((params, result))
        if result.get("ok"):
            return {
                "ok": False,
                "kind": "forbidden",
                "id": fb["id"],
                "text": fb["text"],
                "accepted_step": len(steps) - 1,
                "step": {"action": name, "args": [_literal_value(a) for a in args]},
                "accepted_trace": steps_out + [{"action": result["action"], "params": params}],
                "state": result["state"],
                "loc": fb.get("loc"),
                "hint": "この操作は拒否されるべきだが受理された。ガードか invariant が不足している可能性。",
            }

    last_params, last_result = attempts[-1]
    return {
        "ok": True,
        "scenario": {
            "name": f"forbidden_{fb['id']}",
            "kind": "forbidden",
            "forbidden": fb["id"],
            "requirement": {"id": fb["id"], "text": fb["text"]},
            "steps": steps_out,
            "initial_state": initial,
            "expected_states": expected_states,
            "forbidden_step": {"action": last_result.get("action", name), "params": last_params},
            "rejected_by": last_result.get("kind"),
        },
    }


def validate_forbidden(spec):
    scenarios = []
    for fb in spec.get("forbidden") or []:
        result = replay_forbidden(spec, fb)
        if not result.get("ok"):
            return result
        scenarios.append(result["scenario"])
    return {"ok": True, "scenarios": scenarios}
