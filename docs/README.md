# docs/ マップ

## まず読む

| Document | Contents |
|---|---|
| [`index.html`](index.html) / [`intro/index.ja.html`](intro/index.ja.html) / [`intro/index.en.html`](intro/index.en.html) | **GitHub Pages入口**。英語・日本語を選べる静的HTMLマニュアルとして公開できます。公開元は `main` ブランチの `/docs`、想定URLは `https://ymm-oss.github.io/fsl/` |
| [`intro/get-started.ja.html`](intro/get-started.ja.html) / [`intro/guides.ja.html`](intro/guides.ja.html) / [`intro/reference.ja.html`](intro/reference.ja.html) / [`intro/examples-background.ja.html`](intro/examples-background.ja.html)（英語版は `.en.html`） | **カテゴリハブページ（4本×2言語）**。日本語マニュアルの入口はこの4カテゴリ——はじめる／ガイド／リファレンス／実例と背景。各ハブは `assets/site.js` の `CATEGORIES`/`CHAPTERS` から動的に章一覧を描画するので、手書きの二重リストは存在しません |
| [`intro/concept.ja.html`](intro/concept.ja.html) / [`intro/when-to-use.ja.html`](intro/when-to-use.ja.html) / [`intro/quickstart.ja.html`](intro/quickstart.ja.html) / [`intro/guide.ja.html`](intro/guide.ja.html) / [`intro/business-layer.ja.html`](intro/business-layer.ja.html) / [`intro/requirements-layer.ja.html`](intro/requirements-layer.ja.html) / [`intro/design-layer.ja.html`](intro/design-layer.ja.html) / [`intro/syntax.ja.html`](intro/syntax.ja.html) / [`intro/analysis.ja.html`](intro/analysis.ja.html) / [`intro/language.ja.html`](intro/language.ja.html) / [`intro/cli.ja.html`](intro/cli.ja.html) / [`intro/errors.ja.html`](intro/errors.ja.html) / [`intro/glossary.ja.html`](intro/glossary.ja.html) / [`intro/mechanism.ja.html`](intro/mechanism.ja.html) / [`intro/domain.ja.html`](intro/domain.ja.html) / [`intro/db.ja.html`](intro/db.ja.html) / [`intro/ai.ja.html`](intro/ai.ja.html) / [`intro/examples.ja.html`](intro/examples.ja.html) / [`intro/design-notes.ja.html`](intro/design-notes.ja.html) | **日本語HTMLマニュアル（19章）**。`intro/language.ja.html` と `intro/cli.ja.html` は手書きではなく [`tools/build_site_reference.py`](../tools/build_site_reference.py) が `LANGUAGE.md` / `src/fslc/cli.py` から生成する物で、`LANGUAGE.md`/`cli.py` を変更したら再生成すること（`tests/test_site_reference_snapshot.py` が鮮度を検査する） |
| 同じ19章の `.en.html` 版 | **English HTML manual (19 chapters)**. Same generated-reference note applies to `intro/language.en.html` / `intro/cli.en.html` |
| [`INTRO-formal-methods-and-fsl.md`](INTRO-formal-methods-and-fsl.md) | **Introduction to formal methods and FSL**. Background for non-specialists, the role of FSL in AI-driven development, and considerations for an introductory PoC |
| [`LANGUAGE.md`](LANGUAGE.md) | **Language reference** (full syntax, semantics, CLI, idioms, the three-layer dialects, and NFRs). Read this if you are writing specifications |
| [`RELEASE.md`](RELEASE.md) | **Authoritative release procedure**: version PR, changelog rollover, exact-SHA `main` to `production` promotion, non-publishing workflow dry runs, confirmed annotated tag push, changelog-derived GitHub Release notes, and four-target artifact verification |
| [`intro/analysis.ja.html`](intro/analysis.ja.html) / [`intro/analysis.en.html`](intro/analysis.en.html) / [`GUIDE-analyze.ja.md`](GUIDE-analyze.ja.md) / [`DESIGN-analysis.md`](DESIGN-analysis.md) / [`DESIGN-code-audit.md`](DESIGN-code-audit.md) | **`fslc analyze` structural observation layer**. Site pages, a team-facing Japanese practical guide, and implementation design for TSG, graph projections, focused impact slices, action dependency/conflict graphs, structural metrics, batch analysis, refinement/project/code traceability, graph exports, schemas, and AI-review findings/candidates |
| [`intro/domain.ja.html`](intro/domain.ja.html) / [`intro/domain.en.html`](intro/domain.en.html) / [`DESIGN-domain.md`](DESIGN-domain.md) / [`DESIGN-effect.md`](DESIGN-effect.md) | **fsl-domain Functional DDD / async effect dialect**. Site pages and implementation design for `domain`, aggregate command/event ownership, pure decide/evolve lowering, saga/process-manager coordination, async effect lifecycle checks, multi-target scaffolds, runtime replay, and domain-specific findings |
| [`intro/db.ja.html`](intro/db.ja.html) / [`intro/db.en.html`](intro/db.en.html) / [`DESIGN-db.md`](DESIGN-db.md) | **fsl-db DB / multi-environment compatibility dialect**. Site pages and implementation design for `dbsystem`, schema/artifact/environment compatibility, rollout windows, and DB-specific findings |
| [`intro/ai.ja.html`](intro/ai.ja.html) / [`intro/ai.en.html`](intro/ai.en.html) / [`DESIGN-ai-hard.md`](DESIGN-ai-hard.md) | **fsl-ai hard-contract and agent-structure dialect**. Site pages and implementation design for `ai_component`, recursive `agent` composition, tool authority, human approval guards, runtime replay, and AI-specific findings |
| [`DESIGN-stochastic.md`](DESIGN-stochastic.md) | **fsl-stochastic external evidence layer**. Implemented semantics for precomputed eval JSONL, Bernoulli/proportion metrics, Wilson intervals, result statuses, and the boundary between statistical support and formal proof |
| [`DESIGN-causal.md`](DESIGN-causal.md) | **`causal` profile (review-only causal hypothesis graphs)**. Typed CausalModel, scope containment, lag/persists time semantics, delayed-feedback classification, evidence/expectation plane boundaries, and JSON contracts — causal claims never receive `proved`/`verified` |
| [`DESIGN-v1.md`](DESIGN-v1.md) | Language design document (design principles G1-G5, type-system design decisions, the repair protocol, and the roadmap) |
| [`DESIGN-enum-member-identity.md`](DESIGN-enum-member-identity.md) | Checked-Kernel enum-member identity and lookup: nominal resolution of bare enum-member syntax, shadowing precedence, and the refinement-boundary go/no-go for `KernelModel.enum_members` |

## Implementation design by architecture and feature (DESIGN-*)

| Document | Subject |
|---|---|
| [`DESIGN-layers.md`](DESIGN-layers.md) | **Shared kernel + three dialects** (consulting / requirements / design): overall concept and validation |
| [`DESIGN-dialects.md`](DESIGN-dialects.md) | Implementation spec for the dialects (declaration tags, fsl-req, fsl-biz) |
| [`DESIGN-requirements-stage.md`](DESIGN-requirements-stage.md) | Shared typed `stage()` resolution and lowering for business and requirements expressions |
| [`DESIGN-collection-aggregates.md`](DESIGN-collection-aggregates.md) | Shared finite Binder/Aggregate IR, Set/Seq/range semantics, KPI metadata projections, and Public Kernel normalization |
| [`DESIGN-dialect-dispatch.md`](DESIGN-dialect-dispatch.md) | Shared-lexer dialect registry, significant-token rules, document annotations, diagnostics, and frontend contract |
| [`DESIGN-formatter.md`](DESIGN-formatter.md) | Lossless token/trivia boundary, canonical formatting policy, non-mutating CLI contract, and safe refusal range |
| [`DESIGN-migration.md`](DESIGN-migration.md) | Edition lint taxonomy, checked rewrite rules, explicit refusal boundaries, atomic write contract, and bulk-update procedure |
| [`DESIGN-id-policy.md`](DESIGN-id-policy.md) | Canonical ID ownership/link syntax, built-in templates, project overrides, lint diagnostics, and the no-auto-rename boundary |
| [`DESIGN-nfr.md`](DESIGN-nfr.md) | Non-functional requirements (mapping table, discrete-time SLA: time/urgent/age/deadline) |
| [`DESIGN-induction.md`](DESIGN-induction.md) | The k-induction engine (proved / unknown_cti / CTI) |
| [`DESIGN-induction-lemmas.md`](DESIGN-induction-lemmas.md) | `verify --engine induction --lemma`: independent candidate proof, CTI exclusion/retry, JSON and cache contract |
| [`DESIGN-explicit-engine.md`](DESIGN-explicit-engine.md) | `verify --engine explicit` (Rust-native): Z3-free concrete-state BFS, closure ⇒ `proved`, `unknown_budget` truncation, deterministic-init and binder-domain fail-closed gates; plus the `--engine auto` composite (explicit first, transparent BMC fallback, `engine`/`engine_fallback` tracking) |
| [`DESIGN-bfs-bmc-native-migration.md`](DESIGN-bfs-bmc-native-migration.md) | Accepted #761 migration plan for replacing frozen-Python BFS/BMC parity with a checked native Monitor/legacy-BFS/explicit-BFS/BMC decision and bidirectional witness matrix |
| [`DESIGN-literate.md`](DESIGN-literate.md) | Literate Markdown FSL: in-place blanking extraction of ` ```fsl ` fenced blocks, materialization, position-preserving diagnostics, scope boundaries (WASM waiver, LSP deferral) |
| [`DESIGN-from-state.md`](DESIGN-from-state.md) | Predictive BMC from a complete Monitor/replay logical-state snapshot (`verify --from-state`), including type validation, faithfulness metadata, cache/symmetry boundaries, and induction exclusion |
| [`DESIGN-trans.md`](DESIGN-trans.md) | `trans` (transition invariant / two-state safety) |
| [`DESIGN-temporal.md`](DESIGN-temporal.md) | leadsTo, weak fairness (lasso counterexamples), and respond scenarios |
| [`DESIGN-refinement.md`](DESIGN-refinement.md) | Refinement checking (mapping files, conditional expressions, preserve progress) |
| [`DESIGN-semantic-diff.md`](DESIGN-semantic-diff.md) | `fslc diff` bounded semantic comparison (bidirectional refinement, invariant implication, forbidden replay, scope and gate contract) |
| [`DESIGN-design-family.md`](DESIGN-design-family.md) | Accepted Phase 0 design-family decision: closed sidecar catalog, independent variant eligibility, directed bounded comparison, provenance/digest gaps, dogfood controls, and no native language/CLI commitment (#427) |
| [`DESIGN-diff-git.md`](DESIGN-diff-git.md) | Git/CI adapter for revision-consistent full-tree materialization and changed-spec batch semantic diff |
| [`DESIGN-approval.md`](DESIGN-approval.md) | Digest-bound human approval records, rendering drift checks, and approved-baseline semantic diff |
| [`DESIGN-compose.md`](DESIGN-compose.md) | Spec composition (namespaces, synchronized actions, internal) |
| [`DESIGN-bridge.md`](DESIGN-bridge.md) | Implementation bridge (runtime Monitor / replay / testgen) |
| [`DESIGN-log-replay.md`](DESIGN-log-replay.md) | Production JSONL replay through refinement mapping syntax: record contract, complete-observation boundary, first-divergence JSON, and Monitor execution |
| [`DESIGN-scenarios.md`](DESIGN-scenarios.md) | scenarios and the unsat-core diagnostics for coverage |
| [`DESIGN-seq.md`](DESIGN-seq.md) | Seq<T,N> (partial_op, type whitelist) |
| [`DESIGN-seq-partial-operations.md`](DESIGN-seq-partial-operations.md) | Accepted Seq partial-operation semantics: out-of-prefix reads report `partial_op` consistently across engines while guarded short-circuit reads remain defined |
| [`DESIGN-option-struct.md`](DESIGN-option-struct.md) | Option fields in structs |
| [`DESIGN-nested-option-support.md`](DESIGN-nested-option-support.md) | Accepted #841 implementation plan for full recursive nested `Option` support, fail-closed state-type boundaries, lossless JSON/replay, and cross-engine agreement |
| [`DESIGN-divmod.md`](DESIGN-divmod.md) | Integer division `/` and remainder `%` (total definition of division by zero, partial_op, Euclidean) |
| [`DESIGN-forbidden.md`](DESIGN-forbidden.md) | `forbidden` (negative acceptance criteria / must-forbid) — detecting under-constraint |
| [`DESIGN-vacuity.md`](DESIGN-vacuity.md) | Vacuity checking (invariants whose antecedent is unreachable, leadsTo whose trigger is unreachable, always-true requires) |
| [`DESIGN-strict-tags.md`](DESIGN-strict-tags.md) | The `--strict-tags` lint (matching untagged declarations and unreferenced requirements) |
| [`DESIGN-init-if.md`](DESIGN-init-if.md) | Statement-level `if` in `init` (lowered to path-conditional initial-state constraints, same branch shape as action bodies) |
| [`DESIGN-initialization.md`](DESIGN-initialization.md) | Kernel inline state initializers, normalization to `init`, overlap diagnostics, and implicit-default migration warnings |
| [`DESIGN-inline-range.md`](DESIGN-inline-range.md) | Inline anonymous range types (`x: lo..hi`) |
| [`DESIGN-spec-domains.md`](DESIGN-spec-domains.md) | `entity` / `number` in the kernel `spec` (decoupling a domain from the verification bound) |
| [`DESIGN-def.md`](DESIGN-def.md) | Non-recursive named predicate frontend sugar, expansion order, capture and diagnostic contract |
| [`DESIGN-precedence-policy.md`](DESIGN-precedence-policy.md) | The business-layer no-bypass precedence policy (#75) — why `business` keeps users from writing `state`/`invariant` directly |
| [`DESIGN-ledger.md`](DESIGN-ledger.md) | `fslc ledger` (turning verifier evidence into a per-requirement-id Markdown audit ledger for PM/audit) |
| [`DESIGN-assurance-classes.md`](DESIGN-assurance-classes.md) | Assurance-class vocabulary (`proved`/`bounded`/`replay-observed`/`statistical`/`not_run`) shared by `fslc ledger` and `fslc html`, and what each class does/does not guarantee |
| [`DESIGN-assurance-matrix.md`](DESIGN-assurance-matrix.md) | CI-internal C3 semantic assurance matrix: every semantic-feature/product-surface cell is exercised, a rejecting control, fail-closed unsupported, or reasoned not-applicable |
| [`DESIGN-triangulated-assurance.md`](DESIGN-triangulated-assurance.md) | CI-internal Triangulated Assurance: raw common observations, explicit observer lineages, executable three-edge agreement, calibration, federated ownership, and P1/P2/P3 pilots |
| [`DESIGN-document-requirement-claim-ir.md`](DESIGN-document-requirement-claim-ir.md) | `fslc document` foundation (issue #325): the versioned Requirement Claim IR (RCIR) v1 public contract and the deterministic requirement-claim projector |
| [`DESIGN-document-controlled-language-renderer.md`](DESIGN-document-controlled-language-renderer.md) | ja/en controlled-language renderer (issue #326): converts an RCIR v1 claim set into `fsl_tools::render_requirements_document` prose |
| [`DESIGN-document-cli.md`](DESIGN-document-cli.md) | `fslc document generate` / `fslc document claims` (issue #327): wires the RCIR projector and controlled-language renderer into the two CLI entry points |
| [`DESIGN-document-generated-markers-and-check.md`](DESIGN-document-generated-markers-and-check.md) | Generated block markers and `fslc document check` (issue #329): structural markers/frontmatter on generated artifacts and the purely structural drift checker |
| [`DESIGN-document-glossary.md`](DESIGN-document-glossary.md) | Glossary sidecar for `fslc document` (issue #330): presentation-only glossary generation and glossary-parity awareness in `fslc document check` |
| [`DESIGN-document-evidence-overlay.md`](DESIGN-document-evidence-overlay.md) | Evidence/assurance overlay for `fslc document` (issue #332): overlays saved external verification evidence onto a generated requirements document at requirement granularity |
| [`DESIGN-document-dialect-adapters.md`](DESIGN-document-dialect-adapters.md) | `fslc document`'s dialect boundary (issue #334, v1 slice): explicit rejection of non-kernel dialects rather than adapters, and the scope left open for cross-layer views |
| [`DESIGN-document-coverage-registry.md`](DESIGN-document-coverage-registry.md) | RCIR coverage registry and no-silent-omission gate (issue #328): classifies every authored semantic target into exactly one of `rendered`/deliberately-excluded/flagged-as-missing |
| [`DESIGN-mutate.md`](DESIGN-mutate.md) | `fslc mutate` (spec mutation, requirement stress report) |
| [`DESIGN-explain.md`](DESIGN-explain.md) | `fslc explain --readable` (verification bounds, skeleton enumeration, counterfactuals, witness narration) |
| [`DESIGN-html-report.md`](DESIGN-html-report.md) | `fslc html` (self-contained visual review report from explain + verify evidence) |
| [`DESIGN-typestate.md`](DESIGN-typestate.md) | `fslc typestate` (applicability check for state machine → typestate + TS scaffold) |
| [`DESIGN-blame-assignment.md`](DESIGN-blame-assignment.md) | Counterexample blame assignment (`fslc verify`/`fslc explain`): false-conjunct identification, per-step guard/effect backward slicing, and vacuity blocking-core localization |
| [`DESIGN-incremental-verify.md`](DESIGN-incremental-verify.md) | `fslc verify`'s persistent verdict cache (`src/fslc/verify_cache.py`): exhaustive cache-key enumeration, cross-depth counterexample reuse, and the soundness argument for why a cached verdict can never be stale |
| [`DESIGN-verification-cost.md`](DESIGN-verification-cost.md) | Fixed native/Worker verification cost schema, common Z3 statistics, property attribution, and aggregation semantics |
| [`DESIGN-ci.md`](DESIGN-ci.md) | Accepted merge and product-validation contract: complete Linux evidence before merge, deferred cross-platform validation, and blocking post-merge promotion evidence |
| [`DESIGN-ci-validator-inventory.md`](DESIGN-ci-validator-inventory.md) | Accepted #962 slice-1 CI validator inventory: machine-generated `tests/test_*.py` reachability record, required versus exempt tiers, and fail-closed detection of new unwired validator modules |
| [`DESIGN-hooks-enforcement.md`](DESIGN-hooks-enforcement.md) | Accepted #922 enforcement-boundary decision: shared-worktree Cargo serialization, common hook detectors, CI ownership, and task-completion approval seams |
| [`DESIGN-fsl-logic-test.md`](DESIGN-fsl-logic-test.md) | Accepted finite direct-spec generation and concrete/symbolic agreement test: detector calibration and exploration, never proof by volume or a public-assurance promotion |
| [`DESIGN-semantic-mutation-gate.md`](DESIGN-semantic-mutation-gate.md) | Accepted soundness-critical native-Rust mutation gate: reviewed semantic faults and scoped generic mutations calibrate detector power without changing product exits |
| [`DESIGN-error-envelope-input-shape-population.md`](DESIGN-error-envelope-input-shape-population.md) | Error-envelope matrix input-shape population: independent catalog/dispatch population, native dialect and CLI-leaf totality, and the boundary from the corpus compatibility registry |
| [`DESIGN-conformance-harness.md`](DESIGN-conformance-harness.md) | Dialect corpus conformance harness (`tests/test_dialect_conformance.py`, `tests/dialect_registry.py`): the Monitor/BMC-agreement/oracle safety net over every `.fsl` under `specs/`/`examples/`, with a loud, reviewable exclusion policy — a manual/reference check today, not a CI gate (see the design doc's "Cost and CI wiring") |
| [`DESIGN-coupled-change-metatest.md`](DESIGN-coupled-change-metatest.md) | Coupled-change metatests: native LSP corpus/index coverage in `rust/fsl-lsp/tests/corpus.rs`, plus frozen Python compatibility and DESIGN-doc map checks |
| [`DESIGN-referance-local-audit.md`](DESIGN-referance-local-audit.md) | Accepted opt-in local Referance semantic-drift audit: Store-first provenance evidence with CodeReferance as an auxiliary read-only detector, never a CI, merge, product, promotion, or release gate |
| [`DESIGN-changelog-fragments.md`](DESIGN-changelog-fragments.md) | Accepted #737 decision record: GO for `CHANGELOG.md` `[Unreleased]` fragments (C1), NO-GO for fragmenting the contract documents (C2), with the replay measurement, six fail-closed controls, migration sites, and rollback/reversal rule; implemented by `tools/aggregate_changelog.sh` and `changelog.d/` |
| [`DESIGN-rust-components.md`](DESIGN-rust-components.md) | Evidence-backed current design for all eleven Rust crates: responsibility and state ownership, dependency direction, contracts, candidates, uncertainty, and reevaluation triggers |
| [`DESIGN-rust-component-internals.md`](DESIGN-rust-component-internals.md) | Evidence-backed internal design for all eleven Rust crates: value flow, mutable-state ownership, failure and I/O boundaries, targeted dependency normalization, and touch-driven extraction |
| [`DESIGN-rust-port.md`](DESIGN-rust-port.md) | Accepted phased architecture for a native Rust CLI and browser Web Worker/WASM port, including solver/runtime dependency boundaries, shared semantic diagnostics, and cross-implementation parity gates |
| [`DESIGN-no-user-invariants-warning.md`](DESIGN-no-user-invariants-warning.md) | Accepted #961 suppression contract for the `no_user_invariants` model warning (`invariant`/`trans`/`forbidden`/`implements` only; not `reachable`/`leadsTo`/`acceptance`) |
| [`DESIGN-rust-integration.md`](DESIGN-rust-integration.md) | Required Rust-native product integration gate, contract inventory, dependency boundaries, and optional compatibility evidence |
| [`DESIGN-rust-lsp.md`](DESIGN-rust-lsp.md) | Accepted Rust-native `fslc-lsp` architecture: authoritative analysis boundary, document overlays/indexes, protocol contract, release migration, and ELD grounding laws |
| [`DESIGN-kernel-contract.md`](DESIGN-kernel-contract.md) | **Versioned normalized Kernel JSON boundary for external compilers**: typed expressions, spans and requirement origin, rollback semantics, schema compatibility, native conformance vectors, and release bundle |
| [`DESIGN-replay-trace.md`](DESIGN-replay-trace.md) | **Versioned backward contract for generated-code traces**: exact actions/params, logical ticks, complete state observations, Kernel-version pairing, legacy migration, goldens, and release bundles |
| [`DESIGN-reproducer.md`](DESIGN-reproducer.md) | **Versioned closed artifact for a violated specification's shortest bounded counterexample** (`reproducer.v1`): safety-invariant scope, explicit v1 rejections (leadsTo, refinement, induction/CTI, nondeterministic init), and the export boundary that keeps the `verify` envelope unchanged |
| [`DESIGN-origin-chain.md`](DESIGN-origin-chain.md) | Private source-origin propagation from dialect declarations through typed lowering, verification diagnostics, and counterexamples without changing public Kernel v1 |
| [`DESIGN-kernel-origin-v2.md`](DESIGN-kernel-origin-v2.md) | Accepted Public Kernel v2 provenance graph: portable source identity, exact coordinates, cardinality/reverse lookup, assurance/completeness, negotiation, and v1 migration boundary |
| [`RUST-PORTING.md`](RUST-PORTING.md) | Executable migration method for issue #195: reference authority, per-slice rewrite loop, differential/oracle/replay gates, Phase-0 evidence, and decisions |
| [`DESIGN-analysis.md`](DESIGN-analysis.md) | `fslc analyze` (Typed Semantic Graph, graph projections, focus impact slices, action dependency/conflict graphs, structural metrics, batch mode, refinement/project traceability graphs, DOT/Mermaid exports, schemas, AI-readable structural review findings/candidates) |
| [`DESIGN-code-audit.md`](DESIGN-code-audit.md) | Language-independent `@fsl.trace` annotations and the `analyze --projection code_audit` requirement-to-implementation audit contract |
| [`DESIGN-tag-drift.md`](DESIGN-tag-drift.md) | Deterministic declaration-tag identifier drift findings and `tag-review.v0` external review export contract |
| [`DESIGN-underspecification.md`](DESIGN-underspecification.md) | bounded `divergent_choice` / `unconstrained_effect` AI-review findings and question-form output |
| [`DESIGN-undecided.md`](DESIGN-undecided.md) | reserved `undecided:` declaration metadata, affected-requirement projection, ledger/HTML display, and acknowledged underspecification findings |
| [`DESIGN-annotations.md`](DESIGN-annotations.md) | shared typed requirement/undecided/kind/custom annotation IR, validation, adapters, ordering, and compatibility boundaries |
| [`DESIGN-ui.md`](DESIGN-ui.md) | fsl-ui (screen-transition dialect): spike findings, proposed expansion rules, go/no-go (#9) |
| [`DESIGN-domain.md`](DESIGN-domain.md) | fsl-domain (`domain`) Functional DDD / async effect dialect: aggregate ownership, command/event decide/evolve lowering, saga/process-manager actions, effect lifecycle state, findings, multi-target scaffolds, and runtime replay |
| [`DESIGN-effect.md`](DESIGN-effect.md) | fsl-effect lifecycle semantics used by fsl-domain: correlation, retry, timeout, idempotency, and guarantee boundary |
| [`DESIGN-saga-history.md`](DESIGN-saga-history.md) | Accepted domain design awaiting follow-up implementation: correlation-indexed saga phases preserve one-hot current-event semantics and prevent cross-correlation discharge |
| [`DESIGN-db.md`](DESIGN-db.md) | fsl-db (`dbsystem`) database compatibility dialect: multi-environment schema/artifact/feature-flag checks, finding schema, rollout assumptions, SQL/Prisma importers, and external preservation/engine evidence boundaries |
| [`DESIGN-ai-hard.md`](DESIGN-ai-hard.md) | fsl-ai (`ai_component` / recursive `agent`) dialect: tool authority, human approval, forbidden tools, fallback, event replay, agent scope/grant/orchestration/visibility analysis, finding schema, and guarantee boundaries |
| [`DESIGN-stochastic.md`](DESIGN-stochastic.md) | fsl-stochastic external evidence layer: precomputed eval JSONL, Wilson-bound threshold rules, statistical result schema, status priority, multiple-slice boundary, and external stochastic boundaries |
| [`DESIGN-docs-site.md`](DESIGN-docs-site.md) | This manual site's information architecture, navigation chrome, and the generated-reference-page template (`intro/language.*.html`, `intro/cli.*.html`) — produced with the Relational Design plugin |

## Evidence policy

Field trials, spikes, and audits are temporary evidence. Reusable findings are
distilled into the `DESIGN-*` contracts above, the language reference, agent
skills, executable examples, and regression tests. Git and pull-request history
retain the chronology without making investigation logs a parallel source of
truth.

Worked examples are in [`../specs/`](../specs/) (standalone specs) and [`../examples/`](../examples/)
(bank: implementation conformance / layers: three-layer chain / nfr: SLA).
The AI-agent skills are under [`../skills/`](../skills/), split into shared FSL
syntax plus business, requirements, design, and design-review entry points.
