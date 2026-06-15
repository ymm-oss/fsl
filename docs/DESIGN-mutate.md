# FSL — `fslc mutate` (spec mutation) implementation design

Motivation: issue #6 (category 4/7 of roadmap #1). A spec whose invariants are too weak or
missing stays silently verified (under-constraint). There was no mechanism to measure "how much
the set of properties constrains the model's behavior." Even when tags are present, "whether
that formalization actually constrains anything" (semantic traceability) is invisible to #5's
existence check. This productizes the mutation proof done by hand on fslc itself in DOGFOOD-7.

## 1. CLI

`fslc mutate <f> [--depth K=8] [--by-requirement] [--max-mutants N=200]`. Output
`result:"mutated"`, **exit 0 always** (a generator in the same family as scenarios/testgen;
survivors are review data, not failures. `--fail-on-survivors` is future work).

## 2. Mutate the **dialect-expanded kernel AST**, not the spec dict

It mutates the kernel AST `("spec", name, items)` returned by `parse_src` (with
compose/requirements/business already expanded), and **re-runs `build_spec` for each mutant**
before checking. Reasons:
1. **The type-bound ±1 mutation requires regenerating the `_bounds_*` invariants that
   `build_spec` produces** — directly mutating the spec dict leaves them stale and the mutation
   has no effect.
2. Derived consistency such as `phys_vars` can be left to build_spec.
3. Dialects are handled uniformly. The grammar and verification engine are untouched.

### Mutation operators (deterministic enumeration, no randomness)

| op | error simulated | AST operation |
|---|---|---|
| requires removal | missing guard | delete `("requires", …)` from body |
| requires negation | mistaken condition | wrap with `("not", e)` |
| assignment removal | missing update | delete `("assign", …)` |
| enum swap | wrong transition target | change `("var", member)` to another member of the same enum |
| integer/bound ±1 | off-by-one | `("num", n)`±1, `("type", n, lo, hi)`'s lo/hi ±1 |
| then/else swap | mistaken branch | swap an `if` whose both branches are non-empty |
| fair removal | missing leadsTo fairness assumption | flip the action's fair True→False |

## 3. Kill oracle and baseline gate

Each mutant = mutated AST → `build_spec` → **`verify` (BMC, depth K) + acceptance/forbidden
replay + implements refine**. If any of these returns violated/reachable_failed/error/
refinement_failed, or build_spec raises FslError → **killed** (killer recorded). All clean →
**SURVIVED**. Induction is not used (`unknown_cti` makes the kill decision ambiguous and slow).
**Baseline gate**: if the pre-mutation spec is not verified, refuse (in a buggy spec every
mutant is killed trivially, which is meaningless).

## 4. `--by-requirement` (requirement stress report) — the reverse definition

"What breaks if you remove an invariant" is **fundamentally a no-op for safety**: deleting an
invariant only reduces what is checked and produces no violation (monotonicity). An invariant
can only demonstrate its work by **catching a behavior mutation**. Hence the correct
mechanization is reversed: the kill oracle records each mutant's killer → aggregate by the
`killed_by` requirement tag. **A requirement that killed no behavior mutation = an empty
formalization**, warned as `empty_formalization`. v1 records the first-killer and explicitly
labels this "lower observation bound" (sole-killer redundancy analysis is future work).

## 5. Output / ripple

```json
{"result":"mutated","spec":"…","depth":8,"baseline":"verified",
 "summary":{"total":N,"killed":K,"survived":S},
 "mutants":[{"op","loc","target","status","killed_by","requirement"}],
 "by_requirement":{"REQ-7":{"kills":0,"warning":"empty_formalization"}},
 "notes":["mutant cap 200 reached: 37 dropped"]}
```

New `src/fslc/mutate.py`. Deterministic enumeration + `--max-mutants` truncation is made
explicit in `notes` (no silent cap). Survivors of coverage-false actions are annotated as
"dead at baseline," and equivalent mutants go to a review queue (not a hard failure).
**The verification engine is unmodified.**

## 6. Tests / related

tests/test_mutate.py: cart_v1 guard removal → `_bounds_stock` kill / type-bound +1 kill
(evidence of AST mutation + rebuild) / thinned-invariant survivor / `empty_formalization` /
baseline refusal / coverage-false annotation / truncation annotation / corpus stability /
exit 0. A semantic-level extension of #5 strict-tags; #7 explain's counterfactuals narrate
these kills per invariant. Roadmap #1.
