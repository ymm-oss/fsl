---
name: fsl-design-review
description: A procedure for design exploration and design review using FSL. Mechanically check with fslc refine whether a design proposal, variant, extension, or change breaks a stable contract (the abstract spec), and report the result in the vocabulary of design principles (SOLID — the Open/Closed Principle, the Liskov Substitution Principle, Design by Contract, etc.). Triggers include "explore this design", "do a design review", "does this follow SOLID", "is this extension/change safe", "I want to add a variant", and "is this substitutable". Use fsl-design instead when the task is to author the design-layer spec rather than review a proposal. FSL syntax is delegated to the fsl skill.
---

# FSL Design Exploration — A Procedure for Translating Design Decisions into Contract-Conformance Checks

The core is the following 5-step procedure. Design principles (SOLID etc.) are not
the starting point of the procedure; they appear as judgment lenses at each step.
This skill's job is to translate "is this design good?" into "does this design
conform to the frozen contract?" and have fslc answer with counterexamples.

**Prerequisite**: FSL syntax, the verification commands, and the repair protocol
follow the fsl skill (SKILL.md + reference.md). This skill supplies only the
procedure and the interpretation of results.
If the user needs a new design-layer `.fsl` written before review, use
`fsl-design` for authoring and return here for review/proposal judgment.
Working examples: `specs/bank.fsl` + `bank_impl.fsl` + `bank_refines.fsl`
(refinement), `seat_booking_*` (state-tag-dependent mappings),
`specs/bank_system.fsl` (compose).

## Procedure

### Step 1 — Translate the design question into the language of contracts

Design questions almost always fall into one of four shapes: comparing new design
proposals / adding a variant or extension / reviewing a change to an existing design
/ judging substitutability. In every shape, what you identify is the same:

- **The open part**: what may change (variants, extension points, the changed site)
- **The closed part = the contract**: what must not break
  (invariants, guards, externally observable state)

Lens — OCP: decide the "range you close" here. DIP: what abstraction should the
design proposals all depend on in common. Align with the user on the candidate
contracts as a bullet list before proceeding (do not create a deliverable file;
chat is fine).

Do not infer the open/closed boundary, extension points, substitutability contract,
variant roster, or action correspondence from general design taste. If the source
material does not say what must remain stable or how a detail action maps to an
abstract action, ask. A refinement proof is only meaningful for the contract the
human agreed to freeze.

### Step 2 — Freeze the contract as an abstract spec

After the contract boundary is confirmed, write the contract as an abstract `spec`
and push it through the fsl skill's standard workflow up to `check` → `verify` →
`--engine induction` (proved recommended). If the contract itself is contradictory,
the design exploration cannot even begin — a `violated` here is a defect in the
contract, not in a design proposal.

Lens — DbC: `requires`/`ensures`/`invariant` are the contract directly. Leave
defense of boundaries and partial operations to the automatic checks
(`type_bound`/`partial_op`); do not hand-write it.

**Discipline for the abstract layer (break it and the "closed" part collapses
later)**:
- Speak only about observable state and the contract. Do not bring in
  implementation-driven state.
- **Do not enumerate variants with an enum**. You will end up editing the abstract
  layer every time a variant is added. Keep the concrete type of a variant in the
  detail layer only, and fold it in with `if c then a else b` in the mapping.

### Step 3 — Write each design proposal as a refinement

Describe each design proposal / variant as a "detail `spec` + mapping file". Do not
edit the abstract spec's file. Correspond actions that exist only in the detail
layer to `stutter` only when they are confirmed not to affect observable abstract
state. When a correspondence is unclear, ask before writing the mapping. A
module-addition-style extension is `compose`, not refinement (synchronize/compose
without editing the existing files, and write the cross-cutting invariant in the
composite layer).

Lens — LSP: the content of the refine check (do not strengthen preconditions, do not
weaken postconditions, preserve invariants) is the definition of LSP itself.
SRP/ISP: these act as discipline for splitting a spec per concern and composing,
but **they are not checked** (fslc says nothing even if you violate them).

### Step 4 — Check, and translate the result into a design judgment

`fslc refine impl.fsl abs.fsl map.fsl` (+ `verify` each spec on its own). Also run
`verify --strict-tags` (traceability) and `explain --readable` (the doc rendering) —
standard practice for every review, not high-risk-only.
The mechanical repair steps for the result follow the fsl skill's repair protocol
table, but in design exploration you must always decide, before fixing, "**is the
design proposal bad, or is the contract excessive?**" — either conclusion is a
legitimate outcome of a design review. If the answer is not explicit in the source
or prior confirmation, present the counterexample and ask; do not silently change
the abstract contract, mapping, or detail guards to obtain a green result.

| Check result | Design meaning | Report in principle vocabulary |
|---|---|---|
| refine succeeds | This proposal can substitute/extend without breaking the contract | LSP satisfied. OCP's "open to extension" holds |
| `abs_requires_failed` | The detail layer bypasses an abstract-layer guard (a control shortcut, effectively strengthening a precondition) | LSP violation = not substitutable. `impl_trace` is the forbidden procedure |
| `abs_state_mismatch` | Observable behavior diverges from the contract | LSP violation (a postcondition-equivalent deviation). Also suspect a misreading of the mapping |
| `stutter_changed_abs` | An operation claimed to be "internal detail" causes an externally visible change | A break of encapsulation. The stutter declaration is a lie |
| coverage `false` | An action that can never fire = a dead extension point / dead procedure | Equivalent to YAGNI / dead code. Read `blocking_requires` |
| `unknown_cti` | The contract is true but requires an implicit premise | Discovery of an implicit design premise. If it is a domain truth, make it explicit as an auxiliary invariant |

**Non-vacuity audit (do not take green at face value)**: even
`verified`/`proved`/refine-success can be a hollow spec that **checks nothing of the
contract**. Especially when you drop a design into a state machine and carry contract
facts in ghost variables (situations that mechanically simulate a relation such as
refinement), the carrier ghost can become a "dead variable" and the invariant easily
turns vacuously true. For high-risk contracts:
- Look at `fslc mutate`'s kill-rate. If it is extremely low (rule of thumb <10%) it
  is hollow — being unable to kill mutations = the invariant is not constraining the
  dynamic state.
- `--vacuity` warns with `tautology_over_frozen` on "a tautological invariant that
  depends only on a frozen variable no action ever assigns to." But other hollow
  cases such as over-constraint and equivalent mutations are only visible with mutate.
- When you want to confirm the non-vacuity of a specific invariant, target it with
  `verify --property <Name>` (confirm that breaking one guard makes it violated =
  evidence that the invariant actually bites).

### Step 5 — Report proof and judgment separately

Do not mix the three categories in the report:

1. **What was proved** — contract conformance (LSP/OCP-equivalent). Attach the
   command and result
2. **Counterexamples** — translate the trace into design language and present it
   ("skip X and Y holds"). If there is a `requirement: {id, text}`, reconcile it with
   the requirement's intent
3. **What remains for human judgment** — the goodness of code structure such as
   SRP/ISP/DRY/Demeter, interface design, naming. FSL is not involved

**Do not write "proved SOLID compliance".** What you could prove is only contract
conformance.

**What you proved is that "the design (spec) is consistent with the contract", not
that "the implementation follows that design".** Even if verify/induction/refine are
green, that is consistency at the spec level, not a guarantee that the real code
honors that contract (what fslc guarantees is "the internal consistency of the spec
as written", not "whether the spec is faithful to reality"). If implementation
conformance is also required, **anchor** the abstract spec to the implementation:
`fslc replay` (run the real system's execution log through the spec for a conformance
check) or `fslc testgen` (generate an implementation-conformance pytest via an
Adapter). A spec with no anchor stays a "consistent model" and cannot detect when the
code diverges from it — make this distinction explicit in the report.

## Principle ↔ FSL mechanism correspondence (for reference)

| Principle | How FSL handles it | Strength |
|---|---|---|
| LSP (Liskov substitution) | The content of `fslc refine`'s check itself | Provable |
| OCP (open/closed) | abstract spec unedited + extend with detail spec / compose → conformance check with refine | Provable |
| Design by Contract (DbC) | requires / ensures / invariant are the language core | Native |
| DIP (dependency inversion) | The layer chain (business ⊒ requirements ⊒ design ⊒ implementation) where detail conforms to abstract | Embodied structurally |
| SRP / ISP | concern splitting via compose + visibility control with `internal` | Supported as discipline (not checkable) |
| DRY / KISS / Demeter | Matters of code structure that cannot be translated into FSL's vocabulary | Out of scope |

## Recommended practices (optional — by risk and importance)

- **Structural review before repair**: for AI-authored or heavily patched specs,
  run `fslc analyze <file> --profile ai-review` alongside `check`/`verify`.
  Findings such as `disconnected_requirement`, `unanchored_property`,
  `progressless_cycle`, `unread_state`, and `conservation_candidate` are review
  signals, not LSP/refinement failures. Use them to ask whether the contract has
  traceability holes, missing progress intent, dead state clusters, or implicit
  invariants worth proving before changing the spec. Do not weaken a contract
  merely to remove an
  `analyze` finding; the finding's `formal_status` is normally
  `not_a_violation`.
- **Variant matrix**: when there are multiple variants, push the refine of all
  variants × the abstract contract (a mechanical check that "all variants are
  substitutable"). Only for high-risk contracts.
- **Repurposing for change review**: refining with the pre-change spec as abs and the
  post-change spec as impl turns it into a check of "does this change preserve the old
  contract?" (a confirmation of behavioral compatibility).
- **Up to proved**: pushing through to induction rather than stopping at verified
  (bounded) is especially valuable in the abstract layer, where the contract should
  be stable for the long term.
