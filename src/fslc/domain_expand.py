# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Lower fsl-domain / fsl-effect typed IR into the existing FSL kernel."""
from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Dict, Iterable, List, Optional, Sequence, Tuple

from .domain_ir import (
    DomainAggregate,
    DomainAssignment,
    DomainDecide,
    DomainEffect,
    DomainEvolve,
    DomainField,
    DomainSaga,
    DomainSagaStep,
    DomainSpec,
    DomainType,
)
from .grammar import Ast, PARSER
from .model import FslError


DOMAIN_FINDING_SCHEMA_VERSION = "fsl-domain-finding.v0"
DOMAIN_DIALECT_VERSION = "fsl-domain-effect-mvp.v0"


@dataclass(frozen=True)
class DomainKernelExpansion:
    ast: tuple
    display_names: Dict[str, str]
    source: str
    assumptions: List[dict]
    generated_actions: List[str]


def _err(message, loc=None, hint=None):
    raise FslError(message, kind="semantics", loc=loc, hint=hint)


def _safe(name):
    out = re.sub(r"[^A-Za-z0-9_]", "_", name)
    if not out:
        out = "x"
    if out[0].isdigit():
        out = "_" + out
    return out


def _lower_name(name):
    s = re.sub(r"(.)([A-Z][a-z]+)", r"\1_\2", name)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    return _safe(s.lower())


def _camel(name):
    parts = _safe(name).split("_")
    if not parts:
        return name
    return parts[0] + "".join(p[:1].upper() + p[1:] for p in parts[1:])


def _type_names_in(type_ref: str) -> List[str]:
    return [
        name for name in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", type_ref)
        if name not in {"Int", "Bool", "Map", "Set", "Seq", "Option", "relation"}
    ]


def _kernel_type(type_ref: str) -> str:
    # The domain MVP keeps type names intact. Enum member names are namespaced,
    # but enum type names, domain ranges, and value-object struct names are not.
    return type_ref


def _meta(rule, text):
    return f'"{rule}: {text}"'


def _indent(lines: Iterable[str], spaces=2):
    pad = " " * spaces
    return [pad + line if line else "" for line in lines]


class _ExpansionContext:
    def __init__(self, domain: DomainSpec):
        self.domain = domain
        self.type_map: Dict[str, DomainType] = {}
        for ty in domain.types:
            self.type_map[ty.name] = ty
        self.external_types: Dict[str, DomainType] = {}
        self.enum_members: Dict[Tuple[str, str], str] = {}
        self.member_to_types: Dict[str, List[str]] = {}
        self.display_names: Dict[str, str] = {}
        self.generated_actions: List[str] = []
        self.assumptions = [
            {
                "id": "DOMAIN-ASSUME-BOUNDED-MVP-MODEL",
                "text": (
                    "domain IDs and undeclared scalar input types are modeled as "
                    "finite 0..1 ranges unless declared explicitly"
                ),
            },
            {
                "id": "DOMAIN-ASSUME-GENERATED-SCAFFOLD",
                "text": (
                    "generated Functional DDD code is an implementation scaffold; "
                    "runtime conformance still requires an adapter/replay evidence boundary"
                ),
            },
        ]
        self.inferred_defaults = False

    def collect_missing_types(self):
        declared = set(self.type_map)
        refs = set()
        for aggregate in self.domain.aggregates:
            if aggregate.id_type:
                refs.add(aggregate.id_type)
            for field in aggregate.state:
                refs.update(_type_names_in(field.type_name))
            for command in aggregate.commands:
                for field in command.inputs:
                    refs.update(_type_names_in(field.type_name))
            for event in aggregate.events:
                for field in event.fields:
                    refs.update(_type_names_in(field.type_name))
        for name in sorted(refs - declared):
            self.external_types[name] = DomainType(name=name, kind="external", lo="0", hi="1")
            self.type_map[name] = self.external_types[name]

    def init_enum_members(self):
        for ty in self.type_map.values():
            if ty.kind == "enum":
                for member in ty.members:
                    kernel = f"{ty.name}_{member}"
                    self.enum_members[(ty.name, member)] = kernel
                    self.member_to_types.setdefault(member, []).append(ty.name)

    def is_enum(self, type_name: Optional[str]) -> bool:
        return bool(type_name and type_name in self.type_map and self.type_map[type_name].kind == "enum")

    def enum_value(self, type_name: str, value: str) -> str:
        value = value.strip()
        if (type_name, value) in self.enum_members:
            return self.enum_members[(type_name, value)]
        return value

    def default_expr(self, field: DomainField, type_env: Optional[Dict[str, str]] = None) -> str:
        if field.default is not None:
            return self.normalize_expr(field.default, None, type_env or {}, target_type=field.type_name)
        ty = self.type_map.get(field.type_name)
        self.inferred_defaults = True
        if field.type_name == "Bool":
            return "false"
        if field.type_name == "Int":
            return "0"
        if ty is None:
            return "0"
        if ty.kind == "enum":
            return self.enum_value(ty.name, ty.members[0])
        if ty.kind in ("range", "external"):
            return str(ty.lo or "0")
        if ty.kind == "value_object":
            parts = []
            for sub in ty.fields:
                parts.append(f"{sub.name}: {self.default_expr(sub)}")
            return f"{ty.name} {{ {', '.join(parts)} }}"
        return "0"

    def state_name(self, aggregate: DomainAggregate, field_name: str) -> str:
        return f"{_lower_name(aggregate.name)}_{_safe(field_name)}"

    def event_flag(self, event_name: str) -> str:
        return f"event_{_safe(event_name)}"

    def effect_status_type(self, effect: DomainEffect) -> str:
        return f"{_safe(effect.name)}EffectStatus"

    def effect_status_member(self, effect: DomainEffect, member: str) -> str:
        return f"{_safe(effect.name)}EffectStatus_{member}"

    def effect_status_var(self, effect: DomainEffect) -> str:
        return f"{_lower_name(effect.name)}_status"

    def effect_attempt_type(self, effect: DomainEffect) -> str:
        return f"{_safe(effect.name)}Attempt"

    def effect_attempt_var(self, effect: DomainEffect) -> str:
        return f"{_lower_name(effect.name)}_attempts"

    def request_event_for(self, effect: DomainEffect) -> Optional[str]:
        return effect.handles or effect.request_event

    def correlation_field(self, effect: DomainEffect) -> Optional[str]:
        if effect.correlation_id and "." in effect.correlation_id:
            return effect.correlation_id.rsplit(".", 1)[1]
        if effect.correlation_id:
            return effect.correlation_id
        return None

    def correlation_type(self, effect: DomainEffect) -> Optional[str]:
        field_name = self.correlation_field(effect)
        request = self.request_event_for(effect)
        if not field_name or not request:
            return None
        event = self.domain.event_map().get(request)
        if not event:
            return None
        for field in event.fields:
            if field.name == field_name:
                return field.type_name
        return None

    def normalize_expr(
            self,
            expr: str,
            aggregate: Optional[DomainAggregate],
            type_env: Dict[str, str],
            target_type: Optional[str] = None,
            replace_state: bool = True) -> str:
        out = " ".join(str(expr).strip().split())
        out = out.replace("&&", " and ").replace("||", " or ")
        out = re.sub(r"(?<![<>=!])-(?=>)", "=", out)
        out = out.replace("->", "=>")
        if aggregate is not None:
            out = self._replace_can(out, aggregate, type_env)
        out = self._replace_membership(out, type_env)
        out = self._replace_enum_comparisons(out, type_env)
        if target_type and self.is_enum(target_type):
            m = re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", out)
            if m:
                out = self.enum_value(target_type, out)
        if replace_state and aggregate is not None:
            for field in sorted(aggregate.state, key=lambda f: -len(f.name)):
                out = re.sub(rf"\b{re.escape(field.name)}\b", self.state_name(aggregate, field.name), out)
        return " ".join(out.split())

    def normalize_lvalue(self, target: str, aggregate: DomainAggregate) -> str:
        out = target.strip()
        for field in sorted(aggregate.state, key=lambda f: -len(f.name)):
            out = re.sub(rf"^{re.escape(field.name)}\b", self.state_name(aggregate, field.name), out)
        return out

    def _replace_membership(self, expr: str, type_env: Dict[str, str]) -> str:
        pattern = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s+in\s+\[([^\]]+)\]")

        def repl(match):
            left = match.group(1)
            raw_values = [v.strip() for v in match.group(2).split(",") if v.strip()]
            ty = type_env.get(left)
            parts = []
            for value in raw_values:
                normalized = self.enum_value(ty, value) if ty and self.is_enum(ty) else value
                parts.append(f"{left} == {normalized}")
            return "(" + " or ".join(parts or ["false"]) + ")"

        prev = None
        out = expr
        while out != prev:
            prev = out
            out = pattern.sub(repl, out)
        return out

    def _replace_enum_comparisons(self, expr: str, type_env: Dict[str, str]) -> str:
        pattern = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*(==|!=)\s*([A-Za-z_][A-Za-z0-9_]*)\b")

        def repl(match):
            left, op, right = match.group(1), match.group(2), match.group(3)
            ty = type_env.get(left)
            if ty and self.is_enum(ty):
                right = self.enum_value(ty, right)
            return f"{left} {op} {right}"

        return pattern.sub(repl, expr)

    def _replace_can(self, expr: str, aggregate: DomainAggregate, type_env: Dict[str, str]) -> str:
        pattern = re.compile(r"\bcan\(([A-Za-z_][A-Za-z0-9_]*)\)")

        def repl(match):
            command_name = match.group(1)
            decide = aggregate.decide_map().get(command_name)
            if decide is None:
                return "false"
            pieces = list(decide.requires)
            pieces.extend(f"not ({reject.condition})" for reject in decide.rejects)
            if not pieces:
                return "true"
            normalized = [
                self.normalize_expr(piece, aggregate, type_env, replace_state=False)
                for piece in pieces
            ]
            return "(" + " and ".join(normalized) + ")"

        return pattern.sub(repl, expr)


def validate_domain(domain: DomainSpec):
    if domain.implementation_profile and domain.implementation_profile != "functional_ddd":
        _err(
            f"unsupported implementation_profile '{domain.implementation_profile}'",
            loc=domain.loc,
            hint="the MVP supports implementation_profile functional_ddd",
        )

    names = set()
    for ty in domain.types:
        if ty.name in names:
            _err(f"duplicate domain type '{ty.name}'", loc=ty.loc)
        names.add(ty.name)
        if ty.kind == "enum" and not ty.members:
            _err(f"domain enum '{ty.name}' has no members", loc=ty.loc)
        if ty.kind == "value_object" and not ty.fields:
            _err(f"value_object '{ty.name}' requires at least one field", loc=ty.loc)

    aggregate_names = set()
    event_names = set()
    command_names = set()
    for aggregate in domain.aggregates:
        if aggregate.name in aggregate_names:
            _err(f"duplicate aggregate '{aggregate.name}'", loc=aggregate.loc)
        aggregate_names.add(aggregate.name)
        _validate_unique([f.name for f in aggregate.state], "state field", aggregate.loc)
        _validate_unique([c.name for c in aggregate.commands], "command", aggregate.loc)
        _validate_unique([e.name for e in aggregate.events], "event", aggregate.loc)
        _validate_unique([e.name for e in aggregate.errors], "error", aggregate.loc)
        for command in aggregate.commands:
            if command.name in command_names:
                _err(f"duplicate command '{command.name}'", loc=command.loc)
            command_names.add(command.name)
        for event in aggregate.events:
            if event.name in event_names:
                _err(f"duplicate event '{event.name}'", loc=event.loc)
            event_names.add(event.name)
        command_map = aggregate.command_map()
        event_map = aggregate.event_map()
        error_map = {error.name for error in aggregate.errors}
        for decide in aggregate.decides:
            if decide.command not in command_map:
                _err(f"decide references unknown command '{decide.command}'", loc=decide.loc)
            for reject in decide.rejects:
                if reject.error not in error_map:
                    _err(f"rejects unknown error '{reject.error}'", loc=reject.loc)
            for event in decide.emits:
                if event not in event_map:
                    _err(f"decide emits unknown event '{event}'", loc=decide.loc)
        for evolve in aggregate.evolves:
            if evolve.event not in event_map:
                _err(f"evolve references unknown event '{evolve.event}'", loc=evolve.loc)

    all_events = domain.event_map()
    for effect in domain.effects:
        request = effect.handles or effect.request_event
        if request and request not in all_events:
            _err(f"effect '{effect.name}' handles unknown event '{request}'", loc=effect.loc)
        for event in effect.outcomes:
            if event not in all_events:
                _err(f"effect '{effect.name}' emits unknown event '{event}'", loc=effect.loc)

    saga_names = set()
    for saga in domain.sagas:
        if saga.name in saga_names:
            _err(f"duplicate saga '{saga.name}'", loc=saga.loc)
        saga_names.add(saga.name)
        if not saga.starts_on:
            _err(f"saga '{saga.name}' requires starts_on <Event>", loc=saga.loc)
        if saga.starts_on and saga.starts_on not in all_events:
            _err(f"saga '{saga.name}' starts_on unknown event '{saga.starts_on}'", loc=saga.loc)
        _validate_unique([step.name for step in saga.steps], "saga step", saga.loc)
        for step in saga.steps:
            for event in step.emits:
                if event not in all_events:
                    _err(f"saga step '{step.name}' emits unknown event '{event}'", loc=step.loc)
            for event in step.awaits:
                if event not in all_events:
                    _err(f"saga step '{step.name}' awaits unknown event '{event}'", loc=step.loc)
            if step.timeout_event and step.timeout_event not in all_events:
                _err(f"saga step '{step.name}' timeout emits unknown event '{step.timeout_event}'", loc=step.loc)
        for compensation in saga.compensations:
            for event in (compensation.trigger_event, compensation.after_event):
                if event not in all_events:
                    _err(f"saga '{saga.name}' compensation references unknown event '{event}'", loc=compensation.loc)
            for event in compensation.emits:
                if event not in all_events:
                    _err(f"saga '{saga.name}' compensation emits unknown event '{event}'", loc=compensation.loc)


def _validate_unique(values: Sequence[str], kind: str, loc=None):
    seen = set()
    for value in values:
        if value in seen:
            _err(f"duplicate {kind} '{value}'", loc=loc)
        seen.add(value)


def expand_domain(domain: DomainSpec) -> DomainKernelExpansion:
    validate_domain(domain)
    ctx = _ExpansionContext(domain)
    ctx.collect_missing_types()
    ctx.init_enum_members()
    source = render_kernel_source(domain, ctx)
    ast = Ast().transform(PARSER.parse(source))
    assumptions = list(ctx.assumptions)
    if ctx.inferred_defaults:
        assumptions.append({
            "id": "DOMAIN-ASSUME-DEFAULT-INITIAL-STATE",
            "text": (
                "state fields without explicit defaults use false, lower bound, "
                "or the first enum member as the finite initial model"
            ),
        })
    if domain.sagas:
        assumptions.append({
            "id": "DOMAIN-ASSUME-SAGA-HISTORY-MVP",
            "text": (
                "saga awaits and compensation 'after' clauses are lowered with "
                "per-step event observations; durable process history requires runtime replay evidence"
            ),
        })
    return DomainKernelExpansion(
        ast=ast,
        display_names=ctx.display_names,
        source=source,
        assumptions=assumptions,
        generated_actions=ctx.generated_actions,
    )


def render_kernel_source(domain: DomainSpec, ctx: Optional[_ExpansionContext] = None) -> str:
    ctx = ctx or _ExpansionContext(domain)
    if not ctx.type_map:
        ctx.collect_missing_types()
        ctx.init_enum_members()
    lines = [f"spec {domain.name} \"domain: generated from fsl-domain/fsl-effect\" {{"]
    lines.extend(_indent(_render_type_declarations(ctx)))
    state_lines, init_lines = _render_state_and_init(domain, ctx)
    lines.extend(_indent(["state {"]))
    lines.extend(_indent(state_lines, 4))
    lines.extend(_indent(["}", "init {"]))
    lines.extend(_indent(init_lines, 4))
    lines.extend(_indent(["}"]))
    action_lines = _render_actions(domain, ctx)
    lines.extend(_indent(action_lines))
    lines.extend(_indent(_render_invariants(domain, ctx)))
    lines.extend(_indent(["terminal { false }"]))
    lines.append("}")
    return "\n".join(lines) + "\n"


def _render_type_declarations(ctx: _ExpansionContext) -> List[str]:
    lines: List[str] = []
    for ty in ctx.type_map.values():
        if ty.kind == "enum":
            members = ", ".join(ctx.enum_value(ty.name, member) for member in ty.members)
            lines.append(f"enum {ty.name} {{ {members} }}")
        elif ty.kind in ("range", "external"):
            lines.append(f"type {ty.name} = {ty.lo}..{ty.hi}")
        elif ty.kind == "value_object":
            fields = ", ".join(f"{field.name}: {_kernel_type(field.type_name)}" for field in ty.fields)
            lines.append(f"struct {ty.name} {{ {fields} }}")
    for effect in ctx.domain.effects:
        status_type = ctx.effect_status_type(effect)
        members = [
            ctx.effect_status_member(effect, "NotStarted"),
            ctx.effect_status_member(effect, "Pending"),
            ctx.effect_status_member(effect, "Succeeded"),
            ctx.effect_status_member(effect, "Failed"),
            ctx.effect_status_member(effect, "TimedOut"),
            ctx.effect_status_member(effect, "Cancelled"),
            ctx.effect_status_member(effect, "Compensated"),
        ]
        lines.append(f"enum {status_type} {{ {', '.join(members)} }}")
        max_attempts = effect.retry.max_attempts or 1
        lines.append(f"type {ctx.effect_attempt_type(effect)} = 0..{max_attempts}")
    return lines


def _render_state_and_init(domain: DomainSpec, ctx: _ExpansionContext) -> Tuple[List[str], List[str]]:
    state_lines: List[str] = []
    init_lines: List[str] = []
    event_names = sorted(domain.event_map())
    for aggregate in domain.aggregates:
        type_env = {field.name: field.type_name for field in aggregate.state}
        for field in aggregate.state:
            name = ctx.state_name(aggregate, field.name)
            ctx.display_names[name] = f"{aggregate.name}.{field.name}"
            state_lines.append(f"{name}: {_kernel_type(field.type_name)},")
            init_lines.append(f"{name} = {ctx.default_expr(field, type_env)}")
    for event_name in event_names:
        flag = ctx.event_flag(event_name)
        ctx.display_names[flag] = f"event.{event_name}"
        state_lines.append(f"{flag}: Bool,")
        init_lines.append(f"{flag} = false")
    for effect in domain.effects:
        corr_type = ctx.correlation_type(effect)
        if not corr_type:
            continue
        status_var = ctx.effect_status_var(effect)
        attempts_var = ctx.effect_attempt_var(effect)
        state_lines.append(f"{status_var}: Map<{corr_type}, {ctx.effect_status_type(effect)}>,")
        state_lines.append(f"{attempts_var}: Map<{corr_type}, {ctx.effect_attempt_type(effect)}>,")
        init_lines.append(
            f"forall k: {corr_type} {{ {status_var}[k] = "
            f"{ctx.effect_status_member(effect, 'NotStarted')} }}"
        )
        init_lines.append(f"forall k: {corr_type} {{ {attempts_var}[k] = 0 }}")
        ctx.display_names[status_var] = f"effect.{effect.name}.status"
        ctx.display_names[attempts_var] = f"effect.{effect.name}.attempts"
    return state_lines, init_lines


def _event_flag_assignments(domain: DomainSpec, ctx: _ExpansionContext, emitted: Sequence[str]) -> List[str]:
    emitted_set = set(emitted)
    return [
        f"{ctx.event_flag(event_name)} = {'true' if event_name in emitted_set else 'false'}"
        for event_name in sorted(domain.event_map())
    ]


def _render_actions(domain: DomainSpec, ctx: _ExpansionContext) -> List[str]:
    lines: List[str] = []
    effects_by_request = {}
    for effect in domain.effects:
        request = ctx.request_event_for(effect)
        if request:
            effects_by_request.setdefault(request, []).append(effect)

    for aggregate in domain.aggregates:
        command_map = aggregate.command_map()
        evolve_map = aggregate.evolve_map()
        for decide in aggregate.decides:
            command = command_map[decide.command]
            params = ", ".join(f"{field.name}: {_kernel_type(field.type_name)}" for field in command.inputs)
            action_name = f"{_lower_name(aggregate.name)}_{_lower_name(command.name)}"
            ctx.generated_actions.append(action_name)
            ctx.display_names[action_name] = f"{aggregate.name}.{command.name}"
            type_env = {field.name: field.type_name for field in aggregate.state}
            type_env.update({field.name: field.type_name for field in command.inputs})
            body = [f"action {action_name}({params}) {{"]
            for req in decide.requires:
                body.append(f"  requires {ctx.normalize_expr(req, aggregate, type_env)}")
            for reject in decide.rejects:
                body.append(f"  requires not ({ctx.normalize_expr(reject.condition, aggregate, type_env)})")
            for event_name in decide.emits:
                for effect in effects_by_request.get(event_name, []):
                    corr = ctx.correlation_field(effect)
                    if corr and corr in type_env:
                        status_var = ctx.effect_status_var(effect)
                        pending = ctx.effect_status_member(effect, "Pending")
                        succeeded = ctx.effect_status_member(effect, "Succeeded")
                        body.append(f"  requires {status_var}[{corr}] != {pending}")
                        body.append(f"  requires {status_var}[{corr}] != {succeeded}")
            body.extend("  " + stmt for stmt in _event_flag_assignments(domain, ctx, decide.emits))
            for event_name in decide.emits:
                for stmt in _evolve_assignments(ctx, aggregate, evolve_map.get(event_name), type_env):
                    body.append("  " + stmt)
                for effect in effects_by_request.get(event_name, []):
                    corr = ctx.correlation_field(effect)
                    if corr and corr in type_env:
                        body.append(
                            f"  {ctx.effect_status_var(effect)}[{corr}] = "
                            f"{ctx.effect_status_member(effect, 'Pending')}"
                        )
                        body.append(f"  {ctx.effect_attempt_var(effect)}[{corr}] = 1")
            body.append("}")
            lines.extend(body)

    for effect in domain.effects:
        lines.extend(_render_effect_actions(domain, ctx, effect))
    for saga in domain.sagas:
        lines.extend(_render_saga_actions(domain, ctx, saga))
    return lines


def _evolve_assignments(
        ctx: _ExpansionContext,
        aggregate: DomainAggregate,
        evolve: Optional[DomainEvolve],
        type_env: Dict[str, str]) -> List[str]:
    if evolve is None:
        return []
    out = []
    state = aggregate.state_map()
    for req in evolve.requires:
        out.append(f"requires {ctx.normalize_expr(req, aggregate, type_env)}")
    for assignment in evolve.assignments:
        root = re.match(r"[A-Za-z_][A-Za-z0-9_]*", assignment.target)
        target_type = state[root.group(0)].type_name if root and root.group(0) in state else None
        target = ctx.normalize_lvalue(assignment.target, aggregate)
        expr = ctx.normalize_expr(assignment.expr, aggregate, type_env, target_type=target_type)
        out.append(f"{target} = {expr}")
    return out


def _render_effect_actions(domain: DomainSpec, ctx: _ExpansionContext, effect: DomainEffect) -> List[str]:
    corr = ctx.correlation_field(effect)
    corr_type = ctx.correlation_type(effect)
    owner_by_event = domain.event_owner()
    if not corr or not corr_type:
        return []
    lines: List[str] = []
    status_var = ctx.effect_status_var(effect)
    attempts_var = ctx.effect_attempt_var(effect)
    pending = ctx.effect_status_member(effect, "Pending")
    failed = ctx.effect_status_member(effect, "Failed")
    timed_out = ctx.effect_status_member(effect, "TimedOut")
    succeeded = ctx.effect_status_member(effect, "Succeeded")

    for event_name in effect.outcomes:
        aggregate = owner_by_event.get(event_name)
        if aggregate is None:
            continue
        event = aggregate.event_map()[event_name]
        params = _event_action_params(event, corr, corr_type)
        type_env = {field.name: field.type_name for field in aggregate.state}
        type_env.update({field.name: field.type_name for field in event.fields})
        action_name = f"{_lower_name(effect.name)}_complete_{_lower_name(event_name)}"
        ctx.generated_actions.append(action_name)
        ctx.display_names[action_name] = f"{effect.name}.{event_name}"
        next_status = _status_for_outcome(ctx, effect, event_name)
        body = [f"action {action_name}({params}) {{"]
        body.append(f"  requires {status_var}[{corr}] == {pending}")
        body.extend("  " + stmt for stmt in _event_flag_assignments(domain, ctx, [event_name]))
        body.append(f"  {status_var}[{corr}] = {next_status}")
        body.extend(
            "  " + stmt
            for stmt in _evolve_assignments(ctx, aggregate, aggregate.evolve_map().get(event_name), type_env)
        )
        body.append("}")
        lines.extend(body)

    if effect.retry.max_attempts:
        action_name = f"{_lower_name(effect.name)}_retry"
        ctx.generated_actions.append(action_name)
        params = f"{corr}: {corr_type}"
        body = [f"action {action_name}({params}) {{"]
        body.append(f"  requires {status_var}[{corr}] == {failed} or {status_var}[{corr}] == {timed_out}")
        body.append(f"  requires {attempts_var}[{corr}] < {effect.retry.max_attempts}")
        body.extend("  " + stmt for stmt in _event_flag_assignments(domain, ctx, []))
        body.append(f"  {status_var}[{corr}] = {pending}")
        body.append(f"  {attempts_var}[{corr}] = {attempts_var}[{corr}] + 1")
        body.append("}")
        lines.extend(body)

    # A sticky success transition invariant is useful for generated effect status
    # even though the action guards already prevent duplicate completion.
    _ = succeeded
    return lines


def _event_action_params(event, corr: str, corr_type: str) -> str:
    parts = []
    seen = set()
    for field in event.fields:
        parts.append(f"{field.name}: {_kernel_type(field.type_name)}")
        seen.add(field.name)
    if corr not in seen:
        parts.insert(0, f"{corr}: {corr_type}")
    return ", ".join(parts)


def _status_for_outcome(ctx: _ExpansionContext, effect: DomainEffect, event_name: str) -> str:
    lowered = event_name.lower()
    if effect.timeout_event == event_name or "timeout" in lowered or "timedout" in lowered:
        return ctx.effect_status_member(effect, "TimedOut")
    if effect.failure_event == event_name or "fail" in lowered:
        return ctx.effect_status_member(effect, "Failed")
    if "cancel" in lowered:
        return ctx.effect_status_member(effect, "Cancelled")
    return ctx.effect_status_member(effect, "Succeeded")


def _render_saga_actions(domain: DomainSpec, ctx: _ExpansionContext, saga: DomainSaga) -> List[str]:
    lines: List[str] = []
    lines.extend(_render_saga_observation_actions(domain, ctx, saga))
    for index, step in enumerate(saga.steps):
        action_name = f"saga_{_lower_name(saga.name)}_{_lower_name(step.name)}"
        ctx.generated_actions.append(action_name)
        ctx.display_names[action_name] = f"{saga.name}.{step.name}"
        body = [f"action {action_name}() {{"]
        guards = _saga_step_guards(ctx, saga, step, first=(index == 0))
        for guard in guards:
            body.append(f"  requires {guard}")
        body.extend("  " + stmt for stmt in _event_flag_assignments(domain, ctx, step.emits))
        body.append("}")
        lines.extend(body)

        if step.timeout_event:
            timeout_name = f"{action_name}_timeout"
            ctx.generated_actions.append(timeout_name)
            ctx.display_names[timeout_name] = f"{saga.name}.{step.name}.timeout"
            body = [f"action {timeout_name}() {{"]
            for guard in guards:
                body.append(f"  requires {guard}")
            body.extend("  " + stmt for stmt in _event_flag_assignments(domain, ctx, [step.timeout_event]))
            body.append("}")
            lines.extend(body)

    for compensation in saga.compensations:
        action_name = (
            f"saga_{_lower_name(saga.name)}_compensate_"
            f"{_lower_name(compensation.trigger_event)}_after_{_lower_name(compensation.after_event)}"
        )
        ctx.generated_actions.append(action_name)
        ctx.display_names[action_name] = (
            f"{saga.name}.compensation.{compensation.trigger_event}.after.{compensation.after_event}"
        )
        body = [f"action {action_name}() {{"]
        body.append(f"  requires {ctx.event_flag(compensation.trigger_event)}")
        body.extend("  " + stmt for stmt in _event_flag_assignments(domain, ctx, compensation.emits))
        body.append("}")
        lines.extend(body)
    return lines


def _render_saga_observation_actions(domain: DomainSpec, ctx: _ExpansionContext, saga: DomainSaga) -> List[str]:
    owner_by_event = domain.event_owner()
    event_names = set()
    for step in saga.steps:
        event_names.update(step.awaits)
    for compensation in saga.compensations:
        event_names.add(compensation.trigger_event)
        event_names.add(compensation.after_event)

    lines: List[str] = []
    for event_name in sorted(event_names):
        aggregate = owner_by_event.get(event_name)
        if aggregate is None:
            continue
        event = aggregate.event_map()[event_name]
        params = ", ".join(f"{field.name}: {_kernel_type(field.type_name)}" for field in event.fields)
        action_name = f"saga_{_lower_name(saga.name)}_observe_{_lower_name(event_name)}"
        ctx.generated_actions.append(action_name)
        ctx.display_names[action_name] = f"{saga.name}.observe.{event_name}"
        type_env = {field.name: field.type_name for field in aggregate.state}
        type_env.update({field.name: field.type_name for field in event.fields})
        body = [f"action {action_name}({params}) {{"]
        body.extend("  " + stmt for stmt in _event_flag_assignments(domain, ctx, [event_name]))
        body.extend(
            "  " + stmt
            for stmt in _evolve_assignments(ctx, aggregate, aggregate.evolve_map().get(event_name), type_env)
        )
        body.append("}")
        lines.extend(body)
    return lines


def _saga_step_guards(
        ctx: _ExpansionContext,
        saga: DomainSaga,
        step: DomainSagaStep,
        first: bool) -> List[str]:
    guards: List[str] = []
    if first and saga.starts_on:
        guards.append(ctx.event_flag(saga.starts_on))
    guards.extend(_normalize_saga_condition(ctx, req) for req in step.requires)
    if not step.emits and step.awaits:
        guards.append(_await_condition(ctx, step.awaits_mode, step.awaits))
    return [guard for guard in guards if guard]


def _await_condition(ctx: _ExpansionContext, mode: str, events: Sequence[str]) -> str:
    flags = [ctx.event_flag(event) for event in events]
    if not flags:
        return "true"
    if mode == "all":
        return "(" + " and ".join(flags) + ")"
    return "(" + " or ".join(flags) + ")"


def _normalize_saga_condition(ctx: _ExpansionContext, expr: str) -> str:
    out = " ".join(str(expr).strip().split())
    out = out.replace("&&", " and ").replace("||", " or ").replace("->", "=>")
    event_names = sorted(ctx.domain.event_map(), key=lambda name: -len(name))
    for event_name in event_names:
        out = re.sub(rf"\b{re.escape(event_name)}\b", ctx.event_flag(event_name), out)
    return out


def _render_invariants(domain: DomainSpec, ctx: _ExpansionContext) -> List[str]:
    lines: List[str] = []
    for aggregate in domain.aggregates:
        type_env = {field.name: field.type_name for field in aggregate.state}
        for invariant in aggregate.invariants:
            expr = ctx.normalize_expr(invariant.expr, aggregate, type_env)
            lines.append(
                f"invariant {_safe(aggregate.name)}_{_safe(invariant.name)} "
                f"{_meta('DOMAIN-INVARIANT', f'{aggregate.name}.{invariant.name}')} {{ {expr} }}"
            )
        for ty in ctx.type_map.values():
            if ty.kind != "value_object":
                continue
            for field in aggregate.state:
                if field.type_name != ty.name:
                    continue
                env = {sub.name: sub.type_name for sub in ty.fields}
                for invariant in ty.invariants:
                    expr = invariant.expr
                    for sub in sorted(ty.fields, key=lambda f: -len(f.name)):
                        expr = re.sub(rf"\b{re.escape(sub.name)}\b", f"{field.name}.{sub.name}", expr)
                    expr = ctx.normalize_expr(expr, aggregate, {**type_env, **env})
                lines.append(
                    f"invariant {_safe(aggregate.name)}_{_safe(field.name)}_{_safe(invariant.name)} "
                    f"{_meta('DOMAIN-VALUE-OBJECT', f'{field.name}.{invariant.name}')} {{ {expr} }}"
                )
    for saga in domain.sagas:
        for invariant in saga.invariants:
            expr = _normalize_saga_condition(ctx, invariant.expr)
            lines.append(
                f"invariant {_safe(saga.name)}_{_safe(invariant.name)} "
                f"{_meta('DOMAIN-SAGA', f'{saga.name}.{invariant.name}')} {{ {expr} }}"
            )
    for effect in domain.effects:
        corr = ctx.correlation_field(effect)
        corr_type = ctx.correlation_type(effect)
        if not corr or not corr_type:
            continue
        status_var = ctx.effect_status_var(effect)
        succeeded = ctx.effect_status_member(effect, "Succeeded")
        lines.append(
            f"trans {_safe(effect.name)}_SuccessSticky "
            f"{_meta('DOMAIN-EFFECT', f'{effect.name} success is sticky')} "
            f"{{ forall k: {corr_type} {{ old({status_var}[k]) == {succeeded} => {status_var}[k] == {succeeded} }} }}"
        )
    return lines


def static_domain_findings(domain: DomainSpec, assumptions: Optional[List[dict]] = None):
    validate_domain(domain)
    ctx = _ExpansionContext(domain)
    ctx.collect_missing_types()
    findings = []
    assumptions = list(assumptions or [])

    for aggregate in domain.aggregates:
        command_names = set(aggregate.command_map())
        event_names = set(aggregate.event_map())
        decide_names = set(aggregate.decide_map())
        evolve_names = set(aggregate.evolve_map())
        for missing in sorted(command_names - decide_names):
            findings.append(_finding(
                "missing_decide_for_command",
                "error",
                domain.name,
                aggregate=aggregate.name,
                command=missing,
                failed_rule="command_has_decide",
                guarantee_kind="structural",
                witness={"command": missing},
                repair=[f"add decide {missing} {{ ... }} to aggregate {aggregate.name}"],
                assumptions=assumptions,
            ))
        for missing in sorted(event_names - evolve_names):
            findings.append(_finding(
                "missing_evolve_for_event",
                "error",
                domain.name,
                aggregate=aggregate.name,
                event=missing,
                failed_rule="event_has_evolve",
                guarantee_kind="structural",
                witness={"event": missing},
                repair=[f"add evolve {missing} {{ ... }} or remove the event if it is not state-bearing"],
                assumptions=assumptions,
            ))
        state_names = set(aggregate.state_map())
        for evolve in aggregate.evolves:
            for assignment in evolve.assignments:
                root = re.match(r"[A-Za-z_][A-Za-z0-9_]*", assignment.target)
                if not root or root.group(0) not in state_names:
                    findings.append(_finding(
                        "aggregate_boundary_violation",
                        "error",
                        domain.name,
                        aggregate=aggregate.name,
                        event=evolve.event,
                        failed_rule="aggregate_does_not_modify_foreign_state",
                        guarantee_kind="structural",
                        witness={"assignment": assignment.target},
                        repair=[f"move '{assignment.target}' into {aggregate.name} state or route the change through an event/saga"],
                        assumptions=assumptions,
                    ))
                    findings.append(_finding(
                        "cross_aggregate_update_without_event",
                        "error",
                        domain.name,
                        aggregate=aggregate.name,
                        event=evolve.event,
                        failed_rule="cross_aggregate_change_requires_event_or_saga",
                        guarantee_kind="structural",
                        witness={"assignment": assignment.target},
                        repair=[
                            f"replace direct assignment '{assignment.target}' with a domain event",
                            "or coordinate the change through a saga/process manager",
                        ],
                        assumptions=assumptions,
                    ))
        cancellation_like = _has_cancellation_like_state(aggregate, ctx.type_map)
        if cancellation_like:
            stale_events = {policy.event for policy in aggregate.stale_policies}
            for effect in domain.effects:
                for event_name in effect.outcomes:
                    evolve = aggregate.evolve_map().get(event_name)
                    if evolve and not evolve.requires and event_name not in stale_events and _status_for_name(event_name) == "success":
                        findings.append(_finding(
                            "late_completion_without_stale_policy",
                            "warning",
                            domain.name,
                            aggregate=aggregate.name,
                            event=event_name,
                            effect=effect.name,
                            failed_rule="late_completion_policy",
                            guarantee_kind="structural",
                            witness={"event": event_name, "cancellation_like_state": True},
                            repair=[
                                f"add on_stale {event_name} when <cancelled condition> {{ ... }}",
                                f"or add a requires guard to evolve {event_name}",
                            ],
                            assumptions=assumptions,
                        ))

    for effect in domain.effects:
        if effect.async_effect and not effect.correlation_id:
            findings.append(_finding(
                "uncorrelated_async_completion",
                "error",
                domain.name,
                effect=effect.name,
                failed_rule="completion_requires_request",
                guarantee_kind="structural",
                witness={"effect": effect.name, "correlation_id": None},
                repair=[f"add correlation_id <RequestEvent>.<field> to effect {effect.name}"],
                assumptions=assumptions,
            ))
        if effect.irreversible and not effect.idempotency_key:
            findings.append(_finding(
                "irreversible_effect_without_idempotency_key",
                "error",
                domain.name,
                effect=effect.name,
                failed_rule="idempotency_for_irreversible_effect",
                guarantee_kind="structural",
                witness={"effect": effect.name, "irreversible": True},
                repair=[f"add idempotency_key <stable key> to irreversible effect {effect.name}"],
                assumptions=assumptions,
            ))
        if effect.async_effect and not effect.timeout_event and not effect.retry.max_attempts:
            findings.append(_finding(
                "pending_effect_without_timeout_or_fallback",
                "warning",
                domain.name,
                effect=effect.name,
                failed_rule="timeout_or_fallback_for_pending_effect",
                guarantee_kind="structural",
                witness={"effect": effect.name},
                repair=[f"add timeout after <finite model tick> emits <TimedOutEvent> or retry {{ max_attempts N }}"],
                assumptions=assumptions,
            ))
        if effect.irreversible and not effect.compensation_events and not _saga_compensates_effect(domain, effect):
            findings.append(_finding(
                "missing_compensation_for_irreversible_effect",
                "warning",
                domain.name,
                effect=effect.name,
                failed_rule="irreversible_effect_has_compensation_or_acceptance",
                guarantee_kind="structural",
                witness={"effect": effect.name, "irreversible": True},
                repair=[
                    f"add compensation {{ emits <CompensationEvent> }} to effect {effect.name}",
                    "or add a saga compensation block for failure/timeout outcomes",
                ],
                assumptions=assumptions,
            ))
        if effect.reliable and not (effect.outbox or any(saga.outboxes for saga in domain.sagas)):
            findings.append(_finding(
                "reliable_effect_without_outbox_boundary",
                "warning",
                domain.name,
                effect=effect.name,
                failed_rule="reliable_effect_has_outbox_boundary",
                guarantee_kind="structural",
                witness={"effect": effect.name, "reliable": True},
                repair=[f"add outbox <OutboxName> to effect {effect.name} or the owning saga"],
                assumptions=assumptions,
            ))

    for saga in domain.sagas:
        if not saga.steps:
            findings.append(_finding(
                "saga_dead_end",
                "warning",
                domain.name,
                saga=saga.name,
                failed_rule="saga_has_progress_step",
                guarantee_kind="structural",
                witness={"saga": saga.name, "steps": []},
                repair=[f"add step <Name> {{ emits <Event> }} to saga {saga.name}"],
                assumptions=assumptions,
            ))
        for step in saga.steps:
            if step.async_step and not step.awaits and not step.timeout_event:
                findings.append(_finding(
                    "saga_dead_end",
                    "warning",
                    domain.name,
                    saga=saga.name,
                    step=step.name,
                    failed_rule="async_saga_step_has_terminal_observation",
                    guarantee_kind="structural",
                    witness={"step": step.name, "async": True},
                    repair=[f"add awaits one_of [...] or timeout after <duration> emits <Event> to step {step.name}"],
                    assumptions=assumptions,
                ))
        for cycle in _saga_wait_cycles(saga):
            findings.append(_finding(
                "process_wait_cycle",
                "error",
                domain.name,
                saga=saga.name,
                failed_rule="saga_wait_graph_is_acyclic",
                guarantee_kind="structural",
                witness={"cycle": list(cycle)},
                repair=["break the wait cycle by changing awaits/requires or adding an external completion event"],
                assumptions=assumptions,
            ))
    return findings


def _saga_compensates_effect(domain: DomainSpec, effect: DomainEffect) -> bool:
    if effect.compensation_events:
        return True
    non_success = {event for event in effect.outcomes if _status_for_name(event) == "non_success"}
    if not non_success:
        return False
    for saga in domain.sagas:
        for compensation in saga.compensations:
            if compensation.trigger_event in non_success and compensation.emits:
                return True
    return False


def _saga_wait_cycles(saga: DomainSaga) -> List[Tuple[str, str]]:
    emitted_by = {}
    for step in saga.steps:
        for event in step.emits:
            emitted_by.setdefault(event, set()).add(step.name)
    waits_on = {}
    for step in saga.steps:
        deps = set()
        for event in step.awaits:
            deps.update(emitted_by.get(event, set()))
        for req in step.requires:
            if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", req.strip()):
                deps.update(emitted_by.get(req.strip(), set()))
        waits_on[step.name] = deps
    cycles = []
    for left, deps in waits_on.items():
        for right in deps:
            if left != right and left in waits_on.get(right, set()):
                cycles.append(tuple(sorted((left, right))))
    return sorted(set(cycles))


def _has_cancellation_like_state(aggregate: DomainAggregate, type_map: Dict[str, DomainType]) -> bool:
    for field in aggregate.state:
        ty = type_map.get(field.type_name)
        if ty and ty.kind == "enum":
            for member in ty.members:
                if member.lower() in {"cancelled", "canceled", "rejected", "rolledback", "rolled_back"}:
                    return True
    return False


def _status_for_name(event_name: str) -> str:
    lowered = event_name.lower()
    if "fail" in lowered or "timeout" in lowered or "timedout" in lowered or "cancel" in lowered:
        return "non_success"
    return "success"


def _finding(
        kind,
        severity,
        domain,
        failed_rule,
        guarantee_kind,
        witness,
        repair,
        assumptions,
        aggregate=None,
        command=None,
        event=None,
        effect=None,
        saga=None,
        step=None):
    out = {
        "schema_version": DOMAIN_FINDING_SCHEMA_VERSION,
        "fsl": DOMAIN_DIALECT_VERSION,
        "result": "violated",
        "kind": kind,
        "severity": severity,
        "domain": domain,
        "failed_rule": failed_rule,
        "guarantee_kind": guarantee_kind,
        "evidence": {
            "kind": "static_check",
            "formal_proof": False,
        },
        "witness": witness,
        "repair_candidates": [
            {"kind": "domain_model_change", "weakens_spec": False, "description": text}
            for text in repair
        ],
        "assumptions": assumptions,
    }
    if aggregate:
        out["aggregate"] = aggregate
    if command:
        out["command"] = command
    if event:
        out["event"] = event
    if effect:
        out["effect"] = effect
    if saga:
        out["saga"] = saga
    if step:
        out["step"] = step
    return out
