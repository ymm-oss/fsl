# Dogfooding Round 3 — Full Workflow Demonstration (2026-06-11)

We ran "the development flow FSL envisions" — penetrating every layer of v2.0/v2.1 — end to end on a new domain
(a bank account with a two-tier ledger + audit log).

## Workflow and Results

| Stage | Artifact | Result |
|---|---|---|
| 1. Abstract spec | `specs/bank.fsl` (an account with immediate balance) | **proved (k=1)** on the first try |
| 2. Refinement | `specs/bank_impl.fsl` (a two-tier cleared + pending ledger) | **proved (k=1)** on the first try |
| 3. Faithfulness check | `specs/bank_refines.fsl` (`balance = cleared + pending`) | **refines** on the first try. settle is a stutter, and the strengthened withdraw guard (cleared only) is correctly permitted |
| 4. Composition | `specs/bank_system.fsl` (bank_impl + audit_log, synchronized actions + internal) | verified + **proved (k=1)**. The cross-cutting invariant `audit.balance == cleared + pending + withdrawn` is inductive while coexisting with the components' Seq aggregation invariant |
| 5. Implementation hookup | `examples/bank/` (a plain Python implementation + testgen-generated harness + Adapter wiring) | **8/8 passed** (7 scenario replays + a 100-step random walk with Monitor as the oracle) |

`examples/bank/bank.py` is ordinary app code that knows nothing about FSL. With only the Adapter wiring (about 20 lines),
conformance tests generated from the spec check the implementation's correctness — this is the finished form of the
"bridge between spec and implementation" envisioned since DESIGN-v1.

## Discoveries (2 — both fixed)

### BUG16: testgen mixes display-name dots into the generated function names (SyntaxError)

The composed spec's scenario names (`reach_bank.Settled`) became function names verbatim, making the generated
file un-importable. Fixed with identifier sanitization + collision-numbering + preserving the original name in a
docstring. A "display-layer boundary" bug of the same family as round 2's F6 — a missed propagation of compose's
display-name handling (`__` → `.`).

### BUG17: testgen embeds a cwd-relative path / Monitor mis-classifies path vs. source

The artifact embeds `SPEC_PATH = 'specs/...'`, so it cannot run from anywhere but the repository root. Furthermore,
Monitor parses a nonexistent path string as FSL source, so a failure that should be an io error becomes
UnexpectedCharacters (a repair-protocol violation). Fixed with relative-path resolution anchored at the generated
file + path classification in Monitor.

## Findings

- **F8: every stage of the workflow passed "on the first try".** Unlike rounds 1 and 2, no spec-induced CTIs or
  counterexamples appeared. "Stepwise refinement" — proceeding through abstract → detailed → composed while keeping
  proved at each stage — holds up as the actual feel of using this toolchain.
- **F9: a conditional expression cannot be written in a refinement mapping expression.** (resolved in v2.2) The
  seat-reservation domain we initially considered needed something equivalent to
  `map seats[s] = (st == Sold ? some(holder) : none)`, and since FSL had no conditional expression, it could not be
  expressed as a mapping, so we changed the domain.
  → Initially implemented as an **`if-then-else` expression restricted to mapping expressions** (DESIGN-refinement §2.5).
  The abandoned seat-reservation domain itself became a second concrete example, and we confirmed that
  `map seats[s] = if slots[s].st == Sold then slots[s].holder else none` passes refines in
  `specs/seat_booking{,_impl}.fsl` + `seat_refines.fsl` (the abstract side's count aggregation evaluates correctly
  over the conditional mapping value). Issue #245 later promoted the same node
  to the shared expression grammar; this paragraph records the original v2.2 limitation.
- **F10: the Adapter wiring convention is clear enough.** observe()'s projection (display-name keys, Seq as list,
  Option as None|value) follows the LANGUAGE.md convention with no hesitation. It is practically powerful that the
  random walk automatically reconciles settle's "nothing to settle" guard with the spec's `requires pending > 0`.

## Statistics

- 5 new specs (bank / bank_impl / bank_refines / bank_system / examples), bringing the repository's proved specs to
  13 total (everything except the 2 buggy samples)
- The 2 new bugs (BUG16/17) are both in the generation/bridge subsystems. Zero defects in the verification core
  (BMC / induction / refine / compose semantics) this round.
