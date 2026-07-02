# `sum` Aggregate Expression — Design (#91, Phase 1 of #72)

## 1. Problem

`forall c: Case { P(c) ~> Q(c) }` liveness proofs need a `decreases` measure.
A per-entity measure (`decreases level[c]`) always fails the ranking
discipline under interleaving: the discipline requires *every* enabled
action to strictly decrease the measure, but an action that advances a
*different* entity leaves `level[c]` for the pending binding unchanged
(`rank_failure: "non_decreasing_action"` — see `docs/LANGUAGE.md` §1 and
`skills/fsl/reference.md` rule 7). The documented workaround was a
hand-written global-sum measure, `decreases level[0] + level[1] + level[2]`,
which has to be rewritten by hand every time `instances Case = N` changes.

## 2. Design: bounded-domain expansion

FSL domains are always bounded (`type T = lo..hi`, an `entity`'s
`instances` bound, or a `number`'s `values` bound). This means a "sum over a
domain" is not a new solving primitive — it is exactly the hand-written
idiom above, generalized: enumerate the binder's domain at build/eval time
and fold the instantiated bodies with `+`. No new Z3 theory, no new
transition-relation shape; `sum` is evaluated the same way `forall`/`exists`
already are (instantiate over a finite index set), just accumulated with `+`
instead of `And`/`Or`.

```fsl
leadsTo Responds {
  forall c: Case { level[c] > 0 ~> level[c] == 0 }
  decreases sum k: Case { level[k] }
}
```

Because the enumeration happens at evaluation time against the spec's actual
bound (`instances Case = N`, or a `--instances`/`--values` CLI override,
#86), the measure text never needs to change when `N` changes — this is the
concrete fix for the "rewrite the measure by hand" problem, not just a
syntax convenience.

## 3. Why a new brace-only grammar form, alongside the existing `sum(...)`

The kernel already had an aggregate spelled `sum(v: T of expr [where expr])`
(`grammar.py` `sum_e`, kernel AST tag `"sum"`, 5-tuple
`("sum", v, ty_name, body, cond)`). It is unconditionally usable in general
expressions and is left completely unchanged by this feature — it's a fine,
already-shipped way to fold a `Seq` via an `Idx` domain (see
`specs/audit_log.fsl`), and existing code across `compose.py`, `dialects.py`,
`mutate.py`, and `explain.py` pattern-matches its exact 5-tuple shape.

The new construct instead sits in the `quant` grammar rule alongside
`forall`/`exists` (`"sum" binder [":"] "{" expr "}" -> quant_sum`), reusing
the *general* `binder` production — typed (`k: T`), range (`k in lo..hi`),
and collection (`k in someSet`) binders, each with an optional `where`
clause — exactly like `forall`/`exists` already do. The old `sum(...)` form
only ever accepted a `NAME : qname` binder with no range/collection support.
Since the AST tag `"sum"` was already load-bearing with a fixed 5-tuple
shape across five modules, reusing that tag for a binder-shaped 3-tuple
would have made every existing `tag == "sum"` call site silently misparse
call sites; the new form uses its own kernel AST tag, `"quant_sum"`
(`("quant_sum", binder, body)`), instead. `count`/`sum` themselves set this
precedent: `unique`/`exactlyOne`, which are also binder-shaped cardinality
predicates, got their own tags (`"unique"`, `"exactly_one"`) rather than
overloading `"count"`.

One consequence of sitting at the `quant` grammar tier (like `forall`/
`exists`) rather than at the arithmetic `atom` tier (like `min`/`max`/the old
`sum(...)`): `sum k: T { ... }` is not directly reachable from an arithmetic
comparison without parentheses, e.g. `invariant B { (sum k: T { m[k] }) <= N
}` needs the parens that `min(a, b) <= N` would not. This mirrors
`forall`/`exists` placement exactly, as directed by the issue design; moving
it to the `atom` tier is a possible follow-up but out of scope here (it does
not affect the `decreases <expr>` use case, which places `sum` directly at
the top level with no wrapping needed).

## 4. `where`-clause semantics

`sum k: T where cond { body }` filters the fold: each domain member's
contribution is `body` when `cond` holds for that member, and `0` otherwise
— i.e. symbolically `z3.Sum(z3.If(cond_i, body_i, 0) for i in domain)`, and
concretely the Python equivalent. This reuses `values.iter_binder_terms`
exactly as `forall`/`exists`/`unique`/`exactlyOne` do (the `w` "where" term
each of those already threads through), so `sum`'s where-filtering is
implemented with the same machinery, not a parallel one. An empty domain (or
a `where` that admits no members) sums to `0`.

## 5. Type checking

The bound variable's type resolves the same way a `forall`/`exists` binder's
does (unknown type name -> `kind: "type"` error naming the type). `where`
must be Bool. The body must be Int-family (`Int`, a domain type, or an enum
member reference); a Bool body is a `kind: "type"` error — summing a Bool
silently coercing true/false to 1/0 was judged more likely to hide a typo
(`sum k: Case { flag[k] }` meaning "count of flag[k]") than to be an intended
use, so it is rejected rather than accepted as an implicit count. (`count(x:
T where expr)` remains the spelling for that.) This check runs eagerly
during `build_spec` (so `fslc check` catches it, not just `verify`) via a
generic recursive walk over the built spec's expression trees — the same
`FslError`-based error the rest of the type checker uses; there is no
separate "sum type checker" theory to keep in sync elsewhere.

## 6. Dual-evaluator obligation

This repo's core correctness invariant is that `bmc.py` (symbolic) and
`runtime.py` `Monitor` (concrete) agree on every construct. `sum` is
implemented once, in `values.py` (`eval_sum_binder`), parameterized over the
same `dom`/`ev` abstraction `eval_quant`/`eval_one`/`eval_count` already use;
`bmc.py` and `runtime.py` each add a one-line dispatch (`tag == "quant_sum"`)
plus a one-line wrapper binding `_SYM`/`eval_expr` or `_CONC`/
`eval_concrete` respectively. There is exactly one enumeration/fold
implementation, not two — the dual-evaluator agreement is structural, not a
property that has to be independently verified and kept in sync by hand.
`tests/test_sum_aggregate.py` additionally cross-checks `bmc.eval_expr` and
`runtime.eval_concrete` directly on a pinned state, following the pattern in
`tests/test_evaluator_agreement.py`.

## 7. Relation to #72 Phase 2

`sum` fully resolves the case where every enabled action moves the tracked
total in the same direction (the case in the original bug report: entities
sharing one pool of "work remaining"). It does **not** resolve the case
where some enabled action touches neither the pending binding nor the sum at
all (a bookkeeping action unrelated to any `level[c]`) — the sum then does
not strictly decrease and ranking still fails. That is Phase 2 of #72: a
fairness-aware "helpful action" discipline (`decreases M by <action>`),
which stays a separate, larger piece of work (see the design comment on
#72). Phase 1 (`sum`) ships first because it has no solver-side novelty and
directly fixes the reported instances-independence problem.

## 8. Out of scope

`count`/`min`/`max`-style *binder-shaped* aggregates (a hypothetical
`count k: T { ... }` / `min k: T { ... }` mirroring `sum`'s new binder form)
are not requested by #91 and are not added here — `count(x: T where expr)`
already exists and was not reported as having the instances-independence
problem `sum` was solving. Add them only if a concrete use case shows up.
