<!-- SPDX-License-Identifier: Apache-2.0 -->

# Typed collection expression materialization (Refs #1467)

Status: Accepted bounded repair; integration verification is separate.

## Decision and invariant

Symbolic evaluation must use the checked model's inferred or expected type when
materializing already-admitted recursive Set and Seq expressions: Int, Bool,
Enum, Struct, Option (including Option<Enum> and nested absence), nested Set/Seq,
Set-of-Seq, Seq-of-Set, relation and existing Map values. This repairs expression
semantics, not admission: declaration/storage restrictions remain unchanged.
It adds no grammar, enum payloads, Map literal/storage syntax, serialized Public
Kernel, CLI envelope, dialect or LSP construct. Map members use existing values,
including conditional selections of state Maps, not a new constructor form.

`KernelModel::expression_type` and `equality_operand_type` reuse shared core
inference with the current lexical types. The verifier has one materialization
boundary for inferred and explicit contexts. Receivers, equality, binders and
`let` must not depend on whether a literal has already acquired a runtime tag.
Conditional context reaches both arms before their values are constructed;
equality obtains missing context from the opposite operand, including an empty
left literal or conditional. Operand/arm order must not lose this context.
Expected context descends through nested items, `some`, `none` and empty
collections before evaluation, rather than repairing a failed untyped value
afterward. Methods, chained results and quantifier/aggregate binders share it.

Private `TypedBinding`/`TypedBindings` pair each lexical value with its checked
type, including `none`, shadowing and raw composite values. Reading or iterating
bindings still exposes the original values, not coerced replacements. Preserve
the public Rust alias `Bindings<T> = BTreeMap<String, SymbolicValue<T>>` and its
root export; the private metadata owner is not a new public bindings contract.

**Breaking public Rust library API:** `SymbolicValue::Collection.entries`
changes from `Vec<(Vec<T>, T)>` to `Vec<(Vec<SymbolicValue<T>>, T)>`; each key
component is now a typed recursive value, not a raw solver term. Rust callers
constructing or consuming this public field must materialize recursive keys or
adapt their constructors/accesses. The crate-private `CollectionEntries` alias
does not make the public field change private. Only the exact `Bindings` alias,
its root export and the existing `SymbolicValue` variant census are preserved;
this is not a promise that all public Rust APIs are unchanged.

## Preserved boundaries and remaining work

Materializing an inferred `Int` temporary retains members, slots and Seq length;
it does not intersect values with a target carrier or silently drop overflow.
Finite whole-value coercion still uses equal carriers and equal Seq capacity;
equal-range alias retagging preserves the value. Storage/model-local bounds
remain responsible for reporting violations. The [carrier decision](DESIGN-finite-container-carriers.md)
did not itself add literal materialization, and its historical breaking-removal
evidence is unchanged by this repair.

One recursive value owner supplies logical equality, membership, distinctness,
conditional joins and deterministic live-value ordering. Set construction/add
mask logical duplicates; removal clears matching members; equality compares
membership in both directions, not candidate-vector shape. Seq retains order,
duplicates and live length; only its live prefix contributes to equality/order.
Option presence masks hidden payloads, so `none`, `some(none)` and
`some(some(v))` remain distinct where their checked nested type permits them.

Internal binder ordering follows concrete recursive value order: valid enums
use nominal name/member spelling, Structs nominal identity and field-name order,
Maps sorted keys and recursive values, Seq its live lexicographic prefix, and
Sets/relations sorted distinct live members/pairs (shorter equal prefixes first).
Inactive padding and hidden absent payloads do not participate. This adds no
source-level ordering operator. Relation ordering uses candidate views, never
reorders the fixed declared pair-grid prefix used by closure and transitions.
Alias retagging preserves support; recursive bounds/projection use the target
element/endpoint context, not stale inferred-Int key tags. Outside-carrier Int
temporaries and overflow remain visible until the applicable bound reports them;
construction must not clip values to a declared universe or Seq capacity.

Presence/live-slot bounds, zero-capacity Seq behavior and reached partial-operation
boundaries are unchanged. Existing scalar/enum and deciding composite Seq paths
remain preservation requirements. Invalid enum ordinals, including negative ones,
retain their existing raw-Int counterexample projection (#1283). Existing typed
JSON encoding (including nested Option distinctions), errors, locations, exits
and assurance classes are unchanged; the Rust field break is not a CLI/Kernel
schema break. Solver-free runtime/BFS stays independent of symbolic evaluation;
engine-specific unsupported/budget boundaries still apply. This contract is not
an all-specs proof, an undecided-to-proved guarantee or integration acceptance.

## Regression anchors and historical acceptance evidence

The native CLI tests [typed literals](../../rust/fslc/tests/issue_1467_typed_literals.rs),
[empty context](../../rust/fslc/tests/issue_1467_empty_context.rs),
[materialization edges](../../rust/fslc/tests/issue_1467_materialization_edges.rs)
and [None bindings](../../rust/fslc/tests/issue_1467_none_bindings.rs) distinguish
rejecting invariant outcomes from tautological preservation controls. Carrier,
out-of-domain member and zero-capacity term tests remain preservation anchors.
The [recursive values](../../rust/fslc/tests/issue_1467_composite_values.rs) and
[recursive order](../../rust/fslc/tests/issue_1467_composite_order.rs) suites cover
the extended contract; their fixture verdicts do not promote public assurance.

No acceptance gate or shared inference rule is rewritten by this repair. The
historical own-tree command was `FSLC_CACHE=off <binary-for-SHA> check <form>`,
using each SHA's separately built binary and that SHA's own materialized cwd:

| Source SHA | Corpus forms | Rewritten-test inline forms |
|---|---:|---:|
| `a52bef4931adc67ea2b1fdca827e2318b4ae25ab` | 606 | 3 |
| `e201c0d66834ed8c860be84e96876690487b11af` | 606 | 3 |
| `cc7a538c3b647c1c3bed5bf2088551806442c794` | 606 | 3 |

Corpus scope was every tracked `.fsl` under `specs/`, `examples/` and
`rust/fslc/tests/fixtures/`. Three unique inline FSL inputs cover the six rewritten
zero-capacity term tests; the other rewritten test is a Rust-signature census,
not an FSL fixture source. Both a52->cc7 and e201->cc7 measured empty ok->error
sets for corpus and inline forms, with no corpus path additions/removals. These
are acceptance observations, not 729-form engine agreement, product PASS or
validation of later `b8dc1fdb46615fc3b40238af8f495d7a80e5da4f` integration.

### Repeated native packet comparisons

Identical CI BMC and induction packets differed only in
`cost.solver.memory_mb` (18.12/18.02 and 17.91/17.92 MB). This is native solver
allocation telemetry, so recursive-value tests validate its presence, numeric
type and finite nonnegative value for queried native proof packets, then compare
the remaining packet in full. Existing measured wall-clock exclusions remain.
Solver counters, verdicts, assurance, locations, traces, versions and nested user
fields remain exact. Explicit/no-solver and zero-check memory values are not
excluded. Controls reject missing telemetry on either or both sides and changes
to every retained evidence class; complete original packets are retained.
The bool-closure numeric-control BMC repeats likewise differed only at
`memory_mb` after wall-clock normalization (17.68/17.67 MB). Its comparator uses
the same validation for native-Z3, positive-check, `verified`/`bounded` BMC
packets with depth and checked depth 2; all other memory stays exact, including
`unknown_cti`. The reason is the observed allocation telemetry, not a semantic
or solver-counter exclusion; this does not establish later product acceptance.
