<!-- SPDX-License-Identifier: Apache-2.0 -->

# Typed collection expression materialization (Refs #1467)

Status: Accepted bounded repair; integration verification is separate.

## Decision and invariant

Symbolic evaluation must use the checked model's inferred or expected type when
materializing admitted scalar/enum-element Set and Seq expressions. This repairs
existing expression semantics; it adds no grammar, serialized Public Kernel,
CLI envelope, dialect or LSP construct. Declaration/storage restrictions are not
a blanket rejection of composite expression literals: some such expressions
already pass `check`, independently of their symbolic implementation coverage.

`KernelModel::expression_type` and `equality_operand_type` reuse shared core
inference with the current lexical types. The verifier has one materialization
boundary for inferred and explicit contexts. Receivers, equality, binders and
`let` must not depend on whether a literal has already acquired a runtime tag.
Conditional context reaches both arms before their values are constructed;
equality obtains missing context from the opposite operand, including an empty
left literal or conditional. Operand/arm order must not lose this context.

Private `TypedBinding`/`TypedBindings` pair each lexical value with its checked
type, including `none`, shadowing and raw composite values. Reading or iterating
bindings still exposes the original values, not coerced replacements. Preserve
the public Rust alias `Bindings<T> = BTreeMap<String, SymbolicValue<T>>` and its
root export; the private metadata owner is not a new public bindings contract.

## Preserved boundaries and remaining work

Materializing an inferred `Int` temporary retains members, slots and Seq length;
it does not intersect values with a target carrier or silently drop overflow.
Finite whole-value coercion still uses equal carriers and equal Seq capacity;
equal-range alias retagging preserves the value. Storage/model-local bounds
remain responsible for reporting violations. The [carrier decision](DESIGN-finite-container-carriers.md)
did not itself add literal materialization, and its historical breaking-removal
evidence is unchanged by this repair.

This is not complete composite or relation-valued temporary support. Existing
deciding composite Seq paths are preserved, not replaced with scalar-only
rejection. In particular, the symbolic Set<Option> literal-binder limitation
remains an implementation gap, not a new check-time rule or a permanent engine
exception. Remaining admitted composite/relation gaps stay tracked by #1467;
this repair does not close that issue.

## Regression anchors and historical acceptance evidence

The native CLI tests [typed literals](../../rust/fslc/tests/issue_1467_typed_literals.rs),
[empty context](../../rust/fslc/tests/issue_1467_empty_context.rs),
[materialization edges](../../rust/fslc/tests/issue_1467_materialization_edges.rs)
and [None bindings](../../rust/fslc/tests/issue_1467_none_bindings.rs) distinguish
rejecting invariant outcomes from tautological preservation controls. Carrier,
out-of-domain member and zero-capacity term tests remain preservation anchors.

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
