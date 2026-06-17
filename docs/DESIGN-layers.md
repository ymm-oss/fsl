# FSL v3 — Shared Kernel + Three-Dialect Architecture Design

Use FSL across the three layers of consulting (business), requirements
definition, and design/implementation, connecting the layers transparently.
Conclusion: **feasible. The kernel already exists, and the backbone of the three
layers (the refinement chain) works with the current fslc** (demonstrated by the
spike in §2). What is needed is only a dialect frontend that gives each layer its
vocabulary, plus traceability metadata plumbing.

## 1. Architecture

```
 Dialect 1: fsl-biz (consulting)   Dialect 2: fsl-req (requirements)   Dialect 3: fsl (design; current)
   actor/process/stage/        requirement/usecase/      spec/state/action/
   policy/kpi/handoff          acceptance/actor          invariant/...
        │ expand (AST transform)    │ expand (AST transform)   │ as-is
        ▼                          ▼                         ▼
 ┌───────────────────────────── shared kernel ─────────────────────────────┐
 │ bounded transition system + invariant / reachable / leadsTo (+fair) + automatic checks │
 │ BMC / k-induction / unsat core diagnosis / scenarios / refinement / compose     │
 │ JSON repair protocol / concrete Monitor / replay / testgen                  │
 └────────────────────────────────────────────────────────────────────────┘
        ▲                          ▲                         ▲
        └── refinement ────────────┴── refinement ───────────┘
            (business ⊒ requirements)   (requirements ⊒ design)   + testgen/replay → implementation
```

- **The kernel = the semantics of the current fslc itself**. No new verification feature is needed.
- **A dialect = an AST transform in the frontend**. Isomorphic to the pattern
  proven by compose (`expand_compose`): after expansion it is an ordinary kernel
  spec, so BMC, induction, scenarios, Monitor, and refine all apply to every
  dialect **without modification**.
- **Inter-layer connection = the refinement chain**. The business layer is
  proved first, the requirements layer refines the business layer, the design
  layer refines the requirements layer, and the implementation conforms to the
  design layer via testgen/replay. The verification results for each layer's
  **safety** (invariants, control guards, inclusion of observable behavior)
  propagate downward to the lower layer as "fidelity."
  **However, liveness (leadsTo/responds) does not propagate** — because
  refinement permits stutter, even if a lower layer halts the progress that
  guaranteed an upper-layer response property, it can remain a faithful
  refinement. Liveness policies are re-verified at each layer (see the note in §6).

## 2. Proof spike (two-layer connection on the current kernel)

Carried out on the return-approval domain (2026-06-11, unmodified fslc v2.x):

- **Consulting layer** `ReturnPolicy`: business stages (Requested→Approved/Rejected→
  Refunded) + two policies (an accounting-consistency invariant and a "every
  request is always adjudicated" leadsTo) → **proved**
- **Requirements layer** `ReturnSystem`: adds amounts, an auto-approval threshold,
  and a manager-approval queue → **proved**
- **Inter-layer** `SystemRefinesPolicy`: **refines** via a nested conditional
  enum→enum mapping (`if st == New then Requested else if ...`). Auto-approval
  corresponds to the business "approval," and queue insertion corresponds to stutter.

Inputs to the dialect design obtained from the spike:
- **(L1) conditional action correspondence is required**: "depending on the
  amount, submit performs the business approve or nothing happens" was expressed
  in the current version by an **action split** into submit_small/submit_large.
  The req dialect's expander automates this split (the `branches` of §4.2).
- **(L2) the business vocabulary maps straight onto the kernel**: process=enum+Map,
  policy=invariant/leadsTo, actor=domain type, KPI=ghost counter. Not a single
  new semantic was needed.

## 3. Dialect 1: fsl-biz (consulting)

Target artifacts: business process definitions, policies (business rules),
As-Is/To-Be comparison, process soundness checking (machine detection of "this
state is unreachable under this regulation").

```fsl-biz
business ReturnHandling {
  actor Customer, Manager
  entity Return                            // business entity (verification size below)

  process Return {                         // → enum Stage + Map<Entity, Stage>
    stage Requested -> Approved  by Manager : approve
    stage Requested -> Rejected  by Manager : reject
    stage Approved  -> Refunded  by System  : refund
  }

  kpi refunded counts Return in Refunded   // → ghost counter + consistency invariant

  policy NoRefundWithoutApproval invariant { ... }   // the expression is a kernel expression
  policy EveryRequestDecided responds {              // → leadsTo + fair
    Return in Requested ~> Return in Approved or Rejected
  }
}

verify {
  instances Return = 3
}
```

Expansion rules: `process` → enum + `Map<CaseId, Stage>` + an action per
transition (`by <actor>` is action metadata; a transition whose actor has a
parameter becomes a parameter of the actor type). `kpi ... counts` → Int ghost +
automatic invariant `kpi == count(...)`. `responds` → fair + leadsTo.

**What this layer does not handle (stated explicitly)**: real time, SLA time
values, probability, continuous quantities of money, org charts, and the prose
parts of documents. FSL carries "the checkable skeleton of consulting artifacts"
and does not replace the documents.

Consulting value (a restatement of kernel features): regulatory contradiction =
invariant violation, dead process step = action coverage false + the unsat core
of the blocking regulation, unreachable business goal = reachable_failed,
neglected cases = leadsTo counterexample, As-Is/To-Be consistency = refinement.

## 4. Dialect 2: fsl-req (requirements definition)

Target artifacts: requirements (ID + original text + formalization), use cases,
acceptance criteria.

### 4.1 Syntax

```fsl-req
requirements ReturnSystemReq {
  implements ReturnHandling from "return_policy.fslb"   // refinement declaration against the upper layer

  actor Customer, Manager
  id Case = 0..2
  value Amount = 0..3
  const AUTO_LIMIT = 1

  requirement REQ-1 "returns at or below the threshold are auto-approved" {
    action submit(c: Case, a: Amount) by Customer {
      requires state(c) == New
      requires a > 0
      branches {                                  // ← L1: automatic split for conditional branching
        when a <= AUTO_LIMIT -> AutoApproved  maps approve(c)
        when a >  AUTO_LIMIT -> MgrQueue      maps stutter
      }
    }
  }

  requirement REQ-3 "every request is eventually adjudicated" responds { ... }

  acceptance AC-1 "small amounts are approved immediately" {
    submit(0, 1)  expect state(0) == AutoApproved
  }
}
```

### 4.2 Expansion rules

- `requirement` block → **attach `req_id` / `req_text` metadata** to the
  contained kernel elements (action/invariant/leadsTo). All JSON output
  (violated / unknown_cti / coverage diagnostics / scenarios) carries
  `requirement: {id, text}` — "which requirement broke" appears in the
  counterexample together with the original text (§6).
- `branches` → automatic split into multiple actions with the when condition
  added to requires (`submit__1`, `submit__2`; displayed as
  `submit[a<=AUTO_LIMIT]`). The action correspondence of the refinement mapping
  to the upper layer is **auto-generated** from the `maps` clauses.
- `implements ... from` → synthesize a refinement-file equivalent from the state
  mapping (the `maps` clauses and stage-correspondence declarations), and at
  `fslc verify` time **also run the refine check against the upper layer**
  (the check result JSON has `refines_upper: true/false`).
- `acceptance` → a **fixed scenario** with known steps + expect. Checked via the
  replay mechanism, and it also enters the scenarios output as-is (= the
  acceptance test flows into downstream testgen and becomes a conformance test
  for the implementation).

**What this layer does not handle**: (description at the time of writing —
since then DESIGN-nfr.md has added support for authorization, audit, capacity,
reliability behavior, and discrete-time SLAs. What remains out of scope is
probability, percentiles, real-time ms, and usability). Only what in requirement
documents can be reduced to state and behavior is formalized.

## 5. Dialect 3: fsl (design; current)

The current language as-is. `fslc refine` against the requirements layer, and
connect to the implementation via testgen/replay/Monitor (all implemented).

## 6. The three mechanisms of transparent connection

1. **Refinement chain** (proven): business ⊒ requirements ⊒ design. A violation
   displays which layer's transition broke which upper correspondence, **in the
   upper layer's vocabulary** (`abs_before/after` come out as business stage
   names — an existing display mechanism).

   **[Note] what propagates / what does not**: what refinement (the content
   checked by `fslc refine`) guarantees is **inclusion of safety** — that the
   upper invariants and guards (controls) are not broken in the lower layer
   either. This propagates downward. **Liveness (leadsTo/responds) does not
   propagate**: because refinement permits stutter (an internal step where the
   lower layer does not change the upper state), even if the lower layer drops
   the progress that the upper layer guaranteed via `fair`, refine still passes
   (the mapping does not require fair annotations). Therefore the business
   leadsTo "every request is always adjudicated" of the §2 spike is **not
   inherited automatically** even when the design layer refines the business
   layer. Remedies: (a) `verify` liveness policies individually at each layer
   (put `fair` on the lower action that bears the progress), or (b) require
   fairness preservation by mapping convention. This is a general property of
   forward simulation (safety is preserved, liveness is not), and is not an fslc
   defect. Concrete example: a design that, after placing the submission on an
   internal queue, keeps spinning a stutter loop instead of a non-fair
   adjudication passes refine but breaks the business leadsTo (`verify` produces
   a leadsTo counterexample = a lasso).
2. **Traceability metadata** (new; plumbing only): put `req_id`/`policy_id` on
   nodes of the kernel AST and pass them through into all JSON output. From the
   design layer's counterexample, "violates REQ-1 (original text)" comes out
   directly. A cross-cutting query `fslc trace REQ-1` (which element in which
   layer derives from REQ-1) is also generated from the same metadata.
3. **Downward flow of artifacts**: business-layer leadsTo → a template for the
   requirements-layer respond requirement, requirements-layer acceptance →
   design-layer scenarios → implementation testgen. The reverse direction is the
   upward display of counterexamples (annotating a design-layer CTI with the
   requirement ID).

## 7. Manifest-driven chain command

`fslc chain [fsl-project.toml]` runs the project layer pipeline in order and
returns one consolidated report. The human status table is written to stderr;
the machine-readable JSON envelope is written to stdout and contains one
`layers[]` entry per executed or skipped layer. The top-level result is
`verified` when every layer passes, `violated` when a behavioral/refinement/impl
layer fails, and `error` when any layer returns a spec/IO/internal error. The
process exit code follows the existing `cli.exit_code` convention.

```toml
[business]
file = "specs/business.fsl"
depth = 8

[requirements]
file = "specs/requirements.fsl"

[design]
file = "specs/design.fsl"
depth = 12
refine_against = "requirements"
mapping = "specs/design_refines_requirements.fsl"

[impl]
command = "pytest -q"
```

For `[business]`, `[requirements]`, and `[design]`, adding `depth = K` runs the
existing `verify` path at that depth; omitting `depth` runs the existing `check`
path. `refine_against` names another manifest layer and requires an explicit
`mapping` file because `fslc refine` needs the state/action correspondence. The
implementation command runs with the manifest directory as its working
directory. By default the chain short-circuits on the first failed layer and
marks the remaining planned layers as `skipped`; `--keep-going` records the
failure and continues through the rest of the manifest.

## 8. Phased plan

| Stage | Content | Scale |
|---|---|---|
| 0 | Organize the spike (§2) as `examples/layers/` + this design document | done/small |
| 1 | **Metadata plumbing**: pass req_id/text through from AST → all JSON output (delivers value ahead of dialects: usable even in current fsl via `// @req REQ-1` annotations) | small |
| 2 | **fsl-req dialect**: requirement/acceptance/branches/implements. The expander is isomorphic to compose. The automatic split of `branches` and the automatic synthesis of refinement are the core | medium (about one compose round) |
| 3 | **fsl-biz dialect**: process/policy/kpi. The expander + display in business vocabulary | medium |
| 4 | Three-layer dogfooding (run all three layers + implementation, starting from a consulting document) | medium |

Risks and fallback: the concern that a dialect becomes a leaky abstraction
(kernel concepts are exposed on verification failure) is addressed by a
per-layer repair-protocol table (a dialect edition of the skills). Principle of
not making too many dialects: **do not add new semantics to the kernel**. What
cannot be expressed in a dialect is organized as "write it in that layer's
document (outside FSL)."
