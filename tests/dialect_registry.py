# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Declarative registry of dialects/example corpora (issue #167).

``tests/test_dialect_conformance.py`` scans every ``.fsl`` under
``SCAN_ROOTS`` and classifies it; this module is the data side of that
classification, not logic. A new dialect (or a new example directory) that
nobody registers here fails ``test_dialect_conformance.py`` loudly when that
file is run, instead of the corpus silently sitting outside the
dual-evaluator safety net (the failure mode the 2026-07-08 fsl-db audit
found). That file is a manual/reference check, not a CI gate: no workflow
and no ``tools/check-native-integration.sh`` lane currently invokes it.
"""
from __future__ import annotations

from dataclasses import dataclass

SCAN_ROOTS = ("specs", "examples")


def is_causal_source(source: str) -> bool:
    """Sniff the top-level ``causal`` keyword.

    Native intentionally excludes "causal" from its dialect-dispatch
    ``frontends!`` list (``docs/DESIGN-causal.md`` §1: the causal graph never
    enters ``KernelModel``, ``fsl-runtime``, or ``fsl-solver``), and the frozen
    Python reference has no causal implementation at all, so
    ``src/fslc/dialect_registry.py``'s ``DIALECT_KEYWORDS`` deliberately
    excludes it too. This predicate therefore lives in test infrastructure,
    not ``src/fslc`` — adding it there would misrepresent the frozen
    reference as understanding a dialect it does not implement.
    """
    for line in source.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("//"):
            continue
        tokens = stripped.split()
        return bool(tokens) and tokens[0] == "causal"
    return False


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
    "causal": (
        "causal profile file (is_causal_source, docs/DESIGN-causal.md §1): the "
        "causal graph never enters KernelModel/fsl-runtime/fsl-solver, and "
        "check reports result=causal_model_checked / formal_result=not_run. "
        "Native coverage lives in rust/fslc/tests/causal_cli.rs; the frozen "
        "Python reference has no causal implementation at all"
    ),
}

# repo-relative path -> reason. Individual files the Monitor legitimately
# rejects. Re-asserted every run: a stale entry (the file starts loading)
# fails test_dialect_conformance.py (not CI-enforced; see this module's
# docstring) and must be deleted.
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
    "examples/requirements_stage.fsl": (
        "native-only requirements stage() syntax (issue #243); the frozen "
        "Python reference intentionally retains the pre-Rust compatibility "
        "surface. Native Monitor, symbolic, CLI, origin, and parser coverage "
        "lives in the Rust workspace tests"
    ),
    "examples/gallery/adversarial/governance_semantic_before.fsl": (
        "governance preservation 'before' fragment with no actions. Monitor "
        "requires >=1 action (same shape as examples/self/no_actions.fsl "
        "above), but native `fslc check` accepts it (ok, warnings only) -- it "
        "is not a DECLARED_ERROR fixture. Native coverage lives in "
        "rust/fslc/tests/cli_regression.rs::"
        "native_check_locates_a_semantic_dependency_error_at_the_preservation"
    ),
}
