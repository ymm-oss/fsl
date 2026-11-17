# Dafny core spike (#1908)

An approved FSL spec becomes a TypeScript core that needs no code review: the
person owns the spec, and every step after it is generated or checked by a
machine.

```
node core.mjs <spec.fsl> [--widen T1,T2] [--ai] [--depth n] [--out <app dir>] [--watch]

 fslc kernel ─> gen.mjs ─> out/<Name>.gen.dfy     abstract module: state, init, guards, updates,
                                                   proof obligations without bodies; `symmetric`
                                                   types become a size parameter
 proofs/<Name>.proof.dfy                           refines it, supplies only lemma bodies
   (--ai: prove.mjs asks a model to write it when the current one fails)
 build.sh: structure check + dafny audit + dafny verify --verify-included-files
                                                   ─> the invariants hold for every size
 dafny translate js ─> out/<Name>.cjs              the code that was proved is the code that runs
 gen-ts.mjs ─> out/<Name>.ts (+ .d.cts), tsc       typed reducer over plain JSON
 check.mjs, through out/<Name>.ts:
   exhaustive at the spec's sizes                  one `fslc conformance` call: every reachable
                                                   state × every action instance (to --depth)
   random at --scale × the sizes (default 4)       seeded sequences judged by `fslc replay`;
                                                   the generated Dafny must not change
```

Any failure exits nonzero. `--out` copies `<Name>.ts`, `<Name>.cjs` and
`<Name>.d.cts` into an app (which needs `bignumber.js`, the Dafny runtime's one
dependency); `--watch` re-runs whenever the spec or a file in `proofs/` changes.
`--depth` (default 64) bounds the exhaustive search for specs whose state space
does not close (an unbounded `Int` counter); the report says `closed: false`.
One run takes about 20–30 s (Payment, 1600 states: 27 s), most of it Dafny and
the random-mode replays.

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

- `State` and `Action` have the shape of `fslc replay` trace events (enums as
  member names, structs as objects), so a log of `(action, state)` pairs replays
  against the spec as it is. Enums and structs also become TypeScript types.
- The wrapper accepts only what the proof covers and throws `CoreInputError` for
  anything else: sizes and ids must be safe integers (the compiled core accepts
  `2.5` as a size), enum values must be members, map keys and struct fields must be
  exactly the declared ones, and an incoming state must satisfy `Wf` and `Inv`, the
  precondition of `<action>_StepPreserves`.
- States returned by `init` and `step` are deeply frozen and remembered (per
  sizes), so `step` does not re-check their `Inv`: the proof already says they
  satisfy it. A copy, or a state read back from storage, is a new object and is
  checked in full; `validate` always checks in full.
- `step` is a reducer, so it fits `useReducer`, a store, or one DB transaction
  per action.

## What each piece guarantees

- **gen.mjs** translates Public Kernel v1 and fails closed (exit 2) on anything it
  does not handle. Types the spec declares `symmetric` are widened to a size
  `N_T` (`0 <= x < N_T`); `--widen` adds more, for specs that do not mark their
  identity types (this spike does not change `specs/`). Other ranges and the
  implicit type bounds stay as proof obligations in `Wf`. Handled: domain types,
  enums, structs, `Map`, `Option`, `Bool`, `Int`, field reads and field updates,
  several updates to one variable, `if` in an action body, `count`, `sum`, `min`,
  `max`, quantifiers, `is some(x)` guards, `=>`. fslc rejects a spec whose writes
  may hit one location twice, so distinct writes to one variable are applied in
  turn. Over `specs/*.fsl`: 14 translate, 4 stop with exit 2 (sequences, sets), 5
  are refused by `fslc kernel` (refinement mappings), 0 crash.
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
  `build.sh` again, up to `--attempts` (3) times. A failure located in the generated
  module is a `gen.mjs` defect that no proof can fix; it exits 3 without asking.
  The model writes only the proof file, and only `build.sh` decides, so a wrong or
  cheating reply is rejected like a hand-written one, and a spec with a real bug
  cannot pass.
- **check.mjs** drives `out/<Name>.ts`, the same entry point an app imports, so the
  JSON conversion is inside the check.
  - Exhaustive mode asks `fslc conformance` once for every state reachable within
    the depth and the outcome of every action instance in each. The core must
    accept each of those states (so a generated `Inv` that is too strong is caught),
    allow exactly the `ok` outcomes with the same post-state, and refuse the
    `requires_failed` ones; any other outcome (such as `type_bound`) is a
    divergence. Agreement from the same initial state means equal transition
    relations within the depth.
  - Random mode replays seeded sequences on the enlarged spec with `fslc replay`:
    one replay per run for its allowed steps, plus up to three refused steps per run.
  - Neither mode shows that a generated `Inv` is not too weak; see
    `controls/invariant-states.ts` below.

## Results (fslc 5.1.0 built from d1df480b, Dafny 4.11.0, Node 22.23)

All proofs except SeatBooking's and ShoppingCart's were written by the model
through `prove.mjs` (`claude` 2.1.326), each on its first attempt. All rows have
0 divergences in both modes.

| spec | widened | constructs | Dafny | exhaustive: states, steps | random at 4× (seed 1): sizes, steps |
|---|---|---|---|---|---|
| `seat_booking.fsl` | `SeatId`, `UserId` | count | 27 verified | 25, 225 | 12 × 8, 876 |
| `cart_v1.fsl` | `UserId`, `ItemId` | `is some` guard | 27 verified | 36, 288 | 8 × 8, 756 |
| `fixtures/task_board.fsl` | `TaskId`, `WorkerId` (`symmetric`) | count, `=>` | 38 verified | 44, 660 | 12 × 8, 546 |
| `repair_loop.fsl` | – | enum | 58 verified | 47, 376 | –, 396 |
| `rate_limiter.fsl` | – | `min`, Int | 23 verified | 46, 138 (depth 8) | –, 1124 |
| `bank.fsl` | – | Int | 16 verified | 25, 200 (depth 8) | –, 1122 |
| `bank_impl.fsl` | – | Int | 23 verified | 235, 2115 (depth 8) | –, 926 |
| `auth_lockout.fsl` | `UserId` | struct, `if`, field updates | 39 verified | 81, 648 | 8, 790 |
| `cart_impl.fsl` | `UserId`, `ItemId` | – | 35 verified | 441, 4410 | 8 × 8, 927 |
| `seat_booking_impl.fsl` | `SeatId`, `UserId` | enum, struct | 45 verified | 123, 2214 | 12 × 8, 523 |
| `inventory_reservation.fsl` | `ResId`, `ItemId` | enum, struct, `sum` | 46 verified | 241, 8676 | 12 × 8, 404 |
| `payment.fsl` | `PayId` | enum, struct, `sum`, Int | 46 verified | 1600, 51200 | 8, 494 |

For the closed specs, the state counts equal `fslc verify --engine explicit`
(25, 36, 44, 123, 241, 1600). `run-all.sh` also checks `seat_booking` and
`cart_v1` exhaustively one size up: 63 states / 756 steps and 108 / 1296.

The proof loop with the real model:
- From an empty proof file, `SeatBooking` was proved on the first attempt.
- `proofs/TaskBoard.proof.dfy` is the model's third attempt. All three attempts
  failed on a `gen.mjs` defect (a trigger-less `forall ... ==> true` for
  `Map<_, Bool>`), not on the proof; after the fix the same file passed.
- `cart_v1_buggy.fsl` (`checkout` without the stock guard, so `Qty` can go to -1)
  ended in failure after 3 attempts.
- The differential check caught one translation defect the proofs could not:
  for AuthLockout and Payment (one widened type), Dafny's JS backend erases the
  one-field `Config` datatype, and the wrapper built empty states. The proofs were
  fine; the wrapper now goes only through generated `MkConfig`/`Mk_*`/`Get_*`/`Is_*`
  functions.

Negative controls in `run-all.sh`, each of which must be caught:
- Differential: six in-memory mutants (a guard dropped, an effect skipped), a
  dropped guard under the random mode, and generated-Dafny mutants: the stock guard
  of `checkout` lost, an `if` condition changed (AuthLockout), the wrong enum member
  set (Payment), `min` turned into `max` (RateLimiter), and the ledger `sum` adding
  refunds (Payment, caught because the core refuses states fslc reaches). Each must
  report divergences, not just exit nonzero.
- Proof: the lost stock guard, a weakened per-user cap, a generated module whose
  `StepPreserves` lacks its proof, and `cart_v1_buggy` through `core.mjs`.
- Audit: `assume`, `{:axiom}`, `{:verify false}`, an omitted lemma.
- Structure: a standalone module, an extra module, a second `include` not at the
  start of its line; Dafny itself rejects a redefined predicate and an added
  precondition.
- Invariants: `controls/invariant-states.ts` classifies five TaskBoard states
  (separating `=>` from `and` and from the reversed implication), and fails for
  cores whose generated `DoneIsFree` is `true`, uses `and`, or is reversed, or
  whose `WipLimit` is `true`.
- Wrapper: `controls/wrapper-inputs.ts` checks 23 inputs (non-integers, `NaN`,
  strings, `2**53+1`, negative or unknown sizes, unknown actions/parameters/fields,
  missing keys, a state outside the bounds, a state breaking `PerUserCap`, a state
  made for other sizes, freezing of returned states). It fails when the generated
  wrapper loses its `Inv` check, its integer check, the sizes check on trusted
  states, or the freeze.
- Proof loop, with a stand-in model (`controls/fake-model.mjs`, no tokens): a reply
  that uses `assume` is rejected, a correct one is accepted.

Speed: one `book` at 10,000 seats × 1,000 users takes 5 ms (1.5 s when every step
re-checked `Inv`), 100,000 × 1,000 57 ms; 1,000,000 seats run. The exhaustive check
of SeatBooking takes 0.2 s with `fslc conformance` (15 s with one `fslc replay`
process per step). Each `fslc replay` call takes about 0.36 s of wall time with
almost no CPU, so the random mode keeps them few.

## Not covered

- What holds for every size is the Dafny proof. That the Dafny model means the
  same as the FSL spec is checked only at finite sizes (the spec's, one up, and
  4× by random sequences), so the all-sizes claim also rests on `gen.mjs` being
  faithful.
- A generated `Inv` that is too weak is caught only by the five hand-classified
  TaskBoard states; the differential shows only that it is not too strong.
- Kernel coverage: sequences (`seq_lit`, `Seq` methods), sets, `let`, partial
  operations (division), relations, `transitions`/`leadsTo` properties. `leadsTo`
  (liveness) is a poor fit for this approach and stays with fslc. `reachable`
  properties are not translated (they are not safety obligations).
- Specs that share a spec name (`cart_buggy`, `cart_fixed` and `cart_v1` are all
  `ShoppingCart`) share `out/` and `proofs/` paths; only `cart_v1` is run.
- Cost left: `step` still checks `Wf` and encodes the whole state (linear), and an
  untrusted state is checked in full (|quantified| × |counted|).
- `fslc replay` and `fslc conformance` judge states only; there is no notion of
  action results or outputs, so neither does the core.
- Contract functions and the IO shell (DB transaction per action, outbox, replay
  of production logs) are the follow-ups listed in #1908.
- Not wired into CI (CI has no Dafny); no changelog fragment (no product change).
