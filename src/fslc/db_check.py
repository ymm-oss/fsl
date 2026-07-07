# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""AI-readable fsl-db compatibility checks and finding translation."""
from __future__ import annotations

import json
from copy import deepcopy
from pathlib import Path
from typing import Dict, List, Optional

from .bmc import prove, verify
from .db_expand import (
    DB_DIALECT_VERSION,
    DB_FINDING_SCHEMA_VERSION,
    DEFAULT_RULES,
    expand_dbsystem,
)
from .db_ir import ColumnKey, DbMigration, DbMigrationOp, DbSystem, column_label
from .db_parser import parse_dbsystem
from .model import FslError, build_spec


DB_OBSERVATION_SCHEMA_VERSION = "fsl-db-observation.v0"


def load_dbsystem(path):
    return parse_dbsystem(Path(path).read_text(encoding="utf-8"))


def check_dbsystem(system, depth=8, engine="bmc", deadlock_mode="warn"):
    expansion = expand_dbsystem(system)
    static_findings = _static_findings(system, expansion.assumptions)
    if static_findings:
        return _db_result(
            "violated",
            system,
            expansion.assumptions,
            findings=static_findings,
            kernel=None,
        )

    spec = build_spec(expansion.ast, expansion.display_names)
    if engine == "induction":
        kernel = prove(spec, 1, depth, deadlock_mode=deadlock_mode)
    else:
        kernel = verify(spec, depth, deadlock_mode=deadlock_mode)

    translated = _translate_kernel_result(kernel, system, expansion)
    if translated:
        return _db_result(
            "violated",
            system,
            expansion.assumptions,
            findings=translated,
            kernel=kernel,
        )
    if kernel.get("result") in ("verified", "proved"):
        return _db_result(
            "verified_under_assumptions",
            system,
            expansion.assumptions,
            findings=[],
            kernel=kernel,
        )
    return _db_result(
        kernel.get("result", "error"),
        system,
        expansion.assumptions,
        findings=[],
        kernel=kernel,
    )


def observe_dbsystem(system, trace_path):
    expansion = expand_dbsystem(system)
    events = _load_observation_events(trace_path)
    assumptions = list(expansion.assumptions) + [{
        "id": "DB-ASSUME-OBSERVABILITY-COVERAGE",
        "text": (
            "runtime observation is evidence only; absence from logs is not a proof "
            "that a capability is unused or unsupported"
        ),
    }]
    findings = _observation_findings(system, events, assumptions)
    return {
        "result": "observed_mismatch" if findings else "observed_conformant",
        "dialect": DB_DIALECT_VERSION,
        "finding_schema_version": DB_FINDING_SCHEMA_VERSION,
        "observation_schema_version": DB_OBSERVATION_SCHEMA_VERSION,
        "dbsystem": system.name,
        "assumptions": assumptions,
        "findings": findings,
        "formal_result": "not_run",
        "note": "runtime observation is separate from fsl-db formal compatibility verification",
    }


def _db_result(result, system, assumptions, findings, kernel):
    out = {
        "result": result,
        "dialect": DB_DIALECT_VERSION,
        "finding_schema_version": DB_FINDING_SCHEMA_VERSION,
        "dbsystem": system.name,
        "assumptions": assumptions,
        "findings": findings,
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


def _initial_state(system):
    return {
        key: {
            "exists": column.present,
            "backfilled": column.backfilled,
            "not_null": column.not_null,
        }
        for key, column in system.column_map().items()
    }


def _active_rules(system):
    return set(system.check.rules or DEFAULT_RULES)


def _static_findings(system, assumptions):
    rules = _active_rules(system)
    states = {system.database.initial_schema: _initial_state(system)}
    sources = {system.database.initial_schema: None}
    findings = []

    current_schema = system.database.initial_schema
    current_state = deepcopy(states[current_schema])
    for migration in system.migrations:
        next_state = deepcopy(current_state)
        for op in migration.ops:
            before_op_state = deepcopy(next_state)
            _apply_op(next_state, op)
            findings.extend(_op_findings(
                system,
                migration,
                op,
                before_op_state,
                next_state,
                assumptions,
                rules,
            ))
        states[migration.to_schema] = deepcopy(next_state)
        sources[migration.to_schema] = migration
        current_state = next_state
        current_schema = migration.to_schema

    for env in system.environments:
        for entry in env.artifacts:
            artifact = system.artifact_map()[entry.artifact]
            window = entry.schema_window or env.schema_window
            lo = max(env.schema_window[0], window[0])
            hi = min(env.schema_window[1], window[1])
            for schema_version in range(lo, hi + 1):
                state = states[schema_version]
                source = sources.get(schema_version)
                for column in artifact.reads:
                    if not state[column]["exists"]:
                        findings.append(_finding(
                            kind="column_removed_while_still_read",
                            severity="error",
                            environment=env.name,
                            migration=source.name if source else None,
                            schema_element=column_label(column),
                            artifact=entry.artifact,
                            artifact_version=entry.artifact,
                            failed_rule="all_active_reads_exist",
                            witness={
                                "schema_version": schema_version,
                                "environment_role": entry.role,
                                "declared_capability": "reads",
                            },
                            minimal_conflict_set={
                                "environment": env.name,
                                "artifact": entry.artifact,
                                "migration": source.name if source else None,
                                "schema_element": column_label(column),
                            },
                            repair_candidates=_compat_repairs("reads", entry.artifact, column, env.name),
                            assumptions=assumptions,
                        ))
                for column in artifact.writes:
                    if not state[column]["exists"]:
                        findings.append(_finding(
                            kind="column_removed_while_still_written",
                            severity="error",
                            environment=env.name,
                            migration=source.name if source else None,
                            schema_element=column_label(column),
                            artifact=entry.artifact,
                            artifact_version=entry.artifact,
                            failed_rule="all_active_writes_exist",
                            witness={
                                "schema_version": schema_version,
                                "environment_role": entry.role,
                                "declared_capability": "writes",
                            },
                            minimal_conflict_set={
                                "environment": env.name,
                                "artifact": entry.artifact,
                                "migration": source.name if source else None,
                                "schema_element": column_label(column),
                            },
                            repair_candidates=_compat_repairs("writes", entry.artifact, column, env.name),
                            assumptions=assumptions,
                        ))
                findings.extend(_api_offline_findings(
                    system,
                    env,
                    entry,
                    artifact,
                    schema_version,
                    rules,
                    assumptions,
                ))
    return findings


def _apply_op(state: Dict[ColumnKey, dict], op: DbMigrationOp):
    cell = state[op.column]
    if op.op == "add":
        cell["exists"] = True
        cell["backfilled"] = False
        cell["not_null"] = op.nullability == "not_null"
    elif op.op == "backfill":
        if cell["exists"]:
            cell["backfilled"] = True
    elif op.op == "set_not_null":
        if cell["exists"]:
            cell["not_null"] = True
    elif op.op == "drop":
        cell["exists"] = False
        cell["backfilled"] = False
        cell["not_null"] = False
    elif op.op == "rename":
        target = state[op.columns[0]]
        target["exists"] = True
        target["backfilled"] = cell["backfilled"]
        target["not_null"] = cell["not_null"]
        cell["exists"] = False
        cell["backfilled"] = False
        cell["not_null"] = False
    elif op.op == "split":
        cell["exists"] = False
        cell["backfilled"] = False
        cell["not_null"] = False
        for target_key in op.columns:
            target = state[target_key]
            target["exists"] = True
            target["backfilled"] = True
            target["not_null"] = False
    elif op.op == "merge":
        for source_key in op.columns:
            source = state[source_key]
            source["exists"] = False
            source["backfilled"] = False
            source["not_null"] = False
        cell["exists"] = True
        cell["backfilled"] = True
        cell["not_null"] = False


def _op_findings(
        system,
        migration: DbMigration,
        op: DbMigrationOp,
        before_state,
        state,
        assumptions,
        rules):
    findings = []
    if op.op in ("add", "set_not_null") and state[op.column]["not_null"] and not state[op.column]["backfilled"]:
        findings.append(_finding(
            kind="not_null_before_backfill",
            severity="error",
            environment=None,
            migration=migration.name,
            schema_element=column_label(op.column),
            artifact=None,
            artifact_version=None,
            failed_rule="not_null_after_backfill",
            witness={
                "schema_version": migration.to_schema,
                "operation": op.op,
                "column": column_label(op.column),
            },
            minimal_conflict_set={
                "migration": migration.name,
                "schema_element": column_label(op.column),
            },
            repair_candidates=[
                {
                    "kind": "compat_shim",
                    "weakens_spec": False,
                    "description": (
                        f"backfill {column_label(op.column)} before setting it not_null"
                    ),
                },
                {
                    "kind": "migration_change",
                    "weakens_spec": False,
                    "description": (
                        f"keep {column_label(op.column)} nullable until a later migration"
                    ),
                },
                {
                    "kind": "declaration_change",
                    "weakens_spec": True,
                    "description": (
                        "remove the not_null marker only if the product contract truly allows nulls"
                    ),
                },
            ],
            assumptions=assumptions,
        ))
    annotations = _annotations(migration, op)
    if (
            "destructive_operations_annotated" in rules
            and op.op == "drop"
            and before_state[op.column]["exists"]
            and not annotations.intersection({"destructive", "irreversible"})):
        findings.append(_finding(
            kind="destructive_migration_unannotated",
            severity="error",
            environment=None,
            migration=migration.name,
            schema_element=column_label(op.column),
            artifact=None,
            artifact_version=None,
            failed_rule="destructive_operations_annotated",
            witness={
                "schema_version": migration.to_schema,
                "operation": op.op,
                "column": column_label(op.column),
                "annotations": sorted(annotations),
            },
            minimal_conflict_set={
                "migration": migration.name,
                "schema_element": column_label(op.column),
            },
            repair_candidates=_destructive_repairs(op.column),
            assumptions=assumptions,
        ))
    if (
            "preservation_transforms_annotated" in rules
            and op.op in ("split", "merge")
            and not annotations.intersection({"lossless", "lossy", "irreversible"})):
        findings.append(_finding(
            kind="preservation_transform_unannotated",
            severity="error",
            environment=None,
            migration=migration.name,
            schema_element=_transform_label(op),
            artifact=None,
            artifact_version=None,
            failed_rule="preservation_transforms_annotated",
            witness={
                "schema_version": migration.to_schema,
                "operation": op.op,
                "annotations": sorted(annotations),
            },
            minimal_conflict_set={
                "migration": migration.name,
                "schema_element": _transform_label(op),
            },
            repair_candidates=_preservation_repairs(op),
            assumptions=assumptions,
        ))
    if "data_preserved" in rules and _is_preservation_loss(op, annotations, before_state):
        findings.append(_finding(
            kind="data_preservation_loss",
            severity="error",
            environment=None,
            migration=migration.name,
            schema_element=_transform_label(op),
            artifact=None,
            artifact_version=None,
            failed_rule="data_preserved",
            witness={
                "schema_version": migration.to_schema,
                "operation": op.op,
                "annotations": sorted(annotations),
                "bounded_row_model": True,
            },
            minimal_conflict_set={
                "migration": migration.name,
                "schema_element": _transform_label(op),
            },
            repair_candidates=_preservation_repairs(op),
            assumptions=assumptions,
        ))
    if (
            "rollback_equivalent" in rules
            and "rollbackable" in set(migration.annotations)
            and _breaks_rollback_equivalence(op, annotations, before_state)):
        findings.append(_finding(
            kind="rollback_not_equivalent",
            severity="error",
            environment=None,
            migration=migration.name,
            schema_element=_transform_label(op),
            artifact=None,
            artifact_version=None,
            failed_rule="rollback_equivalent",
            witness={
                "schema_version": migration.to_schema,
                "operation": op.op,
                "annotations": sorted(annotations),
                "check": "up_down_observable_equivalence",
            },
            minimal_conflict_set={
                "migration": migration.name,
                "schema_element": _transform_label(op),
            },
            repair_candidates=_rollback_repairs(op),
            assumptions=assumptions,
        ))
    return findings


def _annotations(migration: DbMigration, op: DbMigrationOp):
    return set(migration.annotations) | set(op.annotations)


def _transform_label(op: DbMigrationOp):
    if op.op == "rename":
        return f"{column_label(op.column)}->{column_label(op.columns[0])}"
    if op.op == "split":
        return f"{column_label(op.column)}->{','.join(column_label(c) for c in op.columns)}"
    if op.op == "merge":
        return f"{','.join(column_label(c) for c in op.columns)}->{column_label(op.column)}"
    return column_label(op.column)


def _is_preservation_loss(op: DbMigrationOp, annotations, before_state):
    if op.op == "drop":
        return before_state[op.column]["exists"]
    if op.op in ("split", "merge"):
        return "lossless" not in annotations
    return False


def _breaks_rollback_equivalence(op: DbMigrationOp, annotations, before_state):
    if op.op == "drop":
        return before_state[op.column]["exists"]
    if op.op in ("split", "merge"):
        return "lossless" not in annotations
    return False


def _entry_active_at(env, entry, schema_version):
    window = entry.schema_window or env.schema_window
    return window[0] <= schema_version <= window[1]


def _providers_for(system, env, schema_version, capability, target):
    providers = []
    artifacts = system.artifact_map()
    for entry in env.artifacts:
        if entry.role == "may_exist" or not _entry_active_at(env, entry, schema_version):
            continue
        artifact = artifacts[entry.artifact]
        if target in getattr(artifact, capability):
            providers.append(entry.artifact)
    return providers


def _api_offline_findings(system, env, entry, artifact, schema_version, rules, assumptions):
    findings = []
    if "api_calls_accepted" in rules:
        for target in artifact.calls:
            if not _providers_for(system, env, schema_version, "accepts", target):
                findings.append(_capability_finding(
                    "api_call_not_accepted",
                    "api_calls_accepted",
                    env.name,
                    entry,
                    schema_version,
                    "calls",
                    target,
                    assumptions,
                ))
    if "api_responses_expected" in rules:
        for target in artifact.expects:
            if not _providers_for(system, env, schema_version, "responds", target):
                findings.append(_capability_finding(
                    "api_response_field_missing",
                    "api_responses_expected",
                    env.name,
                    entry,
                    schema_version,
                    "expects",
                    target,
                    assumptions,
                ))
    if "offline_payloads_accepted" in rules:
        for target in artifact.emits_offline:
            if not _providers_for(system, env, schema_version, "accepts", target):
                ttl = artifact.offline_ttls.get(target)
                findings.append(_capability_finding(
                    "offline_payload_not_accepted",
                    "offline_payloads_accepted",
                    env.name,
                    entry,
                    schema_version,
                    "emits_offline",
                    target,
                    assumptions,
                    extra_witness={"ttl_ticks": ttl} if ttl is not None else None,
                ))
    return findings


def _capability_finding(
        kind,
        failed_rule,
        env_name,
        entry,
        schema_version,
        capability,
        target,
        assumptions,
        extra_witness=None):
    witness = {
        "schema_version": schema_version,
        "environment_role": entry.role,
        "declared_capability": capability,
        "target": column_label(target),
    }
    if extra_witness:
        witness.update(extra_witness)
    return _finding(
        kind=kind,
        severity="error",
        environment=env_name,
        migration=None,
        schema_element=column_label(target),
        artifact=entry.artifact,
        artifact_version=entry.artifact,
        failed_rule=failed_rule,
        witness=witness,
        minimal_conflict_set={
            "environment": env_name,
            "artifact": entry.artifact,
            "schema_element": column_label(target),
        },
        repair_candidates=_capability_repairs(capability, entry.artifact, target, env_name),
        assumptions=assumptions,
    )


def _load_observation_events(trace_path):
    try:
        data = json.loads(Path(trace_path).read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise FslError(f"invalid observation JSON: {exc.msg}", kind="parse") from exc
    if isinstance(data, list):
        return data
    if isinstance(data, dict) and isinstance(data.get("events"), list):
        return data["events"]
    raise FslError("observation JSON must be an array or {\"events\": [...]}", kind="semantics")


def _target_key(raw):
    if isinstance(raw, (list, tuple)) and len(raw) == 2:
        return (str(raw[0]), str(raw[1]))
    text = str(raw)
    left, sep, right = text.partition(".")
    if not sep:
        return ("unknown", text)
    return (left, right)


def _observation_findings(system, events, assumptions):
    findings = []
    artifacts = system.artifact_map()
    envs = {env.name: env for env in system.environments}
    for idx, event in enumerate(events):
        env = envs.get(event.get("environment"))
        artifact_name = event.get("artifact")
        capability = event.get("capability")
        target = _target_key(event.get("target"))
        schema_version = event.get("schema_version")
        if env is None or artifact_name not in artifacts or not _artifact_declared_in_env(env, artifact_name, schema_version):
            findings.append(_observation_finding(
                "unsupported_artifact_observed",
                event,
                idx,
                target,
                assumptions,
                "observed artifact is not declared in the environment/schema window",
            ))
            continue
        artifact = artifacts[artifact_name]
        declared = getattr(artifact, capability, []) if isinstance(capability, str) else []
        if capability in ("reads", "writes") and target not in declared:
            findings.append(_observation_finding(
                "declared_unused_but_observed",
                event,
                idx,
                target,
                assumptions,
                "observed DB access is not declared as an artifact capability",
            ))
        elif capability == "calls" and not _providers_for(system, env, int(schema_version), "accepts", target):
            findings.append(_observation_finding(
                "legacy_api_still_called",
                event,
                idx,
                target,
                assumptions,
                "observed API call is not accepted by an active/supported artifact",
            ))
    return findings


def _artifact_declared_in_env(env, artifact_name, schema_version):
    try:
        version = int(schema_version)
    except (TypeError, ValueError):
        return False
    return any(
        entry.artifact == artifact_name and _entry_active_at(env, entry, version)
        for entry in env.artifacts
    )


def _observation_finding(kind, event, index, target, assumptions, reason):
    return _finding(
        kind=kind,
        severity="error",
        environment=event.get("environment"),
        migration=None,
        schema_element=column_label(target),
        artifact=event.get("artifact"),
        artifact_version=event.get("artifact"),
        failed_rule="runtime_observation",
        witness={
            "event_index": index,
            "schema_version": event.get("schema_version"),
            "capability": event.get("capability"),
            "target": column_label(target),
            "reason": reason,
        },
        minimal_conflict_set={
            "environment": event.get("environment"),
            "artifact": event.get("artifact"),
            "schema_element": column_label(target),
        },
        repair_candidates=_observation_repairs(event, target),
        assumptions=assumptions,
        result="observed_mismatch",
    )


def _finding(
        kind,
        severity,
        environment,
        migration,
        schema_element,
        artifact,
        artifact_version,
        failed_rule,
        witness,
        minimal_conflict_set,
        repair_candidates,
        assumptions,
        result="violated"):
    return {
        "schema_version": DB_FINDING_SCHEMA_VERSION,
        "fsl": DB_DIALECT_VERSION,
        "result": result,
        "kind": kind,
        "severity": severity,
        "environment": environment,
        "migration": migration,
        "schema_element": schema_element,
        "artifact": artifact,
        "artifact_version": artifact_version,
        "failed_rule": failed_rule,
        "witness": witness,
        "minimal_conflict_set": minimal_conflict_set,
        "repair_candidates": repair_candidates,
        "assumptions": assumptions,
        "redaction": {
            "policy": "schema identifiers only; row values, SQL literals, and secrets are not emitted",
        },
    }


def _compat_repairs(capability, artifact, column, env_name):
    label = column_label(column)
    return [
        {
            "kind": "compat_shim",
            "weakens_spec": False,
            "description": f"keep or restore {label} until {artifact} is outside {env_name}",
        },
        {
            "kind": "rollout_window_change",
            "weakens_spec": False,
            "description": f"narrow the {artifact} environment window before dropping {label}",
        },
        {
            "kind": "declaration_change",
            "weakens_spec": True,
            "description": (
                f"remove the declared {capability} capability only if {artifact} truly no longer uses {label}"
            ),
        },
    ]


def _destructive_repairs(column):
    label = column_label(column)
    return [
        {
            "kind": "annotation_change",
            "weakens_spec": False,
            "description": f"mark the operation as irreversible/destructive if {label} loss is intended",
        },
        {
            "kind": "compat_shim",
            "weakens_spec": False,
            "description": f"keep {label} or replace it with a compatibility shim before dropping it",
        },
    ]


def _preservation_repairs(op):
    label = _transform_label(op)
    return [
        {
            "kind": "preservation_mapping",
            "weakens_spec": False,
            "description": f"provide a lossless preservation transform for {label}",
        },
        {
            "kind": "annotation_change",
            "weakens_spec": False,
            "description": f"mark {label} as lossy/irreversible when information loss is intended",
        },
        {
            "kind": "migration_change",
            "weakens_spec": False,
            "description": f"split the migration so old and new representations coexist during backfill for {label}",
        },
    ]


def _rollback_repairs(op):
    label = _transform_label(op)
    return [
        {
            "kind": "rollback_contract_change",
            "weakens_spec": False,
            "description": f"remove rollbackable from the migration unless {label} has an inverse",
        },
        {
            "kind": "preservation_mapping",
            "weakens_spec": False,
            "description": f"add a lossless inverse transform for {label}",
        },
    ]


def _capability_repairs(capability, artifact, target, env_name):
    label = column_label(target)
    return [
        {
            "kind": "compat_shim",
            "weakens_spec": False,
            "description": f"keep an active provider for {label} while {artifact} is in {env_name}",
        },
        {
            "kind": "rollout_window_change",
            "weakens_spec": False,
            "description": f"narrow the {artifact} environment window until {label} is supported",
        },
        {
            "kind": "declaration_change",
            "weakens_spec": True,
            "description": f"remove the declared {capability} capability only if {artifact} truly no longer uses {label}",
        },
    ]


def _observation_repairs(event, target):
    label = column_label(target)
    artifact = event.get("artifact")
    return [
        {
            "kind": "declaration_change",
            "weakens_spec": False,
            "description": f"declare the observed {event.get('capability')} capability for {artifact} on {label}",
        },
        {
            "kind": "rollout_window_change",
            "weakens_spec": False,
            "description": f"keep {artifact} in the environment window until observations stop",
        },
        {
            "kind": "compat_shim",
            "weakens_spec": False,
            "description": f"restore compatibility for observed use of {label}",
        },
    ]


def _translate_kernel_result(kernel, system, expansion):
    if kernel.get("result") != "violated":
        return []
    invariant = kernel.get("invariant")
    meta = expansion.invariant_metadata.get(invariant)
    if not meta:
        return []
    trace = kernel.get("trace") or []
    last = trace[-1] if trace else {}
    witness = {
        "kernel_invariant": invariant,
        "violated_at_step": kernel.get("violated_at_step"),
        "state": last.get("state", last),
    }
    return [_finding(
        kind=meta.get("kind"),
        severity="error",
        environment=meta.get("environment"),
        migration=_migration_from_trace(trace),
        schema_element=meta.get("schema_element"),
        artifact=meta.get("artifact"),
        artifact_version=meta.get("artifact_version"),
        failed_rule=meta.get("failed_rule"),
        witness=witness,
        minimal_conflict_set={
            key: value
            for key, value in {
                "environment": meta.get("environment"),
                "artifact": meta.get("artifact"),
                "migration": _migration_from_trace(trace),
                "schema_element": meta.get("schema_element"),
            }.items()
            if value is not None
        },
        repair_candidates=_repairs_for_meta(meta),
        assumptions=expansion.assumptions,
    )]


def _migration_from_trace(trace):
    for step in reversed(trace or []):
        action = step.get("action") if isinstance(step, dict) else None
        if isinstance(action, str) and action.startswith("migrate_"):
            return action[len("migrate_"):]
    return None


def _repairs_for_meta(meta):
    if meta.get("kind") == "not_null_before_backfill":
        column = meta.get("schema_element")
        return [
            {
                "kind": "compat_shim",
                "weakens_spec": False,
                "description": f"backfill {column} before setting it not_null",
            },
            {
                "kind": "declaration_change",
                "weakens_spec": True,
                "description": "weaken not_null only if nulls are part of the intended contract",
            },
        ]
    column = tuple((meta.get("schema_element") or ".").split(".", 1))
    if len(column) != 2:
        column = ("unknown", "unknown")
    capability = "reads" if meta.get("kind") == "column_removed_while_still_read" else "writes"
    return _compat_repairs(
        capability,
        meta.get("artifact"),
        column,  # type: ignore[arg-type]
        meta.get("environment"),
    )
