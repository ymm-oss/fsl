---
name: fsl
description: Shared FSL language and verifier reference for writing, checking, verifying, repairing, explaining, mutating, refining, replaying, generating scenarios/test scaffolds, and interpreting fslc JSON results. Use directly for FSL syntax, kernel specs, verifier errors, repair loops, and command usage. For role-specific authoring, prefer fsl-business for business flows, fsl-requirements for PM requirements/acceptance/NFR specs, and fsl-design for engineering design/refinement work.
---

# FSL Core — Language, Verifier, and Repair Loop

FSL is a language not present in training data. **Do not write from memory;
follow this guide.** Use the reference index below to read only the topical
material needed for the task. Within the repository, `docs/LANGUAGE.md` is the
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

## How to run

```bash
fslc <subcommand> ...            # authoritative native Rust CLI
```

The Python implementation is a frozen compatibility reference. New language/CLI
features are not backported to it. Use the native `fslc` binary for
`undecided:` syntax and its ledger/HTML/analyze output; do not route new work
through `python -m fslc`.

External compilers and generators must use `fslc kernel <spec>` rather than the
Python AST or source re-parsing. Validate independent implementations with
`fslc conformance <spec> --depth N`; versioning and rollback semantics are in
`docs/DESIGN-kernel-contract.md`.

Output is always a single JSON document on stdout. exit: 0=success
(verified/proved/generated/analyzed), 1=property not satisfied
(violated/reachable_failed/unknown_cti/nonconformant), 2=spec error
(parse/type/semantics/io), 3=internal error.

**The one exception is the inline `implements` seam.** A requirements spec with
`implements Abs from "business.fsl" { ... }` has its refinement to the upper
layer checked during `check`/`verify`, but that verdict is reported *only* in the
`implements` field — it is not folded into the top-level `result` or the exit
code. A broken business seam still returns `result: "ok"` / `"verified"` and
exit 0. Gate it explicitly on `implements.result == "refines"` (the only passing
value; the failing ones are `refinement_failed` and `impl_violated`), or run
`fslc chain`, which applies that gate for you and does exit 1.

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
- An assumption justifying a specific guard / invariant → tag that declaration
  with the canonical typed annotation:
  `@requirement("ASSUME-STOCK-001", "only one user reserves at a time")` on the
  line before `invariant OnePerUser { ... }`

This way assumptions travel with the spec, are visible in PRs, and a future
`--strict-tags` check can distinguish "intended assumptions (tagged)" from
"unfounded fabrications (untagged)."

### Preserve intentionally deferred decisions with `undecided:`

When the specification owner has explicitly decided to leave a choice open,
record that review decision on the declaration instead of inventing a guard or
property:

```fsl
init "undecided: initial operating mode will be selected at rollout" {
  mode = Manual
}
action route() "undecided: routing policy is pending owner approval" {
  ...
}
```

`undecided:` is metadata, not a verification condition. Verification still
checks every behavior allowed by the spec. `fslc ledger` and `fslc html` list
the declaration, reason, and state-dependency-derived affected requirement IDs.
`analyze --profile ai-review` keeps matching `divergent_choice` /
`unconstrained_effect` findings visible and marks them `acknowledged:true` with
`acknowledged_by`; an unmatched finding has no acknowledgement fields and stays
in the unresolved review queue.

Use this marker only after a human owner deliberately accepts the deferral. Do
not use it to hide an agent's uncertainty, a missing source requirement, or a
failed formalization guess. The declaration has one tag slot, so an
`undecided:` declaration cannot simultaneously carry an `ID: text` tag; reports
derive affected IDs from state dependencies. Full syntax and limits are in
the [syntax reference](references/syntax.md) and `docs/DESIGN-undecided.md`. This feature belongs to the
native Rust CLI and is intentionally not added to the frozen Python reference.

## Standard workflow (single spec; treat proved as the standard)

1. Write the spec → `fslc check file.fsl` (syntax and types only, fast; fix
   following the error's `loc`/`expected`/`hint`).
   When checking requirement traceability strictly, add `--strict-tags`
   (and `--requirements ids.txt` if needed). Only when the result is
   ok/verified/proved do untagged declarations and unreferenced requirement IDs
   become warnings.
   **The one canonical way to link a declaration to a requirement ID** is the
   typed annotation on the line before it —
   `@requirement("REQ-SCOPE-001", "one-sentence intent")` — with process
   `covers REQ-SCOPE-001 "..."` as the equivalent dialect sugar, and a
   `MODEL-`/`ASSUME-`prefixed id for modeling intent rather than a source
   requirement. The `invariant X "REQ-1: text" { ... }` string slot is
   non-canonical migration input (`docs/DESIGN-id-policy.md`); `--strict-tags`
   still counts it as tagged, so add `fslc lint file.fsl`, which exits 1 with
   `legacy_string_metadata` plus a machine-applicable replacement.
2. `fslc verify file.fsl --depth 8` → see the [repair protocol](references/errors.md)
   for what each result means
   To ask a bounded operational what-if from a complete `Monitor.state` JSON,
   use `fslc verify file.fsl --from-state state.json --depth 8`; this replaces
   `init`, is BMC-only, and is stamped `bounded_from_snapshot`.
3. Once verified, run `fslc verify file.fsl --engine induction` → done at `proved`
   (note: `--depth K` **includes** step K. Invariants become infinite-depth under
   `proved`; `leadsTo` remains bounded unless it declares `decreases <int expr>`,
   in which case induction can prove that response with an unbounded ranking
   argument). If induction returns `unknown_cti`, candidate auxiliary invariants
   may be machine-judged with repeatable `--lemma "EXPR"`: only independently
   `proved` candidates are used, rejected candidates retain counterexample/CTI
   evidence, and successful output recommends declarations to write back. Never
   treat a candidate as an assumption without this adjudication.
## Minimal syntax (details and the full catalog are in the [syntax reference](references/syntax.md))

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
domain truth. See [layers](references/layers.md), "Authoring specs as readable documentation."

## Rules to always follow (structural pitfalls)

- **No sentinel values (-1, etc.) → use `Option<T>`**. Use structural
  `==` / `!=` when comparing complete Option values; use `x is some(v)` when
  the payload must be bound for a following expression.
- **Do not hand-write "non-negative"-style invariants** → `type Qty = 0..N` checks
  them automatically.
- A **double assignment on the same execution path is an error**. Assigning to the
  same variable after an if as inside a branch is also an error. **Proven duplicate
  writes** keep the legacy duplicate-write message; **conservative rejections** for
  unproved `forall` index distinctness report
  `FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED` with `loc` and a safe `hint` when one
  exists (acceptance is not widened).
- Updates to Set/Seq are **re-assignments**: `s = s.add(x)`, `q = q.pop()`.
- Seq `pop/head/at` and the divisor of `/` `%` **must always be guarded** (requires
  or if). Forgetting is detected as partial_op.
- For an **element-wise** property over a Seq in an invariant, prefer member
  quantification: `forall x in q { P(x) }` (no index arithmetic, nothing to get
  off-by-one). Keep the index-guard idiom — `forall i in 0..CAP-1 { i < q.size() =>
  P(q.at(i)) }` (range derived from the const, never a hard-coded literal) — only
  for properties about position, ordering, adjacency, or no-duplicates, where the
  index itself carries meaning. See [layers](references/layers.md), "Authoring specs as readable
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

## Role-specific authoring entry points

When the task starts from role language rather than raw FSL syntax, use the role
skill first. This prevents business, requirements, and design decisions from being
mixed in one spec.

| Role / intent | Use skill | Examples to read | Constructs mainly written |
|---|---|---|
| Consultant (business flows, regulations, As-Is/To-Be) | `fsl-business` | `examples/consulting/`, `examples/pm/cancel_flow.fsl` | `business` ([layers](references/layers.md)) |
| PM / PdM (requirement definition, acceptance criteria) | `fsl-requirements` | `examples/pm/`, `examples/e2e/2_requirements.fsl` | `requirements` and NFR ([layers](references/layers.md)) |
| Engineer (design, implementation connection) | `fsl-design` | `examples/e2e/`, `examples/bank/` | kernel `spec` + refine mapping + Adapter ([impl](references/impl.md)) |

## Reference index

Read only the reference that answers the current question; do not load the
collection preemptively.

- [errors](references/errors.md) — interpret `fslc` results and choose the next repair move.
- [when-to-use](references/when-to-use.md) — decide whether FSL is the right tool for the problem.
- [commands](references/commands.md) — use extended verifier commands and interpret their JSON contracts.
- [layers](references/layers.md) — work across business, requirements, and design layers, including NFRs.
- [syntax](references/syntax.md) — look up the language grammar, types, expressions, semantic rules, checks, and idioms.
- [advanced](references/advanced.md) — use DB, domain, AI, and typestate features.
- [nl-to-syntax](references/nl-to-syntax.md) — map confirmed natural-language requirements to FSL constructs.
- [practices](references/practices.md) — apply optional authoring practices appropriate to the task's risk.
- [impl](references/impl.md) — connect specifications to implementations and perform causal review.

The flagship example threading all three roles through one domain is
`examples/e2e/` (expense reimbursement).

Once a `requirements`/`spec` dialect file is authored and checked, generating a
stakeholder-facing requirements document from it (rather than authoring or
repairing the `.fsl` itself) is a separate downstream activity — use
`fsl-requirements-document` for that (`fslc document generate`/`claims`/`check`).
It edits only the document's non-normative slot and never the FSL.
