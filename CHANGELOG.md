# Changelog

The change history of this project. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versioning follows [Semantic Versioning](https://semver.org/). Each version corresponds to an annotated git tag (`v1.0.x`).

## [Unreleased]

### Added
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

## [2.4.0] - 2026-06-29

### Documentation
- `skills/fsl/SKILL.md` repair protocol: added the over-constraint mirror of the
  hollowing warning — after fixing a `forbidden`/`violated` by tightening a guard,
  re-run `verify` and confirm the action's `action_coverage` is still `true` (and
  affected `reachable`s still witnessed), since an over-tight guard surfaces as a
  *new* `reachable_failed`/`covered:false`. (Surfaced by the #22 repair DOGFOOD,
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
- **A set of self-specs for meta-circular dogfooding** in `examples/self/`: three specs that model fslc's own design contracts
  in FSL (`fslc_session` = CLI result classification and exit-code severity,
  `fslc_monitor` = stickiness of replay-runtime rejection, `refinement_algebra` = safety
  propagates, liveness does not). All are proved. Pinned-result test `tests/test_self_examples.py`.
- **`terminal { <predicate> }` block (addressing DOGFOOD-11 F23)**: declares a halting state satisfying the predicate
  as an "intended terminal" and excludes it from deadlock checking. Whereas `--deadlock ignore`
  uniformly ignores all halting states, this lets you single out only the intended halts, while unexpected deadlocks are
  still detected. Used by `examples/self/fslc_session` and `fslc_monitor` (LANGUAGE §1/§6).
- **`fslc verify --property <Name>` (addressing DOGFOOD-11 F27)**: checks just a single invariant.
  This makes it easier to confirm a violation of a targeted invariant with a non-vacuous probe (a nonexistent name is a usage error = exit 2).
- **Vacuity detection of dead-ghost tautologies (addressing DOGFOOD-11 F22, top priority)**: `--vacuity`
  now statically detects with Z3 an "invariant that, when a frozen state variable assigned by no action is pinned to its init value, becomes
  always true regardless of the values of dynamic variables" (kind `tautology_over_frozen`). It warns at verification time about
  hollow (always-true) invariants that previously both verify and vacuity missed, surfacing only via mutate's survival rate.
  Invariants that do not reference a frozen variable / do not reference state are out of scope. Confirmed zero false positives across the existing corpus.
- **Transition invariant `trans { }` (addressing DOGFOOD-11 F24)**: `trans Name { old(x) => ... }`
  lets you directly declare cross-action two-state safety. BMC checks each reachable transition, induction checks it in the step case,
  successful output includes `transitions_checked`, and a violation returns `violation_kind:"trans"`.

### Fixed
- **Test suite runs without a `.venv` (CI portability)**: the subprocess-based tests invoked the
  CLI through a hardcoded `ROOT/.venv/bin/python` and a macOS-only `/private/tmp` scratch path,
  which failed on the CI runners. Now use `sys.executable` and `tempfile.gettempdir()`.
- **Include the state in the deadlock warning (addressing DOGFOOD-11 F26)**: the `--deadlock warn` warning
  message now shows which state it halted in (e.g. `deadlock reachable at step 1
  (state: status=ToolFault, ...)`). The state was previously only in the JSON `deadlock.trace`.
- **Soundness bug in `fslc refine`**: when an impl's violating transition reached a terminal (deadlock) state within the bound,
  forcing a full-length trace excluded the violation from all models, so it was missed
  (a non-monotonic behavior where raising the depth reduced detection). Resolved by switching to a dedicated solver that
  checks each prefix with only the constraints up to step t. Added a regression test (a residual case of the
  "vacuous refines" bug class in `docs/DOGFOOD-6.md`).

### Documentation
- **Rescoped the layer-chain propagation claim to safety**: in `DESIGN-layers` §1/§6 and `LANGUAGE` §10,
  made explicit that refinement propagates safety (invariants, control guards, behavioral inclusion) but not liveness
  (`leadsTo`/`responds`), because of stuttering, and that liveness must be re-verified at each layer
  with `fair` required on progress actions.
- **`docs/DOGFOOD-11.md`** (meta-circular dogfooding findings): records the blind spots where `--vacuity`/single verify
  miss "an always-true invariant over a variable that is never assigned (a dead ghost)" and it surfaces only via mutate kill-rate
  (F22), the absence of syntax for declaring intended terminal states (F23), the inability to directly
  assert a forbidden transition (F24), the expressiveness limits for relational/algebraic properties (F25), the deadlock-warn
  message lacking the state name (F26), and the absence of a single-invariant selector (F27).

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
- Added a real-run record of the above workflow, `docs/DOGFOOD-9.md`, and the example
  `examples/validation/order_refund.fsl` (proved). Demonstrated how the positive-example pair
  `reachable FullyRefunded` catches with `reachable_failed` a first version where "the safety invariant passes but the refund path is dead."
- Filled out the DOGFOOD index in `docs/README.md` to 1-9 (also resolving the unlisted 6/7/8).

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
- A blind expressibility test (`docs/DOGFOOD-8.md`, n=3): external validation that another agent, using the skill alone, can get a
  new domain to proved. The above documentation improvements address the gaps this test surfaced.

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
