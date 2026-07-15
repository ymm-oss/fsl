# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Declarative registry of dialects/example corpora (issue #167).

``tests/test_dialect_conformance.py`` scans every ``.fsl`` under
``SCAN_ROOTS`` and classifies it; this module is the data side of that
classification, not logic. A new dialect (or a new example directory) that
nobody registers here fails the conformance gate loudly, instead of the
corpus silently sitting outside the dual-evaluator safety net (the failure
mode the 2026-07-08 fsl-db audit found).
"""
from __future__ import annotations

from dataclasses import dataclass

SCAN_ROOTS = ("specs", "examples")


@dataclass(frozen=True)
class Dialect:
    construct: str  # the file's top-level keyword
    min_files: int  # glob-rot floor: the scan must keep finding at least this many
    depth: int = 4  # BFS/verify agreement bound for this dialect's files


# construct -> Dialect. "kernel" is the design layer's own top-level `spec`.
DIALECTS: dict[str, Dialect] = {
    "kernel": Dialect("spec", 60),
    "business": Dialect("business", 5),
    "requirements": Dialect("requirements", 25),
    "governance": Dialect("governance", 1),
    "compose": Dialect("compose", 2),
    "db": Dialect("dbsystem", 15),
    "domain": Dialect("domain", 3),
    "ai": Dialect("ai_component", 1),
}

# construct -> reason. Whole files with no kernel expansion by design (external
# evidence / structural analysis only) — excluded from the Monitor/BMC pipeline
# by construction, not by a missed registration.
EVIDENCE_CONSTRUCTS: dict[str, str] = {
    "ai-project": (
        "fsl-ai project file (is_ai_project_source): external statistical "
        "evidence only (fslc ai eval/regress/drift/compat), formal_result "
        "not_run, never expands to a kernel spec"
    ),
    "ai-agent": (
        "fsl-ai recursive agent file (is_ai_agent_source): structural analysis "
        "only (agent_analyzed), formal_result not_run, never expands to a "
        "kernel spec"
    ),
}

# repo-relative path -> reason. Individual files the Monitor legitimately
# rejects. Re-asserted every run: a stale entry (the file starts loading)
# fails the gate and must be deleted.
MONITOR_EXCLUSIONS: dict[str, str] = {
    "examples/self/no_actions.fsl": (
        "deliberate no-action edge fixture; Monitor requires >=1 action. "
        "BMC-side coverage lives in tests/test_self_conformance.py"
    ),
    "examples/annotations/annotated_claims.fsl": (
        "native-only declaration-level @annotation syntax (issue #241); the "
        "frozen Python reference does not parse @... before a nested "
        "declaration. Native coverage lives in rust/fsl-syntax and "
        "rust/fsl-core tests"
    ),
    "examples/annotations/annotated_domain.fsl": (
        "native-only declaration-level @annotation syntax on domain nested "
        "declarations (issue #281); the frozen Python reference does not "
        "parse @... before a nested declaration. Native coverage lives in "
        "rust/fsl-syntax and rust/fsl-core tests"
    ),
    "examples/annotations/annotated_dbsystem.fsl": (
        "native-only declaration-level @annotation syntax on dbsystem nested "
        "declarations (issue #281); the frozen Python reference does not "
        "parse @... before a nested declaration. Native coverage lives in "
        "rust/fsl-syntax and rust/fsl-core tests"
    ),
    "examples/annotations/annotated_ai_component.fsl": (
        "native-only declaration-level @annotation syntax on ai_component "
        "nested declarations (issue #281); the frozen Python reference does "
        "not parse @... before a nested declaration. Native coverage lives "
        "in rust/fsl-syntax tests"
    ),
}
