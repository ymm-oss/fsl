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
slice. The target direction is to correct observed wrong-direction dependencies in small,
behavior-preserving changes when the affected responsibility next changes. This record itself does
not authorize a workspace-wide file move or introduce a crate, public schema, language behavior,
solver protocol, or process contract.

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
| E-03 | E2 | `fsl-core::public_kernel` owns checked-model type validation used by `model` and `refinement`; `fsl-verifier::induction` imports liveness/trace helpers from `bmc`; document helpers import `document_render::Locale`; `fslc::verification` uses `super::*`. | Identifies concrete wrong-direction or hidden dependencies. |
| E-04 | E2 | `fsl-tools` has independent document, causal, specialized-dialect, and Kernel-derived families behind root re-exports; the document and causal families also have distinct schemas and focused histories. | Justifies family-level internal boundaries without a crate split. |
| E-05 | E1 | In `295508e9^..c37a81a1418f98c701e22729f3dff4d5e7ddbe5f`, 37 non-merge commits touched `fslc/src/main.rs`, while its command families already have separate integration suites and distinct failure/state contracts. | Justifies a command-family extraction experiment, not a new crate. |
| E-06 | E2 | Runtime rollback, explicit search, symbolic agreement, Public Kernel, document rendering, CLI, browser parity, and LSP tests contain rejecting cases. | Supplies existing fitness functions. |
| E-07 | E3 | Focused syntax/core/runtime/solver/native-Z3/verifier tests passed 219 tests, and `cargo check -p fsl-solver-z3js --locked` passed at the baseline. | Confirms the semantic internals used as design evidence are reproducible. |
| E-08 | E0 | Production load, third-party Rust API use, team ownership, and future feature frequency. | Limits eager restructuring and API-removal claims. |
| E-09 | E2 | `fmt`, readable explain, and TypeScript modes emit raw output; the accepted port contract requires raw modes to remain raw. `testgen::relative_spec_path` directly canonicalizes paths. | Bounds the CLI output design and records the existing tool-I/O exception. |

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
- **defect:** checked type validation owned by `public_kernel`, shared induction policy owned by
  `bmc`, document locale owned by its renderer, duplicated domain `snake` transforms, the native
  verification module's glob dependency on its parent, and test-generation path normalization
  performed inside the derived transform;
- **unknown:** external use of the public legacy runtime BFS and compatibility binaries, operational
  importance of those paths, and whether current change clusters will persist.

The internal placement defects are not asserted product bugs. They are observable dependency or
duplication problems whose correction must preserve existing behavior and negative controls.

## 4. Common internal rules

### 4.1 Facade and dependency direction

`lib.rs` is the public contract surface. In a multi-module crate it should converge toward explicit
module declarations and re-exports, not act as an unnamed shared namespace. Private modules import
the exact lower-level items they need; `use super::*` is forbidden in production modules. A
single-file protocol or backend adapter may implement its cohesive responsibility directly.

No module may own policy solely because it is a convenient import location. When two siblings need
the same rule, move the smallest rule to a neutral lower-level module; do not introduce a service,
registry, context object, or trait unless multiple implementations or state ownership require one.

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
inside semantic or derived-artifact transforms. `testgen::relative_spec_path` is the current bounded
exception that canonicalizes paths inside a transform. No sibling tool may copy that pattern, and
its caller must supply normalized path context before test-generation path semantics are changed.

Move-only refactors must retain both a successful oracle and a rejecting oracle. A successful build
or unchanged positive snapshot alone does not establish semantic preservation.

## 5. Component internal designs

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
- **Action:** retain the current physical layout. Add a module only when a new dialect has an
  independent grammar/value contract; do not split the large shared parser by production count.

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
- Move `validate_expression_type`, `validate_statement_types`, and
  `validate_model_expression_types` from the outward `public_kernel` projection to a neutral
  checked-model/typecheck owner used by `model`, `refinement`, and the exporter.
- Structural Domain lowering and generated-Kernel-source projection remain different outputs, but
  effect-outcome role normalization must have one low-level owner consumed by both paths.
- **Action:** perform those two policy moves when either area next changes; otherwise retain the
  current file tree and public re-exports.

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
  legacy BFS, and trace helpers. Their distinct state and failure contracts justify a target split
  into `eval`, `monitor`, `liveness`, `replay`, `refinement`, and `search`, while preserving root
  re-exports.
- **Action:** extract only the responsibility being changed, one move-only slice at a time. Do not
  perform an eager 2,600-line re-layout, and do not merge the independent symbolic evaluator here.

### `fsl-solver`

- Keep the cohesive single module ordered as immutable SMT vocabulary and result values, error and
  statistics values, `SolverMetrics`, then `SmtSolver`.
- `SolverMetrics` owns query attribution; an adapter owns its assertion stack and model state.
- **Action:** no structural change. Add a private module only if a second independent protocol
  responsibility appears; backend convenience is not sufficient.

### `fsl-solver-z3`

- `Z3Solver` alone owns the native solver, stack depth, version, and metrics. `Z3Term` is the typed
  backend handle; conversion and statistics helpers remain subordinate to the trait implementation.
- Seed/version setup stays in construction. Verification policy never enters this adapter.
- **Action:** retain one module. Any split must follow a new independently testable backend state,
  not term/helper count.

### `fsl-solver-z3js`

- `Z3JsSolver` owns Rust-side version, stack accounting, and metrics. The JavaScript bridge owns
  actual assertion/model state and opaque term handles.
- Promise-based `check` is the async failure boundary; Rust-only compilation cannot establish
  handle lifetime or bridge completeness.
- **Action:** retain one Rust adapter module and the separate JavaScript bridge. Browser parity,
  cancellation, rejection, and teardown remain mandatory external oracles.

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
  `bmc`. Move those helpers to neutral `liveness` and `trace` owners before extending either engine.
- `refinement` may compose the public bounded verifier, but it must not reach into another engine's
  private encoding.
- **Action:** make the neutral-helper move as one behavior-preserving slice, guarded by BMC,
  induction, deadline, and transition-agreement rejecting tests.

### `fsl-tools`

Keep one crate and one root facade over four internal families:

1. requirements documents: `document*`;
2. causal models, analysis, evidence, plans, and projections: `causal*`;
3. specialized AI, DB, and Domain engines;
4. Kernel-derived analysis, mutation, test generation, typestate, ledger, and reports.

Within each family, the direction is checked family model -> analysis/classification -> projection
or rendering. Values such as `RequirementClaimSet` and `CausalModel` are immutable results; private
builders/contexts are per-call accumulators. No family owns process lifecycle or artifact writes.

- Move `Locale` out of `document_render` to a neutral document presentation value so markers,
  glossary, expression rendering, checking, and the renderer do not depend upward on the renderer.
- Keep the assurance classifier shared by ledger and document evidence at
  `ledger::assurance_token`/`assurance_label`, as required by the accepted evidence-overlay design.
  Moving it to a neutral module requires that design to be explicitly superseded and is not part of
  this decision.
- Consolidate the identical private `domain::snake` and `domain_codegen::snake` transform into one
  Domain-family helper because it affects generated names in both paths.
- Retain the fail-closed `public_kernel` adapter shared by typestate, test generation, and Domain
  code generation. Do not merge those distinct tools merely because they share input validation.
- Treat `testgen::relative_spec_path` as a bounded I/O debt: when its path behavior next changes,
  move canonicalization to the caller and pass normalized path context into the pure generator,
  preserving generated pytest output and embedded paths byte-for-byte for existing, missing, and
  symlinked path inputs.
- **Action:** the future structural candidates are the locale owner, duplicated Domain naming
  helper, and test-generation path input. Decide and verify each independently. Do not replace the
  flat root API or create subcrates; family directories are optional presentation after the
  dependency graph is acyclic.

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
- `main.rs` should converge to metadata/help, top-level dispatch, serialization, and exit. Command
  families are formatting/migration, document/approval, causal, Domain/DB/AI, replay/scenarios,
  verification, analysis/mutation/reporting, and refinement/project composition.
- A family module parses its own options, owns its I/O preparation, calls lower owners, and returns
  its existing output plus status. JSON families retain `(Value, i32)`; `fmt`, readable explain,
  TypeScript, and other raw modes remain raw. If emission is later centralized, the maximum needed
  abstraction is a two-variant `Json`/`Raw` output value, not a command framework or generic context
  object.
- `verification.rs` retains prepare/solve/cache/finalize staging, but replaces `use super::*` with
  explicit imports. Shared native/Worker result policy belongs in `verification_output`; native-only
  selection and cache policy belongs in `verification`; `main` must not keep forwarding wrappers or
  duplicate property/output classification.
- `LiterateState`, atomic migration, verify cache, approval trust, and solver execution remain
  distinct state/failure owners rather than a single application state.
- **Action:** first make dependencies explicit, then extract one command family per change. Use the
  causal family and its focused CLI suite as the first reversible experiment; a successful file move
  is insufficient unless a later representative change touches fewer unrelated responsibilities.

### `fsl-wasm`

- `Request`/`Options` own decoded input, `MemoryResolver` owns per-call in-memory files, and each
  verification creates a fresh `Z3JsSolver`. Rust owns no persistent Worker or host-filesystem state.
- Keep the path request decode -> build/check/verify -> shared output projection -> JSON return.
  Browser lifecycle, cancellation, and solver teardown remain in the JavaScript Worker/client.
- Native and browser preparation differ because their host and async boundaries differ. Share
  semantic/result functions where already appropriate, and use parity rather than forcing both
  frontends through a native-oriented orchestration abstraction.
- **Action:** retain the cohesive Rust module. Split request/resolver/handlers only after one gains
  independent reuse or state; line count alone is insufficient.

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
- Keep `index.rs` separate from protocol/state. `server.rs` may remain physically combined until
  overlay lifecycle or transport changes; then extract `store` before handlers so state and I/O can
  be tested without stdio.
- Completion, declaration recognition, and indexing currently have overlapping keyword tables.
  Give shared editor vocabulary one LSP-internal owner if the next grammar change touches more than
  one table; do not move editor-role classification into semantic crates.
- **Action:** retain the current two-module split and its corpus/stdio oracles; use the stated trigger
  rather than performing a speculative rewrite.

## 6. Candidates and adjudication

Mandatory baseline corrections apply to every candidate: raw-output modes remain raw, refinement
validation follows construction of both `KernelModel` inputs, the accepted ledger assurance
classifier remains authoritative, and the direct test-generation path normalization is recorded as
a bounded exception rather than silently declared compliant.

| Candidate | Transformation | Benefit | Cost and risk | Result |
|---|---|---|---|---|
| C0 — retain undocumented internals | Apply only the mandatory factual/contract corrections; add no maintained role map or source change. | Almost zero transition work. | Wrong-direction imports, duplicate policy, and overloaded entry modules remain unowned; does not answer the design question. | Rejected. |
| C1 — publish the internal contract without source moves | Add this role map, hard gates, exceptions, target owners, triggers, and fitness functions. | Gives every future change a target owner while retaining full rollback and avoiding speculative file churn. | Does not itself remove any dependency defect or reduce `main.rs` edit breadth. | **Selected.** |
| C2 — one minimal local structural correction | In a separate change, move one exact duplicated or wrongly owned rule behind current re-exports; the smallest first candidate is the duplicated Domain naming transform. | Tests whether directional normalization reduces duplicate ownership with a narrow diff and rollback. | Visibility/import churn; no measured maintenance benefit yet. | Deferred until an implementation request or the affected code changes. |
| C3 — immediately impose the full target module tree | Move all logical roles into physical modules now. | Uniform visible layout and smaller files. | Large review surface, merge pressure, and no demonstrated semantic or operational gain for cohesive adapters. | Rejected for current evidence. |

C1 is the least-regret current intervention under correctness-first, delivery-speed, and
flat-roadmap weightings. It does not claim that fewer lines per file improve correctness. C2 becomes
preferable only for a named local dependency or duplication with positive and rejecting oracles; C3
becomes preferable only if such small extractions repeatedly fail because the remaining physical
layout prevents isolation.

## 7. Transition, rollback, and fitness functions

This record is the complete C1 transition. If a later request selects C2, use independently
revertible slices in this order of evidence, not as one pre-authorized migration:

1. **Exact duplication:** consolidate the Domain naming transform.
2. **Neutral-policy slices:** move core checked-type validation, verifier liveness/trace helpers, or
   document locale ownership one at a time. Preserve root re-exports and accepted feature contracts.
3. **Path-input slice:** move test-generation canonicalization to the delivery caller and preserve
   generated pytest output and paths byte-for-byte for existing, missing, and symlinked inputs.
4. **Dependency slice:** replace `fslc/src/verification.rs`'s parent glob with explicit imports and
   remove forwarding/duplicate policy only after proving identical callers.
5. **CLI experiment:** extract the causal command family without changing arguments, JSON envelopes,
   raw modes, stdout, exit codes, or tool calls. Observe the next representative causal change before
   extracting another family.
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
