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

**What makes FSL different — connectivity, not just per-spec checking.** Classic
formal methods describe one hard spot and verify it in isolation (the "island"
model). FSL's distinctive value is stitching business ⊒ requirements ⊒ design
together with refinement, so **cross-layer alignment (traceability) is itself
checkable** — the dominant question moves from "is this spec correct?" to "do these
layers still mean the same thing?" Verify a one-off hard spot with a single spec;
when the value is keeping the layers aligned, reach for the connected workflow
(`fslc chain`) and the `fsl-delivery` skill. The two workflows are juxtaposed below.

## First, decide whether FSL fits (self-check)

Before reaching for a spec, run this filter. FSL is not for every task, and forcing
it where it does not fit wastes effort and produces hollow specs. **This is a
judgment aid, not a gate**: when neither payoff below applies, say so and recommend
the better tool (usually ordinary tests) instead of writing FSL anyway.

**Two payoffs justify writing FSL, and either alone is enough — this is not a single
verification-ROI gate:**

- **Verification payoff**: can some order of operations or combination of flags reach
  a state that must never happen?
- **Documentation payoff**: would this feature get a spec or design doc written for it
  regardless? If so, write that doc as FSL — the `.fsl` source replaces the prose doc
  you would write anyway, so it is simultaneously what people read and what the
  verifier checks, with zero drift between the two.

"Out of scope" is reserved for what FSL cannot express (gate 3 below), not for
"verification ROI too low." A linear-path or CRUD feature that would be documented
regardless is in scope as a thin verifiable doc, even with no forbidden state to
prove. Because of the documentation payoff, broad coverage across
business/requirements/design layers is the default target, not the exception —
replacing a prose doc needs no per-feature verification-ROI proof; treat high-risk-
first as adoption *sequencing* when capacity is limited, not as where coverage stops.

**The verification test — judge by _interaction_, not size:** can some **order of
operations or combination of flags** reach a state that must never happen? Even 3
states + 2 flags qualify if back / cancel / retry / permission branching is involved;
a hundred states on one linear path do not by verification payoff alone — check the
documentation payoff above before ruling a feature out.

Three gates, top to bottom. Gates 1 and 3 stop on genuine inexpressibility (no
payoff rescues those); at gate 2, check the documentation payoff before ruling a
feature out of scope:

1. **State machine?** Can you draw boxes (states) + arrows (operations)? No →
   nothing to model (static display, decoration with no state to speak of). Out of
   scope; recommend ordinary tests.
2. **Interaction can reach a bad state, or would this be documented anyway?** order /
   flags / permissions / async / retry combine into a forbidden state → verification
   payoff, write it. Otherwise, a linear path or simple CRUD flow that would still
   get a spec/design doc → documentation payoff, write it as a thin verifiable doc
   (low priority, not out of scope). Neither → tests usually suffice.
3. **Finite & discrete?** real-time values, probability, continuous quantities, or
   free-text meaning are **not** the core. No → the core won't fit the model; FSL is
   at most an aid. (SLA is fine only as a *relative, discrete-step* deadline, not a
   wall-clock value — reference §11.)

Keep "**low priority** (possible but thin)" distinct from "**out of scope** (not
expressible)." High-yield: payments/refunds, approvals/send-backs,
inventory/allocation, permissions/audit, queues/async, SLA/timeout/retry, screen
transitions / double-submit / unsaved-changes. Out of scope: real time, probability,
continuous money, free-text correctness, absolute latency — what FSL cannot express,
not merely what scores low on verification payoff alone.

**The second lens — one of FSL's primary uses, not a fourth gate: is there
connectivity value?** The three gates score a spec *as a single island*, but that is
only half of when FSL pays off. FSL's distinctive edge over classic formal methods is
that it *also* verifies *cross-layer alignment*, so a spec that rates "low priority
(possible but thin)" on its own can be high-value once connection is the point — a
requirement provably honored by the design, a regulatory control that still bites at
the lowest layer, an As-Is→To-Be change that preserves a control. When that alignment
is the deliverable, author the layers and gate the seams with `chain` (the connected
workflow below) even if any single layer is thin; verifying the connection is a
primary use, not a tail-end advanced topic. The converse — the brake on writing *too*
much — is the **abstraction tax**: if there is really only one hard altitude, do *not*
manufacture three layers — you would just write the same thing three times at
different verbosity. Island-shaped hard spots stay single-spec, exactly as before. So
the wider you write across genuine layers, the more alignment you can mechanically
manage — but this stays a judgment lens, never a mandate to manufacture a layer that
does not genuinely exist (FSL formalizes the contract layers that are actually
there; natural-language discovery, UI/API/visual design, coding, and testing still
happen in their own tools). (Same criterion in the manual's "When to Use FSL"
chapter.)

Even past the gates, the value is conditional: **FSL checks the spec, not the
product.** If no human owns the rules, or (for conformance) no faithful Adapter/log
is feasible, keep it to lightweight pre-implementation review and do not claim
implementation conformance. A spec that no mutation kills is hollow comfort — check
`fslc mutate` kill-rate (a very low kill-rate signals a hollow spec).

The resulting spec corpus is not a one-time deliverable: treat it as a living single
source of truth, re-verified on every change (CI regression, drift detection, and
cross-layer change-impact via `refine`), and read directly — by humans and by AI — as
onboarding context for the flow it documents.

Full rationale, plus the per-feature vs per-project distinction, is the manual's
"When to Use FSL" chapter (`docs/intro/when-to-use.{ja,en}.html`).

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

These boundaries are not mere hygiene: each handoff is a **refinement seam, not a
plain baton pass**. The lower layer refines the upper one (`implements` at the
requirements→business seam, `fslc refine` at the design→requirements seam), so the
upper spec is a frozen contract and the seam itself is verified — which is exactly
why weakening the upper layer to make a lower one pass defeats the point.

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
(verified/proved/generated/analyzed), 1=property not satisfied
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
| "P must become Q within K steps" (bounded response) | `leadsTo Name { P ~> within K Q }` |
| "keep P true until Q" (safety, Q may never happen) | `unless Name { P unless Q }` |
| "keep P true until Q, and Q must happen" (safety + progress) | `until Name { P until Q }` |
| business-flow stage response for consultants/PMs | `policy POL-1 "..." every Case in Source must eventually be Target [or Target ...]` |
| business-flow reachability / completion goal | `goal G "..." some Case can reach Target` or `goal G "..." all Case can be Target [or Target ...]` |
| "once X has happened, it can never happen again" (history dependence) | ghost variable (`ever_*`) + invariant |
| "X can be reached / X can end up being reached" (possibility) | `reachable` (witness, or detection of over-constraint) |
| "A is linked to B" / graph reachability / acyclicity / functional relation | `state { r: relation A -> B }` plus `.contains/.add/.remove`, `reachable`, `acyclic`, `functional`, `injective`, `domain`, `range` |
| "within K times / K ticks" (deadline) | kernel `leadsTo ... within K` for step deadlines, or requirements `time` + `deadline` for SLA/tick semantics (reference §11) |
| upper/lower bound or non-negativity of a number | kernel: `type T = lo..hi`; business/requirements dialects: `number T` plus `verify { values T = lo..hi }` (do not hand-write boundary invariants) |
| "at most / less than / at least / greater than" "before / after" | `<= / < / >= / >`. **Make boundary implications explicit in the memo** (the most frequent misreading) |
| "the total equals X" / "the count is X" (aggregate consistency) | an invariant over `sum(...)` / `count(...)` |

## Standard workflow (single spec; treat proved as the standard)

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
4. As needed: `fslc explain file.fsl --depth 8 --readable`
   (emits, as deterministic JSON, the spec skeleton, implicit type-bound/partial_op
   checks, a "what if this rule were absent" counterfactual for each user
   invariant, and reachable/scenarios witnesses; `--readable` emits a text view
   that surfaces verification bounds, fairness, KPI projections, branch lowering,
   and synthesized refinement mappings. For PMs/consultants, ask them to
   adjudicate concrete traces rather than logical formulas),
   `fslc analyze file.fsl --profile ai-review`
   (emits structural review findings over the Typed Semantic Graph, such as
   disconnected requirements, unanchored properties, progressless cycles,
   unwritten state, and unguarded actions. `analyze` also supports batch
   file/directory review, standalone `refinement_graph`, project
   `traceability_graph`, DOT/Mermaid graph exports, and JSON schemas under
   `schemas/fslc/analysis/`. These are review signals with
   `formal_status:"not_a_violation"`, not proof failures),
   When you add natural-language judgment on top of `analyze` output, keep it
   agent-side: cite exact source text and TSG node ids, keep
   `formal_status:"not_a_violation"`, do not turn suggestions into fslc
   violations or CI failures, and do not send source/requirement/comment text to
   an external model unless the user or environment has explicitly opted in,
   For tag/formula alignment, first run
   `fslc analyze file.fsl --export tag-review` and compare one declaration tuple
   at a time. Treat `tag_stale_reference` / `tag_formula_disjoint` as exact
   identifier evidence only, not semantic proof,
   `fslc mutate file.fsl --depth 8 --by-requirement`
   (shows how many model mutations the spec's properties kill; a survivor is not a
   failure but a candidate for a missing invariant / acceptance / forbidden. For a
   spec whose baseline is not verified, it emits no mutation report and returns the
   baseline result), `fslc scenarios` (integration-test skeleton JSON),
   `fslc testgen -o test_x.py`
   (implementation-conformance pytest skeleton), `fslc replay --trace events.json`
   (log conformance), `fslc refine impl.fsl abs.fsl mapping.fsl` (faithfulness check
   of a detailed spec).
   For AI tool-boundary contracts, use `fslc ai check file.fsl` on
   `ai_component` specs and `fslc ai replay file.fsl --logs events.jsonl` for
   runtime event evidence. For recursive fsl-ai agent composition, use
   `fslc ai check file.fsl` on `agent` specs; it returns `agent_analyzed` and
   deterministic `agent_ir` / graph summaries for lexical scope, explicit
   authority/context grants, visibility, orchestration, tool reachability, and
   failure policy. These check syntactic/structural hard facts such as tool
   authority, forbidden tools, human approval, and agent graph boundaries;
   evaluator-backed and statistical AI claims are evidence, not formal proof,
   and are out of Phase 1.
   Note: what verify/induction guarantees is the **internal consistency of the
   spec**, which is separate from **whether the implementation honors the spec
   contract**. If implementation conformance is also required, anchor to the
   implementation with `testgen` (pytest via an Adapter) / `replay` (matching
   against execution logs).
   For scope-sensitive failures, use `fslc sweep file.fsl --instances Case=1..3
   --depth 1..8 [--property Name]`; it reports each run under `sweep.results` and
   the first failing scope under `sweep.minimal_counterexample`.

## Connected workflow (across layers — when alignment is the deliverable)

When the value is cross-layer alignment rather than one spec (the connectivity lens
in the self-check), the workflow changes shape: author each layer, then verify the
**seams** between them. Here the connecting operations are the stars, not an advanced
afterthought. (Layer syntax: reference.md §10 and "Three-layer dialects" below; route
authoring through the role skills.)

1. **Author each layer** as its own spec — `business` ⊒ `requirements` ⊒ `design`
   (⊒ implementation) — via the role skills (fsl-business / fsl-requirements /
   fsl-design). Verify each on its own first with the single-spec workflow above
   (`check` → `verify` → `--engine induction`).
2. **Stitch the seams downward — each seam is a refinement obligation = a contract:**
   - requirements → business: put `implements BusinessName from "business.fsl" { }`
     in the requirements spec; `verify` then also runs the refine and reports it under
     the `implements` field of the result JSON (an empty body auto-generates identity
     refinement when names match).
   - design → requirements: a mapping file + `fslc refine design.fsl requirements.fsl
     mapping.fsl`.
   - when an upper response must survive the seam, add
     `preserve progress { respond AbsLeadsTo by impl_action, ... }` to the mapping
     (see the soundness note below).
3. **Gate the whole chain at once** with `fslc chain fsl-project.toml`: it runs
   business → requirements → design → impl from a manifest and returns a per-layer
   table (a failed layer stops the chain unless `--keep-going`). This is the connected
   analogue of single-spec `verify`.
4. **Read counterexamples by seam.** A `refinement_failed` / `implements.violation`
   names the seam that broke; repair in line with the contract — never weaken the
   upper layer just to make the lower one pass (that hollows out the very traceability
   the chain exists to prove).

### Two soundness facts about connection (read before trusting a chain)

- **Safety descends, liveness does not.** Refinement propagates safety (`invariant` /
  `trans`) downward for free, but a response property (`leadsTo`) does **not** ride
  down with it: a safety refinement can return `refines` while a lower-layer `leadsTo`
  fails. To keep an upper response across a seam, either re-prove it at the layer that
  owns progress, or pull it through the mapping with `preserve progress` (failure is
  `refinement_failed / progress_lost`; the actual lasso exclusion still comes from
  lower-layer `fair action` declarations). This is a general property of forward
  simulation, not an fslc limitation.
- **A chain is exactly as strong as its refinement soundness.** "Verified
  traceability" is real only if each seam genuinely fails when it should; a link that
  silently passes where it ought to break turns the chain into false confidence. So
  treat a green seam like a green single spec — confirm it is not vacuous (e.g. the
  `mutate` kill-rate per layer) and never relax a mapping just to turn a seam green.

## Repair protocol (result → next move)

Machine-readable `faithfulness_class` tags are a quick routing layer over the
existing result/kind fields:

| faithfulness_class | Recommended action |
|---|---|
| `partial_op_unguarded` | Add the missing guard / run bounded Monitor (replay) |
| `frozen_only_invariant` | Run mutate to check kill-rate |
| `intent_unexercised` | Add a single-shot reachable for the action / raise `--depth` |
| `liveness_not_refined` | Re-prove liveness at each layer or add `preserve progress` to the refinement mapping |

| result / violation_kind | Meaning | Next move |
|---|---|---|
| `violated` / `invariant` | Counterexample found (trace is shortest) | Read the trace's `changes` and `violating_bindings`; add a guard or fix the invariant |
| `violated` / `trans` | Two-state safety counterexample found | Compare the trace's previous state with the violating step; decide between adding a guard, fixing the action, or fixing the trans |
| `violated` / `type_bound` | Bounded type out of range (automatic check) | Insufficient guard on `last_action`. Keep within range via `requires` (do not hand-write an invariant) |
| `violated` / `partial_op` | pop/head on an empty Seq, index out of range, or divisor 0 | Guard with `requires q.size() > 0` / `requires d != 0` or an `if` |
| `violated` / `ensures` | Postcondition not satisfied | Decide whether the body or the ensures is correct, and fix accordingly |
| `violated` / `leadsTo` | Response-property counterexample (lasso / stall) | Check the trace's `loop_start`. Either add `fair` to the action that drives progress, or fix the spec |
| `unknown_cti` / `leadsTo_rank` | Ranked response proof failed | Read `rank_failure`: `progress_action_not_fair` means `helpful` named a non-fair action; `helpful_action_not_enabled` means the matching progress action is blocked while P is pending; `non_decreasing_helpful_action` means the helpful action fires without lowering the measure; otherwise repair the rank or pending preservation |
| `reachable_failed` | A state you want to reach is unreachable | Read `action_coverage`'s `blocking_requires` (unsat core). Loosen a guard / add an action / increase `--depth` |
| `unknown_cti` | The invariant is true but not inductive | **The CTI's starting state = a phantom state satisfying all invariants. Add an auxiliary invariant (one that is a domain truth) that excludes it, then re-run.** Check `suggested_invariants` first — for the monotone-counter idiom the result carries ready-made candidate expressions. Track record: converges in one round (e.g. "no duplicates in the queue," "refunds only from Captured") |
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
| `refinement_failed` / `progress_lost` | A `preserve progress` mapping pulled an upper `leadsTo` into the lower layer and found a lasso/stall | Read `progress_failure`, `impl_trace`, `pending_since`, `loop_start`/`stutter`, and the `progress.actions`. Add/restore lower-layer `fair action` on the progress action, add a lower-layer ranked `leadsTo`, or revise the progress mapping |
| `implements.result: violated` within verify | The requirements layer deviates from the upper (business) layer | The contents of `implements.violation` have the same shape as refinement_failed. Same procedure as above + check the `requirement` on the requirements side |
| `error` / `acceptance` | Replay of an acceptance criterion failed | The ID and step of the failed AC are returned. Decide whether the procedure's precondition (state) or the expect is correct, and fix accordingly |
| `error` / `forbidden` | An operation sequence that should be rejected was accepted (under-constraint; the kind that a safety invariant stays silent about) | `accepted_trace` is the accepting path. The requires enabling the last operation is too loose → add a guard or review the spec |
| `error` / `forbidden_setup` | A precondition (non-final) step of the forbidden is not enabled (invalid trace) | Review the setup procedure. The non-final steps are there to reach that point and are not treated as success |
| `statistically_unsupported` / `dataset_invalid` / `evaluator_untrusted` / `insufficient_samples` (fsl-ai evidence commands) | External statistical/migration/drift evidence failed a gate — there is no kernel counterexample to read | Route by the status priority list in `docs/DESIGN-stochastic.md`: fix the evidence (records, calibration, sample size) or the component/rollout — not the spec, and do not expect a trace |

For an action whose coverage is `false`, `blocking_requires` pinpoints "which
requires is blocking it" on a per-clause basis, and `hint` summarizes the
blocking factors. Do not silently ignore it. For branches-split actions,
diagnostics keep the internal name (`submit__b1`) and add a human
`display_name` such as `submit[a <= AUTO_LIMIT]`.

Ordinary refinement still propagates safety, not liveness: safety refinement can
return `refines` while a lower-layer `leadsTo` fails. If the upper response must
be preserved at refine time, add to the mapping:

```fsl
preserve progress {
  respond EveryRequestHandled by answer, refuse, escalate
}
```

This checks the upper `leadsTo` after pulling it through the state mapping. A
failure is `refinement_failed / progress_lost`. The `by` actions are validated
impl action names and review metadata; they do not create fairness or prove
implementation conformance. The actual lasso exclusion still comes from
lower-layer `fair action` declarations.

When a counterexample makes you **change an interpretation** (added a guard,
loosened an invariant, decided how to handle an exception), record that judgment in
the assumption ledger (the `// ASSUME-n:` comments / tags in the `.fsl`) only after
the source material or the human confirms it. If the counterexample exposes a
missing requirement or design decision, ask instead of choosing the repair on the
user's behalf. The shortest path to verified is often "weakening the spec," so
without confirmation and a record of what was weakened and why, you later cannot
distinguish a hollowing-out repair from a legitimate fix. The mirror failure is
**over-constraining**: a guard added to fix a `forbidden`/`violated` can tighten
the action into a dead one. After such a fix, re-run `verify` and confirm the
repaired action's `action_coverage` is still `true` (and any affected `reachable`
still witnessed) — over-tightening surfaces as a *new* `reachable_failed` /
`covered:false`, not as a failure of the original fix.

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

This template uses `type X = lo..hi` throughout, the fastest path to a checkable
kernel spec. When the spec should also read as documentation, prefer `entity X` /
`number X` with the bound moved to a `verify { instances/values }` block instead:
`type Claim = 0..2` reads as a false domain fact ("there are only 3 claims"), while
`entity Claim` + `verify { instances Claim = 3 }` states a verification bound, not a
domain truth. See reference.md §10, "Authoring specs as readable documentation."

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
- For an **element-wise** property over a Seq in an invariant, prefer member
  quantification: `forall x in q { P(x) }` (no index arithmetic, nothing to get
  off-by-one). Keep the index-guard idiom — `forall i in 0..CAP-1 { i < q.size() =>
  P(q.at(i)) }` (range derived from the const, never a hard-coded literal) — only
  for properties about position, ordering, adjacency, or no-duplicates, where the
  index itself carries meaning. See reference.md §10, "Authoring specs as readable
  documentation."
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
  `terminal { }` also passes through unchanged at the `requirements` layer (write
  it against the synthesized `<entity>_stage` map when using `process`, e.g.
  `terminal { forall c: Case { case_stage[c] == Closed } }`); the `business`
  dialect needs no `terminal` syntax at all — it derives the predicate
  automatically from each process's sink stages (stages with no outgoing
  `transition`).

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
to the kernel, so verify/induction/scenarios/Monitor are used identically. This
section is the **per-layer syntax**; for driving the layers end-to-end with
`implements` / `refine` / `chain`, see the connected workflow above.

Treat the layer boundary as part of the contract — it is the refinement seam the
lower layer must honor. Do not move to the lower layer unless the user asks for it or
the relevant role skill directs it.

- `business Name { process/control/policy/kpi/goal }` — the consulting layer. For
  PM/consulting-facing files, prefer the readable stage syntax for common rules:
  `policy ... every Case in Source must eventually be Target [or Target ...]`,
  `policy ... every Case reaching Target [or Target ...] must have passed
  through Waypoint [or Waypoint ...]` (no-bypass; desugars to an invisible
  history flag + kernel invariant), `goal ... some Case can reach Target`, and
  `goal ... all Case can be Target [or Target ...]`. Use explicit
  `responds { forall ... stage(c) ... ~> ... }` / `{ expr }` only when the rule is
  not simple stage progression. Regulation contradiction = invariant violation,
  dead business step = coverage diagnostic, unreachable business goal =
  reachable_failed. Use `control ID "..."` for governance/catalog metadata and
  `policy/goal ... satisfies ControlID` for the actual checkable rule; violations
  then carry both the broken policy/goal and satisfied controls. A standalone
  `governance Name { ... }` catalog can require controls across business specs
  and run preservation refinements during `fslc check`.
- `requirements Name { process E with f: T {...} / kpi / acceptance /
  forbidden / implements Abs from "file" { } }` — the requirements layer. Use
  the process+data profile first for a single-entity lifecycle: transition
  clauses carry inputs (`with`), guards (`when`), field updates (`set`), and
  traceability (`covers`). Put verifier bounds in `verify { instances E = N
  values T = lo..hi }`. With `implements`, verify simultaneously runs the refine
  to the upper layer (the `implements` field in the result JSON); an empty body
  auto-generates identity refinement when names match, `maps auto` is allowed for
  same-name kernel-wrapper state/actions, and auto-mapped process transitions are
  actor-checked; the inline block also takes action-correspondence items
  (`action impl(..) -> abs(..) | stutter`), including an arity change, the same
  syntax a separate refinement file uses. `acceptance` is replay-checked at
  check time and supports `expect E id in Stage` as well as `expect <expr>`,
  then flows scenarios → testgen; action arguments in `acceptance`/`forbidden`
  accept enum member names as well as numeric ordinals. `forbidden` (must-forbid)
  conversely writes an "operation sequence that should be rejected" and
  verifies at check time that the last step is rejected (not-enabled or a
  violation) — if accepted, `kind: "forbidden"`. Carried fields (`f: T`) accept
  `number` (optional initializer, default `lo`), or `Bool`/enum (initializer
  required). Use kernel-wrapper `struct` / `state` / `init`, `fair action`,
  `branches`, and explicit `maps` only for hard cases such as multi-entity
  behavior, conservation rules, SLA/time, or history that needs kernel state.
  An independent channel for catching under-constraint (missing guards) that a
  safety invariant stays silent about (a receptacle for cross-validation where a
  separate agent writes positive/negative traces from NL)
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
- **Functional DDD / async effects**: `domain` specs lower aggregate state,
  command/decide/event/evolve, invariants, saga/process-manager steps, and finite
  async effect lifecycles to the kernel. Use `fslc domain check` for fsl-domain
  findings plus the nested kernel result, `fslc domain expand` to inspect the
  generated spec, `fslc domain generate --target typescript|python|kotlin|swift|rust`
  / `fslc domain testgen` for scaffolds, and `fslc domain replay --logs` for
  runtime command/event/effect evidence. Treat generated code as a scaffold; real
  gateway behavior, queue delivery, wall-clock timeouts, and production
  exactly-once semantics remain outside the proof boundary.
- **AI hard contracts**: `ai_component` specs lower tool authority / human
  approval / forbidden-tool guards to the kernel. Use `fslc ai check` for
  hard-contract findings and `fslc ai replay --logs` for runtime JSONL evidence;
  `replay_conformant` is not proof, and evaluator/statistical AI quality remains
  outside the kernel. If you add `check hard { rule ...; }`, name only the
  rules you mean to certify explicitly — omitting the block is the safe
  default (checks all 5); do not add it as a shortcut past a violation (full
  field list and the verified narrowing behavior: reference.md §1). For
  recursive `agent` review-gate coverage, declare `review_gate <Child>;` on
  every path that must pass human/policy review before reaching a
  high-authority tool — an undeclared path is a silent bypass, not a warning.
  For statistical/migration/drift evidence over precomputed
  eval JSONL (`dataset`/`evaluator`/`statistical_property`/`ai_migration`/
  `observed_property` project-level blocks), use `fslc ai eval`/`regress`/
  `compare`/`drift`/`compat` (syntax and flags in reference.md §1/§7); every
  result is `formal_result:"not_run"`, never `proved`.
- **Recursive AI agents**: `agent` specs are ordinary scoped agents nested inside
  parents, not `sub_agent`s. Use `fslc ai check` to get `agent_analyzed`,
  `agent_ir`, and graph summaries for explicit grants, visibility,
  orchestration/delegation, tool reachability, review-gate bypass, and
  failure_policy. This is structural evidence with `formal_result:"not_run"`.
- **Ghost types (typestate)**: `fslc typestate file.fsl [--ts]` — determines how
  far a state machine (a struct field with enum values / a state variable /
  an `Option<_>` slot) can be mapped onto the host language's typestate (derivable /
  branching / relational). If all transitions are typeable, applicability=full.
  `--ts` outputs a TypeScript skeleton for the derivable portion (reference.md §7)
