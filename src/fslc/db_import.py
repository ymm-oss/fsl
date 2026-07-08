# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Minimal SQL DDL importer for the fsl-db typed IR boundary."""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Tuple

from .db_ir import (
    ColumnKey,
    DbArtifact,
    DbCheck,
    DbColumn,
    DbDatabase,
    DbEnvironment,
    DbEnvironmentArtifact,
    DbMigration,
    DbMigrationOp,
    DbSystem,
    DbTable,
)


@dataclass
class DbImportResult:
    system: DbSystem
    source: str
    source_format: str
    warnings: List[dict] = field(default_factory=list)


def import_db_file(path, name="ImportedDb", source_format="auto"):
    text = Path(path).read_text(encoding="utf-8")
    selected = _select_source_format(path, source_format)
    if selected == "prisma":
        return import_prisma(text, name=name)
    return import_sql(text, name=name)


def import_sql_file(path, name="ImportedDb"):
    return import_sql(Path(path).read_text(encoding="utf-8"), name=name)


def import_sql(sql, name="ImportedDb"):
    warnings: List[dict] = []
    columns: Dict[ColumnKey, DbColumn] = {}
    migrations: List[DbMigration] = []
    current_schema = 0

    for raw in _statements(sql):
        parsed = _parse_statement(raw)
        if parsed is None:
            warnings.append({
                "kind": "unsupported_sql",
                "statement": raw,
                "message": "SQL importer supports CREATE TABLE, ALTER TABLE ADD/DROP/RENAME COLUMN, and UPDATE ... SET for backfill",
            })
            continue
        kind = parsed[0]
        if kind == "create_table":
            _, table, defs = parsed
            for column_name, db_type, not_null in defs:
                columns[(table, column_name)] = DbColumn(
                    table=table,
                    name=column_name,
                    db_type=db_type,
                    present=True,
                    backfilled=True,
                    not_null=not_null,
                )
        else:
            current_schema += 1
            migration = _migration_from_sql(parsed, current_schema - 1, current_schema, columns)
            migrations.append(migration)

    if not columns:
        columns[("imported", "id")] = DbColumn(
            table="imported",
            name="id",
            db_type="Int",
            present=True,
            backfilled=True,
            not_null=True,
        )
        warnings.append({
            "kind": "empty_import",
            "message": "no supported table definition was found; emitted a placeholder schema",
        })

    tables = []
    for table in sorted({key[0] for key in columns}):
        tables.append(DbTable(
            name=table,
            columns=[columns[key] for key in sorted(columns) if key[0] == table],
        ))

    final_columns = _final_existing_columns(columns, migrations)
    artifact = DbArtifact(
        name="imported_artifact",
        reads=final_columns,
        writes=final_columns,
    )
    final_schema = current_schema
    environment = DbEnvironment(
        name="imported",
        schema_window=(0, final_schema),
        artifacts=[
            DbEnvironmentArtifact("active", artifact.name, (final_schema, final_schema)),
            DbEnvironmentArtifact("supported", artifact.name, (final_schema, final_schema)),
        ],
    )
    system = DbSystem(
        name=name,
        database=DbDatabase(
            name="imported",
            initial_schema=0,
            tables=tables,
        ),
        migrations=migrations,
        artifacts=[artifact],
        environments=[environment],
        check=DbCheck(),
    )
    return DbImportResult(
        system=system,
        source=dbsystem_to_source(system),
        source_format="sql-ddl-minimal.v0",
        warnings=warnings,
    )


def import_prisma(schema, name="ImportedDb"):
    warnings: List[dict] = []
    models = _prisma_models(schema)
    model_names = {model_name for model_name, _ in models}
    columns: Dict[ColumnKey, DbColumn] = {}

    for model_name, body in models:
        for raw in body.splitlines():
            line = raw.strip()
            if not line or line.startswith("//"):
                continue
            if line.startswith("@@"):
                warnings.append(_unsupported_prisma(line, "model-level attributes are not imported"))
                continue
            parts = line.split()
            if len(parts) < 2:
                warnings.append(_unsupported_prisma(line, "Prisma importer expects '<field> <type>' field lines"))
                continue
            field, raw_type = parts[0], parts[1]
            type_name = raw_type.rstrip("?")
            if type_name.endswith("[]"):
                warnings.append(_unsupported_prisma(line, "list/relation fields are not imported"))
                continue
            if type_name in model_names or "@relation" in line:
                warnings.append(_unsupported_prisma(line, "relation fields are not imported"))
                continue
            db_type = _prisma_db_type(type_name)
            if db_type is None:
                warnings.append(_unsupported_prisma(line, f"unsupported Prisma scalar type '{type_name}'"))
                continue
            not_null = not raw_type.endswith("?")
            columns[(model_name, field)] = DbColumn(
                table=model_name,
                name=field,
                db_type=db_type,
                present=True,
                backfilled=True,
                not_null=not_null,
            )

    if not columns:
        columns[("imported", "id")] = DbColumn(
            table="imported",
            name="id",
            db_type="Int",
            present=True,
            backfilled=True,
            not_null=True,
        )
        warnings.append({
            "kind": "empty_import",
            "message": "no supported Prisma model fields were found; emitted a placeholder schema",
        })

    tables = []
    for table in sorted({key[0] for key in columns}):
        tables.append(DbTable(
            name=table,
            columns=[columns[key] for key in sorted(columns) if key[0] == table],
        ))

    final_columns = [key for key in sorted(columns)]
    artifact = DbArtifact(
        name="imported_artifact",
        reads=final_columns,
        writes=final_columns,
    )
    environment = DbEnvironment(
        name="imported",
        schema_window=(0, 0),
        artifacts=[
            DbEnvironmentArtifact("active", artifact.name, (0, 0)),
            DbEnvironmentArtifact("supported", artifact.name, (0, 0)),
        ],
    )
    system = DbSystem(
        name=name,
        database=DbDatabase(
            name="imported",
            initial_schema=0,
            tables=tables,
        ),
        artifacts=[artifact],
        environments=[environment],
        check=DbCheck(),
    )
    return DbImportResult(
        system=system,
        source=dbsystem_to_source(system),
        source_format="prisma-schema-minimal.v0",
        warnings=warnings,
    )


def _select_source_format(path, source_format):
    if source_format != "auto":
        return source_format
    suffix = Path(path).suffix.lower()
    if suffix == ".prisma":
        return "prisma"
    return "sql"


def _prisma_models(schema):
    cleaned = re.sub(r"/\*.*?\*/", "", schema, flags=re.S)
    return [
        (match.group(1), match.group(2))
        for match in re.finditer(r"\bmodel\s+(\w+)\s*\{(.*?)\}", cleaned, re.S)
    ]


def _prisma_db_type(type_name):
    return {
        "Int": "Int",
        "BigInt": "Int",
        "String": "Text",
        "Boolean": "Bool",
        "DateTime": "Text",
        "Float": "Value",
        "Decimal": "Value",
        "Json": "Value",
        "Bytes": "Value",
    }.get(type_name)


def _unsupported_prisma(statement, message):
    return {
        "kind": "unsupported_prisma",
        "statement": statement,
        "message": message,
    }

def _statements(sql):
    cleaned = re.sub(r"--[^\n]*", "", sql)
    return [stmt.strip() for stmt in cleaned.split(";") if stmt.strip()]


def _parse_statement(stmt):
    create = re.match(r"create\s+table\s+(\w+)\s*\((.*)\)\s*$", stmt, re.I | re.S)
    if create:
        table = create.group(1)
        defs = []
        for raw_col in _split_columns(create.group(2)):
            parts = raw_col.strip().split()
            if len(parts) < 2:
                continue
            column = parts[0].strip('"')
            db_type = parts[1]
            lowered = " ".join(parts[2:]).lower()
            defs.append((column, db_type, "not null" in lowered or "primary key" in lowered))
        return ("create_table", table, defs)

    add = re.match(r"alter\s+table\s+(\w+)\s+add\s+column\s+(\w+)\s+(\w+)(.*)$", stmt, re.I | re.S)
    if add:
        return ("add_column", add.group(1), add.group(2), add.group(3), "not null" in add.group(4).lower())

    drop = re.match(r"alter\s+table\s+(\w+)\s+drop\s+column\s+(\w+)\s*$", stmt, re.I)
    if drop:
        return ("drop_column", drop.group(1), drop.group(2))

    rename = re.match(r"alter\s+table\s+(\w+)\s+rename\s+column\s+(\w+)\s+to\s+(\w+)\s*$", stmt, re.I)
    if rename:
        return ("rename_column", rename.group(1), rename.group(2), rename.group(3))

    backfill = re.match(r"update\s+(\w+)\s+set\s+(\w+)\s*=", stmt, re.I | re.S)
    if backfill:
        return ("backfill", backfill.group(1), backfill.group(2))

    return None


def _split_columns(body):
    out = []
    depth = 0
    start = 0
    for i, ch in enumerate(body):
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif ch == "," and depth == 0:
            out.append(body[start:i])
            start = i + 1
    out.append(body[start:])
    return out


def _migration_from_sql(parsed, from_schema, to_schema, columns):
    kind = parsed[0]
    name = f"m{to_schema}_{kind}"
    if kind == "add_column":
        _, table, column, db_type, not_null = parsed
        columns.setdefault((table, column), DbColumn(
            table=table,
            name=column,
            db_type=db_type,
            present=False,
            backfilled=False,
            not_null=False,
        ))
        op = DbMigrationOp("add", (table, column), "not_null" if not_null else "nullable")
    elif kind == "drop_column":
        _, table, column = parsed
        op = DbMigrationOp("drop", (table, column), annotations=("irreversible",))
    elif kind == "rename_column":
        _, table, old, new = parsed
        old_column = columns.get((table, old), DbColumn(table=table, name=old))
        columns.setdefault((table, old), old_column)
        columns.setdefault((table, new), DbColumn(
            table=table,
            name=new,
            db_type=old_column.db_type,
            present=False,
            backfilled=False,
            not_null=False,
        ))
        op = DbMigrationOp("rename", (table, old), columns=((table, new),))
    elif kind == "backfill":
        _, table, column = parsed
        op = DbMigrationOp("backfill", (table, column))
    else:
        raise AssertionError(kind)
    return DbMigration(name, from_schema, to_schema, [op])


def _final_existing_columns(columns, migrations):
    state = {key: column.present for key, column in columns.items()}
    for migration in migrations:
        for op in migration.ops:
            if op.op == "add":
                state[op.column] = True
            elif op.op == "drop":
                state[op.column] = False
            elif op.op == "rename":
                state[op.column] = False
                state[op.columns[0]] = True
    return [key for key, present in sorted(state.items()) if present]


def dbsystem_to_source(system: DbSystem):
    lines = [f"dbsystem {system.name} {{", f"  database {system.database.name} {{"]
    lines.append(f"    schema {system.database.initial_schema}")
    for table in system.database.tables:
        lines.append(f"    table {table.name} {{")
        for column in table.columns:
            attrs = ["present" if column.present else "absent"]
            if column.backfilled:
                attrs.append("backfilled")
            attrs.append("not_null" if column.not_null else "nullable")
            lines.append(f"      column {column.name}: {column.db_type} {' '.join(attrs)};")
        lines.append("    }")
    lines.append("  }")
    lines.append("")
    for migration in system.migrations:
        annotations = (" " + " ".join(migration.annotations)) if migration.annotations else ""
        lines.append(f"  migration {migration.name} from {migration.from_schema} to {migration.to_schema}{annotations} {{")
        for op in migration.ops:
            op_annotations = (" " + " ".join(op.annotations)) if op.annotations else ""
            if op.op == "add":
                lines.append(f"    add {column_label(op.column)} {op.nullability or 'nullable'};")
            elif op.op == "drop":
                lines.append(f"    drop {column_label(op.column)}{op_annotations};")
            elif op.op == "backfill":
                lines.append(f"    backfill {column_label(op.column)};")
            elif op.op == "rename":
                lines.append(f"    rename {column_label(op.column)} to {column_label(op.columns[0])}{op_annotations};")
        lines.append("  }")
        lines.append("")
    for artifact in system.artifacts:
        lines.append(f"  artifact {artifact.name} {{")
        if artifact.reads:
            lines.append(f"    reads {', '.join(column_label(c) for c in artifact.reads)};")
        if artifact.writes:
            lines.append(f"    writes {', '.join(column_label(c) for c in artifact.writes)};")
        lines.append("  }")
        lines.append("")
    for env in system.environments:
        lines.append(f"  environment {env.name} {{")
        lines.append(f"    schema {env.schema_window[0]}..{env.schema_window[1]};")
        for flag in env.flags:
            default = f" default {flag.default}" if flag.default else ""
            lines.append(f"    flag {flag.name} {{ {', '.join(flag.variants)} }}{default};")
        for entry in env.artifacts:
            conditions = "".join(
                f" when flag {condition.flag}={condition.variant}"
                for condition in entry.flag_conditions
            )
            if entry.schema_window:
                lines.append(
                    f"    {entry.role} {entry.artifact} "
                    f"when schema {entry.schema_window[0]}..{entry.schema_window[1]}{conditions};"
                )
            else:
                lines.append(f"    {entry.role} {entry.artifact}{conditions};")
        lines.append("  }")
        lines.append("")
    lines.append("  check compatibility {")
    lines.append("    rule all_active_reads_exist;")
    lines.append("    rule all_active_writes_exist;")
    lines.append("    rule removed_only_after_unused;")
    lines.append("    rule not_null_after_backfill;")
    lines.append("  }")
    lines.append("}")
    return "\n".join(lines) + "\n"


def column_label(column: ColumnKey) -> str:
    return f"{column[0]}.{column[1]}"
