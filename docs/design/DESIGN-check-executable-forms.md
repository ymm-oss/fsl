<!-- SPDX-License-Identifier: Apache-2.0 -->

# Checked executable-form boundaries (#1463)

Status: Accepted for #1463, with separate follow-ups in #1480, #1481, #1459 and #1467.

## Decision and breaking removals

The native checked-model builder owns three early rejections, shared by check,
every verify engine, LSP diagnostics, and Worker preflight:

| Previously check-ok form | New boundary | Preserved sibling |
|---|---|---|
| A leadsTo wrapper with a Set/Seq collection binder, or any where filter | Reject with the authored property location | Supported unfiltered typed/range wrappers and quantifiers inside P/Q |
| An init statement forall whose collection source contains a free state reference | Reject with the offending ForAll location, even in dead branches or with empty domains/bodies | State-independent literal sources and ordinary assigned-state reads outside the source |
| Relation indexing, whether a read or indexed assignment | Reject through the shared index-type projection | Map indexing and relation contains/add/remove/domain/range |

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
Root declarations are not rebound to a use, except that a root-authored sync
argument substituted into a component item currently reports errors at that
component's root use with the consuming item's component cause location (a known
limitation). A second component is not mistaken for the first. These diagnostic-only
targets neither replace action trace origins nor appear in Public Kernel provenance
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
the symbolic engines, but explicit rejects it. Existing auto routing sends
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

\#1480 owns static/dynamic range bounds, negative within, and typed-domain
enumeration; #1481 owns Map Struct per-field init coverage and zero-capacity
head; #1459 owns operand/method/relation-builtin typing gaps. Literal shape
relatives remain #1467. Their current acceptance/verdicts are preservation
controls, not additional restrictions or implementation claims in #1463.
