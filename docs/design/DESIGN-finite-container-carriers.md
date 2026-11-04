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
does not reach every expression symbolic evaluation cannot coerce: see the recorded routes below.

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
`types_compatible`, `can_retag` and `inferred_collection_element` still answer separately.

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

Audit: 1794 forms, base `e3c19a39c9c3bf5c315c6002fd18114aa22cd654` against head
`644278436fe0a94af236bd8aaa1207c58fedbe4f`, each built from its own tree with Rust 1.98.0, run as `fslc check`,
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

The other removed forms returned no verdict on bmc or on induction. Their losses are explicit on
82 forms and `fsl-bfs` on 234, where those solver-free routes evaluated a form the symbolic
engines refused. Explicit's include an unused `let u = m0 == m1`, `if false { m0 = m1 }` and
`requires false` followed by `m0 = m1`.

Of the forms still accepted, none reports `cannot coerce` on bmc, explicit or induction, and bmc,
explicit and `fsl-bfs` agree wherever all three give a verdict. Outside these grids three routes
still report it: a Map comparison in a refinement map under `preserve progress` and a rejected
`--lemma` candidate, both recorded above, and a range-typed literal under `old(...)`, which
predates this change. A literal there is not typed against the expectation, so it keeps its own range
and capacity: `a == old(Set { vb })`, `sa == old(Seq { vb })` and `sa == old(Seq { vb, vb, vb })` with
`vb: 0..2`, `a: Set<A>`, `sa: Seq<A, 2>`, and with the same range `sa == old(Seq { va })`, whose
capacity differs. These pass `check`, exit 2 on bmc and induction, and explicit answers. A sweep of
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

On-disk transition set (empty). Base `e3c19a39c9c3bf5c315c6002fd18114aa22cd654`, head
`644278436fe0a94af236bd8aaa1207c58fedbe4f`, each built from its own materialized tree in its own
target directory with `cargo +1.98.0 build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc
--locked --offline`. Every `.fsl` under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` (617
forms: 23/195/399) ran with `FSLC_CACHE=off` and its own tree as cwd, as `fslc check <path>` stable
and with `FSLC_UNSTABLE=fsl5`, and as `fslc kernel <path> --kernel-version 1|2`, twice per side. All
2468 pairs had equal complete stdout, stderr and exit, with no field excluded: 0 ok-to-error and 0
error-to-ok transitions. Of the 7 inline inputs of the rewritten
`issue_1517_conditional_arm_admission`, `f1` goes from ok to the refusal at 6:37 (the test is
renamed `f1_set_arm_outside_the_expectation_is_rejected_at_the_arm`) and 6 are byte-identical,
including `f4`, whose first arm is also a new refusal but whose second arm is still reported first.
Of the 159 inline inputs of `issue_1679_carrier_equality`, checked the same way at
`e6288cebf`, 46 go from exit 0 to exit 2 and 113 are byte-identical, including every control, rebuilt
value and third-route form.

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
