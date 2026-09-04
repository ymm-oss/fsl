#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""CI validator inventory: discover validator modules and required-gate wiring.

Scope, since issue #761 stage 2: `tests/test_*.py` pytest modules and
`tools/check_rust_*.py` frozen-Python/Rust parity harnesses (the family
`docs/DESIGN-ci-validator-inventory.md`'s "Scope boundaries" originally
reserved as out of scope for slice 1).

Guarantee boundary, load-bearing for why `--exempt path:reason` is an
acceptable way to satisfy this check: this tool establishes only that a
validator module's tier and reason were *recorded*, not that the recorded
reason is *correct*. Passing `--exempt tools/check_rust_x.py:some-reason`
once is the entire cost of satisfying `generate`'s guard, and that is
intentional -- the property this check protects is "no validator module can
accumulate silently, unclassified" (issue #761's own root problem: 17
`tools/check_rust_*.py` harnesses existed with nobody having recorded why).
Whether a recorded reason accurately describes the module -- the F1-F7
precondition analysis, native-owner cross-referencing, and retirement
readiness -- is `docs/RUST-PORTING.md`'s job, a human-maintained document
this tool does not read and cannot verify.

Wiring-detection scope, deliberate, shared by PYTEST_PATH and
RUST_HARNESS_PATH alike: both regexes match a path preceded by whitespace,
a quote, or `=`, including inside a `#` comment -- a commented-out mention
is treated the same as a real invocation, and this was already true for
`tests/test_*.py` before this file existed (verified: unchanged by this
extension, not introduced by it). Neither regex has an
`IMPORT_PATH`-style companion for a `from tools.check_rust_x import` form,
because no `REQUIRED_GATE_ENTRYPOINTS` file currently wires a
`tools/check_rust_*.py` harness that way (`tools/check-merge-readiness.sh`'s
own inline `from tests.test_x import` pattern, which `IMPORT_PATH` exists
to catch, has no `tools/check_rust_*.py` analogue today); add one only if
and when such a wiring pattern is actually introduced, not speculatively.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

INVENTORY_PATH = Path(".github/ci-validator-inventory.json")
SCHEMA_VERSION = 1

REQUIRED_GATE_ENTRYPOINTS = (
    "tools/check-merge-readiness.sh",
    ".github/workflows/site-reference-freshness.yml",
)

PYTEST_PATH = re.compile(
    r"(?:^|[\s\"'=])(tests/test_[A-Za-z0-9_]+\.py)"
)
IMPORT_PATH = re.compile(
    r"from\s+tests\.(test_[A-Za-z0-9_]+)\s+import"
)
RUST_HARNESS_PATH = re.compile(
    r"(?:^|[\s\"'=])(tools/check_rust_[A-Za-z0-9_]+\.py)"
)

EXEMPT_REASONS = frozenset(
    {
        "frozen-python-compatibility",
        "hook-local",
        # issue #761 stage 2: tools/check_rust_*.py-specific reasons. Each
        # names a distinct cause a harness is unwired, matching #761's own
        # classification table -- collapsing them into
        # frozen-python-compatibility would lose that distinction (a
        # `manual-developer-run` harness like `surface_parity` does not even
        # import the frozen Python package; `parked-pending-unrelated-work`
        # and `pending-native-migration` are blocked on work with no relation
        # to Python compatibility at all).
        "manual-developer-run",
        "parked-pending-unrelated-work",
        "pending-native-migration",
    }
)

HOOK_LOCAL_TESTS = frozenset({"tests/test_hook_enforcement.py"})


def _git_tracked_py_modules(root: Path, scope_dir: str, name_prefix: str) -> list[str]:
    result = subprocess.run(
        ["git", "ls-files", "-z", "--", scope_dir],
        cwd=root,
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            f"git ls-files failed with exit {result.returncode}: {detail or '(no stderr)'}"
        )
    paths = [
        name
        for name in result.stdout.decode("utf-8").split("\0")
        if name.startswith(name_prefix) and name.endswith(".py")
    ]
    return sorted(paths)


def git_tracked_test_modules(root: Path) -> list[str]:
    return _git_tracked_py_modules(root, "tests", "tests/test_")


def git_tracked_rust_harness_modules(root: Path) -> list[str]:
    return _git_tracked_py_modules(root, "tools", "tools/check_rust_")


def git_tracked_validator_modules(root: Path) -> list[str]:
    return sorted(git_tracked_test_modules(root) + git_tracked_rust_harness_modules(root))


def read_entrypoint_text(root: Path, entrypoint: str) -> str:
    path = root / entrypoint
    return path.read_text(encoding="utf-8")


def wired_validator_modules(root: Path) -> dict[str, list[str]]:
    wiring: dict[str, set[str]] = {}
    for entrypoint in REQUIRED_GATE_ENTRYPOINTS:
        text = read_entrypoint_text(root, entrypoint)
        for match in PYTEST_PATH.finditer(text):
            wiring.setdefault(match.group(1), set()).add(entrypoint)
        for match in IMPORT_PATH.finditer(text):
            path = f"tests/{match.group(1)}.py"
            wiring.setdefault(path, set()).add(entrypoint)
        for match in RUST_HARNESS_PATH.finditer(text):
            wiring.setdefault(match.group(1), set()).add(entrypoint)
    return {
        path: sorted(entrypoints)
        for path, entrypoints in sorted(wiring.items())
    }


def load_inventory(root: Path) -> dict[str, Any]:
    path = root / INVENTORY_PATH
    return json.loads(path.read_text(encoding="utf-8"))


def inventory_index(inventory: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {entry["path"]: entry for entry in inventory["validators"]}


def default_exempt_reason(path: str) -> str:
    # A deliberately conservative fallback. Reached via --bootstrap for a
    # genuinely new path with no prior recorded reason and no explicit
    # --exempt override; also reached, pre-existing and unrelated to issue
    # #761's extension, via an ordinary (non-bootstrap) generate for a path
    # whose wiring/prior tier falls through build_entry()'s more specific
    # branches -- for example a previously `required` module that is no
    # longer wired anywhere (confirmed directly: inventory_document(...,
    # bootstrap=False) with such a prior still reaches this function). It
    # does not attempt to distinguish manual-developer-run,
    # parked-pending-unrelated-work, or pending-native-migration from
    # frozen-python-compatibility for a tools/check_rust_*.py path: getting
    # that distinction right needs the classification behind issue #761's
    # table, which a caller is expected to supply via explicit --exempt
    # pairs (see docs/DESIGN-ci-validator-inventory.md) rather than rely on
    # this guess.
    if path in HOOK_LOCAL_TESTS:
        return "hook-local"
    return "frozen-python-compatibility"


def build_entry(
    path: str,
    *,
    wiring: dict[str, list[str]],
    prior: dict[str, dict[str, Any]] | None = None,
) -> dict[str, Any]:
    entrypoints = wiring.get(path, [])
    if entrypoints:
        return {
            "path": path,
            "tier": "required",
            "entrypoints": entrypoints,
        }
    prior_entry = (prior or {}).get(path)
    if prior_entry and prior_entry.get("tier") == "exempt":
        reason = prior_entry.get("exempt_reason")
        if reason not in EXEMPT_REASONS:
            raise ValueError(f"{path}: invalid prior exempt_reason {reason!r}")
        return {
            "path": path,
            "tier": "exempt",
            "exempt_reason": reason,
        }
    return {
        "path": path,
        "tier": "exempt",
        "exempt_reason": default_exempt_reason(path),
    }


def inventory_document(
    root: Path,
    *,
    discovered: list[str],
    wiring: dict[str, list[str]],
    prior: dict[str, dict[str, Any]] | None = None,
    new_exempt: dict[str, str] | None = None,
    bootstrap: bool = False,
) -> dict[str, Any]:
    entries: list[dict[str, Any]] = []
    missing_classification: list[str] = []
    for path in discovered:
        if path in wiring:
            entries.append(build_entry(path, wiring=wiring, prior=prior))
            continue
        if prior and path in prior:
            entries.append(build_entry(path, wiring=wiring, prior=prior))
            continue
        if new_exempt and path in new_exempt:
            reason = new_exempt[path]
            if reason not in EXEMPT_REASONS:
                raise ValueError(f"{path}: unknown exempt reason {reason!r}")
            entries.append(
                {
                    "path": path,
                    "tier": "exempt",
                    "exempt_reason": reason,
                }
            )
            continue
        if bootstrap:
            entries.append(build_entry(path, wiring=wiring, prior=prior))
            continue
        missing_classification.append(path)
    if missing_classification:
        joined = ", ".join(missing_classification)
        raise RuntimeError(
            "new validator module(s) require explicit classification or required-gate "
            f"wiring before inventory generation: {joined}"
        )
    return {
        "schema_version": SCHEMA_VERSION,
        "required_gate_entrypoints": list(REQUIRED_GATE_ENTRYPOINTS),
        "validators": entries,
    }


def inventory_findings(
    root: Path,
    *,
    discovered: list[str],
    wiring: dict[str, list[str]],
    inventory: dict[str, Any],
) -> list[str]:
    findings: list[str] = []
    indexed = inventory_index(inventory)
    discovered_set = set(discovered)
    indexed_set = set(indexed)

    missing = sorted(discovered_set - indexed_set)
    if missing:
        findings.extend(
            f"untracked validator module (add required-gate wiring or regenerate inventory with --exempt): {path}"
            for path in missing
        )

    ghosts = sorted(indexed_set - discovered_set)
    if ghosts:
        findings.extend(
            f"inventory ghost validator module (remove stale inventory row): {path}"
            for path in ghosts
        )

    if inventory.get("required_gate_entrypoints") != list(REQUIRED_GATE_ENTRYPOINTS):
        findings.append(
            "inventory required_gate_entrypoints drifted from "
            f"{list(REQUIRED_GATE_ENTRYPOINTS)!r}"
        )

    for path in discovered:
        entry = indexed.get(path)
        if entry is None:
            continue
        tier = entry.get("tier")
        wired = path in wiring
        if tier == "required":
            if not wired:
                findings.append(
                    f"required validator is not wired to a required gate entrypoint: {path}"
                )
                continue
            recorded = sorted(entry.get("entrypoints", []))
            actual = wiring[path]
            if recorded != actual:
                findings.append(
                    f"required validator entrypoints drifted for {path}: "
                    f"inventory={recorded!r} actual={actual!r}"
                )
        elif tier == "exempt":
            reason = entry.get("exempt_reason")
            if reason not in EXEMPT_REASONS:
                findings.append(
                    f"exempt validator has unknown exempt_reason for {path}: {reason!r}"
                )
            if wired:
                findings.append(
                    f"exempt validator is wired to a required gate and must be tier=required: {path}"
                )
        else:
            findings.append(f"validator has unknown tier for {path}: {tier!r}")

    wired_not_required = sorted(set(wiring) - {path for path, entry in indexed.items() if entry.get("tier") == "required"})
    if wired_not_required:
        findings.extend(
            f"wired validator missing required inventory row: {path}"
            for path in wired_not_required
        )

    return findings


def _is_validator_path(path: str) -> bool:
    if path.startswith("tests/test_") and path.endswith(".py"):
        return True
    return path.startswith("tools/check_rust_") and path.endswith(".py")


def parse_exempt_pairs(pairs: list[str]) -> dict[str, str]:
    parsed: dict[str, str] = {}
    for pair in pairs:
        if ":" not in pair:
            raise ValueError(f"--exempt expects path:reason, got {pair!r}")
        path, reason = pair.split(":", 1)
        if not _is_validator_path(path):
            raise ValueError(
                f"--exempt path must be tests/test_*.py or tools/check_rust_*.py, got {path!r}"
            )
        parsed[path] = reason
    return parsed


def write_inventory(root: Path, document: dict[str, Any]) -> None:
    path = root / INVENTORY_PATH
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def selftest(root: Path) -> int:
    fixture_root = root / "tests/fixtures/ci-validator-inventory"
    ok = True

    def run_case(
        label: str,
        *,
        discovered: list[str],
        wiring: dict[str, list[str]],
        inventory: dict[str, Any],
        expected_fragments: list[str],
        should_pass: bool,
    ) -> None:
        produced = inventory_findings(
            fixture_root,
            discovered=discovered,
            wiring=wiring,
            inventory=inventory,
        )
        fragments_match = all(
            any(fragment in finding for finding in produced)
            for fragment in expected_fragments
        )
        passed = fragments_match and bool(produced) == (not should_pass)
        if passed:
            return
        print(
            f"selftest: FAIL: {label}: produced={produced!r} "
            f"expected_fragments={expected_fragments!r} should_pass={should_pass}",
            file=sys.stderr,
        )
        nonlocal ok
        ok = False

    inventory = {
        "schema_version": SCHEMA_VERSION,
        "required_gate_entrypoints": list(REQUIRED_GATE_ENTRYPOINTS),
        "validators": [
            {
                "path": "tests/test_required.py",
                "tier": "required",
                "entrypoints": ["tools/check-merge-readiness.sh"],
            },
            {
                "path": "tests/test_exempt.py",
                "tier": "exempt",
                "exempt_reason": "frozen-python-compatibility",
            },
        ],
    }
    run_case(
        "complete inventory",
        discovered=["tests/test_required.py", "tests/test_exempt.py"],
        wiring={
            "tests/test_required.py": ["tools/check-merge-readiness.sh"],
        },
        inventory=inventory,
        expected_fragments=[],
        should_pass=True,
    )
    run_case(
        "untracked validator",
        discovered=[
            "tests/test_required.py",
            "tests/test_exempt.py",
            "tests/test_new.py",
        ],
        wiring={
            "tests/test_required.py": ["tools/check-merge-readiness.sh"],
        },
        inventory=inventory,
        expected_fragments=["untracked validator module"],
        should_pass=False,
    )
    run_case(
        "required but unwired",
        discovered=["tests/test_required.py", "tests/test_exempt.py"],
        wiring={},
        inventory=inventory,
        expected_fragments=["required validator is not wired"],
        should_pass=False,
    )
    run_case(
        "wired but exempt",
        discovered=["tests/test_required.py", "tests/test_exempt.py"],
        wiring={
            "tests/test_required.py": ["tools/check-merge-readiness.sh"],
            "tests/test_exempt.py": ["tools/check-merge-readiness.sh"],
        },
        inventory=inventory,
        expected_fragments=["exempt validator is wired"],
        should_pass=False,
    )
    run_case(
        "inventory ghost",
        discovered=["tests/test_required.py"],
        wiring={
            "tests/test_required.py": ["tools/check-merge-readiness.sh"],
        },
        inventory=inventory,
        expected_fragments=["inventory ghost validator module"],
        should_pass=False,
    )
    run_case(
        "generate blocks new unwired without classification",
        discovered=["tests/test_required.py", "tests/test_exempt.py", "tests/test_new.py"],
        wiring={
            "tests/test_required.py": ["tools/check-merge-readiness.sh"],
        },
        inventory=inventory,
        expected_fragments=["untracked validator module"],
        should_pass=False,
    )

    try:
        inventory_document(
            fixture_root,
            discovered=[
                "tests/test_required.py",
                "tests/test_exempt.py",
                "tests/test_new.py",
            ],
            wiring={
                "tests/test_required.py": ["tools/check-merge-readiness.sh"],
            },
            prior=inventory_index(inventory),
            bootstrap=False,
        )
        print(
            "selftest: FAIL: generate must reject new unwired validators without classification",
            file=sys.stderr,
        )
        ok = False
    except RuntimeError as error:
        if "new validator module(s) require explicit classification" not in str(error):
            print(f"selftest: FAIL: unexpected generate error: {error}", file=sys.stderr)
            ok = False

    generated = inventory_document(
        fixture_root,
        discovered=["tests/test_required.py", "tests/test_exempt.py", "tests/test_new.py"],
        wiring={
            "tests/test_required.py": ["tools/check-merge-readiness.sh"],
            "tests/test_new.py": ["tools/check-merge-readiness.sh"],
        },
        prior=inventory_index(inventory),
        bootstrap=False,
    )
    generated_index = inventory_index(generated)
    if generated_index["tests/test_new.py"]["tier"] != "required":
        print("selftest: FAIL: wired new validator must become required", file=sys.stderr)
        ok = False

    # issue #761 stage 2: the same accepting/rejecting shape, for
    # tools/check_rust_*.py instead of tests/test_*.py -- the failure/success
    # side is calibrated on the identical execution path (inventory_findings
    # / inventory_document), not a separate code path that could silently
    # diverge from the tests/ behavior above.
    rust_inventory = {
        "schema_version": SCHEMA_VERSION,
        "required_gate_entrypoints": list(REQUIRED_GATE_ENTRYPOINTS),
        "validators": [
            {
                "path": "tools/check_rust_required.py",
                "tier": "required",
                "entrypoints": ["tools/check-merge-readiness.sh"],
            },
            {
                "path": "tools/check_rust_exempt.py",
                "tier": "exempt",
                "exempt_reason": "manual-developer-run",
            },
        ],
    }
    run_case(
        "complete inventory (rust harness)",
        discovered=["tools/check_rust_required.py", "tools/check_rust_exempt.py"],
        wiring={
            "tools/check_rust_required.py": ["tools/check-merge-readiness.sh"],
        },
        inventory=rust_inventory,
        expected_fragments=[],
        should_pass=True,
    )
    run_case(
        "untracked rust harness",
        discovered=[
            "tools/check_rust_required.py",
            "tools/check_rust_exempt.py",
            "tools/check_rust_new.py",
        ],
        wiring={
            "tools/check_rust_required.py": ["tools/check-merge-readiness.sh"],
        },
        inventory=rust_inventory,
        expected_fragments=["untracked validator module"],
        should_pass=False,
    )
    run_case(
        "exempt rust harness with unknown reason",
        discovered=["tools/check_rust_required.py", "tools/check_rust_exempt.py"],
        wiring={
            "tools/check_rust_required.py": ["tools/check-merge-readiness.sh"],
        },
        inventory={
            "schema_version": SCHEMA_VERSION,
            "required_gate_entrypoints": list(REQUIRED_GATE_ENTRYPOINTS),
            "validators": [
                rust_inventory["validators"][0],
                {
                    "path": "tools/check_rust_exempt.py",
                    "tier": "exempt",
                    "exempt_reason": "not-a-declared-reason",
                },
            ],
        },
        expected_fragments=["unknown exempt_reason"],
        should_pass=False,
    )

    try:
        inventory_document(
            fixture_root,
            discovered=[
                "tools/check_rust_required.py",
                "tools/check_rust_exempt.py",
                "tools/check_rust_new.py",
            ],
            wiring={
                "tools/check_rust_required.py": ["tools/check-merge-readiness.sh"],
            },
            prior=inventory_index(rust_inventory),
            bootstrap=False,
        )
        print(
            "selftest: FAIL: generate must reject new unwired rust harnesses without classification",
            file=sys.stderr,
        )
        ok = False
    except RuntimeError as error:
        if "new validator module(s) require explicit classification" not in str(error):
            print(f"selftest: FAIL: unexpected generate error (rust harness): {error}", file=sys.stderr)
            ok = False

    generated_exempt = inventory_document(
        fixture_root,
        discovered=[
            "tools/check_rust_required.py",
            "tools/check_rust_exempt.py",
            "tools/check_rust_new.py",
        ],
        wiring={
            "tools/check_rust_required.py": ["tools/check-merge-readiness.sh"],
        },
        prior=inventory_index(rust_inventory),
        new_exempt={"tools/check_rust_new.py": "pending-native-migration"},
        bootstrap=False,
    )
    generated_exempt_index = inventory_index(generated_exempt)
    new_entry = generated_exempt_index["tools/check_rust_new.py"]
    if new_entry["tier"] != "exempt" or new_entry["exempt_reason"] != "pending-native-migration":
        print(
            f"selftest: FAIL: --exempt classification did not stick for a rust harness: {new_entry!r}",
            file=sys.stderr,
        )
        ok = False

    generated_rust = inventory_document(
        fixture_root,
        discovered=[
            "tools/check_rust_required.py",
            "tools/check_rust_exempt.py",
            "tools/check_rust_new.py",
        ],
        wiring={
            "tools/check_rust_required.py": ["tools/check-merge-readiness.sh"],
            "tools/check_rust_new.py": ["tools/check-merge-readiness.sh"],
        },
        prior=inventory_index(rust_inventory),
        bootstrap=False,
    )
    generated_rust_index = inventory_index(generated_rust)
    if generated_rust_index["tools/check_rust_new.py"]["tier"] != "required":
        print("selftest: FAIL: wired new rust harness must become required", file=sys.stderr)
        ok = False

    if ok:
        print("selftest: PASS: inventory_accepting=2 inventory_rejecting=7")
        return 0
    return 1


def check(root: Path) -> int:
    discovered = git_tracked_validator_modules(root)
    wiring = wired_validator_modules(root)
    inventory = load_inventory(root)
    findings = inventory_findings(
        root,
        discovered=discovered,
        wiring=wiring,
        inventory=inventory,
    )
    if findings:
        print("\n".join(findings), file=sys.stderr)
        return 1
    required = sum(1 for entry in inventory["validators"] if entry["tier"] == "required")
    exempt = sum(1 for entry in inventory["validators"] if entry["tier"] == "exempt")
    print(
        "check-ci-validator-inventory: PASS -- "
        f"{len(discovered)} validator module(s), required={required}, exempt={exempt}"
    )
    return 0


def generate(
    root: Path,
    *,
    bootstrap: bool,
    exempt_pairs: list[str],
) -> int:
    discovered = git_tracked_validator_modules(root)
    wiring = wired_validator_modules(root)
    prior = None
    inventory_path = root / INVENTORY_PATH
    if inventory_path.is_file():
        prior = inventory_index(load_inventory(root))
    document = inventory_document(
        root,
        discovered=discovered,
        wiring=wiring,
        prior=prior,
        new_exempt=parse_exempt_pairs(exempt_pairs),
        bootstrap=bootstrap,
    )
    write_inventory(root, document)
    print(
        f"check-ci-validator-inventory: wrote {INVENTORY_PATH.as_posix()} "
        f"({len(document['validators'])} validator module(s))"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command",
        nargs="?",
        choices=("check", "generate", "selftest"),
        default="check",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=None,
        help="repository root (defaults to the parent of tools/)",
    )
    parser.add_argument(
        "--bootstrap",
        action="store_true",
        help="seed exempt rows for the initial inventory only",
    )
    parser.add_argument(
        "--exempt",
        action="append",
        default=[],
        metavar="PATH:REASON",
        help="classify a new unwired validator as exempt",
    )
    args = parser.parse_args()
    root = args.root.resolve() if args.root is not None else Path(__file__).resolve().parent.parent
    if args.command == "selftest":
        return selftest(root)
    if args.command == "generate":
        return generate(root, bootstrap=args.bootstrap, exempt_pairs=args.exempt)
    return check(root)


if __name__ == "__main__":
    raise SystemExit(main())
