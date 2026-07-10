# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Replay production JSONL records through refinement mapping syntax."""

from __future__ import annotations

import json
from pathlib import Path

from .model import FslError, domain_range
from .runtime import Monitor, eval_concrete


_FINITE_LOG_NOTE = "leadsTo properties are not checked by replay (finite logs only)"


def _err(message, kind="semantics", loc=None, hint=None):
    raise FslError(message, kind=kind, loc=loc, hint=hint)


def _find_action(spec, name):
    return next((action for action in spec["actions"] if action["name"] == name), None)


def build_log_mapping(tree, target_spec):
    """Validate a refinement AST for JSONL-to-spec replay.

    The mapping file uses the exact refinement parser and item shapes.  Its
    ``impl`` name labels the external log schema rather than referring to a
    second FSL spec; the ``abs`` side must name the replay target spec.
    """

    if not isinstance(tree, tuple) or tree[0] != "refinement":
        _err("expected refinement mapping file", kind="type")

    _, name, items = tree
    impl_name = None
    abs_name = None
    maps_auto = False
    maps = {}
    actions = {}

    for item in items:
        tag = item[0]
        if tag == "impl":
            impl_name = item[1]
        elif tag == "abs":
            abs_name = item[1]
        elif tag == "maps_auto":
            maps_auto = True
        elif tag == "map":
            _, logical, binder, expr, loc = item
            if logical in maps:
                _err(f"duplicate map for '{logical}'", kind="type", loc=loc)
            if logical not in target_spec["state"]:
                _err(f"unknown abstract state variable '{logical}'", kind="type", loc=loc)
            maps[logical] = {
                "kind": "indexed" if binder is not None else "scalar",
                "binder": binder,
                "expr": expr,
                "loc": loc,
            }
        elif tag == "action_map":
            _, source_name, params, target, loc = item
            if source_name in actions:
                _err(f"duplicate action map for '{source_name}'", kind="type", loc=loc)
            param_names = [param[1] for param in params]
            if len(param_names) != len(set(param_names)):
                _err(f"duplicate parameter in action map '{source_name}'", kind="type", loc=loc)
            if target[0] == "stutter":
                actions[source_name] = {
                    "kind": "stutter",
                    "params": param_names,
                    "loc": loc,
                }
                continue
            _, target_name, arg_exprs = target
            target_action = _find_action(target_spec, target_name)
            if target_action is None:
                _err(f"unknown abstract action '{target_name}'", kind="type", loc=loc)
            if len(arg_exprs) != len(target_action["params"]):
                _err(
                    f"action '{source_name}' -> '{target_name}' expects "
                    f"{len(target_action['params'])} arguments",
                    kind="type",
                    loc=loc,
                )
            actions[source_name] = {
                "kind": "map",
                "params": param_names,
                "abs_action": target_name,
                "arg_exprs": arg_exprs,
                "loc": loc,
            }
        elif tag == "preserve_progress":
            _err(
                "preserve progress is not available for finite production-log replay",
                kind="semantics",
                loc=item[-1],
                hint=_FINITE_LOG_NOTE,
            )

    if impl_name is None:
        _err("refinement missing impl spec name", kind="type")
    if abs_name is None:
        _err("refinement missing abs spec name", kind="type")
    if abs_name != target_spec["name"]:
        _err(
            f"abs name '{abs_name}' does not match replay spec '{target_spec['name']}'",
            kind="type",
        )

    if maps_auto:
        for logical in target_spec["state"]:
            maps.setdefault(
                logical,
                {
                    "kind": "scalar",
                    "binder": None,
                    "expr": ("var", logical),
                    "loc": None,
                },
            )

    for logical in target_spec["state"]:
        if logical not in maps:
            _err(f"missing map for abstract state variable '{logical}'", kind="type")

    return {
        "name": name,
        "impl": impl_name,
        "abs": abs_name,
        "maps": maps,
        "actions": actions,
        "maps_auto": maps_auto,
        "progress": [],
    }


def load_jsonl(path):
    """Load non-empty JSONL records while preserving their physical line."""

    records = []
    for line_number, line in enumerate(Path(path).read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as exc:
            _err(f"invalid JSONL at line {line_number}: {exc.msg}", kind="io")
        if not isinstance(record, dict):
            _err(f"JSONL line {line_number} must be an object", kind="io")
        records.append((line_number, record))
    return records


def _enum_value(value, spec):
    if not isinstance(value, str):
        return value
    matches = []
    for info in spec["types"].values():
        if info.get("kind") == "enum" and value in info["members"]:
            matches.append(info["members"].index(value))
    return matches[0] if len(set(matches)) == 1 and matches else value


def _normalize_input(value, spec):
    if value is None:
        return ("none",)
    if isinstance(value, dict):
        out = {}
        for key, item in value.items():
            normalized = _normalize_input(item, spec)
            out[key] = normalized
            if isinstance(key, str):
                try:
                    out[int(key)] = normalized
                except ValueError:
                    pass
        return out
    if isinstance(value, list):
        return [_normalize_input(item, spec) for item in value]
    return _enum_value(value, spec)


def _eval_mapping_expr(expr, raw_state, binds, spec):
    evaluation_spec = {**spec, "state": {}}
    state = {name: _normalize_input(value, spec) for name, value in raw_state.items()}
    normalized_binds = {name: _normalize_input(value, spec) for name, value in binds.items()}
    try:
        return eval_concrete(expr, state, normalized_binds, evaluation_spec)
    except Exception as exc:
        message = getattr(exc, "message", None) or str(exc)
        _err(message or "mapping expression could not be evaluated", kind="semantics")


def _domain_values(ty, spec):
    if ty[0] == "bool":
        return [False, True]
    lo, hi = domain_range(ty, spec["types"])
    return list(range(lo, hi + 1))


def _display_scalar(value, ty, spec):
    if ty[0] == "bool":
        if not isinstance(value, bool):
            _err(f"expected Bool value, got {value!r}", kind="type")
        return value
    if ty[0] == "int":
        if not isinstance(value, int) or isinstance(value, bool):
            _err(f"expected Int value, got {value!r}", kind="type")
        return value
    if ty[0] == "domain":
        if not isinstance(value, int) or isinstance(value, bool):
            _err(f"expected bounded integer value, got {value!r}", kind="type")
        lo, hi = domain_range(ty, spec["types"])
        if value < lo or value > hi:
            _err(f"mapped value {value} is out of range [{lo}..{hi}]", kind="type")
        return value
    if ty[0] == "enum":
        members = spec["types"][ty[1]]["members"]
        if isinstance(value, str):
            if value not in members:
                _err(f"unknown {ty[1]} enum member '{value}'", kind="type")
            return value
        if not isinstance(value, int) or value < 0 or value >= len(members):
            _err(f"mapped enum ordinal {value!r} is out of range for {ty[1]}", kind="type")
        return members[value]
    _err(f"unsupported scalar mapping type {ty}", kind="type")


def _lookup_map_value(value, key, key_ty, spec):
    candidates = [key, str(key)]
    if key_ty[0] == "bool":
        candidates.extend(["true" if key else "false"])
    elif key_ty[0] == "enum":
        candidates.append(spec["types"][key_ty[1]]["members"][key])
    for candidate in candidates:
        if candidate in value:
            return value[candidate]
    _err(f"mapped state is missing key {candidates[-1]!r}", kind="semantics")


def _to_logical(value, ty, spec):
    if ty[0] in ("bool", "int", "domain", "enum"):
        return _display_scalar(value, ty, spec)
    if ty[0] == "option":
        if value is None or value == ("none",):
            return None
        if isinstance(value, tuple) and value and value[0] == "option_val":
            if not value[1]:
                return None
            value = value[2]
        return _to_logical(value, ty[1], spec)
    if ty[0] == "struct":
        if isinstance(value, tuple) and value and value[0] == "struct_val":
            value = value[2]
        if not isinstance(value, dict):
            _err(f"expected object for struct {ty[1]}", kind="type")
        fields = spec["types"][ty[1]]["fields"]
        missing = [field for field in fields if field not in value]
        if missing:
            _err(f"mapped struct {ty[1]} is missing field '{missing[0]}'", kind="semantics")
        return {field: _to_logical(value[field], fty, spec) for field, fty in fields.items()}
    if ty[0] == "map":
        if not isinstance(value, dict):
            _err("expected object for mapped Map value", kind="type")
        out = {}
        for key in _domain_values(ty[1], spec):
            display_key = (
                "true" if key is True else "false" if key is False
                else spec["types"][ty[1][1]]["members"][key] if ty[1][0] == "enum"
                else str(key)
            )
            out[display_key] = _to_logical(
                _lookup_map_value(value, key, ty[1], spec), ty[2], spec
            )
        return out
    if ty[0] == "seq":
        if isinstance(value, tuple) and value and value[0] == "seq_val":
            value = value[1][: value[2]]
        if not isinstance(value, list):
            _err("expected array for mapped Seq value", kind="type")
        if len(value) > ty[2]:
            _err(f"mapped Seq length {len(value)} exceeds capacity {ty[2]}", kind="type")
        return [_to_logical(item, ty[1], spec) for item in value]
    if ty[0] == "set":
        if isinstance(value, tuple) and value and value[0] == "set_val":
            value = [key for key, present in value[1].items() if present]
        if not isinstance(value, list):
            _err("expected array for mapped Set value", kind="type")
        return sorted((_to_logical(item, ty[1], spec) for item in value), key=str)
    if ty[0] == "relation":
        if not isinstance(value, list):
            _err("expected array of pairs for mapped relation value", kind="type")
        out = []
        for pair in value:
            if not isinstance(pair, list) or len(pair) != 2:
                _err("mapped relation entries must be two-element arrays", kind="type")
            out.append([
                _to_logical(pair[0], ty[1], spec),
                _to_logical(pair[1], ty[2], spec),
            ])
        return out
    _err(f"unsupported mapped state type {ty}", kind="type")


def _map_state(mapping, raw_state, spec):
    if not isinstance(raw_state, dict):
        _err("record.state must be an object", kind="type")
    out = {}
    for logical, ty in spec["state"].items():
        entry = mapping["maps"][logical]
        if entry["kind"] == "scalar":
            value = _eval_mapping_expr(entry["expr"], raw_state, {}, spec)
            out[logical] = _to_logical(value, ty, spec)
            continue
        if ty[0] != "map":
            _err(f"indexed map on non-Map variable '{logical}'", kind="type")
        binder = entry["binder"]
        values = {}
        for key in _domain_values(ty[1], spec):
            value = _eval_mapping_expr(entry["expr"], raw_state, {binder[1]: key}, spec)
            display_key = (
                "true" if key is True else "false" if key is False
                else spec["types"][ty[1][1]]["members"][key] if ty[1][0] == "enum"
                else str(key)
            )
            values[display_key] = _to_logical(value, ty[2], spec)
        out[logical] = values
    return out


def _map_action(mapping, record, spec):
    source_action = record.get("action")
    params = record.get("params", {})
    if not isinstance(source_action, str):
        _err("record.action must be a string", kind="type")
    if not isinstance(params, dict):
        _err("record.params must be an object", kind="type")
    action_map = mapping["actions"].get(source_action)
    if action_map is None and mapping.get("maps_auto"):
        target = _find_action(spec, source_action)
        if target is not None:
            expected = [param[0] for param in target["params"]]
            action_map = {
                "kind": "map",
                "params": expected,
                "abs_action": source_action,
                "arg_exprs": [("var", name) for name in expected],
            }
    if action_map is None:
        _err(f"no action mapping for log action '{source_action}'", kind="semantics")
    expected_params = action_map["params"]
    if set(params) != set(expected_params):
        _err(
            f"parameter mismatch for log action '{source_action}': "
            f"expected {expected_params}, got {list(params)}",
            kind="type",
        )
    if action_map["kind"] == "stutter":
        return source_action, None, {}
    raw_state = record.get("state", {})
    target_action = _find_action(spec, action_map["abs_action"])
    mapped_params = {}
    for target_param, expr in zip(target_action["params"], action_map["arg_exprs"]):
        mapped_params[target_param[0]] = _eval_mapping_expr(expr, raw_state, params, spec)
    return source_action, action_map["abs_action"], mapped_params


def _mismatches(expected, observed, path=""):
    if isinstance(expected, dict) and isinstance(observed, dict):
        out = []
        for key in sorted(set(expected) | set(observed), key=str):
            child = f"{path}.{key}" if path else str(key)
            if key not in expected:
                out.append({"path": child, "expected": None, "observed": observed[key]})
            elif key not in observed:
                out.append({"path": child, "expected": expected[key], "observed": None})
            else:
                out.extend(_mismatches(expected[key], observed[key], child))
        return out
    if isinstance(expected, list) and isinstance(observed, list):
        if expected == observed:
            return []
        return [{"path": path, "expected": expected, "observed": observed}]
    if expected != observed:
        return [{"path": path, "expected": expected, "observed": observed}]
    return []


def replay_mapped_log(spec, records, mapping):
    """Map and replay JSONL records, stopping at the first divergence."""

    monitor = Monitor(spec)
    monitor.reset()
    for record_index, (line_number, record) in enumerate(records):
        before = monitor.state
        try:
            source_action, target_action, mapped_params = _map_action(mapping, record, spec)
            if target_action is None:
                step_result = {"ok": True, "state": before}
            else:
                step_result = monitor.step(target_action, mapped_params)
            if not step_result.get("ok"):
                violation = {
                    **step_result,
                    "source_action": source_action,
                    "mapped_action": target_action,
                }
                return {
                    "result": "nonconformant",
                    "spec": spec["name"],
                    "mapping": mapping["name"],
                    "source": "jsonl_mapping",
                    "failed_at_event": record_index,
                    "failed_at_record": record_index,
                    "log_line": line_number,
                    "violation": violation,
                    "state_before": before,
                    "note": _FINITE_LOG_NOTE,
                }
            observed = _map_state(mapping, record.get("state"), spec)
        except FslError as exc:
            return {
                "result": "nonconformant",
                "spec": spec["name"],
                "mapping": mapping["name"],
                "source": "jsonl_mapping",
                "failed_at_event": record_index,
                "failed_at_record": record_index,
                "log_line": line_number,
                "violation": {
                    "kind": "log_mapping",
                    "message": str(exc),
                    "loc": getattr(exc, "loc", None),
                },
                "state_before": before,
                "note": _FINITE_LOG_NOTE,
            }

        expected = monitor.state
        mismatch = _mismatches(expected, observed)
        if mismatch:
            return {
                "result": "nonconformant",
                "spec": spec["name"],
                "mapping": mapping["name"],
                "source": "jsonl_mapping",
                "failed_at_event": record_index,
                "failed_at_record": record_index,
                "log_line": line_number,
                "violation": {
                    "kind": "state_mismatch",
                    "source_action": source_action,
                    "action": target_action or "stutter",
                    "expected_state": expected,
                    "observed_state": observed,
                    "mismatches": mismatch,
                },
                "state_before": before,
                "note": _FINITE_LOG_NOTE,
            }

    return {
        "result": "conformant",
        "spec": spec["name"],
        "mapping": mapping["name"],
        "source": "jsonl_mapping",
        "steps_checked": len(records),
        "final_state": monitor.state,
        "note": _FINITE_LOG_NOTE,
    }
