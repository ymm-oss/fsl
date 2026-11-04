<!-- SPDX-License-Identifier: Apache-2.0 -->

# Checked executable-form boundaries (#1463)

Status: Accepted for #1463, with separate follow-ups in #1480, #1481, #1459 and #1467.

## Decision and breaking removals

The native checked-model builder owns three early rejections, shared by check,
every verify engine, LSP diagnostics, and Worker preflight:

| Previously check-ok form | New boundary | Preserved sibling |
|---|---|---|
| A leadsTo wrapper with a Set/Seq collection binder, or any where filter | Reject with the authored property location | Supported unfiltered typed/range wrappers and quantifiers inside P/Q |
| An init statement forall whose collection source contains a free state reference | Reject with the offending ForAll location, even in dead branches or with empty domains/bodies | State-independent literal sources and assigned-state reads outside the source that satisfy #1518's same-forall root-read rule |
| Relation indexing, whether a read or indexed assignment | Reject through the shared index-type projection | Map indexing and relation contains/add/remove/domain/range |

The later #1518 init root-read boundary is recorded in
[DESIGN-bridge.md](DESIGN-bridge.md); the preserved reads above remain subject
to that rule and its current-binder-key exception.

These are breaking removals of accepted forms without executable semantics,
not implementations of filtered or collection-bound liveness. The old general
finite-binder wording in LANGUAGE.md and LANGUAGE.ja.md did not express the
accepted temporal/induction designs' fail-closed wrapper boundary. The language
references and skill syntax reference now state the position-specific rules.
The checker deliberately does not run the whole static leadsTo expander:
unrelated domain and bound limitations must not become extra check rejections.

The #1463 decision is that aggregate source typing is independent of candidate count.
Before Count/Sum normalization, the written binder, filter and value pass
the same existing binder/local-scope/expression checks as other aggregates.
An empty domain cannot erase a forbidden relation index or another already
enforced typing error. This also rejects previously accepted Bool indexing
inside an empty aggregate, an incidental closure of a #1459-class hole;
no new type rules are added. Valid empty/nonempty Map aggregates
retain their verdicts, and source errors retain the authored property location.

The written pass does not enumerate nested Count/Sum candidates before an
enclosing binder is substituted. A pattern name awaiting that substitution is
checked in the existing normalized scope, not prematurely treated as a collision;
unrelated collisions and forall/unique scope rules remain enforced. This preserves
the existing explicit positive/negative dependent-nested and Count/Sum pattern
cases without adding symbolic execution support or an error-triggered fallback.

Imported-component diagnostics retain exact ownership through private check
targets during lowering. A late model error uses the existing component-error
policy: anchor the envelope at that component's root use declaration, retaining
the component file/position and public declaration spelling in the cause.
Root declarations are not rebound to a use. A root-authored sync argument, used or not, is
typed before substitution in root header context, and its error is reported at the argument
with a bare message, not through a component wrapper
([root-argument validation](DESIGN-sync-argument-root-validation.md)). This is a breaking
removal for arguments that previously went unchecked (#1517); numeric ranges stay
verification-time.
A second component is not mistaken for the first.
These diagnostic-only targets neither replace action trace origins nor appear in Public Kernel provenance
or assurance counts.
Ownership follows individual guards, lets, statements and ensures expanded into
a sync action; the action as a whole is not assigned to the first component.

## Scope correction is a separate fix

The one core state-reference walker counts only free state names. A binder's
source and range bounds use the enclosing scope; its filter, quantified body
and aggregate value use the child scope. Child scopes never leak into sibling
expressions, and enclosing statement locals also hide same-named state roots.
The public reference list remains lexicographically ordered; inline-initializer
diagnostics retain preorder-first selection. No initializer rule is widened.

This repairs false symbolic and explicit refusals for valid shadowed names,
including explicit init filter/body/RHS/index checks. It does not narrow
check acceptance. A free state reference in the binder's own source, or after
a nested binding's scope ends, is still a state reference. No new pattern
binding, evaluation order, solver dependency, or fallback is introduced.

## Engine-specific exclusion and unchecked models

Nested statement forall in init remains accepted by check and executable by
the symbolic engines subject to #1518's read-order rule, but explicit rejects
it. Existing auto routing sends
that limitation to symbolic BMC. This change does not extend explicit init
coverage or route around newly invalid source forms.

KernelModel fields are mutable to native API callers. The runtime and symbolic
init entries therefore keep calls to the same core collection-source guard,
and the static liveness expander keeps the same wrapper guard. Existing tests
now check the early source rejection and then deliberately inject the original
unsupported domain/filter into an otherwise checked model, preserving the
bounded/ranked/runtime rejecting controls rather than deleting them. They are
not fallbacks for source that check still accepts.

The symbolic init guard has a direct unchecked-model bounded test and an
injected-model CLI induction-entry test. The latter reaches that guard through
induction's bounded base, matching the actual orchestration. The pure induction
step API is init-agnostic and does not gain an artificial init check. Removing
the transition guard must fail both controls. Public Kernel lvalue projection
also uses the same index_types rule rather than a stale Map-or-Relation copy.

## Evidence and exact transition inventory

Base: 5738cc2c711b691a9d453b0190ead1a2ff77fc50. The required sweep runs each
named binary in its own materialized tree over all specs, examples, and CLI
fixtures, plus every input of the rewritten required tests. The original
349-pair collection inventory and its six rejected boundaries are retained;
old/new inputs are captured from actual test emission. The two rewritten
runtime/verifier guard tests retain all their literal/replace inputs, with
their new supported-constructor controls added separately.

Measured implementation: the #1463 change (exact measured head recorded in the PR). Each binary ran
`FSLC_CACHE=off fslc check <form>` in its own Git-archive materialized tree;
tracked source blobs were verified against the named commits. The base binary
SHA256 was 3d4adac58ac5baccfa72574b44bf285e64f88d572a2551a6d4b735a485729f44;
the head binary SHA256 was 741e6e65c3a984ab3ebb75e7a9d0876b821e7aa6087acdc36d03ae89acb9bc4f.
Both runs exited 0 and their complete reports agreed without excluding any
JSON field: 597 corpus forms plus 1016 inline IDs, 1613 total, with 110
accepted-to-error transitions (1 corpus, 109 inline). Both newly added legacy
return fixtures were included and unchanged. This is a complete fresh-base sweep,
not a reuse of a prior base's transition set. A check-time
refinement/trace verdict with exit 1 is not
an input rejection: acceptance here means result is not `error`, not that its
property was satisfied. All 110 transitions below are breaking removals.

This is the complete transition list using Cartesian brace expansion; counts
are fixture IDs, not independent language rules. Inline origins are the
preserved #1457 generator, the shared issue_1463_forms inventory, and the two
rewritten guard tests. Their supported-constructor inputs are also measured.
The aggregate/component and nested/sync ordinal IDs follow their committed shared cases()
order, captured from actual native emissions with resolver files; additional
unchecked-init and core constructors are included. No generator was recreated
to guess the inputs. These are measurements of the implementation identified in the PR,
not a relabeling of its later documentation-only descendants.

| Complete form IDs (expand every brace choice) | Count |
|---|---:|
| `corpus/rust/fslc/tests/fixtures/explicit_init_state_collection.fsl` | 1 |
| `inline/1457/init-stmt/{empty-body,empty-domain}/{Set,Seq}/state/{positive,negative}` | 8 |
| `inline/L1/{Set,Seq}/{state,literal}/{false,true}` | 8 |
| `inline/L1/{utf16-prefix,resolved-component}` | 2 |
| `inline/L2/{z: K,z in 0..1}/{z == 1,not done,true,false}` | 8 |
| `inline/L3/z in Set { 1 }` | 1 |
| `inline/L3/z: K where z == 1` | 1 |
| `inline/I1/{Set,Seq}/{false,true}/{plain,conditional,unreachable,nested}` | 16 |
| `inline/S4/{own-source,sibling-restored,aggregate-source}` | 3 |
| `inline/R1/read/{init,action,invariant,trans,leadsTo,reachable}/{false,true}` | 12 |
| `inline/R1/write/{init,action}` | 2 |
| `inline/R1/receiver/{old,conditional,domain-binder}` | 3 |
| `inline/guard/{StateCollection,ranked-where}` | 2 |
| `inline/aggregate-and-component-locations/case-{00,04,08,12,16,20,24,28,32,36,40,44,48,52,56,60,64,68}` | 18 |
| `inline/aggregate-and-component-locations/case-{01,05,09,25,29,33}` | 6 |
| `inline/aggregate-and-component-locations/case-{72,73,74}` | 3 |
| `inline/aggregate-and-component-locations/unchecked-init/state-source` | 1 |
| `inline/nested-aggregates-and-sync-ownership/case-{68,69,70,71}` | 4 |
| `inline/nested-aggregates-and-sync-ownership/case-{76,77,78,79,80,81,82,83,84,85,86}` | 11 |

All 110 belong to the scope approved for #1463: 22 leadsTo wrappers,
31 free-state init collection sources, 51 relation-index forms (including
18 aggregates), and 6 empty-aggregate Bool-index forms, an incidental #1459-class
closure under the aggregate source-typing decision. The latter are
count-filter/sum-filter/sum-value over an empty literal range or zero-capacity Seq.
Empty Count/Sum now reports every existing type error in the written expressions,
such as the non-Bool filter in `count(x in 1..0 where x + 1)`; the 6 Bool-index
removals are examples in the measured corpus/inline inputs, not an exhaustive
list of type-error kinds.
The three component cases add one form to each original boundary; the unchecked-init
source adds one to the init-source boundary. All 36 Map positive/negative aggregate controls retain
their check/engine behavior.
The 15 nested/sync inventory removals are four quantifier lo/hi relation-index bounds and
eleven sync-expanded or appended-root relation-index forms (read, write, requires,
let and ensures). None is a nested-aggregate or pattern-name removal: all 66
nested/pattern check controls remain accepted with identical complete
base/head envelopes, including empty/nonempty and positive/negative forms.
Those controls were later narrowed by
[checked range admission](#1480-checked-range-admission), which refuses a
dependent bound the engines cannot execute; this inventory is the #1463 measurement.

There were 128 full-output differences: the 110 removals and exactly 18 intended
diagnostic changes to inputs already rejected with error/semantics, exit 2:

| Still-rejected form IDs | Exact intended diagnostic change |
|---|---|
| `inline/1457/boundary/nonfinite` | The same finite-scalar refusal gains authored location 1:161 in message and loc, previously absent, because binder validation now precedes normalization |
| `inline/R2/seq-write-remains-rejected` | Location remains 7:17; indexed update requires Map, not Map or Relation |
| `inline/aggregate-and-component-locations/case-{13,17,37,41,49,53,61,65}` | Bool aggregate indexing remains rejected; replace synthetic 1:1 with authored property 6:3 and remove Relation from the Map/Seq repair message |
| `inline/aggregate-and-component-locations/case-{21,45,57,69}` | Same Bool-index refusal at unchanged 6:3; remove Relation from the Map/Seq repair message |
| `inline/nested-aggregates-and-sync-ownership/case-{72,74}` | Aggregate lo/hi bounds already failed static-range validation; now report the forbidden relation index at authored property 6:3, previously unlocated |
| `inline/nested-aggregates-and-sync-ownership/case-{73,75}` | The same pre-existing aggregate-bound refusal now names part.Capture and piece.fsl:6:3, anchored at the owning root use 2:3 |

Each complete head envelope equals its base envelope with only the exact
message/location edits above; verdict, kind, exit, versions and stderr remain
unchanged. No field or location difference is excluded. Imported failures among
the aggregate/component removals additionally bind the owning second root use at 3:3, preserve
piece.fsl cause locations 8:3/6:5/8:3, and show part.Capture/part.xs rather than
lowered part__ names. These refusals, coordinates, names and position-neutral
leadsTo-wrapper messages are intentional public-output changes, not parity waivers.
Model-expression type errors now point to their owning declaration, including
errors whose base check envelope already had a loc; component type errors,
including pre-existing ones, anchor at the owning root use and retain their
component cause.

## Explicitly separate follow-ups

\#1480 decided static/dynamic range bounds, negative within, and typed-domain
enumeration, recorded in the sections below; #1481 cell 1 owns Map Struct per-field init coverage.
Head, `at(i)`, and index reads of empty zero-capacity Seqs are fixed by #1481
cell 2 in ordinary action/property contexts: supported engines report
`partial_op` when reached. Ranked decreases narrowly refuses fresh-value cases
as described in [Seq partial operations](DESIGN-seq-partial-operations.md);
the following general partial-operation measure probe (#1542) also covers
physical-slot temporaries, including capacity zero, without changing `check`.
\#1459 owns operand/method/relation-builtin typing gaps. Literal shape relatives
remain #1467. Their current acceptance/verdicts are preservation
controls, not additional restrictions or implementation claims in #1463.

#1459 closes the `X1/operand-method-types` row (`acyclic(done)` for Bool `done`),
which now expects a located check rejection at 8:3 and outcomes
`[Error, Error, Error, Error]` instead of `[Ok, Error, Error, Error]`.

## #1480: valid static arithmetic

[Static binder constant expressions](DESIGN-static-binder-constant-expressions.md)
adds successful checked integer folding to the three static binder adapters.
It does not change the early wrapper/filter/init-source rejections above.
Failed folding retains each old helper and its recursive diagnostic precedence.
`X1/static-range` is `[Ok, Safe, Safe, Error]`, and the
[checked range admission](#1480-checked-range-admission) changes `X1/dynamic-range`.
Explicit liveness remains unsupported.
The own-tree inventory of that change compares B `d11f32631f75afa64fa486cdf4f9b46c6e77d28e`
with code H `c86f6bf4422b507f57862f830ad13726f8ce7734`: 610 corpus +299 actual
inline IDs, **zero ok→error transitions** and no full check-output changes.
The linked decision records the exact command, binary identities and limits.

## #1480: shared refusal for unsupported binder domains

The original-issue policy selects early rejection for these existing engine
restrictions, rather than adding execution support. Checked-source admission
shares the range rule with every command in the
[command scope](#which-commands-refuse) below:

| Previously check-ok form | New boundary | Preserved sibling |
|---|---|---|
| Init statement `forall` with a free state read in either integer-range endpoint | Existing compile-time-constant diagnostic at the owning `forall` statement, including empty bodies and unreachable branches | Supported init RHS expressions and lexical locals; ranges elsewhere follow the [range admission](#1480-checked-range-admission) |
| Expression, action-statement or leadsTo-wrapper typed binder over a named Struct | Existing finite-scalar diagnostic through shared binder validation at the owning statement, expression or `leadsTo` | Bool, named Enum/Range binders, ordinary Struct values and collection-of-Struct binders |

When the range rule finds a refusal, existing init ownership and field-read
checks run first. Their earlier diagnostics retain their message and location;
in particular, a read of a partially initialized Struct is not replaced with a
range-domain error. Bare `build_model` does not impose this checked-source range
rule. Raw Runtime constructors retain their existing unlocated range refusal.
Expression and action Typed-Struct validation remains in Core construction.
An init-statement Typed-Struct binder was already refused by the ownership
preflight: retain its complete unlocated finite-scalar error, without a new
Core prefix or location. Only that statement-domain check is deferred;
expressions inside init remain subject to the normal binder validation.
Component range diagnostics retain the original checked-source provenance.

These are **breaking check-admission removals**. No Struct-domain enumeration
or new public Kernel form is added. Existing nested-init restrictions remain
separate. The own-tree sweep is in [corpus transitions](#corpus-transitions): 8 `ok`→`error`
forms are typed binders over a Struct and 16 are init statement `forall` bounds reading
state.

## #1480: checked range admission

Status: accepted by the maintainer's decision on #1480: `check` refuses every
integer range bound the symbolic engines cannot execute, with one rule.

**Rule and owner.** `RangePlan::is_executable` in `fsl_core::binder_range` is the only
eligibility question about a bound; `KernelModel::plan_integer_range` remains the only
grammar. The engines' `parameter_range_values` answers it from the current evaluation.
`fsl_core::check_range_admission` answers it statically, called from
`rust/fslc/src/checked_build.rs` after `check_init_statement_domains`. It reuses the
validation walk (`validate_binder`) with a `RangeRule` on the phase, so scopes,
pattern bindings, spans and component wrappers are those of Core validation, in the
same order: init, actions, invariants, transitions, reachable, leadsTo, terminal.
One rule is stricter and separate: `check_init_statement_domains` refuses any state
read in the bounds of an init **statement** `forall`, including a state already
assigned, at the owning statement (the free-state rule in `model.rs`).

**What a scope establishes.** A state carrier prerequisite holds in `invariant`,
`trans`, `unless`, action guards, `let` right-hand sides, `if` conditions and
statements, where the engines certify the read frame; not in `ensures`, `reachable`,
`terminal`, `until`, or leadsTo P, Q, `decreases` and `helpful`. A lexical prerequisite
holds only for enumerated values: action parameters, range and named-integer binders,
and binders over a Set or Seq literal of any length whose items are solver literals
(integer literals, constants, or such names, including through a `let`). `let` names
and pattern bindings carry no fact, even when bound to a literal. A zero-capacity Seq
has no candidates, and other state Seq elements carry no fact.

**State Sets.** A binder over a state Set of a finite integer element type, or over
`old(xs)` where `old` reads a frame, takes the element type as its envelope wherever
a read frame is certified. That is the same prerequisite as a state name in a bound,
and both sides read it from `binder_range::state_set_element_envelope` and
`state_set_source`: an engine grants the fact only when the frame's certificate holds
that name. In a scope with no certified frame, an element has a fact only if it stays
a solver literal, which depends on what can be written to the Set.
`state_sets_with_symbolic_keys` judges each write: it keeps the keys literal when it is
a Set literal of literal items, `.add` of a literal item on such a Set, `.remove` of
anything from one, a copy of one, or an `if` over two of them; `st.add(n)` and
`Set { n }` reading state do not. The engines keep those keys literal because
`ordered_set_candidates` orders known integer keys without solver terms.

Where a copy carries a key depends on when it runs. Init runs once and in order, so an
init copy carries what init assigned: init is a fixed point, and a state assigned a
literal reads as that literal. An action starts from a frame that
`reframe_collections` has rebuilt over the element domain, so reading a state Set there
gives literal keys whatever earlier steps wrote: `ys = xs` beside `xs = xs.add(n)`
keeps `ys` literal, and only the action's own expression can add a key. `old(xs)` reads
that same frame, so it is literal in `ensures` for any write to `xs`. A postcondition
reads only the state an action left, so init's writes do not count for it; `reachable`,
`leadsTo` and `terminal` can read the initial state, so they do. The one case the frame
does not cover is `--property` on a state that really escapes: the frame then cannot be
rebuilt, its keys stay as written, and the read refuses at run time with
`unsupported integer range execution [planner]: missing-finite-fact`.

**`old` with no frame left.** The engines read `old(...)` from one frame and have none
below it, so a second `old`, or one in a scope with no old frame, stops every engine
with `old() used without old state`. `P unless Q` lowers to a transition that wraps `P`
in `old`, so an `old` inside `P` is a second one. A range read under such an `old` is
refused at `check` with the base's message and location:
`(forall x in old(xs) { count(y in 0..x) >= 0 }) unless b` reports
`invalid model expression: 'x' is not an integer const at 7:28`. No name carries a fact
there, so a state, a parameter and a binder bound alike are refused, while a literal
bound is not.

Out of scope: an `old` with no frame left that no dependent range reads under.
`old(b) unless b`, `(forall e in old(xs) { e >= 0 }) unless b` and
`invariant W { old(count(i in 0..2) >= 0) }` pass `check`, and every engine stops with
that same message, on the base as here. That mismatch has nothing to do with range
admission, and refusing it would be a separate decision about `old`.

The engines record a captured fact for a let but never accept it here. A let holding a
collection is classified where it is bound, so a later binder cannot change what it
means. Init uses the pure `init_expression_envelope` shared with the engine's init
recipes; a condition is known only when it is a literal or a named constant, as the
engine's `init_known_condition` reads it, so a folded `z = (1 < 2)` selects no branch.
LeadsTo wrappers fold each binding's bounds with the engine's `scoped_leadsto_int`,
holding one row at a time. Past 65,536 visited bindings, complete or not
(`WRAPPER_ADMISSION_BINDINGS`), the walk stops and later binders take the planner's
interval over what is known, so admission never holds that product in memory; a binder
the planner cannot bound then has no fact and is refused. The cap is a planner detail:
it limits how much admission walks, not the language, and the engines enumerate past it.
Every visit spends the budget because an always-empty inner binder completes no binding:
`forall a in 0..1000000000000 { forall b in 1..(a*0) { not d ~> d } }` returns from
`check` in under a second, exit 0 as on the base, while bmc and induction still enumerate
the outer range and had not finished after 40 seconds, on the base and here.
A binder with no candidates makes its filter and body dead, as the engines never
evaluate them.

**A known limit of the walk.** `check` judges only the rows the walk reaches, so where an
overflowing row falls decides who reports it. With `const K = 140737488355328`,
`forall a in 65534..65536 { forall b in (a*K)..(a*K) { not d ~> d } }` is refused at
`check` with `integer overflow in multiplication`, because the walk reaches `a = 65536`.
`forall a in 0..70000` stops before that row: `check` exits 0, as on the base, and bmc and
induction exit 2 with the same message. It fails closed, with no verdict, so no limit is
added for it. `an_overflow_past_the_wrapper_walk_is_left_to_the_engines` pins both forms.

**Why they cannot drift silently.** Carrier scopes and state Set binders now share one
prerequisite and one eligibility function, so they cannot disagree on what a certified
frame establishes. The write analysis for scopes with no frame is still a static mirror
of how values stay literal; removing it would need an evaluator run over every path. It
fails closed, because a mismatch in the safe direction is a refusal, and a mismatch in
the other direction is an engine refusal rather than a verdict.
`rust/fslc/tests/issue_1480_check_engine_agreement.rs` is the gate on it: over a grid of
scope, source shape and Set write, every form `check` accepts must not reach an engine
range refusal, an overflow in a bound included. The wrapper walk is the known exception
above: its overflow past the budget lies outside the grid, and that test pins it so the
gate reports it instead of missing it.

| Previously check-ok form | New boundary | Preserved sibling |
|---|---|---|
| Unplanned dependent bound: index, field, `size()`, `if`, `min`, `/`, `%`, `old`, aggregate | Refused, message by binder site below | Closed constant folds, `count(i in (if C > 1 then 0 else 1)..n)` |
| State-dependent bound in `ensures`, `reachable`, `terminal`, `until`, leadsTo P/Q, `decreases`, `helpful` | Refused | The same bound in `invariant`, `trans`, `unless`, `requires`, RHS |
| Bound reading a `let`, pattern binding, Int state, non-literal Seq element, non-literal Set literal item, or, in a scope with no certified frame, an element of a state Set some write gives a non-literal key | Refused | Action parameters, enumerated binders, Set and Seq literal items, state Set binders in a certified frame |
| Interval that does not fit `i64` | Refused | Any representable interval; there is no size cap |
| Typed binder over `Int`, including a leadsTo wrapper. The base's `check` exited 0 and every engine refused it, so no verdict is lost | `type 'Int' is not a finite scalar`, located | Typed binders over `Bool`, named Enum and Range. A spec that declares `type Int` or `enum Int` keeps its binder over that declaration in every scope, as on the base; a `struct Int` has no domain and is refused the same way |
| A `type`, `enum` or `struct` named `Bool`, standalone or in a component. The base returned verdicts, so they are removed | `'Bool' is a reserved FSL type name and cannot be declared` at the declaration; a component reports `component "c.fsl" failed to lower (... at c.fsl:5:3)` at its `use` | `actor Bool`, `use P as Bool`, a state or const named `Bool` |

**Diagnostics.** Each form had a refusal on the named base, and keeps its text.
Aggregates use the Core validation wrapper and `static_public_int`:
`invalid model expression: 'n' is not an integer const at 8:3`, or
`invalid init statement: ...` in init. A bound `static_public_int` cannot read at all,
such as `count(i in 0..abs(n))`, reports `invalid model expression: public Kernel
requires static aggregate range bounds at 8:3`, as on the base. Quantifiers and statement `forall` use the
engine's `KernelModel::static_binder_bound` text with a location added:
`'n' is not an integer const`, `binder bound is not a static integer`,
`invalid static integer expression`. A dependent aggregate inside a bound reports the
aggregate's own refusal. LeadsTo wrappers use the same quantifier text at the
property's location. A component form gets the existing component wrapper. As in the #1480 refusals above, init ownership and field-read
checks report first.

**Breaking removals of verdicts.** The rule is static, so it also refuses forms where
an engine returned a verdict without reaching the bound. Maintainer decision: these
are refused and recorded here. They are an earlier violated property
(`invariant Q { false } reachable R { forall i in 0..n { i >= 0 } }`), selection by
`--property` or `--exclude-property` around the property, and `--depth 0` for
`ensures` or transition bounds. `decreases` and `helpful` arguments are refused
although a BMC verdict exists for them: the BMC encoding reads neither, and its ranking
prepass (`ranked_leadsto_lasso_discharges`) evaluates the measure but withdraws on any
error, so only induction reported the refusal. Base refused these aggregates at
`check`. Explicit-engine verdicts on every refused form are removed.

**Refused forms that now return a verdict.** Several forms the base refused now run,
because the engine gained what the rule assumed rather than the rule gaining a refusal:
a dependent bound in a fair guard or sticky helpful guard; a `trans` body with a partial
operation; an invariant reached by `reachable` diagnosis; a static outer binder around a
dependent inner one on explicit's vacuity lane; several-item Set literals and non-empty
state Set initializers; `--property` selecting away a bound the frame proves.

### Which commands refuse

Measured with `reachable R { forall i in 0..n { i >= 0 } }` beside
`invariant Q { n > 0 }`, which the state violates, base `5564b25cc` against this change,
each exit recorded:

| Route | Commands | Base | This change |
|---|---|---|---|
| Checked load, reports the refusal | `verify`, `testgen`, `mutate`, `sweep`, `scenarios`, `html`, `ledger`, `diff`, `refine` | 1 | 2 |
| Checked load, reports the refusal | `conformance`, `analyze`, `explain` | 0 | 2 |
| Bare load, not refused | `kernel`, `typestate`, `lint`, `migrate`, `document claims` | 0 | 0 |
| Bare load, not refused | `fmt --check` on an unformatted file | 1 | 1 |

`approval` and log and domain replay load through the checked path by reading the code,
and were not measured against a refused form. `coverage` and `causal` load a bare model,
also by reading. These new refusals are breaking removals on the same terms.

Bare `build_model` defers a dependent aggregate bound, so `kernel` and `typestate`,
which project a bare model, reach it in their projection. They report it as base did,
through `locate_unfolded_aggregate`: `invalid model expression: 'n' is not an integer
const at 1:66`, exit 2, with `loc`.

`fmt` is the one command that refuses less: it validates through bare `build_model`,
which now defers that form, so where the base exited 2 this change exits 0. It stays
that way on purpose. `fmt` rewrites text and judges nothing, adopting checked admission
would make it refuse forms the base formats, and `check` still reports the bound.

**`replay` is an exception.** It keeps loading through `load_replay_model`, including
the forms `check` refuses and the dynamic aggregate form the base refused (base exit 2,
this change exit 0), by user decision. It checks the observed prefix through the Monitor,
which evaluates each bound concretely, so a bound refused here can replay as conformant.
Replay reports what happened on that trace; it does not admit the specification.

**Out of this rule.** Bare `build_model` and raw Runtime constructors keep their
boundaries, so `[planner]` and `[eligibility]` remain reachable there. Eligibility that
depends on solver facts stays dynamic: `checked_read_carriers` and the induction
`deferred` refusal in `bmc.rs` and `induction.rs`. Selecting properties does not assume
an unselected bound, and what a run with `--property` reports when the state really
escapes depends on the engine, measured on `n = n + 1` from the top of `0..3`:

- bmc refuses only a read that needs the bound: `[eligibility]`, exit 2, or
  `[planner]: missing-finite-fact` for a binder over a state Set whose frame could not be
  rebuilt. With no such read it reports the selected property: `verified`, exit 0, or
  `violated`, exit 1.
- induction does not select bounds and reports `_bounds_n` `violated`, exit 1, as on the
  base.
- explicit reads concrete values and reports the selected property, exit 0 or 1, even
  where bmc refuses.

There is no execution cap: `DOMAIN_BOUND` bounds enumerated requirement questions before they go to
the solver, not range candidates. A candidate list too long to allocate refuses with
`unsupported integer range execution [resource]`, exit 2. Running out while Z3 builds
terms makes the `z3` bindings panic on the null Z3 returns, and `main` turns a panic in
that crate alone into `solver execution stopped [solver-panic]`, exit 2, in JSON. The
message says the bindings panicked and no verdict was reached, and carries the panic
text and location, which also stay on stderr. It does not say `[resource]` because a
binding defect panics the same way and nothing here tells them apart: memory is the
usual cause, not an established one. A panic that a caller catches and survives, as
`ranked_leadsto_lasso_discharges`'s thread does by falling back to the full lasso
search, is forgotten and cannot label a later crash; any other panic stays exit 101. An
allocation failure elsewhere in Rust or inside Z3 can still end the process with exit
134.

**Issue close note.** The issue's row 1 reads "all engines agree". The explicit engine
does not support `leadsTo`, so for that row explicit reports that refusal rather than a
verdict, on base and here alike.

### Corpus transitions

Sweep at base `5564b25ccb37e3477311fde1dae5eb6298eb6b2d` and head
`45db5cb85c153adaf14b6fb074e3ab227ddef720`, the last code commit. `fslc check <form>` ran
twice with binaries built by Rust 1.98.0, each from a `git archive` of its SHA, with no
difference between the two runs on either side and no timeout at 30 seconds. Every compared
output is whole stdout and the exit code; nothing is excluded.

The forms are 20,067, the union of both sides:

- 639 `.fsl` files: `specs/` 23, `examples/` 195, `rust/fslc/tests/fixtures/` 421. Each runs
  in each tree, the 29 head-only files copied into the base tree beside the fixtures they
  read.
- 19,428 models captured from the tests. A temporary patch in each tree, not in this
  repository, writes every source reaching `parse_document_with` or `parse_surface_spec`,
  with the components a compose root reads, while `cargo test --workspace` runs. That
  reaches every required test the branch rewrites (under `tests/`, 15 modified files, 69 added, none
  removed or renamed) and every `format!` template by its real expansion, in the base's
  version of a test and in the head's.

| Base → head | Corpus | Captured |
|---|---|---|
| Same exit and stdout | 630 | 17,702 |
| `ok`→`error` | 9 | 640 |
| `error`→`ok` | 0 | 1,079 |
| `error`→`error`, other text | 0 | 7 |
| `ok`→`ok`, other output | 0 | 0 |

Every `ok`→`error` is a breaking removal, 649 in all and none under `specs/` or `examples/`.
The corpus files are `rust/fslc/tests/fixtures/explicit_init_range_assigned.fsl` and
`explicit_init_range_unassigned.fsl`, then seven head-only files under
`issue_1480_temporal_outer/`: `const-state-collision`, `empty-static-overflow`, `free-state`,
`overflow-high`, `overflow-low`, `struct-typed`, `unbounded-typed`. By class, with the forms
on which the base's engines returned a verdict:

| Refusal | Forms | From the base's tests | bmc and induction verdict on base | explicit verdict on base |
|---|---|---|---|---|
| `'x' is not an integer const`, also inside `invalid model expression:` or a component wrapper | 495 | 5 | 3 | 150 |
| `binder bound is not a static integer` | 85 | 72 | 4 | 3 |
| Bound that overflows or does not fold | 20 | 0 | 0 | 0 |
| Init statement `forall` bound reading state | 16 | 7 | 0 | 0 |
| Declared `Bool` | 11 | 0 | 11 | 11 |
| Typed binder over the builtin `Int` | 10 | 1 | 0 | 0 |
| Typed binder over a Struct | 8 | 0 | 0 | 0 |
| Negative `within` | 4 | 1 | 0 | 0 |

The 18 forms with a bmc and induction verdict are 11 declared `Bool`, 3 with an earlier
violated property, and 4 where a state shares its name with a const and a leadsTo wrapper
bound reads that name, as in `const n = 1 state { n: 0..2 } leadsTo L { forall i in 0..n
{ ... } }`: the base read the const and reported `violated`, and that bound now reads a state,
so it is refused.

The 7 `error`→`error` forms all come from tests only the head has. Five report `public Kernel
requires static aggregate range bounds` where the base named `'n'`, because the other endpoint
cannot be folded either (`count(i in n..count(j in 0..0))`, `count(i in n..(1/0))`). Two report
`init references state variable 'n' before it is assigned` where the base said `invalid init
statement: 'n' is not an integer const` (`init { q = count(i in 0..n) == 1 n = 0 }`).

What this does not reach: a model built in Rust without parsing; `parse_refinement` mappings,
which `check` does not read (at an earlier head, `map total = count(i in lo..hi)` went from an
error to accepted); two tests `#[ignore]`d on both sides as one-time captures; and the remainder of
`edition_dependency_routes` and `kernel_contract::native_cli_kernel_v2_output_matches_the_origin_golden_contract`,
which stop at their first failed assertion in the patched trees on both sides and pass on this
tree. Of the 64 `format!` sites naming a model in the 24 test files an earlier sweep could not
expand, 62 have a captured expansion, and the other 2 are assertion messages
(`issue_1480_bool_domains.rs:184`, `support/issue_1480_dynamic_range_support.rs:181`).
