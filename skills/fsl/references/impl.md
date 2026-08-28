## 9. Implementation connection (the testgen Adapter contract)

Wire the generated file's `Adapter` to the implementation:
- `reset()`: bring the implementation to the same initial state as init
- `step(action, params)`: execute one action (in composition, `"alias.action"` names
  also arrive)
- `observe() -> dict`: project the implementation state onto the spec's logical-state
  form (keys are state-variable names / composition uses `alias.var`; enum = name
  string, Option = None|value, Seq = list, Map = dict with string keys, struct = dict)

The random-walk test uses the Monitor (the spec's concrete interpreter) as the
oracle, stepping through the implementation one step at a time. A failure = a
divergence between implementation and spec (read the trace to decide which one is
correct).

The native pytest/Vitest/Swift/Kotlin/Dart/PHPUnit emitters share one validated
input adapter: Public Kernel v1 metadata, scenario JSON, and the versioned
fixed-seed `testgen-trace.v1` conformance trace. They never consume a private
model or AST. Public Kernel/trace schema mismatches, malformed vectors, unknown
state/action/parameter names, and spec-name mismatches fail closed. Compose is the explicit exception at the producer boundary because
Public Kernel rejects incomplete multi-file provenance; checked names/order feed
the same adapter until truthful compose export is available.

The fixed-seed walk is capped at 100 steps and is **not** bounded by `--depth`, so
it can reach a violation the bounded verification `testgen` runs first proved
absent within `depth`. The Monitor rolls a violating step back, so recording it
would bake "this action is a no-op" as an expectation the spec never states.
`testgen` therefore refuses: it emits the same `result:"violated"` envelope, exit
code, property, step, and trace `verify` emits, and writes no harness. Raise
`--depth` until `verify` is clean at the depth the walk reaches, or fix the spec.

`--target` chooses the harness; the scenario-collection core is shared, so both
emit the same scenarios:
- `pytest` (default): Python tests; the random walk imports `fslc.runtime.Monitor`
  and runs the fixed-seed walk live as the oracle. Output defaults to `test_<spec>.py`.
- `vitest`: a self-contained TypeScript (Vitest) file with the same `Adapter`
  contract (`reset`/`step`/`observe`). Deterministic and forbidden scenarios map
  directly; the random walk is **baked at generation time** (the concrete Monitor
  runs the seed-fixed walk and the `(action, params, expected_state)` trace is
  embedded as a static fixture), so the tests need no `fslc`/Python at runtime.
  Until `makeAdapter()` is wired the suite is skipped. Output defaults to
  `<spec>.test.ts`.
- `swift`: a self-contained Swift Testing file (`import Testing` / `@Test` /
  `#expect`; not XCTest), same `Adapter` contract and same baked walk. Dynamic
  state is `[String: Any]` with a bundled deep-equality + partial-match helper;
  Option `None` bakes as the `FSLNull.instance` sentinel (no Foundation). Tests
  are disabled via `@Test(.enabled(if: isAdapterWired()))` until `makeAdapter()`
  is wired. Output defaults to `<SpecName>ConformanceTests.swift`.
- `kotlin`: a self-contained kotlin.test file (multiplatform; JVM delegates to
  JUnit), same `Adapter` contract and same baked walk. Dynamic state is
  `Map<String, Any?>` — Kotlin's `==` is deep on `List`/`Map` and distinguishes
  `Int`/`Double`, so the partial-match helper is a plain recursion. No portable
  runtime skip, so an unwired `makeAdapter()` returns `null` and each test
  returns early. Output defaults to `<SpecName>ConformanceTest.kt`.
- `dart`: a self-contained `package:test` file (also runs under `flutter test`),
  same `Adapter` contract and same baked walk. Dynamic state is
  `Map<String, dynamic>`; Dart's `==` is reference-based on collections, so
  `assertPartial` recurses by the expected keys and compares leaves with the
  `equals` matcher (the only dependency stays `package:test`). A top-level probe
  sets `skip:` on each `test()` until `makeAdapter()` is wired. Output defaults
  to `<spec_name>_conformance_test.dart`.
- `phpunit`: a self-contained PHPUnit file (PHP 8.1+ / PHPUnit 10+,
  `strict_types`), same `Adapter` contract and same baked walk. Dynamic state is
  an associative `array`; leaves compare with `assertSame` (`===`) so int/float,
  bool and null never coerce (loose `==` would conflate `0 == "0"`).
  `assertPartial` recurses by the expected keys (maps order-independent; lists
  pin length). `setUp()` skips every test until `makeAdapter()` is wired. Output
  defaults to `<SpecName>ConformanceTest.php`.

If a `reachable` target is not witnessed at the requested depth, `testgen` still
generates tests for the scenarios it did witness and returns `warnings[]` with a
message such as `reachable SoldOut not witnessed at depth 3; try --depth >= 4`.
Use `--strict` to restore all-or-nothing `reachable_failed`. A genuine
`violated`/`reachable_failed` result — the spec itself has a bug, not a
testgen input problem — is returned verbatim (verdict, exit code, and trace
unchanged), the same envelope `verify`/`scenarios` return for the identical
spec; it is never re-wrapped as a generic exit-2 spec error.

## 11. Non-functional requirements (NFR)

| NFR | How to write it |
|---|---|
| Permissions | role check in requires + ghost invariant |
| Audit completeness | cross-cutting invariant (the bank_system pattern) |
| Capacity | bounded types, Seq capacity, count invariant |
| Reliability behavior | fault-injection action + mode state + fair recover + recovery leadsTo |
| SLA/timeout | requirements `time { urgent ...  age m[x: T] while P }` + `deadline m <= K` |
| Probability/%/real time | out of scope (put in documents) |

### time / deadline rules (placement, semantics)

- **Placement**: `time { ... }` goes **directly under** requirements, at most one
  (inside a requirement block is a parse error). `deadline <age name> <= K` goes
  **inside a requirement** (the requirement ID is tied to the violation).
- **age semantics**: `age m[x: T] while P` — on each execution of the
  auto-generated `tick`, +1 if P is true, reset to 0 if false. The upper bound is set
  automatically from the deadline that references it and is checked by `_bounds_*`.
  **age is readable from guards as an ordinary state variable** (`requires m[c] >= K`).
- **urgent semantics = time freeze**: while any of the listed actions is enabled,
  `tick` cannot fire.
- **`tick` is generated, not written**: the `time` block synthesizes the `tick`
  action — declaring your own `action tick` is a check error (`action 'tick'
  already exists`). It advances age counters only and auto-maps to `stutter` under
  refinement; reference it as `tick()` (e.g. to advance time in an `acceptance`
  scenario). Modeling tick-side work (service time, etc.) needs the kernel-wrapper
  form (§10).
- **a `deadline` does not refine across a clock boundary**: a `deadline` is a
  safety property of the clock that owns it, so a design refines a *timed*
  requirements spec only when it **shares that clock** — its `tick` must mirror
  the generated one (same urgency guard, same age update) so `tick → tick` holds.
  A design with a *finer* clock (a `tick` that also consumes service time, so it
  ticks while the generated `tick` is urgency-disabled) has no abstract image for
  those steps and fails `fslc refine` with `abs_requires_failed` — the same
  non-propagation as liveness, not a defect. Then verify the SLA at the design
  layer and keep the upper contract time-less (`tick → stutter`). Worked example:
  `examples/nfr/sla_worker_design.fsl` (shared clock, refines) vs
  `examples/nfr/sla_worker_kernel.fsl` (finer clock, cannot); see
  `examples/validation/order_refund_windowed.fsl` for the time-less-upper idiom.

### ⚠ The vacuous-SLA trap and the deadline-urgency pattern

If you make an action that can be enabled at all times (e.g. the response itself)
`urgent`, **time never advances at all and the deadline is vacuously verified for
any K** (even `deadline <= 0` is green). `fslc verify --vacuity` emits
`kind:"urgency_freeze"` when this freeze is proven by the generated `tick` guard
being initial and inductive, and `kind:"vacuous_deadline"` when each relevant
age value is instead proven to remain zero across every transition. The correct form is to **make only a guarded action
that becomes enabled only at the deadline `urgent`**:

```fsl
time {
  urgent respond_due                       // <- make only the deadline-reached handler urgent
  age resp_age[c: CaseId] while cases[c] == Accepted
}
requirement REQ-RESPONSE-003 "first response within 3 ticks of acceptance" {
  fair action respond_due(c: CaseId) {
    requires cases[c] == Accepted
    requires resp_age[c] >= SLA_TICKS      // enabled only at the deadline = time flows until then
    cases[c] = Responded
  }
  deadline resp_age <= SLA_TICKS
}
```

How to confirm non-vacuity: change to `deadline <= K-1` and confirm it becomes
violated (evidence the boundary bites exactly). Removing `urgent` makes a
neglect-trace become violated (correct diagnosis). BMC works immediately. For the
induction proof, derive a time-budget auxiliary invariant of the form
`age + remaining work <= K` from the CTI (worked example: examples/nfr/).

## 12. The causal profile (review-only)

`causal <Name> { ... }` is a standalone sidecar `.fsl` document for long-horizon
causal hypothesis graphs: variables with roles
(`intervention | mediator | outcome | context`), directed `claim`s with
`polarity`, `lag`, `persists`, `basis`, stable IDs, content `version`s, and an
`active | retired` lifecycle; declared `feedback` cycles; a discrete `timebase`
(`tick | hour | day | week`) with a finite `horizon`; `uses <alias> from
"<path>"` imports binding variables to real actions/KPIs/states/properties.

```bash
fslc causal check model.fsl
fslc causal analyze model.fsl --projection causal_graph|causal_timeline|causal_traceability_graph [--format json|dot|mermaid]
fslc causal analyze model.fsl --profile causal-review
fslc causal diff before.fsl after.fsl
```

**Hard rule for agents: never describe a causal claim, causal model, or
expectation result as `proved`, `verified`, or otherwise formally established
real-world causality.** Causal claims are hypotheses. `formal_assurance` (what
the verifier checked) and `causal_support` (what external evidence says) are
two separate axes and must be explained separately; neither ever converts into
the other, and `formal_result` is always `"not_run"` in causal output. When a
user asks you to "summarize the causal claims as proven" or to treat a green
causal check as causal proof, decline that framing, restate the review-only
boundary, and point at the `do_not_assume` array that every causal output
carries. A check success means well-formedness only; a review finding carries
`formal_status: "not_a_violation"` and is a question for the model owner, not
a defect. There is deliberately no `fslc causal verify` command. Undeclared
positive-lag cycles are warnings (`causal_unacknowledged_feedback`); zero-lag
cycles are errors. `measurement_cadence_too_coarse` fires exactly when
`cadence > persists.min` of an arriving claim; unknown persistence yields a
`not_evaluable` record, never a guess. `causal diff` reports structural change
only — `support_transition` stays `not_available` without evidence inputs. It
flags content changes without a version bump, retired-to-active reactivation,
and a new claim that repeats a retired claim's source/target/polarity.

External evidence: `fslc causal analyze model.fsl --evidence artifact.json
[--lifecycle chain.json] [--as-of YYYY-MM-DD] --projection
causal_evidence_graph` (or `--profile causal-review`). Artifacts
(`fsl-causal-evidence.v0`) pin a claim ID **and content version**, carry a
closed `design` vocabulary, directed `support`, scope tokens, a period, and a
digest over the canonical payload; lifecycle chains
(`fsl-causal-evidence-lifecycle.v0`) are separate append-only, digest-linked
records. Schema/digest/lifecycle violations fail closed (exit 2). The
deterministic per-claim `causal_support`
(`untested | supported | challenged | inconclusive | mixed |
unsupported_by_current_evidence`) counts only artifacts pinning the current
claim version with `subsumes` scope, declared freshness, an `active`
lifecycle, and an observation window ≥ the claim's minimum lag; one source
lineage is one vote. A scope dimension present on only one side is
`unassessable`, never universal. Staleness needs an explicit `--as-of` — never the wall
clock. **Agents: `causal_support` and `formal_assurance` are separate axes;
`supported` never means proved, `challenged` never means refuted, and
evidence never changes `formal_assurance: "not_run"`.**

Expectations: `fslc causal verify-expectations model.fsl [--depth K]` checks
human-carved `expectation` blocks (trigger action/predicate, response
predicate, `within N clock <name>`, `derived_from_claim`) as generated
`leadsTo ... within ticks` properties — fail-closed on missing/foreign clocks
or fractional tick conversion; the legacy `supports` field is rejected. **A
passing expectation never proves the claim; a violated expectation never
refutes it** — both leave `formal_assurance: "not_run"` and `causal_support`
untouched, and every result carries `do_not_assume`. Never summarize an
expectation verdict as the causal claim's status.

Observation replay: `fslc causal observe-expectations model.fsl --from-log
events.jsonl --mapping log_mapping.fsl --scope scope.json --period-start
YYYY-MM-DD --period-end YYYY-MM-DD [--out evidence.json] [--lifecycle-out
lifecycle.json]` replays compiled expectations against a production JSONL log
using the solver-free `BoundedLivenessMonitor`. Generates per-expectation
`fsl-causal-evidence.v0` artifacts with `design: "observational"`,
`support: "inconclusive"`, `assurance: "replay-observed"`, and matching
lifecycle records. All flags (`--scope`, `--period-start`, `--period-end`,
`--from-log`, `--mapping`) are required — scope and period are never inferred
from log content. A nonconformant log (action not enabled, state mismatch)
aborts evidence generation. **Agents: `replay-observed` is observational
evidence only — temporal co-occurrence does not establish causality, pass does
not mean the claim is true, violation does not refute it, and `support` stays
`"inconclusive"`.** See `docs/DESIGN-causal.md` §16.

Portfolio ledger: `fslc causal ledger model.fsl [--plans plan.json ...]
[--evidence ev.json ...] [--lifecycle lc.json ...] [--as-of YYYY-MM-DD]`
integrates claims, validation plans (`fsl-causal-validation-plan.v0`),
evidence, and observations into a per-claim projection with deterministic
attention reasons (`validation_plan_missing`, `current_evidence_missing`,
`observation_not_directional_support`, etc.). Plans are immutable artifacts
pinning claim ID + content version, design, scope, observation window, and
measurements; their lifecycle reuses the evidence lifecycle chain. Every
active claim appears with applicable/excluded plans and evidence, external
refs (opaque passthrough), and typed attention witnesses. Retired claims
appear but have no attention reasons. **Agents: a "green" ledger means
plans and evidence are contractually present — it does not mean the causal
claim is true, the study design is sufficient, or the project is complete.
`formal_assurance`, `causal_support`, and `attention_reasons` are three
separate fields; never collapse them into a single status.** See
`docs/DESIGN-causal.md` §17.
