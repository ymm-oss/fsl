# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Lenient parser for fsl-ai project-level evidence declarations.

The hard-contract ``ai_component`` and recursive ``agent`` dialects keep their
strict Lark parser because they lower to kernel or graph checks. The stochastic,
migration, and observed-evidence declarations are external evidence jobs: their
expressions are labels and threshold clauses, not kernel formulas. This module
therefore parses the proposal's project-level blocks into a small typed metadata
model without trying to prove semantic NLP claims.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Dict, Iterable, List, Optional, Tuple

from .ai_ir import AiComponent
from .ai_parser import parse_ai_component
from .model import FslError


PROJECT_BLOCKS = {
    "ai_action",
    "ai_component",
    "ai_contract",
    "ai_migration",
    "authority",
    "dataset",
    "evaluator",
    "failure_mode",
    "observed_property",
    "retriever",
    "statistical_property",
    "trust_boundary",
}


@dataclass(frozen=True)
class AiMetricRequirement:
    kind: str
    metric: Optional[str] = None
    confidence: Optional[float] = None
    comparator: Optional[str] = None
    threshold: Optional[float] = None
    slice: str = "all"
    min_samples: Optional[int] = None
    condition: Optional[str] = None
    source: str = ""
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiRegressionRequirement:
    metric: str
    direction: str
    comparator: str
    threshold: float
    dataset: Optional[str] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiObservedRequirement:
    kind: str
    metric: str
    comparator: str
    threshold: float
    compared_to: Optional[str] = None
    slice: str = "all"
    source: str = ""
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiDataset:
    name: str
    source: Optional[str] = None
    slices: Dict[str, Tuple[str, ...]] = field(default_factory=dict)
    population: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiEvaluator:
    name: str
    inputs: Tuple[str, ...] = ()
    outputs: Tuple[str, ...] = ()
    calibration_dataset: Optional[str] = None
    calibration_requirements: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiFailureMode:
    name: str
    condition: Optional[str] = None
    severity: Optional[str] = None
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiStatisticalProperty:
    name: str
    target: Optional[str] = None
    dataset: Optional[str] = None
    evaluator: Optional[str] = None
    confidence: float = 0.95
    requirements: Tuple[AiMetricRequirement, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiObservedProperty:
    name: str
    target: Optional[str] = None
    source: Optional[str] = None
    window: Optional[str] = None
    requirements: Tuple[AiObservedRequirement, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiMigrationEndpoint:
    component: str
    model: Optional[str] = None
    prompt: Optional[str] = None
    retriever: Optional[str] = None
    tools: Tuple[str, ...] = ()


@dataclass(frozen=True)
class AiMigration:
    name: str
    from_endpoint: Optional[AiMigrationEndpoint] = None
    to_endpoint: Optional[AiMigrationEndpoint] = None
    hard_contracts: Tuple[str, ...] = ()
    regression_requirements: Tuple[AiRegressionRequirement, ...] = ()
    compatibility_requirements: Tuple[str, ...] = ()
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiRawBlock:
    kind: str
    name: str
    body: str
    loc: Optional[dict] = None


@dataclass(frozen=True)
class AiProject:
    name: str
    components: Tuple[AiComponent, ...] = ()
    datasets: Tuple[AiDataset, ...] = ()
    evaluators: Tuple[AiEvaluator, ...] = ()
    failure_modes: Tuple[AiFailureMode, ...] = ()
    statistical_properties: Tuple[AiStatisticalProperty, ...] = ()
    observed_properties: Tuple[AiObservedProperty, ...] = ()
    migrations: Tuple[AiMigration, ...] = ()
    raw_blocks: Tuple[AiRawBlock, ...] = ()

    def component_map(self) -> Dict[str, AiComponent]:
        return {component.name: component for component in self.components}

    def dataset_map(self) -> Dict[str, AiDataset]:
        return {dataset.name: dataset for dataset in self.datasets}

    def statistical_property_map(self) -> Dict[str, AiStatisticalProperty]:
        return {prop.name: prop for prop in self.statistical_properties}

    def observed_property_map(self) -> Dict[str, AiObservedProperty]:
        return {prop.name: prop for prop in self.observed_properties}

    def migration_map(self) -> Dict[str, AiMigration]:
        return {migration.name: migration for migration in self.migrations}


@dataclass(frozen=True)
class _Block:
    kind: str
    name: str
    body: str
    text: str
    line: int
    column: int


def is_ai_project_source(src: str) -> bool:
    from .dialect_registry import inspect_source

    parser_source = inspect_source(src).source
    stripped = parser_source.lstrip()
    if not stripped:
        return False
    first = re.match(r"([A-Za-z_][A-Za-z0-9_]*)\b", stripped)
    if not first or first.group(1) not in PROJECT_BLOCKS:
        return False
    blocks = _top_blocks(parser_source)
    return len(blocks) != 1 or blocks[0].kind != "ai_component"


def parse_ai_project(src: str, name: str = "AiProject") -> AiProject:
    blocks = _top_blocks(src)
    if not blocks:
        raise FslError("expected fsl-ai project declarations", kind="parse")

    components: List[AiComponent] = []
    datasets: List[AiDataset] = []
    evaluators: List[AiEvaluator] = []
    failure_modes: List[AiFailureMode] = []
    statistical_properties: List[AiStatisticalProperty] = []
    observed_properties: List[AiObservedProperty] = []
    migrations: List[AiMigration] = []
    raw_blocks: List[AiRawBlock] = []

    for block in blocks:
        loc = {"line": block.line, "column": block.column}
        if block.kind == "ai_component":
            components.append(parse_ai_component(block.text))
        elif block.kind == "dataset":
            datasets.append(_parse_dataset(block))
        elif block.kind == "evaluator":
            evaluators.append(_parse_evaluator(block))
        elif block.kind == "failure_mode":
            failure_modes.append(_parse_failure_mode(block))
        elif block.kind == "statistical_property":
            statistical_properties.append(_parse_statistical_property(block))
        elif block.kind == "observed_property":
            observed_properties.append(_parse_observed_property(block))
        elif block.kind == "ai_migration":
            migrations.append(_parse_migration(block))
        else:
            raw_blocks.append(AiRawBlock(block.kind, block.name, block.body, loc))

    _reject_duplicates([item.name for item in datasets], "dataset")
    _reject_duplicates([item.name for item in evaluators], "evaluator")
    _reject_duplicates([item.name for item in statistical_properties], "statistical_property")
    _reject_duplicates([item.name for item in observed_properties], "observed_property")
    _reject_duplicates([item.name for item in migrations], "ai_migration")

    return AiProject(
        name=name,
        components=tuple(components),
        datasets=tuple(datasets),
        evaluators=tuple(evaluators),
        failure_modes=tuple(failure_modes),
        statistical_properties=tuple(statistical_properties),
        observed_properties=tuple(observed_properties),
        migrations=tuple(migrations),
        raw_blocks=tuple(raw_blocks),
    )


def analyze_ai_project(project: AiProject) -> dict:
    return {
        "result": "ai_project_analyzed",
        "dialect": "fsl-ai-project.v0",
        "formal_result": "not_run",
        "ai_project": project.name,
        "components": [component.name for component in project.components],
        "datasets": [dataset.name for dataset in project.datasets],
        "evaluators": [evaluator.name for evaluator in project.evaluators],
        "failure_modes": [mode.name for mode in project.failure_modes],
        "statistical_properties": [prop.name for prop in project.statistical_properties],
        "observed_properties": [prop.name for prop in project.observed_properties],
        "migrations": [migration.name for migration in project.migrations],
        "raw_blocks": [
            {"kind": block.kind, "name": block.name}
            for block in project.raw_blocks
        ],
        "assumptions": [{
            "id": "AI-ASSUME-EXTERNAL-EVIDENCE-JOBS",
            "text": (
                "statistical, migration, and observed AI declarations are external "
                "evidence jobs and do not add probability semantics to fslc verify"
            ),
        }],
    }


def _top_blocks(src: str) -> List[_Block]:
    blocks: List[_Block] = []
    i = 0
    n = len(src)
    while i < n:
        match = re.search(
            r"\b([A-Za-z_][A-Za-z0-9_]*)(?:\s+([A-Za-z_][A-Za-z0-9_]*))?\s*\{",
            src[i:],
        )
        if not match:
            break
        start = i + match.start()
        kind, name = match.group(1), match.group(2) or ""
        if kind not in PROJECT_BLOCKS and kind not in {"slice", "from", "to", "preserve", "no_regression", "compatibility", "calibration", "population"}:
            i = start + 1
            continue
        if kind in PROJECT_BLOCKS and not name:
            i = start + 1
            continue
        brace = i + match.end() - 1
        end = _matching_brace(src, brace)
        if end is None:
            raise FslError(f"unterminated {kind} '{name}' block", kind="parse",
                           loc=_loc_for_offset(src, start))
        if _brace_depth(src[:start]) == 0:
            line, col = _line_col(src, start)
            blocks.append(_Block(
                kind=kind,
                name=name,
                body=src[brace + 1:end],
                text=src[start:end + 1],
                line=line,
                column=col,
            ))
        i = end + 1
    return blocks


def _child_blocks(body: str) -> List[_Block]:
    return _top_blocks(body)


def _matching_brace(src: str, brace: int) -> Optional[int]:
    depth = 0
    in_string = False
    escaped = False
    for i in range(brace, len(src)):
        ch = src[i]
        if in_string:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                in_string = False
            continue
        if ch == '"':
            in_string = True
        elif ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return i
    return None


def _brace_depth(src: str) -> int:
    depth = 0
    in_string = False
    escaped = False
    for ch in src:
        if in_string:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                in_string = False
            continue
        if ch == '"':
            in_string = True
        elif ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
    return depth


def _line_col(src: str, offset: int) -> Tuple[int, int]:
    line = src.count("\n", 0, offset) + 1
    last = src.rfind("\n", 0, offset)
    col = offset + 1 if last < 0 else offset - last
    return line, col


def _loc_for_offset(src: str, offset: int) -> dict:
    line, col = _line_col(src, offset)
    return {"line": line, "column": col}


def _top_lines(body: str) -> List[str]:
    lines = []
    depth = 0
    for raw in body.splitlines():
        line = _strip_comment(raw).strip()
        if not line:
            continue
        before = depth
        depth += line.count("{") - line.count("}")
        if before == 0 and "{" not in line and "}" not in line:
            lines.append(_strip_semi(line))
    return lines


def _strip_comment(line: str) -> str:
    in_string = False
    escaped = False
    for idx in range(len(line) - 1):
        ch = line[idx]
        if in_string:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                in_string = False
        elif ch == '"':
            in_string = True
        elif ch == "/" and line[idx + 1] == "/":
            return line[:idx]
    return line


def _strip_semi(line: str) -> str:
    return line[:-1].strip() if line.endswith(";") else line


def _atom(text: str) -> str:
    text = _strip_semi(text.strip())
    if len(text) >= 2 and text[0] == '"' and text[-1] == '"':
        return text[1:-1]
    return text


def _parse_dataset(block: _Block) -> AiDataset:
    source = None
    population = []
    slices: Dict[str, Tuple[str, ...]] = {}
    for line in _top_lines(block.body):
        if line.startswith("source "):
            source = _atom(line[len("source "):])
    for child in _child_blocks(block.body):
        if child.kind == "population":
            population.extend(_top_lines(child.body))
        elif child.kind == "slice":
            slices[child.name] = tuple(_top_lines(child.body))
    return AiDataset(
        name=block.name,
        source=source,
        population=tuple(population),
        slices=slices,
        loc={"line": block.line, "column": block.column},
    )


def _parse_evaluator(block: _Block) -> AiEvaluator:
    inputs = []
    outputs = []
    calibration_dataset = None
    calibration_requirements = []
    for line in _top_lines(block.body):
        if line.startswith("input "):
            inputs.append(line[len("input "):])
        elif line.startswith("output "):
            outputs.append(line[len("output "):])
    for child in _child_blocks(block.body):
        if child.kind != "calibration":
            continue
        for line in _top_lines(child.body):
            if line.startswith("dataset "):
                calibration_dataset = _atom(line[len("dataset "):])
            elif line.startswith("require "):
                calibration_requirements.append(line[len("require "):])
    return AiEvaluator(
        name=block.name,
        inputs=tuple(inputs),
        outputs=tuple(outputs),
        calibration_dataset=calibration_dataset,
        calibration_requirements=tuple(calibration_requirements),
        loc={"line": block.line, "column": block.column},
    )


def _parse_failure_mode(block: _Block) -> AiFailureMode:
    condition = None
    severity = None
    for line in _top_lines(block.body):
        if line.startswith("condition "):
            condition = line[len("condition "):]
        elif line.startswith("severity "):
            severity = _atom(line[len("severity "):])
    return AiFailureMode(block.name, condition, severity, {"line": block.line, "column": block.column})


def _parse_statistical_property(block: _Block) -> AiStatisticalProperty:
    target = None
    dataset = None
    evaluator = None
    confidence = 0.95
    requirements: List[AiMetricRequirement] = []
    for line in _top_lines(block.body):
        if line.startswith("target "):
            target = _atom(line[len("target "):])
        elif line.startswith("dataset "):
            dataset = _atom(line[len("dataset "):])
        elif line.startswith("evaluator "):
            evaluator = _atom(line[len("evaluator "):])
        elif line.startswith("confidence "):
            confidence = float(_atom(line[len("confidence "):]))
        elif line.startswith("require "):
            requirements.append(_parse_metric_requirement(line, "all", confidence))
    for child in _child_blocks(block.body):
        if child.kind != "slice":
            continue
        for line in _top_lines(child.body):
            if line.startswith("require "):
                requirements.append(_parse_metric_requirement(line, child.name, confidence))
    return AiStatisticalProperty(
        name=block.name,
        target=target,
        dataset=dataset,
        evaluator=evaluator,
        confidence=confidence,
        requirements=tuple(requirements),
        loc={"line": block.line, "column": block.column},
    )


def _parse_metric_requirement(line: str, slice_name: str, default_confidence: float) -> AiMetricRequirement:
    source = line
    expr = line[len("require "):].strip()
    loc = None
    min_match = re.fullmatch(r"min_samples\s*(>=|>|==|<=|<)\s*(\d+)", expr)
    if min_match:
        return AiMetricRequirement(
            kind="min_samples",
            comparator=min_match.group(1),
            threshold=float(min_match.group(2)),
            slice=slice_name,
            min_samples=int(min_match.group(2)),
            source=source,
            loc=loc,
        )
    ci_match = re.fullmatch(
        r"(ci_lower|ci_upper)\s*\(\s*([A-Za-z_][A-Za-z0-9_.]*)\s*,\s*([0-9.]+)\s*\)\s*(>=|>|<=|<|==)\s*([0-9.]+)",
        expr,
    )
    if ci_match:
        metric = _metric_name(ci_match.group(2))
        return AiMetricRequirement(
            kind=ci_match.group(1),
            metric=metric,
            confidence=float(ci_match.group(3)),
            comparator=ci_match.group(4),
            threshold=float(ci_match.group(5)),
            slice=slice_name,
            source=source,
            loc=loc,
        )
    prob_match = re.fullmatch(r"P\s*\(\s*([^)]+)\s*\)\s*(<=|<|>=|>|==)\s*([0-9.]+)", expr)
    if prob_match:
        metric = prob_match.group(1).split("|", 1)[0].strip()
        condition = prob_match.group(1).split("|", 1)[1].strip() if "|" in prob_match.group(1) else None
        return AiMetricRequirement(
            kind="ci_upper" if prob_match.group(2) in ("<=", "<") else "ci_lower",
            metric=_metric_name(metric),
            confidence=default_confidence,
            comparator=prob_match.group(2),
            threshold=float(prob_match.group(3)),
            slice=slice_name,
            condition=condition,
            source=source,
            loc=loc,
        )
    point_match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_.]*)\s*(>=|>|<=|<|==)\s*([0-9.]+)", expr)
    if point_match:
        return AiMetricRequirement(
            kind="point_estimate",
            metric=_metric_name(point_match.group(1)),
            comparator=point_match.group(2),
            threshold=float(point_match.group(3)),
            slice=slice_name,
            source=source,
            loc=loc,
        )
    return AiMetricRequirement(kind="inconclusive", slice=slice_name, source=source, loc=loc)


def _parse_observed_property(block: _Block) -> AiObservedProperty:
    target = None
    source_name = None
    window = None
    requirements: List[AiObservedRequirement] = []
    for line in _top_lines(block.body):
        if line.startswith("target "):
            target = _atom(line[len("target "):])
        elif line.startswith("source "):
            source_name = _atom(line[len("source "):])
        elif line.startswith("window "):
            window = _atom(line[len("window "):])
        elif line.startswith("require "):
            requirements.append(_parse_observed_requirement(line, "all"))
    for child in _child_blocks(block.body):
        if child.kind != "slice":
            continue
        for line in _top_lines(child.body):
            if line.startswith("require "):
                requirements.append(_parse_observed_requirement(line, child.name))
    return AiObservedProperty(
        name=block.name,
        target=target,
        source=source_name,
        window=window,
        requirements=tuple(requirements),
        loc={"line": block.line, "column": block.column},
    )


def _parse_observed_requirement(line: str, slice_name: str) -> AiObservedRequirement:
    expr = line[len("require "):].strip()
    observed = re.fullmatch(
        r"observed\s*\(\s*([A-Za-z_][A-Za-z0-9_.]*)\s*\)\s*(>=|>|<=|<|==)\s*([0-9.]+)",
        expr,
    )
    if observed:
        return AiObservedRequirement(
            kind="observed",
            metric=_metric_name(observed.group(1)),
            comparator=observed.group(2),
            threshold=float(observed.group(3)),
            slice=slice_name,
            source=line,
        )
    drift = re.fullmatch(
        r"drift\s*\(\s*([A-Za-z_][A-Za-z0-9_.]*)\s*\)\s*(>=|>|<=|<|==)\s*([0-9.]+)\s+compared_to\s+([A-Za-z_][A-Za-z0-9_.]*)",
        expr,
    )
    if drift:
        return AiObservedRequirement(
            kind="drift",
            metric=_metric_name(drift.group(1)),
            comparator=drift.group(2),
            threshold=float(drift.group(3)),
            compared_to=drift.group(4),
            slice=slice_name,
            source=line,
        )
    return AiObservedRequirement(
        kind="inconclusive",
        metric="unknown",
        comparator="==",
        threshold=0.0,
        slice=slice_name,
        source=line,
    )


def _parse_migration(block: _Block) -> AiMigration:
    from_endpoint = None
    to_endpoint = None
    hard_contracts = []
    regression_requirements: List[AiRegressionRequirement] = []
    compatibility_requirements = []
    for child in _child_blocks(block.body):
        if child.kind == "from":
            from_endpoint = _parse_endpoint(child)
        elif child.kind == "to":
            to_endpoint = _parse_endpoint(child)
        elif child.kind == "compatibility":
            compatibility_requirements.extend(
                line[len("require "):] for line in _top_lines(child.body)
                if line.startswith("require ")
            )
        elif child.kind == "preserve":
            for line in _top_lines(child.body):
                if line.startswith("hard_contract "):
                    hard_contracts.append(_atom(line[len("hard_contract "):]))
            for grandchild in _child_blocks(child.body):
                if grandchild.kind == "no_regression":
                    dataset = None
                    for line in _top_lines(grandchild.body):
                        if line.startswith("dataset "):
                            dataset = _atom(line[len("dataset "):])
                        elif line.startswith("metric "):
                            regression_requirements.append(_parse_regression_requirement(line, dataset))
    return AiMigration(
        name=block.name,
        from_endpoint=from_endpoint,
        to_endpoint=to_endpoint,
        hard_contracts=tuple(hard_contracts),
        regression_requirements=tuple(regression_requirements),
        compatibility_requirements=tuple(compatibility_requirements),
        loc={"line": block.line, "column": block.column},
    )


def _parse_endpoint(block: _Block) -> AiMigrationEndpoint:
    model = None
    prompt = None
    retriever = None
    tools: Tuple[str, ...] = ()
    for line in _top_lines(block.body):
        if line.startswith("model "):
            model = _atom(line[len("model "):])
        elif line.startswith("prompt "):
            prompt = _atom(line[len("prompt "):])
        elif line.startswith("retriever "):
            retriever = _atom(line[len("retriever "):])
        elif line.startswith("tools "):
            tools = _parse_names(_atom(line[len("tools "):]))
    return AiMigrationEndpoint(
        component=block.name,
        model=model,
        prompt=prompt,
        retriever=retriever,
        tools=tools,
    )


def _parse_regression_requirement(line: str, dataset: Optional[str]) -> AiRegressionRequirement:
    match = re.fullmatch(
        r"metric\s+([A-Za-z_][A-Za-z0-9_.]*)\s+(drop|increase)\s*(<=|<|>=|>|==)\s*([0-9.]+)",
        line,
    )
    if not match:
        raise FslError(f"unsupported no_regression metric clause: {line}", kind="semantics")
    return AiRegressionRequirement(
        metric=_metric_name(match.group(1)),
        direction=match.group(2),
        comparator=match.group(3),
        threshold=float(match.group(4)),
        dataset=dataset,
    )


def _parse_names(text: str) -> Tuple[str, ...]:
    stripped = text.strip()
    if stripped.startswith("[") and stripped.endswith("]"):
        stripped = stripped[1:-1]
    return tuple(name.strip() for name in stripped.split(",") if name.strip())


def _metric_name(text: str) -> str:
    text = text.strip()
    if text.startswith("metric."):
        return text[len("metric."):]
    return text


def _reject_duplicates(values: Iterable[str], label: str):
    seen = set()
    for value in values:
        if value in seen:
            raise FslError(f"duplicate {label} '{value}'", kind="semantics")
        seen.add(value)
