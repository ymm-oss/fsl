# Dogfooding Round 1 — Findings (2026-06-11)

We field-tested v1.0 against four real-domain specs (`specs/auth_lockout.fsl`, `specs/inventory_reservation.fsl`,
`specs/payment.fsl`, `specs/rate_limiter.fsl`) plus seven edge probes.

## Results Summary

| Spec | Result |
|---|---|
| auth_lockout (depth 8) | verified. witness: LockedOut@3, RecoveredAfterLock@5. coverage all true |
| inventory_reservation (depth 5) | verified (48s). AllHeld@3. **depth 8 aborted after an estimated 30+ minutes** (PERF1) |
| payment (depth 6) | verified (4.3s). FullyRefunded@3. coverage all true |
| rate_limiter (depth 6) | verified (0.2s). Exhausted@4. coverage all true |

## New Bugs

### BUG11: check passes a composite struct field type, then verify hits an internal error

- `struct S { v: Option<K> }` → at the time, check ok, but verify hit `kind: "internal", message: "'s__v'"` (raw KeyError).
  In v2.1 this is now formally legalized as an `Option<scalar>` field.
- `struct Outer { i: Inner }` (struct nesting, explicitly marked "not allowed in v1" in design §3.4) → likewise `"'o__i'"`
- `struct S { members: Set<K> }` → verify raises a misleading semantics error
- **Expected behavior**: `check_spec` validates struct field types and rejects anything other than domain / enum / Bool / Int /
  `Option<scalar>` (Set / Map / Seq / struct / nested Option) with `kind: "type"` + hint
  (e.g. "struct fields must be scalar or Option<scalar>; use a separate Map for Set/Map/Seq/struct fields").
  This follows the "every failure has a next move" principle of the repair protocol (§8).

### BUG12: exclusive branches of a nested if/else mis-detected as a "double assignment"

```fsl
action step() {
  if x == 0 { x = 1 }
  else { if x == 1 { x = 2 } else { x = 0 } }
}
```

- → `semantics: double assignment to 'x' on the same execution path` (wrong; the three assignments are all on exclusive paths)
- Cause: `run_into_if` in `bmc.py` (used for nested ifs) does not save/restore `scalar_writes` between the then/else evaluations.
  The outer if's `run_branch` (L572-576) resets it correctly.
- **Expected behavior**: `run_into_if` should also save and restore `scalar_writes` per branch,
  permitting the same variable to be assigned across exclusive paths (true same-path double assignment is still detected).

### BUG13: when an invariant containing `is some(x)` is violated, JSON serialization crashes

```fsl
invariant Match { c is some(j) => j == target }   // when violated…
```

- → raw traceback `TypeError: Object of type ArithRef is not JSON serializable`.
  The violation is detected, but output dies (the worst failure mode for an LLM repair loop).
- Cause: the is-pattern in `eval_expr` leaves a Z3 expression (ArithRef) in `binds`, and
  `violating_bindings` (bmc.py:935-937) passes the raw Z3 AST into the result dict via `_public_bindings(dict(binds))`.
- **Expected behavior**: bindings values should be concretized via `model.eval(...)`, with enums reverse-mapped to display names on output.
  Alternatively, exclude pattern-bound variables from bindings. The violation JSON must always be serializable.

### BUG14: an assignment after an if silently overwrites a write inside the branch (asymmetric detection)

```fsl
action go() {
  if flag { x = 1 }
  x = 2          // ← no error, and x = 1 silently disappears
}
```

- Placing `x = 2` **before** the if correctly raises a double assignment error, but placing it **after** lets it
  pass through, and x ends up 2 even when flag is true (a silent divergence from the author's intent = a soundness problem).
- Cause: the if handling in `compute_updates` restores `scalar_writes` to its pre-if state after evaluating the branches,
  so writes recorded inside a branch are invisible to subsequent statements. Present in both top-level
  (`run`'s if) and nested (`run_into_if`) handling.
- **Expected behavior**: after handling an if, record the **union** of scalar keys written in the then/else branches into
  `scalar_writes`, so a subsequent assignment to the same variable is an error.

### PERF1: BMC is exponential in depth (about 4x per step)

- `inventory_reservation.fsl`: state = Map×2 (including struct values), 3 actions (~36 instances/step),
  and one invariant containing sum(). Measured: depth 2 = 0.46s, depth 4 = 7.8s, depth 5 = 48s
  (about 4x per step). depth 8 aborted after an estimated 30+ minutes.
- Structural factors (bmc.py):
  1. each `reachable` **redoes the full unrolling in a fresh Solver** (verify body + R times × full unrolling)
  2. the ensures check re-evaluates `_eval_requires` and does push/pop for every instance × ensures
  3. every struct assignment generates an ite tree, and expressions can compound and blow up along depth
- Mitigation approaches considered for v1.1 (incremental solver sharing, expression caching, intermediate-variable assignments, etc.).

## Expressiveness Findings (feedback for language design)

- **F1: reachability that talks about the "past" requires a ghost variable.** auth_lockout's
  "can recover after being locked out" was expressed with an `ever_locked` ghost variable.
  As a workaround it is straightforward, but recorded as a concrete example motivating v2.0's `leadsTo`.
- **F2: the binding scope of `is some(j)` reaches the right-hand side of `=>`** (confirmed in probe2; verified is also correct).
  However, design doc §3.3 has no scoping rule — it should be made explicit that "within a logical expression containing `is`,
  the binding is in effect only in contexts where the `is` evaluation is true".
- **F3: a real example surfaced where not being able to write an Option field in a struct is inconvenient.** (resolved in v2.1)
  inventory_reservation really wanted to be written as `Res { item: Option<ItemId> }`, but it was worked around with an
  enum state (item is a meaningless 0 when Free). In v2.1, `Option<scalar>` fields are handled directly via synthetic lowering.
- **F4: orthogonal feature combinations are largely sound.** lvalue subscripts via let, bulk struct assignment,
  sum with an arithmetic-expression body + composite where, count/min/max/abs, Set<Enum>, ghost variables,
  and clamping via const + min — all behave as expected (confirmed in probe2/4/5 and auth_lockout).

## Probe List (candidates for regression tests)

| Probe | Content | Result |
|---|---|---|
| probe1 | Option as a struct field | OK in v2.1 (`Option<scalar>` only) |
| probe2 | does the `is some(j)` binding reach the RHS of `=>` | OK (verified) |
| probe2n | negative case of probe2 (should be violated) | BUG13 (JSON crash) |
| probe3 | `else { if … else … }` nesting | BUG12 (false double assignment) |
| probe4 / 4n | positive/negative case of Set<Enum> | OK / OK (violated@1) |
| probe5 | count/max/abs + count under reachable | OK (witness@2) |
| probe6 | struct nesting (not allowed by design) | same class as BUG11 (internal error) |
| probe7 | Set as a struct field | same class as BUG11 (wrong message) |
| probe8 / 8r | same-variable assignment after/before an if | BUG14 (after: pass-through / before: detected) |

## Status

- BUG11 / BUG12 / BUG13: **fixed** (codex round, 6 regression tests added).
  Of BUG11, `Option<scalar>` fields are legalized in v2.1; the other composite fields are a type error.
- BUG14: **fixed** (propagates the union of branch writes to subsequent statements; regression test added)
- PERF1: **resolved**. Sharing the unrolling (invariant/reachable/deadlock/coverage all ride on a single unrolling),
  Implies-form transitions, expression caching, and strengthening with proven invariants give:
  - inventory depth 5: 48s → 2.2s, depth 8: 30+ min → **5.4s**
  - whole test suite: 57s → **3.5s**
  - profiling conclusion: the bottleneck is Z3 solver time, not the Python side
    (full re-unrolling per reachable was dominant)
- 33 tests green in total. Results unchanged for all sample specs.
