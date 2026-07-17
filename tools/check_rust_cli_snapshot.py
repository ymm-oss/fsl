# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare the Rust native CLI projection with the shared-language golden."""
from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
from pathlib import Path
from typing import Any

from check_rust_surface_parity import surface_cases


ROOT = Path(__file__).resolve().parents[1]
SNAPSHOT = ROOT / "tests" / "snapshots" / "corpus_snapshot.json"
DEFAULT_RUST_BIN = (
    ROOT
    / "rust"
    / "target"
    / "debug"
    / ("fslc.exe" if os.name == "nt" else "fslc")
)


def _warn_kinds(output: dict[str, Any]) -> list[str]:
    return sorted((warning.get("kind") or "none") for warning in output.get("warnings", []))


def _project_check(output: dict[str, Any]) -> dict[str, Any]:
    projected = {"result": output.get("result")}
    if output.get("result") == "ok":
        projected["warnings"] = _warn_kinds(output)
    else:
        projected["kind"] = output.get("kind")
    if "implements" in output:
        implemented = output["implements"]
        projected["implements"] = (
            implemented.get("result") if isinstance(implemented, dict) else implemented
        )
    return projected


def _project_verify(output: dict[str, Any]) -> dict[str, Any]:
    result = output.get("result")
    projected: dict[str, Any] = {"result": result}
    if result in ("verified", "proved"):
        projected["invariants_checked"] = output.get("invariants_checked")
        projected["transitions_checked"] = output.get("transitions_checked")
        deadlock = output.get("deadlock")
        projected["deadlock_found"] = (
            bool(deadlock.get("found")) if isinstance(deadlock, dict) else None
        )
        projected["reachables"] = {
            name: (
                witness.get("witnessed_at_step")
                if isinstance(witness, dict)
                else witness
            )
            for name, witness in sorted((output.get("reachables") or {}).items())
        }
        coverage = output.get("action_coverage") or {}
        projected["action_coverage"] = {
            name: coverage[name] for name in sorted(coverage)
        }
        projected["warnings"] = _warn_kinds(output)
    elif result == "violated":
        projected["violation_kind"] = output.get("violation_kind")
        projected["violated_at_step"] = output.get("violated_at_step")
        projected["name"] = (
            output.get("invariant")
            or output.get("leadsTo")
            or output.get("trans")
            or output.get("name")
        )
        last_action = output.get("last_action")
        projected["last_action"] = (
            last_action.get("name") if isinstance(last_action, dict) else last_action
        )
    elif result == "reachable_failed":
        projected["unreachable"] = sorted(
            name
            for name, witness in (output.get("reachables") or {}).items()
            if isinstance(witness, dict) and not witness.get("witnessed_at_step")
        )
    else:
        projected["kind"] = output.get("kind")
    return projected


def _invoke(binary: Path, arguments: list[str]) -> dict[str, Any]:
    proc = subprocess.run(
        [str(binary), *arguments],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            f"Rust CLI emitted invalid JSON for {arguments}: {proc.stderr.strip()}"
        ) from exc


def _verify_plan(root: Path, path: Path) -> tuple[int, str]:
    if path.parent == root / "specs":
        return 5, "warn"
    command = None
    for line in path.read_text(encoding="utf-8").splitlines()[:16]:
        stripped = line.strip()
        if stripped.startswith("// expected-command:"):
            command = stripped.split(":", 1)[1].strip()
    depth, deadlock = 4, "warn"
    if command:
        parts = shlex.split(command)
        for index, part in enumerate(parts):
            if part == "--depth":
                depth = int(parts[index + 1])
            elif part == "--deadlock":
                deadlock = parts[index + 1]
    return depth, deadlock


def run(root: Path, binary: Path) -> dict[str, Any]:
    golden = json.loads((root / "tests/snapshots/corpus_snapshot.json").read_text())
    cases = [
        path
        for path, _ in surface_cases(
            root,
            frozenset(
                {"spec", "compose", "business", "requirements", "governance", "refinement"}
            ),
        )
    ]
    failures = []
    for path in cases:
        relative = path.relative_to(root).as_posix()
        expected = golden[relative]
        check = _project_check(_invoke(binary, ["check", str(path)]))
        actual: dict[str, Any] = {"check": check}
        if "verify" in expected and check.get("result") == "ok":
            depth, deadlock = _verify_plan(root, path)
            actual["verify"] = _project_verify(
                _invoke(
                    binary,
                    [
                        "verify",
                        str(path),
                        "--depth",
                        str(depth),
                        "--deadlock",
                        deadlock,
                    ],
                )
            )
        if actual != expected:
            failures.append(
                {
                    "path": relative,
                    "expected": expected,
                    "rust": actual,
                }
            )
    return {
        "schema": "fsl-rust-cli-snapshot-parity.v1",
        "scope": "shared-language-corpus",
        "cases": len(cases),
        "matched": len(cases) - len(failures),
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    args = parser.parse_args(argv)
    if not args.rust_bin.is_file():
        parser.error(f"Rust CLI not found: {args.rust_bin}; run cargo build first")
    result = run(args.root, args.rust_bin)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
