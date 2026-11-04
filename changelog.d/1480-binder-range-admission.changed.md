Breaking check-admission removal: `check`, every `verify` engine, LSP and Worker now
refuse every integer range bound the symbolic engines cannot execute, with a located
`semantics` error, exit 2. Refused: unplanned dependent bounds (index, field, `size()`,
`if`, `min`, `/`, `%`, `old`, aggregates); state-dependent bounds in `ensures`,
`reachable`, `terminal`, `until`, leadsTo P/Q, `decreases` and `helpful` arguments;
bounds reading a `let` (even literal), pattern binding, Int state, non-literal Seq
element or non-literal Set literal item; in a scope with no certified read frame,
bounds reading an element of a state Set that some write can give a key read from
state; a dependent range under an `old(...)` with no frame left, which includes an `old`
inside `P` of `P unless Q`; and intervals that do not fit `i64`. Forms that reported a verdict before
reaching the bound lose it: an earlier violated property, `--property` or
`--exclude-property`, `--depth 0`, and explicit-engine verdicts on these forms.
No size cap is added. `conformance`, `analyze`, `explain`, `diff`, `refine`, `testgen`,
`mutate`, `sweep`, `scenarios`, `html` and `ledger` refuse these forms too. `kernel`, `typestate`,
`lint`, `migrate`, `fmt` and `document claims` do not, and `replay` keeps loading them
by design, including a dynamic aggregate form it used to refuse. See
`DESIGN-check-executable-forms.md`.
[#1480 final sweep: the measured base/head form count and `ok`→`error` list are recorded here after the final sweep.]

Forms the engines used to refuse now run where the rule says they can: a dependent
bound in a fair guard, in a sticky helpful guard, in a `trans` body with a partial
operation, under a static outer binder, and in an invariant beside a failing
`reachable`; binders over several-item Set literals and over state Sets with non-empty
or written values; and a run with `--property` whose read frame proves the unselected
bound. A frame that cannot prove it still refuses.

`Bool` is now a reserved type name. Breaking removal: `enum Bool`, `type Bool` and
`struct Bool` are refused at the declaration, where a typed binder and an expression
used to read the name differently. A typed binder over `Int`, including a `leadsTo`
wrapper, is refused at `check` with `type 'Int' is not a finite scalar`.

A form that runs out of memory enumerating a range now refuses with a `[resource]` error,
exit 2, instead of a panic with empty stdout. A panic in the Z3 bindings, which is what
running out while building solver terms produces, refuses with a `[solver-panic]` error
carrying the panic text, exit 2; it says no verdict was reached and does not claim a
cause. No size limit is added.

Reject two previously check-admitted forms before engine dispatch: init statement
`forall` integer-range bounds with free state reads, and typed binders over a named
Struct in an expression, action or `leadsTo` wrapper, which engines already refuse as
non-scalar. Diagnostics retain owning source locations. Supported init RHS reads,
Bool, Enum and Range binders and ordinary and collection Struct values are unchanged.
Range admission runs after existing init ownership and read checks, preserving their
earlier diagnostics and component provenance. Already-refused init-statement typed
Struct domains retain their complete unlocated finite-scalar error.

Static binder ranges share successful checked constant folding across aggregate
normalization, symbolic verification and static leadsTo and Monitor bindings, including
Euclidean division and remainder, `min`, `max`, `abs` and typed conditionals with
selected-arm arithmetic. `KernelModel::constant_integer` is a new Rust API that reads
the current constant map without caching. A plain `0..n` over a state that shares its
name with a const now reads the state: it used to prove what the explicit engine
violated, and now every engine reports `violated`.

Builtin `Bool` binders keep their `false`, `true` identity in quantifiers, statement
`forall` coverage and static `leadsTo` wrappers. Breaking admission removal: negative
constant `within` deadlines fail at the owning property before engine dispatch; zero
and positive deadlines are unchanged. Property-keyed JSON and bounded unranked liveness
assurance are unchanged.

Add the Rust-only integer range planning API under `fsl_core::binder_range` and
`KernelModel::plan_integer_range`. Plans keep name provenance and prerequisite ownership,
and full `i64` cardinality uses `u128` without enumerating candidates.

Unfiltered outer `leadsTo` range binders can use already bound wrapper locals with
checked integer arithmetic and typed Boolean conditions, keeping declared binding types,
expansion order and diagnostics. Free state bounds are refused even when a const shares
the name. `within` remains a nonnegative declaration constant; collection and filtered
wrappers and explicit liveness remain unsupported.
