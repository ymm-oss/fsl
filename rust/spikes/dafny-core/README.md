# Dafny core spike (#1908)

An approved FSL spec becomes a TypeScript core that needs no code review: the
person owns the spec, and every step after it is generated or checked by a
machine.

```
node core.mjs <spec.fsl> [--ai] [--out <app dir>] [--watch]

 fslc kernel ─> gen.mjs ─> out/<Name>.gen.dfy     abstract module: state, init, guards, updates,
                                                   proof obligations without bodies; `symmetric`
                                                   types become a size parameter
 proofs/<Name>.proof.dfy                           refines it, supplies only lemma bodies
   (--ai: prove.mjs asks a model to write it when the current one fails)
 build.sh: structure check + dafny audit + dafny verify --verify-included-files
                                                   ─> the invariants hold for every size
 dafny translate js ─> out/<Name>.cjs              the code that was proved is the code that runs
 gen-ts.mjs ─> out/<Name>.ts (+ .d.cts), tsc       typed reducer over plain JSON
 check.mjs, through out/<Name>.ts, judged by fslc replay:
   exhaustive at the spec's sizes                  every reachable state × every action instance
   random at --scale × the sizes (default 4)       seeded; the generated Dafny must not change
```

Any failure exits nonzero. `--out` copies `<Name>.ts`, `<Name>.cjs` and
`<Name>.d.cts` into an app (which needs `bignumber.js`, the Dafny runtime's one
dependency); `--watch` re-runs whenever the spec or a file in `proofs/` changes.
One run takes about 1–3 minutes, most of it Dafny and `fslc replay` per step.

Requirements: `dafny` 4.11, a release `fslc` at `../../target/release/fslc`
(or `FSLC=...`), Node 22.18+, `npm ci`. `--ai` also needs the `claude` CLI
(`CLAUDE=...` to use another command).

```bash
npm ci
node core.mjs ../../../specs/seat_booking.fsl --widen SeatId,UserId
node example/seat-booking.ts   # an app using the generated core at 500 seats x 100 users
./run-all.sh                   # every positive run and negative control; RUN_AI=1 adds the real-model ones
```

## Using the core from TypeScript

`out/<Name>.ts` is generated from the Kernel. It is the only entry point:

```ts
import { init, step, validate, type Sizes, type State, type Action } from "./SeatBooking.ts";

const sizes: Sizes = { SeatId: 500, UserId: 100 };       // any size: the proof covers all
let state: State = init(sizes);                           // plain JSON: { seats: { "0": null, ... } }
const next = step(sizes, state, { action: "book", params: { s: 10, u: 7 } });
if (next) state = next;                                   // null: the spec does not allow it
```

- `State` and `Action` have the shape of `fslc replay` trace events, so a log of
  `(action, state)` pairs replays against the spec as it is.
- The wrapper accepts only what the proof covers and throws `CoreInputError` for
  anything else: sizes and ids must be safe integers (the compiled core accepts
  `2.5` as a size), map keys must be exactly the type's range, and an incoming
  state must satisfy `Wf` and `Inv`, the precondition of `<action>_StepPreserves`.
  A state from `init`/`step` always does; one read back from storage is checked
  again on every call.
- `step` is a reducer, so it fits `useReducer`, a store, or one DB transaction
  per action.

## What each piece guarantees

- **gen.mjs** translates Public Kernel v1 and fails closed (exit 2) on anything it
  does not handle. Types the spec declares `symmetric` are widened to a size
  `N_T` (`0 <= x < N_T`); `--widen` adds more, for specs that do not mark their
  identity types (neither `seat_booking` nor `cart_v1` does, and this spike does not
  change `specs/`). Other ranges and the implicit type bounds stay as proof
  obligations in `Wf`. Over `specs/*.fsl`: 8 translate, 10 are refused, 0 crash.
- **The proof module** cannot change what is proved. `build.sh` requires the file
  to contain one `include` (of the generated module) and one `module`
  (`<Name>Proof refines <Name>Spec`); without this a standalone `<Name>Proof`
  with `Inv = true` passes both `dafny verify` and `dafny audit`. Refinement keeps
  every body and `requires`/`ensures` of the generated module (Dafny rejects a
  redefined predicate or an added precondition), and `build.sh` refuses any
  `dafny audit` finding or warning (`assume`, `{:axiom}`, `{:verify false}`, a
  missing body). `--verify-included-files` makes Dafny verify the generated
  module's own obligations too (index ranges, termination), which it skips by default.
- **prove.mjs** (the AI half) runs `build.sh`; if the proof is missing or fails, it
  sends the generated module, the current proof and the errors to the model
  (`claude -p` with no tools), writes the reply to the proof file and runs the same
  `build.sh` again, up to `--attempts` (3) times. The model writes only the proof
  file, and only `build.sh` decides, so a wrong or cheating reply is rejected like
  a hand-written one, and a spec with a real bug cannot pass.
- **check.mjs** drives `out/<Name>.ts`, the same entry point an app imports, so the
  JSON conversion is inside the check. Each step goes to `fslc replay --trace` with
  the path that reached it: a step the core allows must be `conformant`, one it
  refuses must be `requires_failed`. Exhaustively, agreement on every step from the
  same initial state means equal transition relations at those sizes. This guards
  the translator, the Dafny compiler and the wrapper.

## Results (fslc 5.1.0 built from d1df480b, Dafny 4.11.0, Node 22.23)

| spec | widened | Dafny | exhaustive at spec sizes: states (core / `fslc verify --engine explicit`), steps | random at 4×: sizes, steps (seed 1) | divergences |
|---|---|---|---|---|---|
| `seat_booking.fsl` | `SeatId`, `UserId` (`--widen`) | 27 verified | 25 / 25, 225 | 12 × 8, 1500 | 0 |
| `cart_v1.fsl` | `UserId`, `ItemId` (`--widen`) | 27 verified | 36 / 36, 288 | 8 × 8, 1500 | 0 |
| `fixtures/task_board.fsl` | `TaskId`, `WorkerId` (`symmetric`) | 38 verified | 44 / 44, 660 | 12 × 8, 1500 | 0 |

`run-all.sh` also checks `seat_booking` and `cart_v1` exhaustively one size up
(`SeatId = 0..3`, `UserId = 0..2`): 63 states / 756 steps and 108 states / 1296
steps, 0 divergences, with byte-identical generated Dafny.

The proof loop with the real model (`claude` 2.1.326):
- From an empty proof file, `SeatBooking` was proved on the first attempt.
- `TaskBoard` has no hand-written proof: `proofs/TaskBoard.proof.dfy` is the model's
  third attempt. All three attempts failed on a `gen.mjs` defect, not on the
  proof: the generator emitted a trigger-less `forall k :: ... ==> true` for
  `Map<_, Bool>`, and Dafny warned. After that was fixed in `gen.mjs`, the same
  file passed every gate unchanged.
- `cart_v1_buggy.fsl` (`checkout` without the stock guard, so `Qty` can go to -1)
  ended in failure after 3 attempts, with no `assume`/axiom in the last reply.

Negative controls in `run-all.sh`, each of which must be caught:
- Differential: six in-memory mutants (a guard dropped, an effect skipped), a
  translator-shaped mutant (the stock guard of `checkout` lost in the generated
  Dafny), and a dropped guard under the random mode at 4× sizes.
- Proof: that translator mutant, a weakened per-user cap, a generated module whose
  `StepPreserves` lacks its proof.
- Audit: `assume`, `{:axiom}`, `{:verify false}`, an omitted lemma.
- Structure: a standalone module, an extra module, a second `include` not at the
  start of its line; Dafny itself rejects a redefined predicate and an added
  precondition.
- Wrapper: `controls/wrapper-inputs.ts` checks 21 inputs (non-integers, `NaN`,
  strings, `2**53+1`, negative or unknown sizes, unknown actions/parameters/fields,
  missing keys, a state outside the bounds, a state breaking `PerUserCap`), and it
  fails when the generated wrapper loses its `Inv` check or its integer check.
- Proof loop, with a stand-in model (`controls/fake-model.mjs`, no tokens): a reply
  that uses `assume` is rejected, a correct one is accepted.

## Not covered

- What holds for every size is the Dafny proof. That the Dafny model means the
  same as the FSL spec is checked only at finite sizes (the spec's, one up, and
  4× by random sequences), so the all-sizes claim also rests on `gen.mjs` being
  faithful.
- Kernel coverage is what these specs need: domain types, `Map`, `Option`, `Bool`,
  `Int`, `count`, quantifiers, `is some(x)` guards, `=>`. Not handled: `let`,
  partial operations (division), enums, structs, sets, two updates to one variable,
  `transitions`/`leadsTo` properties. `reachable` properties are not translated
  (they are not safety obligations).
- The wrapper checks `Inv` on every call (linear in the state), and `CountK`
  recursion depth grows with the size of the counted type.
- `fslc replay` judges the observed state only; it has no notion of action results
  or outputs, so neither does the core.
- Contract functions and the IO shell (DB transaction per action, outbox, replay
  of production logs) are the follow-ups listed in #1908.
- Not wired into CI (CI has no Dafny); no changelog fragment (no product change).
