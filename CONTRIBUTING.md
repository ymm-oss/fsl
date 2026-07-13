# Contribution Guide

Contributions to FSL (`fslc`) are welcome. Bug reports, feature proposals,
documentation improvements, and code changes are all appreciated.

## Read These First

- [`README.md`](README.md) — Overview and setup
- [`docs/LANGUAGE.md`](docs/LANGUAGE.md) — Language reference (complete)
- [`docs/README.md`](docs/README.md) — Map of the documentation
- [`docs/DESIGN-*.md`](docs/) — Design decisions for each feature
- [`docs/DOGFOOD-*.md`](docs/) — Dogfooding notes (record of bugs and findings)

## Development Environment

The only dependencies are `lark` (pure Python) and `z3-solver` (prebuilt wheel).
No C++ compiler or separate Z3 installation is required (Python 3.9+).

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"     # Installs lark, z3-solver, pytest and editable-installs fslc
```

## Testing

Changes must pass the tests.

```bash
pytest -q                   # Full test suite (about 8 minutes)
pytest tests/test_v1.py -q  # A single file
```

CI is tiered to keep pull-request feedback usable:

- Pull requests run the full suite once on Python 3.12.
- Pull requests also run a compatibility smoke suite on Python 3.9-3.12:
  `tests/test_version.py`, `tests/test_v1.py`, `tests/test_temporal.py`, and
  `tests/test_logic_temporal_sugar.py`.
- Pushes to `main`, scheduled runs, and manual workflow runs execute the full
  Python 3.9-3.12 matrix.

The test suite cross-checks the Z3 and concrete Monitor evaluators against each
other using witness diffs, and additionally validates against a Z3-independent
brute-force oracle (`tests/oracle.py`) to catch misses (false negatives where
something that is truly violated/refinement_failed is reported as
verified/proved/refines). **Add a regression test for any change that alters
behavior.**

## Guidelines for Changes

- **Changes to the verifier (`src/fslc/`)**: Do not break existing tests, and add
  tests under tests/ for new behavior. If you change detectors or semantics,
  update the corresponding `docs/DESIGN-*.md` as well.
- **Adding a language feature**: Update grammar/model/bmc (and runtime if needed)
  consistently, reflect the change in `docs/LANGUAGE.md` and
  `skills/fsl/reference.md`, and leave a `docs/DESIGN-<feature>.md`. These
  couplings are enforced by `tests/test_coupled_change_meta.py`: a new grammar
  production, dialect, or CLI subcommand fails CI until it is indexed in
  `src/fslc/lsp/index.py` and mapped to a `docs/DESIGN-<feature>.md` in
  `docs/README.md`, or recorded in the test's allowlist with an explicit
  reason (see `docs/DESIGN-coupled-change-metatest.md`).
- **Changing the normalized public Kernel contract**: update
  `schemas/fslc/kernel/`, the Rust exporter and Monitor/verifier agreement tests,
  checked-in conformance vectors, `docs/DESIGN-kernel-contract.md`,
  `docs/LANGUAGE.md`, `skills/fsl/reference.md`, and `CHANGELOG.md` together.
  Never silently drop an unsupported node; return an explicit semantic error.
- **Adding a dialect or a new `examples/` corpus directory**: register the
  dialect's top-level construct (and a `min_files` floor for its example
  corpus) in `tests/dialect_registry.py`. `tests/test_dialect_conformance.py`
  fails the build on any `.fsl` under `specs/`/`examples/` that no registry
  entry claims, and every claimed file must pass the full `parse -> desugar ->
  build_spec -> Monitor load -> BMC/Monitor expression agreement -> verify-vs-
  oracle verdict agreement` pipeline. A file the Monitor legitimately cannot
  load needs a documented `MONITOR_EXCLUSIONS` entry — never a silent skip
  (see `docs/DESIGN-conformance-harness.md`).
- **Adding or changing specs (`.fsl`)**: Run `fslc check` → `verify` →
  `--engine induction`, and also confirm the spec is non-vacuous (`fslc mutate`
  kill-rate, `--vacuity`). Avoid hollowing out specs (weakening invariants to
  make them go green).
- **Commit granularity**: One topic per commit. Add the key points to the
  `[Unreleased]` section of `CHANGELOG.md`.

## License and Copyright Notice

- This project is licensed under the **Apache License 2.0**. By submitting a pull
  request, you agree that your contribution is provided under the same license.
- Add an SPDX header at the top of new Python source files:

  ```python
  # SPDX-License-Identifier: Apache-2.0
  # Copyright <year> <your name>
  ```

- When making large changes to existing files, adding a copyright line is optional.

## Reporting and Proposals

- File bugs and proposals as GitHub Issues. Attach a minimal reproducing `.fsl`,
  the command you ran, and observed vs. expected behavior.
- Report security issues privately following the procedure in
  [`SECURITY.md`](SECURITY.md) rather than as an Issue.

We follow the [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
