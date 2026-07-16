# Changelog

The change history of this project. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versioning follows [Semantic Versioning](https://semver.org/). Each version corresponds to an annotated git tag (`v1.0.x`).

## [Unreleased]

### Added
- `fslc lint` now enforces a built-in, kind-aware canonical ID policy and accepts
  an explicit `--project fsl-project.toml` override. Requirement, acceptance,
  forbidden, policy, goal, control, model, and assumption IDs have distinct
  default templates; project tables can replace individual kinds while retaining
  all other defaults. Results record the resolved policy, invalid configuration
  fails closed, zero-padded numeric ID components retain their exact source
  spelling through parsing/lowering, and ID findings are deliberately non-machine-applicable so
  `migrate` never guesses cross-artifact renames. Typed `@requirement` annotations
  are the canonical declaration-link syntax; legacy `"ID: text"` metadata remains
  migration input.
- `docs/intro/language.ja.html` now renders from a new, section-aligned Japanese
  translation, `docs/LANGUAGE.ja.md`, instead of reusing the English `docs/LANGUAGE.md`
  body verbatim. FSL keywords, `fslc` commands/flags, diagnostic/result identifiers,
  JSON, and fenced code blocks stay untranslated and byte-identical to the English
  source; only headings and prose are Japanese. `tools/build_site_reference.py` fails
  loudly if the two files' `## ` section counts drift out of 1:1 alignment, and page
  anchors/`SECTION_BLURBS` lookups stay keyed off the English heading so cross-page
  links are unaffected. This supersedes the earlier "no translation" stance (see
  `docs/DESIGN-docs-site.md` D7); `docs/LANGUAGE.ja.md` now moves alongside
  `docs/LANGUAGE.md` under the "a language feature moves all of its files together"
  rule (`AGENTS.md`).
- Literate Markdown FSL: `fslc check`, `fslc verify`, and `fslc scenarios` now
  accept `.md` files containing ` ```fsl ` fenced code blocks directly. Lines
  outside fsl blocks are blanked in place so that diagnostic positions (line
  numbers, columns, counterexample locs) point to the original Markdown
  document. Multiple fsl blocks form one compilation unit; files without fsl
  fences are rejected with a clear diagnostic. Fence detection follows the
  CommonMark grammar (backtick or tilde runs of length >= 3, matched by
  character and length), so a non-fsl fence can safely contain a literal
  ` ```fsl ` example. The verify cache key is stable across repeated runs of
  the same document (issue #193).
- `fslc-lsp` is now a native Rust language server backed directly by the authoritative
  syntax, core, and analysis implementation. Existing diagnostics, navigation, symbols,
  rename, semantic tokens, completion, hover, and code actions no longer require Python;
  corpus/index coverage and stdio lifecycle tests run in the Rust workspace (issue #310).
- Required product CI now has one Rust-native integration entrypoint covering the workspace,
  dependency boundaries, CLI/schema contracts, native/browser agreement, and WASM. The redundant
  Python CLI parity job and its frozen-surface exception snapshot are removed; compatibility Python
  tests remain explicit manual evidence. FSL source locations, testgen templates, and generated
  artifact digests are portable across the supported native runner matrix (issue #307).
- `fslc analyze --projection code_audit --code <path>` now maps checked
  executable Kernel requirement targets to a closed, language-independent
  `@fsl.trace` annotation convention. Deterministic JSON reports complete,
  partial, missing, orphan, and target-mismatch coverage without presenting
  structural claims as proof; versioned input/output schemas and a compliant
  fixture define the public contract (issue #222).
- Replay-trace schema `1.2.0` opts into solver-free bounded `leadsTo`
  monitoring over initial, action, and stutter observations. Inclusive deadline
  failures carry property/binding/timing evidence; successful output separates
  safety from bounded liveness, preserves finite-prefix `pending`, and names
  unchecked unbounded properties. Native BMC and a test-only Python oracle
  cross-check the monitor, and positive/overdue NFR fixtures ship in both Public
  Kernel release bundles (issue #225).
- Replay-trace schema `1.1.0` adds explicit `action: null` observation points.
  Equal-state stutters preserve the projected action trace; reported transient
  implementation states are nonconformant while unreported intermediates are
  outside invariant judgment. Replay now delegates action outcomes, partial
  guards, and rollback to `Monitor::attempt`, with differential fixtures in both
  Public Kernel release bundles (issue #224).
- `fslc replay --trace` now consumes the closed `replay-trace.v1` external
  compiler contract: trace/Kernel versions, exact spec action/parameters,
  canonical ticks, complete typed initial and post-action state, and optional
  opaque producer timestamps. Typed observation differences return leaf-level
  nonconformance while malformed/incomplete contracts fail closed. Positive and
  negative goldens plus the schema ship in both Public Kernel release bundles;
  legacy action-only arrays/`{events}` remain an explicit unversioned adapter
  (issue #221).
- `fslc lint` now reports edition-aware diagnostics with stable taxonomy,
  severity, spans, canonical replacements, and machine applicability. `fslc
  migrate` is dry-run by default and `--write` atomically applies only a fully
  parsed, checked, location-free Public-Kernel-equivalent file set. Shared
  formatter edits cover legacy enums/operators and quantifiers; typed metadata,
  unambiguous local action correspondences, and implicit defaults use semantic
  planners. Unsafe comment movement, branch/duplicate mappings, and invalid
  `&&` are explicit refusals. The native LSP exposes applicable diagnostics
  as quick-fix Code Actions (issue #249).
- `fslc fmt` now formats one registered FSL document or stdin to canonical
  stdout without mutating it; `--check` accepts multiple paths and reports a
  machine-readable 0/1/2 result. A lossless token/trivia tree preserves line
  comments, blank lines, raw spelling, spans, and annotation attachment while
  domain enum/logical and quantifier legacy forms normalize to accepted
  equivalents. Corpus-wide idempotence and location-free semantic round trips
  cover every registered dialect; opaque agent bodies and ambiguous
  comment-bearing structural rewrites fail at exact spans (issue #248).
- `fslc verify --engine auto` composes the explicit-state and BMC engines:
  explicit runs first (faster, and can prove `closure: true`) and falls back
  transparently to BMC exactly when explicit cannot decide the spec on its
  own (an unsupported feature such as `leadsTo` or nondeterministic init, or
  `unknown_budget`). Every result carries `engine: "explicit"`/`"bmc"` naming
  whichever engine decided, and a fallback additionally carries
  `engine_fallback: {from, reason, kind}` so a caller can distinguish a
  bounded BMC verdict from an unbounded explicit one, and a permanent
  unsupported-feature gate from a transient budget one, without parsing
  prose. `auto` shares verdict-cache entries with plain `--engine
  explicit`/`bmc` runs of the same spec — the cache key is always the engine
  that actually decided, never `auto` itself — and does not change the
  default engine or extend to any other subcommand's `--engine` option
  (Rust-only, issue #226).
- Finite quantifiers, `count`, `sum`, `unique`, and `exactlyOne` now share one
  Binder/Aggregate IR across typed, range, Set, and Seq domains. Optional
  filters use one scope/type-check path; empty aggregates are zero and Seq
  duplicates preserve positional multiplicity. Legacy colon quantifiers remain
  detectable as non-canonical input, business/requirements KPI declarations
  survive as typed metadata projections, and collection aggregates normalize to
  existing Public Kernel v1/v2 expression shapes (issues #242 and #217).
- Requirements expressions now accept `stage(entity)` and qualified
  `<process-path>.stage(entity)` through the same structural node and resolver
  as business expressions. Entity-typed binders/parameters select the process;
  qualified process declarations disambiguate shared entity types; generated
  stage maps remain Kernel detail while public origins, explain output, and
  violations retain the source accessor (issue #243).
- Conditional expressions (`if condition then a else b`) are now accepted in
  every expression context. The shared parser replaces the refinement-only
  path; concrete, explicit, symbolic, analysis, mutation, formatter, LSP, and
  Public Kernel paths use one node, and partial operations are evaluated only
  on the selected branch (issue #245).
- `Option<T>` now supports structural `==` / `!=` between complete Option
  values, including `some(expr)`. Concrete and symbolic evaluation compare the
  presence tag first and ignore the payload when absent; `is some(binding)`
  remains the distinct binding form (issue #246).

### Fixed
- Native `check` now propagates malformed governance-preservation diagnostics
  as a located type error with exit 2 instead of silently returning `ok`. A
  resolver-backed core contract also rejects missing dependencies, unknown or
  unsatisfied controls, duplicate declarations, unknown artifacts, empty
  preservations, and referenced-name mismatches without last-write-wins data
  loss. Native CLI and browser Worker now share these checks and progress-
  preservation verification, including process-exit and browser parity cases.
- Model construction now rejects duplicate action names, duplicate action
  parameters, and empty inline parameter ranges before name-based runtime
  dispatch or finite-domain enumeration can become ambiguous.
- The native concrete `Monitor` now revalidates a previously enumerated action
  against the current state, rejects parameters outside their declared
  Bool/enum/range/domain, and shares one guard-evaluation path across
  enumeration, direct attempts, and selected steps. Builtin Bool action
  parameters are accepted as documented while unbounded/non-scalar parameters
  fail during model construction. A two-stage FSL requirements model captures
  fresh selection, stale reuse, and raw-input rejection; native
  symbolic/concrete differential tests and mutation evidence supply its
  implementation anchors and negative controls.
- Python compatibility envelopes now scope faithfulness routing to actual
  diagnostics, so user state fields named `kind`, `classification`, or other
  diagnostic discriminators neither crash nor receive routing metadata (issue
  #278).

### Changed
- Chronological field-trial reports were distilled into authoritative design
  contracts, language/skill rules, maintained examples, and executable regression
  tests, then removed as a parallel documentation source. The native pytest
  generator now also sanitizes composed scenario display names with deterministic
  collision suffixes while preserving the original name in the docstring. Literate
  Markdown commands now use process-isolated materializations while normalizing
  cache identity to the source document, preventing concurrent checks from deleting
  one another's input.
- Native Rust internals now separate top-level CLI routing from command-family parsing and base
  ledger generation from approval evaluation. Observable CLI, ledger, and approval digest
  contracts are unchanged.
- `fslc verify --engine auto` (issue #226) now dispatches through a
  `VerificationEngine::Auto` variant alongside the other engines instead of
  resolving before the shared parser, loads the model once per attempt (the
  static fail-closed gate and the real explicit-state run share it, removing
  a redundant load/gate re-check on the common decide-by-explicit path), and
  persists the fallback trace on the BMC cache entry itself so a repeat
  `auto` cache hit restores the exact original `engine_fallback` rather than
  recomputing a generic one. Output contract, cache-sharing with plain
  `--engine explicit`/`bmc` runs, and the `engine_fallback: {from, reason,
  kind}` shape are unchanged.
- Native domain TypeScript, Python, Kotlin, Swift, and Rust scaffolds now share
  one versioned input adapter over Public Kernel v1 and the public
  `domain-scaffold-metadata.v1` compatibility bridge. Target emitters no longer
  receive `DomainSpec`; incompatible schemas and missing lowered member
  counterparts are rejected, all five outputs match pre-migration goldens, the
  valid domain corpus generates for every target, and `domain testgen` reuses
  the same TypeScript adapter/effect emitter instead of a duplicate
  implementation (issue #213).
- Native pytest, Vitest, Swift, Kotlin, Dart, and PHPUnit test generation now
  shares one fail-closed adapter over Public Kernel v1 metadata, scenario JSON,
  and the new versioned `testgen-trace.v1` fixed-seed conformance trace. Target emitters no longer
  receive the private model/AST, and their byte output is unchanged. Compose
  uses an explicit checked names/order bridge until Public Kernel can publish
  truthful multi-file provenance (issue #214).
- Standalone refinement files, requirements `implements` blocks, action-level
  `maps` clauses, and synthesized auto/identity mappings now lower through one
  typed `ActionCorrespondence` IR. Impl parameter annotations, target arity and
  argument types, actor compatibility, duplicate origins/spans, progress lookup,
  and concrete refinement execution share that validation path; requirements
  auto-mapping no longer has a separate indexing path (issue #238).
- Native/WASM parity now runs every Worker-supported surface document from the
  shared `specs/` and `examples/` corpus and structurally compares complete
  envelopes. Only schema-validated timing/backend identity and replay-validated
  witness state values and commuting-step order are normalized while state
  keys, action/source identity, and change shapes remain exact; structured failures retain difference paths,
  both envelopes, and version metadata. Native and browser Z3 seeds are fixed,
  and the gate includes a pre-#267 duplicate-write regression (issue #273).
- Distribution is now fail-closed: every Rust workspace crate is marked
  non-publishable, the dormant PyPI workflow and obsolete Python/PyInstaller
  binary path are removed, and GitHub Releases are documented as the only
  official distribution surface (issue #272).
- Native, induction, explicit, and browser verification results now share one
  fixed `cost` schema. Z3 backends report check count/time, nullable common
  conflict/decision/propagation/memory statistics, and deterministic
  per-property check attribution; native and Worker parity tests enforce the
  same keys and nullability (issue #271).
- Native CLI and browser Worker result envelopes now identify the verifier,
  `fsl-core`, and loaded Z3 versions through one `versions` schema. Native
  verdict-cache keys and entries use the linked Z3 runtime version instead of a
  hard-coded release string (issue #268).
- Domain finite variants now use canonical `enum Name { Member, ... }` syntax,
  while bounded numeric domains remain `type Name = lo..hi`. The 2.x legacy
  union spelling remains compatible with a stable
  `deprecated_domain_enum_union` diagnostic and loss-aware replacement;
  `--edition next` rejects it. Canonical and legacy forms share the same typed
  model, Kernel lowering, and verdict, and public examples now use the
  canonical spelling (issue #244).
- Native Rust domain lowering now carries a non-serialized origin chain from typed
  domain nodes through Surface/Kernel models into validation, verification,
  counterexamples, and `explain`. Source identity/full spans, declaration
  paths, lowering steps, primary/secondary sources, one-to-many/many-to-one,
  and generated-only nodes remain distinguishable; requirement traceability is
  stored separately. Diagnostics prefer user declarations and retain generated
  Kernel names as machine detail. Public Kernel v1 schema/version/goldens and
  default `fslc kernel` output remain unchanged (issue #240; v2 publication is
  tracked by #256). Rust consumers should construct parser/model values through
  the supported parse/build APIs rather than struct literals because the
  internal span and origin carriers add fields to those implementation types.
- Native Rust domain lowering (issue #239) now resolves typed domain symbols,
  enum members, `can()`, finite membership, state reads, and nested lvalues
  structurally, then builds Kernel AST directly without generated-source
  render/reparse or substring semantics. Resolver/type failures report original
  domain spans; empty membership and map-state initialization are explicit;
  characterization now records direct-lowering origins and fail-closed checks.
- Native Rust domain parsing (issue #236) now retains defaults, ranges, guards,
  rejection conditions, assignments, invariants, stale policies, effect
  correlation/idempotency expressions, and saga guards as unresolved typed
  syntax nodes with exact source spans. Field identifiers/type references and
  declaration nodes are span-aware as well. Domain-only membership and legacy
  logical spellings are structural, effect paths retain their prior restricted
  grammar, existing lowering remains compatible through a rendering adapter,
  and malformed expressions now report their original domain-source location
  instead of a generated Kernel coordinate.

### Fixed
- Native and browser verification now share duplicate-write validation and
  verification warning generation. Duplicate writes are rejected while building
  the checked Kernel model for `check` and `verify`; model, vacuity, deadlock,
  and action-coverage warnings come from solver-independent shared crates rather
  than frontend-local implementations. The Worker no longer accepts ambiguous
  write order or emits an unconditional empty warning list. Indexed writes that
  may alias are rejected unless constant indexes prove them distinct, and
  induction selects typed warning kinds instead of message substrings (issue
  #267).
- The bounded `leadsTo ... within N` deadline check no longer misses a
  violation when the path deadlocks after the deadline. The deadline probe ran
  as a single post-hoc pass after the BMC unrolling loop, by which point every
  non-final step had a permanently-asserted forward transition, so a
  missed-deadline path that subsequently deadlocked was excluded from the
  solver context and the spec falsely verified at any `--depth` past the
  deadlock. The probe now runs inside the per-step loop, anchored at each
  window's deadline step before that step's forward-transition assertion —
  the same restructuring issue #260 applied to the stagnation check
  (issue #266).
- Bounded `leadsTo` deadlock-stagnation detection no longer requires `--depth`
  to land exactly on the stalling step. The BMC unrolling loop permanently
  asserted a forward transition out of every non-final step before the
  deadlock-stagnation check ran as a single post-hoc pass, so a deadlocked
  path below the requested depth was excluded from the solver context and the
  spec falsely verified. The check now runs inside the per-step loop, before
  that step's forward-transition assertion, matching the frozen Python
  reference's existing behavior (issue #260).
- The browser Worker now reports `leadsTo` violations. The Worker envelope
  previously ignored the bounded verifier's leadsTo verdict, so a spec whose
  `must eventually` policy is violated was reported as `verified` in the
  browser while the native CLI reports `violated` — a confidently green false
  negative. The Worker now emits the native CLI's leadsTo fields (bindings,
  `pending_since`, `stutter`, `hint`, and the counterexample trace).
- The browser Z3 bridge now builds if-then-else terms through the z3-solver
  `If` API and resolves constant sorts from their FSL sort descriptors instead
  of a TypeScript-only `__typename` marker that does not exist at runtime.
  Previously any spec with `Bool` state or `count` aggregation failed in the
  browser Worker with an internal error while verifying natively.
- Windows native CI now keeps the conformance coverage and approval Markdown
  snapshots on LF and canonicalizes both the specification and repository-root
  paths before binding approval records to Git, avoiding false snapshot drift
  and false "outside its Git repository" errors without weakening exact checks.
- Rust CLI contract validation (issue #220) now preserves exact compatibility
  with the frozen Python surface while checking native-only commands and
  options against an explicit structural allowlist. CI runs the focused
  contract and help-parity suite, including a mutation regression that rejects
  unlisted choice drift and runtime probes for invalid choices and help paths.

### Added
- Native Rust now accepts `@requirement(id, text?)`, `@undecided(reason)`,
  `@kind(id, text?)`, and multi-segment custom-namespace annotations directly
  on a declaration (`init`, `action`, `invariant`/`trans`/`reachable`/`until`/
  `unless`/`leadsTo`, a process `transition`, and `requirement`/`acceptance`/
  `forbidden` blocks) in the spec/business/requirements/compose dialects, not
  only before a document's dialect keyword. A shared parser helper backs both
  attachment points — no per-dialect copy. The new syntax and the legacy
  string-metadata/`covers` forms desugar to the same typed relation and union
  when both target one declaration; a requirement block's annotations fan out
  to every action/property it contains. A malformed or stray annotation
  (nothing to attach to) reports a precise coded diagnostic
  (`FSL-ANNOTATION-TARGET`/`-ARGUMENTS`/`-PATH`/`-SYNTAX`). Public Kernel v1/v2
  JSON and `python_ast` projections remain unchanged (issue #241).
- Native Rust now also accepts the same declaration-level `@...` syntax on
  `domain` (aggregate `command`/`decide`/`evolve`/`invariant`, `projection`,
  `effect`, saga `step`), `ai_component` (`tool`, `authority` and its
  individual rule lines, `fallback` and its `when` items, `check`), and
  `dbsystem` (`migration`, `check compatibility` rule lines) nested
  declarations. A `command`/`decide`/matching-`evolve` group unions onto the
  one action they generate together; an `effect` or saga `step` broadcasts to
  every action it generates. `AiAuthority`'s `may_suggest`/`may_execute`/
  `requires_human_approval`/`forbidden` rule lists are now span/annotation-
  carrying `AiAuthorityRule` nodes while `python_ast()` still projects the
  original plain string-array JSON shape. A `dbsystem` migration or
  compatibility-rule annotation reaches the checked model and TSG/ledger by
  rendering back to `@...` source rather than through the lossy legacy
  `quote_meta` string convention. Public Kernel v1/v2 JSON and `python_ast`
  projections remain unchanged (issue #281).
- Approval records can now opt into detached Ed25519 signatures with
  `approval create --signing-key`. The strict v2 schema binds the complete
  canonical record, while `approval check`, `approval diff`, and `ledger`
  require explicit repeatable trust anchors and distinguish signed, unsigned,
  and signature-invalid evidence. Unsigned v1 behavior is unchanged (issue
  #269).
- Native dialect selection now lexes each document once and dispatches through a
  duplicate-checked keyword registry shared by Kernel, CLI, WASM, and mirrored
  Python/LSP entrypoints. Leading BOM/comments/whitespace and typed top-level
  annotations are handled before selection without mistaking annotation arguments
  for dialects; specialized frontends consume the original token stream. Add
  exact empty/unknown diagnostic codes, spans, and supported-keyword lists, an
  evidence-only `agent` surface, and span-retaining multi-segment `SymbolPath`
  values. Qualified syntax shares the same arbitrary-depth path and per-segment
  spans, dispatch errors expose machine-readable supported dialects, and LSP
  diagnostics route annotated agent/AI-project sources consistently (issue #247).
- Native Rust now carries ordered, typed `Requirement`, `Undecided`, `Kind`,
  and namespaced `Custom` annotations through a common target-keyed IR. Legacy
  declaration strings, spec badges, requirement blocks, process `covers`, and
  acceptance/forbidden IDs adapt into the same validated relation; identical
  requirements deduplicate, conflicting text fails at its annotation span, and
  semantic queries are order-independent. Explicit requirement forms retain
  exact spans and reject reserved `undecided` IDs. Strict tags, analysis/TSG,
  scenarios, verifier diagnostics, and ledgers consume every relation while
  retaining lexical singular projections. Public Kernel v1/v2 JSON remains
  unchanged (issue #237).
- Opt-in Public Kernel v2 publishes the internal domain origin chain as a
  deterministic provenance graph with portable typed source identities,
  UTF-8 byte and Unicode-scalar coordinates, primary/secondary origins,
  lowering steps, generated-only/unknown assurance, exact-revision source-node
  IDs, target bindings, reverse lookup, and machine-readable completeness.
  Rust and CLI callers explicitly negotiate `PublicKernelVersion::V2` /
  `--kernel-version 2`; unsupported majors fail closed. Add matching v2 Kernel,
  conformance, and provenance-coverage schemas/goldens plus an independently
  checksummed v2 release bundle. Public Kernel v1, its goldens, default CLI,
  and compose rejection remain unchanged (issue #256).
- The browser Worker verify envelope now carries the counterexample `trace`
  (per-step state, action, and changes) for invariant and type-bound
  violations, matching the native CLI trace shape. Browser clients previously
  only received the violated property name and step.
- Kernel state fields now accept deterministic inline initializers and normalize
  them to the existing `init` assignment semantics across Monitor, explicit
  exploration, BMC, induction, and Public Kernel v1. State-reading/order-dependent
  expressions and inline/explicit root overlap fail with source diagnostics.
  Existing domain enum/Bool/range/external defaults and requirements number
  lower-bound defaults now emit the edition-aware `implicit_initial_value`
  warning with the selected value, reason, and machine-applicable insertion edit;
  requirements Bool/enum fields remain explicitly initialized (issue #250).
- Domain-expression characterization corpus (issue #235): Rust CI now freezes
  pre-typed-AST surface expressions and locations, normalized semantic model
  structure, representative public Kernel expression/origin output, generated
  Kernel fragments, CLI diagnostics and verification traces, and concrete
  Monitor-to-symbolic agreement. The corpus covers logical operators, enums,
  membership, `can`, aggregate state, root/index/field lvalues, defaults,
  invariants, stale policies, effects, sagas, invalid expressions, and a
  deterministic AI-native prompt/spec attempt baseline. This is migration
  evidence only and intentionally changes no language semantics.
- Codex CLI, IDE, and desktop environment: bounded repository instructions, worktree-local
  task packets, explicit checkpoint/task-start skills, shared FSL skill discovery,
  read-only exploration and review agents, concise SessionStart context, and CI-tested
  configuration contracts.
- Rust-authoritative Claude Code environment: concise shared instructions, path-scoped rules,
  worktree-local task packets, checkpoint/task-start skills, native verifier hooks, focused
  exploration/diagnostic/review agents, publication permission prompts, and CI-tested hook contracts.
- Conformance corpus feature coverage matrix (issue #223): native Rust
  `fslc_rust::coverage::coverage_matrix()` structurally cross-references the
  fixed `kernel_contract.fsl`/`conformance_failures.fsl` fixture manifest
  against their generated contracts and conformance vectors, reporting for
  every kernel `semantics` key, `outcome.kind`, value encoding, partial
  operation, quantifier, finite parameter domain, and requirement ID whether
  it is `exercised` (a real vector demonstrates it) or `declared` (the
  contract states it but no vector in the corpus can fire it — reserved for
  `terminal_deadlock`, `fairness_weak`, and the six `partial_op_*` rows). No
  row is hardcoded; `coverage_matrix()` itself returns `Err` naming every
  feature that falls short of its required level, so a newly uncovered
  feature fails CI loudly instead of shipping silently. Closes real gaps the
  matrix caught: the corpus never fired a `trans` violation, never
  distinguished Euclidean from truncating integer division, and never
  carried a `Bool` value — `conformance_failures.fsl` gained a
  `regress`/`Monotone` pair, a literal negative-operand `euclid_divide`
  action, and a `flag`/`flip` pair, and its golden vectors were
  regenerated (also updating `fsl-verifier`'s failure-fixture agreement test
  to check the new successful transitions with the same Monitor↔symbolic
  path used for `kernel_contract.fsl`, since not every action in the fixture
  is a deliberate failure anymore). Add
  `schemas/fslc/kernel/conformance-coverage.v1.schema.json`, golden JSON/
  Markdown fixtures, `rust/fslc/tests/conformance_coverage.rs` (schema/
  outcome-kind sync, required-level enforcement, golden equality, schema ID
  sync), a release-bundle update, and a "Conformance coverage matrix"
  section in `docs/DESIGN-kernel-contract.md`. No new CLI surface; the
  frozen Python reference remains unchanged.
- Public-Kernel-backed native typestate generation (issue #215): Rust
  `fslc typestate` now performs applicability analysis and TypeScript scaffold
  generation from versioned public Kernel JSON v1 instead of private
  `KernelModel` structures. The adapter validates schema identity/version,
  restores declaration order from public spans, and preserves existing report
  and `--ts` bytes with golden tests. The old private-model adapter is retired
  from the native CLI path; the frozen Python reference remains unchanged.
- Explicit-state exploration engine (issue #212): native Rust
  `fslc verify --engine explicit` enumerates the concrete state space on the
  Z3-free path (`fsl-runtime` BFS with `BTreeSet` dedup and parent-link
  traces). Closure — no new states within `--depth` — returns `proved`
  (`closure: true`), a complete unbounded proof that needs no lemmas even for
  true-but-not-inductive invariants where k-induction reports `unknown_cti`;
  depth exhaustion returns bounded `verified`; exceeding `--explicit-budget`
  (default 1,000,000 visited states) returns the new `unknown_budget` verdict
  (exit 1), never a silent `verified`. Violations reuse the BMC
  shortest-counterexample trace schema, and results carry `states_explored`,
  `max_frontier_width`, and `depth_reached`. Fail-closed rejections keep the
  engine sound: `leadsTo` properties, nondeterministic `init`, and
  `init forall` binder domains referencing state variables (both reference
  engines require compile-time-constant domains). Verdict-cache keys include
  the engine and budget. Includes corpus verdict-agreement and trace-replay
  integration tests, `docs/DESIGN-explicit-engine.md`, shared FSL skill
  guidance, and `tools/bench_explicit.py`; the frozen Python reference
  implementation is intentionally unchanged.
- Versioned normalized public Kernel contract (issue #208): native Rust
  `fslc kernel` exports checked/lowered models as typed, source-traceable JSON
  without the Python AST, while `fslc conformance` emits deterministic
  language-neutral success/disabled/rollback vectors. Add v1 JSON Schemas,
  requirement/lowering origins, explicit partial-operation conditions, atomic
  Monitor rollback for all failed steps, a Monitor↔symbolic transition agreement
  gate, golden fixtures, compatibility policy, release bundle, and external
  compiler guidance. The frozen Python reference remains unchanged.
- Digest-bound approval records (issue #190): add native Rust `fslc approval
  create|check|diff` for reviewed ledger, HTML, and scenario artifacts. Versioned
  sidecars bind a location-insensitive lowered-kernel digest, normalized rendered
  artifact digest, generation inputs, requirement IDs, approver, timestamp, and
  reconstructable Git baseline. `fslc ledger --approval` now shows per-requirement
  `approved` / `drifted` state with the complete baseline digest and a direct
  semantic-diff command. Includes JSON Schema, snapshot/integration coverage, and
  `docs/DESIGN-approval.md`; the frozen Python reference remains unchanged.
- Intentional undecided declarations (issue #189): reserve the
  `"undecided: reason"` metadata tag, including tagged `init`, and add
  dependency-derived affected requirement IDs to new ledger and HTML undecided
  sections. Bounded underspecification findings remain visible and now distinguish
  exact declaration matches with additive `acknowledged` / `acknowledged_by`
  fields. Includes native Rust integration/snapshot coverage and
  `docs/DESIGN-undecided.md`, shared FSL skill guidance, and bilingual manual-site
  coverage for the syntax and acknowledged-review workflow. The frozen Python
  reference implementation is intentionally unchanged.
- Complete the native Rust migration for issue #195: generic `check` and bounded
  `verify` now agree with the Python reference across all 181 FSL corpus files,
  database documents lower to executable compatibility kernels, collection
  binders and `unique` preserve language semantics, multi-fragment initialization
  is retained, and AI project/agent routing plus parse-error classification match
  the public CLI contract. Port full domain scaffolding and requirement-oriented
  ledger rendering (including implementation logs and external evidence) without
  a Python command fallback. The installer now places the checksummed native Rust
  release on the public `fslc` path; Python remains an optional LSP and reference
  implementation.
- Rust port Phase 0 (issue #195): add a deterministic Python AST JSON exporter
  that serves as the reference oracle for cross-language parser differential
  tests. It covers every `.fsl` file under `specs/` and `examples/`, records
  evidence-only frontends explicitly, preserves source-relative resolution, and
  never silently drops parse/lowering failures. Add the initial typed Rust
  expression parser, typed kernel `spec` and refinement surface parsers with
  178/178 parseable surface-corpus AST parity, and `fsl-core` spec/compose
  lowering with 81/81 kernel AST parity. Add a typed kernel model, independent
  concrete Monitor/BFS, backend-neutral async-check solver interface, pinned
  native Z3 4.16.0 backend, and incremental symbolic BMC. Python BFS, Rust BFS,
  and Rust BMC agree on the decision surface for all 20 monitorable `specs/`
  programs at depth 3. Native `check`/`verify` match the existing stable snapshot
  projection for all 23 `specs/` files at depth 5; native scenario identities
  match 20/20 and all 86 generated scenarios cross-replay. BMC witnesses are
  replayed in both implementation directions. Full `check`/`verify` envelopes
  match 43/43 command cases with a reviewed witness-only allowlist, and bounded
  `leadsTo` checking covers deadlock stutter, deadlines, and fair lassos with
  3/3 focused liveness-envelope and cross-replay parity. Phase 2 starts with
  native k-induction (11/11 focused base/CTI/ranking envelopes) and bounded
  refinement including progress preservation (6/6 focused envelopes). Add exact Python↔Rust differential runners, plus a
  pinned `z3-solver` 4.16.0 disposable-Worker round-trip/throughput spike and CI
  gate. Add native Phase-2 sweep/refinement-chain/project-chain commands, typed
  DB/AI/domain tooling and Phase-3 report command entry points with a 107/107
  differential gate covering exact typestate and full built-in/external mutation
  adjudication (including requirement and refinement attribution), full
  explain/counterfactual output, byte-identical pytest and five alternate
  testgen targets (including forbidden rejection cases), core analysis graph
  JSON plus DOT/Mermaid exports (including standalone refinement mappings),
  declaration-level tag-review export, mixed spec/refinement batch analysis,
  project traceability and missing-anchor findings, structural AI-review
  findings (tag drift, unanchored properties, unread/unwritten state, and
  unguarded actions, progress-cycle detection/suppression, bounded unconstrained
  effects, conservation candidates, and acceptance-backed divergent choices),
  focused byte-identical HTML/ledger artifacts, raw CLI output parity for
  version, typestate, testgen, explain, domain expansion/test generation,
  HTML, and ledger output, and broad HTML tag/attribute
  structure parity across workflow, inventory, vending, and forbidden specs. Add
  the production Rust WASM Worker, official npm Z3
  bridge, COOP/COEP playground assets, and a headless-browser gate that proves
  forced cancellation recovery and native/WASM verdict parity. Fix stale
  successor-expression caching that produced false-positive divergent-choice
  findings when CPython reused short-lived state-object ids. Broad HTML parity
  compares byte-identical static content plus complete tag/attribute structure;
  solver-selected dynamic witnesses remain covered by bidirectional replay. See
  `docs/RUST-PORTING.md` for the repeatable rewrite method and current evidence.
  Complete the native CLI option contract, persistent verification cache,
  snapshot and implementation-log replay, auxiliary-lemma proofs, semantic
  diff (including Git tree inputs), expression fuzz agreement, and native
  runtime/tool/CLI regressions. Release artifacts now include Intel macOS.
- Rust port architecture (issue #195): an accepted, phased design for a
  repository-local Cargo workspace that targets a native single-binary CLI and a
  browser Web Worker/WASM kernel backed by the official `z3-solver` npm package.
  The design makes the Monitor/oracle solver-independence a crate-graph invariant,
  freezes the JSON/exit-code contract, and defines parser, envelope, trace-replay,
  and corpus-parity gates for every migration phase. This is a design milestone,
  not a claim that the Rust implementation exists. See
  `docs/DESIGN-rust-port.md`.
- Declaration tag drift review (issue #188): `analyze --profile ai-review`
  gains deterministic `tag_stale_reference` and `tag_formula_disjoint`
  findings based only on exact code-shaped identifiers. `analyze --export
  tag-review` emits tagged actions/properties with rendered formal definitions
  under the new `tag-review.v0` schema for declaration-local external review;
  it never calls a model or promotes wording judgments to violations. See
  `docs/DESIGN-tag-drift.md`.
- Named predicate frontend sugar (issue #187): file-local `def name(p: Type) =
  expr` declarations can be used in kernel, requirements, and compose
  expressions. Calls are inlined before model construction, keeping
  model/BMC/runtime unchanged and equivalent to hand expansion. Unknown calls,
  arity mismatch, direct/mutual recursion, shadowing, and capture-changing
  substitution are explicit diagnostics. Includes a tagged flagship example;
  see `docs/DESIGN-def.md`.
- Git-aware semantic diff (issue #186): `fslc diff --git BASE..HEAD [SPEC]`
  materializes both complete tracked trees before delegating to the
  VCS-independent #176 comparison, so imports resolve from their own revision.
  Omitting `SPEC` compares every changed `.fsl` file. JSON records both commit
  hashes and `git_archive_full_tree`; batch gates aggregate child `--forbid`
  violations. See `docs/DESIGN-diff-git.md`.
- Bounded underspecification findings (issue #179): `fslc analyze --profile
  ai-review` now uses a fixed depth-4 BMC probe to emit `divergent_choice` when
  two distinct actions are enabled in the same reachable state and split an
  invariant/acceptance outcome, and `unconstrained_effect` when an unread state
  can receive different next values from two enabled actions. Findings include
  `evidence_basis:"bounded_bmc"`, reachable branch/successor evidence, and a
  question-form `spec_question`; they remain review-only
  `formal_status:"not_a_violation"`. Strong semantic findings suppress duplicate
  `unread_state`/`unguarded_action` approximations, while structural findings
  remain when no bounded witness exists. The v0 schema gains additive optional
  question/evidence fields. See `docs/DESIGN-underspecification.md`.
- External mutation adjudication (issue #178): `fslc mutate SPEC --from
  mutants.jsonl` appends externally generated full-spec or exact-replacement
  mutations to the built-in catalog and judges valid records through the same
  BMC/acceptance/forbidden/refinement oracle. Malformed JSON/instructions,
  duplicate ids, parse/name/type/construction errors, and spec-name changes are
  first-class `invalid` generation-quality findings rather than false kills.
  Mutants now carry `source:"builtin"|"external"`; combined and per-source
  summaries include `invalid` and kill rates over killed+survived only.
  `--max-mutants` caps the built-in catalog, enabling external-only runs with
  `--max-mutants 0`. See `docs/DESIGN-mutate.md`.
- Verified auxiliary lemma candidates for k-induction (issue #177): repeatable
  `fslc verify --engine induction --lemma "EXPR"` independently proves each
  candidate using the original transition system and implicit bounds, without
  assuming the original user invariants. False/non-inductive/invalid candidates
  are rejected with their own counterexample, CTI, or parse/type result; only
  `proved` candidates can strengthen the target proof. The retry loop evaluates
  candidates in each target CTI model, records which lemma excluded which CTI
  and steps under `lemma_cti_exclusions`, and on final `proved` emits source
  declarations under `auxiliary_invariant_recommendation` without rewriting the
  file. Lemma text/order is included in the persistent verify cache key, and
  BMC use is rejected as a usage error. See
  `docs/DESIGN-induction-lemmas.md`.
- Semantic specification diff (issue #176): `fslc diff OLD.fsl NEW.fsl
  --depth K` classifies bounded changes through bidirectional auto-refinement
  (`behavior_added` / `behavior_removed`), SMT implication of user-invariant
  conjunctions (`invariant_weakened` / `invariant_strengthened`), and replay of
  OLD negative scenarios against NEW (`forbidden_relaxed`). Directional
  differences include counterexample witnesses; incompatible names are
  explicit `unknown` unless a one-direction `--mapping` is supplied. Changes
  to `verify` entity/number bounds are first-class `scope_changed` findings and
  comparisons record/use NEW's scope. Findings are analysis (exit 0); only an
  explicit `--forbid kind,...` gate exits 1. See
  `docs/DESIGN-semantic-diff.md`.
- Predictive BMC from a state snapshot (issue #175): `fslc verify SPEC
  --from-state state.json` replaces the declared init with a complete
  Monitor/replay logical-state JSON and searches for violations from that point.
  Snapshot runs are BMC-only, bypass the verdict cache, preserve concrete
  identities by disabling symmetry reduction, and stamp
  `faithfulness.scope:"bounded_from_snapshot"` with `spec_init:"not_used"`.
  Full type/shape validation covers every kernel state type; partial snapshots
  are rejected rather than treated as unconstrained. See
  `docs/DESIGN-from-state.md`.
- Production-log replay mappings (issue #174): `fslc replay SPEC --from-log
  events.jsonl --mapping log_mapping.fsl` reuses the refinement parser and
  mapping expressions to translate external action/parameter names and observed
  post-action state into the target spec. The Z3-free Monitor stops at the first
  rejected action, mapping failure, or state mismatch and reports both the
  zero-based record index and one-based JSONL line. The initial contract requires
  complete observed state; missing fields are nonconformant rather than silently
  unconstrained. See `docs/DESIGN-log-replay.md`.
- Assurance classes (issue #171): a shared `fslc.assurance` classifier turns
  every command's result dict (BMC `verify`, k-induction `prove`, and the
  fsl-ai/fsl-db/fsl-domain `formal_result:"not_run"` producers — replay,
  observe, eval/regress/compare/drift, compat, agent/project analysis) into
  one of `proved(induction)` / `bounded(BMC depth k)` / `replay-observed` /
  `statistical(Wilson c%)` / `not_run`. `fslc ledger` gains a 保証クラス
  column (per requirement id, from the weakest of its tagged elements) plus a
  new `--engine bmc|induction` flag and a repeatable `--evidence
  <result.json>` flag to fold external evidence into the classification; a
  `## 外部エビデンス` section lists evidence not tied to a requirement id.
  `fslc html` gains an Assurance row/column and the same `--engine` flag.
  Presentation-only: no change to verification semantics, the JSON envelope,
  or exit codes. See `docs/DESIGN-assurance-classes.md`.
- Counterexample blame assignment (issue #170): a `violated` result with
  `violation_kind` `invariant`/`type_bound` now carries top-level
  `blame.conjuncts[]` (which AND-conjunct of the invariant is false, with
  violating bindings) and per-step `blame: {guards[], effects[]}` on the
  trace (a backward slice naming the `requires` clauses and state-writing
  statements that fed the blamed conjunct(s), verified to exclude untouched
  sibling variables, not just list every write). `fslc explain`'s
  counterfactuals inherit both automatically with zero explain-side logic.
  `vacuous_implication`/`vacuous_leadsto` findings gain `classification`
  (`insufficient_depth`/`over_constrained`) and `blocking` (the invariants
  actually making the antecedent/trigger impossible) — reusing the existing
  reachable-diagnosis unsat-core machinery, with a fix for a self-reference
  bug found while wiring it up (a vacuous_implication's own invariant was
  always appearing in its own blocking core). All additive; no change to
  existing fields or exit codes. Moved the AST-to-text renderer out of
  `explain.py` into a new leaf module `src/fslc/render.py` so `bmc.py` can
  render blame text without an import cycle. See
  `docs/DESIGN-blame-assignment.md`.
- Persistent verdict cache for `fslc verify` (issue #169, `src/fslc/verify_cache.py`):
  a sha256 key over the post-desugaring kernel AST, raw entry-file text, every
  verify option, and an implementation fingerprint (fslc/z3/lark/Python
  versions + a hash of the installed package's source) makes an unchanged
  re-run in the write→verify→repair loop return instantly instead of
  re-solving. A `violated` result is additionally reused at any deeper
  requested depth (a counterexample's earliest step does not depend on the
  search bound). Hits add one additive JSON field
  (`"cache":{"hit","key","source"}`); misses are byte-identical to today's
  output, and any cache-layer failure degrades to an ordinary uncached run —
  never affects the verdict. New `--no-cache` flag / `FSLC_CACHE=off` env var
  to opt out; `FSLC_CACHE_VERIFY=1` re-runs the engine on every hit and
  reports a divergence as an internal error. Stage 1 of the design
  (whole-verdict cache + cross-depth reuse) only — property-level
  differential re-verification is explicitly deferred. See
  `docs/DESIGN-incremental-verify.md`.
- Dialect corpus conformance harness (issue #167): a declarative
  `tests/dialect_registry.py` (dialect construct → example corpus, plus
  documented exclusions for fsl-ai project/agent files and one Monitor
  edge-case fixture) backs a new `tests/test_dialect_conformance.py` CI gate
  that runs `parse -> desugar -> build_spec -> Monitor load -> BMC/Monitor
  expression agreement -> verify-vs-oracle verdict agreement` over every
  `.fsl` under `specs/`/`examples/`, closing the gap the 2026-07-08 fsl-db
  audit found (an entire dialect corpus silently sat outside the dual-
  evaluator safety net while `pytest -q` stayed green). No `pytest.skip`
  anywhere — every non-conformance file is a classified, asserted case.
  Along the way, hardened the shared expression-agreement check (now in
  `tests/agreement.py`, reused by `tests/test_evaluator_agreement.py`): array
  state is pinned as a fully-determined term instead of per-key equalities,
  and agreement is proved by solver unsat-check rather than `Model.eval`,
  fixing spurious disagreements on `Set<domain>` bound checks (a real z3
  quantifier-evaluation gap the broadened corpus scan surfaced, not a
  bmc.py/runtime.py semantics bug — see `docs/DESIGN-conformance-harness.md`).
- Coupled-change metatests (issue #168): `tests/test_coupled_change_meta.py`
  mechanizes the "grammar/dialect/CLI command moves its LSP index entry and
  DESIGN doc together" discipline that was previously only a human checklist.
  A corpus-wide, two-stage check (structural scan + position cross-check)
  verifies every grammar production's NAME/REQ_ID tokens reach
  `src/fslc/lsp/index.py`'s symbols/references, with a reviewed, staleness-
  checked allowlist for genuine free-form labels; a second check verifies
  every kernel dialect and CLI command maps to an existing
  `docs/DESIGN-*.md`, and that `docs/README.md`'s DESIGN-doc map is
  bidirectional. Prototyping this metatest re-found the same class of bug
  `d1770c4` fixed, twice more, fixed in this PR: fsl-domain sources crashed
  the LSP entirely (`_parser_for_source` never dispatched to the domain
  grammar), fsl-ai project files (multi-block bundles that happen to start
  with `ai_component`) crashed it too (now indexed via
  `ai_project._top_blocks` directly, matching how that dialect is actually
  parsed), and several fsl-db/fsl-ai productions
  (`env_flag`/`database_def`/`delegation_edge`/`agent_event`/
  `agent_output_def`) were silently unindexed -- including a pre-existing
  bug in `_visit_env_artifact` that dropped every `when flag F=V` condition
  on a database artifact. See `docs/DESIGN-coupled-change-metatest.md`.
- Rebuilt the `docs/intro/` manual site's information architecture around 4
  fixed categories (Get Started / Guides / Reference / Examples & Background),
  designed with the Relational Design plugin (decisions in
  `docs/DESIGN-docs-site.md`). Added a categorized sidebar with
  real `<h2>`/`aria-labelledby` groups, a breadcrumb, 4 category hub pages, and
  7 new chapters: `quickstart` (a 5-minute install-to-proof walkthrough),
  `examples` (a guide to `examples/`/`specs/`), `errors` (the JSON envelope,
  exit-code contract, and a failure-mode index), `glossary`, and `design-notes`
  (an index of authoritative `DESIGN-*.md` documents). Added `tools/build_site_reference.py`,
  which generates `intro/language.*.html` and `intro/cli.*.html` — exhaustive,
  disclosure-tree references reproduced verbatim from `docs/LANGUAGE.md` and
  introspected from `src/fslc/cli.py` — plus `tests/test_site_reference_snapshot.py`
  to catch a forgotten regeneration. `syntax.*.html` is now a 30-minute reading
  primer that points to the generated language reference rather than a second
  hand-maintained grammar table. Fixed `docs/LANGUAGE.md` §7 missing the
  `fslc ledger` command and `docs/README.md` missing 5 `DESIGN-*.md` entries.
- fsl-ai stochastic/migration/drift evidence commands: `fslc ai eval` now checks
  precomputed eval JSONL against `statistical_property` Wilson-bound thresholds,
  `fslc ai regress` checks aggregate `ai_migration.no_regression` metric
  drop/increase clauses, `fslc ai compare` reports metric deltas,
  `fslc ai drift` checks runtime telemetry thresholds/drift, and
  `fslc ai compat` emits shared DB artifact capability profiles. All return
  external evidence with `formal_result:"not_run"` rather than kernel proof.
- fsl-domain / fsl-effect v0 for Functional DDD and async lifecycle modeling:
  `domain` files now parse aggregate state, command/event/error, pure
  `decide`/`evolve`, projections, saga/process-manager steps, async effects
  with correlation/idempotency/retry/timeout/compensation/outbox boundaries, and
  lower the checkable slice to the existing kernel. Added
  `fslc domain check|analyze|expand|generate|testgen|replay`, TypeScript
  Functional DDD scaffolds with process-manager output, simple
  Python/Kotlin/Swift/Rust scaffolds, domain adapter/conformance scaffolds,
  stable `fsl-domain-finding.v0` findings, examples under `examples/domain/`,
  and docs/skills/manual updates. Runtime replay is observation evidence; the
  v0 boundary keeps external API/queue/wall-clock and production exactly-once
  guarantees outside formal proof.
- Documented the agent-side natural-language review boundary for
  `fslc analyze`: `fslc` remains deterministic and language-agnostic, while AI
  agents may add non-authoritative review suggestions only when they cite source
  text and graph nodes, keep `formal_status:"not_a_violation"`, and respect
  explicit privacy opt-in for external model calls. (#117)
- fsl-ai recursive `agent` composition: `fslc ai check` now parses nested
  agents as ordinary scoped agents, emits deterministic AI-readable
  `agent_ir`/graph summaries, validates explicit authority/context grant
  boundaries, and reports structural findings for unsafe delegation,
  visibility, review-gate, and tool-reachability patterns. Existing
  `ai_component` hard-contract behavior remains backward-compatible. (#157)
- fsl-stochastic external evidence layer and AI compatibility integration:
  `docs/DESIGN-stochastic.md` fixes statistical evidence semantics as an
  external layer over precomputed eval JSONL, Bernoulli/proportion metrics, and
  Wilson intervals only (`formal_result:"not_run"`; no `fslc verify` semantics
  change). `dbsystem artifact` now supports generic `requires` / `provides`
  capability profiles so AI model/prompt/retriever/tool-schema/output-schema
  compatibility is checked in the same environment/schema/flag snapshots as
  DB/API/mobile/server artifacts, with `required_capability_missing` findings
  and `DB-ASSUME-AI-CAPABILITY-PROFILES`. (#139, #140)
- (Documentation) Team-facing Japanese practical guide for `fslc analyze`
  (`docs/GUIDE-analyze.ja.md`): a plain-language walkthrough of the structure
  map, graph projections, `--profile ai-review` findings (with real command
  outputs), DOT/Mermaid exports, batch mode, project traceability, and the
  "not a verifier" boundary, plus review/PR/CI workflow integration. Linked from
  the doc map in `docs/README.md`. Includes a prior-art positioning section
  (Petri net structural analysis, SPARK/GNATprove flow analysis, Frama-C
  PDG/slicing, Event-B/Rodin, proof-assistant dependency graphs, mCRL2 static
  LPS analysis) stating plainly which parts of `analyze` are borrowed ideas and
  which parts of the combination are uncommon.
- fsl-ai hard-contract dialect (`ai_component`) for AI tool-boundary
  safety. The dialect parses declared tools, authority, human approval,
  forbidden tools, symbolic business preconditions, and fallback metadata,
  lowers the deterministic hard-contract slice to the existing kernel, and adds
  `fslc ai check` / `fslc ai replay` with stable AI-readable findings
  (`fsl-ai-finding.v0`). Runtime replay is explicitly observation evidence
  (`formal_result:"not_run"`), while evaluator-backed and statistical AI claims
  remain out of the kernel. Includes fixtures in `examples/ai/`, schemas under
  `schemas/fslc/ai/`, docs in `docs/DESIGN-ai-hard.md`, bilingual site pages,
  and updated language/skill references. (#135, #136, #137, #138)
- fsl-db MVP (`dbsystem`) for database migration compatibility across schema
  versions, artifacts, and environments. The dialect parses typed DB IR, expands
  to the existing kernel via `Map<Column, Bool>` lifecycle state and generated
  migration actions/invariants, and adds `fslc db check` for stable
  AI-readable findings (`fsl-db-finding.v0`) such as
  `column_removed_while_still_read`, `column_removed_while_still_written`, and
  `not_null_before_backfill`. Includes golden fixtures in `examples/db/`,
  docs in `docs/DESIGN-db.md`, and updated language/skill references. (#122,
  #123, #124, #125, #126, #127, #128)
- fsl-db post-MVP compatibility extensions: destructive/irreversible migration
  annotations, bounded data preservation and rollback-equivalence findings,
  rename/split/merge transforms, API response and offline payload compatibility,
  runtime observation evidence via `fslc db observe`, and a minimal SQL DDL
  importer via `fslc db import`. The new checks stay under explicit assumptions
  for finite rollout windows, offline TTL ticks, bounded row models, and
  observability coverage. (#129, #130, #131, #132, #133, #134)
- fsl-db DB/multi-environment follow-ups: finite feature-flag variants in
  `environment` blocks with `when flag name=value` artifact windows and
  `DB-ASSUME-FINITE-FLAG-STATE`, a minimal Prisma schema importer
  (`prisma-schema-minimal.v0`) alongside SQL DDL import, and external
  preservation/engine evidence JSON schemas plus fixtures that keep sampled,
  audited, and dry-run evidence separate from formal proof
  (`formal_result:"not_run"`). (#144, #145, #146, #147)
- Issue #104 follow-ups: typed `relation A -> B` state with relation helpers
  (`.contains/.add/.remove`, `reachable`, `acyclic`, `functional`,
  `injective`, `domain`, `range`); `helpful action(args)` metadata for
  per-binding ranked `leadsTo` induction under interleaving; opt-in
  `fslc sweep` with `sweep.results` and `sweep.minimal_counterexample`; richer
  `preserve progress` diagnostics (`progress_failure` classification and
  lower-layer `fair action` repair hints); and `fslc html` relation/refinement
  visual evidence.
- `fslc analyze` structural observation JSON. `--projection tsg` emits a stable
  Typed Semantic Graph over requirements, actions, state variables, properties,
  acceptance/forbidden scenarios, and traceability metadata. Graph projections
  (`action_state_graph`, `requirement_property_graph`, `property_state_graph`)
  add deterministic components, SCCs, representative cycles, and degree
  summaries. `--profile ai-review` emits AI-readable review findings
  (`disconnected_requirement`, `unanchored_property`, `progressless_cycle`) with
  `formal_status:"not_a_violation"` so structural observations are not confused
  with proof failures. See `docs/DESIGN-analysis.md`.
- `fslc analyze` now supports batch file/directory analysis, DOT/Mermaid graph
  exports, standalone `refinement_graph`, project-manifest
  `traceability_graph`, versioned JSON Schema files under
  `schemas/fslc/analysis/`, two additional AI-review findings
  (`unwritten_state`, `unguarded_action`), and opt-in LSP informational
  diagnostics for analysis findings via `FSLC_LSP_ANALYSIS_DIAGNOSTICS=1`.
- `fslc analyze` structural follow-ups for AI-assisted review: graph projections
  now include additive `metrics` (cycle rank, fan-in/fan-out hubs, and graph
  counts), `action_dependency_graph` exposes structural `enables` and
  write/write `conflicts_with` action relations, and `impact_graph --focus
  NODE` emits an upstream/downstream slice around a TSG node. The `ai-review`
  profile adds `unread_state` using a transitive relevance closure over effect
  reads, and `conservation_candidate` proposes weighted-sum invariants from
  restricted counter-like `Int` effects without treating them as proof evidence.
  (#149, #150, #151, #152, #153)
- Optional **spec-level tag** classifying a whole spec: an intent string right
  after the spec name, `spec ReturnUI "ui: screen flow" { … }`. Metadata only —
  it desugars to nothing and is never verified (corpus snapshot unchanged) — and
  is surfaced by `fslc explain` (`skeleton.spec_kind = {id, text}` + a `Kind:` line
  in `--readable`) and by `fslc html` (a neutral badge next to the spec title).
  It carries the machine-readable and at-a-glance "this is a UI spec" classification
  the fsl-ui spike (issue #9) identified, without the `expand_ui` dialect; see
  `docs/DESIGN-ui.md`. Touches `grammar.py`, `model.py`, `explain.py`,
  `html_report.py`. (The surfaced field is `spec_kind`, not `kind`, because the
  enveloped result tree reserves `kind` for the diagnostic discriminator.)

### Fixed
- Rust BMC witness replay now starts from the solver-projected step-zero state,
  so partially initialized aggregate components remain legitimately free and
  invariant counterexamples return `violated` instead of an internal Monitor
  init mismatch. Explicit `--from-state` snapshots remain authoritative, and
  top-level `kind: "internal"` envelopes now consistently exit 3 per the CLI
  contract. The frozen Python reference is unchanged. (#219)
- **CI**: `tests/test_site_reference_snapshot.py` (added with the doc-site
  rebuild) was missing from every `.github/ci-shards/shard-*.txt`, so
  `pr-shard-coverage` was failing on any PR touching the test suite. Added it
  to `shard-2.txt`.
- **Soundness**: `fslc verify --engine induction` could report a `leadsTo
  ... decreases ... helpful` property `"proved"` when it was genuinely false.
  With two or more distinct `helpful` action declarations,
  `_prove_leadsto_rank_no_deadlock` only checked that *some* helpful match is
  enabled at every pending state (a disjunction); if which instance is
  enabled alternates (e.g. two helpful actions gated on opposite parity of
  some other variable), no single instance is ever *continuously* enabled,
  so its `fair` declaration never actually obligates it to run under weak
  fairness, and an unrelated action can stall the obligation forever. Added
  `_prove_leadsto_rank_helpful_sticky` (`src/fslc/bmc.py`), a new induction
  proof obligation requiring each helpful instance to stay enabled (or `Q`
  resolve) until it fires, reported as `rank_failure:
  "helpful_action_enabledness_not_sticky"` when it can't be shown. Wired into
  both the explicit-`decreases` path and the auto-synthesized-measure path
  in `_prove_ranked_leadstos` (the latter was missed in an earlier pass of
  this fix and reached the same false "proved, synthesized: true" outcome
  through a separate candidate loop). The common single-helpful-action idiom
  is unaffected (`tests/test_helpful_leadsto.py`, `docs/LANGUAGE.md`,
  `skills/fsl/reference.md`).
- **Soundness**: a second, independent false-`"proved"` gap in the same
  `helpful`/`decreases` ranking rule, reproducible even with a single
  `helpful` action — `_prove_leadsto_rank_progress` only required a
  non-helpful action to keep the leadsTo obligation pending
  (`Or(q_next, p_next)`), not to avoid *increasing* the measure. A fair,
  always-enabled helpful action decreasing the measure by a bounded amount
  each time it fires does not guarantee eventual convergence if an unrelated
  action can increase the measure by more than that in between — the
  helpful action still fires under weak fairness, but the measure can
  diverge instead of reaching zero. `pending_preserved` now additionally
  requires `measure_next <= measure` for the non-helpful branch, reported as
  `rank_failure: "non_helpful_action_increases_measure"` when it can't be
  shown (`src/fslc/bmc.py`, `tests/test_helpful_leadsto.py`,
  `docs/LANGUAGE.md`, `skills/fsl/reference.md`).
- `runtime.py`'s concrete `Monitor` rejected every fsl-db-generated spec
  (15/18 `examples/db/*.fsl`) with a spurious `state variable 'column_exists'
  assigned more than once in init`, because the duplicate-init-assignment
  check keyed on the base Map variable alone; `db_expand.py` legitimately
  writes one flat `column_exists[Col] = ...` statement per column, each to a
  different key of the same map. This silently disabled the dual-evaluator
  agreement safety net (oracle/agreement/trace-soundness tests, `replay`,
  `testgen`) for the whole dbsystem dialect, since BMC accepted these specs
  fine. `_check_deterministic_init` now disambiguates a Map/index target by
  `(base, key)` when the key is a concrete (non-forall-bound) value, so
  distinct keys of the same map no longer collide; a genuine same-key
  duplicate is still rejected, and a `forall`-keyed init is unaffected.
- The FSL language server's raw-tree index (`src/fslc/lsp/index.py`) no
  longer hard-codes the kernel-only grammar: `build_index` now picks
  `ai_parser.AI_PARSER`/`db_parser.DB_PARSER` for `ai_component`/`dbsystem`
  sources (same dialect-sniffing already used by `parser.parse_src`), so
  go-to-definition/references/hover/semantic-tokens no longer go dark with an
  uncaught `UnexpectedCharacters` for those files (`fslc check` diagnostics
  were unaffected, since they go through a different path). Added indexing
  for `tool`/`table`/`column`/`artifact`/`migration`/`environment`
  declarations and their references (`authority` tool lists, `col_ref`,
  `env_artifact`). Also fixed two kernel-grammar indexing gaps found in the
  same audit: a `leadsTo ... helpful NAME(...)` action name was a bare Token
  the generic child walk skipped, so it never resolved or showed up in
  find-references; and `deadline NAME <= expr` registered `NAME` as a new
  `property` symbol instead of a reference to the already-declared
  `age NAME[...]` variable it targets. Added `helpful`/`relation`/`acyclic`/
  `functional`/`injective`/`domain`/`range` to the completion keyword list.
- `ai_component` rejects two `fallback` entries that share the same `reason`
  (they would silently collide into one generated `fallback_<reason>`
  action). `docs/DESIGN-ai-hard.md` now also documents explicitly that
  `fallback` is structural-only in the hard-contract layer — no invariant is generated over
  `fallback_required`, since `target` has no corresponding kernel action to
  check it against yet (`src/fslc/ai_expand.py` `validate_ai_component`).
- `fslc refine` no longer silently merges a same-named impl/abs enum (or
  struct) that has a different member list (or field set): it now rejects
  the pair as a `kind: "type"` static error instead of letting an impl-only
  member get reinterpreted as whichever abs member sits at the same ordinal
  index, which could previously turn a real refinement violation into a
  false `"refines"`. Domain types (`lo..hi`) still may share a name with
  different bounds (`src/fslc/refine.py` `_merge_types_meta`/
  `_type_defs_conflict`; `docs/DESIGN-refinement.md`, `docs/LANGUAGE.md`,
  `skills/fsl/reference.md`).
- The `acyclic`/`reachable` relation helpers no longer unroll an unmemoized
  O(n^n) Z3 expression tree, which made verification of a self-relation with
  as few as 7 domain values effectively hang; `_relation_reachable_expr`
  (`src/fslc/bmc.py`) now memoizes the bounded-path recursion into an
  O(n^2)-node shared DAG.
- `fslc db observe` and `fslc db import` no longer return exit code 3
  (reserved for internal errors) for their normal result values
  (`observed_conformant`/`observed_mismatch`/`imported`/
  `imported_with_warnings`); `exit_code()` in `src/fslc/cli.py` now maps them
  to the documented 0/1 contract.
- Restored `docs/index.html` to a language selector only and added bilingual
  `docs/intro/db.{ja,en}.html` content pages for the fsl-db DB /
  multi-environment compatibility manual entry.
- `fslc html`/`fslc explain` now render a declaration's full logic instead of a
  truncated first source line: action `requires`/`ensures` and property bodies
  (`invariant`/`reachable`/`trans`/`leadsTo`) are rendered from the AST via the
  existing `_expr_to_text` pretty-printer, so multi-line predicates (e.g. a
  `reachable` block with a `forall` body) show completely, and a
  component-origin/composed action gets a real guard string instead of the old
  "source unavailable" sentinel (`src/fslc/explain.py`). Dialect-generated
  declarations (a `deadline`-derived invariant, the time-block-generated
  `tick` action) are now tagged `"generated"` in the explain skeleton
  (additive field) and routed out of the human Actions/Properties tables into
  the auto-checks table in the HTML report, with a neutral "generated by
  verifier" label replacing the old sentinel; the report also drops a
  `_Age*`-style generated domain type out of the entity/domain listing.
  `fslc html` further surfaces business-layer content that reached the
  skeleton but never the page: enum member lists in the State Model type
  column, an Entities & Domains panel, a KPI table, and a derived
  stage-transition ("process flow") panel reconstructed from enum-valued
  state plus guard/assignment shape — dialect-agnostic, so any hand-written
  enum state machine gets one too. `ID: sentence` requirement/policy meta
  moves from a trailing "Requirement" column (which showed literal "none"
  cells) to an inline caption under each row's own name, dropped entirely
  when absent; the same treatment now applies to a new optional "Actor"
  column. Fixed `_type_text` rendering `Bool`/`Int` state as a raw Python
  tuple. Sections are reordered model-first (State Model, Actions,
  Properties, *then* Verification Status), and the hero subtitle is now a
  one-line summary derived from the spec's own counts instead of a fixed
  string. Two more "none" filler spots dropped with the same principle: the
  property table's "Deadline" column now only appears when at least one
  property has a `leadsTo ... within`, and an action's Ensures cell is left
  empty rather than a "none" chip when the action has no `ensures` clause.
  Fixed a correctness bug in `_expr_to_text` (`src/fslc/explain.py`) that the
  above rewiring exposed: it never parenthesized a child operand, so e.g.
  `not (pending[r] and served[r])` rendered as `not pending[r] and
  served[r]` — which re-parses as `(not pending[r]) and served[r]`, the
  inverted formula (seen live in `sla_worker_design`'s
  `PendingServedExclusive`). `_expr_to_text` now tracks each operator's
  precedence tier (mirroring the grammar's `implies < or < and < not < is <
  cmp < sum < product < unary` chain) and parenthesizes a child exactly when
  its own precedence is looser than what its position requires — covering
  `not (A and B)`, `(A or B) and C`, and non-associative arithmetic like
  `a - (b - c)`/`a / (b / c)`, while still rendering `A and B or C` without
  spurious parens. This is the function `explain --readable` also uses, so
  the fix applies to both surfaces. Also suppressed the redundant
  `<actionname>: by <actor>` requirement caption on actions now that a
  business-dialect transition's actor has its own Actor column (real
  `REQ-n`/`POL-n` prose captions are unaffected).
- (LSP) `textDocument/references` with `includeDeclaration=true` now returns the
  declaration even when the cursor is on a *cross-file* reference (e.g. a
  `use`-imported `alias.member` in another spec). Previously the declaration was
  dropped in that case: `references_at()` only emits the declaration when it lives
  in the current document, and the workspace loop in `_workspace_references`
  (`src/fslc/lsp/server.py`) scans other files' references, never their
  declaration symbol, so a symbol declared in a different workspace file was
  omitted from the results. `go-to-definition` was unaffected.

### Changed
- `skills/` reframed around the doc-substitution philosophy: a `.fsl` spec
  replaces the prose spec/design doc, not just proves a property, so the
  self-check in `skills/fsl/SKILL.md` now scores both verification payoff and
  documentation payoff (a linear-path/CRUD feature that would be documented
  anyway is in scope), `--strict-tags` and `explain --readable` moved from
  high-risk-only to defaults for every contract (`skills/fsl-delivery/SKILL.md`,
  `skills/fsl-design-review/SKILL.md`), and `skills/README.md`,
  `skills/fsl/SKILL.md`, `skills/fsl-delivery/SKILL.md` now state the
  corpus-as-living-source-of-truth framing.
- `docs/intro/` (`concept`, `when-to-use`, `guide`, `index`, `design-layer`;
  `.en.html`/`.ja.html`) now state the same spec-as-documentation philosophy:
  `concept`, `index`, and `design-layer` say the `.fsl` source replaces the
  prose spec/design doc rather than supplementing it; `when-to-use` reframes
  from verification-ROI triage to a "write it wherever you'd document
  anyway" default, and narrows "out of scope" to genuine inexpressibility
  (continuous/probabilistic/free-text) rather than low payoff; `guide` keeps
  high-risk-first as adoption sequencing, not the end state; `concept`,
  `guide`, and `when-to-use` add the living-corpus/CI framing — the spec
  corpus stays continuously re-verified, not proved once.
- CI now treats Python as a frozen reference implementation and validates only
  the primary Rust workspace, dependency boundary, native Z3 matrix, official
  `z3-solver` Worker probe, and production WASM browser surface. Remove the
  Python pytest matrices, Python/Rust differential gate, and obsolete shard
  manifests; differential tools remain available for manual migration audits.

## [2.7.0] - 2026-07-04

### Added
- (LSP) `fslc-lsp` now serves four more `textDocument/*` features as thin
  lsprotocol adapters over the existing raw-tree index
  (`src/fslc/lsp/server.py`, `src/fslc/lsp/index.py` unchanged): `hover`,
  `references` (same-file plus cross-file, following `use`-imported aliases
  and workspace spec-name resolution), `completion` (triggered on `.` for
  alias-member completion, alongside local symbols and keywords), and
  `semanticTokens/full` (legend-registered token types/modifiers).

### Fixed
- (LSP) The raw-tree symbol/reference indexer (`src/fslc/lsp/index.py`) now
  covers three parse-tree shapes it previously fell through to generic
  child-recursion for: a `policy ... every <Case> reaching ... must have
  passed through ...` precedence policy's case name (`type` reference), a
  struct literal's field keys (`field` references), and every `field_suffix`
  beyond the first in a multi-level `a.b.c` postfix chain (`field`
  references) — the pre-existing `a.b` alias.member `value` reference is
  unchanged.

## [2.6.3] - 2026-07-03

### Fixed
- `fslc verify --instances`/`--values` overrides (#86) now propagate into an
  inline `implements` abstract spec, restricted to the entity/number names the
  abstract declares. Refinement is a same-world-size forward simulation, so a
  shrunken impl (`--instances Claim=1`) against a still-full-size abstract
  previously failed the refinement with `map_out_of_bounds` (surfaced only in
  the `implements` sub-field, with the overall `result` staying `verified`).
  Both sides now shrink together; an impl-only carried number (e.g. `Amount`,
  absent from a business abstract) applies to the impl only. (#94)

## [2.6.2] - 2026-07-03

### Fixed
- `docs/LANGUAGE.md` and `skills/fsl/reference.md` wrongly claimed there is
  no `sum()` aggregate usable in a `decreases` ranking measure, steering
  readers toward hand-written sums (`decreases level[0] + level[1]`) that
  only scale to domains small enough to enumerate by hand. The kernel's
  bounded `sum(x: T of expr [where expr])` aggregate has always been usable
  there (`decreases sum(k: Case of level[k])`), proves `unbounded`
  completeness under `--engine induction`, and is instances-count
  independent (composes with `--instances` overrides, #86). Corrected both
  docs and added regression coverage in `tests/test_sum_decreases.py`. (#91,
  see also the still-open per-entity fairness gap, #72)

## [2.6.1] - 2026-07-03

### Fixed
- `fslc verify --instances`/`--values` overrides (#86) no longer hard-error
  when an `acceptance`/`forbidden` scenario hardcodes an id/number from the
  spec's original (larger) world. When an override is active, a scenario
  whose replay fails purely because it references a value outside the
  overridden bounds (an out-of-range action argument or an out-of-range
  `expect` index) is downgraded per-scenario to a skip, reported via
  `warnings` (`kind: "acceptance_skipped"`/`"forbidden_skipped"`); other
  scenarios still replay normally. Without an override, or for any other
  failure reason, behavior is unchanged (hard error). This makes
  `--instances Case=1 --property <Liveness>` usable without editing
  acceptance/forbidden scenarios written for the full-size model. (#89)

## [2.6.0] - 2026-07-03

### Added
- `fslc verify --instances NAME=N` / `--values NAME=LO..HI` (both repeatable)
  override a `verify { ... }` block's `entity`/`number` bounds from the CLI,
  so liveness/induction runs can shrink the model without editing the spec.
  An undeclared `NAME` or a malformed value is a spec error (exit 2); the
  effective override is echoed back as `bounds_overrides` in the JSON
  envelope. (#86)

### Fixed
- Business-dialect no-bypass precedence policies (`every <Entity> reaching
  <Targets> must have passed through <Waypoints>`, #75) now auto-synthesize a
  second, stabilizing auxiliary invariant (`<PolicyId>_stability`) alongside
  the history flag: `forall c { stage[c] in dominated(Waypoints) =>
  visited[c] }`, where the dominated set is computed by one reachability
  pass over the process's transition graph with the waypoint nodes removed.
  Previously a compliant precedence policy verified under BMC but stalled at
  `unknown_cti` under `--engine induction` on a ghost CTI (`stage[c] ==
  Waypoint && visited[c] == false`, unreachable in practice but not provable
  by induction alone); it now proves at k=1, including for cyclic process
  graphs and waypoint disjunctions. Docs: `docs/DESIGN-precedence-policy.md`,
  `docs/LANGUAGE.md`, `skills/fsl/reference.md`. (#85)

## [2.5.0] - 2026-07-02

### Added
- Requirements-layer `process E with f: T { ... }` carried fields now accept
  `Bool` and enum types declared in the same spec, not just `number`. Number
  fields keep the original default (the domain's `lo` bound) and gained an
  optional explicit initializer (`f: T = <const-expr>`); `Bool`/enum fields
  have no invented default and **require** an explicit initializer
  (`f: Bool = true/false`, `f: T = Member`) — a missing one is a check-time
  error. Docs: `docs/LANGUAGE.md`, `docs/DESIGN-dialects.md`,
  `skills/fsl/reference.md`. (#70)
- The requirements-dialect inline `implements Abs from "file" { }` block now
  accepts `action <impl>(<params>) -> <abs>(<args>) | stutter` items (grammar.py
  `?implements_item` gains `refinement_action`), the same action-correspondence
  syntax as a separate refinement file, including an arity change between the
  impl and abs action's parameters. The inline desugar already merged the
  block's items into the same mapping AST a separate-file `refinement` parses,
  so `dialects.py`/`refine.py` needed no changes — including duplicate-map
  detection (`kind: "type"`, `"duplicate action map for '<name>'"`) when an
  inline `action ...` item and a requirement action's `maps` clause both target
  the same impl action. (#73; docs: `docs/DESIGN-refinement.md` §1.2,
  `docs/LANGUAGE.md`, `skills/fsl/reference.md`)
- k-induction `unknown_cti` results now suggest auxiliary invariants for the
  common monotone-counter idiom: when the CTI trace shows an `Int`/domain
  scalar or a `Map<K, Int>` counter moving in only one direction and starting
  on the unreachable side of the concrete initial value (a ghost/huge/negative
  start), the result gains `"suggested_invariants": ["<expr>", ...]` and the
  matching sentence is appended to `hint` (e.g. `"audit >= 0"` or, for a
  uniformly-initialized map, `"forall k: Case { audit[k] >= 0 }"`).
  Post-processing only (trace diff against `runtime.Monitor(spec).reset()`) —
  no solver/engine semantics change, so verdicts are unaffected; the field is
  additive and absent when no such counter is found. (`bmc.py`, `docs/LANGUAGE.md`,
  `docs/DESIGN-induction.md`, `skills/fsl/reference.md`, `tests/test_cti_suggestions.py`) (#74)
- Business-layer no-bypass precedence policy: `policy ID "text" every <Entity>
  reaching <Stage> [or <Stage> ...] must have passed through <Stage> [or
  <Stage> ...]`. Synthesizes an invisible `Map<Entity, Bool>` history flag
  (dedup'd across policies over the same process/waypoint-set), sets it on
  the transition(s) landing on a waypoint, and compiles to a kernel invariant
  carrying the policy's REQ-ID — closing "no-bypass" controls at the business
  layer without descending to `requirements`. See
  `docs/DESIGN-precedence-policy.md`. (#75)
- `terminal { }` now works in the `requirements` dialect (`terminal_def` is a
  `requirements_item`; it passes through unchanged to the kernel spec, same
  one-block-per-spec rule as the kernel). The `business` dialect gets no new
  syntax: `terminal { }` is instead derived automatically from each process's
  sink stages (stages with no outgoing `transition`) — if every process has
  at least one sink, the generated predicate is the conjunction, over
  processes, of "every entity of that process is at one of its sinks". A
  process with no sink at all (a genuine cycle) still generates no terminal,
  matching prior behavior exactly. Previously, neither dialect could declare
  intended stop states, so `--deadlock ignore` was the only way to silence
  the deadlock check at a completed stage — discarding detection of
  unintended deadlocks along with it. (#69)
- Allow the builtin `Bool` as an action parameter type (`p: Bool`), matching
  its existing use as a state `Map` value/key. `Bool` params are first-class
  z3/concrete booleans in expressions — usable bare as a guard
  (`requires b` / `requires not b`) or assigned into `Bool`-typed state
  (`flag[i] = b`) — not a 0/1 int carrier, keeping BMC and the concrete
  `Monitor` in agreement. `Int` stays rejected (unbounded, can't be
  enumerated); the error now hints at a range parameter
  (`p in <lo>..<hi>`). (#68)
- `acceptance`/`forbidden` action arguments now accept enum member names (and
  const names) in addition to numeric ordinals, matching the name resolution
  already used by `requires`/`invariant`/`expect` expressions
  (`_is_enum_member` in `values.py`). An undefined name is still a
  `kind: "acceptance"`/`"forbidden"` check-time error, now reported as
  "undefined const or enum member". (#67)

### Documentation
- Documented `leadsTo ... decreases` placement and ranking discipline in
  `docs/LANGUAGE.md` and `skills/fsl/reference.md`: `decreases` sits outside
  the `forall` wrapper (nesting it inside is a parse error, not a "ranking
  doesn't work under forall" limitation); a per-entity measure
  (`decreases level[c]`) always fails under interleaving
  (`rank_failure: "non_decreasing_action"`) because every enabled action must
  strictly decrease it; the working idiom is a global sum measure over a
  fixed small domain (`decreases level[0] + level[1]`), since there is no
  `sum()` aggregate to generalize it (fairness-aware per-entity ranking is
  tracked separately as #72). Also added a targeted parse-error hint in
  `cli.py` for `decreases` nested inside a `forall` body. (#71)
- Updated the workflow skills (`skills/fsl/SKILL.md`, `skills/fsl-business/SKILL.md`,
  `skills/fsl-requirements/SKILL.md`) to track the v2.5 feature set:
  auto-derived business `terminal` and the requirements-layer `terminal`
  passthrough (#69), `Bool`/enum carried fields with initializers (#70),
  `unknown_cti`'s `suggested_invariants` (#74), the no-bypass precedence
  `policy` form (#75), inline `implements { action ... }` correspondence with
  arity changes (#73), `Bool` action params (#68), and enum member names in
  `acceptance`/`forbidden` arguments (#67). `skills/fsl/reference.md` already
  covered these; the workflow skills had not. (#67 #68 #69 #70 #73 #74 #75)

## [2.4.0] - 2026-06-29

### Documentation
- `skills/fsl/SKILL.md` repair protocol: added the over-constraint mirror of the
  hollowing warning — after fixing a `forbidden`/`violated` by tightening a guard,
  re-run `verify` and confirm the action's `action_coverage` is still `true` (and
  affected `reachable`s still witnessed), since an over-tight guard surfaces as a
  *new* `reachable_failed`/`covered:false`. (Surfaced by the #22 repair validation run,
  where fslc-arm agents did exactly this self-check.)
- Documented the cross-layer discrete-time SLA rule: a `deadline` is a safety
  property of the clock that declares it, so a refinement carries it only across a
  *shared* clock (a finer-clock design fails `fslc refine` with
  `abs_requires_failed` — the same non-propagation as liveness). Added a worked
  shared-clock refinement (`examples/nfr/sla_worker_design.fsl` +
  `sla_worker_refines.fsl` → `refines`), corrected the `examples/nfr/README.md`
  framing (the kernel and requirements specs are *different machines*, not two
  encodings of one), and recorded the analysis and the deferred options in
  `docs/DESIGN-nfr.md` §6 / `docs/DESIGN-layers.md` §6 / `docs/LANGUAGE.md` /
  `skills/fsl/reference.md`. No kernel/grammar/refine change. (#56)
- Clarified in `docs/LANGUAGE.md` and `skills/fsl/reference.md`: the `implements { }`
  block takes only state `map` / `maps auto` / `preserve progress` (action↔action
  correspondence goes on the requirement-level action's `maps` clause, not inside
  `implements`), and the `time`-block `tick` is generated (declaring `action tick` is a
  check error), advances age only, auto-maps to `stutter`, and is referenced as
  `tick()`. (#58)
- Reframed the "When to Use FSL" manual chapter (`docs/intro/when-to-use.{ja,en}.html`)
  and the `skills/fsl/SKILL.md` self-check toward FSL's distinctive edge: cross-layer
  alignment is a **primary use**, not an optional "second lens." The page now leads
  with two payoffs — ① a single spec's way of breaking (the classic island check) and
  ② layer-spanning alignment (write the contract layers in FSL and the machine keeps
  catching cross-layer drift) — so it no longer reads as generic formal-methods
  adoption guidance. Brakes kept intact (abstraction tax, hollow-spec / mutate
  kill-rate, judgment-aid-not-mandate, "contract spine, not the entire product
  process").

### Added
- `fslc ledger <spec.fsl> [--impl-log run.json] [-o ledger.md]`: a business
  **audit ledger** (Markdown) that re-organizes `verify` / `scenarios` / `replay`
  findings **by requirement id** so a PM / governance / internal-audit reader can
  decide approve/reject/risk-accept per requirement from the ledger alone. A
  presentation layer over the verifier (no new evaluation): the `trace_type`
  discriminator (#23) drives a per-finding business translation, governance
  columns (risk/decider) come from `control` metadata when present (fill-in
  otherwise), the guarantee limit is stated in positive form, and raw JSON is
  demoted to a collapsed appendix. Docs: `docs/DESIGN-ledger.md`,
  `skills/fsl/reference.md` §7. (#24)
- `fslc` JSON results now carry a `trace_type` repair-routing discriminator on
  every counterexample/failure (`invariant` | `sla` | `type_bound` | `trans` |
  `ensures` | `partial_op` | `deadlock` | `leadsTo` | `leadsTo_rank` | `reachable`
  | `refinement` | `acceptance` | `forbidden` | `vacuity` | `conformance` |
  `induction_cti`), so an agent can route a fix by channel and tell an SLA
  deadline from a structural invariant. Derived in the CLI envelope from existing
  fields (no engine change); passing/spec-error results carry none. `requirement`
  is now also hoisted to the `refinement_failed` root, for parity with verify
  violations. The other repair inputs the issue proposed already exist (`trace`,
  `checked_to_depth`+`completeness`, `hint`/`recommended_action`,
  `unreached[].blocking_requires`) and are documented as the repair-field map in
  `skills/fsl/reference.md` §7. Backward compatible (additive). (#23)
- `init` blocks now support statement-level `if`/`else` in both the symbolic
  verifier and concrete runtime monitor. (#55)
- State declarations may now use inline anonymous range types such as
  `state { x: 0..3 }`, desugared to the existing domain type machinery. (#57)
- `fslc testgen --target phpunit`: a PHPUnit emitter (PHP 8.1+ / PHPUnit 10+,
  `declare(strict_types=1)`), the sixth harness on the pluggable emitter from #43.
  Same `reset`/`step`/`observe` `Adapter` contract and same baked-walk design. Leaves
  compare with `assertSame` (`===`), keeping `int`/`float`, `bool` and `null` from
  coercing (PHP's loose `==` would conflate `0 == "0"`); `_php_literal` renders `int`
  (`1`) distinct from `float` (`1.0`). `assertPartial` recurses by the expected keys
  (maps match order-independently — sidestepping PHP's numeric-string-key coercion —
  and list-shaped values also pin length). `setUp()` calls `markTestSkipped` until
  `makeAdapter()` is wired. Output defaults to `<SpecName>ConformanceTest.php` (PSR-4
  class = file name). Tests gate syntax with `php -l` (skips when php is absent).
  Docs: `docs/LANGUAGE.md` §12, `skills/fsl/reference.md` §9,
  `docs/DESIGN-bridge.md` §3.5. (#47)
- `fslc testgen --target dart`: a `package:test` emitter (also runs under
  `flutter test`), the fifth harness on the pluggable emitter from #43. Same
  `reset`/`step`/`observe` `Adapter` contract and same baked-walk design. Dynamic
  state is `Map<String, dynamic>`; Dart's `==` is reference-based on collections, so
  the bundled `assertPartial` recurses by the expected keys and compares leaves with
  the `equals` matcher (re-exported by `package:test`, keeping the generated file's
  only dependency `package:test`). `package:test`'s `skip:` is static, so a top-level
  probe runs once in `main()` and conditionally skips every `test()` until
  `makeAdapter()` is wired. Strings escape `$` (interpolation). Output defaults to
  `<spec_name>_conformance_test.dart` (snake_case). Docs: `docs/LANGUAGE.md` §12,
  `skills/fsl/reference.md` §9, `docs/DESIGN-bridge.md` §3.4. (#46)
- `fslc testgen --target kotlin`: a kotlin.test emitter (multiplatform; the JVM
  delegates to JUnit), the fourth harness on the pluggable emitter from #43. Same
  `reset`/`step`/`observe` `Adapter` contract and same baked-walk design. Dynamic
  state is `Map<String, Any?>`, where Kotlin's structural `==` is deep on `List`/`Map`
  and distinguishes `Int` from `Double`, so the partial-match helper is a plain
  recursion. The one framework-forced divergence: kotlin.test has no portable runtime
  skip, so `makeAdapter()` returns `null` until wired and each `@Test` returns early
  rather than reporting "skipped". Strings escape `$` (Kotlin string templates).
  Output defaults to `<SpecName>ConformanceTest.kt`. Docs: `docs/LANGUAGE.md` §12,
  `skills/fsl/reference.md` §9, `docs/DESIGN-bridge.md` §3.3. (#45)
- `fslc testgen --target swift`: a Swift Testing emitter (`import Testing` / `@Test` /
  `#expect`; not XCTest), the third harness on the pluggable emitter from #43. Same
  `reset`/`step`/`observe` `Adapter` contract and same baked-walk design as Vitest:
  deterministic and forbidden-rejection scenarios translate directly; the random walk
  is baked at generation time so the file needs no `fslc`/Python at runtime. Dynamic
  state is `[String: Any]` with a bundled `fslEqual`/`assertPartial` deep-equality
  helper (`Int`/`Double` kept distinct); an Option `None` bakes as the self-contained
  `FSLNull.instance` sentinel (no Foundation). Tests are disabled via
  `@Test(.enabled(if: isAdapterWired()))` until `makeAdapter()` is wired. Output
  defaults to `<SpecName>ConformanceTests.swift` (output naming is now a per-target
  registry). Docs: `docs/LANGUAGE.md` §12, `skills/fsl/reference.md` §9,
  `docs/DESIGN-bridge.md` §3.2. (#44)
- `fslc testgen --target {pytest,vitest}` (default `pytest`): the emitter is now
  pluggable. `testgen.py` separates the language-independent scenario-collection
  core (`scenarios()`) from per-target emitters (`emit_pytest`/`emit_vitest`), so a
  new harness is a backend, not a redesign. The first new target is **Vitest**: a
  self-contained TypeScript file with the same `reset`/`step`/`observe` `Adapter`
  contract. Deterministic and forbidden-rejection scenarios translate directly; the
  `random-walk` oracle is **baked at generation time** — the Python `Monitor` runs the
  fixed-seed (`Random(0)`) walk and the `(action, params, expected_state)` trace is
  embedded as a static fixture, so the generated tests require no `fslc`/Python at
  runtime (the single independent oracle stays in Python). pytest output is unchanged
  (byte-for-byte on identical scenario input). Vitest output defaults to
  `<spec>.test.ts`; `--target` and `target` flow through `cli.py`/`run_testgen`.
  Docs: `docs/LANGUAGE.md` §12 and `skills/fsl/reference.md` §9. (#43)
- Kernel `spec` now accepts `entity`/`number` declarations (previously dialect-only),
  desugared to `type X = lo..hi` via the `verify` block (`instances`/`values`). This
  separates domain identity from the verification world size so a design-layer spec
  reads as documentation instead of asserting a domain size that is only a model
  bound. Reuses the requirements desugar path (no new semantics); see
  `docs/DESIGN-spec-domains.md`. A new "Authoring specs as readable documentation"
  section in `skills/fsl/reference.md` records the doc-first conventions, and
  `examples/e2e/3_design.fsl` / `examples/e2e/2_requirements.fsl` are rewritten to it.
- (Skill) `skills/fsl-from-code/` — reverse-engineers an FSL design-layer spec from
  existing source code. Encodes the three-zone extraction (mechanical skeleton vs.
  human-confirmed invariants vs. mechanical truth-check), a formalization-memo
  question set that forces cross-action invariant discovery, and a two-axis
  anti-hollow gate (`fslc mutate` for invariant teeth + the `testgen` harness
  replayed against the real code for fidelity). Anchored downward via conformance,
  not upward via refinement; specgraphen evaluated and not adopted.
- examples/structural: Step 1 demand-validation specs for issue #35 (Alloy-style structural discovery via the populate+reachable idiom)
- (Docs) New manual chapter "When to Use FSL" (`docs/intro/when-to-use.{ja,en}.html`,
  wired into the chapter nav as #2, after Concept): criteria for deciding whether to
  use FSL — the interaction-not-size test, three gates, the high-yield / low-priority
  / out-of-scope split, the spec-not-product preconditions, per-feature vs
  per-project altitudes, and a layer selector. Concept's closing CTA now leads here.
- (Skill) `skills/fsl/SKILL.md` gained a "First, decide whether FSL fits" self-check
  (the one test + three gates, framed as a recommendation rather than a gate) so the
  agent filters out non-FSL-shaped tasks and recommends tests instead of writing a
  hollow spec.
- (Skill) `skills/fsl-requirements/SKILL.md` now defines an explicit faithfulness
  gate (definition of done): a step-1 coverage map from every source requirement to
  an FSL element, mandatory provenance tags (`covers` or `MODEL:`/`ASSUME-n:`) on
  every declaration, and a `fslc check --strict-tags` gate that must report zero
  `untagged` and zero `unreferenced_requirement`. Closes the gap where specs derived
  from a requirements document came out both thin (dropped requirements) and
  over-reaching (invented rules).

### Changed
- (Skill) Reframed the FSL skill set from "single-spec verification" toward
  "verifying cross-layer connection" (issue #48) — framing/routing only, no kernel
  or verifier change. `skills/fsl/SKILL.md` gains a one-paragraph connectivity lead,
  a connectivity-value lens in the fit self-check (a recommended lens with an
  abstraction-tax warning, explicitly *not* a fourth gate), a "Connected workflow
  (across layers)" juxtaposed with the single-spec workflow with `implements` /
  `refine` / `preserve progress` / `chain` as first-class operations, and a
  heading-level "Two soundness facts about connection" (safety descends but liveness
  does not; a chain is only as strong as its refinement soundness). Role boundaries
  are now stated as refinement seams (contracts), not plain baton passes, and the six
  role skills (`fsl-business` / `fsl-requirements` / `fsl-design` /
  `fsl-design-review` / `fsl-delivery` / `fsl-from-code`) are audited to express the
  handoff consistently. `skills/README.md` promotes cross-layer alignment
  (`fsl-delivery`) to a primary use, and the same connectivity-value criterion is
  synced into the manual's "When to Use FSL" chapter
  (`docs/intro/when-to-use.{ja,en}.html`).
- Removed the maintainer contact email from `SECURITY.md` and `pyproject.toml`;
  vulnerability reports are now routed solely through GitHub Security Advisories
  (Private Vulnerability Reporting).

## [2.3.0] - 2026-06-22

### Added
- (LSP) Added a Python `fslc-lsp` language server with diagnostics, document
  symbols, and go-to-definition over raw Lark parse trees, including compose
  `use ... from` cross-file resolution. Added a VSCode extension scaffold with
  `.fsl` language registration, TextMate highlighting, and an LSP client.
- (Install) `install.sh` now installs the LSP server (the `[lsp]` extra) and
  links `fslc-lsp` into `~/.local/bin` alongside `fslc`, so the VSCode extension
  finds it on `PATH`.
- (VSCode) The extension is now packageable as a `.vsix` (`vsce package`): added
  `.vscodeignore`, a `vscode:prepublish` compile step, and a `repository` field.
  The `release-binaries` workflow builds the `.vsix` and attaches it to the
  GitHub Release on a `v*` tag.

### Fixed
- (LSP) Refinement mapping files are parsed without the standalone-spec state
  block diagnostic, and their `impl`/`abs`, mapped action, and state map
  references resolve across workspace `.fsl` files by spec name.

## [2.2.0] - 2026-06-21

### Changed
- (Grammar) Eliminated ~50-line duplicate `ref_expr` grammar by collapsing it to
  a thin `?ref_expr: ite | expr` wrapper; `if/then/else` remains accepted only in
  the four mapping/acceptance positions (map_def, mapped_action_target,
  req_mapped_action_target, acceptance_arg) and is rejected in ordinary spec
  expressions.
- (Grammar) Removed the dead `glue_action` rule that was never referenced.
- (Grammar/model) Extracted module-level `_args()` helper in `grammar.py` to
  remove three identical `maybe_placeholders` sentinel-filtering list comprehensions
  in the `Ast` transformer.
- (Model) Added `_check_reserved` to `model.py` to reject user-defined state
  variables, consts, and action parameters whose names collide with FSL bare-atom
  literals (`true`, `false`, `none`), giving a clear `kind=name` error instead
  of silent parse ambiguity. Keywords that require an immediately following `(`
  (`count`, `sum`, `stage`, `min`, `max`, `abs`, `old`, `unique`, `exactlyOne`,
  `some`) or a binder (`forall`, `exists`) are contextual and parse unambiguously
  as bare identifiers, so they are not reserved.

### Added
- (Governance/Business) Business specs can declare `control` metadata and attach
  it to checkable `policy`/`goal` declarations with `satisfies`. Violations now
  carry satisfied controls in the requirement payload. New top-level
  `governance` catalogs validate cross-business control delegation and run
  preservation refinements during `fslc check`.
- (HTML report) `fslc html <file.fsl> [-o report.html]` generates a self-contained
  review report from `explain` + `verify`: status summary, state/action tables,
  an action-to-state SVG graph (arrows show the write direction), traces, witnesses,
  counterfactual evidence, escaped source, and raw JSON payloads. Counterexamples
  surface a verdict banner and highlight the violating trace step, with violation
  facts humanized in the status table; reachable runs are labelled as such. Without
  `-o`, the HTML is written to stdout.
- (Refinement) Mapping files can opt into liveness-preserving refinement with
  `preserve progress { respond <AbsLeadsTo> by <impl actions...> }`. The check
  pulls the named abstract `leadsTo` through the state mapping and reports
  `refinement_failed / progress_lost` when the lower layer can stall or spin
  while the abstract response remains pending.

## [2.1.0] - 2026-06-18

### Added
- (Induction) Automatic synthesis of `leadsTo` ranking measures (follow-up to
  #11). When a `leadsTo` has no author-supplied `decreases`, `fslc verify
  --engine induction` tries a small set of candidate measures (stage-rank sum
  derived from the action graph, integer distance-to-target, pending count) and,
  if one satisfies the existing ranking obligations, proves the response
  unboundedly and reports `"proof": "ranking", "synthesized": true`. Sound by
  construction — candidates are validated by the unchanged proof obligations, so
  a wrong candidate can only fail, never yield a false proof; if none works the
  leadsTo keeps its bounded result. Branching multi-action flows with no global
  ranking measure fall back to bounded checking as before.

## [2.0.0] - 2026-06-18

Theme: **human-readable business/requirements dialects** (issue #21) — verification
bounds, KPIs, and refinement mappings stop masquerading as model facts, and the
requirements layer gains a readable process+data profile close to the business surface.

### Added
- (Dialects) `entity <Name>` / `number <Name>` type-kinds and a top-level
  `verify { instances E = N ; values X = lo..hi }` block that holds the bounded-model
  sizes in one honest place. Kernel `type X = lo..hi` is unchanged.
- (Requirements) A `process <Entity> with <field>: <T> { ... }` profile: business-style
  `stages`/`initial`/`transition ... by`, extended with `with <input>`, `when <guard>`,
  `set <field> = <expr>`, and `covers REQ-n "text"` traceability. It lowers to a kernel
  state machine; an empty `implements X from "..." { }` body auto-generates the identity
  refinement when names match. Acceptance gains a readable `expect <Entity> <id> in <Stage>`.
- (Requirements) `maps auto` is now allowed inside an `implements { }` block.
- (Safety) Auto-mapped process transitions are statically actor-checked against the
  business layer (the verifier cannot — actors are not refinement state), so an actor
  mismatch is a check-time error instead of a green-but-wrong refinement.
- (Explain) `fslc explain --readable` renders a deterministic text view that surfaces
  verification bounds, fairness, KPI projections, branch lowering, and the synthesized
  refinement mapping (auto-mapped entries flagged for actor/intent review).

### Changed
- (Dialects) `kpi NAME = count ENTITY in STAGE` is now a declarative derived projection
  (available in business and requirements) carried as metadata — no ghost counter, no
  per-transition increment, no auto `_kpi_*` consistency invariant.
- The "spec declares no user invariants" warning no longer fires when the spec has
  leadsTo/reachable/trans properties, acceptances, forbidden flows, or an `implements`
  refinement (those are checked too).

### Removed
- **BREAKING:** business `case X = lo..hi` (use `entity X` + `verify { instances X = N }`).
- **BREAKING:** `kpi ... counts ... in ...` (use `kpi ... = count ... in ...`); the
  KPI counter-consistency auto-invariant and the "decrement KPI unsupported" error are
  gone (a projection is exact by construction).

## [1.5.0] - 2026-06-18

Theme: **honest verification bounds, AI-legible diagnostics, and tractable liveness** —
verify/induction results now declare their `completeness`, `checked_to_depth`, and `cost`;
diagnostics gain a `faithfulness_class` routing tag, split-action `display_name`s, sharper
`insufficient_depth`/`over_constrained` reachable classification, and a new `urgency_freeze`
vacuity lane; liveness gains ranking-function (`decreases`) proofs and `symmetric type`/`enum`
reduction; and the workflow adds `fslc chain`, partial `testgen`, `--exclude-property`, and
`maps auto`, plus a compose `fair`-not-inherited warning and friendlier identifier parse errors.

### Added
- (Temporal) FSL now supports `symmetric type` and `symmetric enum` declarations
  for interchangeable entity identities. `leadsTo` lasso and stall checks use a
  canonical representative for per-entity `Map<SymmetricType, ...>` / `Set`
  rows, reducing symmetric liveness search without changing the JSON envelope.
- (Induction) `leadsTo` declarations can now include `decreases <int expr>`.
  Under `fslc verify --engine induction`, fslc proves the response unboundedly
  with a ranking argument: the measure is non-negative while `P` is pending and
  `Q` is false, pending states cannot deadlock, and every enabled action either
  establishes `Q` or keeps `P` pending while strictly decreasing the measure.
- (Refinement) Mapping files now support `maps auto`, which synthesizes identity
  mappings for same-named compatible state variables and same-named compatible
  actions unless an explicit `map` or `action ... ->` entry overrides it.
- (Testgen) `fslc testgen` now generates partial pytest scaffolds when some
  `reachable` targets are not witnessed at the requested depth. Witnessed
  scenarios are still emitted, unwitnessed targets appear in `warnings[]` with a
  depth hint, and `--strict` restores the previous `reachable_failed` abort.
- (Vacuity) `fslc verify --vacuity` now emits `kind:"urgency_freeze"` for the
  requirements time/deadline trap where Z3 proves the generated urgent condition
  holds initially and is preserved by every action, so generated `tick` is dead
  and deadline invariants are vacuous.
- (Verifier CLI) `fslc verify --exclude-property <Name>` is repeatable and
  skips named invariants, `trans`, `leadsTo`, and `reachable` properties in both
  BMC and induction runs. It mirrors the 1.4.0 cross-kind `--property` resolver;
  exclusion wins when both options name the same property.
- (Verifier JSON) `verify` / induction results now expose boundedness metadata:
  `completeness`, `checked_to_depth`, and `cost.elapsed_s`. BMC `verified` is
  explicitly `completeness:"bounded"`, induction `proved` is
  `completeness:"unbounded"`, and bounded `verified` adds a saturation hint when
  normal exploration first witnesses a reachable/vacuity/coverage fact at depth K.
- (Reachability diagnostics) `reachable_failed.unreached[]` now classifies each
  target as `insufficient_depth` or `over_constrained`; over-constrained targets
  include a `blocking_requires` unsat-core-style list naming the blocking type
  bounds/invariants. The same classification is emitted by `fslc scenarios`.
- (Diagnostic routing) Diagnostics can now carry additive `faithfulness_class`
  and `recommended_action` fields for `partial_op_unguarded`,
  `frozen_only_invariant`, `intent_unexercised`, and the reserved
  `liveness_not_refined` route.
- (Diagnostics UX) Branch-split action diagnostics keep the internal action name
  and add `display_name` such as `submit[a <= AUTO_LIMIT]`. Coverage
  `blocking_requires` hints now summarize the blocking factors after a cheap
  core-minimization pass.
- (Chain CLI) `fslc chain [fsl-project.toml]` runs a manifest-defined
  business -> requirements -> design -> implementation pipeline, reusing the
  existing `check`, `verify`, `refine`, and shell implementation commands. It
  writes a consolidated status table to stderr, JSON to stdout, supports
  `--keep-going`, and exits non-zero when any layer fails.

### Changed
- (Parser/UX) Invalid identifier characters such as `foo$bar` now produce a
  focused parse diagnostic that states the identifier rule instead of leaking raw
  Lark terminal expectations.
- (Refinement) Mapping-file action formals now accept `name: Type` annotations,
  validated against the implementation action's declared parameter types.
- (Documentation) Clarified compose synchronized-argument compatibility as structural
  over bounded value ranges rather than nominal type names, including same-range
  and narrower-target repro results. Also documented action-level
  `maps stutter` in requirements/refinement docs and clarified that distinct
  fields of the same `Map<K, Struct>` element may be updated independently in
  one action.

### Fixed
- (Explain JSON) Counterfactual violation diagnostics no longer emit redundant
  raw `internal_invariant` compose names when the public `invariant` field already
  carries the dotted display name.
- (Compose) Non-fair synchronized actions that reference fair component actions
  now emit a `fair_not_inherited` warning instead of silently hiding the dropped
  composite-level liveness assumption.

## [1.4.0] - 2026-06-17

Theme: **probing single properties and friendlier IDs** — `verify --property`
becomes a general property probe across all declaration kinds, requirement-style
IDs accept underscores, and the liveness/safety scaling trade-off is documented.

### Added
- **`verify --property <Name>` now targets any property kind**, not just
  invariants. The name is resolved across `invariant`, `trans`, `leadsTo`, and
  `reachable` declarations and checked in isolation while the full action model
  still steps, so a single property can be probed on its own (e.g. iterating on a
  slow `leadsTo` without gating the safety checks).
- **Underscores are accepted in requirement-style IDs** (`REQ_ID`): `acceptance`,
  `forbidden`, `requirement`, `policy`, and `goal` IDs now allow `AC_DONE` in
  addition to `AC-DONE`, matching the underscore already permitted in
  action/invariant/trans names. Purely widens the accepted set — existing
  hyphenated IDs are unchanged.

### Changed
- **`--property` not-found diagnostics** now read `no such property: X
  (available: …)` and list every property kind. Under `--engine induction`
  (k-induction proves safety invariants only), naming a `trans`/`leadsTo`/
  `reachable` now reports that the induction engine cannot prove it and to use the
  default `bmc` engine, instead of a misleading "no such invariant".
- **Documented the liveness/safety scaling difference** (`skills/fsl/reference.md`
  §7): `leadsTo` cost grows roughly exponentially in the number of concurrent
  entities (the textbook BMC-liveness state explosion), while safety stays cheap.
  Added the practical strategy — verify liveness on a reduced model and safety
  separately at full size, and use `--property` to isolate one liveness property
  while iterating.

## [1.3.1] - 2026-06-17

Theme: **FSL delivery orchestration skill** — making the business → requirements →
design → implementation-conformance workflow directly invokable as a lifecycle
skill, while also adding readable business-stage syntax.

### Added
- **`fsl-delivery` Agent Skill**: a lifecycle coordinator that routes multi-layer
  work across `fsl-business`, `fsl-requirements`, `fsl-design`, and
  `fsl-design-review`, keeps layer boundaries explicit, and reports business,
  requirements, design/refinement, and implementation-conformance proof states
  separately. The install script and skill documentation now include it.
- **Readable fsl-biz stage syntax for PM/consulting-facing policies and goals**:
  `policy ... every Case in Stage must eventually be Target [or Target ...]`,
  `goal ... some Case can reach Stage`, and
  `goal ... all Case can be Stage [or Stage ...]`. These are AST sugar for the
  existing `responds` / `reachable` forms, so kernel semantics and JSON output
  remain unchanged while common business-flow rules no longer require reading
  `stage(c) == ... ~> ...` formulas.

## [1.3.0] - 2026-06-16

Theme: **propagation review for layer chains (fsl-design-review)** — establishing that
refinement propagates safety but not liveness, and adding end-to-end chain checking.
Also unifies the two FSL expression evaluators behind a shared, domain-parameterized core.

### Changed
- **Unified the symbolic (`bmc.py`, Z3) and concrete (`runtime.py`, Monitor) FSL evaluators**
  behind a single shared core (`src/fslc/values.py`) parameterized by a per-evaluator domain
  object (`_SymDomain` / `_ConcDomain`). The two evaluators previously re-implemented the same
  expression semantics, a drift hazard where the verifier and the replay Monitor could disagree.
  Unified: count, sum, quant, the Option/Seq/struct comparisons, `is`-patterns, field/index access,
  and map access. Behavior-preserving — the verdict-level output is byte-identical across the whole
  spec corpus, guarded by two new safety-net tests (`tests/test_corpus_snapshot.py`,
  `tests/test_evaluator_agreement.py`). Genuinely divergent pieces (Seq/Set method evaluators,
  `compute_updates`, `_eval_requires`, display) are intentionally left per-evaluator. Internal
  refactor only — no change to the CLI, JSON output, exit codes, or grammar.
- Split the over-long `cli.main`, `dialects.expand_business`, and `compose.expand_compose` into
  named private stages (no behavior change).

### Added
- **`fslc refine` chain mode (mapping composition)**: when you line up successive `(spec mapping)`,
  it composes adjacent mappings (states α_AC = α_BC ∘ α_AB, actions a→b→c / stutter) and
  checks **bottom ⊒ top directly**. On success it returns the composed `action_map` and `chain`; on failure it returns
  the first broken link, `failed_link`. Because bounded refinement is transitive at the same depth,
  the composition check is equivalent to all adjacent links holding (`DESIGN-refinement` §7, example `examples/refinement_chain`).
  State mappings are composed at the Z3 level, and indexed maps, Option, and structs are handled by the existing eval.
- Examples `examples/refinement_liveness` (safety propagates, liveness does not, resolved with fair) and
  `examples/refinement_chain` (chain checking), each with its own checking test.
- **A set of self-specs** in `examples/self/`: three specs that model fslc's own design contracts
  in FSL (`fslc_session` = CLI result classification and exit-code severity,
  `fslc_monitor` = stickiness of replay-runtime rejection, `refinement_algebra` = safety
  propagates, liveness does not). All are proved. Pinned-result test `tests/test_self_examples.py`.
- **`terminal { <predicate> }` block**: declares a halting state satisfying the predicate
  as an "intended terminal" and excludes it from deadlock checking. Whereas `--deadlock ignore`
  uniformly ignores all halting states, this lets you single out only the intended halts, while unexpected deadlocks are
  still detected. Used by `examples/self/fslc_session` and `fslc_monitor` (LANGUAGE §1/§6).
- **`fslc verify --property <Name>`**: checks just a single invariant.
  This makes it easier to confirm a violation of a targeted invariant with a non-vacuous probe (a nonexistent name is a usage error = exit 2).
- **Vacuity detection of dead-ghost tautologies**: `--vacuity`
  now statically detects with Z3 an "invariant that, when a frozen state variable assigned by no action is pinned to its init value, becomes
  always true regardless of the values of dynamic variables" (kind `tautology_over_frozen`). It warns at verification time about
  hollow (always-true) invariants that previously both verify and vacuity missed, surfacing only via mutate's survival rate.
  Invariants that do not reference a frozen variable / do not reference state are out of scope. Confirmed zero false positives across the existing corpus.
- **Transition invariant `trans { }`**: `trans Name { old(x) => ... }`
  lets you directly declare cross-action two-state safety. BMC checks each reachable transition, induction checks it in the step case,
  successful output includes `transitions_checked`, and a violation returns `violation_kind:"trans"`.

### Fixed
- **Test suite runs without a `.venv` (CI portability)**: the subprocess-based tests invoked the
  CLI through a hardcoded `ROOT/.venv/bin/python` and a macOS-only `/private/tmp` scratch path,
  which failed on the CI runners. Now use `sys.executable` and `tempfile.gettempdir()`.
- **Include the state in the deadlock warning**: the `--deadlock warn` warning
  message now shows which state it halted in (e.g. `deadlock reachable at step 1
  (state: status=ToolFault, ...)`). The state was previously only in the JSON `deadlock.trace`.
- **Soundness bug in `fslc refine`**: when an impl's violating transition reached a terminal (deadlock) state within the bound,
  forcing a full-length trace excluded the violation from all models, so it was missed
  (a non-monotonic behavior where raising the depth reduced detection). Resolved by switching to a dedicated solver that
  checks each prefix with only the constraints up to step t. Added a regression test (a residual case of the
  "vacuous refines" regression class in `examples/gallery/`).

### Documentation
- **Rescoped the layer-chain propagation claim to safety**: in `DESIGN-layers` §1/§6 and `LANGUAGE` §10,
  made explicit that refinement propagates safety (invariants, control guards, behavioral inclusion) but not liveness
  (`leadsTo`/`responds`), because of stuttering, and that liveness must be re-verified at each layer
  with `fair` required on progress actions.
- Documented the self-spec findings in their authoritative surfaces: dead-ghost
  vacuity, intended terminal states, transition invariants, finite-state
  expressiveness limits, deadlock diagnostics, and single-property selection.

### License / distribution (preparing for OSS release)
- **Finalized the license as Apache License 2.0** (rights holder: Copyright 2026 Ryoichi Izumita).
  Added `LICENSE` (full text) and `NOTICE`. Updated pyproject's license to the SPDX form `Apache-2.0`
  (previously only the `MIT` label with no LICENSE file), and tidied up authors, urls, classifiers, and keywords.
  Added an `SPDX-License-Identifier: Apache-2.0` header to all Python sources. The dependencies (lark / z3-solver) are
  both MIT and compatible with Apache-2.0.
- Updated the public repository URL to `github.com/ymm-oss/fsl` (links in README / install.sh / CHANGELOG,
  and reworded private-assumption phrasing for public release). Removed the generated search index `docs/index.bleve/` from tracking,
  added it to `.gitignore`, and also added Claude Code's local settings to the ignore list.

## [1.2.10] - 2026-06-15

Theme: **audit triage (issue #12) — settling two design decisions (doc alignment)**. We analyzed that
keeping the code as-is is appropriate and aligned the DESIGN documents with the actual state and intent.

### Documentation
- **Aligned the check ordering in DESIGN-refinement §2** with reality. For t>0 (between steps), the transition correspondence is checked
  before the type-bound check; for t=0 (initial state), the type-bound check (`map_out_of_bounds`) is done before the init correspondence.
  Because at t=0 a range escape almost always accompanies an init mismatch, we prioritize `map_out_of_bounds`, which can directly point at
  a mapping-expression bug — reflecting the design intent (the purpose in §2), and resolving the
  self-contradiction in the previous ordering description.
- Made explicit in **DESIGN-seq §5** the cross-engine difference for invariants containing unguarded partial Seq operations
  (`head`/`pop`/`at`). `verify`/`prove` (BMC) read don't-cares symbolically, while the runtime
  `Monitor` concretely returns `partial_op`. Because there is essentially no guarantee that a don't-care matches between symbolic and concrete,
  we strongly recommend the size-guarded idiom (the guarded version is verified to agree across both engines).

## [1.2.9] - 2026-06-15

Theme: **audit triage (issue #12) — settling design-decision items (continuation of Batch E-c)**.
Items previously treated as deferred were addressed in line with the recommendation, after verifying on real hardware.

### Fixed
- **A `push` to a full `Seq` was reported by the runtime (Monitor) as `partial_op`**; changed to report it as
  **`type_bound`** (a violation of the implicit `_bounds_*` length invariant), to match BMC / DESIGN-seq
  (`runtime.py`). This resolved the conformance fidelity gap where the same operation split into BMC=`type_bound` / runtime=`partial_op`.
  push always appends as a total function, and exceeding capacity is detected by the post-store bounds invariant.
- Added a note about the case where **`fslc refine` returns an impl's own invariant violation as-is**
  (`refine.py`). Clarified that this is a property of the refinement *input* (the impl spec), not the refinement verdict,
  so it is not confused with `refinement_failed` (LANGUAGE §10).
- (Documentation) Added to `parse()`'s docstring that if you need compose's display names you should use `parse_src`
  (`parse()` discards `display_names`, so dotted aliases appear under their physical names).

### Kept as-is by design decision (recorded in issue #12)
- The t=0 check ordering for refinement: there is tension between the ordering description in DESIGN-refinement §2 and
  `map_out_of_bounds`'s usefulness for "directly pointing at a mapping-expression bug," and existing tests expect bounds-first.
  We keep the current behavior, which precisely points at mapping bugs, and leave the §2 interpretation to maintainer judgment.
- Don't-care handling of Seq head/pop/at in invariant context: a guarded invariant is protected by short-circuiting,
  so the practical harm is small, and `in_invariant` propagation would be a broad change, so we keep the current behavior.

## [1.2.8] - 2026-06-15

Theme: **audit triage (issue #12) — runtime/refine/doc alignment batch (Batch E-c)**.
Items requiring design interpretation were addressed selectively after verifying on real hardware.

### Fixed
- Fixed an issue where **`Monitor.step()` evaluated let before requires and, when the guard did not hold, returned
  `partial_op` instead of `requires_failed`** (`runtime.py`).
  Calling an action that writes `let h = q.head()` after `requires q.size() > 0` on an empty queue
  produced a partial_op rather than a guard failure. Now let and requires are interleaved in source order, and
  on the branch where the guard fails, the subsequent let's partial op is never reached
  (DESIGN-v1 §5: let is usable only in subsequent requires).
- Fixed an issue where **`fslc refine` did not type-check the argument expressions of action mappings**
  (`refine.py`). Per DESIGN-refinement §3, it now matches the abs action's parameter types against
  the static types of the mapping argument expressions (when a type is unknown it skips the check to avoid false positives).
- (Documentation) Added to `parser.py`'s docstring that compose/requirements expansion raises `FslError`
  directly (not wrapped in VisitError). Clarified the DESIGN-bridge §3 statement that "generated artifacts
  import only runtime and pytest" to match the implementation, which also allows `random` for fixed-seed walks
  and `pathlib` for path resolution.

### Remarks (deferred this time by design decision; continued in issue #12)
- The t=0 check ordering for refinement (`map_out_of_bounds` and init correspondence): there is tension between
  the ordering description in DESIGN-refinement §2 and "the usefulness of map_out_of_bounds for directly pointing at a mapping-expression bug" (§2),
  and existing tests expect bounds-first. We keep the current behavior and leave it to maintainer judgment.
- Don't-care handling of Seq head/pop/at in invariant context (alignment with BMC): a guarded
  invariant is protected by short-circuiting, so the practical harm is small, while `in_invariant` propagation would be a broad change, so we
  defer it.
- `parse()`'s base_dir fallback / discarding display_names: this affects the compatibility of the library API,
  so we defer it (the CLI already uses parse_src + the parent directory).

## [1.2.7] - 2026-06-15

Theme: **audit triage (issue #12) — model/grammar alignment batch (Batch E-b)**.
Fixed 4 drifts between LANGUAGE.md / DESIGN-v1 and the code.

### Fixed
- Fixed an issue where **`Set<Bool>` / `Map<Bool, ·>` were rejected as type errors**. LANGUAGE.md §2 includes
  Bool among the bounded scalars. Added Bool to `is_bounded_scalar_type`, and made the Z3 encoding,
  `domain_range`, the implicit bounds invariant, the runtime concrete evaluation, refinement mappings, and display
  support Bool keys/elements (`_z3_domain_value`/`_map_domain`/`_display_map_key`).
- Fixed an issue where **`fslc check` rejected a spec with zero actions as a semantic error**. In LANGUAGE.md
  §7.1, check is syntax/name/types only. Introduced `build_spec(semantic_check=...)`, and the rejection of zero
  actions is now done only on the verify/prove/scenarios side (check is ok, verify is as before).
- Fixed an issue where **the business-dialect-only `stage(...)` passed check in a regular spec and then could not be evaluated by verify**.
  Added `check_stage_usage` to build_spec's semantic check, rejecting `stage(...)` in a kernel spec as a type error
  (since it does not remain in the spec after business expansion, business is unaffected).
- Fixed an issue where **the deprecation warning for `Map<Int, ·>` was only emitted for a spec with a domain type**. Removed the condition so that
  a deprecation warning (with a rewrite hint) is emitted for every spec using an Int-key Map.
- (Test) Hardened the warning assertion in `test_cart::test_fixed_verifies`. Because the Map<Int> warning now
  always fires, the vacuously-true premise that "all warnings are Map<Int" had become fragile. Changed it to confirm the presence of
  the expected 2 Map<Int deprecation warnings (which can coexist with verify's advisory warnings).

## [1.2.6] - 2026-06-15

Theme: **audit triage (issue #12) — explain/testgen batch (Batch E-a)**.

### Fixed
- Fixed an issue where **`fslc explain` uniformly excluded weakening of the init block from the counterfactual search**
  (`explain.py`). A user invariant established in init (e.g. BalanceNonNegative in audit_log.fsl)
  was misreported as "no counterfactual." Now init weakening is also included in the search, and
  the diagnostic is tagged with `origin: "init"` / `label: "init weakening"`.
- Fixed an issue where **the `requirement` of `fslc explain`'s acceptance/forbidden witnesses became None**.
  Registered the id/text of acceptance/forbidden in reqs, and included the scenario's
  `acceptance`/`forbidden` fields in requirement resolution.
- Fixed an issue where **`fslc testgen` did not generate the rejection check for forbidden scenarios**
  (`testgen.py`). In addition to replaying the setup steps, it now generates a pytest that runs the final step and asserts that it is rejected
  (`ok==False`, and the `rejected_by` kind matches when required).
- Added a regression test for compose expansion (Batch D) (verifying the const prefix for type/binder_range/param_range
  and the alias-access rewriting of sync arguments).

## [1.2.5] - 2026-06-15

Theme: **audit triage (issue #12) — compose expansion batch (Batch D)**. Fixed an issue where, when a component's
`const` was referenced in a range/binder/param/sync argument, at expansion time the const was prefixed with `alias__`
but the reference in the expression was not rewritten, leaving it unresolved.

### Fixed
- Fixed an issue where **the domain bound expression of a component's `type T = 0..MAX` was not rewritten and went unresolved**
  (`compose.py` `_prefix_component_items`). After expansion it was inconsistent with `alias__MAX`.
- Fixed an issue where **the lo/hi expressions of `binder_range` (`forall k in 0..MAX`) and `param_range` (`action f(n in 0..MAX)`)
  were not rewritten**. `_rewrite_binder`/`_rewrite_params` now receive the component's const set and
  prefix const references in range bounds.
- Fixed an issue where **`alias.x` references in sync argument expressions were not rewritten and remained in the post-expansion AST**
  (`_expand_sync_action`). They are now resolved to physical names with `_rewrite_expr` before assignment.
- The static check for "type mismatch of synchronized arguments" (DESIGN-compose §2) is not implemented this time, since the compose layer
  has no type inference; it is left to the type check after `build_spec` (the arity check is still performed as before).

## [1.2.4] - 2026-06-15

Theme: **audit triage (issue #12) — acceptance/forbidden/mutate batch (Batch C)**.
Fixed 6 items in acceptance/forbidden scenario replay and mutation aggregation, plus 1 parse bug for
zero-argument steps found during verification.

### Fixed
- Fixed a bug where **a zero-argument call (`noop()`) in an acceptance/forbidden step was parsed as `[None]` and
  caused an arity mismatch** (`grammar.py`). Removed the None coming from `maybe_placeholders`
  (same kind as the refinement zero-argument mapping in commit cca8627). It went undetected because existing examples always used calls with arguments.
  Found while writing the Batch C tests.
- Fixed an issue where **a `const` reference in an acceptance/forbidden step argument was not resolved and was passed as a bare string,
  causing `bad_call`** (`acceptance.py`). Changed `_literal_value` to resolve `spec["consts"]`
  (an undefined const is a structured error).
- Fixed an issue where **when the `expect` expression was non-bool, `_EvalError` propagated and `run_check` crashed without returning an envelope**.
  Now expect evaluation is caught and turned into a structured acceptance-failure result.
- Fixed an issue where **unknown actions / arity mismatches in forbidden setup/final steps raised an exception instead of returning a structured failure dict**.
  Now it returns a result with `failed_step`, and the kind is also split into
  `forbidden_setup`/`forbidden` (resolving the shared `_err`'s fixed `kind="acceptance"`).
- Fixed an issue where **the aggregation of `fslc mutate --by-requirement` mixed in acceptance/forbidden ids and kills**
  (`mutate.py`). Per DESIGN-mutate §4 it now targets only the formalization of the requirement block,
  excluding AC/FB ids (also resolving the erroneous `empty_formalization` attached to AC-2, etc.).

## [1.2.3] - 2026-06-15

Theme: **audit triage (issue #12) — typestate batch (Batch B)**. Fixed 2 from-state
extraction misses in `fslc typestate`. Both misjudged a sound transition as `relational` and unduly dropped the entity's
applicability to `none`.

### Fixed
- Fixed an issue where **the from-state could not be extracted from a conjunction in `requires`, leading to a misjudgment as `relational`**
  (`typestate.py`). For a binding like `requires e.st == A and q > 0`, the `and` node was
  unhandled, so the from-state `e.st == A` was not picked up. Extended `_enum_guard_states` /
  `_opt_guard_states` to recurse over `and` the same way as `or`.
- Fixed an issue where **the from-state in an `if` condition was not extracted, leading to branch transitions being misjudged as `relational`**.
  For a branch like `if light == Red { light = Green }`, it now derives each branch's from-state from the enclosing condition
  (a status-only `else` is treated as the complement). With this,
  `tiny_traffic_light.tick` is correctly classified as `branching` (with a from-state), and
  applicability becomes `full`. `typestate:325` (emitting branching to `_emit_ts`) is also resolved accordingly.

## [1.2.2] - 2026-06-15

Theme: **starting triage of the automated code audit (issue #12) — soundness batch (Batch A)**.
We triaged 44 items (42 unverified + 2 verified-but-unfixed) via 7-way parallel verification and real reproduction, and fixed the 5
that directly bear on soundness/correctness. All confirmed against real CLI behavior.

### Fixed
- Fixed an issue where **a `Set<bounded scalar>` got no implicit type-bound invariant, so out-of-range elements were missed**
  (`model.py`/`bmc.py`/`runtime.py`). `s.add(99)` into a `Set<Id>` (Id=0..3) stayed
  `verified` (a false verification success). Introduced the `set_bounds` AST node, which checks via Z3 ForAll / concrete evaluation that all elements are
  within the element type's range (no false positive for an explicitly initialized set).
- Fixed an issue where **the value-bound invariant for `Map<Int, bounded value>` was not generated, so out-of-range values were missed**.
  `m[0] = 99` into a `Map<Int, Qty>` (Qty=0..5) stayed `verified`.
  Introduced the `map_value_bounds` AST node, which checks the value type's bound over the Int-key Map's effective domain
  (the existing convention `0..max(consts)` of `_map_domain`).
- Fixed an issue where **`fslc explain`'s `--max-mutants` terminated early before the weakening search**
  (`explain.py`). Changed the cutoff to count actually-processed weakening mutants rather than the index across the whole
  `enumerate_mutants` (resolving the loss of counterfactuals).
- Fixed an issue where **a `_PartialOp` (partial operation) during invariant evaluation leaked from `step()` as an exception**
  (`runtime.py`). When an invariant expression hit a division by zero, the head of an empty Seq, etc., the exception propagated,
  violating the DESIGN-bridge §1.2 contract that "step() always returns a result dict." It now returns a structured result as a `partial_op`
  violation.
- Fixed an issue where **`fslc testgen` completely broke with a `NameError` (`parse` not imported) when `-o` was omitted**
  (`testgen.py`). `default_output_name` was calling the unimported `parse`.
  Like `generate_test_file`, it now uses `parse_src(src, base_dir)` and passes the spec file's parent directory as base_dir
  for relative-path resolution of compose specs.

## [1.2.1] - 2026-06-15

Theme: **fixing verified bugs detected by the automated code audit (composer-2.5)**. Fixed 3 items
including 1 bearing on the verifier's soundness (issues #13/#14/#15). All confirmed against real CLI behavior.

### Fixed
- Fixed an issue where **the `where` clause of a `leadsTo` binding was discarded and a wrong `violated` was reported**
  (issue #13). Because `expand_leadsto_bindings` (`bmc.py`) dropped the binder's `where` and enumerated all
  domain values, even `forall p: T where p > 0` checked `p = 0` as a separate binding,
  potentially producing a spurious counterexample at a value not satisfying where (verifier soundness). Fixed to concretely evaluate `where`
  and filter the enumeration. Also fixed the over-constraint (unsat) where `init_constraints`'s `run_collect`
  ignored the `where` of a nested `forall`.
- Fixed an issue where **a missing spec file was reported as `kind:"internal"`/exit 3**
  (issue #14). In `run_check`/`run_scenarios` (`cli.py`), the `except FileNotFoundError`
  came after `except Exception` and was unreachable. Reordered them so io errors become
  `kind:"io"`/exit 2 per LANGUAGE.md §7.
- Fixed an issue where **compile-time integer division `/` was unimplemented** (only `+ - *`) (issue #15).
  Per the "four arithmetic operations" in DESIGN-v1 §3.1, added division to `eval_const` (`model.py`). Its semantics
  match the runtime (`_euc_div`, Euclidean division), and division by zero is a compile error of `kind:"type"`.
  So that a range bound like `type K = 0..(MAX / 2)` passes.

## [1.2.0] - 2026-06-14

Theme: **the validation suite for AI formalization** (roadmap #1 completed). Added a set of detectors that close the gap
between the "internal consistency of the spec" that the verifier guarantees and "whether the spec is faithful to the original intent,"
casting a detection net over the errors AI writes into a spec (under-constraint, vacuity, omission/fabrication, mix-up).
The discipline before writing (formalization memo, recommended practices) goes into the skill, and the effect is measured on an error-injection bench.

### Fixed
- Fixed an issue where **a refinement zero-argument abstract action mapping** (`action foo() -> bar()`)
  failed with the spurious error `expects 0 arguments` (removing the None coming from `maybe_placeholders` in `grammar.py`'s
  `mapped_action_target`/`req_mapped_action_target`). It went undetected because existing specs all mapped zero-argument impls
  to `stutter`. A byproduct of the fsl-ui spike (#9).

### Added
- **The fsl-ui spike** (#9): a study of a screen-transition dialect. We hand-wrote the screen flow of a return-request flow in plain fsl,
  and confirmed it is verified + proved and that the refine into the requirements layer also holds
  (`examples/ui_spike/`, findings in `docs/DESIGN-ui.md`). Screen flows can be expressed without changing the kernel semantics,
  and the dialect looks viable as AST sugar (go/no-go in DESIGN-ui).
- Added **`fslc explain` (issue #7)**. It turns a spec's skeleton (state/action/requires/writes/
  properties/implicit type bounds and partial_op checks) into JSON via loc-based source excerpting and structural traversal, and, per
  user invariant, generates a counterfactual trace of removing requires/assignment/fair, reusing the
  `mutate`/`verify` machinery. For invariants where no counterfactual is found at depth K, it does not error but states so explicitly,
  and shapes reachable/scenarios witnesses into a staged narration.
- Added **`fslc typestate` (design spec → typestate / applicability check for phantom types + TS scaffold)**.
  For each `(entity, action)`, it judges `derivable` if the from-state is a **local guard on the entity's own state**
  (`requires e.status == S`), `branching` if it is a data-dependent to-state inside an `if`, and `relational`
  if it **assigns a state but has no local guard** (the premise lives in an external structure such as a queue).
  `relational`/`branching` are not exposed in the type; they remain as runtime/verification obligations along with reasons (diagnostics)
  and the action's requirement IDs (the business-layer `transition ... by <actor>`, etc.). An entity's `applicability` is
  `full` only when all transitions are `derivable`/`branching` (it does not claim full while dropping transitions it could not understand).
  The corresponding state machines come in three forms: **enum-valued struct fields, enum-valued state variables (the business
  `process`/stages), and `Option<_>` slots (none/some ≈ Empty/Filled)**.
  `--ts` emits to stdout the TypeScript only for derivable entities. The output is the same JSON envelope as the other commands
  (`result:"typestate"`, exit 0).
- Added **`fslc mutate`** (issue #6). It applies a deterministic single mutation
  (delete/negate requires, delete an assignment, swap enums, ±1 on integer/type bounds, swap then/else,
  delete fair) to the post-dialect-expansion kernel AST, re-runs `build_spec` per mutant, and reports in JSON whether it can be killed
  by BMC/acceptance/forbidden/refinement. A spec whose baseline is not clean is not mutated and returns the
  baseline result. `--by-requirement` aggregates by the requirement tag of the killed property and warns on a
  zero kill as `empty_formalization`. Survivors are review data, and `mutate`'s exit is always 0.
- Added the **`--strict-tags` lint** (issue #5) to `fslc check` / `fslc verify`.
  Only for ok/verified/proved successful results, it emits as warnings the untagged action/invariant/reachable/leadsTo and
  unreferenced requirement IDs (`--requirements ids.txt` and the requirements-dialect `requirement`
  block). The dialect-generated `tick` is excluded by an explicit marker, and the default (no flag)
  output is as before.
- Added **vacuity checks** (issue #4) to `fslc verify`. On the verified/proved path it emits as warnings
  `vacuous_implication` (the unreachable antecedent of an implication invariant), `vacuous_leadsto`
  (an unreachable leadsTo trigger), and `always_true_requires` (a requires clause that is always true
  under the preceding requires context). Added `--vacuity warn|error|ignore`
  (default warn), where error makes it `result:"error"` / exit 2.
  Actions with coverage false and compose synchronized actions are out of scope for `always_true_requires`
  (a synchronized action's clauses are inherited copies from its components — an identical guard across components is by design,
  with each component defending its own contract, and is checked by verifying the component spec alone).
- Added **`forbidden` (negative acceptance criteria / must-forbid)** (issue #3) to the requirements dialect.
  `forbidden FB-1 "source text" { <steps> expect rejected }` writes an "operation sequence that should be rejected,"
  and at check time the concrete Monitor verifies that all setup steps are ok and **the final step is rejected**
  (either not-enabled or an invariant/type_bound/partial_op/ensures violation). If it is
  accepted, `kind: "forbidden"` (detecting under-constraint = a missing guard that a safety invariant would stay silent on);
  if the setup is not enabled, `kind: "forbidden_setup"`. It outputs `forbidden_<ID>`
  to scenarios (with `rejected_by`) and flows into testgen's negative tests. The verification engine and
  Monitor are unmodified.

### Documentation / workflow
- Added the **validation workflow for AI formalization** (issue #2) to the skill.
  The discipline that closes the gap between the "internal consistency" the verifier guarantees and faithfulness to the original intent:
  a **formalization memo** before writing (chat output, with only assumptions folded into the `.fsl` as `// ASSUME-n:`
  comments/tags), a **natural-language→syntax reverse-lookup table**, the discipline of appending to the assumption ledger during repair,
  and **recommended practices** (positive-example pairs, one requirement = one declaration, domain sizing,
  cross-verification of high-risk specs — all optional; heavy procedures are not mandated).
- Added `examples/validation/order_refund.fsl` (proved) as the maintained example
  of the workflow. Demonstrated how the positive-example pair
  `reachable FullyRefunded` catches with `reachable_failed` a first version where "the safety invariant passes but the refund path is dead."
- Expanded the documentation index for the validation artifacts available at that release.

## [1.1.0] - 2026-06-12

### Added
- **Integer division `/` and remainder `%`** (added to arithmetic, at the same precedence as `*`). Division by zero is
  defined totally as 0 in both evaluators (and explicitly pinned in the Z3 encoding), and in an action context the divisor != 0 is
  checked as an implicit `partial_op`. Negatives are Euclidean (`0 <= a%b < |b|`).
  → This lets you write axis recovery (`c / SLOTS`, etc.) when 2D data is flattened into a single key.

### Documentation / idioms
- Added to LANGUAGE.md and the skill the **idiom for flattening 2D data** (Maps cannot be nested → a single product-domain type + `/` `%`).
- **Codifying discrete-time SLAs**: the placement rules for `time`/`deadline`, the semantics of `age`, and the meaning of
  `urgent` = freezing time. In particular, made explicit the **trap of making an always-enabled action urgent, which makes the
  deadline hold vacuously**, and the correct **deadline-urgency pattern**
  (make urgent only a guarded action that is enabled only when the deadline is reached).
  Added the official example `examples/nfr/support_sla.fsl` (proved).
- External authoring trials motivated the above documentation improvements for
  independently producing a proved spec from the skill alone.

## [1.0.3] - 2026-06-12

### Added
- Added `CHANGELOG.md` (this file), so the changes per release can be seen at a glance.

## [1.0.2] - 2026-06-12

### Fixed
- **BUG-020**: Fixed an issue where `Monitor.enabled()` raised a `_PartialOp` exception on a `let` containing a guarded partial operation
  (e.g. `let j = queue.head()` after `requires queue.size() > 0`).
  Now `requires` is evaluated first and short-circuits, so an action that does not satisfy its guard is simply treated as not enabled. The
  `partial_op` detection during `step()` execution is maintained as before. `fslc verify` was already correct, and
  the impact was on the runtime Monitor / replay / testgen.

### Quality assurance (tests)
- Added a **Z3-independent brute-force ground-truth oracle** (`tests/oracle.py`). It BFS-exhausts the bounded reachable states with the Monitor's
  concrete semantics and cross-checks the ground truth of invariant violations, reachability, and
  deadlock against the BMC verdict (detecting false negatives = misses).
- Added test suites for **concrete-replay soundness** of counterexample traces and witnesses, an **independent refinement oracle**,
  **metamorphic** properties (guard removal → violation, rename invariance, depth monotonicity), and
  **robustness** (JSON serialization, exit-code consistency, no leakage of internal names).
- Total tests 208 → 301 (+69 skipped, about 260 seconds).

### Documentation
- Updated the README to the current state with test count, docs list, examples tree, etc.

## [1.0.1] - 2026-06-12

### Fixed
- **A soundness bug in refine**: when the impl deadlocked just short of the search depth,
  the full expansion became unsatisfiable so all violation checks were missed, and it wrongly
  returned `refines`. Fixed by switching to incrementally checking each reachable prefix and cutting off
  at the depth where it became unsat (resolving the miss of control violations).

### Added
- `fslc version` / `fslc --version` / `-V` (version display).

## [1.0.0] - 2026-06-11

The de facto first release. FSL (AI-native formal specification language) and the verifier `fslc`.

### Core verification
- **BMC** (bounded model checking, shortest counterexample) / **k-induction** (`--engine induction`,
  infinite-depth `proved` and `unknown_cti`→auxiliary-invariant loop).
- `invariant` / `reachable` (witness) / `leadsTo` + weak fairness (`fair`,
  lasso counterexamples). Automatic checks: type bounds, partial operations (`partial_op`),
  action coverage (unsat-core diagnostics), and deadlock.
- Type system: domain types, enum, struct (`Option<scalar>` fields allowed),
  `Option<T>` / `Map` / `Set` / `Seq<T, N>`.

### Implementation bridge, composition, refinement
- `fslc scenarios` (integration-test scaffold), `fslc replay` (log conformance),
  `fslc testgen` (pytest conformance scaffold), `fslc.runtime.Monitor` (concrete execution).
- `fslc refine` (fidelity checking via refinement mapping, with conditional expressions in mapping expressions).
- `compose` (namespaced composition, synchronized actions, `internal`).

### The three-layer dialects and traceability
- `business` (consulting) / `requirements` (requirements, with `branches`, `acceptance`,
  `implements`) / `spec` (design), chained via refinement.
- The declaration tag `"ID: source text"` propagates requirement IDs into all diagnostics (counterexamples, CTIs, coverage, scenarios).

### Non-functional requirements
- Behaviors for authorization, audit, capacity, and reliability are done with idioms, while SLAs/timeouts are checked with
  discrete time (`time` / `urgent` / `age` / `deadline`).

### Distribution and use
- A case-study gallery (valid examples / invalid-example catalog / adversarial), examples for PM/consulting/three-role integration, and
  an example conformance test against a plain Python implementation.
- A one-liner installer (with ZIP-download support) and an Agent Skill for AI agents.

[Unreleased]: https://github.com/ymm-oss/fsl/compare/v1.3.1...HEAD
[1.3.1]: https://github.com/ymm-oss/fsl/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/ymm-oss/fsl/compare/v1.2.10...v1.3.0
[1.2.10]: https://github.com/ymm-oss/fsl/compare/v1.2.9...v1.2.10
[1.2.9]: https://github.com/ymm-oss/fsl/compare/v1.2.8...v1.2.9
[1.2.8]: https://github.com/ymm-oss/fsl/compare/v1.2.7...v1.2.8
[1.2.7]: https://github.com/ymm-oss/fsl/compare/v1.2.6...v1.2.7
[1.2.6]: https://github.com/ymm-oss/fsl/compare/v1.2.5...v1.2.6
[1.2.5]: https://github.com/ymm-oss/fsl/compare/v1.2.4...v1.2.5
[1.2.4]: https://github.com/ymm-oss/fsl/compare/v1.2.3...v1.2.4
[1.2.3]: https://github.com/ymm-oss/fsl/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/ymm-oss/fsl/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/ymm-oss/fsl/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/ymm-oss/fsl/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/ymm-oss/fsl/compare/v1.0.3...v1.1.0
[1.0.3]: https://github.com/ymm-oss/fsl/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/ymm-oss/fsl/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/ymm-oss/fsl/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/ymm-oss/fsl/releases/tag/v1.0.0
