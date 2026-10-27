# Named predicates (`def`)

Issue: #187.

## Goal and syntax

Long guards and properties can name business vocabulary without extending the
verification kernel:

```fsl
def eligible(c: Claim) = submitted[c] and amount[c] <= AUTO_LIMIT

invariant OnlyEligible "REQ-2: only eligible claims are auto-approved" {
  forall c: Claim { auto_approved[c] => eligible(c) }
}
```

`def NAME(p: Type, ...) = expr` is available at the top level of `spec`,
`requirements`, and `compose`. It is an expression-only, non-recursive
predicate; it cannot contain statements or mutate state.

## Frontend-only expansion

Immediately after the source AST is built, the frontend collects the file's
definitions, replaces each call with capture-safe parameter substitution, and
removes every `def` item. This happens before requirements/compose expansion
and before `build_spec`. Consequently model checking, induction, scenarios,
the concrete Monitor, mutation, and refinement receive exactly the same
existing expression nodes as a hand-expanded specification.

Definitions are lexical to one source file. Imported files expand their own
definitions when parsed; names are not exported through compose aliases. This
keeps the kernel and cross-file name-resolution surface unchanged.

## Rejection rules

- Duplicate definitions and duplicate parameter names are name errors.
- An unknown predicate call is a name error.
- Arity mismatch is a type error.
- Direct and mutual recursion are semantic errors, even if the definition is
  never called. Expansion therefore always terminates.
- A parameter may not be shadowed by a binder in its definition.
- If substituting a call argument would capture one of its free variables, the
  call is rejected with a source-level diagnostic asking the author to rename
  the binder. The implementation never creates a synthetic binder name.
- Breaking removal (#1450): a caller action parameter, let, expression
  quantifier/aggregate binder, or statement `forall` that would capture a free
  name in the expanded def body now rejects at the call site with exit 2,
  `kind: "semantics"`, and `would capture variable`. The def's own parameters
  are excluded; rename the caller binding. Existing pattern/global collision
  refusals remain unchanged.
  Non-global pattern bindings exported from `requires` or a binary left operand
  also count as caller bindings and now reject calls that capture def free names.

The capture rules favor an explicit local repair over silently changing meaning
or leaking compiler-generated names into counterexamples.

**Exception / remaining soundness risk (Refs #1528):** the binders declared
directly by a `leadsTo` property are not threaded into this def capture check for
before/after expressions, `helpful` arguments, `decreases`, or `within`.
They can still capture a def-body free name and produce a false green; rename
the binder before relying on verification. Expression quantifiers and
aggregates inside the property's expressions are covered. This change does
not claim to close the declaration-level property-binder path.
The binder's own `where` expression is checked, so capture may now reject before
the existing unsupported-wrapper-where diagnostic. A capture in that `where`
does not demonstrate that the wrapper's other positions are covered.

## Caller scope and compose source names (#1450)

The existing argument-free-name versus body-binder check remains in place.
The converse check expands the body in its own parameter scope, walks its free
names with the shared ordered native walker, then checks caller-local collisions
before parameter substitution. Nested def calls therefore expose transitive
free names without mistaking caller arguments for def globals. Scope entry and
exit are paired within the temporary expander; no extra state reaches the Kernel.
Action guards see preceding guards, while statements and ensures see the final
guard scope. Binder domains use the outer scope; filters and bodies see their
binder. Sync actuals use header scope only, not additional lets or component locals.
The shared free-name walker uses the same ordered binary pattern continuation
as source rewriting: a def body's own `is some(v)` binds later occurrences in
that body rather than inventing a free name. Existing caller/global pattern
collisions still reach the ordinary pattern checker with the original message
and pattern position. A single mutable-set action walker grows guard scope for
expansion, component rewriting, alias resolution and sync-source inventories;
callers do not maintain separate Requires/Let growth rules.

Compose passes all Common source items, not only defs, to the expander so it can
inventory state, constants and enum members. Definition collection still selects
only Def items; this does not eagerly expand the other Common items. Components
are loaded first, and their bare enum member names supplement that inventory,
before compose-source expansion. This preserves the existing pattern-shadowing
diagnostic instead of replacing it with a new capture error for an imported enum.
Expansion still precedes prefixing, alias rewriting and sync substitution; source
definition tables remain separate. Loading components before validating compose
defs can change which error is reported first when both sources are invalid.
Action parameter ranges still expand after action items in this implementation;
if both contain undefined predicate calls, the body error can precede the
parameter-range error. Both reject with exit 2 and the same name-error kind;
this validation-order change is not repaired here.

## Initial implementation boundary evidence (#1450)

For the initial implementation, the command shapes `<base-binary> check <path>`
and `<head-binary> check <path>`
were run over all 597 `.fsl` files under `specs/`, `examples/`, and
`rust/fslc/tests/fixtures/`, in materialized source trees for base
`9af1fa5f36f91e415b7ba249888f4ada4cf7a0db` and implementation
`d806e6bc6677016dac3803ac5559d3f0b1e3da67`. The ok-to-error transition set is
empty; all check stdout/stderr bytes and exits agree. No existing required inline
fixtures were rewritten, removed, renamed or stopped. Independent inline capture
controls establish the breaking removal even though the repository corpus has
none of those newly rejected calls.

All 20 corpus files declaring compose were also compared with `check` and
`kernel --kernel-version 1` / `2`: zero differing files, no excluded fields,
with each materialized tree established as its own repository root. An initial
v2 error comparison exposed different ambient archive-root prefixes for
`semantics_compose_component_parse_failure.fsl`; two same-base controls and the
base/head comparison agree byte-for-byte after setting those repository roots,
without rewriting any output. This corpus evidence does not close #1528.

## Diagnostics and display

Call-site errors carry the call's source line and column. After successful
expansion, proof and counterexample output continues to name the surrounding
action, invariant, transition, or reachable declaration. The frontend does
not add internal action/property names, and no `__def...` identifier can reach
the model or output. Retaining predicate-call provenance for richer formula
pretty-printing is possible future work; it is not part of the JSON contract.

The flagship example is [`examples/named_predicate.fsl`](../../examples/named_predicate.fsl).
