# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""AI-readable fsl-db compatibility checks and finding translation."""
from __future__ import annotations

from copy import deepcopy
from pathlib import Path
from typing import Dict, List, Optional

from .bmc import prove, verify
from .db_expand import DB_DIALECT_VERSION, DB_FINDING_SCHEMA_VERSION, expand_dbsystem
from .db_ir import ColumnKey, DbMigration, DbMigrationOp, DbSystem, column_label
from .db_parser import parse_dbsystem
from .model import build_spec


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


def _static_findings(system, assumptions):
    states = {system.database.initial_schema: _initial_state(system)}
    sources = {system.database.initial_schema: None}
    findings = []

    current_schema = system.database.initial_schema
    current_state = deepcopy(states[current_schema])
    for migration in system.migrations:
        next_state = deepcopy(current_state)
        for op in migration.ops:
            _apply_op(next_state, op)
            findings.extend(_op_findings(system, migration, op, next_state, assumptions))
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


def _op_findings(system, migration: DbMigration, op: DbMigrationOp, state, assumptions):
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
    return findings


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
        assumptions):
    return {
        "schema_version": DB_FINDING_SCHEMA_VERSION,
        "fsl": DB_DIALECT_VERSION,
        "result": "violated",
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
