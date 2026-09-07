# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compare Python and Rust CLI envelopes with a narrow reviewed allowlist.

Disposition (docs/RUST-PORTING.md F8): deletion deferred. This is the only
harness that compares the frozen Python envelope against the native one for
the *full* ``check``/``verify`` envelope (``leadsto_parity`` and
``dialect_parity`` separately do the same kind of comparison, narrowed to
their own dialect/case sets). ``rust/fsl-wasm/test-browser.mjs``'s
native/Worker parity compares native against WASM and does not observe the
frozen Python side, so it does not own this detector and is not a substitute
for it. The shared ``_diff``/
``_normalize`` helpers moving to ``rust_parity_util.py`` (#913) removed the
only reason this script's *deletion* was ever blocked on another consumer,
but that does not make retiring the comparison edge itself safe -- that is a
separate compatibility decision, tracked in #988.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from fslc.cli import run_check, run_verify

from check_rust_cli_snapshot import DEFAULT_RUST_BIN, _invoke
from rust_parity_util import _diff, _normalize


ROOT = Path(__file__).resolve().parents[1]

def run(root: Path, binary: Path, depth: int) -> dict[str, Any]:
    failures = []
    cases = sorted((root / "specs").glob("*.fsl"))
    comparisons = 0
    for path in cases:
        relative = path.relative_to(root).as_posix()
        python_check = _normalize(run_check(path))
        rust_check = _normalize(_invoke(binary, ["check", str(path)]))
        comparisons += 1
        differences = _diff(python_check, rust_check)
        if differences:
            failures.append({"path": relative, "command": "check", "differences": differences})
        if python_check.get("result") != "ok":
            continue
        python_verify = _normalize(
            run_verify(path, depth, "warn", use_cache=False)
        )
        rust_verify = _normalize(
            _invoke(
                binary,
                [
                    "verify",
                    str(path),
                    "--depth",
                    str(depth),
                    "--deadlock",
                    "warn",
                    "--no-cache",
                ],
            )
        )
        comparisons += 1
        differences = _diff(python_verify, rust_verify)
        if differences:
            failures.append({"path": relative, "command": "verify", "differences": differences})
    return {
        "schema": "fsl-rust-full-envelope-parity.v1",
        "scope": "specs",
        "depth": depth,
        "cases": len(cases),
        "comparisons": comparisons,
        "matched": comparisons - len(failures),
        "failures": failures,
        "allowlist": {
            "$.cost.elapsed_s": "wall-clock timing",
            "$.trace": "nondeterministic witness; bidirectional Monitor replay gate",
            "$.deadlock.trace": "nondeterministic witness; bidirectional Monitor replay gate",
            "$.reachables.*.witness": "nondeterministic witness; bidirectional Monitor replay gate",
            "$.violating_bindings": "derived from nondeterministic witness",
            "$.last_action.params": "derived from nondeterministic witness",
            "$.blame.conjuncts.*.violating_bindings": "derived from nondeterministic witness",
            "$.warnings.*.message[state]": "derived from replayed deadlock witness",
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    parser.add_argument("--depth", type=int, default=5)
    args = parser.parse_args(argv)
    result = run(args.root, args.rust_bin, args.depth)
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
