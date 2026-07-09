# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`fslc` is the verifier for **FSL**, an AI-native formal specification language. A spec (`.fsl`) is
parsed, compiled to Z3, and checked by **bounded model checking** (BMC) and **k-induction**. Every
command emits **machine-readable JSON** on stdout — the tool is designed to sit inside an LLM
write→verify→repair loop, so the JSON envelope and exit codes are a stable contract, not incidental
output. The full language reference is `docs/LANGUAGE.md`; the doc map is `docs/README.md`.

## Commands

Dev setup (this is a git worktree; system Python lacks `z3`/`lark`, so a venv is required to run the
**working-tree** code — the globally installed `fslc` on `PATH` points at `~/.fsl`, not this tree):

```bash
python3 -m venv .venv && source .venv/bin/activate
pip install -e ".[dev]"        # lark, z3-solver, pytest + editable fslc
```

Run the CLI (after the editable install, `fslc …` and `python -m fslc …` both target the working tree):

```bash
fslc check  specs/cart_v1.fsl                     # parse + types only — the fast iteration loop
fslc verify specs/cart_v1.fsl --depth 8           # BMC: verdict + shortest counterexample/witness
fslc verify specs/cart_v1.fsl --engine induction  # k-induction (infinite-depth proof)
fslc refine impl.fsl abs.fsl mapping.fsl          # does the detailed spec refine the abstract one
fslc chain  fsl-project.toml                       # run business → requirements → design → impl
```

(Other subcommands: `scenarios`, `replay`, `testgen`, `mutate`, `explain`, `html`, `typestate` —
see `cli.py` `_build_arg_parser` for the complete surface.)

Testing:

```bash
pytest -q                          # full suite — SLOW (several minutes); not the inner-loop signal
pytest tests/test_v1.py -q         # one file
pytest tests/test_v1.py -k name    # one test
pip install pytest-xdist && pytest -n auto   # optional parallelism (xdist is not a dev dependency)
```

For fast iteration, gate on `fslc check`/`verify` of the specific spec and the one or two relevant
test files; reserve the full `pytest` run for final confirmation. **When you touch the verifier
semantics, dialects, or any `.fsl` under `specs/`/`examples/`, the corpus snapshot
(`tests/test_corpus_snapshot.py`) will diff — never skip it.** Regenerate it only after an
*intended* behavior change:

```bash
FSLC_SNAPSHOT_UPDATE=1 pytest tests/test_corpus_snapshot.py -q
```

## Architecture

**The pipeline (one path, every command shares it):** `parser.parse_src` runs the Lark grammar
(`grammar.py`, including the `Ast` transformer) to produce a tuple AST. The three *frontend dialects*
— `compose`, `requirements`, `business` — are **desugared into the same kernel AST** here
(`compose.py`, `dialects.py`) *before* anything downstream runs, so model/BMC only ever see kernel
specs. `model.build_spec` validates that AST and builds the `spec` dict (Z3 sorts, constants, typed
state/actions). An engine module then consumes the `spec` dict and returns a result `dict`. `cli.py`
wraps every result in `_envelope` (adds `{"fsl": "1.0", …}` + faithfulness metadata), prints JSON,
and maps the `result` field to an exit code via `exit_code()`.

This means: **a kernel-AST change ripples through `grammar → model → bmc → runtime`, and a new
surface syntax is usually a desugaring in `dialects.py`/`compose.py` that the kernel never knows
about.** Prefer adding to the frontend over widening the kernel.

**Dual evaluator + independent oracle (the core correctness invariant).** There are two evaluators
of FSL semantics that must agree:

- `bmc.py` (~4.7k lines, the heart) — symbolic: unrolls transitions into Z3 and solves.
- `runtime.py` `Monitor` — a concrete, Z3-free interpreter (also powers `replay` and `testgen`).

`tests/test_evaluator_agreement.py` cross-checks them step-by-step on witness replay. Separately,
`tests/oracle.py` is a **Z3-independent** BFS brute-forcer driving `Monitor` to catch *false
negatives* (something truly violated being reported verified/proved/refines) — the failure mode Z3
bugs hide. A change that makes BMC and Monitor disagree, or that the oracle catches, is a real
regression, not a flaky test.

**Module map by responsibility** (`src/fslc/`):

| Concern | Module |
|---|---|
| Grammar + AST transformer | `grammar.py` |
| Parse entry / refinement-file parse | `parser.py` |
| `build_spec`, type→Z3 sort, const eval, `FslError` | `model.py`, `values.py` |
| BMC `verify` / k-induction `prove` / `scenarios` / traces | `bmc.py` |
| Concrete interpreter (replay, testgen backend) | `runtime.py` |
| Refinement checking (`refine`, chains) | `refine.py` |
| Spec composition (namespaces, synchronized actions) | `compose.py` |
| Frontend dialects (requirements / business desugaring) | `dialects.py` |
| `mutate`, `explain`, `typestate`, `testgen`, `html` reports | same-named modules |
| Project manifest runner (`fslc chain`) | `chain.py` |
| CLI dispatch, JSON envelope, exit codes | `cli.py` |
| Acceptance / forbidden validation, faithfulness | `acceptance.py`, `diagnostics.py` |

**Three-layer dialects.** Specs are written in consulting (business) / requirements / design layers,
chained by refinement so requirement IDs propagate across diagnostics. The layers are one shared
kernel plus dialect frontends — see `docs/DESIGN-layers.md` and `docs/DESIGN-dialects.md`.

**JSON/exit-code contract** (`exit_code()` in `cli.py`): `0` = verified/proved/refines/conformant/
generated/typestate/mutated/explained/ok; `1` = violated/reachable_failed/unknown_cti/nonconformant/
refinement_failed; `2` = spec error (parse/name/type/semantics/io, and vacuity under `--vacuity
error`); `3` = internal error. A few commands (`testgen`, `explain --readable`, `typestate --ts`,
`html`/`testgen` without `-o`) write raw content to stdout instead of the JSON envelope.

## Conventions specific to this repo

- **New Python files need an SPDX header**: `# SPDX-License-Identifier: Apache-2.0` + `# Copyright <year> <name>`.
- **A language feature must move all of its files together**: grammar/model/bmc (and `runtime.py` if
  it affects concrete semantics), plus `docs/LANGUAGE.md`, `skills/fsl/reference.md`, and a
  `docs/DESIGN-<feature>.md`. Add a regression test for any behavior change. (See `CONTRIBUTING.md`.)
- **Changing `docs/LANGUAGE.md` or the CLI surface (`src/fslc/cli.py`) also moves the site**: run
  `python tools/build_site_reference.py` to regenerate `docs/intro/language.*.html` and
  `docs/intro/cli.*.html` (committed generated output — `tests/test_site_reference_snapshot.py`
  fails the build if you forget).
- **Do not "hollow out" specs** to make them go green — weakening an invariant to dodge a
  counterexample defeats the point. When adding/changing a `.fsl`, confirm it stays non-vacuous
  (`fslc mutate` kill-rate, `--vacuity`).
- **The `skills/` directory is canonical**; `.claude/skills/` are symlinks to it. Agent-facing
  language rules live in `skills/fsl/reference.md` and must track grammar changes.
- Commit one topic per change and add the key points to the `[Unreleased]` section of `CHANGELOG.md`.
