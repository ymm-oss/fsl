# FSL v1.1 — implementation design for unsat-core hints and `fslc scenarios`

Of the v1.1 items in DESIGN-v1.md §10, this specifies two features: coverage diagnostics
and the implementation bridge.

## 1. Coverage diagnostics via unsat core

### 1.1 Problem

For an action whose `action_coverage` is `false`, the current behavior returns only the
fact that "it is never enabled within depth K". The LLM's next move is decided only once
it knows "which requires is blocking it" (the principle of repair protocol §8).

### 1.2 Specification

For an action A whose coverage is false, perform additional diagnostics:

1. Against the state σ_K at the final depth K (the final state of the unrolling used in
   the coverage check), for each instance of A (per parameter binding), turn the requires
   conjunction into assumption literals `p_i ⇔ requires_i` and run
   `solver.check(p_1, ..., p_n)`.
2. If unsat, obtain from `solver.unsat_core()` a **near-minimal set** of requires clauses
   that block enabledness (Z3's core need not be guaranteed minimal. v1.1 prioritizes
   responsiveness over precision and uses the core as-is).
3. When there are multiple instances (parameterized action), report the **intersection of
   the cores of all instances** as the representative. If the intersection is empty,
   report the core of the first instance and attach `bindings`.
4. Because there are clause combinations that "become sat at some t" across depth, not
   just at K but at each step t = 0..K, the check rides on the same "step-by-step, per-t"
   loop as the coverage body itself. Only for actions that were false to the end is the
   **core at t = K** reported (returning the core for all t is redundant; the K point is
   highly representative as "the clause still blocking even at the most-developed state").

### 1.3 JSON

Extend `action_coverage` backward-compatibly from the conventional `{name: bool}`: a true
action stays `true` as before, and only a false action becomes an object.

```json
"action_coverage": {
  "add_to_cart": true,
  "checkout": {
    "covered": false,
    "name": "checkout",
    "blocking_requires": [
      { "loc": {"line": 27, "column": 3}, "text": "stock[i] > 0" }
    ],
    "bindings": {"u": 0, "i": 1},      // only when the core is binding-dependent
    "hint": "never enabled within depth K; blocking requires: stock[i] > 0; weaken a guard, add a setup action, or increase --depth",
    "faithfulness_class": "intent_unexercised"
  }
}
```

- `text` is reverse-formatted (pretty-printed) from the AST. If the v1 implementation has
  no pretty-printer, only `loc` is required, and `text` is best-effort (clipping the
  source line is acceptable).
- The existing test's `coverage[name] is True` assertion still passes (the true side stays
  bool).
- The hint of the coverage warning (inside warnings) references this structure.

### 1.4 Implementation notes

- In the `_action_coverage` loop, for each uncovered action conjoin its requires-clause
  list via the implication `z3.Bool(f"__cov_{action}_{i}_{j}")`, and use `check(assumptions)`.
  Unlike ordinary `s.add`, this needs no push/pop and lets the core be obtained.
- The core is minimized with a cheap deletion pass before rendering `blocking_requires`
  and the human `hint`; the raw entries remain per requires clause.
- The requires AST and loc are already in the instance dict (`inst["requires"]`). Per
  clause (per requires statement) is sufficient; do not decompose the conjunction more
  finely.

## 2. `fslc scenarios` — generating integration-test templates

### 2.1 Purpose

Emit "integration-test templates against the implementation" from the spec in
machine-readable JSON. This is the first step of the spec→impl bridge (DESIGN-v1.md §10).
The LLM reads this JSON and converts it into test code in the implementation language.

### 2.2 CLI

```
fslc scenarios <file.fsl> [--depth K]
```

- Using the same BMC mechanism as verify, collect the following and output a single JSON
  to stdout:
  1. **a witness trace per reachable** (= the happy-path scenario)
  2. **the shortest enabled trace per action** (a byproduct of the coverage check; a trace
     that ends by executing that action once at the end)
  3. **the deadlock trace** (when found. "a state where nothing more can be done" becomes a
     terminal-state test in the implementation)
- If the spec is violated, return the same violated JSON as verify and exit 1 (do not make
  scenarios from a broken spec).
- If any `reachable` is not witnessed, return `reachable_failed` like verify. Each
  `unreached[]` entry carries a `classification`: `insufficient_depth` when the
  target is satisfiable as a state predicate but was not witnessed by depth K, or
  `over_constrained` when the target is unsatisfiable under type bounds/invariants
  (with `blocking_requires` naming the blocking unsat core).
- `fslc testgen` consumes the same scenario machinery in partial mode: witnessed
  `reachable` targets still become pytest scenarios, while unwitnessed targets are
  returned as warnings such as
  `reachable SoldOut not witnessed at depth 3; try --depth >= 4`. `testgen
  --strict` disables partial mode and preserves the `reachable_failed` abort.

### 2.3 Output JSON

```json
{
  "fsl": "1.0",
  "result": "scenarios",
  "spec": "ShoppingCart",
  "depth": 8,
  "scenarios": [
    {
      "name": "reach_SoldOut",
      "kind": "reachable",
      "property": "SoldOut",
      "steps": [
        { "action": "add_to_cart", "params": {"u": 0, "i": 0} },
        { "action": "checkout",    "params": {"u": 0} }
      ],
      "initial_state": { ... },
      "expected_states": [ {...}, {...} ],   // logical state after each step (same form as witness)
      "final_check": "SoldOut"
    },
    {
      "name": "cover_remove_from_cart",
      "kind": "action_coverage",
      "action": "remove_from_cart",
      "steps": [ ... ],
      "initial_state": { ... },
      "expected_states": [ ... ]
    },
    {
      "name": "deadlock_terminal",
      "kind": "deadlock",
      "steps": [ ... ],
      "note": "after these steps no action is enabled"
    }
  ]
}
```

- `steps[].params` is in display form (enum names, etc.), the same form as verify's
  `last_action.params`.
- `expected_states` is the same form as the witness's `state` (no internal names).
- Implication for the LLM: explain in a single sentence in the JSON's `"convention"`
  field the conversion convention "set up `initial_state` → convert each step into an
  implementation API call → after each step assert **only the mentioned fields** of
  `expected_states[i]`".

### 2.4 Implementation notes

- The extraction of witness / coverage traces is something verify already holds internally
  (the witness of reachables, the step-by-step search of coverage). The only new part of
  scenarios is constructing a concrete trace from "the model at the moment it became
  enabled" in the coverage search (currently only the sat/unsat bool is taken).
- Add a `run_scenarios(path, depth)` to cli.py analogous to `run_verify`, with exit codes
  0 (generation success) / 1 (violated) / 2,3 (error).

## 3. Test plan

1. scenarios for cart_v1: `reach_SoldOut` exists, and the state transitions when the steps
   are "executed by hand" in order match `expected_states` (write a test that simulates
   and verifies in Python — it doubles as a consistency check that the result matches the
   bmc transitions).
2. coverage scenarios: `cover_<name>` is generated for every action.
3. With a spec that has a blocked action (a requires that is always false):
   - verify: the loc of that requires in `action_coverage.<name>.blocking_requires`
   - scenarios: no scenario is generated for that action, with an explanation in `warnings`
4. Existing-schema compatibility: an action with coverage true stays a bool (existing tests
   pass unmodified).
5. deadlock scenario: a `deadlock_terminal` scenario is emitted for the DeadEnd spec of
   test_warnings_format_and_deadlock_trace.
