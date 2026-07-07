# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Typed IR for the fsl-db multi-environment compatibility dialect."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, List, Optional, Sequence, Tuple


ColumnKey = Tuple[str, str]
SchemaWindow = Tuple[int, int]


@dataclass(frozen=True)
class DbColumn:
    table: str
    name: str
    db_type: str = "Value"
    present: bool = True
    backfilled: bool = True
    not_null: bool = False
    loc: Optional[dict] = None

    @property
    def key(self) -> ColumnKey:
        return (self.table, self.name)

    @property
    def label(self) -> str:
        return f"{self.table}.{self.name}"


@dataclass(frozen=True)
class DbTable:
    name: str
    columns: List[DbColumn] = field(default_factory=list)
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DbDatabase:
    name: str
    initial_schema: int
    tables: List[DbTable] = field(default_factory=list)
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DbMigrationOp:
    op: str
    column: ColumnKey
    nullability: Optional[str] = None
    loc: Optional[dict] = None

    @property
    def column_label(self) -> str:
        return f"{self.column[0]}.{self.column[1]}"


@dataclass(frozen=True)
class DbMigration:
    name: str
    from_schema: int
    to_schema: int
    ops: List[DbMigrationOp] = field(default_factory=list)
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DbArtifact:
    name: str
    reads: List[ColumnKey] = field(default_factory=list)
    writes: List[ColumnKey] = field(default_factory=list)
    calls: List[ColumnKey] = field(default_factory=list)
    accepts: List[ColumnKey] = field(default_factory=list)
    expects: List[ColumnKey] = field(default_factory=list)
    emits_offline: List[ColumnKey] = field(default_factory=list)
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DbEnvironmentArtifact:
    role: str
    artifact: str
    schema_window: Optional[SchemaWindow] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DbEnvironment:
    name: str
    schema_window: SchemaWindow
    artifacts: List[DbEnvironmentArtifact] = field(default_factory=list)
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DbCheck:
    rules: List[str] = field(default_factory=list)
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DbSystem:
    name: str
    database: DbDatabase
    migrations: List[DbMigration] = field(default_factory=list)
    artifacts: List[DbArtifact] = field(default_factory=list)
    environments: List[DbEnvironment] = field(default_factory=list)
    check: DbCheck = field(default_factory=DbCheck)
    loc: Optional[dict] = None

    def column_map(self) -> Dict[ColumnKey, DbColumn]:
        return {column.key: column for table in self.database.tables for column in table.columns}

    def artifact_map(self) -> Dict[str, DbArtifact]:
        return {artifact.name: artifact for artifact in self.artifacts}

    def rule_set(self) -> set:
        return set(self.check.rules)


def column_label(column: ColumnKey) -> str:
    return f"{column[0]}.{column[1]}"


def schema_values(window: SchemaWindow) -> Sequence[int]:
    lo, hi = window
    return range(lo, hi + 1)
