Changed (#1463, breaking): check now rejects leadsTo collection/where wrapper
binders, init statement forall collection sources with free state references,
and relation index reads/writes, with shared core diagnostics and authored
locations across native CLI, LSP and Worker. These forms previously checked
without executable semantics. Written aggregate binders, filters and values
are checked before Count/Sum normalization, so empty expansion cannot hide
relation indexing or existing type errors. This intentionally also removes
Bool indexing inside empty literal-range/zero-capacity Seq aggregates
(the #1463 decision to check aggregate source types regardless of candidate count;
a #1459-class closure; no new type rules are added).
Empty Count/Sum now reports every existing type error in the written expressions,
such as the non-Bool filter in `count(x in 1..0 where x + 1)`; the 6 Bool-index
removals are examples in the measured corpus/inline inputs, not an exhaustive
list of type-error kinds.
Raw validation does not expand dependent inner Count/Sum before outer
substitution or prematurely reject a pattern name awaiting substitution;
existing nested and pattern positive/negative controls retain acceptance.
Map indexing, relation methods, state-independent
init literals and ordinary assigned-state reads remain supported. Nested init
forall remains an explicit limitation routed by auto to symbolic BMC.

Separately fixed the false symbolic/explicit rejection of local binder names
that shadow state, without narrowing check acceptance. Remaining range/domain
forms are #1480, init/zero-capacity issues are #1481, typing gaps are #1459,
and collection-literal shape issues remain #1467.

Public output changes are intentional: the newly forbidden forms return a
located semantics error (exit 2) before engine routing; aggregate errors retain
their authored property location instead of synthetic 1:1. Imported failures
anchor at the owning root use, retain the component filename/cause location,
and show public names such as part.xs/part.Capture, not lowered part__ names.
Wrapper messages say leadsTo wrapper rather than misleadingly bounded leadsTo.
Model-expression type errors now point to their owning declaration, including
errors whose base check envelope already had a loc; component type errors,
including pre-existing ones, anchor at the owning root use and retain their
component cause.

The base 5738cc2c711b691a9d453b0190ead1a2ff77fc50 versus the #1463 change
(exact measured head recorded in the PR) sweep ran twice in their own trees
using `FSLC_CACHE=off fslc check <form>`. Both runs exited 0: 597 corpus forms +
1016 inline IDs = 1613, with 110 accepted-to-error removals (1 corpus, 109 inline),
identical complete reports and no excluded fields. These are 22 wrapper,
31 init-source, 51 relation-index (18 aggregate), and 6 empty-aggregate Bool-index
forms. The complete fresh-base sweep includes both added legacy-return fixtures
without a difference. All 66 nested-aggregate/pattern check controls preserve
identical accepted outputs; its 15 removals are four quantifier relation-index
bounds and eleven sync-expanded/root relation-index forms. Complete removal IDs:

| Removed form ID set | Count |
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

Of 128 full-output differences, the remaining 18 stay error/semantics, exit 2:
`inline/1457/boundary/nonfinite` gains authored message/loc 1:161;
`inline/R2/seq-write-remains-rejected` keeps loc 7:17 but requires Map, not Map
or Relation; `inline/aggregate-and-component-locations/case-{13,17,37,41,49,53,61,65}` moves 1:1 to 6:3
and changes Map/Relation/Seq repair text to Map/Seq;
`inline/aggregate-and-component-locations/case-{21,45,57,69}` changes only that text, retaining 6:3.
`inline/nested-aggregates-and-sync-ownership/case-{72,74}` already failed static aggregate bounds; the relation
index now reports property 6:3 instead of no location;
`inline/nested-aggregates-and-sync-ownership/case-{73,75}` anchors the same existing failure at use 2:3
and retains the part.Capture cause at piece.fsl:6:3.
Other envelope fields remain unchanged; no output drift is excluded.
Exit-1 refinement/trace verdicts are not input errors.
