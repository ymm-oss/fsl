# Repository Guidelines

## Project Structure & Module Organization

`fslc` is a Python package under `src/fslc/`. The main entry points are `cli.py` for the command line, `parser.py` and `grammar.py` for parsing, `model.py` for semantic checks, `bmc.py` for verification/proof logic, `runtime.py` for the concrete monitor, and `refine.py` for refinement checks. Tests live in `tests/`, with fixture specs in `tests/fixtures/` and snapshots in `tests/snapshots/`. Example and sample FSL programs are split between `specs/` and `examples/`. User-facing references are in `docs/`, while AI agent skills are maintained in `skills/`.

## Build, Test, and Development Commands

Create a local editable install before changing code:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"
```

Run the full suite with `pytest -q`; CI runs this on Python 3.9 through 3.12. Run a focused file with `pytest tests/test_v1.py -q`. Exercise the CLI locally with commands such as `python -m fslc check specs/cart_v1.fsl`, `python -m fslc verify specs/cart_v1.fsl --depth 8`, and `python -m fslc verify specs/cart_v1.fsl --engine induction`.

## Coding Style & Naming Conventions

Use standard Python style: 4-space indentation, `snake_case` for functions and variables, `PascalCase` for classes, and concise module-level docstrings where useful. Keep CLI output machine-readable JSON where existing commands do so. New Python source files should include the Apache-2.0 SPDX header shown in `CONTRIBUTING.md`.

## Testing Guidelines

Tests use `pytest` and follow `tests/test_*.py` naming. Add regression coverage for behavior changes, especially verifier semantics, parser/grammar updates, refinement logic, runtime behavior, and diagnostics. When changing `.fsl` specs, verify syntax, bounded behavior, induction, mutation/vacuity where relevant, and update snapshots only when the semantic change is intentional.

## Commit & Pull Request Guidelines

History uses Conventional Commit-style messages, often with scopes, such as `feat(induction): ...`, `docs(examples): ...`, and `chore(release): ...`. Keep one topic per commit and add notable changes to `CHANGELOG.md` under `[Unreleased]`. Pull requests should describe the problem, the change, test evidence, linked issues, and any docs or skill updates needed for language changes.

## Agent-Specific Instructions

When adding language features, update grammar, model, verifier/runtime as needed, plus `docs/LANGUAGE.md`, relevant `docs/DESIGN-*.md`, and `skills/fsl/reference.md`. Do not weaken specs only to make checks pass; preserve intent and include reproducing `.fsl` cases for bugs.
