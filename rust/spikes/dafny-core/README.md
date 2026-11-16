# Dafny core spike (#1908)

Can an approved FSL spec become a TypeScript-importable core that needs no code
review? This spike takes the path

```
fslc kernel  ->  gen.mjs  ->  <Name>.gen.dfy   (abstract module: state, init, guards, updates,
                                                and the proof obligations without bodies)
proofs/<Name>.proof.dfy  (refines it; supplies only the lemma bodies — the part an AI writes)
structure + dafny audit + dafny verify --verify-included-files
                         ->  holds for every size of the widened types
dafny translate js       ->  out/<Name>.cjs  (the code that was proved is the code that runs)
check.mjs                ->  every reachable state x every action instance, judged by fslc replay
```

Run (needs `dafny` 4.11 and a release `fslc` at `../../target/release/fslc`, or `FSLC=...`):

```bash
npm ci
./run-all.sh        # builds, proves, checks both specs, then runs every negative control
```

## What each piece guarantees

- **gen.mjs** translates Public Kernel v1 and fails closed (exit 2) on anything it
  does not handle. Over `specs/*.fsl`: 8 translate, 10 are refused, 0 crash.
  Domain types named with `--widen` become a size in `Config` (`0 <= x < N_T`);
  other ranges and the implicit type bounds stay as proof obligations in `Wf`.
- **The proof module** cannot change what is proved. `build.sh` requires the file
  to include only the generated module and to declare only
  `module <Name>Proof refines <Name>Spec` (without this check a standalone
  `<Name>Proof` with `Inv = true` passes both `dafny verify` and `dafny audit`).
  Refinement then keeps every body and `requires`/`ensures` of the generated module
  (Dafny rejects a redefined predicate or an added precondition), and `build.sh`
  refuses any `dafny audit` finding (`assume`, `{:axiom}`, `{:verify false}`, a
  missing body). Verification uses `--verify-included-files`: by default Dafny does
  not verify the generated module's own obligations (index ranges, termination).
- **`<action>_Step`** is the entry point for TypeScript: a total function that
  checks well-formedness, parameter ranges and the guard itself and returns `None`
  otherwise, so no call site reaches unproved behaviour. `<action>_StepPreserves`
  proves it keeps the invariants.
- **check.mjs** compares the compiled core with the fslc Monitor at the spec's own
  sizes. Each step goes to `fslc replay --trace` with the path that reached it:
  an enabled JS step must be `conformant`; a disabled one must be `requires_failed`.
  Agreement on every step from the same initial state means equal transition
  relations at those sizes. This guards the translator and the Dafny compiler.
  It calls `<action>_Step`, the same entry point TypeScript uses.

## Results (fslc 5.1.0 built from d1df480b, Dafny 4.11.0, Node 22)

| spec | widened | Dafny | reachable states (JS / `fslc verify --engine explicit`) | steps judged | divergences |
|---|---|---|---|---|---|
| `seat_booking.fsl` | `SeatId`, `UserId` | 27 verified | 25 / 25 | 225 | 0 |
| `cart_v1.fsl` | `UserId`, `ItemId` | 27 verified | 36 / 36 | 288 | 0 |

In Dafny, `PerUserCap` (seat_booking) and the `Qty = 0..3` stock bound (cart_v1)
hold for any number of seats, users and items; `fslc`'s `proved` covers only the
sizes written in the spec. The link between the Dafny model and FSL is checked at
finite sizes only. Besides the spec's own sizes, `run-all.sh` copies each spec with
one widened type enlarged (`SeatId = 0..3`, `UserId = 0..2`). The generated Dafny is
byte-identical below the header, and the same compiled core agrees with the Monitor
there: 63 states / 756 steps and 108 states / 1296 steps, 0 divergences.
`cart_v1_buggy.fsl` translates, and its proof fails as it should.

Negative controls (`run-all.sh`, 27 expectations, two runs, same results each time):
- The differential check rejects six in-memory mutants (a guard dropped, an effect
  skipped) and a translator-shaped mutant (the stock guard of `checkout` lost in the
  generated Dafny).
- Dafny rejects the proof of that mutant, of a weakened per-user cap, and of a
  generated module whose `StepPreserves` lacks its proof.
- The audit rejects a proof that uses `assume`, `{:axiom}`, `{:verify false}`, or
  omits a lemma.
- `build.sh` rejects a standalone or two-module proof file, and Dafny rejects a
  refining proof that redefines a predicate or adds a precondition.

## Not covered by this spike

- No `.d.ts`: TypeScript imports `out/<Name>.cjs` untyped; values are Dafny
  `Seq`/`BigNumber`/datatype objects. A generated typed wrapper is the next step.
  `_Step` re-checks `Wf` on every call (linear in the state size).
- `gen.mjs` "translating" a spec means it emitted Dafny; only seat_booking, cart_v1
  and cart_v1_buggy went through Dafny. `reachable` properties are not translated
  (they are not safety obligations).
- Which types may widen is passed on the command line; specs mark nothing
  (`symmetric` is used by neither spec).
- Kernel constructs outside these two specs (let, partial operations, enums,
  structs, sets, two updates to one variable, transitions/leadsTo properties).
- Contract functions and the IO shell (follow-ups listed in #1908).
