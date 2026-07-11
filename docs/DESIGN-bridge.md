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
  For a Map/index target (`m[K] = ...`), "once" is per concrete key when the
  key is a literal or enum member rather than a `forall`-bound variable: flat
  `m[K1] = ...` / `m[K2] = ...` statements for two *different* keys are not a
  duplicate (this is exactly how a dialect like fsl-db populates a map one
  column at a time); the same key assigned twice, or a key that is itself a
  bound loop variable (where two iterations could alias), still is.
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

The original `--trace` form below accepts already-normalized spec actions. For
production JSONL whose action/state names differ from the spec, use
`--from-log ... --mapping ...`; that extension reuses refinement mapping syntax
and is specified in `DESIGN-log-replay.md`.

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
fslc testgen <file.fsl> [--depth K] [--strict] [--target pytest|vitest|swift|kotlin|dart|phpunit] [-o <out>]   # default target pytest; default file test_<spec name lowercased>.py to stdout
```

The scenario-collection core (`scenarios()`) is language independent, so `testgen.py`
splits into that shared core (`_collect_scenarios`) plus per-target emitters
(`emit_pytest` / `emit_vitest` / `emit_swift`, …) chosen by `--target`. Adding a
harness (Jest, Go, …) is a new emitter, not a redesign — the same kernel-stays-narrow
principle as the dialect frontends.

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
   If some `reachable` targets are not witnessed at the requested depth, testgen
   still embeds the witnessed scenarios and returns warning JSON naming each
   missing target with a depth hint. `--strict` restores all-or-nothing
   `reachable_failed`.
3. **Random-walk conformance test**: with `Monitor` as the oracle, choose actions
   from `mon.enabled()` by pseudorandom (fixed seed, `random.Random(0)`) for
   N=100 steps, and on every step assert `adapter.step(...)` → `observe() == mon.state`.
   If the Monitor side produces a violation (invariant etc.), distinguish it in
   the fail message as **a bug in the spec itself**.
4. While the Adapter is unimplemented (NotImplementedError), make all tests
   `pytest.skip`, so that pytest does not error even right after generation.

### 3.1 `--target vitest` (TypeScript / Vitest)

The Vitest emitter renders the **same scenarios** to a self-contained TypeScript
file with the same `reset`/`step`/`observe` `Adapter` contract. Parts 1, 2, and 4
above port directly: an `Adapter` interface + `makeAdapter()` stub, deterministic
scenario tests (`assertPartial`), forbidden-rejection tests (`assertRejected`), and
skip-when-unwired (a top-level guard flips `test` to `test.skip`).

Part 3 — the random walk — is the one real design point, because TypeScript has no
`Monitor`. The chosen approach **bakes the trace at generation time**: the Python
Monitor runs the fixed-seed (`Random(0)`) walk and the resulting
`(action, params, expected_state)` sequence is embedded as a static fixture; the
Vitest test only replays it and asserts. Rationale:

- The walk is already deterministic (`Random(0)` + deterministic Monitor), so baking
  under the same seed yields the identical trace — equal coverage, nothing lost.
- It keeps the **single independent oracle** invariant: there is still exactly one
  Monitor (in Python), not a second reimplementation in TypeScript.
- The generated file is `fslc`-free at runtime — no shell-out, no Python dependency.

Rejected alternatives: (b) shelling out to `fslc` from the TS test (adds a runtime
coupling), and (c) porting `Monitor` to TS (duplicates the dual-evaluator surface
that the agreement/oracle tests exist to protect). Output defaults to
`<spec>.test.ts`.

### 3.2 `--target swift` (Swift Testing)

The Swift emitter renders the same scenarios to a self-contained Swift Testing file
(`import Testing` / `@Test` / `#expect` / `#require` — **not XCTest**), reusing the
language-independent baked walk. The Swift-specific points:

- **Dynamic dict + equality.** `params`/`observe()` are `[String: Any]`. `Any`
  values have no usable `==`, so the harness bundles `fslEqual` (a deep equality over
  the JSON-normal world: `Bool`/`Int`/`Double`/`String`/`FSLNull`/`[Any]`/`[String:
  Any]`, with `Int` and `Double` kept distinct) and an `assertPartial` that recurses
  by the expected keys and asserts only the fields the spec mentions.
- **Null.** An Option `None` bakes as `FSLNull.instance`, a one-line sentinel struct,
  so the generated file depends only on `Testing` (no Foundation/`NSNull`).
- **Skip-when-unwired.** `makeAdapter()` throws until wired; each `@Test` carries
  `.enabled(if: isAdapterWired())`, so the suite is *disabled* (not failed) until an
  adapter is connected — the Swift analog of the pytest skip / Vitest `test.skip`.
- **Literals.** `_swift_literal` renders int as `Int`, float as `Double` (always with
  a decimal point), bool/null per above, and strings with Swift escape rules
  (`\u{XX}`, which differ from JSON). The baked walk is an inline labelled-tuple
  array `[(action:params:expected:)]` (kept local to the test, so no global
  non-`Sendable` state). Output defaults to `<SpecName>ConformanceTests.swift`.

Rejected alternatives mirror Vitest's: no shell-out to `fslc`, no second `Monitor`
port. An `Encodable` generated-struct model was considered over dict-plus-helper but
deferred — the dict keeps the first version small and matches the language-independent
scenario JSON directly.

### 3.3 `--target kotlin` (kotlin.test)

The Kotlin emitter renders the same scenarios to a self-contained kotlin.test file,
again reusing the baked walk. The choices:

- **Framework: kotlin.test** (over JUnit5 / Kotest). It is multiplatform and on the
  JVM delegates to JUnit, so the generated file carries the lightest dependency. The
  imports are fixed to `kotlin.test.{Test, assertEquals, assertFalse, assertNotNull,
  assertTrue}`.
- **Dynamic dict is the easy case.** `params`/`observe()` are `Map<String, Any?>`,
  and Kotlin's structural `==` is already deep on `List`/`Map` and discriminates a
  boxed `Int` from a `Double`, so `assertPartial` is a plain recursion that asserts
  only the expected keys and leans on `assertEquals` for leaves.
- **Skip-when-unwired.** kotlin.test has no portable runtime skip (no
  `assumeTrue`), so `makeAdapter(): Adapter?` returns `null` until wired and each
  `@Test` starts `val a = makeAdapter() ?: return` — an unwired suite no-ops rather
  than fails. This is the one deliberate divergence from the pytest/Vitest/Swift
  "reported as skipped" behaviour, forced by the framework.
- **Literals.** `_kotlin_literal` renders int as `Int`, float as `Double`, `null`,
  `listOf`/`mapOf` (empty ones carry explicit type args), and strings with Kotlin
  escapes — notably `$` must be escaped (string templates). The baked walk is a
  `List<Triple<String, Map<String, Any?>, Map<String, Any?>>>`. Output defaults to
  `<SpecName>ConformanceTest.kt`.

(No `swiftc -parse`-style syntax gate in tests: kotlinc has no dependency-free
parse-only mode — a real compile needs kotlin-test on the classpath — so the Kotlin
tests assert on harness shape and the baked walk instead.)

### 3.4 `--target dart` (package:test)

The Dart emitter renders the same scenarios to a self-contained `package:test` file
(which also runs under `flutter test`), reusing the baked walk. The choices:

- **Equality is the catch.** `params`/`observe()` are `Map<String, dynamic>`, and
  Dart's `==` is *reference* equality on `List`/`Map`, not structural. Rather than
  pull in `package:collection`'s `DeepCollectionEquality`, `assertPartial` recurses
  by the expected keys and compares leaves/sequences with the `equals` matcher, which
  *is* deep and is re-exported by `package:test` — so the generated file's only
  dependency stays `package:test`.
- **int/float.** Unlike PHP, Dart treats `1 == 1.0` as true, so `_dart_literal`
  renders the right syntax (`1` vs `1.0`) but the values compare equal — this is Dart
  semantics, not a fidelity gap.
- **Skip-when-unwired.** `package:test`'s `skip:` argument is static, so a top-level
  `_adapterWired()` probe runs once in `main()` and every `test(..., skip: wired ?
  null : 'Adapter not wired')` is conditionally skipped until an adapter is connected.
- **Literals.** Strings are single-quoted with `$` escaped (interpolation); empty
  collections carry explicit type args (`<String, dynamic>{}`); the baked walk is a
  `List<Map<String, dynamic>>` of `{action, params, expected}`. Output defaults to
  `<spec_name>_conformance_test.dart` (snake_case + the runner's `_test.dart` suffix).

(No syntax gate in tests: `dart analyze` needs a pub package context, so there is no
clean dependency-free parse-only mode — the Dart tests assert on shape + baked walk.)

### 3.5 `--target phpunit` (PHPUnit)

The PHPUnit emitter targets PHP 8.1+ / PHPUnit 10+ (`declare(strict_types=1)`),
reusing the baked walk. The choices:

- **Strict leaf equality is the whole point.** PHP's loose `==` makes `0 == "0"`
  and `1 == true` true, which is unsafe for a conformance test, so every leaf is
  compared with `assertSame` (`===`), and `_php_literal` keeps `int` (`1`) distinct
  from `float` (`1.0`) — `assertSame(1, 1.0)` is false.
- **Dict vs the numeric-key trap.** `params`/`observe()` are associative `array`s,
  but PHP coerces a numeric string key like `'0'` to int `0`, which collapses the
  map/list distinction. `assertPartial` therefore recurses by the *expected* keys
  (so a `Map<0..1, …>` matches by key — order-independent — and only the mentioned
  fields are asserted) and, for a genuinely list-shaped expected (`array_is_list`),
  also pins the length so sequences stay exact. Both sides see the same key coercion,
  so it cancels out.
- **Skip-when-unwired.** `makeAdapter()` throws until wired; `setUp()` probes it and
  calls `markTestSkipped`, so the whole class is skipped (not failed) until an
  adapter is connected — the PHPUnit analog of the pytest skip.
- **Literals.** Strings are single-quoted (PHP single-quotes interpolate nothing;
  only `\\`/`\'` are escaped); JSON arrays render to PHP lists, JSON objects to
  associative arrays; the baked walk is a `private const WALK`. Output defaults to
  `<SpecName>ConformanceTest.php` (PSR-4: class name = file name), test methods are
  `testScenario_<name>` / `testRandomWalkConformance`.

(Syntax gate in tests: `php -l` lints syntax without loading PHPUnit — a clean
dependency-free check like swiftc -parse, so the PHP tests run it when php is present
and skip it otherwise.)

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
