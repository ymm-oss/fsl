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
  rules remain in force. The diagnostic-order exceptions below preserve exit
  and kind, but change the first message and location for two already-invalid forms.
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

The argument-free-name versus body-binder check remains in place; its first
diagnostic can change for the pattern-bound argument case a01 below.
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

Earlier Kernel v1/v2 response equality is not positive compose-lowering evidence:
valid compose inputs are rejected by both exporters because they cannot preserve
component source filenames after lowering (exit 2). Identical export errors do
not demonstrate preservation of the lowered model. That interpretation of the
Kernel comparison is withdrawn; the semantic evidence below replaces it.

## Compose semantic preservation evidence

The built implementation `dd7b47885680076412ab2f0b46ebe8854ff6758b` was compared
with base `11f01225e244dabde0f8dfd9aaa665bc47dccfd4`, using the same commands and
sources in each materialized tree. All 597 corpus forms have identical `check`
stdout/stderr bytes and exits, with zero ok-to-error transitions on this base.

All 20 corpus compose declarations and two additional refactor controls were
compared with `check`, `analyze`, and
`verify --engine bmc --depth 4 --no-cache`. Of the corpus inputs, 13 check/analyze
successfully; BMC reports 12 bounded `verified` results and one `reachable_failed`.
The other seven inputs reject on both sides. Both refactor controls check and
analyze successfully and return bounded `verified` results. These are real model
and verification paths, not the unsupported public-Kernel export path.

Across the 22 inputs and three commands, 51 output pairs agree in all bytes;
the 15 successful-model BMC pairs differ only in observed elapsed-duration
numbers. There are no other byte, stderr or exit differences. The 253 excluded
numeric occurrences are live `/cost/elapsed_s`, `/cost/solver/check_elapsed_s`
and `/cost/properties/<index>/elapsed_s` values. Every excluded key exists on
both sides with finite, nonnegative values. Two same-base repeats over all 66
pairs confirm those timing differences; only those numeric token spans are
masked, preserving all other bytes. No verdict, location, assurance, solver
counter, memory value or trace field is excluded. BMC agreement at depth 4 is
bounded evidence and does not close #1528.

## Diagnostic-order exceptions

- **v01 — invalid parameter range and action body:** if both contain undefined
  predicate calls, both versions reject with exit 2 and kind `name`, but the
  parameter-range error can become the action-body error. For the documented
  `bad1()` / `bad2()` example this changes `undefined predicate 'bad1' at 4:20`
  to `undefined predicate 'bad2' at 4:37`. Restoring the old order is deferred.
- **a01 — pattern-bound argument colliding with a def-body binder:** both
  versions reject with exit 2 and kind `semantics`. The old conservative
  argument-name collector reported `would capture variable 'v'` at the call;
  the pattern-aware collector can instead reach the ordinary pattern-shadowing
  diagnostic. For the example whose argument is `(o is some(v)) and v == 1`
  and whose def body binds `v`, the first location changes from 6:27 to 6:16.
  Exact legacy diagnostic restoration is deferred. Renaming the argument's
  pattern binding to `w` is accepted by both versions.

These are explicitly documented diagnostic changes, not new acceptances. The
positions above refer to the corresponding example layouts, not every input.

## Diagnostics and display

Expansion errors emitted at a predicate call carry its source line and column;
later model errors use their ordinary model-checking locations, as in a01.
After successful
expansion, proof and counterexample output continues to name the surrounding
action, invariant, transition, or reachable declaration. The frontend does
not add internal action/property names, and no `__def...` identifier can reach
the model or output. Retaining predicate-call provenance for richer formula
pretty-printing is possible future work; it is not part of the JSON contract.

The flagship example is [`examples/named_predicate.fsl`](../../examples/named_predicate.fsl).
