---
name: fsl
description: Shared FSL language and verifier reference for writing, checking, verifying, repairing, explaining, mutating, refining, replaying, generating scenarios/test scaffolds, and interpreting fslc JSON results. Use directly for FSL syntax, kernel specs, verifier errors, repair loops, and command usage. For role-specific authoring, prefer fsl-business for business flows, fsl-requirements for PM requirements/acceptance/NFR specs, and fsl-design for engineering design/refinement work.
---

# FSL Core — Language, Verifier, and Repair Loop

FSL is a language not present in training data. **Do not write from memory;
follow this guide and reference.md.** Read `reference.md` in the same directory
for syntax details, the full expression catalog, and the idiom collection (always
read it before writing a spec). Within the repository, `docs/LANGUAGE.md` is the
complete reference and `specs/*.fsl` are working examples (cart_v1 is the basic
form, mutex_queue is Seq+leadsTo, and bank_* are refinement+compose examples).

## Choose the right role skill first

This skill is the shared language/verifier reference. For authoring from natural
language, use the narrow role skill first and return here for syntax and repair:

| User intent | Primary skill | Deliverable boundary |
|---|---|---|
| Business process, As-Is/To-Be, controls, KPIs, goals | `fsl-business` | `business` spec and business evidence |
| PM/PdM requirements, acceptance criteria, forbidden flows, NFR/SLA | `fsl-requirements` | `requirements` spec and scenarios |
| Engineering design, internal state/actions, mapping to requirements | `fsl-design` | kernel `spec`, mapping, refine/testgen handoff |
| Design review, variants, SOLID/LSP/OCP/substitutability | `fsl-design-review` | contract-conformance judgment |

If a PM asks for a requirements specification, do not continue into design
artifacts unless explicitly asked. If a consultant asks for business controls, do
not infer system requirements. If an engineer asks for design, do not weaken the
upper business/requirements contract to make refinement pass.

## Prerequisite: the fslc verifier

This skill only supplies language knowledge; verification is done by the `fslc`
CLI. If it is not installed, run `pip install -e .` from the FSL repository (the
root containing `pyproject.toml`) — the only dependencies are lark and z3-solver,
and no native build is required. In environments where `fslc` is not on PATH,
`python -m fslc ...` works identically.

## How to run

```bash
fslc <subcommand> ...            # if installed as editable
python -m fslc <subcommand> ...  # or via the venv python
```

Output is always a single JSON document on stdout. exit: 0=success
(verified/proved/generated), 1=property not satisfied
(violated/reachable_failed/unknown_cti/nonconformant), 2=spec error
(parse/type/semantics/io), 3=internal error.

## Before writing a spec: source fidelity and the formalization memo

FSL is a specification language, not a requirements generator. Encode only facts
that are present in the source material or assumptions the human has explicitly
confirmed. **Do not fill missing requirements, business rules, error handling,
timing, priorities, actors, lifecycle states, design boundaries, or refinement
mappings just to make a complete or verified `.fsl`.** If a missing choice affects
the state schema, an action's enabledness, a transition target, an invariant,
`leadsTo`, a deadline, or a refinement mapping, stop at the memo and ask a
question before writing or changing the spec.

It is acceptable to make representation-only assumptions that do not change
behavior (for example, choosing small finite domain sizes for model checking), but
label them as modeling assumptions and keep them separate from business/design
assumptions. If the user asks for a draft despite open questions, write only the
confirmed fragment and mark the rest as questions; do not invent guards or
invariants to close the gap.

### Formalization memo (post it in chat; do not make a separate file)

When deriving FSL from natural-language requirements, business rules, or code,
**do not jump straight to writing `.fsl`**. First post a **formalization memo** in
chat and get human confirmation before formalizing. What fslc guarantees is the
"internal consistency of the spec as written," not whether "the spec is faithful
to the original intent" — that gap (AI misreadings, dropped requirements,
arbitrary gap-filling) is closed by this memo. The memo is scaffolding for
thinking and confirmation, not a deliverable, so **do not make a separate file**
for it (keep the loop lightweight; the only deliverable is the `.fsl` itself):

- **Glossary and ledger**: candidate state variables, actions (who, and when
  enabled), and candidate enums / domain types with their value ranges
- **Requirement normalization**: for each requirement, one line each for trigger /
  constraint / exception / **boundary implications** (at-least vs. greater-than,
  before vs. after, inclusive vs. exclusive). This is where misreadings most
  frequently occur
- **Assumption ledger**: confirmed assumptions and representation-only modeling
  choices. Do not use this ledger to silently decide missing product, business, or
  design policy
- **Questions for the human**: judgments that cannot be decided during
  formalization (priority of business rules, precedence of exceptions, lifecycle
  states, retry/error behavior, timing/deadline semantics, ownership of actions,
  abstraction boundaries, refinement correspondences, etc.)

The human only needs to read this memo and the verifier's counterexamples —
**do not make them review logical formulas directly**. Write the `.fsl` only after
the memo has received human confirmation or correction for any choice that changes
behavior.

### Keep only confirmed assumptions in the spec (fold them into the .fsl, not a separate memo file)

Most of the memo can disappear into chat, but **if the confirmed assumption ledger
is discarded, you later cannot trace "why this interpretation was chosen," which
is a problem**. A separate file would drift out of sync with the spec, so **keep
confirmed assumptions in the `.fsl` itself as comments / tags**:

- Global assumptions → a ledger block at the top of the spec:
  `// ASSUME-1: stock is reserved by only one user at a time`
- An assumption justifying a specific guard / invariant → tag that declaration:
  `invariant OnePerUser "ASSUME-1: only one user reserves at a time" { ... }`

This way assumptions travel with the spec, are visible in PRs, and a future
`--strict-tags` check can distinguish "intended assumptions (tagged)" from
"unfounded fabrications (untagged)."

## Natural language → syntax mapping (from the formalization memo to the spec)

Map the sentences extracted during requirement normalization (the memo above) to
syntax using the following correspondence. Whereas the idiom collection in
reference.md §8 goes "FSL → the correct way to write it," this is the reverse
lookup "natural language → which construct." **Free-form logical formulas not
covered by this table are easy to misread, so mark them for human confirmation in
the formalization memo.**

| Natural-language pattern | FSL construct |
|---|---|
| "must never" / "always the case" (prohibition, invariance) | `invariant` (safety) |
| "prohibit/constrain a change from one state to the next" (two-state safety) | `trans` (use `old()` to reference the pre-transition state) |
| "can only do X when Y" (precondition) | an action's `requires` |
| "once X happens, Y must eventually happen" (response, progress) | `leadsTo` + `fair` on the action that drives progress |
| business-flow stage response for consultants/PMs | `policy POL-1 "..." every Case in Source must eventually be Target [or Target ...]` |
| business-flow reachability / completion goal | `goal G "..." some Case can reach Target` or `goal G "..." all Case can be Target [or Target ...]` |
| "once X has happened, it can never happen again" (history dependence) | ghost variable (`ever_*`) + invariant |
| "X can be reached / X can end up being reached" (possibility) | `reachable` (witness, or detection of over-constraint) |
| "within K times / K ticks" (deadline) | requirements `time` + `deadline` (reference §11) |
| upper/lower bound or non-negativity of a number | domain type `type T = lo..hi` (do not hand-write boundary invariants) |
| "at most / less than / at least / greater than" "before / after" | `<= / < / >= / >`. **Make boundary implications explicit in the memo** (the most frequent misreading) |
| "the total equals X" / "the count is X" (aggregate consistency) | an invariant over `sum(...)` / `count(...)` |

## Standard workflow (treat proved as the standard)

1. Write the spec → `fslc check file.fsl` (syntax and types only, fast; fix
   following the error's `loc`/`expected`/`hint`).
   When checking requirement traceability strictly, add `--strict-tags`
   (and `--requirements ids.txt` if needed). Only when the result is
   ok/verified/proved do untagged declarations and unreferenced requirement IDs
   become warnings.
2. `fslc verify file.fsl --depth 8` → see the table below for what each result means
3. Once verified, run `fslc verify file.fsl --engine induction` → done at `proved`
   (note: `--depth K` **includes** step K. Invariants become infinite-depth under
   `proved`; `leadsTo` remains bounded unless it declares `decreases <int expr>`,
   in which case induction can prove that response with an unbounded ranking
   argument)
4. As needed: `fslc explain file.fsl --depth 8`
   (emits, as deterministic JSON, the spec skeleton, implicit type-bound/partial_op
   checks, a "what if this rule were absent" counterfactual for each user
   invariant, and reachable/scenarios witnesses. For PMs/consultants, ask them to
   adjudicate concrete traces rather than logical formulas),
   `fslc mutate file.fsl --depth 8 --by-requirement`
   (shows how many model mutations the spec's properties kill; a survivor is not a
   failure but a candidate for a missing invariant / acceptance / forbidden. For a
   spec whose baseline is not verified, it emits no mutation report and returns the
   baseline result), `fslc scenarios` (integration-test skeleton JSON),
   `fslc testgen -o test_x.py`
   (implementation-conformance pytest skeleton), `fslc replay --trace events.json`
   (log conformance), `fslc refine impl.fsl abs.fsl mapping.fsl` (faithfulness check
   of a detailed spec).
   Note: what verify/induction guarantees is the **internal consistency of the
   spec**, which is separate from **whether the implementation honors the spec
   contract**. If implementation conformance is also required, anchor to the
   implementation with `testgen` (pytest via an Adapter) / `replay` (matching
   against execution logs).

## Repair protocol (result → next move)

Machine-readable `faithfulness_class` tags are a quick routing layer over the
existing result/kind fields:

| faithfulness_class | Recommended action |
|---|---|
| `partial_op_unguarded` | Add the missing guard / run bounded Monitor (replay) |
| `frozen_only_invariant` | Run mutate to check kill-rate |
| `intent_unexercised` | Add a single-shot reachable for the action / raise `--depth` |
| `liveness_not_refined` | Re-prove liveness at each layer |

| result / violation_kind | Meaning | Next move |
|---|---|---|
| `violated` / `invariant` | Counterexample found (trace is shortest) | Read the trace's `changes` and `violating_bindings`; add a guard or fix the invariant |
| `violated` / `trans` | Two-state safety counterexample found | Compare the trace's previous state with the violating step; decide between adding a guard, fixing the action, or fixing the trans |
| `violated` / `type_bound` | Bounded type out of range (automatic check) | Insufficient guard on `last_action`. Keep within range via `requires` (do not hand-write an invariant) |
| `violated` / `partial_op` | pop/head on an empty Seq, index out of range, or divisor 0 | Guard with `requires q.size() > 0` / `requires d != 0` or an `if` |
| `violated` / `ensures` | Postcondition not satisfied | Decide whether the body or the ensures is correct, and fix accordingly |
| `violated` / `leadsTo` | Response-property counterexample (lasso / stall) | Check the trace's `loop_start`. Either add `fair` to the action that drives progress, or fix the spec |
| `reachable_failed` | A state you want to reach is unreachable | Read `action_coverage`'s `blocking_requires` (unsat core). Loosen a guard / add an action / increase `--depth` |
| `unknown_cti` | The invariant is true but not inductive | **The CTI's starting state = a phantom state satisfying all invariants. Add an auxiliary invariant (one that is a domain truth) that excludes it, then re-run.** Track record: converges in one round (e.g. "no duplicates in the queue," "refunds only from Captured") |
| warning / `vacuous_implication` | The antecedent of an implication invariant is never reached within depth | Check whether an action / reachable witness that makes the antecedent hold is missing, or whether the antecedent expression is reversed or too strong relative to intent. Do not simply weaken the consequent |
| warning / `vacuous_leadsto` | The leadsTo trigger is not reached within depth | Check the action / guard / initial condition for entering the trigger state. Look first at whether P (not the response target Q) actually occurs in the spec |
| warning / `always_true_requires` | Under the context of the preceding requires, this requires clause is not effective as a constraint | Decide whether the clause is redundant or whether a path to the state where the clause bites is missing. Do not delete it automatically |
| warning / `tautology_over_frozen` | An invariant that depends only on frozen variables no action ever assigns to, and is dynamically always true (a dead ghost = hollow) | Make the variable `const`, or suspect a missing action that should change it. A sign that the invariant "thinks it is checking a contract but checks nothing" |
| `error` / `parse` | Syntax error | Follow `loc` and `expected` (candidate tokens) |
| `error` / `type` | Type error | Follow the `hint` (e.g. `x == some(e)` → bind with `x is some(v)` and compare) |
| `error` / `semantics` | Double assignment, etc. | Do not assign to the same variable twice on the same path (an if's then/else are separate paths, so it is allowed) |
| `error` / `vacuous` | init is unsatisfiable (contradictory assignments, etc.) | Review init. Check that you are not giving one state variable contradictory values. A violation from an out-of-range value is different and becomes `violated`/`type_bound` |
| `refinement_failed` / `abs_requires_failed` | A detailed-layer transition breaks an upper-layer guard (e.g. a shortcut skipping approval) | Read `impl_action` and `impl_trace`. Add a guard to the detailed layer, or review the interpretation of the correspondence (`maps` / mapping) |
| `refinement_failed` / `abs_state_mismatch` / `stutter_changed_abs` / `map_out_of_bounds` | Mapping inconsistency (an update has no correspondence / a stutter nonetheless changes upper-layer state / a mapped value is out of the type's range) | Compare the `mismatch` path with `abs_before/after`. Fix the mapping expression or the action correspondence |
| `implements.result: violated` within verify | The requirements layer deviates from the upper (business) layer | The contents of `implements.violation` have the same shape as refinement_failed. Same procedure as above + check the `requirement` on the requirements side |
| `error` / `acceptance` | Replay of an acceptance criterion failed | The ID and step of the failed AC are returned. Decide whether the procedure's precondition (state) or the expect is correct, and fix accordingly |
| `error` / `forbidden` | An operation sequence that should be rejected was accepted (under-constraint; the kind that a safety invariant stays silent about) | `accepted_trace` is the accepting path. The requires enabling the last operation is too loose → add a guard or review the spec |
| `error` / `forbidden_setup` | A precondition (non-final) step of the forbidden is not enabled (invalid trace) | Review the setup procedure. The non-final steps are there to reach that point and are not treated as success |

For an action whose coverage is `false`, `blocking_requires` pinpoints "which
requires is blocking it" on a per-clause basis, and `hint` summarizes the
blocking factors. Do not silently ignore it. For branches-split actions,
diagnostics keep the internal name (`submit__b1`) and add a human
`display_name` such as `submit[a <= AUTO_LIMIT]`.

Liveness is still checked separately from refinement: safety refinement can
return `refines` while a lower-layer `leadsTo` fails. Treat
`liveness_not_refined` as the routing tag for leadsTo-refinement diagnostics; the
current clean signal is usually the separate `violated` / `leadsTo` verification
result at that layer.

When a counterexample makes you **change an interpretation** (added a guard,
loosened an invariant, decided how to handle an exception), record that judgment in
the assumption ledger (the `// ASSUME-n:` comments / tags in the `.fsl`) only after
the source material or the human confirms it. If the counterexample exposes a
missing requirement or design decision, ask instead of choosing the repair on the
user's behalf. The shortest path to verified is often "weakening the spec," so
without confirmation and a record of what was weakened and why, you later cannot
distinguish a hollowing-out repair from a legitimate fix.

## Minimal syntax (details and the full catalog are in reference.md)

The following is a self-contained template that passes `fslc check` as-is (the
element types of Map/Option/Seq are all declared as domain types — **every type
you use must be declared with `type ... = lo..hi` or `enum`**; an undeclared type
becomes an `unknown type` error):

```fsl
spec Cart {
  const CAP = 3
  type ItemId = 0..1
  type UserId = 0..1
  type JobId  = 0..1
  type Qty    = 0..5                     // domain type = bounded integer; range is checked automatically
  enum St { Open, Closed }
  struct Order { st: St, qty: Qty, buyer: Option<UserId> }

  state {
    stock: Map<ItemId, Qty>,
    cart:  Option<ItemId>,
    q:     Seq<JobId, CAP>
  }
  init {
    forall i: ItemId { stock[i] = 1 }
    cart = none
    q = Seq {}
  }

  action add_to_cart(i: ItemId) {
    requires cart == none
    cart = some(i)
  }

  fair action abandon() {                // always possible, so Served (below) holds
    requires cart != none
    cart = none
  }

  fair action checkout(u: UserId) {      // fair = weak fairness (for leadsTo)
    requires cart is some(i)             // i is bound here
    requires stock[i] > 0
    stock[i] = stock[i] - 1              // every RHS reads the old state (simultaneous assignment)
    cart = none
    ensures stock[i] == old(stock[i]) - 1
  }

  // Do not write a boundary invariant like "stock[i] >= 0" (Qty=0..5 checks it automatically).
  // Below is an example of a genuine, non-boundary safety invariant (in the <expr> position).
  invariant QueueStaysEmpty { q.size() == 0 }   // unchanging since no action touches q
  trans StockNeverIncreases { stock[0] <= old(stock[0]) } // two-state safety
  reachable SoldOut { stock[0] == 0 }           // a witness is returned
  leadsTo Served { cart is some(j) ~> cart == none }   // ~> is leadsTo-only
  terminal { stock[0] == 0 }                    // intended terminal state (excluded from the deadlock check)
}
```

## Rules to always follow (structural pitfalls)

- **No sentinel values (-1, etc.) → use `Option<T>`**. `x == some(e)` is a type
  error — extract with `x is some(v)`. `== none` / `!= none` are allowed.
- **Do not hand-write "non-negative"-style invariants** → `type Qty = 0..N` checks
  them automatically.
- A **double assignment on the same execution path is an error**. Assigning to the
  same variable after an if as inside a branch is also an error.
- Updates to Set/Seq are **re-assignments**: `s = s.add(x)`, `q = q.pop()`.
- Seq `pop/head/at` and the divisor of `/` `%` **must always be guarded** (requires
  or if). Forgetting is detected as partial_op.
- When talking about a Seq in an invariant, use an index guard:
  `forall i in 0..CAP-1 { i < q.size() => P(q.at(i)) }` (write the range as
  `0..CAP-1`, derived from the const — hard-coding a literal will not track a
  capacity change).
- **Nested Maps (`Map<K1, Map<K2,V>>`) are not allowed** → flatten two axes into a
  single product domain type (`type Cell = 0..ROOMS*SLOTS-1`) and recover the axes
  with `c / SLOTS` and `c % SLOTS`.
- "X is preserved from the previous state to the next state" is `trans`. `old()`
  can only be used inside `ensures` / `trans`.
- A **history/response** like "Y happened sometime after X" cannot be written with
  state — add a ghost variable (`ever_locked`, etc.), or use `leadsTo` for a
  response property.
- An **intended terminal state** (processing complete, etc. — a state where
  stopping is correct) would become a deadlock warning → declare it with
  `terminal { <predicate> }` (applying `--deadlock ignore` globally hides even
  unintended deadlocks). Stops not included in terminal continue to be detected.

## Recommended practices (optional — by risk; may be skipped for small specs)

Unlike the "rules to always follow" above, this is **not mandatory**. Imposing
heavy procedures on every spec kills the lightweight loop, so apply them only to
important constraints and high-risk specs.

- **Pair with a positive example**: when you write an invariant, attach one
  `reachable` or `acceptance` near its boundary showing that "behavior that should
  be allowed is still possible." This lets you self-detect over-guarding
  (over-constraint) and vacuous invariants. Especially effective when a repair
  strengthened a guard. Example: attaching `reachable SoldOut { stock[0] == 0 }` to
  a stock-decrementing spec confirms "selling out is reachable = not over-guarded."
- **One requirement = one declaration**: avoid a huge conjunctive invariant and
  split declarations per requirement. The counterexample's `requirement` tag then
  bites, diagnostics are easier to read, and which requirement broke is clear in
  one round-trip.
- **Domain sizing**: for properties about interactions between entities, use at
  least 3 entities (with 2, symmetry hides bugs); make capacities values where you
  can try "limit + 1"; and standardize checks at depth 8 + induction.
- **Cross-validation (high-risk specs only)**: for specs where errors are serious,
  such as payments or permissions, (a) have a separate agent that has not seen the
  source translate the `.fsl` into natural language and reconcile it item-by-item
  against the requirements list, or (b) fix the state schema and have two agents
  independently write the dynamics + properties, then `replay` each other's
  `scenarios` against the other's spec to expose discrepancies. Costly, so use it
  selectively.

## Role-specific authoring entry points

When the task starts from role language rather than raw FSL syntax, use the role
skill first. This prevents business, requirements, and design decisions from being
mixed in one spec.

| Role / intent | Use skill | Examples to read | Constructs mainly written |
|---|---|---|
| Consultant (business flows, regulations, As-Is/To-Be) | `fsl-business` | `examples/consulting/`, `examples/pm/cancel_flow.fsl` | `business` (reference.md §10) |
| PM / PdM (requirement definition, acceptance criteria) | `fsl-requirements` | `examples/pm/`, `examples/e2e/2_requirements.fsl` | `requirements` (reference.md §10) + NFR (reference.md §11) |
| Engineer (design, implementation connection) | `fsl-design` | `examples/e2e/`, `examples/bank/` | kernel `spec` + refine mapping + Adapter (reference.md §9) |

The flagship example threading all three roles through one domain is
`examples/e2e/` (expense reimbursement).

## Three-layer dialects (consulting / requirements / design)

A spec can be written in three layers. Chain **business ⊒ requirements ⊒ design ⊒
implementation** via refinement (syntax in reference.md §10). Every layer expands
to the kernel, so verify/induction/scenarios/Monitor are used identically:

Treat the layer boundary as part of the contract. Do not move to the lower layer
unless the user asks for it or the relevant role skill directs it.

- `business Name { process/policy/kpi/goal }` — the consulting layer. For
  PM/consulting-facing files, prefer the readable stage syntax for common rules:
  `policy ... every Case in Source must eventually be Target [or Target ...]`,
  `goal ... some Case can reach Target`, and
  `goal ... all Case can be Target [or Target ...]`. Use explicit
  `responds { forall ... stage(c) ... ~> ... }` / `{ expr }` only when the rule is
  not simple stage progression. Regulation contradiction = invariant violation,
  dead business step = coverage diagnostic, unreachable business goal =
  reachable_failed
- `requirements Name { requirement REQ-1 "source" {...} / acceptance / branches /
  implements Abs from "file" {map ...} }` — the requirements layer. With
  `implements`, verify simultaneously runs the refine to the upper layer (the
  `implements` field in the result JSON). `acceptance` is replay-checked at check
  time and flows scenarios → testgen. `forbidden` (must-forbid) conversely writes
  an "operation sequence that should be rejected" and verifies at check time that
  the last step is rejected (not-enabled or a violation) — if accepted,
  `kind: "forbidden"`. An independent channel for catching under-constraint
  (missing guards) that a safety invariant stays silent about (a receptacle for
  cross-validation where a separate agent writes positive/negative traces from NL)
- The design layer is an ordinary `spec` (the main subject of this guide). Connect
  it to the requirements layer with `fslc refine`
- **Traceability**: a `"ID: source"` tag immediately before a declaration's `{`.
  `requirement: {id, text}` appears in violated / CTI / coverage / scenarios — when
  you read a counterexample, always look at the requirement and repair in line with
  that requirement's intent

## Advanced features (the relevant reference.md section, when needed)

- **Non-functional requirements**: permissions, auditing, capacity, and
  reliability behavior can be written with ordinary invariant/leadsTo. SLA/timeout
  use the requirements `time`+`deadline` (reference.md §11)
- **Aggregation over Seq**: `sum(i: Idx of log.at(i) where i < log.size())` (Idx is
  a domain type covering the capacity)
- **Composition**: `compose X { use A as a from "a.fsl" ... }`, synchronized
  actions `action s(..) = a.act(..) || b.act2(..) { .. }`, `internal a.act`
- **refinement**: a mapping file (`map abs_var = expr`,
  `action impl -> abs(..) | stutter`, the mapping-expression-only
  `if c then a else b`) + `fslc refine`
- **Implementation connection**: wire the Adapter (reset/step/observe) of the file
  generated by `fslc testgen` into the implementation. observe has the same shape
  as the spec's logical state (enum as a name, Option as None|value, Seq as a list,
  composition as `alias.var` keys)
- **Ghost types (typestate)**: `fslc typestate file.fsl [--ts]` — determines how
  far a state machine (a struct field with enum values / a state variable /
  an `Option<_>` slot) can be mapped onto the host language's typestate (derivable /
  branching / relational). If all transitions are typeable, applicability=full.
  `--ts` outputs a TypeScript skeleton for the derivable portion (reference.md §7)
