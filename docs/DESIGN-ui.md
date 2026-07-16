# FSL — fsl-ui (screen-transition / UI-state dialect): spike findings and design

issue #9. Exploration of a design-family dialect. **The scope is limited to interaction
design (screen transitions / UI state)** (visual design has no transition-system
semantics and cannot be checked by the kernel, so it is out of scope. Consistent with
the DESIGN-layers principle). This document covers the findings of the spike (handwrite
in plain fsl → verify → refine into the requirements layer) and a proposed expansion
rule set if we go. The running assets are in `examples/ui_spike/`.

## Spike conclusion: technical feasibility confirmed (conditional GO)

A returns-domain application screen flow (Form → Submitting → ReadyToPay/MgrPending →
Done/Error) was **handwritten in plain fsl, and both verification and refinement into
the requirements layer passed**. No change to the kernel semantics is needed, and fsl-ui
looks viable as an AST expansion (syntactic sugar).

## What was confirmed

### 1. Plain fsl fully expresses screen flows (zero new semantics)

| Design question | Kernel feature | Spike result |
|---|---|---|
| Are all screens reachable (dead screens)? | `reachable` | CanDone/CanError/CanMgr all witnessed |
| No dead ends / infinite loading? | `leadsTo` | `SubmitResolves: Submitting ~> not Submitting` proved |
| Consistency of guarded transitions | `requires` | submit impossible with an empty form, etc. |
| Double-submit prevention | `invariant` | `submitting => screen == Submitting` proved |
| Screen = state / transition = operation | `enum` / `action` | as-is |

`ReturnUI` is **verified + proved (k=1)**.

### 2. The UI flow refines the requirements layer (architecture verification)

`fsl-req ⊒ fsl-ui` (running alongside as a sibling of the design layer) holds
mechanically. The UI flow (impl) **refines** the requirements essence (abs):
- UI-only steps (enter_amount/submit/resp_mgr/resp_error/retry) → `stutter`
- Steps that commit the domain (resp_auto/mgr_approved → approve, pay → pay) →
  requirement actions

This demonstrates that #9's core value — "does a screen path exist for the acceptance-
criteria step sequence of a requirement?" — can be guaranteed by a mechanical check
(it can detect, with a counterexample, "the requirement is defined but the UI has no
path to it").

## Pitfalls found (things the expander should absorb)

### F-UI-1 (bug, fixed): refinement's 0-argument abstract action mapping

`action pay() -> pay()` (0-arg impl → 0-arg abstract) failed with a spurious
`expects 0 arguments` error. The cause was that Lark's `maybe_placeholders` turns empty
parentheses into `(None,)`, counting it as one argument. It went undetected because
existing refinement mapped all 0-arg impls to `stutter`. Fixed by stripping None in
`mapped_action_target` / `req_mapped_action_target` in `grammar.py` (a byproduct of this
spike).

### F-UI-2: gate draft (form-input) state with the mapping

Directly wiring `map amt = amount` leaks form input (uncommitted) into the abstract view
(`stutter_changed_abs`). A state-tag mapping that **makes it visible only on committed
screens** is needed:
`map amt = if screen == ReadyToPay or screen == Done then amount else 0` (isomorphic to
seat_booking). The "draft vs. confirmed" distinction characteristic of UI flows.

### F-UI-3 (important): enum collision between screen names and domain state names

If the UI's `Screen.Paid` and the requirement's `St.Paid` share a name, the right-hand
side of the mapping expression `if screen == Paid then Paid` resolves to the impl-side
enum, giving `abs_state_mismatch`. **Screen names easily clash with domain state names**,
so this is a frequent hazard for fsl-ui. In the spike it was avoided by renaming to
`Done`.
→ The expander should namespace the screen enum (e.g., `ui_Screen`) to structurally
prevent the collision.

### F-UI-4: the back stack is Map+depth, not Seq

For back (LIFO), `Seq<T,N>` is FIFO and unsuitable. It can be expressed with the
`Map<Depth,Screen> + depth` idiom (`NavStack` is verified); full is a guard/type_bound on
depth, and empty back is `requires depth > 0`. Note that because of simultaneous
assignment, `cur = hist[depth - 1]` reads the old value of depth. No new kernel is needed,
but **the expander should generate this idiom** (handwriting is cumbersome).

## Proposed expansion rules (if we go, `expand_ui`, isomorphic to compose/expand_business)

| Dialect syntax (proposed) | Kernel expansion |
|---|---|
| `screen S { A, B, ... }` | `enum ui_S { A, ... }` (namespaced, F-UI-3 countermeasure) + `state { screen: ui_S }` |
| `navigate <act> A -> B [requires …]` | `action <act> { requires screen == A … screen = B }` |
| `back` (when valid) | generate the `Map<Depth,Screen> + depth` idiom (F-UI-4) |
| `modal M over A { … }` | sub-screen enum + return transition |
| `loading`/`async` state | struct/product of screen × async state (how to apply the sugar needs further study) |
| `implements <Req> from "…" { map … }` | auto-generate refinement into the requirements layer (absorbing F-UI-2/3 via mapping generation) |

## What this layer does not handle (outside FSL)

Visual design (color, typography, layout, aesthetic judgment), usability, animation.
Things without transition-system semantics are documented in each layer's docs (the
fundamental principle of not expanding into the kernel).

## How to proceed / go-no-go

- The technical risk has been eliminated by the spike (expressible, refinable, no new
  kernel needed).
- The remaining design work is the **back-stack sugar** and the **namespacing of the
  screen enum** (F-UI-3/4, both identified).
- The intended users are design engineers / handoff verification, or **an AI
  transcribing a Figma flow diagram into fsl-ui and checking it** (the AI-Native
  position). Designers writing it directly is unrealistic.
- **Decision**: go if there is demand for UI-flow checking (`expand_ui` + tests +
  field validation). If demand is thin, the operation of "write screen flows in plain fsl" (the
  spike's ReturnUI as a template) delivers enough value for now. **The F-UI-1 fix is
  useful independently of the dialect, so it has already been merged ahead.**

## Shipped increment: the spec-level `kind` tag (Tier 1, not the dialect)

Between "write screen flows in plain fsl" (the ReturnUI template) and a full
`expand_ui` dialect there is a much cheaper middle purchase, now implemented: an
**optional spec-level tag** classifying the whole spec.

```fsl
spec ReturnUI "ui: return-request screen flow (behavioral slice only)" { … }
```

- **What it buys**: the "this is a UI spec" recognition — for a *human reader* (a
  leading, typed, in-header cue, and a badge in `fslc explain --readable` / `fslc html`)
  and for a *machine* (`skeleton.spec_kind = {id, text}` in the explain JSON is a
  read-off-able ontology classification). This is the spec-unit version of the
  screen/navigate ontology, without the lossy desugaring.
- **What it deliberately does not do**: no `screen`/`navigate`/`modal` line-level
  vocabulary (that is Tier 3, the `expand_ui` dialect, gated on demand — it is where
  cost jumps discontinuously and the F-UI-2/3/4 expander risk lives).
- **Cost / faithfulness**: zero kernel semantics (the tag desugars to nothing — the
  same invariant principle below), so the corpus snapshot is unchanged. The tag text is
  metadata only and never verified; keep the parenthetical scope note (`behavioral slice
  only`) so heightened recognition does not outrun what FSL actually models.
- **Files it moved**: `grammar.py` (optional `meta_tag?` on `spec_def`), `model.py`
  (`spec["kind"]`), `explain.py` (`skeleton.spec_kind` + readable line), `html_report.py`
  (title badge), `docs/LANGUAGE.md`, `skills/fsl/reference.md`, this note, and a
  regression test. The surfaced field remains `spec_kind` to distinguish spec metadata
  from the established diagnostic `kind`; faithfulness routing is provenance-scoped and
  does not interpret arbitrary nested payload keys as diagnostics.

## Invariant principle

Do not change the kernel semantics. Dialects are AST expansion only. Things that cannot
be expressed are documented in the layer's docs.
