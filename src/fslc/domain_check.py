# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""High-level fsl-domain / fsl-effect checking and generation entry points."""
from __future__ import annotations

from pathlib import Path

from .bmc import prove, verify
from .domain_codegen.simple import generate_simple_target
from .domain_codegen.typescript import generate_typescript
from .domain_expand import (
    DOMAIN_DIALECT_VERSION,
    DOMAIN_FINDING_SCHEMA_VERSION,
    expand_domain,
    static_domain_findings,
)
from .domain_parser import parse_domain
from .domain_replay import replay_domain_file
from .domain_testgen import generate_domain_test_bundle
from .model import build_spec


def load_domain(file):
    return parse_domain(Path(file).read_text(encoding="utf-8"))


def check_domain_source(domain, depth=8, engine="bmc", deadlock_mode="warn"):
    expansion = expand_domain(domain)
    findings = static_domain_findings(domain, expansion.assumptions)
    hard_findings = [finding for finding in findings if finding.get("severity") == "error"]
    if hard_findings:
        return {
            "result": "violated",
            "dialect": DOMAIN_DIALECT_VERSION,
            "finding_schema_version": DOMAIN_FINDING_SCHEMA_VERSION,
            "domain": domain.name,
            "formal_result": "not_run",
            "findings": findings,
            "assumptions": expansion.assumptions,
            "kernel_source": expansion.source,
        }

    spec = build_spec(expansion.ast, expansion.display_names)
    if engine == "induction":
        kernel = prove(spec, 1, depth, deadlock_mode=deadlock_mode)
    else:
        kernel = verify(spec, depth, deadlock_mode=deadlock_mode, source_lines=expansion.source.splitlines())
    result = "verified_under_assumptions" if kernel.get("result") in ("verified", "proved") else "violated"
    return {
        "result": result,
        "dialect": DOMAIN_DIALECT_VERSION,
        "finding_schema_version": DOMAIN_FINDING_SCHEMA_VERSION,
        "domain": domain.name,
        "spec": spec["name"],
        "formal_result": kernel.get("result"),
        "kernel": kernel,
        "findings": findings,
        "assumptions": expansion.assumptions,
        "generated_actions": expansion.generated_actions,
    }


def analyze_domain(domain):
    expansion = expand_domain(domain)
    findings = static_domain_findings(domain, expansion.assumptions)
    return {
        "result": "analyzed",
        "dialect": DOMAIN_DIALECT_VERSION,
        "finding_schema_version": DOMAIN_FINDING_SCHEMA_VERSION,
        "domain": domain.name,
        "profile": domain.implementation_profile,
        "aggregates": [
            {
                "name": aggregate.name,
                "id_type": aggregate.id_type,
                "state": [{"name": field.name, "type": field.type_name} for field in aggregate.state],
                "commands": [command.name for command in aggregate.commands],
                "events": [event.name for event in aggregate.events],
                "errors": [error.name for error in aggregate.errors],
                "invariants": [invariant.name for invariant in aggregate.invariants],
            }
            for aggregate in domain.aggregates
        ],
        "effects": [
            {
                "name": effect.name,
                "async": effect.async_effect,
                "reliable": effect.reliable,
                "irreversible": effect.irreversible,
                "handles": effect.handles or effect.request_event,
                "outcomes": list(effect.outcomes),
                "correlation_id": effect.correlation_id,
                "idempotency_key": effect.idempotency_key,
                "retry_max_attempts": effect.retry.max_attempts,
                "timeout_event": effect.timeout_event,
                "outbox": effect.outbox,
                "inbox": effect.inbox,
            }
            for effect in domain.effects
        ],
        "sagas": [
            {
                "name": saga.name,
                "starts_on": saga.starts_on,
                "steps": [
                    {
                        "name": step.name,
                        "async": step.async_step,
                        "requires": list(step.requires),
                        "emits": list(step.emits),
                        "awaits_mode": step.awaits_mode,
                        "awaits": list(step.awaits),
                        "timeout_event": step.timeout_event,
                    }
                    for step in saga.steps
                ],
                "compensations": [
                    {
                        "trigger_event": compensation.trigger_event,
                        "after_event": compensation.after_event,
                        "emits": list(compensation.emits),
                    }
                    for compensation in saga.compensations
                ],
                "outboxes": list(saga.outboxes),
                "inboxes": list(saga.inboxes),
                "invariants": [invariant.name for invariant in saga.invariants],
            }
            for saga in domain.sagas
        ],
        "findings": findings,
        "assumptions": expansion.assumptions,
    }


def expand_domain_source(domain):
    expansion = expand_domain(domain)
    return {
        "result": "expanded",
        "dialect": DOMAIN_DIALECT_VERSION,
        "domain": domain.name,
        "kernel_source": expansion.source,
        "assumptions": expansion.assumptions,
    }


def generate_domain_scaffold(domain, target="typescript"):
    if target != "typescript":
        files = generate_simple_target(domain, target)
    else:
        files = generate_typescript(domain)
    return {
        "result": "generated",
        "dialect": DOMAIN_DIALECT_VERSION,
        "domain": domain.name,
        "target": target,
        "files": [{"path": path, "content": content} for path, content in sorted(files.items())],
    }


def generate_domain_tests(file, depth=8, deadlock_mode="warn", target="vitest", strict=False):
    bundle = generate_domain_test_bundle(
        file,
        depth=depth,
        deadlock_mode=deadlock_mode,
        target=target,
        strict=strict,
    )
    return {
        "result": "generated",
        "dialect": DOMAIN_DIALECT_VERSION,
        "domain": bundle["domain"],
        "target": bundle["target"],
        "content": bundle["content"],
        "warnings": bundle.get("warnings", []),
    }


def replay_domain_logs(file, logs):
    return replay_domain_file(load_domain(file), logs)
