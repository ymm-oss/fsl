# FSL — counterexample blame assignment (false conjuncts, contributing guards/effects, vacuity cores)

Motivation: issue #170. An agent repairing from a counterexample needs *localization*, not just
the shortest trace: which conjunct of an AND-invariant is false, which guard/effect of which
step fed the violation, and — for vacuity — which assumptions contradict. Blame **identifies**;
it never proposes weakenings (repair suggestions are out of scope by the anti-hollowing rule).

## 1. Carriers

The single implementation point is the **`verify` violated payload** built in
`bmc._bmc_explore` at the invariant-violation return (the only site where the Z3 model `m`,
`states`, `choices`, and `instances` coexist). Everything else inherits for free:

- `fslc verify` — `violated` results with `violation_kind` in `{invariant, type_bound}` carry
  `blame` (top level) and per-step `blame` on trace entries.
- `fslc explain` — `mutate._oracle` calls `verify`, and `explain._counterfactuals` copies every
  non-`trace`/`spec`/`result` key into `violation` and the trace into `trace`, so counterfactual
  narratives carry `blame` with **zero explain-side changes**. The `"__" not in blob` test
  polices name leakage on this path.
- `fslc verify --engine induction` — `prove()` delegates the base case to `verify`, so base-case
  violations inherit. Step-case CTIs (`unknown_cti`) are out of scope (a CTI state is not a
  reachable counterexample; blaming its history would be fiction).

Out of scope for conjunct blame: `ensures`/`trans`/`leadsTo`/`partial_op`/`deadlock` violations
(their property is not an AND of state predicates in the same sense; extension is mechanical if
wanted later).

## 2. False-conjunct identification (`blame.conjuncts`)

An invariant is `{name, expr, implicit, loc, meta}` with `expr` a tuple AST; conjunctions are
left-nested `("bin","and",l,r)`, possibly under `("forall", binder, body)` prefixes.

- `_split_conjuncts(expr)` (new, bmc.py): recursive descent through `forall` and `and` **only**
  (forall distributes over and; `exists`/`=>` stop the descent), accumulating the binder prefix
  per leaf and re-wrapping each leaf `("forall", b, leaf)`. Result: a list of closed conjunct
  ASTs in source order. A conjunct-free invariant yields a 1-element list (uniform shape).
- `_conjunct_blame(model, inv, state, spec, expr_cache)` (new): per conjunct,
  `eval_expr(conj, state, {}, spec)` then `model.eval(cond, model_completion=True)` → `holds`.
  For false conjuncts, reuse `violating_bindings(model, conj, state, spec)` unchanged — it
  already descends `forall`/`and` and emits public bindings via `_public_model_bindings`.
  Text is rendered from the AST (see §5), never by source slicing — works under compose.
  **Exception**: an `implicit` (`type_bound`) invariant is never split/rendered this way — its
  `expr` can embed synthetic internal names (a Seq's `<var>__data`/`<var>__len` phys vars, a
  Map's `__k` binder) with no `display_names` entry. `_conjunct_blame` treats it as a single
  opaque, already-known-false conjunct (`"{target var} stays within its declared type bounds"`),
  matching how `explain._auto_checks` already treats these invariants (target name only, never
  the body). Found by `tests/test_robustness.py`'s corpus-wide no-`__` sweep while implementing.
- Cost: model evaluations only, no solver calls.

```json
"blame": {"conjuncts": [
  {"index": 0, "text": "forall o: OrderId: shipped.contains(o) => orders[o].status == Shipped",
   "holds": false, "violating_bindings": [{"o": 1}]},
  {"index": 1, "text": "revenue >= 0", "holds": true}
]}
```

## 3. Per-step guard/effect attribution (trace-entry `blame`)

**Honest scope.** A per-statement unsat core over the step relation is not practical here: the
transition is one monolithic conjunction built by `transition()`/`compute_updates`, and
restructuring it into per-statement tracked assertions would fork the Z3 encoding — exactly the
dual-evaluator (bmc vs `runtime.Monitor`) divergence risk the repo guards against. Instead:
a **dynamic backward slice** over the concrete trace (path-resolved through the model), plus a
**model-substitution counterfactual test** at the failing step. Both are model evaluations; the
solve is untouched.

`_blame_trace_steps(model, states, choices, instances, spec, expr_cache, t, false_conjs)` (new):

1. Read set `R` := union of `_referenced_state_vars(conj, spec)` over the false conjuncts.
2. For `k = t .. 1`: the executed instance is `instances[model.eval(choices[k-1])]` (same lookup
   as `_build_trace`); binds incl. lets via `_eval_requires`. Resolve the executed statement path
   by model-evaluating each `("if", cond, …)` condition at `states[k-1]`.
   - **effects** — `assign` statements on the executed path whose `_lvalue_base_name` root is in
     `R`. Each contributes `{target, text, loc}`; then `R ∪= _referenced_state_vars(rhs)` and
     the reads of the enclosing path conditions (standard slicing closure).
   - **guards** — the step's `requires` clauses whose expr reads any var in the updated `R`:
     the guards that admitted the state feeding the blamed writes. `{text, loc}` each.
Every action-bearing trace entry (`k >= 1`) gets `blame` (possibly with empty lists), decorated
on the violated payload's trace only — witness/scenario traces are untouched:

```json
{"step": 2, "action": {"name": "ship", "params": {"o": 1}}, "changes": {...},
 "blame": {"guards":  [{"text": "orders[o].status == Paid", "loc": {"line": 35}}],
           "effects": [{"target": "shipped", "text": "shipped = shipped.add(o)",
                        "loc": {"line": 37}}]}}
```

**Implemented scope note**: the "necessity" refinement described in an earlier draft of this
design (a `z3.substitute` counterfactual per blamed variable at the failing step, marking an
effect `"necessary": true` when undoing it alone would repair the conjunct) shipped as a
**deferred follow-up**, not part of this cut. The guard/effect backward slice above is the
implemented mechanism and already satisfies the issue's acceptance criterion ("反例トレースの各
ステップに、関与したguard/effectの識別子が付与される") without it — adding a possibly-wrong
"necessary" flag was judged higher-risk-for-marginal-value than shipping the slice alone; a
future PR can add it as a strictly additive field.

## 4. Vacuity: naming the contradicting assumptions

The unsat-core machinery already exists and is reused, not reinvented:
`_diagnose_unreached_reachable` (tracked invariants → blocking core) and
`_diagnose_action_coverage` + `_minimize_requires_core` (tracked requires → `blocking_requires`).
Never-enabled actions therefore already name their contradictory requires; the gap is on the
vacuity findings themselves. `_finalize_vacuity_findings` gains a `source_lines` param (`verify`
passes it through); findings gain additive fields:

- **`vacuous_implication` / `vacuous_leadsto`** (implemented) — extract the core of
  `_diagnose_unreached_reachable` into `_diagnose_unreachable_expr(expr, spec, depth,
  source_lines, tag, exclude_name)` and call it on the pending item's exists-wrapped
  antecedent/trigger `expr`. Adds `classification` (`"over_constrained"` | `"insufficient_depth"`)
  and `blocking` (same entry shape as `unreached[].blocking_requires`:
  `{kind, name, loc, text?, requirement?}`). `classification` deliberately triggers the existing
  `intent_unexercised` faithfulness decoration, matching `unreached[]`.
  **Correctness note found while implementing**: `vacuous_implication`'s target expr is that
  invariant's *own* antecedent (`A` in `A => C`); asserting all invariants including the one
  being diagnosed directly constrains `A` (trivially so when `C` is `false`, since `A => false`
  is `not A`), so the unsat core would always name the invariant itself instead of the real
  blocking assumption. Fixed with an `exclude_name` parameter that skips the invariant under
  diagnosis in the "assert all invariants" step — verified against a synthetic fixture
  (`test_vacuous_implication_finding_names_insufficient_depth_when_structurally_possible`) that
  regressed to `over_constrained: [self]` without the fix.
- **`always_true_requires`** — **deferred**, not part of this cut. It needs genuinely new solver
  logic (`_implied_by_core`: `assert_and_track` per preceding guard/implicit invariant, assert
  `Not(guard[req_idx])`, read the unsat core) rather than reusing an existing mechanism, and the
  issue's acceptance criteria are satisfied by conjunct blame + trace blame +
  `vacuous_implication`/`vacuous_leadsto` blocking without it. A follow-up can add
  `implied_by`/`classification` to `always_true_requires` findings the same way, strictly
  additive.

One tracked solver query per finding; findings are rare, cost negligible.

## 5. AST renderer moves to a leaf module

Conjunct/guard/effect text must be rendered from the AST (source-line slicing fails for
compose-component locs — DESIGN-explain §3 — and conjuncts have no per-conjunct loc). bmc.py
cannot import explain.py (explain imports bmc). The pretty-printer is self-contained, so:
new `src/fslc/render.py` (SPDX header) housing `expr_to_text`, `_render_operand`, `_expr_prec`,
`_BIN_PREC`, `_binder_to_text`, `_type_ref_to_text`, `_display_from`, plus a `public_var(name,
spec)` helper (display label + `__`→`.`). explain.py re-imports under the old private names
(tests import `from fslc.explain import _expr_to_text`); bmc.py imports `render`. No cycles.

## 6. JSON contract (strictly additive)

| Where | New key | Type |
|---|---|---|
| `verify` violated (invariant/type_bound) | `blame.conjuncts[]` `{index,text,holds,violating_bindings?}` | list |
| violated `trace[k]` (k≥1) | `blame.guards[]` `{text,loc?}` / `blame.effects[]` `{target,text,loc?}` | dict |
| `explain` `counterfactuals[].violation` / `.trace[k]` | same, inherited | — |
| `vacuous_implication` / `vacuous_leadsto` finding | `classification`, `blocking` | str, list |

No existing field changes meaning or disappears; exit codes untouched. Display discipline: all
names pass through `display_label`/`_public_model_bindings`/`render.public_var` (no `__`
leakage). Nested blame entries avoid the reserved discriminators `result`/`violation_kind`/
`covered` and use `kind` only in the already-established `blocking` entry sense, so
`with_faithfulness`'s recursive walk stays inert on them.

## 7. Ripple / tests

- bmc.py: `_split_conjuncts`, `_conjunct_blame`, `_lvalue_text`, `_walk_executed_assigns`,
  `_blame_trace_steps`, `_diagnose_unreachable_expr` (refactored out of
  `_diagnose_unreached_reachable`, now also used by vacuity); call sites: the invariant-violation
  return in `_bmc_explore`, `_finalize_vacuity_findings` (+ its `verify()` call site, threading
  `source_lines`). explain.py: no logic changes beyond the render re-export (blame is inherited
  for free through `mutate._oracle` → `explain._counterfactuals`'s existing "copy every
  non-trace/spec/result key" behavior). New `src/fslc/render.py`.
- tests/test_explain.py: false-conjunct identification on an AND invariant
  (`test_verify_violated_blame_identifies_false_conjunct`); per-step guard/effect blame
  (`test_verify_violated_trace_steps_carry_guard_effect_blame` — asserts the untouched sibling
  variable `y` is absent from `effects` while the blamed `x`'s write is present, i.e. the slice
  is precise, not "every write"); counterfactual inheritance
  (`test_explain_counterfactual_inherits_blame`, order_workflow `ShippedWasPaid`); the existing
  no-`__`-blob check (`test_explain_json_has_no_internal_double_underscore_names`) already covers
  blame via inheritance, confirmed by re-running it after this change;
  `test_conjunct_blame_never_renders_implicit_bound_invariant_body` is a direct regression test
  for the internal-name leak found via `tests/test_robustness.py`'s corpus-wide no-`__` sweep
  while implementing this (a Seq's implicit `_bounds_<var>` invariant rendered its synthetic
  `<var>__data`/`<var>__len` phys names and `__k` binder verbatim — see §2/§4 fix notes).
  tests/test_vacuity.py:
  `classification`/`blocking` on `vacuous_implication` in both directions (`insufficient_depth`
  with empty `blocking` when structurally possible; `over_constrained` naming the real blocking
  invariant, not the invariant under diagnosis).
- Corpus snapshot (`tests/test_corpus_snapshot.py`) pins the verdict tier only (not
  traces/warning fields) — ran unchanged, confirming additive blame does not diff it.
- Soundness: no solver-path change; blame is post-hoc model evaluation (plus the existing
  reachability-style unsat-core query, reused for vacuity, which was already part of the
  verifier). `tests/test_oracle_agreement.py` / `tests/test_evaluator_agreement.py` /
  `tests/test_trace_soundness.py` ran unchanged and pass — bmc/Monitor agreement unaffected.

## 8. Non-goals / deferred

Repair proposals (weaken guard / drop conjunct) — excluded by design; blame output must stay
descriptive. Deferred, not shipped in this cut: the effect-level "necessary" refinement (§3), the
`always_true_requires` `implied_by`/`classification` fields (§4), per-map-key effect necessity,
CTI blame, and conjunct blame for ensures/trans/leadsTo.
