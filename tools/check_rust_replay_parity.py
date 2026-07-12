# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare the public native and Python replay command contracts."""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from fslc.cli import run_replay  # noqa: E402
from tools.check_rust_cli_snapshot import DEFAULT_RUST_BIN  # noqa: E402


def _project(output: dict[str, Any]) -> dict[str, Any]:
    result = output.get("result")
    projected = {"result": result}
    if result == "conformant":
        projected.update(
            {
                "spec": output.get("spec"),
                "steps_checked": output.get("steps_checked"),
                "final_state": output.get("final_state"),
            }
        )
    elif result == "nonconformant":
        violation = output.get("violation") or {}
        projected.update(
            {
                "spec": output.get("spec"),
                "failed_at_event": output.get("failed_at_event"),
                "violation_kind": violation.get("kind"),
                "state_before": output.get("state_before"),
            }
        )
    else:
        projected["kind"] = output.get("kind")
    return projected


def _rust_replay(binary: Path, spec: Path, trace: Path) -> dict[str, Any]:
    proc = subprocess.run(
        [str(binary), "replay", str(spec), "--trace", str(trace)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    return json.loads(proc.stdout)


def run(root: Path, binary: Path) -> dict[str, Any]:
    spec = root / "specs/cart_v1.fsl"
    good = [
        {"action": "add_to_cart", "params": {"u": 0, "i": 0}},
        {"action": "checkout", "params": {"u": 0}},
    ]
    cases = [
        ("array_conformant", good),
        ("object_conformant", {"events": good}),
        (
            "requires_nonconformant",
            [*good, {"action": "checkout", "params": {"u": 0}}],
        ),
    ]
    failures = []
    for name, events in cases:
        with tempfile.NamedTemporaryFile(
            "w", suffix=".json", encoding="utf-8", delete=False
        ) as stream:
            json.dump(events, stream, ensure_ascii=False)
            trace = Path(stream.name)
        try:
            python = _project(run_replay(str(spec), str(trace)))
            rust = _project(_rust_replay(binary, spec, trace))
        finally:
            trace.unlink(missing_ok=True)
        if python != rust:
            failures.append(
                {"case": name, "python": python, "rust": rust}
            )
    return {
        "schema": "fsl-rust-replay-parity.v1",
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
