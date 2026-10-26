---
name: fsl-requirements
description: Write and verify FSL requirements-layer specs for PMs/PdMs from natural-language product requirements, acceptance criteria, forbidden flows, requirement IDs, traceability, and NFRs such as permissions, audit, capacity, reliability, and SLA/deadline behavior. Use when the deliverable is a requirements specification, not business process discovery, software architecture, internal design, or implementation conformance.
---

# FSL Requirements Layer

Use this skill to formalize PM/PdM requirements as a `requirements Name { ... }`
FSL file. The deliverable is a requirements contract that development can consume:
requirement IDs, source text, operations, guards, acceptance criteria, forbidden
flows, and optional conformance to an upper business layer.

Before writing syntax, read `../fsl/SKILL.md`, then use its reference index to
load only the needed language and verifier detail. The complete requirements
example (process+data profile, `implements` of a business spec, `covers`
traceability, `kpi`, `acceptance`, and the `verify { instances / values }`
bound) is in `../fsl/references/layers.md`, section "requirements (the
requirements layer)". Read the NFR section of `../fsl/references/impl.md` only
when SLA/deadline behavior is in scope.

## Faithfulness gate (definition of done)

The two failures this layer exists to prevent are **dropping** a requirement (a
thin spec) and **inventing** a rule the source never stated. Hold both ends:

- **Cover the source.** Every requirement in the document maps to at least one
  transition / invariant / acceptance / forbidden, or is recorded in the memo as
  out-of-scope or needs-decision. The step-1 coverage map forces the hard
  requirements in instead of stopping at the happy path.
- **Ground the spec.** Every declaration carries provenance — process
  `covers REQ-CHECKOUT-001 "..."` to source text, or the typed annotation
  `@requirement("REQ-CHECKOUT-001", "...")` on a declaration that links to an ID
  a `requirement` block owns, with a `MODEL-`/`ASSUME-`prefixed id for an
  explicit modeling choice. Those two are the canonical forms; the
  `"REQ-1: text"` string slot is non-canonical
  migration input, so never write it. An untagged declaration is an ungrounded
  guess, not a style nit.
- **Do not invent to reach green.** If `check`/`verify` needs a guard, bound,
  state, actor, deadline, or exception the source does not state, that is a step-1
  question for the human — not a default you may pick to reach `verified`.
- **Make it mechanical.** `fslc check <file> --strict-tags` flags every `untagged`
  declaration (an invented rule) and every `unreferenced_requirement` (a
  requirement nothing checks); **done = zero of both**. Add `--requirements ids.txt`
  (the memo's source IDs in a file, one per line — not a comma list) to also
  catch requirements omitted entirely. Keep
  provenance in the `.fsl` as tags, not a separate file.

## Boundary

Produce requirements-layer artifacts only:

- A `requirements` spec, preferably using the process+data profile:
  `process Entity with field: Type { transition ... with/when/set/covers ... }`
- `acceptance` and `forbidden` blocks for positive and negative flows
- Optional `implements BusinessName from "business.fsl" { }`, `maps auto`, or
  explicit maps when a confirmed business layer exists
- Scenario/test skeleton output from `fslc scenarios` when requested

Do not write internal design state, architecture, queues/outboxes, service modules,
database schemas, implementation adapters, or design-to-requirements refinement
maps unless the user explicitly moves to design work. If a requirement implies a
design decision but does not state it, ask rather than choosing.

Once this spec is checked, generating a stakeholder-facing requirements document
from it is a separate downstream activity, not part of this skill — use
`fsl-requirements-document` (`fslc document generate`/`claims`/`check`) for that.
It edits only the document's non-normative slot; it never edits the `.fsl` this
skill produced.

## Workflow

1. Post a requirements formalization memo in chat before writing `.fsl`:
   - glossary: externally visible entities, states, actors, operations, enums
   - requirement normalization: trigger, constraint, exception, boundary
     implication for each requirement
   - coverage map: every source requirement ID → the FSL element that will carry
     it; mark anything you cannot cover out-of-scope or needs-decision (this is
     what keeps the spec from silently dropping requirements)
   - acceptance and forbidden traces, with expected outcome
   - upper business contract, if any, and candidate `implements` mapping
   - assumptions and questions that affect behavior
2. Preserve source fidelity. Requirement IDs and verbatim text belong in
   `covers REQ-CHECKOUT-001 "..."` on process transitions, or in a
   `requirement REQ-CHECKOUT-001 "..."` block that owns the ID for kernel-wrapper
   declarations, so diagnostics and scenarios trace back to the PM document. Any
   other declaration links to an owned ID with the typed annotation
   `@requirement("REQ-CHECKOUT-001", "...")` — and one carrying a
   `MODEL-`/`ASSUME-`prefixed id when it is not a direct source requirement, so
   the strict-tags gate can tell intended modeling from fabrication. Do not use
   the non-canonical `"REQ-1: text"` string slot.
3. Encode only externally observable product behavior. State names should describe
   product/system states, not engineering mechanisms.
4. Use the process+data profile first for a single-entity lifecycle:
   `number Amount`, `process Claim with amount: Amount`, transition `with`
   inputs, `when` guards, `set` field updates, and `covers` traceability. Put
   finite bounds in a top-level `verify` block with `instances Claim = N` and
   `values Amount = lo..hi`. Carried fields also accept `Bool` and enum types
   declared in the spec: `number` keeps an optional initializer
   (`f: Amount = <const-expr>`, defaults to the domain's `lo`), `Bool`/enum
   fields require one (`retried: Bool = false`, `f: T = Member`) — this covers
   simple history flags without routing to kernel-wrapper.
5. Use the kernel-wrapper form (`struct` / `state` / `init`, `fair action`,
   `branches`, explicit `maps`) only for hard cases: multi-entity behavior,
   conservation rules, SLA/time, history that needs kernel state (multiple
   entities, sequences, etc. — a simple flag fits in a carried field, see step 4),
   or a user-visible operation whose data-dependent outcomes need separate upper
   correspondences.
6. Put positive examples in `acceptance`; put must-reject procedures in
   `forbidden`. Use `forbidden` for missing-guard risk because a safety invariant
   often stays silent when an invalid operation is accepted. Prefer
   `expect Entity id in Stage` for process-stage checks; use `expect <expr>` for
   other predicates. `acceptance`/`forbidden` action arguments accept enum member
   names (not just numeric ordinals); an action parameter may also be typed
   `Bool` (a first-class boolean, not a 0/1 int — `Int` is still rejected as
   unbounded).
7. For `implements`:
   - `implements BusinessName from "business.fsl" { }` auto-generates identity
     refinement when process/action/stage names match
   - `maps auto` is allowed inside `implements` for same-name kernel-wrapper
     state/actions; explicit `map` and action correspondences override it
   - whenever the seam is evaluated (not in a `verify` scoped with
     `--property` and the like), a process transition auto-mapped to a
     same-named business action is actor-checked; a requirements transition
     whose actor differs from that business action's actor is a check-time
      error even with covers (its text is not an actor declaration; #1441).
      A transition with no same-named business action maps to `stutter`
      and is not actor-checked
   - the inline block also accepts action correspondence items —
     `action impl(...) -> abs(...) | stutter` — including an arity change
     between impl and abs params, the same syntax a separate refinement file uses
8. For NFRs:
   - permissions: role guard plus invariant when needed
   - audit/capacity/reliability: ordinary invariants and response properties
   - SLA/deadline: use requirements `time` and `deadline`; confirm non-vacuity by
     checking that a tighter deadline fails
9. Run `fslc check --strict-tags` as the faithfulness gate — not done until it
   reports zero `untagged` and zero `unreferenced_requirement` (add
   `--requirements ids.txt` — the memo's IDs in a file, one per line, not a comma
   list — to also catch fully omitted requirements).
   `--strict-tags` counts a legacy `"REQ-1: text"` string as tagged, so also run
   `fslc lint <file>`: it exits 1 with `legacy_string_metadata` and a
   machine-applicable `@requirement(...)` replacement for every non-canonical
   form, and with `non_canonical_id` for an ID outside the active policy.
   Not done until `fslc lint` also reports no findings.
   `--strict-tags` findings are warnings on an exit-0 `ok` result, so read
   `warnings[]`, not only the exit code.
   Then `fslc verify --depth 8` (read `implements.result`), and for stable
   requirements `fslc verify --engine induction`. Run `fslc scenarios` to hand
   development the acceptance/forbidden skeletons. Reading those results,
   repairing from them, measuring strength with `mutate`, and the completion
   report format are in `../fsl-verify/SKILL.md`.

## Guardrails

- Completeness is part of the contract: a spec that formalizes only some
  requirements is not "minimal," it is unfinished. Cover every source requirement
  or record why not (out-of-scope / needs-decision) — but never close a gap by
  inventing a rule the source does not state.
- Carry every rejection rule the source states in a checked declaration, not
  in prose alone. Put the rejecting condition on the operation it restricts —
  the transition's source stage, a `when` guard, or a kernel action's
  `requires`, whichever expresses it — and add a `forbidden` trace that
  exercises the rejection (Workflow step 6). A rule that lives only in
  `requirement` block text, `covers` / `@requirement` text, or a `//` comment
  is not checked by `verify`, `mutate`, or conformance, and does not reach a
  reader or tool that takes the declarations without the prose. An omitted
  rejecting condition is not a neutral gap: the model then accepts every call
  the operation's remaining conditions allow, which can include calls the
  source rejects. If an invariant or a `forbidden` trace catches that,
  `verify` fails — add the condition the source states (or ask, if it states
  none) rather than deleting the invariant or the trace to get green. If
  nothing catches it, `verify` stays green, and scenarios, generated tests,
  and conformance vectors can then encode the acceptance as expected behavior.
- Stop at the requirements contract. Do not continue into design just because a
  design could be inferred. The handoff downward is a **refinement seam, not a plain
  baton pass**: when design work begins, this spec becomes the frozen contract the
  design layer refines via `fslc refine`, so author it as a contract a lower layer can
  honor. Upward, `implements BusinessName` is the same kind of seam to the business
  layer — `verify` checks it and reports it under `implements` in the result JSON.
- Do not add retries, storage rules, asynchronous jobs, queues, screen internals,
  or API shape unless the source explicitly requires them.
- A green `implements` result means the requirements contract conforms to the
  agreed business layer. It does not prove any design or implementation. Green
  means `implements.result == "refines"` in the result JSON. A
  `refinement_failed` / `impl_violated` seam makes `check`/`verify` exit 1 with
  that value at the top level. Assert the field, or gate with `fslc chain`.
- When a counterexample suggests a repair, decide whether it is a missing
  requirement, a wrong interpretation, or an intentional exception before changing
  guards or invariants.
