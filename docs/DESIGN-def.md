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

The last rule favors an explicit local repair over silently changing meaning
or leaking compiler-generated names into counterexamples.

## Diagnostics and display

Call-site errors carry the call's source line and column. After successful
expansion, proof and counterexample output continues to name the surrounding
action, invariant, transition, or reachable declaration. The frontend does
not add internal action/property names, and no `__def...` identifier can reach
the model or output. Retaining predicate-call provenance for richer formula
pretty-printing is possible future work; it is not part of the JSON contract.

The flagship example is [`examples/named_predicate.fsl`](../examples/named_predicate.fsl).
