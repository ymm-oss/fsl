<!-- SPDX-License-Identifier: Apache-2.0 -->

# Rust component internal design

Status: accepted current-architecture record. Evidence baseline:
`c37a81a1418f98c701e22729f3dff4d5e7ddbe5f`.

## 1. Decision

Keep the eleven crate boundaries defined by
[`DESIGN-rust-components.md`](DESIGN-rust-components.md), and organize each crate internally as a
directed transformation rather than applying one folder template everywhere:

```text
owned vocabulary/value types
  -> normalization and validation
     -> stateful engine or derived analysis
        -> projection
           -> host adapter / entrypoint
```

A crate implements only the roles it needs. Cohesive protocol and backend adapters remain single
modules. Multi-family crates keep a stable root facade and make dependencies point from delivery
and projections toward policy-free internal values, never the reverse.

The selected intervention is **a contract-first internal role map without source restructuring**.
It records the logical design, accepted exceptions, and evidence required before a structural
slice. It catalogs small, behavior-preserving target directions for observed wrong-direction
dependencies, but each source move requires the separately scoped C2 authorization gate in
sections 6 and 7. This record itself authorizes no source move and does not introduce a crate,
public schema, language behavior, solver protocol, or process contract.

This decision is at function/type and in-crate module level (L0/L1), with a twelve-month horizon.
It supersedes neither feature-specific accepted design notes nor the parent crate-boundary record.

## 2. Decision frame and hard gates

| Item | Value |
|---|---|
| Question | How should values, transformations, mutable state, side effects, failures, and public facades be assigned inside each authoritative Rust crate? |
| Scope | Production modules of the eleven members of `rust/Cargo.toml`; test and compatibility binaries are considered as oracles or sibling paths. |
| Outcome | A change has one semantic owner, one mutable-state owner, an explicit downward dependency path, and a rejecting test at every formal boundary. |
| Risk posture | Conservative toward semantic convergence, false-green verification, output drift, and broad move-only diffs; neutral toward module count and file length. |
| Runtime evidence | No production traces, incidents, SLOs, or profiling data were available. |
| History quality | Git history is available, but the observed Rust migration and feature period is short and merge-heavy. |

Hard gates are:

1. `KernelModel`, `Monitor`, `SmtSolver` implementations, and `ServerState` remain the distinct
   checked-model, concrete-state, solver-state, and editor-state owners.
2. A lower-level value or validation module must not depend on a renderer, command, protocol, or
   backend module merely to reuse policy.
3. Concrete and symbolic evaluators remain independent implementations joined by agreement tests;
   consolidation must not remove this independent oracle.
4. Search, replay, and verification engines call the authoritative transition/evaluation owner;
   they do not reimplement commit, rollback, guard, or type-conformance semantics.
5. Semantic evaluation receives I/O only through an explicit resolver/input. No new implicit
   filesystem access enters derived transforms; existing named path-presentation adapters are
   bounded below. Artifact writes, signature/trust handling, stdout, and exit status remain
   delivery concerns.
6. Failures remain typed or structured until a delivery projection maps them to a stable JSON or
   protocol contract. Unsupported shapes fail closed.
7. An internal move does not change the public Kernel/replay/CLI/Worker/LSP contract, raw-output
   mode, normal Cargo dependency graph, or root-level Rust API unless a separate contract change
   explicitly says so.
8. Every formal-to-implementation anchor retains a negative control that rejects a known invalid
   expression, transition, trace, projection, or artifact.

## 3. Evidence and contract classification

Evidence grades are E3 (reproducible measurement), E2 (direct artifact), E1 (triangulated
inference), and E0 (assumption).

| ID | Grade | Claim and source | Decision use |
|---|---|---|---|
| E-01 | E2 | Module declarations, imports, public re-exports, and state types in all eleven crates expose the current internal dependency and state-owner graph. | Establishes the current structure. |
| E-02 | E2 | `dispatch::parse_document`, `build_model`, `Monitor`, `SmtSolver`, verification engines, tool-family facades, CLI/Worker entrypoints, and `DocumentIndex` form observable transformation stages. | Establishes the target direction. |
| E-03 | E2 | The baseline found checked-model type validation in `fsl-core::public_kernel`, induction liveness/trace helpers in `bmc`, document locale in `document_render`, duplicated tools Domain naming, and `fslc::verification` using `super::*`. The first four were resolved independently by #420, #421, #422, and #419. | Identifies concrete wrong-direction or hidden dependencies and records which evidence has already been acted on. |
| E-04 | E2 | `fsl-tools` has independent document, causal, specialized-dialect, and Kernel-derived families behind root re-exports; the document and causal families also have distinct schemas and focused histories. | Justifies family-level internal boundaries without a crate split. |
| E-05 | E1 | In `295508e9^..c37a81a1418f98c701e22729f3dff4d5e7ddbe5f`, 37 non-merge commits touched `fslc/src/main.rs`, while its command families already have separate integration suites and distinct failure/state contracts. | Justifies a command-family extraction experiment, not a new crate. |
| E-06 | E2 | Runtime rollback, explicit search, symbolic agreement, Public Kernel, document rendering, CLI, browser parity, and LSP tests contain rejecting cases. | Supplies existing fitness functions. |
| E-07 | E3 | Focused syntax/core/runtime/solver/native-Z3/verifier tests passed 219 tests, and `cargo check -p fsl-solver-z3js --locked` passed at the baseline. | Confirms the semantic internals used as design evidence are reproducible. |
| E-08 | E0 | Production load, third-party Rust API use, team ownership, and future feature frequency. | Limits eager restructuring and API-removal claims. |
| E-09 | E2 | `fmt`, readable explain, and TypeScript modes emit raw output; the accepted port contract requires raw modes to remain raw. Test generation receives one delivery-normalized path context, and `testgen::relative_spec_path` is a lexical projection. | Bounds the CLI output design and records the resolved testgen I/O boundary. |

Scoped classifications are:

- **required:** the semantic state owners and hard gates above; accepted versioned Kernel, replay,
  CLI, Worker, and LSP contracts; raw-output modes; single standard parser dispatch; checked
  lowering; concrete rollback; backend-neutral symbolic semantics; fail-closed projections;
  native/browser parity; latest-buffer LSP behavior;
- **de facto compatibility:** root-level Rust re-exports used by current workspace consumers, the
  public legacy runtime BFS used by its bundled helper binary, and compatibility-binary invocation
  shapes used by repository parity tooling; preserve them during an internal move unless a separate
  retirement decision is made;
- **accidental:** current filenames, flat versus nested modules, private helper placement,
  root-module implementation bodies, and per-call accumulator shapes;
- **resolved defect:** checked type validation formerly owned by `public_kernel`, shared induction
  policy formerly owned by `bmc`, document locale formerly owned by its renderer, and duplicated
  tools Domain `snake` transforms were corrected by #420, #421, #422, and #419 respectively;
- **remaining defect:** the native verification module's glob dependency on its parent;
- **unknown:** external use of the public legacy runtime BFS and compatibility binaries, operational
  importance of those paths, and whether current change clusters will persist.

The internal placement defects are not asserted product bugs. They are observable dependency or
duplication problems whose correction must preserve existing behavior and negative controls.

## 4. Common internal rules

### 4.1 Facade and dependency direction

`lib.rs` is the public contract surface. Inside a separately authorized C2 slice, a multi-module
crate should converge toward explicit module declarations and re-exports, not act as an unnamed
shared namespace. Private modules import the exact lower-level items they need; `use super::*` is
forbidden in production modules. A single-file protocol or backend adapter may implement its
cohesive responsibility directly.

No module may own policy solely because it is a convenient import location. When a separately
authorized C2 slice addresses a rule needed by two siblings, move the smallest rule to a neutral
lower-level module; do not introduce a service, registry, context object, or trait unless multiple
implementations or state ownership require one.

### 4.2 State and transformations

- Immutable source, Kernel, symbolic, and artifact values cross module boundaries by value or
  shared reference.
- Every long-lived mutable state has one owner. Helper modules receive the minimum state they need;
  they do not mutate another engine through callbacks.
- Search owns queues, visited sets, parents, and budgets. It does not own transition meaning.
- Projections are deterministic functions of checked results and explicit presentation options.
  They do not repair, reinterpret, or silently omit unsupported semantics.

### 4.3 Failures and side effects

Parsers, lowerers, engines, and tool families return their native error/result type. The CLI,
Worker, or LSP converts that result once at its boundary. Filesystem access is either an explicit
delivery operation, a named resolver, or a declared path-presentation adapter; writes never occur
inside semantic or derived-artifact transforms. `fslc-rust` constructs test generation's explicit
normalized path context, including independent spec and output-parent canonicalization/fallback;
`testgen::relative_spec_path` remains a filesystem- and CWD-free lexical projection.

Move-only refactors must retain both a successful oracle and a rejecting oracle. A successful build
or unchanged positive snapshot alone does not establish semantic preservation.

## 5. Component internal designs

This section records current ownership and possible target directions. Any
source movement described as a target, trigger, or experiment is a candidate,
not authorization. Selecting one requires the independently scoped C2 gate in
sections 6 and 7; until then, the current physical layout remains the decision.

### `fsl-syntax`

Logical direction:

```text
ast / surface / annotation value types
  -> lexer + shared annotation/expression parsing
     -> standard dialect parsers
        -> dispatch::parse_document
           -> lossless source projection / formatter
```

- `ast.rs`, `surface.rs`, and `annotation.rs` own source-faithful values and spans.
- `lexer.rs`, `annotation_parse.rs`, `syntax_expr.rs`, and the shared parts of `parser.rs` own
  reusable recognition mechanics. Per-call token cursors and pending annotations remain local.
- `ai.rs`, `db.rs`, and `domain.rs` own dialect-specific recognition. `dispatch.rs` is the sole
  standard-FSL acceptance front door and lexes once.
- `causal.rs` remains an explicit review-only sibling outside standard dispatch. `literate.rs` is
  preprocessing, not a second parser. `lossless.rs` may consume parser results but must not decide
  semantic acceptance.
- **Current decision:** retain the current physical layout. Add a module only when a new dialect has
  an independent grammar/value contract; do not split the large shared parser by production count.

### `fsl-core`

Logical direction:

```text
syntax + lowering vocabulary + origin sidecar
  -> dialect / compose / domain lowering
     -> KernelSpec
        -> checked-model validation and KernelModel
           -> refinement validation against implementation/abstraction models
           -> diagnostics / expression text / trace / Public Kernel projections
```

- `KernelSpec` is unchecked normalized input; `model.rs::KernelModel` is the checked semantic owner.
  `ModelBuilder` owns construction-only maps and releases an immutable model.
- `compose.rs::FileResolver` is the I/O boundary. `FsResolver` is a native convenience adapter, not
  permission for lowerers or validators to read paths directly.
- **Resolved neutral-policy slice (#420):** `typecheck` owns
  `validate_expression_type`, `validate_statement_types`, and
  `validate_model_expression_types` for `model`, `refinement`, and the Public Kernel exporter.
  #397 does not reopen or duplicate that completed move.
- Structural Domain lowering and generated-Kernel-source projection remain different outputs.
  `fsl-syntax::DomainEffect` owns value-level role conflict detection and the explicit outcome
  union; `domain_lowering` owns checked event cross-reference validation and status-member mapping
  consumed by both core projection paths.
- **Resolved phase selection (#397):** retain the current physical dialect/domain owners. The
  duplicate private core `lower_name` implementations are a weak static signal, but their distinct
  structural-lowering and generated-source outputs have no naming drift or naming-policy change
  after their introduction. Re-evaluate when one path changes, names diverge, or a shared
  naming-policy defect is demonstrated. Preserve the current file tree and public re-exports
  meanwhile.

#### Core dialect/domain lowering evaluation

The current value flow remains directed. `fsl-syntax` owns dialect recognition and Domain value
normalization. `compose::parse_kernel_source` is the `SurfaceDocument` dispatch gate.
`dialect::lower_domain` coordinates checked Domain lowering, `domain_lowering` owns structural
Domain-to-Surface lowering plus checked outcome-event validation/status mapping, and `domain` owns
the separate generated Kernel-source projection while consuming that checked policy. The
syntax-owned `DomainEffect` methods expose role conflicts, explicitly assigned events, and the
complete outcome union to both core paths.

The twelve-month horizon includes the full available Rust history, not only the accepted baseline.
The two core `lower_name` implementations were introduced by the native migration/direct-lowering
work (`81b31eb` and `a851588`) and have not changed since. In contrast, `d4f5232` and `a0f6f31`
corrected real drift in effect-outcome handling across these same structural and generated-source
paths, then placed status mapping and checked validation behind a shared core owner. That is direct
evidence that a genuinely shared policy can create cross-path amplification and should be
centralized. It is not evidence that the distinct naming outputs have drifted or that sharing their
helper now reduces total cost. Since the accepted baseline, only #420 touched the evaluated paths.

| Choice | Intervention | Benefit | Cost and risk | Decision |
|---|---|---|---|---|
| C0: retain current owners | No source move; keep the two output paths separate | Preserves independently testable structural and generated-source projections | The two private naming helpers remain visibly duplicated, and a truly shared future policy can require synchronized edits | **Selected while naming-specific evidence is insufficient** |
| C1: share only core naming | Move `safe`/`lower_name` behind one private owner | Removes duplicate lines and gives future naming changes one edit point | Couples outputs without observed naming drift or change frequency; a later path-specific rule could require another split | Defer pending an independent naming signal |
| C2: impose a dialect/domain module tree | Move recognition, dispatch, lowering, and projections into a new hierarchy | Makes conceptual stages physically prominent | Broad import and review churn with no measured semantic, operational, or maintenance benefit | Reject |

This is an implementation-local-optima audit, not a file-size judgment:

| Audit variable | Local evaluation | Expanded evaluation |
|---|---|---|
| `B` boundary | One private naming helper | Syntax recognition through both checked Domain projections |
| `M` metric | Minimum duplicate code | Semantic ownership, independent evolution, and contract-preserving change cost |
| `N` change scope | Share two helper functions | Move one policy owner or restructure the dialect/domain pipeline |
| `T` time | Current static layout | Full available history within the twelve-month horizon and the next naming/lowering change |

Observed evidence is the import/value-flow graph, distinct output contracts, existing controls,
unchanged naming helpers, and the earlier effect-outcome drift and correction. It is inferred that
sharing the naming helper now would add coupling without lowering measured naming-policy change
amplification. Whether the next naming change will affect both paths identically is an unverified
hypothesis. C0 is preferable at the function and module boundaries; feature and lifecycle evidence
is mixed because adjacent effect-outcome policy did require cross-path correction, while the naming
policy did not. System and operational evidence is absent. There is therefore no demonstrated
preference inversion for the remaining naming candidate, but an all-boundaries C0 claim would be
too strong. For the remaining naming candidate, the result is `insufficient-evidence`, severity
1/15 (`E0 A0 F0 K0 T1`), confidence C2 from source and history. The adjacent effect-outcome history
limits the broader phase claim but does not establish naming-specific amplification, so `A` remains
zero. `F0` does not erase the historical outcome defect; it records that the defect was corrected
and no naming-boundary failure is observed. `T1` records that a future private-helper move remains
easy to reverse. Current separation is locally rational because the paths produce different
representations and can fail at different stages.

No migration or compatibility bridge is needed for C0. If a future independent signal selects C1,
perform it as one revertible private-helper move and roll back by reverting that change. Entry and
exit controls must cover both positive and rejecting cases: successful structural lowering and
generated-source projection; invalid expressions; conflicting or duplicate effect-outcome roles;
name/projection mismatch; unsupported Public Kernel projection; origin and deterministic ordering;
and symbolic/concrete Domain agreement. Any move must preserve the typed model, Kernel schema and
ordering, and must not repeat #420's checked-model validation work.

### `fsl-runtime`

Logical direction:

```text
State / Bindings / RuntimeError
  -> concrete value evaluation and assignment validation
     -> Monitor atomic transition + BoundedLivenessMonitor
        -> replay / refinement / trace generation
        -> explicit search and legacy BFS
```

- `Monitor { model, state, step }` is the sole committed logical-state owner. A post-update failure
  exposes an attempted state as evidence but does not mutate committed state.
- `BoundedLivenessMonitor` owns only pending obligations and logical time. Search engines own only
  frontier, seen, parent, coverage, and budget state.
- `explicit.rs` correctly reuses `Monitor`. The legacy `bfs` and product explicit engine may keep
  different result contracts, but any shared successor enumeration must call `Monitor` rather than
  copy transition policy.
- `lib.rs` currently contains the public facade, evaluator, monitor, liveness, replay, refinement,
  legacy BFS, and trace helpers. Their distinct state and failure contracts identify a candidate
  split into `eval`, `monitor`, `liveness`, `replay`, `refinement`, and `search`, while preserving
  root re-exports.
- **Candidate slice (not authorization):** if selected through the C2 gate, extract only the
  responsibility being changed, one move-only slice at a time. Do not perform an eager 2,600-line
  re-layout, and do not merge the independent symbolic evaluator here.

### `fsl-solver`

- Keep the cohesive single module ordered as immutable SMT vocabulary and result values, error and
  statistics values, `SolverMetrics`, then `SmtSolver`.
- `SolverMetrics` owns query attribution; an adapter owns its assertion stack and model state.
- **Current decision:** no structural change. Add a private module only if a second independent
  protocol responsibility appears; backend convenience is not sufficient.

### `fsl-solver-z3`

- `Z3Solver` alone owns the native solver, stack depth, version, and metrics. `Z3Term` is the typed
  backend handle; conversion and statistics helpers remain subordinate to the trait implementation.
- Seed/version setup stays in construction. Verification policy never enters this adapter.
- **Current decision:** retain one module. Any split must follow a new independently testable
  backend state, not term/helper count.

### `fsl-solver-z3js`

- `Z3JsSolver` owns Rust-side version, stack accounting, and metrics. The JavaScript bridge owns
  actual assertion/model state and opaque term handles.
- Promise-based `check` is the async failure boundary; Rust-only compilation cannot establish
  handle lifetime or bridge completeness.
- **Current decision:** retain one Rust adapter module and the separate JavaScript bridge. Browser
  parity, cancellation, rejection, and teardown remain mandatory external oracles.

### `fsl-verifier`

Target dependency direction:

```text
value
  -> eval
     -> transition
        -> shared trace projection + liveness constraints
           -> bmc / induction
        -> agreement
bmc -> refinement progress
```

- Each verification call owns symbolic state vectors and choices locally; the mutable assertion and
  model state remains behind `&mut S: SmtSolver`.
- `value`, `eval`, and `transition` are the shared symbolic semantic kernel. `agreement` observes
  that kernel and does not become a production execution dependency on `fsl-runtime`.
- `induction` currently imports `leadsto_bindings`, `leadsto_condition`, and `project_trace` from
  `bmc`. A candidate target moves those helpers to neutral `liveness` and `trace` owners before
  extending either engine.
- `refinement` may compose the public bounded verifier, but it must not reach into another engine's
  private encoding.
- **Candidate slice (not authorization):** if selected through the C2 gate, make the neutral-helper
  move as one behavior-preserving slice, guarded by BMC, induction, deadline, and
  transition-agreement rejecting tests.

### `fsl-tools`

Keep one crate and one root facade over four internal families:

1. requirements documents: `document*`;
2. causal models, analysis, evidence, plans, and projections: `causal*`;
3. specialized AI, DB, and Domain engines;
4. Kernel-derived analysis, mutation, test generation, typestate, ledger, and reports.

Within each family, the direction is checked family model -> analysis/classification -> projection
or rendering. Values such as `RequirementClaimSet` and `CausalModel` are immutable results; private
builders/contexts are per-call accumulators. No family owns process lifecycle or artifact writes.

- **Candidate target (not authorization):** move `Locale` out of `document_render` to a neutral
  document presentation value so markers, glossary, expression rendering, checking, and the
  renderer do not depend upward on the renderer.
- Keep the assurance classifier shared by ledger and document evidence at
  `ledger::assurance_token`/`assurance_label`, as required by the accepted evidence-overlay design.
  Moving it to a neutral module requires that design to be explicitly superseded and is not part of
  this decision.
- **Resolved C2 slice (#419):** `domain_naming::snake` is the one tools-family owner used by Domain
  analysis and code generation. Its simple all-uppercase normalization intentionally remains
  distinct from core's acronym-aware, identifier-safe `lower_name` policy.
- Retain the fail-closed `public_kernel` adapter shared by typestate, test generation, and Domain
  code generation. Do not merge those distinct tools merely because they share input validation.
- **Resolved C2 slice (#423):** `fslc-rust` owns test-generation path canonicalization and passes an
  explicit normalized path context into the pure generator. Preserve generated pytest output and
  embedded paths byte-for-byte for existing, missing, and symlinked path inputs.
- **Resolved C2 slice (#395):** `fsl-tools::analysis` owns the pure checked-model classifier for
  `conservation_candidate` findings. `fslc-rust` retains ai-review family orchestration, aggregate
  ordering and finding IDs, the `analysis-findings.v0` envelope, mode validation, transport, and
  exit status. Further analysis families require their own evidence and independently scoped move.
- **Resolved candidate inventory:** locale ownership and tools Domain naming were completed by
  #422 and #419. Their completion does not authorize another tools move. Any future candidate needs
  new evidence, its own issue and accepted scope/design, positive and rejecting oracles, pre-PR
  audit, and an independently revertible pull request. Do not replace the flat root API or create
  subcrates; family directories are optional presentation after the dependency graph is acyclic.

### `fslc-rust`

The crate has two internal products with one process contract:

```text
reusable library: typed replay/coverage/diagnostic/verification projections

native binary: argument parsing
  -> command-family preparation and I/O
     -> semantic/tool engine calls
        -> existing family-specific JSON or raw output contract
           -> shared exit normalization
           -> stdout + process exit
```

- The default-feature-free library keeps reusable delivery policy needed by Worker and LSP. Native
  solver, signing, report, cache, and filesystem responsibilities remain behind `native-cli`.
- **Candidate target (not authorization):** `main.rs` could converge to metadata/help, top-level
  dispatch, serialization, and exit. Command families are formatting/migration, document/approval,
  causal, Domain/DB/AI, replay/scenarios, verification, analysis/mutation/reporting, and
  refinement/project composition.
- A family module parses its own options, owns its I/O preparation, calls lower owners, and returns
  its existing output plus status. JSON families retain `(Value, i32)`; `fmt`, readable explain,
  TypeScript, and other raw modes remain raw. If emission is later centralized, the maximum needed
  abstraction is a two-variant `Json`/`Raw` output value, not a command framework or generic context
  object.
- **Resolved C2 slice (#396):** `verification.rs` retains prepare/solve/cache/finalize staging with
  explicit imports, while `verification_output` is the sole owner of BMC-compatible native,
  explicit/auto, and Worker result projection. Native-only engine selection and cache policy remain
  in `verification`; `main` keeps process dispatch and serialization without renderer forwarding or
  duplicated property/output classification. Explicit evidence crosses the output boundary only
  after Monitor replay; exact and cross-depth cache reuse validates the entry shape and binds it to
  its independently computed option family. This does not authorize a full engine module tree.
- **Resolved CLI experiment (#393):** the causal parser, I/O preparation, tool calls, and result
  projection now form the single `causal.rs` family module. `main.rs` retains only top-level
  dispatch, the causal-source `check` route, shared process helpers, serialization, and exit
  normalization. The module imports each of its eight lower-crate dependencies and ten shared
  parent helpers explicitly; it introduces no command framework, context object, fallback, or new
  output type. Before extraction, the causal family occupied 1,452 contiguous lines and 13
  functions inside the 15,709-line `main.rs`; afterward `main.rs` is 14,255 lines and the family is
  a 1,468-line independently revertible module. The existing 38 causal CLI contracts plus explicit
  argument/stderr, raw-stdout, evidence, location, and exit-status controls preserve the process
  boundary. The first post-extraction representative change was test-only contract hardening and
  still touched one focused test file, so it did **not** reduce file count; no production causal
  follow-up has yet demonstrated lower edit count. This result supports clearer ownership but does
  not authorize extracting a second family or adopting a typed command framework.
- **Resolved output-boundary evaluation (#394):** retain family parsers and semantic producers as
  their existing tuple/JSON contracts, reject typed `Command`/`CommandOutcome`, and admit only a
  future native-process `Json`/`Raw` delivery value. That value is not implemented by this
  evaluation. Follow-up #441 is the separate, independently revertible implementation issue and may
  centralize only regular-command stdout and exit delivery; metadata/help, `fmt`, command-family
  parsing, semantic results, stderr side channels, Worker transport, and LSP remain outside it.
- `LiterateState`, atomic migration, verify cache, approval trust, and solver execution remain
  distinct state/failure owners rather than a single application state.
- **Candidate experiment (not authorization):** if selected through the C2 gate, first make
  dependencies explicit, then extract at most one command family in the independently revertible
  change. The causal family and its focused CLI suite are the first proposed experiment; a
  successful file move is insufficient unless a later representative change touches fewer
  unrelated responsibilities.

#### Native CLI output-boundary evaluation

The authoritative checked-in native CLI contract contains all 50 leaf commands after #442 added
the six causal leaves (`check`, `analyze`, `verify-expectations`, `observe-expectations`, `diff`, and
`ledger`). Seven parser dispatchers return `Result<(Value, i32), String>`, while 67 lower producers
return `(Value, i32)`. These counts distinguish process delivery from semantic and family outcomes;
a delivery concern is not evidence that all 74 boundaries need one application model.

| Output contract | Leaf inventory | Current process behavior |
|---|---|---|
| JSON only | all seven `ai` leaves; `chain`; `check`; `compat check`; `db check/observe`; `diff`; `domain analyze/check/generate/replay`; all three `approval` leaves; `document check`; `mutate`; `refine`; `replay`; `scenarios`; `sweep`; `verify`; `conformance`; `kernel`; `lint`; `migrate`; all six `causal` leaves | Shared pretty JSON stdout and normalized status. `chain` additionally retains its table on stderr. |
| JSON or raw success | `analyze`; `db import`; `domain expand/testgen`; `explain`; `html`; `ledger`; `document generate/claims`; `testgen`; `typestate` | Nine source sites extract successful content, print exact bytes, and exit. Failures and file-output modes remain JSON. Document generation also projects warnings to stderr. |
| Dedicated | `fmt`; `version` | `fmt` already uses its own `Source`/`Json` result and preserves check/error status. Version emits one raw line and exits. |
| Pre-dispatch metadata | `--cli-contract`; every `-h`/`--help` path | Exact embedded bytes bypass normal command dispatch. These are process metadata, not command outcomes. |

The nine raw-success sites represent eleven leaves because `testgen`/`html`/`ledger` share one
delivery site, reducing those three leaves to one site. `explain` and `typestate` share a parser arm
but retain separate delivery sites and distinct newline/multi-entity construction. Raw does not
mean success is always status zero: the existing status remains authoritative. `document claims`
raw output happens to be JSON text, but it is the RCIR artifact rather than the CLI envelope and
therefore remains `Raw`.

| Choice | Change surface | Removed duplication | Contract and migration risk | Decision |
|---|---|---|---|---|
| C0: tuples plus direct raw branches | No production change | None; nine success paths retain print/exit delivery | Lowest immediate risk, but every new raw mode must reproduce the delivery rules | Retain until the separately scoped C1 slice is ready |
| C1: minimal `Json`/`Raw` delivery value | Top-level regular-command emission, version, and the nine raw-success sites; existing family dispatchers and 67 producers stay unchanged | One stdout/exit owner for regular commands | Medium and bounded: exact newline, stderr, failure fallback, and exit behavior can drift | Adopt as the one-slice follow-up #441 |
| C2: typed `Command` and `CommandOutcome` | 50 leaves, seven dispatchers, and up to 67 tuple producers, with pressure to cross Worker/LSP boundaries | Could type parser and semantic variants, but no measured duplicate policy requires it | High coupling, large migration valley, and no production evidence of incremental benefit over C1 | Reject |

This decision is an implementation-local-optima audit of direct raw delivery, not a claim that
`Value` or local command parsing is inherently defective:

| Audit variable | Local evaluation | Expanded evaluation |
|---|---|---|
| `B` boundary | One command arm | The native process contract across all 50 leaves |
| `M` metric | Minimum local ceremony and exact nearby output | Contract preservation plus repeated delivery-change cost |
| `N` change scope | One parser/runner branch | Top-level emitter and all raw-success sites in one revertible change |
| `T` time | Deliver the current feature | Later raw modes and corrections across the CLI lifecycle |

| Evidence status | Record |
|---|---|
| Observed | 50 leaves, seven dispatchers, 67 tuple producers, nine raw-success sites, exact process tests, and the migration/DB-Domain/document commit sequence |
| Inferred | C1 can remove repeated regular-command delivery without moving the family or semantic boundaries; this must still be proved by the #441 diff and controls |
| Unverified hypothesis | Future raw additions will recur often enough to reduce total maintenance cost; production maintenance time, external callers, incidents, and team coordination are not observed |

| Evaluation boundary | C0 benefit and cost | C1 benefit and cost | Preferred with current evidence |
|---|---|---|---|
| Function | Direct writes make exact bytes obvious; no wrapper, but local exit terminates control flow | Adds a delivery variant and return construction | C0 |
| Module | Each raw arm is self-contained, while `main.rs` repeats delivery in nine places | One regular-command emission owner; stderr remains explicit | C1, narrowly |
| Feature flow | A feature author can finish locally, but must reproduce failure fallback and status rules | Feature output crosses one extra seam and needs exact controls | C0 for a single isolated feature |
| Native process | Shared JSON emission coexists with bypasses and direct exits | One regular-command stdout/exit policy without semantic unification | C1 |
| Operation/organization | No measured operational or multi-team penalty | No measured operational benefit | Insufficient evidence; tie |
| Lifecycle | Each later raw mode or correction can repeat the process rules | Later raw modes reuse one delivery seam, but must retain byte-specific construction | C1 if recurrence continues |

Direct branches were locally rational during native migration: they made byte-sensitive behavior
explicit and avoided forcing unrelated semantic results into a framework. The wider structure now
has nine copies of the same successful-output extraction/print/exit shape. History supplies an
independent evolution signal: the migration introduced the initial paths, while DB/Domain and
document features added further paths in separate commits. The later document-warning feature added
its envelope field and raw-mode stderr projection together because the raw artifact cannot carry the
envelope; this is a deliberately compensated side channel, not evidence of a prior production
failure or later correction. CLI maintainers and contract-test owners carry the bounded future cost
of preserving such compensations. There is no observed runtime, customer, multi-team, incident, or
production edit-count evidence, and #393's first follow-up did not reduce file count.

At function and family boundaries C0 remains preferable. At the native binary's regular-command
delivery boundary and lifecycle horizon, C1 becomes narrowly preferable because it centralizes one
process concern without changing semantic owners. C2 never becomes preferable in the observed
boundaries. The direct raw-delivery candidate is therefore `time-delayed`, severity 3/15
(`E1 A1 F0 K0 T1`), confidence C2 from agreeing structure and history. Boundary failure remains
zero because document warning routing shipped with the warning feature and no incident or
pre-correction failure is observed.

The counterfactuals are concrete and reversible: C0 retains current behavior; C1 changes one
delivery seam and rolls back by reverting one commit; C2 requires a coordinated parser/producer
migration and is rejected. C1's migration valley is one temporarily wider CLI diff with elevated
exact-byte and exit-risk across all raw branches; focused negative controls are the entry condition,
and reverting that single change is the rollback. C1 must not absorb stderr payloads into `Raw`,
copy `FmtCliOutput`, add a generic context, or make native and Worker transports share an artificial
command type. Intentional separation remains rational for `fmt`, metadata/help, Worker, and each
family's semantic model, so those are explicitly not local-optimum candidates.

Before a C1 implementation may merge, positive and rejecting controls must prove all of the
following:

| Contract | Required positive and negative control |
|---|---|
| Raw stdout | Exact bytes for every raw-capable leaf, including `print` versus `println`, trailing newlines, multi-entity typestate output, and the raw RCIR artifact |
| Failure fallback | A failing raw-capable invocation still emits the existing JSON error envelope rather than partial raw output |
| JSON evidence | Verdict, evidence, location, assurance, and envelope fields remain exact for representative success and failure cases |
| Exit | Preserve 0/1/2 and normalized internal error 3; raw delivery must use the producer's status rather than assume zero |
| Stderr | Preserve empty usage stderr where specified, document warnings, and the `chain` table independently of stdout delivery |
| Dedicated paths | Preserve metadata/help/version bytes and `fmt` source/check/error behavior exactly |
| Other transports | Native/Worker parity remains unchanged; the Worker continues its check/verify-only JSON contract |

Existing raw parity cases, `formatter_cli`, `document_cli`, typestate goldens, causal boundary tests,
and native metadata tests are useful anchors, but the implementation issue must add a rejecting
control for each newly centralized branch rather than treating green positive output as sufficient.
Re-evaluate the decision if #441 touches family producers, cannot preserve a stream contract without
a third delivery concern, or a real production follow-up contradicts the expected edit reduction.

### `fsl-wasm`

- `Request`/`Options` own decoded input, `MemoryResolver` owns per-call in-memory files, and each
  verification creates a fresh `Z3JsSolver`. Rust owns no persistent Worker or host-filesystem state.
- Keep the path request decode -> build/check/verify -> shared output projection -> JSON return.
  Browser lifecycle, cancellation, and solver teardown remain in the JavaScript Worker/client.
- Native and browser preparation differ because their host and async boundaries differ. Share
  semantic/result functions where already appropriate, and use parity rather than forcing both
  frontends through a native-oriented orchestration abstraction.
- **Current decision:** retain the cohesive Rust module. A split remains a C2 candidate even after
  one gains independent reuse or state; line count alone is insufficient.

### `fsl-lsp`

Logical direction:

```text
authoritative parse gate
  -> DocumentIndex editor projection
     -> open-document store + overlay resolver
        -> request/notification handlers
           -> stdio transport
```

- `DocumentIndex` remains ephemeral and source-positioned. It may token-walk only after the
  authoritative parser accepts the source and must not become a semantic model.
- `ServerState` alone owns latest open buffers, versions, and workspace roots. `StoreResolver`
  reads the open overlay before the filesystem.
- Keep `index.rs` separate from protocol/state. `server.rs` may remain physically combined. If
  overlay lifecycle or transport changes, extracting `store` before handlers is a C2 candidate so
  state and I/O can be tested without stdio.
- Completion, declaration recognition, and indexing currently have overlapping keyword tables. A
  shared LSP-internal vocabulary owner is a C2 candidate if the next grammar change touches more
  than one table; editor-role classification must not move into semantic crates.
- **Current decision:** retain the current two-module split and its corpus/stdio oracles; use the
  stated trigger rather than performing a speculative rewrite.

## 6. Candidates and adjudication

Mandatory baseline corrections apply to every candidate: raw-output modes remain raw, refinement
validation follows construction of both `KernelModel` inputs, the accepted ledger assurance
classifier remains authoritative, and the direct test-generation path normalization is recorded as
a bounded exception rather than silently declared compliant.

The candidates below compare future choices; none authorizes source movement by
itself. Selection still requires a dedicated issue and accepted scope/design.

| Candidate | Transformation | Benefit | Cost and risk | Result |
|---|---|---|---|---|
| C0 — retain undocumented internals | Apply only the mandatory factual/contract corrections; add no maintained role map or source change. | Almost zero transition work. | Wrong-direction imports, duplicate policy, and overloaded entry modules remain unowned; does not answer the design question. | Rejected. |
| C1 — publish the internal contract without source moves | Add this role map, hard gates, exceptions, target owners, triggers, and fitness functions. | Gives every future change a target owner while retaining full rollback and avoiding speculative file churn. | Does not itself remove any dependency defect or reduce `main.rs` edit breadth. | **Selected.** |
| C2 — one minimal local structural correction | In separate changes, move one exact duplicated or wrongly owned rule behind current re-exports. The baseline Domain naming, typecheck, liveness/trace, locale, path-input, and verification-renderer candidates have now been adjudicated independently. | Tests whether directional normalization reduces duplicate ownership with a narrow diff and rollback. | Visibility/import churn; each additional move still needs fresh evidence rather than inheriting authorization. | Completed slices authorize no further change; #397 selects C0 for core dialect/domain lowering. |
| C3 — immediately impose the full target module tree | Move all logical roles into physical modules now. | Uniform visible layout and smaller files. | Large review surface, merge pressure, and no demonstrated semantic or operational gain for cohesive adapters. | Rejected for current evidence. |

C1 is the least-regret current intervention under correctness-first, delivery-speed, and
flat-roadmap weightings. It does not claim that fewer lines per file improve correctness. C2 becomes
preferable only for a named local dependency or duplication with positive and rejecting oracles; C3
becomes preferable only if such small extractions repeatedly fail because the remaining physical
layout prevents isolation.

## 7. Transition, rollback, and fitness functions

This record is the complete C1 transition. If a later request selects C2, use independently
revertible slices in this order of evidence, not as one pre-authorized migration:

1. **Exact duplication (completed by #419):** consolidate the tools Domain naming transform.
2. **Neutral-policy slices (completed by #420, #421, and #422):** move core checked-type validation,
   verifier liveness/trace helpers, and document locale ownership one at a time. Preserve root
   re-exports and accepted feature contracts.
3. **Path-input slice (completed by #423):** test-generation canonicalization is owned by the
   delivery caller; generated pytest output and paths remain byte-identical for existing, missing,
   and symlinked inputs.
4. **Dependency slice:** replace `fslc/src/verification.rs`'s parent glob with explicit imports and
   remove forwarding/duplicate policy only after proving identical callers.
5. **CLI experiment (completed by #393):** the causal command family is extracted with unchanged
   arguments, JSON envelopes, raw modes, stdout, exit codes, and tool calls. Its first test-only
   follow-up did not reduce the one-file edit scope; require a production causal follow-up before
   claiming change-amplification improvement or extracting another family.
6. **Touch-driven slices:** when runtime, LSP store, or another command family changes, move only the
   affected logical owner and its focused tests.

Each slice rolls back by reverting that slice; there is no data migration, dual-write, feature flag,
or irreversible step. Entry requires a named owner, import direction, positive oracle, and rejecting
oracle. Exit requires unchanged public exports and schemas, no new upward dependency, focused tests,
and the relevant product gate.

| Intent | Minimum check |
|---|---|
| Syntax or core move | focused crate tests plus duplicate-write, unsupported Public Kernel, origin, and both Domain-lowering-path negatives |
| Runtime move | `fsl-runtime` tests plus failed-step rollback, replay rejection, explicit budget/unsupported-shape, and verifier transition agreement |
| Solver/adapter move | solver metrics and native backend contract; browser harness for JavaScript bridge behavior |
| Verifier move | BMC/induction/deadline tests plus altered-successor, failed-outcome, and corrupted-evidence agreement controls |
| Tools move | affected family tests, schema/digest/tamper negatives, and CLI contract tests for the family |
| CLI/Worker/LSP move | native integration, exact envelope/exit checks, browser parity/cancellation, LSP latest-buffer/corpus/diagnostic identity as applicable |
| Any completed code slice | formatting, Clippy, workspace tests/build, then `./tools/check-native-integration.sh` for release-significant changes |

## 8. Residual risk and reevaluation

Decision confidence is **medium**. Code and executable tests strongly identify state owners and
several wrong-direction dependencies, but there is no production telemetry, ownership map, measured
maintenance-time baseline, third-party Rust usage inventory, or blind temporal holdout. Evidence
collection was separated across semantic, tools, and delivery passes, but proposal and adjudication
still share one primary evaluator.

Re-evaluate this design when:

- a supposedly neutral helper needs state from two engines or changes their verdict independently;
- three command-family extractions still require broad imports or the same unrelated fixtures;
- a cohesive adapter gains a second independently versioned state/failure contract;
- a new grammar feature repeatedly changes multiple LSP vocabulary tables despite the shared owner;
- production incidents or profiling identify an isolation/performance boundary absent here;
- a public runtime or compatibility-binary consumer requires a lifecycle not visible in this
  repository; or
- a normal-maintenance holdout contradicts the short migration-period change clusters.

The recommendation is falsified if the neutral-owner moves increase semantic duplication, weaken a
negative control, or make representative changes touch more owners than before. In that case revert
the isolated slice and retain the logical contract without forcing its physical module tree.
