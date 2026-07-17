# Requirements stage access

Status: accepted

## Contract

Business and requirements expressions use one structural source node:
`Expr::Stage { process: Option<SymbolPath>, entity, ... }`. The node must be
eliminated before checked Kernel construction. Concrete evaluation, symbolic
evaluation, and public Kernel export reject an unlowered node.

The resolver tracks typed parameters and binders. `stage(c)` selects every
process whose final entity type is `c`'s type. One candidate lowers to the
generated process stage-map index. Zero candidates and non-entity/unbound
arguments are type errors. Several candidates are ambiguous and list their
paths. `<path>.stage(c)` filters by the exact structural `SymbolPath`.

Process declarations accept the same path representation. An unqualified
`process Claim` retains the existing generated names. Qualified declarations,
such as `process claims.Claim`, share the entity type `Claim` while generating
path-distinct enum and state symbols.

The resolved map value supplies the expected stage enum, so ordinary Kernel
type checking validates the compared member. Requirements `terminal` remains
explicit; process sinks are not promoted to terminal states.

## Origin and presentation

Lowering binds the generated stage map to its process declaration and binds
each source stage access to its containing Kernel target. Public Kernel v2
therefore contains the lowered state symbol plus a `resolve_stage_access`
origin step and source span. Explain and violation formatting structurally
recognize generated stage-map origins and render `stage(c)` without changing
the checked expression.

Acceptance expressions use the same resolver before Monitor replay. No
dialect-specific runtime implementation or generated-name source fallback is
permitted.
