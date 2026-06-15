# DOGFOOD-11: Meta-Circular Dogfooding — Verifying fslc's Own Design Contract in FSL to Expose the Detectors' Blind Spots (2026-06-15)

DOGFOOD 1-10 targeted external domains such as banking, reservation, and SLA. This time, for the first time, we
modeled fslc's own behavioral contract — meta-circular dogfooding. The artifacts are the 3 specs in
`examples/self/`.

## Results

- `fslc_session` formally proved the CLI exit-code severity classification: success requires check pass,
  proved⊒verified, internal errors non-repairable.
- `fslc_monitor` proved replay reject-stickiness: once nonconformant is irreversible, conformant only when all steps
  ok.
- After the fix, `refinement_algebra` non-trivially checks "safety propagates, liveness does not".

## Insights

- **F22 (most important, a detector blind spot):** neither --vacuity nor a single verify detects a "tautological
  invariant over a state variable that is never assigned (a dead ghost)". The first draft of refinement_algebra was
  verified with 0 vacuity warnings, yet its mutate kill-rate was 6.4% (73/78 survived). The other 2 were 71%/67%.
  The mutate survival rate was the only indicator of hollowing (an extension of DOGFOOD-10 F21 "invariant weakening
  is visible only in mutate"). Improvement candidate: it closes cheaply if the vacuity check can statically warn on
  an "invariant/consequent that references only variables that are assigned by no action".

- **F23 (design/language gap):** there is no syntax to declare an intended terminal state (proved/conformant/
  tool_fault, etc.), so the only option is to apply --deadlock ignore globally → unintended deadlocks get hidden at
  the same time. repair_loop.fsl also requires --deadlock ignore for the same reason. A per-state/per-action
  terminal/final annotation would distinguish intended halting from a bug.

- **F24 (language gap):** there is no property syntax to directly assert "from this state this action cannot fire /
  this transition is forbidden", so it can only be expressed indirectly with ghost+guard. Occurred in all 3
  (RejectIsSticky / NoStepAfterReject / ToolFaultNotRepairable).

- **F25 (expressiveness):** relational/algebraic properties like reflexivity and transitivity of refinement cannot
  be written as axioms; one can only "simulate the process" as a state machine. This tends to invite the dead-ghost
  trap of F22 (demonstrated by refinement_algebra).

- **F26 (minor):** the --deadlock=warn warning message string lacks the deadlock state name ("deadlock reachable at
  step N" only). The JSON deadlock.trace contains the full final state (bmc.py:2851 just doesn't put it in the
  string).

- **F27 (testability):** there is no means to check targeting a single invariant only (an equivalent of
  --property/--invariant). verify checks all invariants at once and reports "the first violation found", so even
  when you want to confirm a violation of a specific invariant (e.g. SafetyPropagates) with a non-vacuity probe, a
  more general invariant (SafetyPreservedAtEveryLayer) gets reported first, requiring effort to narrow conditions so
  the targeted invariant becomes the reported one. A single-property option is a candidate to improve probe
  precision.

## Modification Status

Of the findings the investigation surfaced, those for which code modification was begun:

| Finding | Action | Status |
|---|---|---|
| F23 (declaring intended halting) | Added a new `terminal { <predicate> }` block (grammar/model/bmc). Halting states satisfying the predicate are excluded from the deadlock check. Made examples/self terminal and removed the `--deadlock ignore` dependency | **done** (`94cf68f`) |
| F26 (deadlock state display) | Include the state in the warn message. E.g. `deadlock reachable at step 1 (state: status=ToolFault, ...)` | **done** (`94cf68f`) |
| F27 (single-invariant check) | Added `verify --property <Name>`. A nonexistent name is a usage error (exit 2) | **done** (`94cf68f`) |
| F22 (dead-ghost tautology) | Added to `--vacuity` a Z3 static detection of an "invariant that becomes a tautology regardless of the dynamic variables' values, when frozen variables assigned by no action are fixed to their init values" (kind `tautology_over_frozen`). Invariants that reference no frozen variable / reference no state are excluded. Tidied refinement_algebra's trivial baseline ghosts (mutate kill-rate held at 77.2%). Confirmed zero false positives across the entire existing corpus | **done** |
| F24 (transition-forbidden syntax) | Added a new transition invariant `trans { old(x) => ... }` (grammar/model/bmc/runtime). Two-state safety across actions can be declared directly, expressing the self-spec's sticky/irreversibility properties without a ghost. Checked by BMC + induction step-case + replay (DESIGN-trans.md) | **done** |
| F25 (expressiveness of algebraic properties) | An essential limit of the language. Out of scope for modification | deferred |

## Anchoring to Implementation Conformance (Model Verification → Implementation Verification)

Initially the self-spec was **a model describing fslc's design contract**, and what `verify`/induction proved was
only **the model's internal consistency**. There was no link between the model and the real code (`src/fslc/cli.py`),
so "does the implementation uphold this contract" was unverified — the core gap of this project (what fslc
guarantees is "internal consistency of the written spec", not "fidelity of the spec to reality") applied to the
self-spec too.

`tests/test_self_conformance.py` filled this gap. It runs the real CLI pipeline (check → verify → induction) over a
spec corpus producing diverse outcomes, and:
1. each result and the process exit code match `exit_code()`'s severity table (the real exit code is checked
   directly),
2. `ProvedImpliesVerified` / `SuccessRequiresCheck` hold on the real results,
3. the real result sequence is mapped onto `fslc_session`'s action sequence and `fslc replay` is **conformant**
   (the real CLI's transitions conform to the model state machine),
4. a hand-written contract-violating trace is **nonconformant** (`verify_ok` alone is rejected by
   `requires status==CheckOk` = the anchor has teeth, a negative control).

With this, meta-circular dogfooding was lifted from "model verification" to "**implementation-conformance
verification**".

Coverage was then extended further:
- **fslc_session**: in addition to the core check→verify→induction, a verify-time user error
  (added a `verify_user_error` action; check passes only syntax/type but verify becomes a semantics error, e.g.
  no_actions.fsl), and the auxiliary subcommands (scenarios/explain/mutate/typestate/refine success·failure/replay
  conformant·nonconformant) are run against the real CLI, mapped onto actions, and confirmed conformant.
- **fslc_monitor**: runs the real `Monitor`/`run_replay` on a guarded spec (cart_v1) for normal / mid-way reject /
  empty log, directly asserting that "halt on the first reject and process nothing afterward (confirmed via
  `failed_at_event` and log length)" matches NoStepAfterReject + replays it into the monitor. The negative controls
  (step_ok after reject, etc.) are nonconformant.

The only unanchored case is **tool_fault (internal error = exit 3)** — because internal errors are not triggered
deliberately (it is kept in the model). Within the current corpus, there is no discrepancy between the model
contract and the real behavior.

## Reproduction

```bash
E=examples/self

# fslc_session / fslc_monitor have terminal { } declarations, so --deadlock ignore is not needed
./.venv/bin/python -m fslc check  $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl --engine induction
./.venv/bin/python -m fslc mutate $E/fslc_session.fsl

./.venv/bin/python -m fslc check  $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl --engine induction
./.venv/bin/python -m fslc mutate $E/fslc_monitor.fsl

./.venv/bin/python -m fslc check  $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl --engine induction
./.venv/bin/python -m fslc mutate $E/refinement_algebra.fsl
```
