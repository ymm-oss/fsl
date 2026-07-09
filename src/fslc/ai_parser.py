# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Parser for the fsl-ai dialects."""
from __future__ import annotations

from lark import Lark, Transformer, v_args
from lark.exceptions import UnexpectedInput

from .ai_ir import (
    AiAgent,
    AiAgentContract,
    AiAgentGrant,
    AiAgentOutput,
    AiAuthority,
    AiComponent,
    AiDelegationEdge,
    AiFailurePolicy,
    AiFallback,
    AiHardCheck,
    AiTool,
)
from .model import FslError


AI_GRAMMAR = r"""
start: ai_source

?ai_source: ai_component | agent_def

ai_component: "ai_component" NAME "{" component_item* "}"
?component_item: model_def | prompt_def | retriever_def | temperature_def
               | input_def | output_def | tools_def
               | tool_def | authority_def | fallback_def | check_def

agent_def: "agent" NAME "{" agent_item* "}"
?agent_item: model_def | prompt_def | context_def | tools_def | tool_def
           | authority_def | grant_def | agent_output_def | orchestration_def
           | failure_policy_def | contract_def | trust_def | review_gate_def
           | agent_def

model_def: "model" atom ";"?
prompt_def: "prompt" atom ";"?
retriever_def: "retriever" atom ";"?
temperature_def: "temperature" NUMBER ";"?
input_def: "input" atom ";"?
output_def: "output" atom ";"?
context_def: "context" names ";"?
tools_def: "tools" names ";"?
trust_def: "trust" NAME ";"?
review_gate_def: "review_gate" NAME ";"?

tool_def: "tool" NAME tool_attr* "{" tool_item* "}"
?tool_attr: "irreversible" -> tool_irreversible
?tool_item: tool_schema | tool_precondition | tool_effect
tool_schema: "schema" atom ";"?
tool_precondition: "precondition" NAME ";"?
tool_effect: "effect" NAME ";"?

authority_def: "authority" NAME? "{" authority_item* "}"
?authority_item: auth_may_suggest | auth_may_execute | auth_requires_human_approval | auth_forbidden
auth_may_suggest: "may_suggest" names ";"?
auth_may_execute: "may_execute" names ";"?
auth_requires_human_approval: "requires_human_approval" names ";"?
auth_forbidden: "forbidden" names ";"?

grant_def: "grant" grant_kind names ";"?
?grant_kind: "authority" -> grant_authority_kind
           | "context" -> grant_context_kind

agent_output_def: "output" NAME "visibility" names ";"?

orchestration_def: "orchestration" "{" orchestration_item* "}"
?orchestration_item: delegation_edge
delegation_edge: NAME ARROW NAME ";"?

failure_policy_def: "failure_policy" "{" failure_policy_item* "}"
failure_policy_item: "when" agent_event ARROW failure_action ";"?
agent_event: NAME "." NAME
?failure_action: retry_action | NAME -> failure_target
retry_action: "retry" "up_to" INT

contract_def: "contract" "{" contract_item* "}"
?contract_item: contract_hard | contract_rule
contract_hard: "hard" "{" contract_rule* "}"
contract_rule: "rule" NAME ";"?

fallback_def: "fallback" "{" fallback_item* "}"
fallback_item: "when" NAME "require" NAME ";"?

check_def: "check" "hard" "{" check_item* "}"
check_item: "rule" NAME ";"?

?names: name_list | bracket_name_list
name_list: NAME ("," NAME)* ","?
bracket_name_list: "[" name_list "]"
?atom: NAME -> atom_name
     | STRING -> atom_string

ARROW: "->"
NAME: /[a-zA-Z_][a-zA-Z_0-9]*/
STRING: /"[^"]*"/
COMMENT: /\/\/[^\n]*/
%import common.INT
%import common.NUMBER
%import common.WS
%ignore WS
%ignore COMMENT
"""


def _loc(meta):
    if meta is None:
        return None
    return {"line": meta.line, "column": meta.column}


def _unquote(text):
    raw = str(text)
    return raw[1:-1] if len(raw) >= 2 and raw[0] == '"' and raw[-1] == '"' else raw


@v_args(inline=True, meta=True)
class AiAst(Transformer):
    def NAME(self, *args):
        return str(args[-1])

    def STRING(self, *args):
        return _unquote(args[-1])

    def atom_name(self, meta, name):
        return name

    def atom_string(self, meta, value):
        return value

    def model_def(self, meta, value):
        return ("model", value, _loc(meta))

    def prompt_def(self, meta, value):
        return ("prompt", value, _loc(meta))

    def retriever_def(self, meta, value):
        return ("retriever", value, _loc(meta))

    def temperature_def(self, meta, value):
        return ("temperature", float(value), _loc(meta))

    def input_def(self, meta, value):
        return ("input", value, _loc(meta))

    def output_def(self, meta, value):
        return ("output", value, _loc(meta))

    def tool_irreversible(self, meta):
        return ("irreversible", True)

    def tool_schema(self, meta, value):
        return ("schema", value, _loc(meta))

    def tool_precondition(self, meta, name):
        return ("precondition", name, _loc(meta))

    def tool_effect(self, meta, name):
        return ("effect", name, _loc(meta))

    def tool_def(self, meta, name, *parts):
        irreversible = False
        schema = None
        effect = None
        preconditions = []
        for part in parts:
            if part[0] == "irreversible":
                irreversible = True
            elif part[0] == "schema":
                if schema is not None:
                    raise FslError(f"tool '{name}' declares schema more than once", loc=part[2])
                schema = part[1]
            elif part[0] == "precondition":
                preconditions.append(part[1])
            elif part[0] == "effect":
                if effect is not None:
                    raise FslError(f"tool '{name}' declares effect more than once", loc=part[2])
                effect = part[1]
        return AiTool(
            name=name,
            schema=schema,
            irreversible=irreversible,
            preconditions=tuple(preconditions),
            effect=effect,
            loc=_loc(meta),
        )

    def name_list(self, meta, *names):
        return tuple(names)

    def bracket_name_list(self, meta, names):
        return tuple(names)

    def auth_may_suggest(self, meta, names):
        return ("may_suggest", names, _loc(meta))

    def auth_may_execute(self, meta, names):
        return ("may_execute", names, _loc(meta))

    def auth_requires_human_approval(self, meta, names):
        return ("requires_human_approval", names, _loc(meta))

    def auth_forbidden(self, meta, names):
        return ("forbidden", names, _loc(meta))

    def authority_def(self, meta, *parts):
        buckets = {
            "may_suggest": [],
            "may_execute": [],
            "requires_human_approval": [],
            "forbidden": [],
        }
        loc = _loc(meta)
        for part in parts:
            if isinstance(part, str):
                continue
            buckets[part[0]].extend(part[1])
        return AiAuthority(
            may_suggest=tuple(buckets["may_suggest"]),
            may_execute=tuple(buckets["may_execute"]),
            requires_human_approval=tuple(buckets["requires_human_approval"]),
            forbidden=tuple(buckets["forbidden"]),
            loc=loc,
        )

    def context_def(self, meta, names):
        return ("context", tuple(names), _loc(meta))

    def tools_def(self, meta, names):
        return ("tools", tuple(names), _loc(meta))

    def trust_def(self, meta, name):
        return ("trust", name, _loc(meta))

    def review_gate_def(self, meta, name):
        return ("review_gate", name, _loc(meta))

    def grant_authority_kind(self, meta):
        return "authority"

    def grant_context_kind(self, meta):
        return "context"

    def grant_def(self, meta, kind, names):
        return AiAgentGrant(kind=kind, names=tuple(names), loc=_loc(meta))

    def agent_output_def(self, meta, name, visibility):
        return AiAgentOutput(name=name, visibility=tuple(visibility), loc=_loc(meta))

    def delegation_edge(self, meta, source, _arrow, target):
        return AiDelegationEdge(source=source, target=target, loc=_loc(meta))

    def orchestration_def(self, meta, *edges):
        return ("orchestration", list(edges), _loc(meta))

    def agent_event(self, meta, agent, condition):
        return (agent, condition)

    def retry_action(self, meta, limit):
        return ("retry", int(limit), None)

    def failure_target(self, meta, target):
        return ("target", None, target)

    def failure_policy_item(self, meta, event, _arrow, action):
        agent, condition = event
        action_kind, retry_limit, target = action
        return AiFailurePolicy(
            agent=agent,
            condition=condition,
            action=action_kind,
            target=target,
            retry_limit=retry_limit,
            loc=_loc(meta),
        )

    def failure_policy_def(self, meta, *items):
        return ("failure_policy", list(items), _loc(meta))

    def contract_rule(self, meta, name):
        return name

    def contract_hard(self, meta, *rules):
        return tuple(rules)

    def contract_def(self, meta, *items):
        rules = []
        for item in items:
            if isinstance(item, tuple):
                rules.extend(item)
            else:
                rules.append(item)
        return AiAgentContract(hard_rules=tuple(rules), loc=_loc(meta))

    def fallback_item(self, meta, reason, target):
        return AiFallback(reason=reason, target=target, loc=_loc(meta))

    def fallback_def(self, meta, *items):
        return ("fallback", list(items), _loc(meta))

    def check_item(self, meta, name):
        return name

    def check_def(self, meta, *rules):
        return AiHardCheck(tuple(rules), _loc(meta))

    def ai_component(self, meta, name, *items):
        model = None
        prompt = None
        retriever = None
        temperature = None
        input_schema = None
        output_schema = None
        tools = []
        authority = AiAuthority()
        fallback = []
        check = AiHardCheck()
        seen_authority = False
        seen_check = False

        for item in items:
            if isinstance(item, AiTool):
                tools.append(item)
            elif isinstance(item, AiAuthority):
                if seen_authority:
                    raise FslError("ai_component may declare authority at most once", loc=item.loc)
                authority = item
                seen_authority = True
            elif isinstance(item, AiHardCheck):
                if seen_check:
                    raise FslError("ai_component may declare check hard at most once", loc=item.loc)
                check = item
                seen_check = True
            elif isinstance(item, tuple) and item[0] == "fallback":
                fallback.extend(item[1])
            elif isinstance(item, tuple) and item[0] == "model":
                if model is not None:
                    raise FslError("ai_component may declare model at most once", loc=item[2])
                model = item[1]
            elif isinstance(item, tuple) and item[0] == "prompt":
                if prompt is not None:
                    raise FslError("ai_component may declare prompt at most once", loc=item[2])
                prompt = item[1]
            elif isinstance(item, tuple) and item[0] == "retriever":
                if retriever is not None:
                    raise FslError("ai_component may declare retriever at most once", loc=item[2])
                retriever = item[1]
            elif isinstance(item, tuple) and item[0] == "temperature":
                if temperature is not None:
                    raise FslError("ai_component may declare temperature at most once", loc=item[2])
                temperature = item[1]
            elif isinstance(item, tuple) and item[0] == "tools":
                tools.extend(AiTool(name=name) for name in item[1])
            elif isinstance(item, tuple) and item[0] == "input":
                if input_schema is not None:
                    raise FslError("ai_component may declare input at most once", loc=item[2])
                input_schema = item[1]
            elif isinstance(item, tuple) and item[0] == "output":
                if output_schema is not None:
                    raise FslError("ai_component may declare output at most once", loc=item[2])
                output_schema = item[1]

        return AiComponent(
            name=name,
            model=model,
            prompt=prompt,
            retriever=retriever,
            temperature=temperature,
            input_schema=input_schema,
            output_schema=output_schema,
            tools=tools,
            authority=authority,
            fallback=fallback,
            check=check,
            loc=_loc(meta),
        )

    def agent_def(self, meta, name, *items):
        model = None
        prompt = None
        context = ()
        tool_names = ()
        tools = []
        authority = AiAuthority()
        grants = []
        outputs = []
        orchestration = []
        failure_policy = []
        contracts = []
        children = []
        trust = None
        review_gates = []
        seen_authority = False
        seen_context = False
        seen_tools = False
        seen_orchestration = False
        seen_failure_policy = False

        for item in items:
            if isinstance(item, AiAgent):
                children.append(item)
            elif isinstance(item, AiTool):
                tools.append(item)
            elif isinstance(item, AiAuthority):
                if seen_authority:
                    raise FslError("agent may declare authority at most once", loc=item.loc)
                authority = item
                seen_authority = True
            elif isinstance(item, AiAgentGrant):
                grants.append(item)
            elif isinstance(item, AiAgentOutput):
                outputs.append(item)
            elif isinstance(item, AiDelegationEdge):
                orchestration.append(item)
            elif isinstance(item, AiFailurePolicy):
                failure_policy.append(item)
            elif isinstance(item, AiAgentContract):
                contracts.append(item)
            elif isinstance(item, tuple) and item[0] == "model":
                if model is not None:
                    raise FslError("agent may declare model at most once", loc=item[2])
                model = item[1]
            elif isinstance(item, tuple) and item[0] == "prompt":
                if prompt is not None:
                    raise FslError("agent may declare prompt at most once", loc=item[2])
                prompt = item[1]
            elif isinstance(item, tuple) and item[0] == "context":
                if seen_context:
                    raise FslError("agent may declare context at most once", loc=item[2])
                context = item[1]
                seen_context = True
            elif isinstance(item, tuple) and item[0] == "tools":
                if seen_tools:
                    raise FslError("agent may declare tools at most once", loc=item[2])
                tool_names = item[1]
                seen_tools = True
            elif isinstance(item, tuple) and item[0] == "orchestration":
                if seen_orchestration:
                    raise FslError("agent may declare orchestration at most once", loc=item[2])
                orchestration.extend(item[1])
                seen_orchestration = True
            elif isinstance(item, tuple) and item[0] == "failure_policy":
                if seen_failure_policy:
                    raise FslError("agent may declare failure_policy at most once", loc=item[2])
                failure_policy.extend(item[1])
                seen_failure_policy = True
            elif isinstance(item, tuple) and item[0] == "trust":
                if trust is not None:
                    raise FslError("agent may declare trust at most once", loc=item[2])
                trust = item[1]
            elif isinstance(item, tuple) and item[0] == "review_gate":
                review_gates.append(item[1])

        return AiAgent(
            name=name,
            model=model,
            prompt=prompt,
            context=tuple(context),
            tool_names=tuple(tool_names),
            tools=tools,
            authority=authority,
            grants=grants,
            outputs=outputs,
            orchestration=orchestration,
            failure_policy=failure_policy,
            contracts=contracts,
            children=children,
            trust=trust,
            review_gates=tuple(review_gates),
            loc=_loc(meta),
        )

    def start(self, meta, child):
        return child


AI_PARSER = Lark(
    AI_GRAMMAR,
    parser="lalr",
    maybe_placeholders=False,
    propagate_positions=True,
)


def is_ai_component_source(src):
    return src.lstrip().startswith("ai_component")


def is_ai_agent_source(src):
    return src.lstrip().startswith("agent")


def is_ai_source(src):
    stripped = src.lstrip()
    return stripped.startswith("ai_component") or stripped.startswith("agent")


def parse_ai_component(src):
    parsed = parse_ai_source(src)
    if not isinstance(parsed, AiComponent):
        raise FslError("expected ai_component source", kind="semantics")
    return parsed


def parse_ai_source(src):
    try:
        tree = AI_PARSER.parse(src)
    except UnexpectedInput as e:
        e.source = src
        raise
    return AiAst().transform(tree)
