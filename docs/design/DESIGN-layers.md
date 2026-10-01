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
  The process+data profile now covers the common single-entity lifecycle directly;
  the kernel-wrapper form still automates this split with `branches` for hard
  multi-outcome correspondences (§4.2).
- **(L2) the business vocabulary maps straight onto the kernel**: process=enum+Map,
  policy=invariant/leadsTo, actor=domain type, KPI=count projection metadata. Not a single
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
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject  Requested -> Rejected by Manager
    transition refund  Approved  -> Refunded by Manager
  }

  kpi refunded = count Return in Refunded  // → count projection metadata

  control CTRL-DECISION "every request preserves adjudication control"
    owner Manager
    severity high
    applies_to Return

  policy EveryRequestDecided "every request is eventually decided"
    satisfies CTRL-DECISION
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can be completed"
    all Return can be Refunded or Rejected
}

verify {
  instances Return = 3
}
```

Expansion rules: `process` → enum + `Map<CaseId, Stage>` + an action per
transition (`by <actor>` is action metadata; a transition whose actor has a
parameter becomes a parameter of the actor type). `kpi name = count Entity in
Stage` → a typed `ProjectionDef` containing the shared Aggregate/Binder IR,
available to native explain but not a ghost counter or automatic `_kpi_*`
invariant. Entity/stage resolution is checked during lowering. `control ID
"text"` is governance metadata only;
`policy/goal ... satisfies CTRL` attaches that control to the generated
invariant/leadsTo/reachable so violations identify both the broken policy and the
control it was meant to satisfy. Readable `every ... must eventually ...`
policies lower to leadsTo.

When a control is reused across processes or sits above a specific business
flow, write an optional governance catalog:

```fsl
governance EnterpriseReturnControls {
  authority Operations owns CTRL-DECISION
  control CTRL-DECISION "every request preserves adjudication control"

  delegates ReturnHandling from "return_policy.fsl" {
    require CTRL-DECISION
  }

  preservation ReturnReform {
    before AsIsReturn from "asis_return.fsl"
    after  ToBeReturn from "tobe_return.fsl"
    preserve CTRL-DECISION
    checked_by refinement "tobe_refines_asis.fsl"
  }
}
```

`fslc check` on the governance file validates control references, confirms that
delegated business specs satisfy required controls either via business-side
`satisfies` metadata or explicit `CTRL is satisfied_by policy|goal ID` mappings,
and runs preservation refinements at depth 8.

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
requirements ExpenseRequirements {
  implements ExpenseToBe from "1_business.fsl" { }

  number Amount
  const AUTO_LIMIT = 1

  process Claim with amount: Amount {
    stages Draft, Submitted, Approved, Rejected, Paid
    initial Draft
    transition submit       Draft     -> Submitted by Employee with a: Amount when a > 0 set amount = a covers REQ-1 "The applicant submits an expense claim by entering an amount"
    transition auto_approve Submitted -> Approved  by System  when amount <= AUTO_LIMIT covers REQ-2 "Claims at or below AUTO_LIMIT are auto-approved by the system"
    transition mgr_approve  Submitted -> Approved  by Manager when amount >  AUTO_LIMIT covers REQ-3 "Claims above AUTO_LIMIT are approved by a manager"
    transition reject       Submitted -> Rejected  by Manager when amount >  AUTO_LIMIT covers REQ-3 "Claims above AUTO_LIMIT may be rejected by a manager"
    transition pay          Approved  -> Paid      by Finance covers REQ-4 "Only approved claims are paid"
  }

  kpi paid_claims = count Claim in Paid

  acceptance AC-1 "Approval flow: a low-amount claim is auto-approved and paid" {
    submit(0, 1) auto_approve(0) pay(0)
    expect Claim 0 in Paid
  }
}

verify {
  instances Claim = 3
  values Amount = 0..3
}
```

### 4.2 Expansion rules

- `number X` + `verify { values X = lo..hi }` becomes the bounded kernel type
  `type X = lo..hi`; lifecycle entity sizes come from
  `verify { instances E = N }`.
- `process E with f: T` is the default single-entity lifecycle form. It expands
  to the stage enum, entity stage map, carried field maps, deterministic init,
  and fair transition actions. Transition `with`/`when`/`set`/`covers` lower to
  action params, requires, assignments, and metadata.
- `kpi name = count E in S` records the same typed Aggregate/Binder projection
  as the business layer, available to explain/scenarios without adding a ghost
  counter.
- `requirement` block → **attach `req_id` / `req_text` metadata** to the
  contained kernel elements (action/invariant/leadsTo). All JSON output
  (violated / unknown_cti / coverage diagnostics / scenarios) carries
  `requirement: {id, text}` — "which requirement broke" appears in the
  counterexample together with the original text (§6).
- `branches` remains in the kernel-wrapper fallback. It automatically splits an
  action into multiple actions with the when condition added to requires
  (`submit__1`, `submit__2`; displayed as `submit[a<=AUTO_LIMIT]`). The action
  correspondence of the refinement mapping to the upper layer is generated from
  the `maps` clauses.
- `implements ... from` → synthesize a refinement-file equivalent from the state
  mapping. An empty body auto-generates identity refinement when names match;
  `maps auto` is allowed for same-name kernel-wrapper state/actions; explicit
  maps override either form. Auto-mapped process transitions are actor-checked.
  At `fslc verify` time, fslc **also runs the refine check against the upper
  layer** (the result JSON has `implements: {abs, result}`).
- `acceptance` → a **fixed scenario** with known steps + expect. Checked via the
  replay mechanism, and it also enters the scenarios output as-is (= the
  acceptance test flows into downstream testgen and becomes a conformance test
  for the implementation). The readable stage assertion
  `expect <Entity> <id> in <Stage>` is available alongside `expect <expr>`.

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
   (put `fair` on the lower action that bears the progress), or (b) add
   `preserve progress { respond <AbsLeadsTo> by <impl actions...> }` to the
   refinement mapping. That opt-in pulls the upper `leadsTo` through the state
   mapping and reports `refinement_failed / progress_lost` if the lower layer can
   spin or stall while the upper response remains pending. This is a general
   property of forward simulation (safety is preserved, liveness is not), and is
   not an fslc defect. **Discrete-time SLAs follow the same rule**: a `deadline`
   is a safety property of the clock that declares it, so a refinement carries it
   only across a *shared* clock — a design with a finer clock (extra `tick`-side
   time steps) has no abstract image for those steps and cannot refine a coarser
   timed abstract. Verify a timed property at the clock-owning layer; share the
   clock to carry it down (`DESIGN-nfr.md` §6). Concrete example: a design that, after placing the
   submission on an internal queue, keeps spinning a stutter loop instead of a
   non-fair adjudication passes ordinary refine but breaks the business leadsTo;
   with `preserve progress`, the same mapping fails as a progress-lost refinement.
2. **Traceability metadata** (new; plumbing only): put `req_id`/`policy_id` on
   nodes of the kernel AST and pass them through into all JSON output. From the
   design layer's counterexample, "violates REQ-1 (original text)" comes out
   directly. A cross-cutting query such as
   `fslc analyze fsl-project.toml --projection traceability_graph --format json`
   (which element in which layer derives from REQ-1) is also generated from the
   same metadata.
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
layer fails, `indeterminate` when the only failing layer is an `[impl]` layer
without evidence that a test executed (see "`[impl]` evidence" below), and
`error` when any layer returns a spec/IO/internal error. The process exit code
follows the existing `cli.exit_code` convention (`indeterminate` exits 1).

```toml
[business]
file = "business.fsl"
depth = 8

[requirements]
file = "requirements.fsl"

[design]
file = "design.fsl"
depth = 12
refine_against = "requirements"
mapping = "design_refines_requirements.fsl"

[impl]
command = "pytest -q --junitxml=impl-report.xml"
report = "impl-report.xml"
```

The layer filenames above are placeholders for whatever the project calls its
layers; `tests/fixtures/chain/fsl-project.toml` is a runnable manifest of this
shape, with its four layer files beside it.

For `[business]`, `[requirements]`, and `[design]`, adding `depth = K` runs the
existing `verify` path at that depth; omitting `depth` runs the existing `check`
path. `refine_against` names another manifest layer and requires an explicit
`mapping` file because `fslc refine` needs the state/action correspondence. The
implementation command runs with the manifest directory as its working
directory (a bare filename such as the documented default `fsl-project.toml`
resolves to `.`, not to an empty `current_dir`). By default the chain
short-circuits on the first failed layer and marks the remaining planned
layers as `skipped`; `--keep-going` records the failure and continues through
the rest of the manifest.

### `[impl]` evidence (issue #1200)

An exit status of 0 does not say that any test ran. A suite whose tests are all
skipped (for example a `fslc testgen --allow-unwired` scaffold whose adapter is
not wired), or that collected none, exits 0 too, and before #1200 the chain
reported that as a passing implementation layer. Runners reach that state in
ordinary use: Gradle reports `UP-TO-DATE`/`NO-SOURCE` and runs nothing,
`vitest --passWithNoTests` and a `swift test --filter` that matches nothing
exit 0, and PHPUnit's "No tests executed!" is not a failure by default. So an
`[impl]` table must now say what its exit status is evidence of, and it may
carry only three keys: `command`, `report`, and `evidence`. Any other key — a
typo such as `reprot` included — is a `kind:"parse"` error at exit 2 that
names the key, rather than a silently ignored requirement.

| `[impl]` keys | Command exit | Report | Layer `result` | Layer `exit_code` |
|---|---|---|---|---|
| `command` only | 0 | — | `indeterminate` | 1 |
| `command` only | ≠ 0 | — | `failed` | 1 |
| `command` + `evidence = "exit_code"` | 0 | — | `passed` (`detail.evidence: "exit_code_only"`, top-level `warnings[]`) | 0 |
| `command` + `evidence = "exit_code"` | ≠ 0 | — | `failed` (`detail.evidence: "exit_code_only"`) | 1 |
| `command` + `report` | ≠ 0 | any | `failed` | 1 |
| `command` + `report` | 0 | ≥ 1 executed, 0 failure/error | `passed` | 0 |
| `command` + `report` | 0 | a `<failure>`/`<error>` test case | `failed` | 1 |
| `command` + `report` | 0 | 0 executed (none, or all skipped) | `indeterminate` | 1 |
| `command` + `report` | 0 | missing, unchanged by the run, or not JUnit XML | `indeterminate` | 1 |
| an unknown key, `report = ""`, `report` and `evidence` together, or `evidence` other than `"exit_code"` | — (not run) | — | `error` (`kind:"parse"`) | 2 |

- **What `report` is.** A path, relative to the manifest directory, of a JUnit
  XML file the command writes, or of a directory whose `*.xml` files are
  summed (Gradle's `build/test-results/test`). JUnit XML is the one
  machine-readable format every testgen target's runner writes:
  `pytest --junitxml=FILE`, `vitest run --reporter=junit --outputFile=FILE`,
  `swift test --xunit-output FILE`, Gradle/`kotlin.test` on the JVM,
  `dart test --reporter json | tojunit`, `phpunit --log-junit FILE`.
  Runner notes:
  - **SwiftPM 6.** When XCTest also runs in the session, Swift Testing's
    results do not go to `FILE` but to `FILE-swift-testing.xml` beside it
    (`FILE` minus its extension, plus `-swift-testing`, plus the extension).
    That is the path computation in
    `swift-package-manager` `Sources/Commands/SwiftTestCommand.swift`
    (`swiftTestingXUnitDestinationPath`, read at commit `166166b3`). A
    generated Swift Testing suite therefore needs `report` to name that file,
    or a directory holding both, or the run to pass `--disable-xctest`.
  - **Gradle.** Pass `--rerun-tasks` (or `cleanTest`): an `UP-TO-DATE` test
    task writes no new report, which the freshness rule below reads as
    `indeterminate`, not as a pass.
  - **Pipes.** `dart test --reporter json | tojunit > r.xml` reports the
    converter's exit status, not the runner's, so a failing run can exit 0.
    The report check still sees the failing `<testcase>`; the exit code alone
    would not.
- **How it is counted.** One test per `<testcase>` element; one with a
  `<skipped>`, `<failure>`, or `<error>` child is skipped, failed, or erroring.
  The `tests`/`skipped` attributes of `<testsuite>` are not read, because
  runners differ in which ones they write. CDATA sections and comments are
  removed first, and the start-tag scanner skips a `>` inside a quoted
  attribute value, so neither captured test output nor a test name such as
  `t[a/>b]` can fake or hide a marker (`rust/fslc/src/junit_report.rs`).
- **Freshness.** Before the command runs, the chain records each existing
  candidate file (the file, or every `*.xml` in the directory) by length,
  modification time at full precision, and on Unix inode and change time.
  After the run only files that are new or whose record changed count, so a
  report left over from an earlier run is never this run's evidence, even one
  written within the same second. A runner that rewrites identical bytes on a
  file system that keeps neither precise modification nor change times can be
  read as unchanged; that errs toward `indeterminate`. The chain never deletes
  or writes the report.
- **Partial skips pass.** A report with at least one executed test and some
  skipped ones is `passed`; the counts are in the layer's `detail.tests`
  (`total`, `executed`, `skipped`, `failures`, `errors`) for a reviewer to
  read, and the stderr table shows `executed=E/T`. Only "nothing executed" is
  decided as `indeterminate` here.
- **Exit-code evidence is an explicit, visible opt-in.** A command that is not
  a test suite (`make check`, a smoke script) declares
  `evidence = "exit_code"`. Its layer is then decided by the exit status as
  before #1200, but it carries `detail.evidence: "exit_code_only"`, the stderr
  table shows `evidence=exit_code_only`, and the envelope gets a top-level
  `warnings[]` entry `{"kind": "impl_exit_code_only", "layer": "impl",
  "message": ...}`, so the weaker evidence is never mistaken for a counted
  test run. Without either key the command still runs, so a failing command is
  `failed`; only its exit 0 becomes `indeterminate`.

### Parallel layers (`--jobs N`, issue #1151)

`--jobs N` runs up to `N` layers at once. The default is 1, the serial loop
above. The core count is not the default because memory multiplies with
workers, and single verifications in this repository have measured 11 GB
(#697) and 28 GB (#1041). The job count does not change the result: stdout,
the stderr table, and the exit code are those of `--jobs 1`. Two figures are
exempt. Elapsed times differ between any two runs. The solver's `memory_mb`
is Z3's per-process peak, so while other workers run it includes their
memory. The contract is the one #1108 set for `sweep`/`mutate`:

- **Which steps may overlap.** A layer's result depends only on its own
  manifest table, the target table of its refine link, and the files those
  name. It never depends on another layer's result. The chain's order decides
  only which layers run at all. So all `spec` and refine steps may run
  concurrently, with one exception. Two `spec` layers whose files have
  identical bytes can share a verify-cache entry (the keys are
  content-addressed, #1148). Run serially, the later layer would hit the entry
  the earlier one wrote. Such layers therefore run in one worker, in manifest
  order, and get the same `cache` annotation as in a serial run. `[impl]` is
  a side effect, and whether it runs at all depends on the earlier layers. It
  runs alone after every other step, and only when the serial loop would
  reach it.
- **One Z3 context per step, at every job count.** z3 0.20 keeps one
  `Context` per thread, and it is not `Send`. A context that has already built
  an earlier layer's terms can lead the solver to a different witness for the
  next layer. Measured on `examples/agentic_rag` at depth 8, the design
  layer's `reachable_failed` witness and solver counts from a serial chain
  differed from `fslc verify agentic_rag_design.fsl --depth 8` for the same
  file and depth. So each `spec` and refine step runs on a thread of its own,
  including at `--jobs 1`. That thread has the same 8 MiB stack as the main
  worker. As a result a layer reports what `fslc verify` reports for it, which
  is also what the verify cache holds. This changes the serial chain's output
  for such manifests, from the order-dependent witness to the standalone one.
- **Order.** Each step's result goes into the slot for its manifest position.
  The existing aggregation reads the slots in manifest order, so the order in
  which steps complete never reaches the output.
- **First failure and early exit.** Workers claim steps in manifest order.
  Without `--keep-going`, a failing step stops workers from starting any later
  step. A panicking step does the same with or without `--keep-going`. Every
  step before the serial run's first stopping step still runs, because
  skipping one would require an earlier stopping step. The report is decided
  once every step up to that point has reported, and the chain then prints it
  and exits without waiting. A solver cannot be interrupted from outside its
  thread, so a later step that is already running is detached, and the
  process exits under it. Measured on a release build, with a `[business]`
  layer that fails at step 0 and the slow `[design]` test layer at depth 11
  (0.91 s on its own): without the early exit `--jobs 4` took 0.89–1.02 s
  against 0.04–0.09 s at `--jobs 1`, because the discarded design layer ran
  to the end. With it, `--jobs 4` takes 0.03–0.05 s. The output was the same
  in all runs, and 100 repeated `--jobs 4` runs all exited 1 with identical
  output. Steps print nothing, so a detached step cannot write
  after the table. A detached step killed while storing a cache entry leaves
  at most its temporary file (see below). One that finishes in the moment
  before the process exits still stores its entry. That entry is a correct
  verdict, but a following run can hit it where a run after a serial chain
  would miss. A panic is re-raised with its original payload only when the
  manifest-order walk reaches it, which is where the serial loop panics.
  The one stderr difference left is a panic in a later step that a serial run
  would never have started: its panic message can still be printed before
  the process exits.
- **Cache writes.** An entry is written to a temporary file and then renamed
  into place. The temporary name carries a per-process sequence number as
  well as the process id, so two workers in one process never write through
  the same temporary file. A failed write or rename removes the temporary
  file. The cache key does not include the job count.

The manifest bounds the speed-up. A chain has at most three `spec` layers
and three refine links, so the longest layer sets the floor. Verifying many
independent specs of a project in one pass is a different axis, and so is
the `sweep`/`mutate` grid of #1108.

The manifest reader is fail-closed (issue #489): a top-level section name
other than `[business]`, `[requirements]`, `[design]`, or `[impl]` — including
a plain typo — is a `kind: "parse"` error at exit 2 rather than a silently
dropped layer, and a manifest with zero recognized sections (including an
empty file) is the same error rather than a vacuous `verified` with
`layers: []`. A `depth` or `refine_depth` value that is present but not a
plain non-negative integer (for example, one followed by a TOML inline
comment) is also a `kind: "parse"` error for that layer at exit 2; only an
*absent* key defaults (to `check` for `depth`, or to 8 for `refine_depth`'s
cascade through `depth` and the target layer's `depth`). A malformed value is
never silently substituted with a default, since that would understate a
depth the manifest author explicitly declared.

## 8. Phased plan

| Stage | Content | Scale |
|---|---|---|
| 0 | Organize the spike (§2) as `examples/layers/` + this design document | done/small |
| 1 | **Metadata plumbing**: pass req_id/text through from AST → all JSON output (delivers value ahead of dialects: usable even in current fsl via `// @req REQ-1` annotations) | small |
| 2 | **fsl-req dialect**: process+data, requirement/acceptance/branches/implements. The expander is isomorphic to compose. Process+data covers the common lifecycle; `branches` remains for hard kernel-wrapper cases; automatic synthesis of refinement is the core | medium (about one compose round) |
| 3 | **fsl-biz dialect**: process/policy/kpi. The expander + display in business vocabulary | medium |
| 4 | Three-layer validation (run all three layers + implementation, starting from a consulting document) | medium |

Risks and fallback: the concern that a dialect becomes a leaky abstraction
(kernel concepts are exposed on verification failure) is addressed by a
per-layer repair-protocol table (a dialect edition of the skills). Principle of
not making too many dialects: **do not add new semantics to the kernel**. What
cannot be expressed in a dialect is organized as "write it in that layer's
document (outside FSL)."
