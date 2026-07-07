# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Typed IR for the fsl-ai hard-contract MVP dialect."""
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
