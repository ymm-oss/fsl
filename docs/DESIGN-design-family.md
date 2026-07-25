# FSL design-family Phase 0 decision

Status: Accepted evidence-gated spike decision for issue #427. This document does
not commit a language construct, Public Kernel node, or product CLI command.

## Decision

Keep a design family as a versioned sidecar workflow over existing native
`fslc` producers. Do not add `design_family` grammar, union variant state spaces,
or add `fslc family` in Phase 0.

Gate A (catalog and formal eligibility) and Gate B (directed pair comparison)
are **go as an agent/workflow convention**. They are **no-go as a native language
or CLI feature without new usage evidence**. The durable Phase 0 evidence is:

- the closed `fsl-design-family.v0` manifest schema;
- three maintained family fixtures with three independent variants each;
- a native integration-test harness that invokes the existing `fslc` process
  contract and retains raw JSON, stderr, argv, and real exit codes;
- positive paths and rejecting controls for refinement, dependency drift,
  comparison orientation, bounded completeness, and nested `implements`;
- this accepted decision and its explicit gaps.

The harness is intentionally test-owned. It establishes that orchestration is
possible without widening the product surface. A later independent command or
thin product CLI requires a separate issue with observed repeated use, an exact
transitive-input contract, and a semantic-digest API that covers every admitted
variant dialect.

Gate C (AI implementation experiments and leading indicators) and Gate D (causal
bridge) remain outside this decision. Issue #446 owns the quantitative/decision
boundary. A family report never contains ranking, recommendation, selection,
causal support, or an AI-run scheduler.

## Boundary

```text
common contract
  -> independent checked variants
  -> variant -> contract refinement
  -> directed OLD/NEW semantic diff
  -> stable summary + unmodified producer evidence
```

A family is orchestration metadata. It has no state, action, transition, or
property and is never lowered to the Kernel. Every contract and variant remains
an independent checked model. The harness never generates a correspondence from
names or reverses a declared mapping.

The Phase 0 prototype lives at:

- schema: `schemas/fslc/design-family/design-family.v0.schema.json`
- fixtures: `rust/fslc/tests/fixtures/design_family/`
- harness: `rust/fslc/tests/issue_427_design_family.rs`

The fixtures are artificial verification models, not asserted production-domain
rules. Their `MODEL-*` comments keep that boundary visible.

## Manifest v0

The closed JSON manifest has these required fields:

- `schema_version`: exactly `fsl-design-family.v0`;
- `family_id`: stable family identity;
- `contract`: source, checked symbol, and exact declared inputs;
- `verification`: Phase 0 fixes the engine to `bmc` plus an inclusive depth;
- `variants`: stable ID, independent source/symbol, refinement mapping, and
  exact declared inputs;
- `comparisons`: stable ID, ordered OLD/NEW IDs, NEW-owned scope, depth, exact
  mapping, and `new_to_old` mapping direction;
- `bundle_control`: a checked compose entry, imported dependency, and exact
  input list used for the dependency-drift negative control.

Unknown fields fail schema validation. Variant and comparison IDs must be
unique; comparison endpoints must resolve to listed variants. The schema permits
only repository-relative paths. Runtime ownership of the manifest is external
to `fslc`; this v0 schema is a spike artifact, not a product compatibility
promise.

## Orchestration protocol

For each family the prototype performs the following operations and does not
omit failed rows:

1. Validate the closed manifest and all ID references.
2. Run `fslc document claims` for the contract and every variant. Check the
   declared symbol and retain `fsl-kernel-ast-v1+sha256` semantic identity.
3. Run native `check` and configured `verify` for the contract and every variant.
4. Treat `implements.result != refines` as failure even when the top-level
   process exits zero.
5. Run every declared `fslc refine VARIANT CONTRACT MAPPING --depth K`; check
   that the producer names the same abstract contract and retain
   `checked_to_depth`.
6. Require at least two distinct variant semantic digests. A duplicate digest
   is reported as a deterministic warning naming the digest and variant IDs,
   but does not count as another semantic candidate.
7. Probe each comparison mapping as `NEW -> OLD`, require its checked impl/abs
   symbols to match the declared endpoints, then run directed
   `fslc diff OLD NEW --depth K --mapping M` only after the
   prerequisite forbidden-replay completeness fix (#460). Retain OLD, NEW,
   scope owner, mapping direction, depth, bounded completeness, summary, and
   findings.
8. Check the compose import probe and hash the manifest-declared source bundle.
9. Emit a stable summary projection and keep every raw producer row separately.

Semantic diff findings remain informative unless an explicit gate is configured
by a future contract. `no_semantic_change` is never rendered as equivalence;
every comparison retains `completeness: bounded` and depth.

## Report and exit contract sketch

The stable report projection is `fsl-design-family-report.v0` and contains:

- family result plus independent Gate A catalog/eligibility and Gate B pair
  comparison statuses;
- contract and variant IDs, symbols, sources, semantic digests, check results,
  and per-model verification verdict/assurance/completeness, requested engine,
  producer-reported engine when present, and depth;
- per-variant refinement result and depth;
- ordered comparison identity, scope owner, mapping identity/direction, depth,
  producer result/summary, and bounded completeness;
- source-bundle and exact mapping digests;
- producer component versions;
- a canonical report digest over this stable projection.

Raw evidence is a separate array of rows containing phase, subject, argv, actual
process exit code, exact stdout bytes, parsed stdout JSON, and exact stderr bytes.
The stable report digest deliberately excludes raw solver cost, cache metadata,
and wall-clock-sensitive evidence. Raw evidence is not normalized or rewritten.
This separation is necessary: byte-deterministic summary identity and unmodified
producer output cannot be the same payload.

Aggregation preserves this precedence:

| Class | Family exit |
|---|---:|
| Internal producer failure | 3 |
| Manifest, I/O, parse, type, reference, or producer-contract failure | 2 |
| Verification, refinement, or explicitly gated comparison failure | 1 |
| Eligible family or informative diff finding | 0 |

The prototype exercises the precedence function directly and runs independent
rows even after a sibling fails. A deliberately invalid variant refinement
produces a family exit 1 while all three variant and refinement rows, all raw
producer exits, and Gate B remain present. A future command that adds dependency-
ordered short-circuiting must use explicit `blocked`, `skipped`, and `not_run`
rows instead of deleting work that could not run.

Manifest read, parse, schema, and reference failures return a deterministic
exit-2 failed report rather than aborting the harness. Producer rows are checked
for a JSON envelope before verdict classification; non-JSON output and process
statuses outside 0 through 3 fail closed as internal exit 3. Negative controls
exercise both boundaries. Phase-specific required fields and the process
exit/verdict pair are also checked, so a typed but contradictory producer row is
a producer-contract exit 2 rather than evidence of eligibility.

## Digest and provenance result

| Identity | Phase 0 source | Result / gap |
|---|---|---|
| Semantic digest | `fslc document claims` → `fsl-kernel-ast-v1+sha256` | Reused without inventing another digest. Public only for the document-supported `spec`/`requirements` surface; not a general all-dialect family API. |
| Source bundle digest | SHA-256 over path/length-framed manifest-declared exact bytes | Deterministic and dependency-drift-sensitive. The repository has no public exact transitive-input enumerator, so completeness depends on the closed manifest list. |
| Mapping digest | SHA-256 over exact mapping bytes | Sufficient for exact mapping identity; comments and formatting intentionally change it. |
| Producer | Native check/verify/refine/diff raw JSON + real exit and versions | Complete for executed commands; no family producer exists. |
| Report digest | Canonical JSON of the stable projection | Stable by construction and separate from raw evidence noise. |

The source-bundle limitation is material. A maintained user-facing command must
either obtain the exact resolver dependency closure or fail closed on a clean,
exact Git tree identity. It must not silently call a directory-wide verifier
cache key a source-bundle digest.

## Existing `chain` reuse decision

`fslc chain` supplies useful behavior knowledge but is not a suitable internal
implementation dependency for the spike.

| Concern | Reuse decision |
|---|---|
| Native check/verify/refine JSON and process contract | Reuse through child processes. |
| Nested `implements.result` failure rule | Reuse the rule and negative control. |
| Result/assurance vocabulary | Reuse without collapsing verdict and method. |
| Chain manifest parser | Do not reuse: it is private, fixed to business/requirements/design/impl, and not a typed variant catalog. |
| Chain runner | Do not reuse: it fixes BMC in one path, does not represent directed pairs, and normalizes internal exit 3 to 2. |
| Chain summary | Do not reuse: it cannot preserve family identities, digests, comparison scope, or raw producer provenance. |

This preserves the product boundary and avoids extracting a generic framework
from one fixed layer pipeline before another production owner exists.

## Dogfood evidence and rejecting controls

The maintained families are:

1. order processing: synchronous, event-driven, and saga;
2. concurrent work ownership: shared lock, actor mailbox, and partitioned queue;
3. persistence: CRUD, event log, and snapshot plus log.

Each contract and variant checks and verifies at the declared bound; every
variant refines the same contract. Each family also has a deliberately invalid
mapping that must return `refinement_failed`. The shared controls establish:

- changing only a checked imported dependency changes the source-bundle digest;
- depth-1 comparison remains explicitly bounded and is never called equivalent;
- reversing OLD/NEW changes the directed result while retaining both identities;
- a top-level successful `check` with nested `implements.result:
  refinement_failed` maps to a gated exit 1 rather than family success;
- a failed variant refinement yields a failed Gate A/family report without
  omitting other variants, comparison results, or raw evidence;
- an unknown manifest field and an unknown comparison endpoint fail closed;
- raw stdout and real exit codes exist for every executed producer;
- the stable report digest recomputes exactly.

## Leading-indicator handoff gap

The family report may hand the following identities to a separate experiment
protocol:

- family and variant IDs;
- exact declared source-bundle, semantic, and mapping digests;
- contract and formal eligibility results;
- comparison orientation, scope, depth, and bounded/unknown evidence;
- producer versions and raw evidence.

It deliberately does not contain the following experiment facts:

- implementation repository/tree and build artifacts;
- model, prompt, tools, budget, execution order, random seed, or scheduler;
- task corpus, repetitions, exclusion rules, or source-lineage independence;
- test harness/conformance artifact identity;
- leading-indicator definitions, units, windows, thresholds, uncertainty, or
  pre-registration;
- lagging KPI observations, causal support, utility/loss, or human decision.

Those belong to the versioned protocol/decision boundary in #446. Their absence
must yield `not_run` or `inconclusive`, never a ranking inferred from formal
eligibility.

## Option comparison

| Option | Evidence quality | Product cost | Decision |
|---|---|---|---|
| Folder/agent convention only | Weak identity and omission gates | Low | Reject as the sole mechanism. |
| Closed sidecar + test-owned native harness | Deterministic, rejecting, no product contract | Low | Accept for Phase 0 and agent/workflow use. |
| Maintained independent command | Usable outside tests, but creates distribution and compatibility obligations | Medium | Defer until repeated use and dependency-closure gaps are resolved. |
| Thin `fslc family` command | Best discoverability, permanent CLI/schema obligations | High | No-go in Phase 0. |
| `design_family` language/Kernel feature | Would incorrectly pull orchestration into model semantics | Highest | No-go. |

The existing `fsl-design-review` procedure already requires a variant matrix
against a frozen abstract contract. No new agent-skill clause is accepted by
this spike: there is no measured residual/ablation evidence showing that another
skill instruction improves over that procedure. The closed schema, executable
negative controls, and this design are the durable output specification.

## Reevaluation triggers

Open a separate implementation issue only when at least one trigger has concrete
evidence:

- two or more maintained consumers need the same report outside tests;
- agents repeatedly omit invalid variants or lose raw exits despite the closed
  schema and existing review procedure;
- a public resolver API can return an exact transitive source closure;
- semantic identity is required for dialects not supported by `document claims`;
- users require a stable stored report rather than PR/CI evidence.

Any future proposal must retain all negative controls here and compare an
independent command against a thin CLI. It must not use successful dogfood alone
as evidence for a language feature.
