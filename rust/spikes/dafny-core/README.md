# Dafny core spike (#1908)

An approved FSL spec becomes a TypeScript core that needs no code review: the
person owns the spec, and every step after it is generated or checked by a
machine.

```
node core.mjs <spec.fsl> [--widen T1,T2] [--name N] [--ai] [--depth n] [--liveness-by-fslc] [--out <app dir>] [--watch]

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
 testgen.mjs                                       `fslc testgen --target vitest`, unchanged but
                                                   for its adapter, run against out/<Name>.ts
   random at --scale × the sizes (default 4)       seeded sequences judged by `fslc replay`;
                                                   the generated Dafny must not change
 invcheck.mjs                                      the generated invariants against fslc's on
                                                   sampled unreachable states (too weak/strong)
```

Any failure exits nonzero. `--out` copies `<Name>.ts`, `<Name>.cjs` and
`<Name>.d.cts` into an app (which needs `bignumber.js`, the Dafny runtime's one
dependency); `--watch` re-runs whenever the spec or a file in `proofs/` changes.
`--depth` (default 64) bounds the exhaustive search for specs whose state space
does not close (an unbounded `Int` counter); the report says `closed: false`.
Widening scales the type and nothing else: constants and int ranges keep their
values (MutexQueue at 12 processes still queues at most `NPROC = 3`), and
`core.mjs` refuses to widen a type whose bound is a constant the spec also reads
(`fixtures/negative/const_bound.fsl`). `--name` sets the output name for specs that share a spec name (`cart_fixed` is
also `ShoppingCart`). `--liveness-by-fslc` accepts a spec with `leadsTo`: Dafny
proves only the safety obligations, `fslc verify` must pass the liveness ones at
the spec's sizes, and the report carries that verdict under
`liveness_not_proved_by_dafny`. The core's transitions equal the spec's, so the
verdict carries over to the core at those sizes, not to every size.
One run takes about 25–55 s (SeatBooking 25 s, Payment and OrderWorkflow 55 s),
most of it Dafny, the random-mode replays and invcheck.

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
  enums, structs, `Map`, `Option`, `Bool`, `Int`, bounded `Seq<T, N>` (`size`,
  `push`, `pop`, `head`, `at`, `contains`), `Set<T>` (`size`, `add`, `remove`,
  `contains`), field reads and field updates, several updates to one variable,
  `if` in an action body, `let` (also between guards), `count`, `sum`, `min`, `max`,
  `/` and `%`, quantifiers over types and int ranges, `is some(x)` bindings under
  `and` and `=>`. fslc rejects a spec whose writes may hit one location twice, so
  distinct writes to one variable are applied in turn. Partial operations
  (`head`/`pop`/`at` on a too-short `Seq`, a zero divisor) become Dafny
  well-formedness obligations where they occur, so a generated module verifies only
  if none is reachable; Dafny's int division is Euclidean like FSL's. Over
  `specs/*.fsl`: all 18 that `fslc kernel` accepts translate (`mutex_queue` with
  `--liveness-by-fslc`), and 5 are refused by `fslc kernel` (3 refinement mapping
  files with no state, 2 `compose` specs that Kernel v1 cannot carry).
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
  module (a `gen.mjs` defect, or a partial operation the spec can reach) is one no
  proof can fix; it exits 3 without asking.
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
  - Neither mode shows that a generated `Inv` is not too weak: every reachable
    state satisfies both.
- **testgen.mjs** runs the suite `fslc testgen --target vitest` emits for the spec
  (its cover/reach scenarios and its random walk, with the Monitor's states baked
  in) against `out/<Name>.ts`. Only two things are wired: `makeAdapter()` drives
  the core, and the `vitest` import is a small runner (`testgen-runner.mjs`).
  The suite compares only the fields its expected states name, so the check also
  requires every step it does not mark forbidden to be taken by the core. Within
  the spec's sizes this is a subset of the exhaustive mode; it is there because
  #1908 names testgen traces, and it checks the suite an app would get from fslc.
  `forbidden` scenarios exist only in the requirements and business dialects (a
  kernel `spec` refuses them), so for the specs here that branch is unused.
- **invcheck.mjs** does. It samples states inside the type bounds (reachable states
  with one value changed, and random ones), asks the core whether `validate`
  accepts each, and asks fslc on a copy of the spec whose `init` is that state:
  `fslc verify --depth 0` reports `violated` at step 0 exactly when it breaks an
  invariant. Any disagreement fails the run.

## Results (fslc 5.1.0 built from d1df480b, Dafny 4.11.0, Node 22.23)

`./run-all.sh`: 78/78 expectations, on two runs with identical reports. All
proofs except SeatBooking's and ShoppingCart's were written by the model through
`prove.mjs` (`claude` 2.1.326): TaskBoard's on the third attempt (the first two
failed on a `gen.mjs` defect), every other one on the first. Every row has 0 divergences and 0 invariant disagreements,
and every row's `fslc testgen` suite passes (column "testgen").

| spec | widened | constructs | Dafny | exhaustive: states, steps | random at 4× (seed 1): sizes, steps | testgen: scenarios, steps | invcheck: states (hold / break) |
|---|---|---|---|---|---|---|---|
| `seat_booking.fsl` | `SeatId`, `UserId` | count | 27 verified | 25, 225 | 12 × 8, 876 | 4, 105 | 27 (25 / 2) |
| `cart_v1.fsl` | `UserId`, `ItemId` | `is some` guard | 27 verified | 36, 288 | 8 × 8, 756 | 5, 109 | 144 (144 / 0) |
| `cart_fixed.fsl` (`--name CartFixed`) | `UserId`, `ItemId` | constant bounds | 29 verified | 36, 288 | 8 × 8, 756 | 4, 105 | 200 (145 / 55) |
| `fixtures/task_board.fsl` | `TaskId`, `WorkerId` (`symmetric`) | count, `=>` | 38 verified | 44, 660 | 12 × 8, 546 | 6, 33 | 200 (44 / 156) |
| `fixtures/batching.fsl` | – | `/`, `%` | 25 verified | 1395, 20925 (depth 8) | 1055 | 4, 104 | 200 (63 / 137) |
| `repair_loop.fsl` | – | enum | 58 verified | 47, 376 | 396 | 12, 35 | 64 (56 / 8) |
| `rate_limiter.fsl` | – | `min`, Int | 23 verified | 46, 138 (depth 8) | 1124 | 5, 110 | 200 (128 / 72) |
| `bank.fsl` | – | Int | 16 verified | 25, 200 (depth 8) | 1122 | 4, 104 | 9 (7 / 2) |
| `bank_impl.fsl` | – | Int | 23 verified | 235, 2115 (depth 8) | 926 | 5, 108 | 200 (146 / 54) |
| `auth_lockout.fsl` | `UserId` | struct, `if`, field updates | 39 verified | 81, 648 | 8, 790 | 7, 116 | 200 (84 / 116) |
| `cart_impl.fsl` | `UserId`, `ItemId` | – | 35 verified | 441, 4410 | 8 × 8, 927 | 5, 106 | 200 (200 / 0) |
| `seat_booking_impl.fsl` | `SeatId`, `UserId` | enum, struct | 45 verified | 123, 2214 | 12 × 8, 523 | 6, 110 | 200 (61 / 139) |
| `inventory_reservation.fsl` | `ResId`, `ItemId` | enum, struct, `sum` | 46 verified | 241, 8676 | 8 × 12, 404 | 6, 22 | 200 (26 / 174) |
| `payment.fsl` | `PayId` | enum, struct, `sum`, Int | 46 verified | 1600, 51200 | 8, 494 | 7, 20 | 200 (25 / 175) |
| `audit_log.fsl` | – | `Seq`, int-range `forall`, `sum` over `at` | 15 verified | 121, 484 | 350 | 4, 13 | 200 (10 / 190) |
| `job_pipeline.fsl` | `JobId` | `Seq`, `let` between guards, `=>` binding | 53 verified | 544, 3808 | 12, 621 | 8, 37 | 200 (66 / 134) |
| `order_workflow.fsl` | `OrderId` | `Set`, `if` | 45 verified | 9261, 250047 | 12, 540 | 7, 24 | 200 (36 / 164) |
| `mutex_queue.fsl` (`--liveness-by-fslc`) | `ProcId` | `Seq`, `leadsTo` left to fslc | 34 verified | 16, 128 | 12, 735 | 10, 121 | 160 (16 / 144) |

- `mutex_queue`'s `leadsTo WaiterGetsLock` is `verified`, `bounded` (depth 8) by
  `fslc verify`; it is not proved for every size.
- `cart_v1` and `cart_impl` have no invariants beyond the type bounds, so invcheck
  finds every in-bounds state holds.
- The exhaustive states come from `fslc conformance`; for the closed specs they
  also match `fslc verify --engine explicit` (25, 36, 44, 123, 241, 1600), but that
  is fslc agreeing with itself, not evidence about the core. `run-all.sh` also checks `seat_booking` and
  `cart_v1` exhaustively one size up: 63 states / 756 steps and 108 / 1296.

The proof loop with the real model:
- From an empty proof file, `SeatBooking` was proved on the first attempt.
- `proofs/TaskBoard.proof.dfy` is the model's third attempt. All three attempts
  failed on a `gen.mjs` defect (a trigger-less `forall ... ==> true` for
  `Map<_, Bool>`), not on the proof; after the fix the same file passed.
- `cart_v1_buggy.fsl` and `cart_buggy.fsl` (both lose the stock guard of
  `checkout`) ended in failure after 3 attempts each.
- The differential check caught one translation defect the proofs could not:
  for AuthLockout and Payment (one widened type), Dafny's JS backend erases the
  one-field `Config` datatype, and the wrapper built empty states. The wrapper now
  goes only through generated `MkConfig`/`Mk_*`/`Get_*`/`Is_*` functions.

Negative controls in `run-all.sh`, each of which must be caught:
- testgen suite: a skipped `checkout` effect (ShoppingCart) must fail a scenario,
  and a refused `cancel` (SeatBooking) must be reported as a step the spec allows.
- Differential: six in-memory mutants (a guard dropped, an effect skipped), a
  dropped guard under the random mode, and generated-Dafny mutants, each of which
  must report divergences (not just exit nonzero): the stock guard of `checkout`
  lost, an `if` condition changed (AuthLockout), the wrong enum member (Payment),
  `min` turned into `max` (RateLimiter), the ledger `sum` adding refunds (Payment),
  truncating instead of Euclidean division (Batching), `pop` taking the last item
  (MutexQueue), a `let` reading the last queued job (JobPipeline), and `ship` not
  adding to the `Set` (OrderWorkflow).
- Proof: the lost stock guard, a weakened per-user cap, a generated module whose
  `StepPreserves` lacks its proof, `cart_v1_buggy` and `cart_buggy` through
  `core.mjs`, and a module with an unguarded `head()`/`pop()`
  (`fixtures/negative/unguarded_head.fsl`) that Dafny does not verify.
- Translation: `mutex_queue`'s `leadsTo` is refused without `--liveness-by-fslc`,
  and widening `T` in `fixtures/negative/const_bound.fsl` (`type T = 0..M` with
  `requires t < M`) is refused. Both must give their expected message.
- Audit: `assume`, `{:axiom}`, `{:verify false}`, an omitted lemma.
- Structure: a standalone module, an extra module, a second `include` not at the
  start of its line; Dafny itself rejects a redefined predicate and an added
  precondition.
- Invariants: cores whose generated `DoneIsFree` is `true`, uses `and` for `=>`, or
  is reversed, or whose `WipLimit` is `true`, are rejected both by
  `controls/invariant-states.ts` (five hand-classified states) and by invcheck,
  which must report disagreements (too weak for the first two and the last, too
  strong for the reversed one).
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
  4× by random sequences, plus sampled states for the invariants), so the
  all-sizes claim also rests on `gen.mjs` being faithful.
- `leadsTo` (liveness) is not proved by Dafny; with `--liveness-by-fslc` it is
  left to `fslc verify` at the spec's sizes (bounded for `mutex_queue`).
- invcheck samples states; it is not exhaustive except where the space is small
  (SeatBooking: all 27 in-bounds states).
- Kernel coverage left: relations, `transitions` properties, and `compose` specs
  (refused by `fslc kernel` v1). No spec under `specs/` uses the first two.
  `reachable` properties are not translated (they are not safety obligations).
- Cost left: `step` still checks `Wf` and encodes the whole state (linear), and an
  untrusted state is checked in full (|quantified| × |counted|).
- `fslc replay` and `fslc conformance` judge states only; there is no notion of
  action results or outputs, so neither does the core.
- Contract functions and the IO shell (DB transaction per action, outbox, replay
  of production logs) are the follow-ups listed in #1908.
- Not wired into CI (CI has no Dafny); no changelog fragment (no product change).
