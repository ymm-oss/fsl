# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Lower fsl-db typed IR into the existing FSL kernel AST."""
from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Dict, List, Optional, Tuple

from .db_ir import (
    ColumnKey,
    DbArtifact,
    DbEnvironment,
    DbEnvironmentArtifact,
    DbMigration,
    DbSystem,
    column_label,
)
from .model import FslError


DB_FINDING_SCHEMA_VERSION = "fsl-db-finding.v0"
DB_DIALECT_VERSION = "fsl-db-mvp.v0"

DEFAULT_RULES = (
    "all_active_reads_exist",
    "all_active_writes_exist",
    "removed_only_after_unused",
    "not_null_after_backfill",
    "destructive_operations_annotated",
    "preservation_transforms_annotated",
    "api_calls_accepted",
    "api_responses_expected",
    "offline_payloads_accepted",
)
ALLOWED_RULES = frozenset(DEFAULT_RULES + (
    "data_preserved",
    "rollback_equivalent",
))


@dataclass(frozen=True)
class DbKernelExpansion:
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


def _column_member(column: ColumnKey):
    return "col_" + _safe(column[0]) + "_" + _safe(column[1])


def _artifact_member(name):
    return "art_" + _safe(name)


def _inv_name(*parts):
    return "__".join(_safe(part) for part in parts)


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


def _bin(op, left, right):
    return ("bin", op, left, right)


def _and_all(exprs):
    if not exprs:
        return _bool(True)
    out = exprs[0]
    for expr in exprs[1:]:
        out = _bin("and", out, expr)
    return out


def _or_all(exprs):
    if not exprs:
        return _bool(False)
    out = exprs[0]
    for expr in exprs[1:]:
        out = _bin("or", out, expr)
    return out


def _not(expr):
    return ("not", expr)


def _meta(rule, text):
    return {"id": rule, "text": text}


def _active_rules(system):
    rules = list(system.check.rules or DEFAULT_RULES)
    for rule in rules:
        if rule not in ALLOWED_RULES:
            _err(
                f"unknown db compatibility rule '{rule}'",
                loc=system.check.loc,
                hint="supported rules: " + ", ".join(sorted(ALLOWED_RULES)),
            )
    return set(rules)


def validate_dbsystem(system):
    """Validate reference integrity and MVP rollout-shape constraints."""
    if not system.database.tables:
        _err("database requires at least one table", loc=system.database.loc)

    table_names = set()
    columns = {}
    for table in system.database.tables:
        if table.name in table_names:
            _err(f"duplicate table '{table.name}'", loc=table.loc)
        table_names.add(table.name)
        seen_columns = set()
        for column in table.columns:
            if column.name in seen_columns:
                _err(f"duplicate column '{column.label}'", loc=column.loc)
            seen_columns.add(column.name)
            if not column.present and (column.backfilled or column.not_null):
                _err(
                    f"absent column '{column.label}' cannot be backfilled or not_null",
                    loc=column.loc,
                )
            if column.not_null and not column.backfilled:
                _err(
                    f"not_null column '{column.label}' must start backfilled",
                    loc=column.loc,
                )
            columns[column.key] = column

    if not columns:
        _err("database requires at least one column", loc=system.database.loc)

    migration_names = set()
    current = system.database.initial_schema
    reached_versions = {current}
    for migration in system.migrations:
        if migration.name in migration_names:
            _err(f"duplicate migration '{migration.name}'", loc=migration.loc)
        migration_names.add(migration.name)
        if migration.to_schema <= migration.from_schema:
            _err(
                f"migration '{migration.name}' must increase schema version in the MVP",
                loc=migration.loc,
                hint="rollback/down migrations are reserved for the preservation/rollback phase",
            )
        if migration.from_schema != current:
            _err(
                f"migration '{migration.name}' starts at schema {migration.from_schema}, "
                f"but the declared rollout plan is currently at {current}",
                loc=migration.loc,
                hint="MVP dbsystem migrations are a single declared rollout sequence",
            )
        for op in migration.ops:
            refs = [op.column] + list(op.columns)
            for ref in refs:
                if ref not in columns:
                    _err(f"unknown column '{column_label(ref)}'", loc=op.loc)
        current = migration.to_schema
        reached_versions.add(current)

    artifacts = {}
    for artifact in system.artifacts:
        if artifact.name in artifacts:
            _err(f"duplicate artifact '{artifact.name}'", loc=artifact.loc)
        for ref in _artifact_db_columns(artifact):
            if ref not in columns:
                _err(f"unknown column '{column_label(ref)}'", loc=artifact.loc)
        artifacts[artifact.name] = artifact

    if not artifacts:
        _err("dbsystem requires at least one artifact", loc=system.loc)

    env_names = set()
    for env in system.environments:
        if env.name in env_names:
            _err(f"duplicate environment '{env.name}'", loc=env.loc)
        env_names.add(env.name)
        _validate_window(env.schema_window, "environment schema", env.loc)
        for version in range(env.schema_window[0], env.schema_window[1] + 1):
            if version not in reached_versions:
                _err(
                    f"environment '{env.name}' includes schema {version}, "
                    "which is not reachable in the declared migration plan",
                    loc=env.loc,
                )
        for entry in env.artifacts:
            if entry.artifact not in artifacts:
                _err(f"unknown artifact '{entry.artifact}'", loc=entry.loc)
            window = _effective_window(env, entry)
            _validate_window(window, "artifact schema window", entry.loc)
            if window[0] < env.schema_window[0] or window[1] > env.schema_window[1]:
                _err(
                    f"artifact '{entry.artifact}' window {window[0]}..{window[1]} "
                    f"must stay within environment '{env.name}' schema "
                    f"{env.schema_window[0]}..{env.schema_window[1]}",
                    loc=entry.loc,
                )

    if not system.environments:
        _err("dbsystem requires at least one environment", loc=system.loc)

    _active_rules(system)


def _validate_window(window, label, loc=None):
    if window[0] > window[1]:
        _err(f"{label} has an empty range {window[0]}..{window[1]}", loc=loc)


def _artifact_db_columns(artifact: DbArtifact):
    return list(artifact.reads) + list(artifact.writes)


def _effective_window(env: DbEnvironment, entry: DbEnvironmentArtifact):
    return entry.schema_window or env.schema_window


def _schema_range(system):
    values = [system.database.initial_schema]
    for migration in system.migrations:
        values.extend([migration.from_schema, migration.to_schema])
    for env in system.environments:
        values.extend([env.schema_window[0], env.schema_window[1]])
        for entry in env.artifacts:
            if entry.schema_window:
                values.extend([entry.schema_window[0], entry.schema_window[1]])
    return min(values), max(values)


def _column_members(system):
    return {_key: _column_member(_key) for _key in system.column_map()}


def _artifact_members(system):
    return {artifact.name: _artifact_member(artifact.name) for artifact in system.artifacts}


def _schema_window_expr(window):
    lo, hi = window
    version = _var("schema_version")
    return _bin("and", _bin(">=", version, _num(lo)), _bin("<=", version, _num(hi)))


def _outside_schema_window_expr(window):
    return _not(_schema_window_expr(window))


def _compat_expr(window, column_member):
    return _or_all([
        _outside_schema_window_expr(window),
        _idx("column_exists", _var(column_member)),
    ])


def _rule_text(rule, env, role, artifact, column):
    if rule == "all_active_reads_exist":
        return (
            f"{role} artifact {artifact} in {env} must not read missing column "
            f"{column_label(column)}"
        )
    if rule == "all_active_writes_exist":
        return (
            f"{role} artifact {artifact} in {env} must not write missing column "
            f"{column_label(column)}"
        )
    return (
        f"column {column_label(column)} may be removed only after {artifact} "
        f"is outside the {env} compatibility window"
    )


def expand_dbsystem(system):
    validate_dbsystem(system)
    rules = _active_rules(system)
    columns = system.column_map()
    col_member = _column_members(system)
    artifact_member = _artifact_members(system)
    schema_lo, schema_hi = _schema_range(system)

    items = [
        ("__spec_meta", _meta("db", "database multi-environment compatibility")),
        ("type", "SchemaVersion", _num(schema_lo), _num(schema_hi)),
        ("enum", "Column", [col_member[key] for key in columns]),
        ("enum", "Artifact", [artifact_member[name] for name in artifact_member]),
        ("state", [
            ("decl", "schema_version", ("name", "SchemaVersion")),
            ("decl", "column_exists", ("map", ("name", "Column"), ("bool",))),
            ("decl", "column_backfilled", ("map", ("name", "Column"), ("bool",))),
            ("decl", "column_not_null", ("map", ("name", "Column"), ("bool",))),
        ]),
    ]

    init = [("assign", ("var", "schema_version"), _num(system.database.initial_schema), None)]
    for key, column in columns.items():
        member = _var(col_member[key])
        init.append(_assign_index("column_exists", member, _bool(column.present), column.loc))
        init.append(_assign_index("column_backfilled", member, _bool(column.backfilled), column.loc))
        init.append(_assign_index("column_not_null", member, _bool(column.not_null), column.loc))
    items.append(("init", init))

    generated_names = []
    for migration in system.migrations:
        action_name = "migrate_" + _safe(migration.name)
        generated_names.append(action_name)
        body = [
            ("requires", _bin("==", _var("schema_version"), _num(migration.from_schema)), migration.loc)
        ]
        for op in migration.ops:
            member = _var(col_member[op.column])
            if op.op == "add":
                body.append(_assign_index("column_exists", member, _bool(True), op.loc))
                body.append(_assign_index("column_backfilled", member, _bool(False), op.loc))
                body.append(_assign_index(
                    "column_not_null",
                    member,
                    _bool(op.nullability == "not_null"),
                    op.loc,
                ))
            elif op.op == "backfill":
                body.append(("requires", _idx("column_exists", member), op.loc))
                body.append(_assign_index("column_backfilled", member, _bool(True), op.loc))
            elif op.op == "set_not_null":
                body.append(("requires", _idx("column_exists", member), op.loc))
                body.append(("requires", _idx("column_backfilled", member), op.loc))
                body.append(_assign_index("column_not_null", member, _bool(True), op.loc))
            elif op.op == "drop":
                body.append(_assign_index("column_exists", member, _bool(False), op.loc))
                body.append(_assign_index("column_backfilled", member, _bool(False), op.loc))
                body.append(_assign_index("column_not_null", member, _bool(False), op.loc))
            elif op.op == "rename":
                target = _var(col_member[op.columns[0]])
                body.append(("requires", _idx("column_exists", member), op.loc))
                body.append(_assign_index("column_exists", member, _bool(False), op.loc))
                body.append(_assign_index("column_backfilled", member, _bool(False), op.loc))
                body.append(_assign_index("column_not_null", member, _bool(False), op.loc))
                body.append(_assign_index("column_exists", target, _bool(True), op.loc))
                body.append(_assign_index("column_backfilled", target, _bool(True), op.loc))
                body.append(_assign_index("column_not_null", target, _idx("column_not_null", member), op.loc))
            elif op.op == "split":
                body.append(("requires", _idx("column_exists", member), op.loc))
                body.append(_assign_index("column_exists", member, _bool(False), op.loc))
                body.append(_assign_index("column_backfilled", member, _bool(False), op.loc))
                body.append(_assign_index("column_not_null", member, _bool(False), op.loc))
                for target_key in op.columns:
                    target = _var(col_member[target_key])
                    body.append(_assign_index("column_exists", target, _bool(True), op.loc))
                    body.append(_assign_index("column_backfilled", target, _bool(True), op.loc))
                    body.append(_assign_index("column_not_null", target, _bool(False), op.loc))
            elif op.op == "merge":
                target = member
                for source_key in op.columns:
                    source = _var(col_member[source_key])
                    body.append(("requires", _idx("column_exists", source), op.loc))
                    body.append(_assign_index("column_exists", source, _bool(False), op.loc))
                    body.append(_assign_index("column_backfilled", source, _bool(False), op.loc))
                    body.append(_assign_index("column_not_null", source, _bool(False), op.loc))
                body.append(_assign_index("column_exists", target, _bool(True), op.loc))
                body.append(_assign_index("column_backfilled", target, _bool(True), op.loc))
                body.append(_assign_index("column_not_null", target, _bool(False), op.loc))
        body.append(("assign", ("var", "schema_version"), _num(migration.to_schema), migration.loc))
        items.append(("action", action_name, [], body, migration.loc, False, _meta(
            "DB-MIGRATION",
            f"{migration.name}: schema {migration.from_schema} -> {migration.to_schema}",
        )))

    if not system.migrations:
        action_name = "observe_schema_" + _safe(str(system.database.initial_schema))
        generated_names.append(action_name)
        items.append(("action", action_name, [], [
            ("requires", _bin("==", _var("schema_version"), _num(system.database.initial_schema)), system.database.loc),
            ("assign", ("var", "schema_version"), _num(system.database.initial_schema), system.database.loc),
        ], system.database.loc, False, _meta(
            "DB-SNAPSHOT",
            f"{system.database.name}: schema {system.database.initial_schema} static compatibility snapshot",
        )))

    invariant_metadata = {}
    if "not_null_after_backfill" in rules:
        for key in columns:
            name = _inv_name("db_not_null_after_backfill", key[0], key[1])
            member = _var(col_member[key])
            expr = _bin(
                "=>",
                _idx("column_not_null", member),
                _and_all([
                    _idx("column_exists", member),
                    _idx("column_backfilled", member),
                ]),
            )
            meta = _meta(
                "DB-NOT-NULL",
                f"{column_label(key)} can be not_null only after it exists and is backfilled",
            )
            items.append(("invariant", name, expr, None, meta))
            generated_names.append(name)
            invariant_metadata[name] = {
                "kind": "not_null_before_backfill",
                "failed_rule": "not_null_after_backfill",
                "schema_element": column_label(key),
            }

    for env in system.environments:
        for entry in env.artifacts:
            artifact = system.artifact_map()[entry.artifact]
            window = _effective_window(env, entry)
            if "all_active_reads_exist" in rules or "removed_only_after_unused" in rules:
                for key in artifact.reads:
                    name = _inv_name(
                        "db_read", env.name, entry.role, entry.artifact, key[0], key[1])
                    expr = _compat_expr(window, col_member[key])
                    rule = "all_active_reads_exist"
                    meta = _meta("DB-COMPAT-READ", _rule_text(rule, env.name, entry.role, entry.artifact, key))
                    items.append(("invariant", name, expr, entry.loc, meta))
                    generated_names.append(name)
                    invariant_metadata[name] = _compat_metadata(
                        "column_removed_while_still_read", rule, env.name, entry, key, window)
            if "all_active_writes_exist" in rules or "removed_only_after_unused" in rules:
                for key in artifact.writes:
                    name = _inv_name(
                        "db_write", env.name, entry.role, entry.artifact, key[0], key[1])
                    expr = _compat_expr(window, col_member[key])
                    rule = "all_active_writes_exist"
                    meta = _meta("DB-COMPAT-WRITE", _rule_text(rule, env.name, entry.role, entry.artifact, key))
                    items.append(("invariant", name, expr, entry.loc, meta))
                    generated_names.append(name)
                    invariant_metadata[name] = _compat_metadata(
                        "column_removed_while_still_written", rule, env.name, entry, key, window)

    final_schema = system.migrations[-1].to_schema if system.migrations else system.database.initial_schema
    items.append(("terminal", _bin("==", _var("schema_version"), _num(final_schema)), None))
    items.append(("__generated", generated_names))

    assumptions = [
        {
            "id": "DB-ASSUME-ROLLING-SNAPSHOT",
            "text": (
                "environment schema ranges denote finite snapshots reachable in the "
                "declared migration order; percentages are modeled only as coexistence windows"
            ),
        },
        {
            "id": "DB-ASSUME-CAPABILITY-DECLARATIONS",
            "text": "artifact capability declarations are complete for the checked compatibility window",
        },
    ]
    if any(artifact.emits_offline for artifact in system.artifacts):
        assumptions.append({
            "id": "DB-ASSUME-OFFLINE-TTL-FINITE",
            "text": "offline TTL values are finite logical ticks, not wall-clock time or probability",
        })
    if "data_preserved" in rules or "rollback_equivalent" in rules:
        assumptions.append({
            "id": "DB-ASSUME-BOUNDED-ROW-MODEL",
            "text": (
                "preservation and rollback checks use a bounded abstract row model; "
                "they are not a proof over all production rows"
            ),
        })

    display_names = {
        "schema_version": "schema.version",
        "column_exists": "column.exists",
        "column_backfilled": "column.backfilled",
        "column_not_null": "column.not_null",
    }
    return DbKernelExpansion(
        ast=("spec", system.name, items),
        display_names=display_names,
        invariant_metadata=invariant_metadata,
        assumptions=assumptions,
    )


def _compat_metadata(kind, rule, env_name, entry, column, window):
    return {
        "kind": kind,
        "failed_rule": rule,
        "environment": env_name,
        "environment_role": entry.role,
        "artifact": entry.artifact,
        "artifact_version": entry.artifact,
        "schema_element": column_label(column),
        "schema_window": [window[0], window[1]],
    }
