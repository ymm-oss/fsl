# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Parser for the fsl-db MVP dialect."""
from __future__ import annotations

from collections import defaultdict

from lark import Lark, Transformer, v_args
from lark.exceptions import UnexpectedInput

from .db_ir import (
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
from .model import FslError


DB_GRAMMAR = r"""
start: dbsystem

dbsystem: "dbsystem" NAME "{" db_item* "}"
?db_item: database_def | migration_def | artifact_def | environment_def | check_def

database_def: "database" NAME "{" database_item* "}"
?database_item: db_schema | table_def
db_schema: "schema" INT ";"?

table_def: "table" NAME "{" column_def* "}"
column_def: "column" NAME column_type? column_attr* ";"?
column_type: ":" NAME
?column_attr: "present" -> col_present
            | "absent" -> col_absent
            | "backfilled" -> col_backfilled
            | "not_null" -> col_not_null
            | "nullable" -> col_nullable

migration_def: "migration" NAME "from" INT "to" INT "{" migration_op* "}"
?migration_op: add_op | drop_op | backfill_op | set_not_null_op
add_op: "add" col_ref add_nullability? ";"?
add_nullability: "nullable" -> op_nullable
               | "not_null" -> op_not_null
drop_op: "drop" col_ref ";"?
backfill_op: "backfill" col_ref ";"?
set_not_null_op: ("set_not_null" | "not_null") col_ref ";"?

artifact_def: "artifact" NAME "{" artifact_item* "}"
artifact_item: artifact_capability col_ref_list ";"?
artifact_capability: "reads" -> cap_reads
                   | "writes" -> cap_writes
                   | "calls" -> cap_calls
                   | "accepts" -> cap_accepts
                   | "expects" -> cap_expects
                   | "emits_offline" -> cap_emits_offline
col_ref_list: col_ref ("," col_ref)*
col_ref: NAME "." NAME

environment_def: "environment" NAME "{" environment_item* "}"
?environment_item: env_schema | env_artifact
env_schema: "schema" INT ".." INT ";"?
env_artifact: env_role NAME env_window? ";"?
env_role: "active" -> env_active
        | "supported" -> env_supported
        | "may_exist" -> env_may_exist
env_window: "when" "schema" INT ".." INT

check_def: "check" "compatibility" "{" check_item* "}"
check_item: "rule" NAME ";"?

NAME: /[a-zA-Z_][a-zA-Z_0-9]*/
INT: /[0-9]+/
COMMENT: /\/\/[^\n]*/
%import common.WS
%ignore WS
%ignore COMMENT
"""


def _loc(meta):
    if meta is None:
        return None
    return {"line": meta.line, "column": meta.column}


@v_args(inline=True, meta=True)
class DbAst(Transformer):
    def NAME(self, *args):
        return str(args[-1])

    def INT(self, *args):
        return int(str(args[-1]))

    def col_present(self, meta):
        return ("present", True)

    def col_absent(self, meta):
        return ("present", False)

    def col_backfilled(self, meta):
        return ("backfilled", True)

    def col_not_null(self, meta):
        return ("not_null", True)

    def col_nullable(self, meta):
        return ("not_null", False)

    def column_type(self, meta, name):
        return ("db_type", name)

    def column_def(self, meta, name, *parts):
        attrs = {
            "db_type": "Value",
            "present": True,
            "backfilled": True,
            "not_null": False,
        }
        explicit = set()
        for part in parts:
            key, value = part
            if key == "db_type":
                attrs["db_type"] = value
                continue
            explicit.add(key)
            attrs[key] = value
        if "present" in explicit and not attrs["present"]:
            attrs.setdefault("backfilled", False)
            attrs["backfilled"] = False
            attrs["not_null"] = False
        return ("column", name, attrs, _loc(meta))

    def db_schema(self, meta, version):
        return ("schema", version, _loc(meta))

    def table_def(self, meta, name, *columns):
        out = []
        for column in columns:
            _, cname, attrs, loc = column
            out.append(DbColumn(
                table=name,
                name=cname,
                db_type=attrs["db_type"],
                present=attrs["present"],
                backfilled=attrs["backfilled"],
                not_null=attrs["not_null"],
                loc=loc,
            ))
        return DbTable(name=name, columns=out, loc=_loc(meta))

    def database_def(self, meta, name, *items):
        initial_schema = None
        tables = []
        for item in items:
            if isinstance(item, tuple) and item[0] == "schema":
                if initial_schema is not None:
                    raise FslError("database schema may be declared at most once", loc=item[2])
                initial_schema = item[1]
            else:
                tables.append(item)
        if initial_schema is None:
            raise FslError("database requires `schema <version>`", loc=_loc(meta))
        return DbDatabase(name=name, initial_schema=initial_schema, tables=tables, loc=_loc(meta))

    def col_ref(self, meta, table, column):
        return (table, column)

    def op_nullable(self, meta):
        return "nullable"

    def op_not_null(self, meta):
        return "not_null"

    def add_op(self, meta, column, nullability=None):
        return DbMigrationOp("add", column, nullability or "nullable", _loc(meta))

    def drop_op(self, meta, column):
        return DbMigrationOp("drop", column, None, _loc(meta))

    def backfill_op(self, meta, column):
        return DbMigrationOp("backfill", column, None, _loc(meta))

    def set_not_null_op(self, meta, column):
        return DbMigrationOp("set_not_null", column, None, _loc(meta))

    def migration_def(self, meta, name, from_schema, to_schema, *ops):
        return DbMigration(name, from_schema, to_schema, list(ops), _loc(meta))

    def cap_reads(self, meta):
        return "reads"

    def cap_writes(self, meta):
        return "writes"

    def cap_calls(self, meta):
        return "calls"

    def cap_accepts(self, meta):
        return "accepts"

    def cap_expects(self, meta):
        return "expects"

    def cap_emits_offline(self, meta):
        return "emits_offline"

    def col_ref_list(self, meta, *refs):
        return list(refs)

    def artifact_item(self, meta, capability, refs):
        return capability, refs

    def artifact_def(self, meta, name, *items):
        caps = defaultdict(list)
        for capability, refs in items:
            caps[capability].extend(refs)
        return DbArtifact(
            name=name,
            reads=caps["reads"],
            writes=caps["writes"],
            calls=caps["calls"],
            accepts=caps["accepts"],
            expects=caps["expects"],
            emits_offline=caps["emits_offline"],
            loc=_loc(meta),
        )

    def env_active(self, meta):
        return "active"

    def env_supported(self, meta):
        return "supported"

    def env_may_exist(self, meta):
        return "may_exist"

    def env_window(self, meta, lo, hi):
        return (lo, hi)

    def env_schema(self, meta, lo, hi):
        return ("schema", (lo, hi), _loc(meta))

    def env_artifact(self, meta, role, artifact, window=None):
        return DbEnvironmentArtifact(role, artifact, window, _loc(meta))

    def environment_def(self, meta, name, *items):
        schema_window = None
        artifacts = []
        for item in items:
            if isinstance(item, tuple) and item[0] == "schema":
                if schema_window is not None:
                    raise FslError("environment schema may be declared at most once", loc=item[2])
                schema_window = item[1]
            else:
                artifacts.append(item)
        if schema_window is None:
            raise FslError("environment requires `schema <lo>..<hi>`", loc=_loc(meta))
        return DbEnvironment(name, schema_window, artifacts, _loc(meta))

    def check_item(self, meta, name):
        return name

    def check_def(self, meta, *items):
        return DbCheck(list(items), _loc(meta))

    def dbsystem(self, meta, name, *items):
        database = None
        migrations = []
        artifacts = []
        environments = []
        check = DbCheck()
        for item in items:
            if isinstance(item, DbDatabase):
                if database is not None:
                    raise FslError("dbsystem may declare only one database", loc=item.loc)
                database = item
            elif isinstance(item, DbMigration):
                migrations.append(item)
            elif isinstance(item, DbArtifact):
                artifacts.append(item)
            elif isinstance(item, DbEnvironment):
                environments.append(item)
            elif isinstance(item, DbCheck):
                check = item
        if database is None:
            raise FslError("dbsystem requires a database block", loc=_loc(meta))
        return DbSystem(name, database, migrations, artifacts, environments, check, _loc(meta))

    def start(self, meta, child):
        return child


DB_PARSER = Lark(
    DB_GRAMMAR,
    parser="lalr",
    maybe_placeholders=False,
    propagate_positions=True,
)


def is_dbsystem_source(src):
    return src.lstrip().startswith("dbsystem")


def parse_dbsystem(src):
    try:
        tree = DB_PARSER.parse(src)
    except UnexpectedInput as e:
        e.source = src
        raise
    return DbAst().transform(tree)
