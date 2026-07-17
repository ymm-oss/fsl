# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Domain-specific adapter/conformance scaffold generation."""
from __future__ import annotations

from pathlib import Path

from .domain_codegen.typescript import generate_typescript
from .domain_parser import parse_domain
from .testgen import generate_test_bundle


def default_domain_testgen_output(file, target="vitest"):
    stem = Path(file).stem
    if target == "vitest":
        return f"{stem}.domain.conformance.test.ts"
    return f"{stem}.domain.conformance.py"


def generate_domain_test_bundle(file, depth=8, deadlock_mode="warn", target="vitest", strict=False):
    src = Path(file).read_text(encoding="utf-8")
    domain = parse_domain(src)
    if target != "vitest":
        bundle = generate_test_bundle(file, depth=depth, deadlock_mode=deadlock_mode, strict=strict, target=target)
        return {
            "domain": domain.name,
            "target": target,
            "content": bundle["content"],
            "warnings": bundle.get("warnings", []),
        }

    generic = generate_test_bundle(
        file,
        depth=depth,
        deadlock_mode=deadlock_mode,
        strict=strict,
        target="vitest",
    )
    generated = generate_typescript(domain)
    adapter_files = {
        path: content
        for path, content in generated.items()
        if path.endswith("/adapter.ts") or path == "effects.ts"
    }
    header = [
        "// Auto-generated fsl-domain conformance scaffold.",
        "// Wire makeAdapter() to the generated aggregate adapter or your implementation adapter.",
        "",
    ]
    adapter_notes = []
    for path, content in adapter_files.items():
        adapter_notes.extend([
            f"// --- scaffold: {path} ---",
            *("// " + line if line else "//" for line in content.splitlines()),
            "",
        ])
    content = "\n".join(header + adapter_notes) + generic["content"]
    return {
        "domain": domain.name,
        "target": target,
        "content": content,
        "warnings": generic.get("warnings", []),
    }

