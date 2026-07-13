# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Deterministic grammar-derived expression fuzzing across Python and Rust."""

from __future__ import annotations

import argparse
import json
import random
from pathlib import Path

from check_rust_ast_parity import DEFAULT_RUST_BIN, compare_expression


ATOMS = (
    "0",
    "1",
    "-2",
    "true",
    "false",
    "none",
    "x",
    "old(x)",
    "some(1)",
    "Set { 0, 1 }",
    "Seq { 0, 1 }",
    "record.value",
    "items[i]",
)
ARITHMETIC = ("+", "-", "*", "/", "%")
COMPARISON = ("==", "!=", "<", "<=", ">", ">=")
LOGIC = ("and", "or", "=>")
METHODS = ("size()", "contains(0)", "at(0)")


def generated_expression(rng: random.Random, depth: int) -> str:
    if depth <= 0:
        return rng.choice(ATOMS)
    choice = rng.randrange(8)
    if choice == 0:
        return f"not ({generated_expression(rng, depth - 1)})"
    if choice == 1:
        return f"-({generated_expression(rng, depth - 1)})"
    if choice in (2, 3):
        operator = rng.choice(ARITHMETIC if choice == 2 else COMPARISON)
        return (
            f"({generated_expression(rng, depth - 1)}) {operator} "
            f"({generated_expression(rng, depth - 1)})"
        )
    if choice == 4:
        return (
            f"({generated_expression(rng, depth - 1)}) {rng.choice(LOGIC)} "
            f"({generated_expression(rng, depth - 1)})"
        )
    if choice == 5:
        return f"some({generated_expression(rng, depth - 1)})"
    if choice == 6:
        return f"({generated_expression(rng, depth - 1)}).{rng.choice(METHODS)}"
    return (
        f"forall i in 0..1: ({generated_expression(rng, depth - 1)}) "
        f"{rng.choice(LOGIC)} ({generated_expression(rng, depth - 1)})"
    )


def run(binary: Path, cases: int, seed: int) -> dict:
    rng = random.Random(seed)
    expressions = [generated_expression(rng, rng.randrange(1, 4)) for _ in range(cases)]
    failures = []
    for source in expressions:
        try:
            failure = compare_expression(source, binary)
        except Exception as error:  # Both parsers must accept generated grammar cases.
            failure = {"source": source, "kind": "python_parse_error", "error": str(error)}
        if failure is not None:
            failures.append(failure)
    return {
        "schema": "fsl-rust-grammar-fuzz.v1",
        "seed": seed,
        "cases": cases,
        "matched": cases - len(failures),
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-bin", type=Path, default=DEFAULT_RUST_BIN)
    parser.add_argument("--cases", type=int, default=256)
    parser.add_argument("--seed", type=int, default=195)
    args = parser.parse_args()
    result = run(args.rust_bin, args.cases, args.seed)
    print(json.dumps(result, indent=2, ensure_ascii=False, sort_keys=True))
    return 0 if not result["failures"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
