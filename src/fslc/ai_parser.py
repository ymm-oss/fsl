# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Parser for the fsl-ai hard-contract MVP dialect."""
from __future__ import annotations

from lark import Lark, Transformer, v_args
from lark.exceptions import UnexpectedInput

from .ai_ir import AiAuthority, AiComponent, AiFallback, AiHardCheck, AiTool
from .model import FslError


AI_GRAMMAR = r"""
start: ai_component

ai_component: "ai_component" NAME "{" component_item* "}"
?component_item: model_def | prompt_def | input_def | output_def
               | tool_def | authority_def | fallback_def | check_def

model_def: "model" atom ";"?
prompt_def: "prompt" atom ";"?
input_def: "input" atom ";"?
output_def: "output" atom ";"?

tool_def: "tool" NAME tool_attr* "{" tool_item* "}"
?tool_attr: "irreversible" -> tool_irreversible
?tool_item: tool_schema | tool_precondition | tool_effect
tool_schema: "schema" atom ";"?
tool_precondition: "precondition" NAME ";"?
tool_effect: "effect" NAME ";"?

authority_def: "authority" NAME? "{" authority_item* "}"
?authority_item: auth_may_suggest | auth_may_execute | auth_requires_human_approval | auth_forbidden
auth_may_suggest: "may_suggest" name_list ";"?
auth_may_execute: "may_execute" name_list ";"?
auth_requires_human_approval: "requires_human_approval" name_list ";"?
auth_forbidden: "forbidden" name_list ";"?

fallback_def: "fallback" "{" fallback_item* "}"
fallback_item: "when" NAME "require" NAME ";"?

check_def: "check" "hard" "{" check_item* "}"
check_item: "rule" NAME ";"?

name_list: NAME ("," NAME)* ","?
?atom: NAME -> atom_name
     | STRING -> atom_string

NAME: /[a-zA-Z_][a-zA-Z_0-9]*/
STRING: /"[^"]*"/
COMMENT: /\/\/[^\n]*/
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
            input_schema=input_schema,
            output_schema=output_schema,
            tools=tools,
            authority=authority,
            fallback=fallback,
            check=check,
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


def parse_ai_component(src):
    try:
        tree = AI_PARSER.parse(src)
    except UnexpectedInput as e:
        e.source = src
        raise
    return AiAst().transform(tree)
