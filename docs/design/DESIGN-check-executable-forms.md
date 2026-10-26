<!-- SPDX-License-Identifier: Apache-2.0 -->

# Checked executable-form boundaries (#1463)

Status: Accepted (ANS-1463; scoped by ANS-1463b).

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

Aggregate source typing is independent of candidate count (ANS-1463agg).
Before Count/Sum normalization, the written binder, filter and value pass
the same existing binder/local-scope/expression checks as other aggregates.
An empty domain cannot erase a forbidden relation index or another already
enforced typing error. This also rejects previously accepted Bool indexing
inside an empty aggregate, an incidental closure of a #1459-class hole, not
an implementation of new operand rules. Valid empty/nonempty Map aggregates
retain their verdicts, and source errors retain the authored property location.

Imported-component diagnostics retain exact ownership through private check
targets during lowering. A late model error uses the existing component-error
policy: anchor the envelope at that component's root use declaration, retaining
the component file/position and public declaration spelling in the cause.
Root declarations are not rebound to a use, and a second component is not
mistaken for the first. These diagnostic-only targets neither replace action
trace origins nor appear in Public Kernel provenance or assurance counts.

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

Base: 666acee8ad5f40a1ca5e205419091245f5a5d6de. The required sweep runs each
named binary in its own materialized tree over all specs, examples, and CLI
fixtures, plus every input of the rewritten required tests. The original
349-pair collection inventory and its six rejected boundaries are retained;
old/new inputs are captured from actual test emission. The two rewritten
runtime/verifier guard tests retain all their literal/replace inputs, with
their new supported-constructor controls added separately.

Implementation head: 280035520fdb50a76dbfc0932223c281e5a029fb. Each binary ran
`FSLC_CACHE=off fslc check <form>` in its own Git-archive materialized tree;
tracked source blobs were verified against the named commits. The base binary
SHA256 was da93da015d205f7d31cccb941aeb452152641f707f6f7292bd09564c4e7e5e98;
the head binary SHA256 was 16786b0997da13d992d8b7082736f6e7ac85f98253fff9ca6aac7b73e8928cba.
Both complete measurements agreed without excluding any JSON field: 594 corpus
forms plus 849 inline IDs, 1443 total, with 67 accepted-to-error transitions
(1 corpus, 66 inline). A check-time refinement/trace verdict with exit 1 is not
an input rejection: acceptance here means result is not `error`, not that its
property was satisfied. All 67 transitions below are breaking removals.

This is the complete transition list using Cartesian brace expansion; counts
are fixture IDs, not independent language rules. Inline origins are the
preserved #1457 generator, the shared issue_1463_forms inventory, and the two
rewritten guard tests. Their supported-constructor inputs are also measured.

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

There were 68 full-output differences. The only non-removal was
`inline/R2/seq-write-remains-rejected`: both versions reject with exit 2,
kind semantics and location 7:17; its message now says the indexed update
requires Map, not Map or Relation. No verdict/location/exit drift is excluded.

## Explicitly separate follow-ups

#1480 owns static/dynamic range bounds, negative within, and typed-domain
enumeration; #1481 owns Map Struct per-field init coverage and zero-capacity
head; #1459 owns operand/method/relation-builtin typing gaps. Literal shape
relatives remain #1467. Their current acceptance/verdicts are preservation
controls, not additional restrictions or implementation claims in #1463.
