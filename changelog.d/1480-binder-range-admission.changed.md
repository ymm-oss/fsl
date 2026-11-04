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
Breaking-removal sweep: `fslc check <form>` with binaries built by Rust 1.98.0, each in its
own `git archive` tree, base `5564b25ccb37e3477311fde1dae5eb6298eb6b2d`, head
`45db5cb85c153adaf14b6fb074e3ab227ddef720`, over 20,067 forms: 639 `.fsl` files (`specs/` 23,
`examples/` 195, `rust/fslc/tests/fixtures/` 421) and 19,428 models captured while running
the workspace tests on both sides. `ok`→`error`: 649, none under `specs/` or `examples/`:
495 `'x' is not an integer const`, 85 `binder bound is not a static integer`, 20 bounds that
overflow or do not fold, 16 init statement `forall` bounds reading state, 11 declared `Bool`,
10 typed binders over the builtin `Int`, 8 over a Struct, 4 negative `within`. Corpus files:
`explicit_init_range_assigned.fsl`, `explicit_init_range_unassigned.fsl`, and seven head-only
files under `issue_1480_temporal_outer/`.

Forms the engines used to refuse now run where the rule says they can: a dependent
bound in a fair guard, in a sticky helpful guard, in a `trans` body with a partial
operation, under a static outer binder, and in an invariant beside a failing
`reachable`; binders over several-item Set literals and over state Sets with non-empty
or written values; and a run with `--property` whose read frame proves the unselected
bound. A frame that cannot prove it still refuses.

`Bool` is now a reserved type name. Breaking removal: `enum Bool`, `type Bool` and
`struct Bool` are refused at the declaration, standalone or in a composed component,
where a typed binder and an expression used to read the name differently. Breaking
removal: a typed binder over the builtin `Int`, including a `leadsTo` wrapper, is refused
at `check` with `type 'Int' is not a finite scalar`, where every engine already refused
it. A spec that declares `type Int` or `enum Int` keeps its binder in every scope.

With `--property` on state that really escapes, bmc refuses a read that needs the bound
with `[eligibility]`, and a binder over a state Set whose frame could not be rebuilt with
`[planner]: missing-finite-fact`; induction reports the escape, and explicit reports the
selected property.

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
