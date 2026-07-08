# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Typed IR for the fsl-ai dialects."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple


@dataclass(frozen=True)
class AiTool:
    name: str
    schema: Optional[str] = None
    irreversible: bool = False
    preconditions: Tuple[str, ...] = ()
    effect: Optional[str] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiAuthority:
    may_suggest: Tuple[str, ...] = ()
    may_execute: Tuple[str, ...] = ()
    requires_human_approval: Tuple[str, ...] = ()
    forbidden: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiFallback:
    reason: str
    target: str
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiHardCheck:
    rules: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiComponent:
    name: str
    model: Optional[str] = None
    prompt: Optional[str] = None
    input_schema: Optional[str] = None
    output_schema: Optional[str] = None
    tools: List[AiTool] = field(default_factory=list)
    authority: AiAuthority = field(default_factory=AiAuthority)
    fallback: List[AiFallback] = field(default_factory=list)
    check: AiHardCheck = field(default_factory=AiHardCheck)
    loc: Optional[dict] = None

    def tool_map(self) -> Dict[str, AiTool]:
        return {tool.name: tool for tool in self.tools}

    def approval_required_tools(self) -> set:
        explicit = set(self.authority.requires_human_approval)
        irreversible = {tool.name for tool in self.tools if tool.irreversible}
        return explicit | irreversible

    def executable_tools(self) -> set:
        return set(self.authority.may_execute) | set(self.authority.requires_human_approval)

    def suggestible_tools(self) -> set:
        return set(self.authority.may_suggest)


@dataclass(frozen=True)
class AiAgentGrant:
    kind: str
    names: Tuple[str, ...]
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiAgentOutput:
    name: str
    visibility: Tuple[str, ...]
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiDelegationEdge:
    source: str
    target: str
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiFailurePolicy:
    agent: str
    condition: str
    action: str
    target: Optional[str] = None
    retry_limit: Optional[int] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiAgentContract:
    hard_rules: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiAgent:
    name: str
    model: Optional[str] = None
    prompt: Optional[str] = None
    context: Tuple[str, ...] = ()
    tool_names: Tuple[str, ...] = ()
    tools: List[AiTool] = field(default_factory=list)
    authority: AiAuthority = field(default_factory=AiAuthority)
    grants: List[AiAgentGrant] = field(default_factory=list)
    outputs: List[AiAgentOutput] = field(default_factory=list)
    orchestration: List[AiDelegationEdge] = field(default_factory=list)
    failure_policy: List[AiFailurePolicy] = field(default_factory=list)
    contracts: List[AiAgentContract] = field(default_factory=list)
    children: List["AiAgent"] = field(default_factory=list)
    trust: Optional[str] = None
    review_gates: Tuple[str, ...] = ()
    loc: Optional[dict] = None

    def tool_map(self) -> Dict[str, AiTool]:
        declared = {name: AiTool(name=name) for name in self.tool_names}
        declared.update({tool.name: tool for tool in self.tools})
        return declared

    def all_tool_names(self) -> set:
        return set(self.tool_names) | {tool.name for tool in self.tools}

    def authority_names(self) -> set:
        return (
            set(self.authority.may_suggest)
            | set(self.authority.may_execute)
            | set(self.authority.requires_human_approval)
        )

    def executable_tools(self) -> set:
        return set(self.authority.may_execute) | set(self.authority.requires_human_approval)

    def approval_required_tools(self) -> set:
        explicit = set(self.authority.requires_human_approval)
        irreversible = {tool.name for tool in self.tools if tool.irreversible}
        return explicit | irreversible
