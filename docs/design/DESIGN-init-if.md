# Init If Design

`init` supports statement-level `if` with the same branch shape as action bodies.
The verifier lowers it to path-conditional initial-state constraints instead of
introducing a separate transition.

For:

```fsl
init { if C { S1 } else { S2 } }
```

the BMC evaluates `C` over the initial symbolic state `s0` and the current binder
environment. It collects constraints from the then branch and adds each as
`Implies(C, constraint)`. If an else branch exists, it collects those constraints
and adds each as `Implies(Not(C), constraint)`.

Nested `if` and `forall ... where` compose by nesting the same implication
wrappers. The concrete monitor mirrors this semantics by evaluating the condition
over the current initial valuation and applying assignments from only the taken
branch.
