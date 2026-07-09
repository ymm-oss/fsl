# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Parser for the fsl-domain / fsl-effect v0 dialect."""
from __future__ import annotations

from lark import Lark, Transformer, v_args

from .domain_ir import (
    DomainAggregate,
    DomainAssignment,
    DomainAwait,
    DomainCommand,
    DomainDecide,
    DomainEffect,
    DomainError,
    DomainEvent,
    DomainEvolve,
    DomainField,
    DomainInvariant,
    DomainProjection,
    DomainReject,
    DomainRetry,
    DomainSaga,
    DomainSagaCompensation,
    DomainSagaStep,
    DomainSpec,
    DomainStalePolicy,
    DomainType,
)
from .model import FslError


DOMAIN_GRAMMAR = r"""
start: domain_def

domain_def: "domain" NAME "{" domain_item* "}"
?domain_item: implementation_profile_def | type_def | value_object_def
            | aggregate_def | effect_def | await_def | saga_def | projection_def

implementation_profile_def: "implementation_profile" NAME profile_block? ";"?
profile_block: "{" profile_item* "}"
profile_item: NAME NAME ";"?

type_def: "type" NAME "=" TYPE_BODY ";"?

value_object_def: "value_object" NAME "{" value_object_item* "}"
?value_object_item: field_def | invariant_def

aggregate_def: "aggregate" NAME "{" aggregate_item* "}"
?aggregate_item: id_def | state_def | command_def | event_def | error_def
               | decide_def | evolve_def | invariant_def | projection_def | on_stale_def
id_def: "id" NAME ";"?

state_def: "state" "{" state_field* "}"
state_field: NAME ":" type_ref field_default? ";"?
field_default: "=" RAW_EXPR

field_def: input_field | bare_field
input_field: "input" NAME ":" type_ref ";"?
bare_field: NAME ":" type_ref ";"?

command_def: "command" NAME "{" field_def* "}"
event_def: "event" NAME "{" field_def* "}"
error_def: "error" NAME ";"?

decide_def: "decide" NAME "{" decide_item* "}"
?decide_item: requires_def | rejects_def | emits_def
requires_def: "requires" RAW_EXPR ";"?
rejects_def: "rejects" NAME "when" RAW_EXPR ";"?
emits_def: "emits" emit_names ";"?
?emit_names: "one_of" bracket_name_list -> one_of_names
           | bracket_name_list
           | name_list

evolve_def: "evolve" NAME "{" evolve_item* "}"
?evolve_item: requires_def | assign_def
assign_def: lvalue "=" RAW_EXPR ";"?
lvalue: NAME ("[" RAW_EXPR "]")? ("." NAME)?

invariant_def: "invariant" NAME "{" RAW_EXPR "}"

projection_def: "projection" NAME "{" projection_item* "}"
?projection_item: projection_from | projection_fields
projection_from: "from" NAME ";"?
projection_fields: "fields" bracket_name_list ";"?

on_stale_def: "on_stale" NAME "when" RAW_EXPR "{" stale_item* "}"
?stale_item: emits_def

effect_def: "effect" NAME "{" effect_item* "}"
?effect_item: async_def | irreversible_def | idempotency_def | correlation_def
            | reliable_def | field_def | handles_def | emits_def | request_event_def
            | success_event_def | failure_event_def | timeout_event_def
            | retry_def | timeout_def | compensation_def | outbox_def | inbox_def
async_def: "async" ";"?
reliable_def: "reliable" BOOL? ";"?
irreversible_def: "irreversible" BOOL? ";"?
idempotency_def: "idempotency_key" REF ";"?
correlation_def: "correlation_id" REF ";"?
handles_def: "handles" NAME ";"?
request_event_def: "request_event" NAME ";"?
success_event_def: "success_event" NAME ";"?
failure_event_def: "failure_event" NAME ";"?
timeout_event_def: "timeout_event" NAME ";"?
retry_def: "retry" "{" retry_item* "}"
?retry_item: max_attempts_def | backoff_def
max_attempts_def: "max_attempts" INT ";"?
backoff_def: "backoff" NAME ";"?
timeout_def: "timeout" "after" TIME_VALUE "emits" NAME ";"?
compensation_def: "compensation" "{" compensation_item* "}"
?compensation_item: emits_def

await_def: "await" NAME "{" await_item* "}"
?await_item: waits_for_def | await_on_def
waits_for_def: "waits_for" await_mode bracket_name_list ";"?
await_mode: "one_of" -> await_one_of
          | "all" -> await_all
          | "any" -> await_any
await_on_def: "on" NAME ARROW NAME ";"?

saga_def: "saga" NAME "{" saga_item* "}"
?saga_item: starts_on_def | saga_step_def | saga_compensation_block
          | invariant_def | outbox_def | inbox_def
starts_on_def: "starts_on" NAME ";"?
saga_step_def: "step" NAME "{" saga_step_item* "}"
?saga_step_item: async_def | requires_def | emits_def | awaits_def | timeout_def
awaits_def: "awaits" await_mode bracket_name_list ";"?
saga_compensation_block: "compensation" "{" saga_compensation_item* "}"
saga_compensation_item: "when" NAME "after" NAME "{" stale_item* "}"
outbox_def: "outbox" NAME ";"?
inbox_def: "inbox" NAME ";"?

name_list: NAME ("," NAME)* ","?
bracket_name_list: "[" name_list "]"

?type_ref: "Int" -> type_int
         | "Bool" -> type_bool
         | NAME "<" type_ref "," type_ref ">" -> type_generic2
         | NAME "<" type_ref ">" -> type_generic1
         | NAME -> type_name

BOOL: "true" | "false"
ARROW: "->"
REF: /[A-Za-z_][A-Za-z_0-9]*(\.[A-Za-z_][A-Za-z_0-9]*)*/
TIME_VALUE: /[0-9]+[A-Za-z]+|[0-9]+/
TYPE_BODY: /[^\n;{}]+/
RAW_EXPR: /[^\n{};]+/
NAME: /[a-zA-Z_][a-zA-Z_0-9]*/
INT: /[0-9]+/
COMMENT: /\/\/[^\n]*/
%import common.WS
%ignore WS
%ignore COMMENT
"""


PARSER = Lark(
    DOMAIN_GRAMMAR,
    parser="lalr",
    maybe_placeholders=False,
    propagate_positions=True,
)


def _loc(meta):
    if meta is None:
        return None
    return {"line": meta.line, "column": meta.column}


def _clean(text):
    return " ".join(str(text).strip().split())


def is_domain_source(src: str) -> bool:
    stripped = src.lstrip()
    return stripped.startswith("domain ")


@v_args(inline=True, meta=True)
class DomainAst(Transformer):
    def NAME(self, *args):
        return str(args[-1])

    def INT(self, *args):
        return int(str(args[-1]))

    def BOOL(self, *args):
        return str(args[-1]) == "true"

    def REF(self, *args):
        return str(args[-1])

    def RAW_EXPR(self, *args):
        return _clean(args[-1])

    def TYPE_BODY(self, *args):
        return _clean(args[-1])

    def TIME_VALUE(self, *args):
        return str(args[-1])

    def type_int(self, meta):
        return "Int"

    def type_bool(self, meta):
        return "Bool"

    def type_name(self, meta, name):
        return name

    def type_generic1(self, meta, name, inner):
        return f"{name}<{inner}>"

    def type_generic2(self, meta, name, left, right):
        return f"{name}<{left}, {right}>"

    def field_default(self, meta, expr):
        return expr

    def state_field(self, meta, name, type_name, default=None):
        return DomainField(name=name, type_name=type_name, default=default, loc=_loc(meta))

    def input_field(self, meta, name, type_name):
        return DomainField(name=name, type_name=type_name, loc=_loc(meta))

    def bare_field(self, meta, name, type_name):
        return DomainField(name=name, type_name=type_name, loc=_loc(meta))

    def field_def(self, meta, field):
        return field

    def name_list(self, meta, *names):
        return tuple(names)

    def bracket_name_list(self, meta, names):
        return tuple(names)

    def one_of_names(self, meta, names):
        return tuple(names)

    def emit_names(self, meta, names):
        return tuple(names)

    def implementation_profile_def(self, meta, name, _profile_block=None):
        return ("implementation_profile", name, _loc(meta))

    def profile_item(self, meta, *_parts):
        return None

    def profile_block(self, meta, *_items):
        return None

    def type_def(self, meta, name, body):
        body = _clean(body)
        if "|" in body:
            members = tuple(_clean(part) for part in body.split("|") if _clean(part))
            return DomainType(name=name, kind="enum", members=members, loc=_loc(meta))
        if ".." in body:
            lo, hi = body.split("..", 1)
            return DomainType(name=name, kind="range", lo=_clean(lo), hi=_clean(hi), loc=_loc(meta))
        raise FslError(
            f"domain type '{name}' must be an enum union (A | B) or a bounded range (lo..hi)",
            loc=_loc(meta),
        )

    def value_object_def(self, meta, name, *items):
        fields = []
        invariants = []
        for item in items:
            if isinstance(item, DomainField):
                fields.append(item)
            elif isinstance(item, DomainInvariant):
                invariants.append(item)
        return DomainType(
            name=name,
            kind="value_object",
            fields=tuple(fields),
            invariants=tuple(invariants),
            loc=_loc(meta),
        )

    def id_def(self, meta, type_name):
        return ("id", type_name, _loc(meta))

    def state_def(self, meta, *fields):
        return ("state", tuple(fields), _loc(meta))

    def command_def(self, meta, name, *fields):
        return DomainCommand(name=name, inputs=tuple(fields), loc=_loc(meta))

    def event_def(self, meta, name, *fields):
        return DomainEvent(name=name, fields=tuple(fields), loc=_loc(meta))

    def error_def(self, meta, name):
        return DomainError(name=name, loc=_loc(meta))

    def requires_def(self, meta, expr):
        return ("requires", expr, _loc(meta))

    def rejects_def(self, meta, error, condition):
        return DomainReject(error=error, condition=condition, loc=_loc(meta))

    def emits_def(self, meta, names):
        return ("emits", tuple(names), _loc(meta))

    def decide_def(self, meta, command, *items):
        requires = []
        rejects = []
        emits = []
        for item in items:
            if isinstance(item, DomainReject):
                rejects.append(item)
            elif item[0] == "requires":
                requires.append(item[1])
            elif item[0] == "emits":
                emits.extend(item[1])
        return DomainDecide(
            command=command,
            requires=tuple(requires),
            rejects=tuple(rejects),
            emits=tuple(emits),
            loc=_loc(meta),
        )

    def lvalue(self, meta, name, index=None, field=None):
        out = name
        if index is not None:
            out += f"[{index}]"
        if field is not None:
            out += f".{field}"
        return out

    def assign_def(self, meta, target, expr):
        return DomainAssignment(target=target, expr=expr, loc=_loc(meta))

    def evolve_def(self, meta, event, *items):
        requires = []
        assignments = []
        for item in items:
            if isinstance(item, DomainAssignment):
                assignments.append(item)
            elif item[0] == "requires":
                requires.append(item[1])
        return DomainEvolve(
            event=event,
            requires=tuple(requires),
            assignments=tuple(assignments),
            loc=_loc(meta),
        )

    def invariant_def(self, meta, name, expr):
        return DomainInvariant(name=name, expr=expr, loc=_loc(meta))

    def projection_from(self, meta, name):
        return ("from", name, _loc(meta))

    def projection_fields(self, meta, names):
        return ("fields", tuple(names), _loc(meta))

    def projection_def(self, meta, name, *items):
        source = None
        fields = []
        for item in items:
            if item[0] == "from":
                source = item[1]
            elif item[0] == "fields":
                fields.extend(item[1])
        return DomainProjection(name=name, source=source or "", fields=tuple(fields), loc=_loc(meta))

    def on_stale_def(self, meta, event, condition, *items):
        emits = []
        for item in items:
            if item[0] == "emits":
                emits.extend(item[1])
        return DomainStalePolicy(event=event, condition=condition, emits=tuple(emits), loc=_loc(meta))

    def aggregate_def(self, meta, name, *items):
        id_type = None
        state = []
        commands = []
        events = []
        errors = []
        decides = []
        evolves = []
        invariants = []
        projections = []
        stale_policies = []
        for item in items:
            if isinstance(item, tuple) and item[0] == "id":
                id_type = item[1]
            elif isinstance(item, tuple) and item[0] == "state":
                state.extend(item[1])
            elif isinstance(item, DomainCommand):
                commands.append(item)
            elif isinstance(item, DomainEvent):
                events.append(item)
            elif isinstance(item, DomainError):
                errors.append(item)
            elif isinstance(item, DomainDecide):
                decides.append(item)
            elif isinstance(item, DomainEvolve):
                evolves.append(item)
            elif isinstance(item, DomainInvariant):
                invariants.append(item)
            elif isinstance(item, DomainProjection):
                projections.append(item)
            elif isinstance(item, DomainStalePolicy):
                stale_policies.append(item)
        aggregate = DomainAggregate(
            name=name,
            id_type=id_type,
            state=tuple(state),
            commands=tuple(commands),
            events=tuple(events),
            errors=tuple(errors),
            decides=tuple(decides),
            evolves=tuple(evolves),
            invariants=tuple(invariants),
            stale_policies=tuple(stale_policies),
            loc=_loc(meta),
        )
        return ("aggregate", aggregate, tuple(projections))

    def async_def(self, meta):
        return ("async", True, _loc(meta))

    def reliable_def(self, meta, value=True):
        return ("reliable", bool(value), _loc(meta))

    def irreversible_def(self, meta, value=True):
        return ("irreversible", bool(value), _loc(meta))

    def idempotency_def(self, meta, ref):
        return ("idempotency_key", ref, _loc(meta))

    def correlation_def(self, meta, ref):
        return ("correlation_id", ref, _loc(meta))

    def handles_def(self, meta, name):
        return ("handles", name, _loc(meta))

    def request_event_def(self, meta, name):
        return ("request_event", name, _loc(meta))

    def success_event_def(self, meta, name):
        return ("success_event", name, _loc(meta))

    def failure_event_def(self, meta, name):
        return ("failure_event", name, _loc(meta))

    def timeout_event_def(self, meta, name):
        return ("timeout_event", name, _loc(meta))

    def max_attempts_def(self, meta, attempts):
        return ("max_attempts", attempts, _loc(meta))

    def backoff_def(self, meta, name):
        return ("backoff", name, _loc(meta))

    def retry_def(self, meta, *items):
        max_attempts = None
        backoff = None
        loc = _loc(meta)
        for item in items:
            if item[0] == "max_attempts":
                max_attempts = item[1]
            elif item[0] == "backoff":
                backoff = item[1]
        return ("retry", DomainRetry(max_attempts=max_attempts, backoff=backoff, loc=loc), loc)

    def timeout_def(self, meta, duration, event):
        return ("timeout", str(duration), event, _loc(meta))

    def compensation_def(self, meta, *items):
        events = []
        for item in items:
            if item[0] == "emits":
                events.extend(item[1])
        return ("compensation", tuple(events), _loc(meta))

    def outbox_def(self, meta, name):
        return ("outbox", name, _loc(meta))

    def inbox_def(self, meta, name):
        return ("inbox", name, _loc(meta))

    def effect_def(self, meta, name, *items):
        values = {
            "async_effect": False,
            "reliable": False,
            "irreversible": False,
            "idempotency_key": None,
            "correlation_id": None,
            "handles": None,
            "outcomes": [],
            "request_event": None,
            "success_event": None,
            "failure_event": None,
            "timeout_event": None,
            "retry": DomainRetry(),
            "timeout_after": None,
            "compensation_events": [],
            "outbox": None,
            "inbox": None,
        }
        for item in items:
            if isinstance(item, DomainField):
                continue
            tag = item[0]
            if tag == "async":
                values["async_effect"] = True
            elif tag == "reliable":
                values["reliable"] = item[1]
            elif tag == "irreversible":
                values["irreversible"] = item[1]
            elif tag == "idempotency_key":
                values["idempotency_key"] = item[1]
            elif tag == "correlation_id":
                values["correlation_id"] = item[1]
            elif tag == "handles":
                values["handles"] = item[1]
            elif tag == "emits":
                values["outcomes"].extend(item[1])
            elif tag == "request_event":
                values["request_event"] = item[1]
            elif tag == "success_event":
                values["success_event"] = item[1]
            elif tag == "failure_event":
                values["failure_event"] = item[1]
            elif tag == "timeout_event":
                values["timeout_event"] = item[1]
            elif tag == "retry":
                values["retry"] = item[1]
            elif tag == "timeout":
                values["timeout_after"] = item[1]
                values["timeout_event"] = values["timeout_event"] or item[2]
                if item[2] not in values["outcomes"]:
                    values["outcomes"].append(item[2])
            elif tag == "compensation":
                values["compensation_events"].extend(item[1])
            elif tag == "outbox":
                values["outbox"] = item[1]
            elif tag == "inbox":
                values["inbox"] = item[1]
        if values["success_event"] and values["success_event"] not in values["outcomes"]:
            values["outcomes"].append(values["success_event"])
        if values["failure_event"] and values["failure_event"] not in values["outcomes"]:
            values["outcomes"].append(values["failure_event"])
        if values["timeout_event"] and values["timeout_event"] not in values["outcomes"]:
            values["outcomes"].append(values["timeout_event"])
        if values["request_event"] and not values["handles"]:
            values["handles"] = values["request_event"]
        return DomainEffect(
            name=name,
            async_effect=values["async_effect"],
            reliable=values["reliable"],
            irreversible=values["irreversible"],
            idempotency_key=values["idempotency_key"],
            correlation_id=values["correlation_id"],
            handles=values["handles"],
            outcomes=tuple(values["outcomes"]),
            request_event=values["request_event"],
            success_event=values["success_event"],
            failure_event=values["failure_event"],
            timeout_event=values["timeout_event"],
            retry=values["retry"],
            timeout_after=values["timeout_after"],
            compensation_events=tuple(values["compensation_events"]),
            outbox=values["outbox"],
            inbox=values["inbox"],
            loc=_loc(meta),
        )

    def await_one_of(self, meta):
        return "one_of"

    def await_all(self, meta):
        return "all"

    def await_any(self, meta):
        return "any"

    def waits_for_def(self, meta, mode, events):
        return ("waits_for", mode, tuple(events), _loc(meta))

    def await_on_def(self, meta, source, _arrow, target):
        return ("on", source, target, _loc(meta))

    def await_def(self, meta, name, *items):
        mode = "one_of"
        events = ()
        branches = []
        for item in items:
            if item[0] == "waits_for":
                mode = item[1]
                events = item[2]
            elif item[0] == "on":
                branches.append((item[1], item[2]))
        return DomainAwait(name=name, mode=mode, events=tuple(events), branches=tuple(branches), loc=_loc(meta))

    def starts_on_def(self, meta, event):
        return ("starts_on", event, _loc(meta))

    def awaits_def(self, meta, mode, events):
        return ("awaits", mode, tuple(events), _loc(meta))

    def saga_step_def(self, meta, name, *items):
        async_step = False
        requires = []
        emits = []
        awaits_mode = "one_of"
        awaits = []
        timeout_after = None
        timeout_event = None
        for item in items:
            tag = item[0]
            if tag == "async":
                async_step = True
            elif tag == "requires":
                requires.append(item[1])
            elif tag == "emits":
                emits.extend(item[1])
            elif tag == "awaits":
                awaits_mode = item[1]
                awaits.extend(item[2])
            elif tag == "timeout":
                timeout_after = item[1]
                timeout_event = item[2]
        return DomainSagaStep(
            name=name,
            async_step=async_step,
            requires=tuple(requires),
            emits=tuple(emits),
            awaits_mode=awaits_mode,
            awaits=tuple(awaits),
            timeout_after=timeout_after,
            timeout_event=timeout_event,
            loc=_loc(meta),
        )

    def saga_compensation_item(self, meta, trigger, after, *items):
        emits = []
        for item in items:
            if item[0] == "emits":
                emits.extend(item[1])
        return DomainSagaCompensation(
            trigger_event=trigger,
            after_event=after,
            emits=tuple(emits),
            loc=_loc(meta),
        )

    def saga_compensation_block(self, meta, *items):
        return ("saga_compensations", tuple(items), _loc(meta))

    def saga_def(self, meta, name, *items):
        starts_on = None
        steps = []
        compensations = []
        invariants = []
        outboxes = []
        inboxes = []
        for item in items:
            if isinstance(item, DomainSagaStep):
                steps.append(item)
            elif isinstance(item, DomainInvariant):
                invariants.append(item)
            elif isinstance(item, tuple) and item[0] == "starts_on":
                starts_on = item[1]
            elif isinstance(item, tuple) and item[0] == "saga_compensations":
                compensations.extend(item[1])
            elif isinstance(item, tuple) and item[0] == "outbox":
                outboxes.append(item[1])
            elif isinstance(item, tuple) and item[0] == "inbox":
                inboxes.append(item[1])
        return DomainSaga(
            name=name,
            starts_on=starts_on,
            steps=tuple(steps),
            compensations=tuple(compensations),
            invariants=tuple(invariants),
            outboxes=tuple(outboxes),
            inboxes=tuple(inboxes),
            loc=_loc(meta),
        )

    def domain_def(self, meta, name, *items):
        profile = None
        types = []
        aggregates = []
        effects = []
        awaits = []
        sagas = []
        projections = []
        for item in items:
            if item is None:
                continue
            if isinstance(item, tuple) and item[0] == "implementation_profile":
                profile = item[1]
            elif isinstance(item, DomainType):
                types.append(item)
            elif isinstance(item, tuple) and item[0] == "aggregate":
                aggregates.append(item[1])
                projections.extend(item[2])
            elif isinstance(item, DomainEffect):
                effects.append(item)
            elif isinstance(item, DomainAwait):
                awaits.append(item)
            elif isinstance(item, DomainSaga):
                sagas.append(item)
            elif isinstance(item, DomainProjection):
                projections.append(item)
        return DomainSpec(
            name=name,
            implementation_profile=profile,
            types=tuple(types),
            aggregates=tuple(aggregates),
            effects=tuple(effects),
            awaits=tuple(awaits),
            sagas=tuple(sagas),
            projections=tuple(projections),
            loc=_loc(meta),
        )

    def start(self, meta, domain):
        return domain


def parse_domain(src: str) -> DomainSpec:
    return DomainAst().transform(PARSER.parse(src))
