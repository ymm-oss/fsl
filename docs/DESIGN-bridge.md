# FSL v2.0 — Implementation Bridge (runtime monitor / replay / testgen) Implementation Design

DESIGN-v1.md §10 v2.0 "the body of the implementation bridge." Three bridges are
built between the spec and the implementation:

1. **`fslc.runtime.Monitor`** — a concrete interpreter of the spec (no Z3, pure
   Python). Embed it in the implementation to use it for runtime conformance
   checking (runtime monitoring).
2. **`fslc replay`** — a CLI that checks an event log of the real system against
   the spec. The first entry point usable without writing code.
3. **`fslc testgen`** — generates a conformance-test (pytest) skeleton for the
   implementation. Replay tests of scenarios (existing) + random-walk property tests.

Design principle: because it is a **second implementation with the same semantics
as the verifier (the Z3 evaluator)**, the two cross-check each other via
**differential testing** by replaying witness traces (§6).

## 1. `fslc.runtime` — the concrete interpreter

A new module `src/fslc/runtime.py`. It does not import Z3 (`parse`/`build_spec`
may be used — if model.py imports z3, we do **not** require going as far as "z3
unnecessary at runtime use" by making the sort-construction part a lazy import,
etc. In v2.0, use within the same package is sufficient).

### 1.1 Value representation (logical values — the same convention as verify's JSON display)

| FSL type | Python representation |
|---|---|
| Int / domain type | int |
| Bool | bool |
| enum | str (member name) |
| Option | None or the contained value |
| struct | dict (field name → value) |
| Map | dict (logical value of the key → value). The key is total (enumerate the bounded keys and initialize) |
| Set | Python set |
| Seq | Python list |

### 1.2 API

```python
from fslc.runtime import Monitor

mon = Monitor(spec_source_or_path)      # parse + build_spec + static checks
mon.reset()                             # -> state dict (initial state)
r = mon.step("checkout", {"u": 0})      # -> result dict (below)
mon.state                               # current logical state (dict, JSON-compatible)
mon.enabled()                           # -> [{"action": str, "params": dict}, ...]
```

The result of `step` (the same vocabulary as verify's JSON):

```json
{ "ok": true, "state": { ... }, "changes": { "stock[0]": {"from": 1, "to": 0} } }
{ "ok": false, "kind": "requires_failed", "action": "checkout",
  "params": {"u": 0}, "requires": {"loc": ..., "text": "..."}, "state": { ... } }
{ "ok": false, "kind": "ensures" | "type_bound" | "partial_op" | "invariant",
  "name": "<invariant name or _bounds_* or _partial_*>", "loc": ...,
  "state": { ... }, "hint": "..." }
```

- On violation, **do not change the state** (requires_failed / partial_op /
  type_bound / ensures / invariant all roll back before the transition). No
  exception is thrown — always return a result dict (easy to embed as a monitor,
  and readable by the LLM).
- The semantics of `step` are identical to BMC: evaluate all requires (no
  short-circuit) → apply the body by simultaneous assignment (all RHS read the
  old state) → partial_op check (considering path conditions) → ensures
  (old = old state) → invariant of the new state + automatic bounds check.
- An unknown action name, a missing parameter, or being out of type range is `kind: "bad_call"`.

### 1.3 Determinism of init

Concrete execution requires a deterministic initial state. Static check at
`Monitor` construction:

- init assigns to every state variable **exactly once** (forall bulk assignment allowed).
- The RHS of init may reference only const and **already-assigned** state
  variables (evaluated top to bottom). A violation is `FslError(kind="semantics")`
  + hint "runtime monitor requires a deterministic init".
- All existing specs satisfy this (a spec that does not satisfy it passes verify
  but errors in Monitor — prompting a fix on the spec side).

### 1.4 Expression evaluation

Implement the concrete evaluator `eval_concrete(expr, state, binds, spec, old_state)`
inside `runtime.py`. It takes the same AST as bmc.py's `eval_expr` as input and
evaluates with the Python values of §1.1. Note the following:

- Quantification/aggregation (forall/exists/count/sum) loop straightforwardly over a bounded enumeration.
- The binding of `is some(x)`, `min/max/abs`, and equality of struct/Seq/Set are over the representation of §1.1.
- Out-of-range `at`/`head`/`pop` on a Seq is **reported as partial_op at the call
  site** (the path condition is naturally protected by if-evaluation — in
  concrete execution only the branch actually taken is evaluated, matching BMC's
  "path-condition implication").
- There is no division by zero etc., but an unexpected evaluation error is wrapped in `kind: "internal"`.

## 2. `fslc replay` — conformance checking of event logs

```
fslc replay <file.fsl> --trace <events.json>
```

Input (the assumed shape of the log the real system emits — the same shape as scenarios' steps):

```json
{ "events": [ { "action": "add_to_cart", "params": {"u": 0, "i": 1} }, ... ] }
```

A JSON whose top level is just an array (`[ {...}, ... ]`) is also accepted.

Output:

```json
{ "fsl": "1.0", "result": "conformant", "spec": "ShoppingCart",
  "steps_checked": 12, "final_state": { ... } }
{ "fsl": "1.0", "result": "nonconformant", "spec": "ShoppingCart",
  "failed_at_event": 4, "violation": { ...the step's ok:false result... },
  "state_before": { ... },
  "hint": "the implementation performed an action the spec forbids at this state (or reached a state violating an invariant)" }
```

exit code: conformant = 0, nonconformant = 1, input/spec error = 2.

## 3. `fslc testgen` — generation of a conformance-test skeleton

```
fslc testgen <file.fsl> [--depth K] [-o <out.py>]    # default: test_<spec name lowercased>.py to stdout
```

The output is a **self-contained pytest file** (the primary dependencies are
`fslc.runtime` and `pytest`. In addition, only the standard library needed for
wiring and replay may be imported — `random` (below, 3.) for the fixed-seed
pseudorandom walk and `pathlib` for resolving the SPEC path):

1. **Adapter stub**: a class for the user to wire up the implementation.
   ```python
   class Adapter:
       """Connect your implementation to the spec actions/state."""
       def reset(self): raise NotImplementedError
       def step(self, action: str, params: dict): ...   # drive the implementation for one action
       def observe(self) -> dict: ...                   # project the implementation's state into the spec's state shape
   ```
2. **Scenario replay test** (run the scenarios mechanism at testgen time and
   embed it): feed each scenario's steps to the Adapter, and assert that after
   each step `observe()` matches **only the fields mentioned** in `expected_states[i]`.
3. **Random-walk conformance test**: with `Monitor` as the oracle, choose actions
   from `mon.enabled()` by pseudorandom (fixed seed, `random.Random(0)`) for
   N=100 steps, and on every step assert `adapter.step(...)` → `observe() == mon.state`.
   If the Monitor side produces a violation (invariant etc.), distinguish it in
   the fail message as **a bug in the spec itself**.
4. While the Adapter is unimplemented (NotImplementedError), make all tests
   `pytest.skip`, so that pytest does not error even right after generation.

## 4. CLI / public API

- cli.py: add the `replay` / `testgen` subcommands. Existing commands unchanged.
- `fslc/__init__.py`: expose `from .runtime import Monitor`.

## 5. Constraints / out of scope

- leadsTo is not checked in the runtime monitor (a response property over a
  finite log cannot distinguish "not yet reached" from "violated." Stated in the
  replay output's `note`).
- The fair annotation has no effect at runtime.
- Monitoring under concurrent execution (thread safety) is out of scope (stated in the documentation).

## 6. Test plan (tests/test_runtime.py)

1. **Differential testing (most important)**: for every sample spec, replay
   `fslc verify`'s reachable witness trace (and scenarios' steps) with `Monitor`,
   and confirm that **each step's state exactly matches the witness's state**.
   (Mechanical verification of the semantic agreement between the Z3 evaluator
   and the concrete evaluator. A generalization of the existing test_scenarios §3.1.)
2. requires_failed: a step whose guard fails is ok: false / state unchanged.
3. Each violation of partial_op / type_bound / ensures / invariant is detected in
   Monitor too with the same kind (replay, one step at a time, the counterexample
   trace of a spec that becomes violated under verify, and the same kind is
   returned at the last step).
4. Non-deterministic init (a missing assignment) is a semantics error at Monitor construction.
5. replay: a conformant log / a nonconformant log (confirm failed_at_event and
   violation.kind) / array-form input.
6. testgen: the generated file is importable, skips if the Adapter is
   unimplemented, and with the Monitor wired in place of the Adapter, the
   "self-conformance" makes all tests pass (= verification that the generated
   skeleton works as-is).
7. enabled(): matches the expected instance enumeration in a known state.

## 7. Reflecting into documentation

- A "§10 bridge to the implementation" section in LANGUAGE.md: how to use Monitor
  / replay / testgen and the workflow (spec proved → testgen → implement Adapter → pytest).
- Add replay / testgen to the README's usage.
- An "implemented" note on the relevant items in DESIGN-v1.md §10.
