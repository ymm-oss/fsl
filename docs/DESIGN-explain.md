# FSL — `fslc explain --readable` (human-readable rendering and counterfactuals) implementation design

Motivation: issue #7 (category 6 of roadmap #1). Having PMs and consultants directly
review the logical formulas written by an AI is not realistic, whereas humans are good at
judging the correctness of concrete traces. There was no mechanism to let the machine assist
the spec→human direction of translation. This shifts human review from "reading logical
formulas" to "adjudicating concrete examples."

## 1. CLI / output

`fslc explain <f> [--depth K]` → `result:"explained"`, exit 0, JSON output.
`fslc explain <f> [--depth K] --readable` emits the deterministic text review
view. **No LLM used** (deterministic formatting only; prose generation is left
to the agent-side skill). The verification engine is unmodified; #6 mutate and
verify are reused.

```json
{"result":"explained",
 "skeleton":{"state":…, "actions":[{"name","actor?","requires_text","writes","ensures_text","requirement"}],
             "properties":[{"kind","name","body_text","requirement"}], "auto_checks":…},
 "counterfactuals":[{"invariant","weakening":{op,loc,target},"trace","requirement"}],
 "witnesses":[…]}
```

## 2. The three artifacts

1. **Skeleton enumeration** — outputs the state schema, verification bounds, KPI
   projections, branch lowering, synthesized refinement mapping, each action's
   "who / when (requires) / what it changes," and each property's "what it
   forbids / guarantees," together with requirement tags and implicit checks
   (type bounds, partial_op).
2. **Counterfactual narratives** — for each user invariant, presents the shortest counterexample
   under the minimal model weakening that makes the invariant violated, as "without this rule,
   the following sequence breaks REQ-3." This automates the manual demo in `cart_v1_buggy.fsl`.
3. **Witness narration** — formats reachable / scenarios traces with display names plus the
   original requirement text.

## 3. Building the skeleton WITHOUT an AST pretty-printer (important)

**FSL has no AST→string formatter.** What `_requires_blocking_entry` (bmc.py) does is
`source_lines[line-1].strip()` — **slicing out a source line by its loc**, not rendering the
AST. Therefore:
- The state schema, action params, and **"what is changed" are computed by structurally
  scanning the left-hand sides of assign statements**.
- The **text of requires / property bodies is shown verbatim via loc slicing** (in a
  dialect-expanded spec the loc points at the original business-layer text, so seeing things
  like "by Manager" is actually desirable).
- **compose**: locs originating from a component point at a file that has not been loaded, so
  source slicing is impossible → fall back to names/structure (do not crash).

## 4. Counterfactuals sit thinly on top of #6 mutate (using all operators)

In essence this is "the #6 mutate kills re-sorted per user invariant and narrated." No new
verification logic is written. Key points:
- The counterfactual search draws on **all of mutate's operators (guard removal + assignment
  removal + fair removal)**. **Requires removal alone is not enough**: what breaks
  `ShippedWasPaid` in order_workflow is not a `requires` removal but **removal of the state
  assignment `orders[o].status = Shipped` in ship** (leaving only `shipped.add(o)` violates
  it via "in the set yet not Shipped").
- `killed_by` includes non-invariants too (ensures = action name, reachable name, bounds) →
  counterfactuals are **limited to user invariants**. Reachable kills are a different kind
  ("without this, X cannot be reached").
- **"No counterfactual" is legitimate and common**: order_workflow's `NonNegativeRevenue`
  cannot be broken by any single weakening within depth (it is either redundant — implied by
  other invariants — or requires greater depth). State this gracefully rather than erroring.
  Continuous with #6's `empty_formalization`.

## 5. Ripple / tests

New `src/fslc/explain.py` (~120 lines, reusing mutate/verify) + a cli subcommand. Display
names only (no internal-name leakage), including nested counterfactual violation payloads
where `invariant` carries the public dotted name and no raw `internal_invariant` duplicate is
emitted. JSON-serializable. tests/test_explain.py: cart_v1
skeleton / the `ShippedWasPaid` counterfactual appears via **assignment removal** /
NonNegativeRevenue yields "no counterfactual" / the dialect cancel_flow carries the original
requirement text / compose does not crash / exit 0. Roadmap #1.
