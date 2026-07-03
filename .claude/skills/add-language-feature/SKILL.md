---
name: add-language-feature
description: Scaffold and checklist a new FSL kernel language feature so every file the repo requires to move together actually moves. Use when adding or changing FSL surface syntax or kernel semantics (grammar, model, verifier, or runtime behavior) — it enforces the "a language feature moves all of its files together" rule from CLAUDE.md/CONTRIBUTING.md and the corpus-snapshot discipline.
---

# add-language-feature — move all the coupled files together

CLAUDE.md and CONTRIBUTING.md require that a language feature move **all** of its files
in one change. The single most common mistake is landing the grammar/verifier edit and
forgetting a doc, the agent-facing reference, the design note, the regression test, or
the snapshot regeneration. This skill turns that rule into a checklist and does the
scaffolding.

## First decide: frontend or kernel?

The pipeline is `parser → (dialects/compose desugar) → model.build_spec → bmc/runtime`.
**Prefer adding to the frontend over widening the kernel.** A new *surface syntax* is
usually a desugaring in `dialects.py` / `compose.py` that lowers to existing kernel AST —
the kernel, model, and bmc never change. Only widen the kernel when the feature genuinely
needs new semantics. Say which path this feature takes before editing.

## The coupled-file checklist

Kernel-AST change (new semantics):
- [ ] `src/fslc/grammar.py` — grammar rule + `Ast` transformer producing the kernel tuple AST
- [ ] `src/fslc/model.py` — `build_spec` validation + type→Z3 sort handling for the new node
- [ ] `src/fslc/bmc.py` — symbolic evaluation (unroll into Z3)
- [ ] `src/fslc/runtime.py` — **concrete `Monitor` evaluation, if concrete semantics change**
      (the dual evaluator must agree; see the `fsl-soundness-reviewer` agent)

Frontend-only change (desugaring):
- [ ] `src/fslc/dialects.py` and/or `src/fslc/compose.py` — desugar to existing kernel AST

Always, regardless of path:
- [ ] `docs/LANGUAGE.md` — the complete language reference (must stay complete)
- [ ] `skills/fsl/reference.md` — the agent-facing rules (must track grammar changes;
      `.claude/skills/fsl` is a symlink to `skills/fsl`, so edit the canonical file)
- [ ] `docs/DESIGN-<feature>.md` — a new design note recording the decision and rationale
- [ ] `tests/test_<feature>.py` — a regression test for the new behavior
- [ ] `CHANGELOG.md` — a bullet under `## [Unreleased]`
- [ ] SPDX header on any *new* `.py` file (`# SPDX-License-Identifier: Apache-2.0` + copyright)

## Verify (fast loop first, snapshot last)

1. Iterate with `.venv/bin/python -m fslc check <spec>` and
   `… verify <spec> --depth N` on a spec that exercises the feature — this is the fast
   signal, not the full suite.
2. Prove the infinite-depth case where relevant: `… verify <spec> --engine induction`.
3. Run the targeted tests: `pytest tests/test_<feature>.py tests/test_evaluator_agreement.py -q`.
4. **The corpus snapshot will diff.** Never skip `tests/test_corpus_snapshot.py`. If the
   diff reflects the intended behavior change, regenerate it (and eyeball the diff first):
   `FSLC_SNAPSHOT_UPDATE=1 .venv/bin/python -m pytest tests/test_corpus_snapshot.py -q`
5. Final confirmation before wrapping up: `pytest -q` (slow, ~8 min).

## Don't

- Don't weaken/hollow a spec to make it pass. Confirm non-vacuity (`fslc mutate` kill-rate,
  `--vacuity`) — see the `new-spec` skill.
- Don't let bmc and runtime disagree; that is a real regression the agreement test and the
  Z3-independent oracle exist to catch.
