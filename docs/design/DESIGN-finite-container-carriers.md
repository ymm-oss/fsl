<!-- SPDX-License-Identifier: Apache-2.0 -->

# Whole finite-container carrier compatibility (#1472, #1627)

Status: Accepted.

## Decision

Whole values of Set, Seq, Map and relation are compatible only when their
corresponding carriers agree. This is one rule, extending #1522's relation
endpoint decision through the shared `same_carrier` helper, not four independent
conversion policies. Numeric aliases compare resolved lower and upper bounds;
equal-size shifted ranges are different. Set elements must have equal carriers;
Seq additionally requires identical capacity; Map requires both key and value
carriers to agree; relation requires both endpoint carriers to agree. Nested
container carriers are compared recursively. Enums and structs remain nominal.
The Set/Seq/Map gate is scoped to finite carriers. `Int`, including an inferred
literal element type, has no finite carrier: when either corresponding type
contains `Int`, the prior recursive compatibility is retained, except as amended
for a model expression by #1679 below. Inferred Seq
literal capacity follows that prior rule as well. This preserves both orders of
`s == Set { 1 }` and `s == Seq { 1 }`, empty literals and contextual conditional
assignments; it does not implement the literal semantics deferred to #1467.
When an uncontextualized Set/Seq conditional has one Int-derived branch and
one already known finite branch, inference retains that finite branch's type
and Seq capacity. Either arm order therefore preserves the carrier through a
`let`; a later incompatible whole assignment cannot use `Set<Int>` or
`Seq<Int,N>` as a bridge. Standalone and same-carrier conditionals still check.

This rule applies to assignments, equality/inequality, conditional arms,
projection results such as `domain`/`range`, collection receivers and aggregate
binders, and whole refinement state mappings. Both models resolve their own
numeric types before refinement comparison: identical names do not imply equal
ranges across numeric models. Same-name struct-field and enum/range collisions
now reject through #1599's shared `merge_refinement_model` and
`RefinementErrorKind::Type` before map validation; see
[the refinement contract](DESIGN-refinement.md).
Indexed Map mappings still compare each mapped value,
not whole Map key domains. Scalar Int/Range compatibility, Option payload
compatibility and numeric arguments to `add`, `contains`, `push` and relation
methods remain unchanged. In particular, an out-of-range scalar is not silently
dropped: accepted temporary values retain it and storage bounds can reject it.

This is a **breaking removal**, including both narrow-to-wide and wide-to-narrow
whole-value conversions, empty values, different Map keys and Seq capacities.
Use matching declared carriers; a wider range is not a supported container
supertype. The existing assignability error and existing expression/arm location
reject incompatible ordinary expressions before any engine evaluates them
(`semantics`, exit 2). Refinement retains its existing map-expression error
(`type`, exit 2). Upstream #1642 (`4885b44b`) recovers the authored map location
when the lower validator returns an unknown span; a synthetic map without an
authored span still retains the unknown **1:1** fallback.

Equal-range aliases remain compatible. Symbolic `coerce` changes only the outer
type tag for these whole values, preserving every entry, slot and length. It
neither extends nor truncates support. Scalar and Option coercion stay on their
existing paths. No syntax or literal materialization is added; composite literals and
their remaining semantics belong to #1467, including #1524's admitted literal
examples. This decision resolves the Set disagreement in #1627 rather than
documenting symbolic refusal as a supported engine difference.

## Amendment (#1679): carriers symbolic evaluation cannot coerce

**Breaking removal (#1679).** The Decision above keeps prior recursive compatibility for any
type containing `Int`. That admitted two forms `check` passed and bmc and induction then refused
with `cannot coerce symbolic value`, while `fsl-bfs` answered:

- **A Map holding `Int`.** `Map<K, Int>` against `Map<K, A>`. Coercion rebuilds a Set, Seq or
  Option value but can only retag a Map, and no literal or inference produces a Map, so the
  retained compatibility never had a working Map case. A Map's carriers must now agree in a
  model expression, including a Map inside `Option`, `Set` or `Seq`.
- **An arm related to the expectation only through `Int`.** With `let t = Set { 0 }`,
  `a == (if c then t else b)` checked `t` against `a` and `b` against `t`, and never `b` against
  `a`; a literal in that arm is contextual and was already refused. Every arm now also answers to
  a Set, Seq or Option expectation, in addition to the first-arm check. This narrows admission
  only. DESIGN-sync-argument-root-validation rejected replacing the first-arm check with the
  expectation, which changed admission in both directions; this does not replace it.

Both apply to the model's own expressions: action guards, lets, statements, `ensures`, `old(...)`,
init and every property kind, evaluated or not. `old(...)` keeps its operand's type, and its
operand's arms answer to what surrounds it without its literals being typed against that. So this
does not reach every expression symbolic evaluation cannot coerce: see the recorded routes below,
and #1683 for a literal under `old(...)`.

**Keeping the prior rule.** A refinement map, a scenario `expect` and a `--lemma` candidate stay on
the prior rule through `ValidationPhase::concrete_only`. The name is older than what it covers:

| Position | Evaluation | Result with `Map<K, A>` against `Map<K, Int>` |
|---|---|---|
| Scenario `expect` | concrete | `expect l == r` verifies on every engine, and `expect l != r` fails acceptance |
| Refinement map, refinement step | concrete | `map m = m` and `map same = m == y` refine, `map same = m != y` is `refinement_failed` |
| Refinement map under `preserve progress` | symbolic | **Recorded gap.** `map same = m == y` is exit 2, `cannot coerce`, on `refine`, and refines without `preserve progress`. `map m = m` and a Seq arm map still refine |
| `--lemma` candidate | symbolic | `rejected` with `cannot coerce`, `used` false, main result unchanged. A mismatch never evaluated leaves the candidate usable: `x == 0 and y == 0 and forall i: E { m0 == m1 }` is `proved`, `used`, and the main property it supports is `proved` |

Refusing a map's comparison would lose verdicts on the first refinement-step row. Giving a candidate
an invariant's check would change the last row's main result from `proved` to `unknown_cti`, so a
candidate keeps the prior rule.

**One rule.** `CarrierCoercion`, through `KernelModel::carrier_coercion`, is the rule `check` and
`coerce` both ask. `check` refuses its `MapMismatch`; `coerce` rebuilds a value on its `RebuildInt`.
`types_compatible`, `can_retag` and `inferred_collection_element` answered separately until #1683.

**Collections no literal expectation types.** A collection holding `Int` below an element checked
and then reported `cannot coerce` on bmc and induction, and on explicit except in a `trans` or a read
into state, while `fsl-bfs` answered, whenever it reached the comparison as anything but a literal the
expectation can type, because `coerce` converted only an element that was itself `Int`. That covers:

- a `let` binding: `let t = Set { some(0) }` against `Set { p }` for `p: Option<A>`;
- an `is` binding: `some(Set { some(1) }) is some(x) and Set { p } == x`;
- a method's result: `Set { p, none } == Set { o }.add(none)`, `.remove`, `.push`, `.head()`;
- a method's argument: `Set { Set { p } }.contains(t)`, `.add(t)`, `.remove(t)`, and
  `Seq { Seq { p } }.push(t)`;
- a read into state: `p = (if c then Seq { p } else t).head()`;

in a guard and in an `invariant`, `reachable`, `trans` or `leadsTo`. `coerce` now converts these after
retagging fails, so a value that had any coercion before is unaffected. No verdict is lost, where
refusing them at `check` would have lost `fsl-bfs` answers on each. `fsl-bfs` reports no
violation for a failing `leadsTo` or `reachable`, and explicit refuses `leadsTo`, so those two positions
have fewer engines.

A Seq holding `Int` keeps prior compatibility, including a differing capacity: `q == a` and
`q == d` for `q: Seq<Int, 2>`, `a: Seq<A, 2>`, `d: Seq<A, 3>` return a verdict on every engine.
So do `some(t) == some(a)`, `Seq { t } == Seq { a }` and `Option<Int>` against `Option<A>`. This
is narrower than "whole finite containers require equal carriers": `Int` has no finite carrier.

Audit: 1794 forms, base `268fdf62e6449c28c7048d04ca0030bd3f23c3b8` against head
`d7d35d672e61d5cce8ee3dfea0428f892240b347`, each built from its own tree with Rust 1.98.0, run as `fslc check`,
`fslc verify --engine bmc --depth 2`, `--engine explicit --depth 2`, `--engine induction` and
`fsl-bfs <spec> 2`. Shapes were Set, Seq, Map and Option, nested through literals and lets, with `Int`
against range carriers and equal-range aliases, in `==`, `!=`, conditional arms in either order and
operand side, assignment, `let`, `old(...)`, guards behind a dead `false and`, `ensures`, invariant,
reachable, trans, terminal, leadsTo, until and unless. A declared nested carrier such as
`Map<K, Seq<A, 2>>` is `unsupported state type` on both sides, so it is reachable only through
literals.

| Grid | Forms | ok to error | of which bmc and induction verdict | Explicit lost | `fsl-bfs` lost | ok to ok | error to error, same message |
|---|---|---|---|---|---|---|---|
| Original matrix | 672 | 198 | 0 | 67 | 198 | 191 | 283 |
| Wider positions, sibling literals, methods | 67 | 24 | 1 | 6 | 23 | 29 | 14 |
| Never-evaluated positions, ignored properties, nesting | 36 | 18 | 10 | 4 | 8 | 6 | 12 |
| Let-bound and literal nesting | 648 | 0 | 0 | 0 | 0 | 156 | 492 |
| Literal reads | 315 | 0 | 0 | 0 | 0 | 315 | 0 |
| `old(...)` | 16 | 5 | 0 | 5 | 5 | 4 | 7 |
| Let-bound, state matching and differing | 40 | 0 | 0 | 0 | 0 | 40 | 0 |

No form went from error to ok. The explicit and `fsl-bfs` columns count only the forms outside the
never-evaluated column.

**Removed with a verdict on bmc and induction: 11 forms, all never evaluated.** Each returned a
verdict on bmc, explicit, induction and `fsl-bfs` on base and is refused on head, by decision: a
mismatch stays refused where it is written.

| Form | Flag | Base bmc, explicit, induction, `fsl-bfs` |
|---|---|---|
| `invariant P { forall i: E { m0 == m1 } }`, `type E = 1..0` (two grids) | | verified, proved, proved, no violation |
| `invariant P { not (exists i: E { m0 == m1 }) }` | | the same |
| `invariant P { forall i: E { sa == (if c then q else sb) } }` | | the same |
| `requires forall i: E { m0 == m1 }` | | the same |
| `action go(i: E) { requires m0 == m1 ... }` | | the same |
| `action go(i: E) { m0 = m1 }` | | the same |
| `invariant Bad { m0 == m1 }` | `--exclude-property Bad` | the same |
| `reachable Bad { m0 == m1 }` | `--exclude-property Bad` | the same |
| `trans Bad { old(m0) == m1 }` | `--exclude-property Bad` | the same |
| `invariant Bad { m0 == m1 }` | `--property Q` | the same |

In these grids the other removed forms returned no verdict on bmc or on induction. Their losses are explicit on
82 forms and `fsl-bfs` on 234, where those solver-free routes evaluated a form the symbolic
engines refused. Explicit's include an unused `let u = m0 == m1`, `if false { m0 = m1 }` and
`requires false` followed by `m0 = m1`.

Outside the grids, a `decreases` measure is the same kind of position. bmc does not read one, and
induction reads one only once it ranks. `leadsTo L { n < 3 ~> n == 3 decreases (if m0 == m1 then 1 else 0) }`
in a spec where `n` never moves returned `violated` on both without evaluating the measure, and with
an action that reaches `n == 3` it returned `verified` on bmc and `cannot coerce` on induction. Both
are refused, since a mismatch stays refused where it is written.

`check` walks a model twice, in ordinary validation and then in range admission (#1480), and
`build_model`, which is all `fsl-bfs` and the Monitor have, runs only the first. Every action,
property, `leadsTo` and terminal scope carries the rule on both walks, including `old(...)`'s own arm
and its operand, and a binder's bound is checked there rather than by the range planner, which
evaluates an endpoint itself and also serves the positions that keep the prior rule.

Of the forms still accepted, none reports `cannot coerce` on bmc, explicit or induction, and bmc,
explicit and `fsl-bfs` agree wherever all three give a verdict. Outside these grids three routes
still reported it at this amendment: a Map comparison in a refinement map under `preserve progress`
and a rejected `--lemma` candidate, both recorded above, and a range-typed literal under `old(...)`,
which predated this change and closed in #1683. A literal there was not typed against the expectation
in evaluation, so it kept its own range and capacity: `a == old(Set { vb })`, `sa == old(Seq { vb })` and `sa == old(Seq { vb, vb, vb })` with
`vb: 0..2`, `a: Set<A>`, `sa: Seq<A, 2>`, and with the same range `sa == old(Seq { va })`, whose
capacity differs. These passed `check`, exited 2 on bmc and induction, and explicit answered. A sweep of
480 `old(...)` forms found 18 of them, each Set or Seq; the controls with a literal holding `Int`, or
of the expectation's own carrier, answer on every engine. 22 forms that reported `cannot coerce` now return
a result.
A rebuilt value is not one of the mismatches the other two routes report. In 144 `--lemma` runs with
one in the candidate, each `rejected` with `cannot coerce` on base, none reports it now. All 21
`proved` candidates held as an invariant on explicit and `fsl-bfs`, all 7 `proved` main results and
all 72 `violated` ones matched explicit, and the other 123 candidates were `rejected`, 99 of them
violated on explicit and 24 true there but not inductive. In 20 refinement maps with one under
`preserve progress`, the result equalled that of the same map written by hand as `p == o` or
`p == some(1)`, 4 `refines` and 16 `refinement_failed`, and the two maps' values were equal on explicit
and `fsl-bfs`. No concrete engine evaluates the pulled `leadsTo`, so that is the comparison available.
For the let-bound collections, each form ran with the compared state matching and differing: bmc,
explicit and `fsl-bfs` gave a violation exactly when it matched, induction gave `violated` or
`proved` or `unknown_cti`, and a literal outside the carrier (`some(5)` against `Option<A>`) was never
equal. Explicit's own `leadsTo` refusal is unchanged. The grids are a finite measurement and were
not retained; `rust/fslc/tests/issue_1679_carrier_equality.rs` pins representative forms of each row.

On-disk transition set (empty). Base `268fdf62e6449c28c7048d04ca0030bd3f23c3b8`, head
`d7d35d672e61d5cce8ee3dfea0428f892240b347`, each built from its own materialized tree in its own
target directory with `cargo +1.98.0 build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -p fsl-runtime --bins
--locked --offline`. Every `.fsl` under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` (646
forms: 23/195/428) ran with `FSLC_CACHE=off` and its own tree as cwd, as `fslc check <path>` stable
and with `FSLC_UNSTABLE=fsl5`, and as `fslc kernel <path> --kernel-version 1|2`, twice per side. All
2584 pairs had equal complete stdout, stderr and exit, with no field excluded: 0 ok-to-error and 0
error-to-ok transitions. Of the 7 inline inputs of the rewritten
`issue_1517_conditional_arm_admission`, `f1` goes from ok to the refusal at 6:37 (the test is
renamed `f1_set_arm_outside_the_expectation_is_rejected_at_the_arm`) and 6 are byte-identical,
including `f4`, whose first arm is also a new refusal but whose second arm is still reported first.
Of the 238 inline inputs of `issue_1679_carrier_equality`, checked the same way, 113 are byte-identical,
including every control, rebuilt value and third-route form, 117 go from exit 0 to exit 2, and 8 are
binder bounds the range rule already refused on `check`, which now report the carrier first.

## Amendment (#1683): one carrier rule

`check` and `coerce` now answer from one function, `CarrierCoercion` through
`KernelModel::carrier_coercion`, with four answers: `Retag`, `RebuildInt`, `MapMismatch` and
`NotAssignable`. Assignability is every answer but the last. `coerce` retags on the first, rebuilds on
the second and reports `cannot coerce` on the others. A `check` phase that reaches coercion refuses
`MapMismatch`. `compatible_carrier`, `can_retag` and `inferred_collection_element` are gone.

**What stays outside the rule, and why.** Nothing in `coerce` needs the solver: `solver_sort` reads only
the model's types.

| What | Where | Why |
|---|---|---|
| `ValidationPhase.symbolic` | `check` | It says where an expression is evaluated, and the rule sees two types. Dropping it refuses a scenario `expect` and a refinement step's map that answer on every engine (`concretely_evaluated_positions_keep_their_verdicts`) |
| `is_coerced_container` | `check` | It picks the positions the arm rule runs at. Running it at a Map too moves an existing refusal from one arm to the other. The rule it applies is `types_compatible`, so this one |
| `None`, `SetLiteral`, `SeqLiteral` arms | `coerce` | A literal has no carrier of its own, only elements, and each element goes back through `coerce` |
| `Option` arm | `coerce` | An Option is rebuilt whatever its carrier, and its value answers to the rule on the way down. The rule says the same (`RebuildInt`), so nothing is decided twice |
| `Scalar` arm | `coerce` | A scalar is one term. It retags to any scalar sort, including `Bool` to `Int`, which `check` refuses. [R] Stays looser than the rule because no form reaches it past `check`: no grid below did, and it fails open only for a form `check` refused |

**One narrowing in `coerce` itself.** `inferred_collection_element` rebuilt `Set<Int>` as `Set<Bool>` or a
Set of an enum, or the reverse, and the rule answers `NotAssignable`. [R] No form reaches that: `check`
refuses each, in the grids `set_a_e`, `seq_bool`, `setlit_bool` and `setlit_e`, and in the `old(...)`
probe `Set { 1 } == old(Set { true })`. Against the same carrier `Set<Int>` now keeps its value instead
of rebuilding an equal one.

**A literal under `old(...)`.** `infer_type` has always carried the expectation through `old(...)`, and
`eval_expected` carried it through every other form that does, so this was the one place the two
disagreed.

- `eval_expected` now carries it there too. **This closes the third route above.** `a == old(Set { vb })`,
  `sa == old(Seq { vb })`, `sa == old(Seq { vb, vb, vb })` and `sa == old(Seq { va })` return a result that
  agrees with explicit, and with the same form outside `old(...)`.
- **Breaking removal.** `check` now walks `old`'s operand against the expectation too, so a Map a literal
  holds there answers to the rule, as it does outside `old(...)`. Before, `infer_type` only saw the literal
  take the expectation. For `m0: Map<K, Int>`, `m1: Map<K, A>` this refuses `some(m0) == old(some(m1))`,
  `some(m0) != old(some(m1))`, `Set { m0 } == old(Set { m1 })`, `Seq { m0 } == old(Seq { m1 })` and
  `Set { some(m0) } == old(Set { some(m1) })`, with the located assignability error. Where such a form is
  evaluated it reported `cannot coerce` on bmc and induction, so no verdict goes with it. **Under a
  never-evaluated position it returned a verdict, and that verdict is lost:** `forall i: E { ... }`,
  `not (exists i: E { ... })` and an arm under such a binder, for `type E = 1..0`, and a property that
  `--exclude-property` leaves out, each `verified` on bmc and `proved` on induction. This is the never-evaluated kind #1679 decided: a mismatch stays refused where
  it is written. The earlier walk stays, so an uncontextualized `old(none)` is still refused.

**The routes that remain.** A Map comparison in a refinement map under `preserve progress`, and a rejected
`--lemma` candidate. Neither is on bmc or induction's model path, and refusing either at `check` loses a
verdict. A map under `preserve progress` that the pulled `leadsTo` does not read returns `refines`, with
`map same = m == y` for those Maps, and `cannot coerce` only when `leadsTo L { same and ... }` reads it. The
candidate is the #1679 row above: one form's main result would go from `proved` to `unknown_cti`.

**The matrix.** Generated, not retained, except for the bounded grid in the last row. Base
`1559e8eba90f73615d4efd00fcdccc30cf525b42`, head `ab1cacbc6fbd19363a4c1203a13fcddb306a4d82`. Each ran as
`fslc check`, and where it passed as `verify --engine bmc --depth 2`, `--engine induction`,
`--engine explicit --depth 2`, `fsl-bfs <spec> 2` and `fsl-replay-actions` with one `go`.

| Grid | Forms | Refused, base | Refused, head | ok to error | of which verdict on base | `cannot coerce`, base | `cannot coerce`, head | Result gained | Verdict changed or lost otherwise |
|---|---|---|---|---|---|---|---|---|---|
| Carrier shape by position, by property kind | 4577 | 1822 | 1826 | 4 | 0 | 38 | 0 | 34 | 0 |
| Wrappers of `old(...)`, with never-evaluated positions | 5253 | 2137 | 2207 | 70 | 30 | 156 | 0 | 116 | 0 |
| The #1679 grids again | 1754 | 1053 | 1053 | 0 | 0 | 0 | 0 | 0 | 0 |
| `issue_1683_one_carrier_rule`, retained | 560 | 194 | 198 | 4 | 0 | 15 | 0 | 11 | 0 |

No form went from error to ok. Every refused form has a location. Shapes are Set, Seq, Option, Map, a
relation, a scalar and an enum, with range, equal-range alias, `Int` and capacity differences, plus literals
holding `Int`, a range value, a differing capacity, `Bool` and an enum, nested literals and `some`.
Positions are `==`, `!=` and either order, conditional arms on either side, `old(...)` around and inside
each, `some`, Set and Seq literals with sibling elements, `.contains`, `.add`, `.remove`, `.push`, `.head`, `.at`,
`is`, `forall`, `exists`, `count`, `sum`, `=>`, a dead `and`, `requires`, `ensures`, `let`, assignment, `if`, and for
10 representative shapes `invariant`, `trans`, `reachable`, `leadsTo` and `terminal`. bmc and explicit disagree
on no form where both give a verdict. bmc and `fsl-bfs` differ on 12 forms, each a `leadsTo`, for which `fsl-bfs` reports no violation
as recorded above. `fsl-bfs` and the Monitor gave the same output on base and head wherever both ran; the
Monitor was not compared with another engine.

**Not covered:** `until` and `unless` bodies, `decreases`, action parameters, binder bounds, init statements
and expressions, `def` and predicate arguments, struct fields, Map index writes, deeper literal nesting than two
levels, compose and the dialects that lower to these forms, `--exclude-property` and `--property` beyond the #1679
grid, and a scenario `expect`, refinement map, `--lemma` candidate or KPI projection beyond the #1679 tests. A
declared nested carrier such as `Set<Option<A>>` is `unsupported state type`, so it is reached only through
literals, as in the #1679 let-bound grid. The grids are a finite measurement.

**What still detects a wrong shared rule.** Merging removes the check this question used to have: `cannot coerce`
past `check` meant the two rules disagreed, and now they cannot. DESIGN-triangulated-assurance does not count
a shared classification as independent evidence, and this is not registered as a triangulated claim.

- Two hand-written tables that do not call the rule for their expectation. `rust/fsl-core/tests/carrier_coercion_rule.rs` states the answer for each pair. `rust/fsl-verifier/src/carrier_coercion_tests.rs` asks `coerce` directly, past `check`, and has the solver compare the coerced value with the same value built at the target type.
- Pinned refusal counts per shape in `rust/fslc/tests/issue_1683_one_carrier_rule.rs`, and the forms pinned in `issue_1679_carrier_equality`.
- Explicit, `fsl-bfs` and the Monitor, which evaluate without asking the rule. A rule that wrongly admits or rebuilds a value can disagree with them on a verdict. They run after the same `build_model`, so **a rule that wrongly refuses a form refuses it on every engine, and no agreement check sees that.** Only the tables and the pinned counts do.

On-disk transition set (empty). Base and head as above, each built from its own tree in its own target directory
with `cargo build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -p fsl-runtime --bins --locked --offline`,
Rust 1.98.0. Every `.fsl` under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` (646 forms: 23/195/428) ran with
`FSLC_CACHE=off` and its own tree as cwd, as `fslc check <path>` stable and with `FSLC_UNSTABLE=fsl5`, and as
`fslc kernel <path> --kernel-version 1|2`, twice per side. All 2584 pairs had equal complete stdout, stderr and exit,
with no field excluded: 0 ok-to-error and 0 error-to-ok, 0 unstable. Of the 425 forms base accepts, `verify --engine bmc
--depth 2` and `--engine induction` gave equal stdout, stderr and exit on both, once `elapsed_s` fields and `memory_mb`
were excluded, because each differs between two runs of one binary: 303, 104 and 18 exit 0, 1 and 2 on bmc, 312, 95 and
18 on induction, none reporting `cannot coerce`.

Inline inputs of `issue_1679_carrier_equality`, whose third-route test this rewrites and renames, and of
`issue_1683_one_carrier_rule`: 689 single-file forms checked the same way, 678 byte-identical and 11 ok to error, each a
Map under a literal under `old(...)` in the new test. The 20 refinement files in the #1679 test were not checked on their own.

## Mutation classification

A statically ill-typed map is invalid mutant input, independent of runtime
oracle order. Static implements validation runs before the main and requirement
oracles, while the runtime refinement walk remains after both leave the mutant
clean. The shared refinement validator carries a
typed `RefinementErrorKind::Type`; mutation consumes that kind, not a phase name
or message substring, and publishes through the same `MutationOracle::Invalid`
path used for check-invalid builtins and external mutants. Whole-container
implements-map mutants that change a shared numeric range therefore move from
`error` / `summary.errored` to `invalid` / `summary.invalid`. Invalids contribute
to neither side of `kill_rate` and do not add `oracle_errors` to the gate.
Solver, runtime evaluation, dependency and other oracle failures remain errors;
budget cutoffs remain inconclusive. No new summary field or gate policy is added.

| Sibling path | Shared validation / classification boundary |
|---|---|
| Requirements implements, maps auto or explicit maps | Same refinement type validator and typed mutation classifier, for builtin and external mutants |
| Standalone refinement / refines verdict | Same validator rejects ill-typed maps before concrete checking; ordinary refine errors keep their existing envelope, not a mutation status |
| Governance preservations | Existing baseline governance preflight uses refinement validation before mutation enumeration; governance is not a new per-mutant oracle |
| Compose | Lowering/model errors already use the check-invalid mutation path; composed abstract models use the same refinement validator; no consumer-local carrier rule |

The new mutation detector covers auto/explicit maps, the invalid detail,
`kill_rate`, judged count and a passing gate alongside a valid survivor control.
Existing injected solver/evaluation controls still produce errored outcomes and
fail gates. The unmodified #1262 budget fixture now stays green: its four
range-changing mutants are invalid, while the two intended budget cutoffs retain
the original inconclusive gate and exit. This does not declare standalone
refinement or governance documents newly supported mutate inputs.

## Type-boundary evidence

The current measurement compares base `796876558b83f2dedd7dafdf04ef77fced3de033`
with `3c99c93b79414c3a3755451901c5c44506e4ac03`. Both were extracted with `git archive`
into a fresh directory and built independently with Rust 1.98.0. Each binary
ran `fslc check <relative-path>` in its own tree, with cache disabled, twice over
all **606** `.fsl` files in `specs/`, `examples/` and `rust/fslc/tests/fixtures/`,
plus **16** original inline sources from every rewritten existing test.
Another **18** literal-boundary forms were checked twice per binary: empty and
nonempty Set/Seq assignment and both equality orders remain accepted; attempted
Map literal syntax remains a parse error on both. Another **18** conditional
bridge forms cover two arm orders, Set/Seq range mismatch and Seq capacity,
plus standalone and same-carrier controls. All **658** same-binary check repeats
were byte-identical, including stderr and exit. The **corpus ok→error set is
measured empty**: all 606 corpus base/head outputs match in full. Across all 658
forms there are **nine check ok→error removals**, three original inline forms
and six bridge forms, and **zero diagnostic-only error→error changes**:

| Original inline source | Removed carrier mix | Existing location |
|---|---|---|
| `obligation_catalog.rs`, `FALSE_VACUITY` / `CondMap` | `Map<Small,V>` vs `Map<K,V>`, `Small=0..0`, `K=0..2` | 20:41 |
| `issue_1522_relation_endpoint_bounds.rs`, `MAP` | conditional `Map<B,Bool>` vs `Map<A,Bool>`, `B=0..2`, `A=0..1` | 7:35 |
| `issue_1522_relation_endpoint_bounds.rs`, `SET_WIDENING` | `Set<N>` to `Set<M>`, `N=0..2`, `M=0..4` (#1627 form) | 6:48 |
| Set conditional/let, literal then finite and finite then literal | `Set<N>` to `Set<M>`, `N=0..2`, `M=0..4` (two forms) | 8:3 |
| Seq conditional/let, both arm orders | `Seq<N,2>` to `Seq<M,2>`, `N=0..2`, `M=0..4` (two forms) | 8:3 |
| Seq conditional/let, both arm orders | `Seq<N,2>` to `Seq<N,3>` (two forms) | 8:3 |

Four additional original **admitted refinement-map forms** now reject statically:
runtime `SeqWide` to `SeqAbs`, runtime `SeqWider` to `SeqGuarded`, CLI standalone
`SEQ_WIDE_IMPL` to `SEQ_ABS`, and CLI inline `seq_req(-1)`. Their capacity is 2;
their model-local `V` ranges are respectively `-1..2` vs `0..2`, `0..3` vs
`0..2`, and `-1..2` vs `0..2` for both CLI forms. These previously reported
`refinement_failed` (exit 1, `map_out_of_bounds` or `abs_requires_failed`); now
they report the existing map assignability error (exit 2), with authored
locations 1:40, 1:45, 4:3 and 2:3 respectively. The inline
case changes both check and verify, but is one source removal, not two.
Three standalone refine and two inline verify consumers supplement the check
sweep, twice per binary; legal `seq_req(0)` remains verified. These are all
equal-carrier-rule removals, not a change to indexed scalar mappings.

Separate base/head observations used literals 0 and 3 against 0..2 for Set/Seq,
both if-arm orders, both Boolean choices and standalone/same-carrier lets:
32 forms, four routes, twice on each binary. Check stayed accepted. When 3 is
selected and stored, all three engines report a violation; otherwise explicit
agrees with the actual concrete execution. Existing symbolic shape errors
remain errors where #1467 applies; no new verified/proved result hides a concrete
violation, and no stable verdict/kind/message changed between base and head.

## Authored map diagnostic integration (#1642)

Four rejection controls originally pinned the unknown 1:1 location. With
upstream authored recovery, their complete non-location contract remains the
same (Type kind and exact message, plus CLI error/exit2); only span/location
changes. Each replacement is anchored independently to the unchanged source:

| Test | New start/end | Literal source anchor |
|---|---|---|
| `old_argument_fixture_whole_seq_map_is_rejected` | 1:40–1:43, offsets39..42 | `map` in `SEQ_WIDE_MAP` |
| `old_guard_fixture_whole_seq_map_is_rejected` | 1:45–1:48, offsets44..47 | `map` in `SEQ_WIDER_MAP` |
| `old_standalone_whole_seq_map_is_rejected` | 4:3–4:6, offsets45..48 | `map` in `SEQ_MAP` |
| `old_inline_whole_seq_map_is_rejected` | 2:3–2:13, offsets24..34 | `implements` in `old_seq_req(-1)`, not `maps auto` |

Disabling only authored recovery makes all four exact-location detectors reject
1:1 after their message/kind checks; restoring it restores the expected spans.
This is independent of carrier-gate, retagging, conditional and mutation-status
calibrations. The unknown-span fallback is not removed or claimed unreachable.

## Diagnostic priority with preserved progress

After integration with #1595, independent probes exercised a state map
`seq = if (p is some(z)) then xs else xs` and a preserved abstract progress
property with a binder named `z`. With equal Seq carriers, both base and head
report the mapping-pattern capture at the existing `respond` location 1:116.
With implementation `V=0..3` versus abstract `V=0..2`, head reports the map's
carrier error first, at the then-existing unknown location1:1, whether or not the
pattern also captures `z`. Base still accepts the type-only form and reports
the capture for the doubly invalid form. A fresh pattern `w` with equal carriers
refines on both. All four probes ran twice per binary. Thus the mixed probe has
an observed error→error priority change outside the earlier 637-form sweep; the
#1595 capture check is not removed or bypassed for well-typed maps.
Those are historical pre-#1642 observations, not a current universal1:1 promise.

This is a standalone refinement boundary. Attempting `preserve progress`
inside inline `implements` remains a parse error on both binaries, before
mutation enumeration. It is not newly assigned a mutation classification.

## Executable controls and limits

`issue_1472_collection_carriers.rs` pins located refusals across check, BMC,
induction and explicit, matched-carrier reachable violations, same-range Set
alias retagging and scalar/Option preservation. The migrated catalog/runtime/CLI
tests retain the complete original sources and preserve runtime argument,
guard-priority and mutation-attribution negatives with well-typed fixtures.
The two obsolete #1522 Set/Map check-ok controls now pin located refusals with
their original sources, including replay. Removing the Set carrier gate makes
the assignment detector see check ok; removing alias retagging restores
`cannot coerce symbolic value` and fails the alias detector. Exact production
source hashes and empty diffs established both mutation restorations.
