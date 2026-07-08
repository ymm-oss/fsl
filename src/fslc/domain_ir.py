# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Typed IR for the fsl-domain / fsl-effect MVP dialect."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple


@dataclass(frozen=True)
class DomainField:
    name: str
    type_name: str
    default: Optional[str] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainType:
    name: str
    kind: str
    members: Tuple[str, ...] = ()
    lo: Optional[str] = None
    hi: Optional[str] = None
    fields: Tuple[DomainField, ...] = ()
    invariants: Tuple["DomainInvariant", ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainCommand:
    name: str
    inputs: Tuple[DomainField, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainEvent:
    name: str
    fields: Tuple[DomainField, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainError:
    name: str
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainReject:
    error: str
    condition: str
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainDecide:
    command: str
    requires: Tuple[str, ...] = ()
    rejects: Tuple[DomainReject, ...] = ()
    emits: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainAssignment:
    target: str
    expr: str
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainEvolve:
    event: str
    requires: Tuple[str, ...] = ()
    assignments: Tuple[DomainAssignment, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainInvariant:
    name: str
    expr: str
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainProjection:
    name: str
    source: str
    fields: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainStalePolicy:
    event: str
    condition: str
    emits: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainAggregate:
    name: str
    id_type: Optional[str] = None
    state: Tuple[DomainField, ...] = ()
    commands: Tuple[DomainCommand, ...] = ()
    events: Tuple[DomainEvent, ...] = ()
    errors: Tuple[DomainError, ...] = ()
    decides: Tuple[DomainDecide, ...] = ()
    evolves: Tuple[DomainEvolve, ...] = ()
    invariants: Tuple[DomainInvariant, ...] = ()
    stale_policies: Tuple[DomainStalePolicy, ...] = ()
    loc: Optional[dict] = None

    def command_map(self) -> Dict[str, DomainCommand]:
        return {command.name: command for command in self.commands}

    def event_map(self) -> Dict[str, DomainEvent]:
        return {event.name: event for event in self.events}

    def decide_map(self) -> Dict[str, DomainDecide]:
        return {decide.command: decide for decide in self.decides}

    def evolve_map(self) -> Dict[str, DomainEvolve]:
        return {evolve.event: evolve for evolve in self.evolves}

    def state_map(self) -> Dict[str, DomainField]:
        return {field.name: field for field in self.state}


@dataclass(frozen=True)
class DomainRetry:
    max_attempts: Optional[int] = None
    backoff: Optional[str] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainEffect:
    name: str
    async_effect: bool = False
    reliable: bool = False
    irreversible: bool = False
    idempotency_key: Optional[str] = None
    correlation_id: Optional[str] = None
    handles: Optional[str] = None
    outcomes: Tuple[str, ...] = ()
    request_event: Optional[str] = None
    success_event: Optional[str] = None
    failure_event: Optional[str] = None
    timeout_event: Optional[str] = None
    retry: DomainRetry = field(default_factory=DomainRetry)
    timeout_after: Optional[str] = None
    compensation_events: Tuple[str, ...] = ()
    outbox: Optional[str] = None
    inbox: Optional[str] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainAwait:
    name: str
    mode: str
    events: Tuple[str, ...]
    branches: Tuple[Tuple[str, str], ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainSagaStep:
    name: str
    async_step: bool = False
    requires: Tuple[str, ...] = ()
    emits: Tuple[str, ...] = ()
    awaits_mode: str = "one_of"
    awaits: Tuple[str, ...] = ()
    timeout_after: Optional[str] = None
    timeout_event: Optional[str] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainSagaCompensation:
    trigger_event: str
    after_event: str
    emits: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainSaga:
    name: str
    starts_on: Optional[str] = None
    steps: Tuple[DomainSagaStep, ...] = ()
    compensations: Tuple[DomainSagaCompensation, ...] = ()
    invariants: Tuple[DomainInvariant, ...] = ()
    outboxes: Tuple[str, ...] = ()
    inboxes: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class DomainSpec:
    name: str
    implementation_profile: Optional[str] = None
    types: Tuple[DomainType, ...] = ()
    aggregates: Tuple[DomainAggregate, ...] = ()
    effects: Tuple[DomainEffect, ...] = ()
    awaits: Tuple[DomainAwait, ...] = ()
    sagas: Tuple[DomainSaga, ...] = ()
    projections: Tuple[DomainProjection, ...] = ()
    loc: Optional[dict] = None

    def type_map(self) -> Dict[str, DomainType]:
        return {ty.name: ty for ty in self.types}

    def aggregate_map(self) -> Dict[str, DomainAggregate]:
        return {agg.name: agg for agg in self.aggregates}

    def event_owner(self) -> Dict[str, DomainAggregate]:
        out = {}
        for aggregate in self.aggregates:
            for event in aggregate.events:
                out[event.name] = aggregate
        return out

    def event_map(self) -> Dict[str, DomainEvent]:
        out = {}
        for aggregate in self.aggregates:
            out.update(aggregate.event_map())
        return out
