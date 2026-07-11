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
| [`intro/analysis.ja.html`](intro/analysis.ja.html) / [`intro/analysis.en.html`](intro/analysis.en.html) / [`GUIDE-analyze.ja.md`](GUIDE-analyze.ja.md) / [`DESIGN-analysis.md`](DESIGN-analysis.md) | **`fslc analyze` structural observation layer**. Site pages, a team-facing Japanese practical guide, and implementation design for TSG, graph projections, focused impact slices, action dependency/conflict graphs, structural metrics, batch analysis, refinement/project traceability graphs, graph exports, schemas, and AI-review findings/candidates |
| [`intro/domain.ja.html`](intro/domain.ja.html) / [`intro/domain.en.html`](intro/domain.en.html) / [`DESIGN-domain.md`](DESIGN-domain.md) / [`DESIGN-effect.md`](DESIGN-effect.md) | **fsl-domain Functional DDD / async effect dialect**. Site pages and implementation design for `domain`, aggregate command/event ownership, pure decide/evolve lowering, saga/process-manager coordination, async effect lifecycle checks, multi-target scaffolds, runtime replay, and domain-specific findings |
| [`intro/db.ja.html`](intro/db.ja.html) / [`intro/db.en.html`](intro/db.en.html) / [`DESIGN-db.md`](DESIGN-db.md) | **fsl-db DB / multi-environment compatibility dialect**. Site pages and implementation design for `dbsystem`, schema/artifact/environment compatibility, rollout windows, and DB-specific findings |
| [`intro/ai.ja.html`](intro/ai.ja.html) / [`intro/ai.en.html`](intro/ai.en.html) / [`DESIGN-ai-hard.md`](DESIGN-ai-hard.md) | **fsl-ai hard-contract and agent-structure dialect**. Site pages and implementation design for `ai_component`, recursive `agent` composition, tool authority, human approval guards, runtime replay, and AI-specific findings |
| [`DESIGN-stochastic.md`](DESIGN-stochastic.md) | **fsl-stochastic external evidence layer**. Implemented semantics for precomputed eval JSONL, Bernoulli/proportion metrics, Wilson intervals, result statuses, and the boundary between statistical support and formal proof |
| [`DESIGN-v1.md`](DESIGN-v1.md) | Language design document (design principles G1-G5, type-system design decisions, the repair protocol, and the roadmap) |

## Implementation design by architecture and feature (DESIGN-*)

| Document | Subject |
|---|---|
| [`DESIGN-layers.md`](DESIGN-layers.md) | **Shared kernel + three dialects** (consulting / requirements / design): overall concept and validation |
| [`DESIGN-dialects.md`](DESIGN-dialects.md) | Implementation spec for the dialects (declaration tags, fsl-req, fsl-biz) |
| [`DESIGN-nfr.md`](DESIGN-nfr.md) | Non-functional requirements (mapping table, discrete-time SLA: time/urgent/age/deadline) |
| [`DESIGN-induction.md`](DESIGN-induction.md) | The k-induction engine (proved / unknown_cti / CTI) |
| [`DESIGN-from-state.md`](DESIGN-from-state.md) | Predictive BMC from a complete Monitor/replay logical-state snapshot (`verify --from-state`), including type validation, faithfulness metadata, cache/symmetry boundaries, and induction exclusion |
| [`DESIGN-trans.md`](DESIGN-trans.md) | `trans` (transition invariant / two-state safety) |
| [`DESIGN-temporal.md`](DESIGN-temporal.md) | leadsTo, weak fairness (lasso counterexamples), and respond scenarios |
| [`DESIGN-refinement.md`](DESIGN-refinement.md) | Refinement checking (mapping files, conditional expressions, preserve progress) |
| [`DESIGN-semantic-diff.md`](DESIGN-semantic-diff.md) | `fslc diff` bounded semantic comparison (bidirectional refinement, invariant implication, forbidden replay, scope and gate contract) |
| [`DESIGN-compose.md`](DESIGN-compose.md) | Spec composition (namespaces, synchronized actions, internal) |
| [`DESIGN-bridge.md`](DESIGN-bridge.md) | Implementation bridge (runtime Monitor / replay / testgen) |
| [`DESIGN-log-replay.md`](DESIGN-log-replay.md) | Production JSONL replay through refinement mapping syntax: record contract, complete-observation boundary, first-divergence JSON, and Monitor execution |
| [`DESIGN-scenarios.md`](DESIGN-scenarios.md) | scenarios and the unsat-core diagnostics for coverage |
| [`DESIGN-seq.md`](DESIGN-seq.md) | Seq<T,N> (partial_op, type whitelist) |
| [`DESIGN-option-struct.md`](DESIGN-option-struct.md) | Option fields in structs |
| [`DESIGN-divmod.md`](DESIGN-divmod.md) | Integer division `/` and remainder `%` (total definition of division by zero, partial_op, Euclidean) |
| [`DESIGN-forbidden.md`](DESIGN-forbidden.md) | `forbidden` (negative acceptance criteria / must-forbid) — detecting under-constraint |
| [`DESIGN-vacuity.md`](DESIGN-vacuity.md) | Vacuity checking (invariants whose antecedent is unreachable, leadsTo whose trigger is unreachable, always-true requires) |
| [`DESIGN-strict-tags.md`](DESIGN-strict-tags.md) | The `--strict-tags` lint (matching untagged declarations and unreferenced requirements) |
| [`DESIGN-init-if.md`](DESIGN-init-if.md) | Statement-level `if` in `init` (lowered to path-conditional initial-state constraints, same branch shape as action bodies) |
| [`DESIGN-inline-range.md`](DESIGN-inline-range.md) | Inline anonymous range types (`x: lo..hi`) |
| [`DESIGN-spec-domains.md`](DESIGN-spec-domains.md) | `entity` / `number` in the kernel `spec` (decoupling a domain from the verification bound) |
| [`DESIGN-precedence-policy.md`](DESIGN-precedence-policy.md) | The business-layer no-bypass precedence policy (#75) — why `business` keeps users from writing `state`/`invariant` directly |
| [`DESIGN-ledger.md`](DESIGN-ledger.md) | `fslc ledger` (turning verifier evidence into a per-requirement-id Markdown audit ledger for PM/audit) |
| [`DESIGN-assurance-classes.md`](DESIGN-assurance-classes.md) | Assurance-class vocabulary (`proved`/`bounded`/`replay-observed`/`statistical`/`not_run`) shared by `fslc ledger` and `fslc html`, and what each class does/does not guarantee |
| [`DESIGN-mutate.md`](DESIGN-mutate.md) | `fslc mutate` (spec mutation, requirement stress report) |
| [`DESIGN-explain.md`](DESIGN-explain.md) | `fslc explain --readable` (verification bounds, skeleton enumeration, counterfactuals, witness narration) |
| [`DESIGN-html-report.md`](DESIGN-html-report.md) | `fslc html` (self-contained visual review report from explain + verify evidence) |
| [`DESIGN-typestate.md`](DESIGN-typestate.md) | `fslc typestate` (applicability check for state machine → typestate + TS scaffold) |
| [`DESIGN-blame-assignment.md`](DESIGN-blame-assignment.md) | Counterexample blame assignment (`fslc verify`/`fslc explain`): false-conjunct identification, per-step guard/effect backward slicing, and vacuity blocking-core localization |
| [`DESIGN-incremental-verify.md`](DESIGN-incremental-verify.md) | `fslc verify`'s persistent verdict cache (`src/fslc/verify_cache.py`): exhaustive cache-key enumeration, cross-depth counterexample reuse, and the soundness argument for why a cached verdict can never be stale |
| [`DESIGN-conformance-harness.md`](DESIGN-conformance-harness.md) | Dialect corpus conformance harness (`tests/test_dialect_conformance.py`, `tests/dialect_registry.py`): the Monitor/BMC-agreement/oracle safety net as a CI gate over every `.fsl` under `specs/`/`examples/`, with a loud, reviewable exclusion policy |
| [`DESIGN-coupled-change-metatest.md`](DESIGN-coupled-change-metatest.md) | Coupled-change metatests (`tests/test_coupled_change_meta.py`): LSP index coverage (grammar production ↔ visitor handler, corpus-driven, two-stage) and DESIGN-doc coverage (dialect/CLI command ↔ `docs/DESIGN-*.md`) |
| [`DESIGN-analysis.md`](DESIGN-analysis.md) | `fslc analyze` (Typed Semantic Graph, graph projections, focus impact slices, action dependency/conflict graphs, structural metrics, batch mode, refinement/project traceability graphs, DOT/Mermaid exports, schemas, AI-readable structural review findings/candidates) |
| [`DESIGN-ui.md`](DESIGN-ui.md) | fsl-ui (screen-transition dialect): spike findings, proposed expansion rules, go/no-go (#9) |
| [`DESIGN-domain.md`](DESIGN-domain.md) | fsl-domain (`domain`) Functional DDD / async effect dialect: aggregate ownership, command/event decide/evolve lowering, saga/process-manager actions, effect lifecycle state, findings, multi-target scaffolds, and runtime replay |
| [`DESIGN-effect.md`](DESIGN-effect.md) | fsl-effect lifecycle semantics used by fsl-domain: correlation, retry, timeout, idempotency, and guarantee boundary |
| [`DESIGN-db.md`](DESIGN-db.md) | fsl-db (`dbsystem`) database compatibility dialect: multi-environment schema/artifact/feature-flag checks, finding schema, rollout assumptions, SQL/Prisma importers, and external preservation/engine evidence boundaries |
| [`DESIGN-ai-hard.md`](DESIGN-ai-hard.md) | fsl-ai (`ai_component` / recursive `agent`) dialect: tool authority, human approval, forbidden tools, fallback, event replay, agent scope/grant/orchestration/visibility analysis, finding schema, and guarantee boundaries |
| [`DESIGN-stochastic.md`](DESIGN-stochastic.md) | fsl-stochastic external evidence layer: precomputed eval JSONL, Wilson-bound threshold rules, statistical result schema, status priority, multiple-slice boundary, and external stochastic boundaries |
| [`DESIGN-docs-site.md`](DESIGN-docs-site.md) | This manual site's information architecture, navigation chrome, and the generated-reference-page template (`intro/language.*.html`, `intro/cli.*.html`) — produced with the Relational Design plugin |

## Dogfooding records (DOGFOOD-*)

Findings, bugs, and discoveries from putting each feature into real use. These form the basis of the design decisions.

1. [`DOGFOOD-1.md`](DOGFOOD-1.md) — v1.0 field evaluation (found BUG11-14, PERF1)
2. [`DOGFOOD-2.md`](DOGFOOD-2.md) — proved as standard practice, Seq (discovery of the aggregation idiom)
3. [`DOGFOOD-3.md`](DOGFOOD-3.md) — full workflow (abstract → refine → compose → implementation)
4. [`DOGFOOD-4.md`](DOGFOOD-4.md) — penetration of the three-layer dialects (cross-layer diagnostics by requirement ID)
5. [`DOGFOOD-5.md`](DOGFOOD-5.md) — NFR / discrete-time SLA
6. [`DOGFOOD-6.md`](DOGFOOD-6.md) — bug hunt in the example gallery (two refine misses)
7. [`DOGFOOD-7.md`](DOGFOOD-7.md) — golden-oracle test suite (Monitor BFS, trace soundness, BUG-020)
8. [`DOGFOOD-8.md`](DOGFOOD-8.md) — blind expressibility test (external validation of G1)
9. [`DOGFOOD-9.md`](DOGFOOD-9.md) — real run of the validation workflow (memo → positive-example pair → repair)
10. [`DOGFOOD-10.md`](DOGFOOD-10.md) — fault-injection benchmark (measuring detector capture rate by category × mechanism)
11. [`DOGFOOD-11.md`](DOGFOOD-11.md) — meta-circular dogfooding (modeling fslc's own design contract in FSL; detector blind spots F22-F24)

Worked examples are in [`../specs/`](../specs/) (standalone specs) and [`../examples/`](../examples/)
(bank: implementation conformance / layers: three-layer chain / nfr: SLA).
The AI-agent skills are under [`../skills/`](../skills/), split into shared FSL
syntax plus business, requirements, design, and design-review entry points.
