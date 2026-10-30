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
contains `Int`, the prior recursive compatibility is retained. Inferred Seq
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
