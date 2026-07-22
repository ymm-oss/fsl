# Normalized public Kernel contract and conformance corpus

Status: implemented by issue #208. The native Rust implementation is the
authority; the frozen Python reference is intentionally unchanged.

## Goal and boundary

External compilers, generators, and analyzers consume a versioned JSON contract
after FSL parsing, dialect lowering, name resolution, and semantic checking. They
must not parse FSL source, depend on Rust syntax-tree internals, or consume the
historical Python-shaped AST JSON.

The boundary is:

```text
FSL source / dialect
  -> Rust parse and lowering
  -> checked KernelModel
  -> public Kernel JSON v1
  -> external compiler or generator
```

`fsl_core::public_kernel_contract` is the Rust API. `fslc kernel SPEC` is the
language-neutral process API. The output's `$schema` resolves to
`schemas/fslc/kernel/kernel.v1.schema.json`; `schema_version` is `1.0.0`.

## Contract shape

The top-level contract contains:

| Field | Contract |
|---|---|
| `spec` | Logical name plus source file and original dialect. |
| `semantics` | Simultaneous updates, pre-state reads, disabled requires, rollback failures, `old`, Euclidean integer division, terminal handling, and weak fairness. |
| `constants`, `types`, `state` | Fully normalized declarations with structural types and lowering origin. |
| `init` | Typed normalized statements plus requirement/origin metadata. Kernel state inline initializers appear only as equivalent root assignments. |
| `actions` | Finite typed parameters, ordered `guards` (requires/let binding scope), compatibility projections in `requires`/`lets`, simultaneous updates, ensures, partial-operation conditions, requirement IDs, origin, and span. |
| `properties` | Invariants, transition properties, reachability, progress, and terminal expression. |

Every expression has a `kind`, structural `type`, and source `span`; consumers do
not reparse expressions or infer types. Statement and declaration origins state
the source dialect, originating declaration or requirement ID, whether lowering
occurred, and whether the node was generated. Action/property/statement spans
include start and end positions. The internal surface tree retains inline state
field and initializer spans for diagnostics and migration, but Public Kernel v1
does not publish the sugar as a second declaration form. Direct scalar/type/state
declaration spans in the v1 projection remain explicitly `null`; normalized
initializer statements carry their source span. This is an intentional v1
distinction, not a missing field.

That v1 `origin` object is a frozen, lossy compatibility projection. The Rust
frontend's authoritative internal provenance is the non-serialized origin chain in
[`DESIGN-origin-chain.md`](DESIGN-origin-chain.md), where declaration/source
origin and requirement traceability are separate types. The historical v1
projection may still place a requirement tag in `origin.declaration` to remain
byte-compatible; internal consumers must not infer source identity from it.
The internal chain is absent from this closed schema and will be published only
by Public Kernel v2 (#256).

Unsupported or incompletely lowered structures fail export with a semantic error.
The exporter never silently omits an unknown type, predicate call, expression,
lvalue, or non-scalar parameter domain.

The Rust domain frontend reaches this boundary through typed name/type
resolution and direct AST-to-AST lowering. Generated Kernel source from
`fslc domain expand` is a debug representation only and is never reparsed to
construct the checked contract. Resolver diagnostics therefore retain the
original domain node coordinates.

Compose lowering currently loses the component filename while retaining its line
span. Public Kernel v1 therefore rejects `compose` input explicitly instead of
fabricating a root-file source location. A future schema version may add
multi-source provenance once it is retained by the checked Kernel.

## Transition and failure semantics

Action right-hand sides read the same pre-state and updates commit
simultaneously. `old(expr)` also reads the pre-state. A false `requires` clause
means the action instance is not enabled.

Partial operations (`head`, `pop`, `at`, sequence indexing, division, and
remainder) are listed with a typed failure condition. A partial operation,
invariant violation, transition-property violation, or failed `ensures` rolls
back the whole step. The Monitor returns the input state and leaves its internal
state unchanged. For diagnostics it separately retains `attempted_state`, the
uncommitted candidate that violated an invariant, transition property, or
ensure. This rollback rule is shared by the schema and executable vectors.

## Conformance corpus

`fslc conformance SPEC --depth N` enumerates every finite action instance at
each bounded reachable state and emits:

- the stable logical states (`s0`, `s1`, ...);
- successful successor states and field-level changes;
- disabled instances as `requires_failed`; and
- runtime/property failures with `state_changed:false` and the unchanged state.

The schema is `schemas/fslc/kernel/conformance.v1.schema.json`. The checked-in
input/golden pair
`rust/fslc/tests/fixtures/conformance_failures.{fsl,v1.json}` fixes v1 failure
semantics. `kernel_contract.fsl` covers domains, enums, structs, maps, sets,
sequences, options, quantification, simultaneous updates, requirements,
properties, partial operations, and terminal states. All ordering is
deterministic.

Conformance values preserve nested option identity with tagged
`{"kind":"none"}` and `{"kind":"some","value":...}` objects. This keeps
`none` distinct from `some(none)` during reachable-state deduplication. Other
scalar and collection values retain their ordinary JSON representation.

### Deterministic testgen trace

Target harness generation has a different operational contract from exhaustive
bounded exploration: it must retain the historical fixed-seed, at-most-100-step
walk without expanding the reachable state space or changing generated bytes.
`fslc_rust::testgen_trace_vectors` publishes that single path as
`schemas/fslc/kernel/testgen-trace.v1.schema.json`. The envelope records its
schema version, the Public Kernel version, and spec identity. State values retain
ordinary Monitor JSON because they are embedded directly into generated
harnesses; this contract is therefore separate from the exhaustive corpus's
tagged Option representation.

The shared testgen adapter consumes Public Kernel v1 and this versioned trace,
then cross-checks full state fields, action names, and exact parameter sets.
Scenario expected states may be partial but cannot name fields outside the
Kernel contract. An incompatible or malformed trace fails before target
rendering. This resolves the Public Kernel/conformance-vector boundary required
by issue #214 without weakening the exhaustive corpus or changing legacy output.

## Conformance coverage matrix

Status: implemented by issue #223. The fixed fixture manifest
(`kernel_contract.fsl` at depth 2, `conformance_failures.fsl` at depth 1) is
comprehensive by construction, not by inspection: an external implementer
reading only the golden vectors has no mechanical way to tell which
documented kernel semantic, outcome kind, value encoding, or structural
feature the corpus actually demonstrates versus merely declares. The
coverage matrix answers that question directly.

`fslc_rust::coverage::coverage_matrix()` parses both fixtures, builds their
public Kernel contracts and conformance vectors in memory, and structurally
detects, for a fixed list of feature rows, whether each is:

- **exercised** — a concrete vector or state snapshot in the generated
  corpus demonstrates the feature (e.g. an `outcome.kind == "trans"` vector,
  a state snapshot encoding a `Map` value, a literal negative-operand
  division whose recorded result matches Euclidean, not truncating,
  division); or
- **declared** — the contract states the feature exists (a
  `partial_operations` entry, `fair: true`, a `terminal` expression) but no
  vector in the fixed fixture manifest can fire it. This level is reserved
  for features that are structurally unobservable in a single bounded
  transition vector — `terminal_deadlock` and `fairness_weak` are liveness/
  whole-trace properties with no finite-vector witness — and for the six
  `partial_op_*` rows, where a `partial_operations` declaration is the
  required bar and a firing vector is additional, non-required evidence.

No row is ever hardcoded to a fixed level: every detector inspects the
generated JSON (expression trees, `outcome.kind`, state snapshots) directly.

Collection/range aggregates are source and private checked-IR features, not a
new Public Kernel v1/v2 representation. Before export, finite `count`/`sum`
over Set, Seq, or range binders is normalized to existing `ite`, arithmetic,
membership, `.size()`, and `.at()` expression nodes. Seq expansion includes the
structural `index < size` guard for every capacity slot, so inactive slots are
not counted and partial-operation evidence remains guarded. Existing typed
aggregate JSON keeps its exact `{binding, domain, condition}` shape (`sum`
without a filter retains `condition: null`). Adding `{binder: ...}` to the
published `count`/`sum` kinds would be a representation change and therefore
requires a future Public Kernel major version.
`coverage_matrix()` is itself the enforcement gate: if any feature row falls
short of its required level, it returns `Err` naming every shortfall instead
of producing a matrix that silently under-reports coverage. This is how
issue #223's "warn about newly uncovered features" requirement is
implemented — as a loud test failure, matching this repository's existing
snapshot-test discipline, rather than a warning that can be ignored.

The feature rows cover: the eight `semantics` keys in
`kernel.v1.schema.json`; the seven `outcome.kind` values
`fsl_runtime::Monitor` can emit; eight value-encoding rows (int range, bool,
enum, struct, option, map, set, seq — a nested `Option<Option<_>>` tagged
`some(none)` is additional, non-required evidence for the `value_option` row,
since the fixed fixture manifest does not carry one; that shape is instead
covered by the dedicated `conformance_distinguishes_nested_options_and_guard_partials`
unit test); the six partial operations (`head`, `pop`, `at`, `index`, `divide`,
`remainder`); and `quantification` (`forall`/`exists`), `param_finite_domains`,
and `requirement_traceability`.

Outputs:

- `schemas/fslc/kernel/conformance-coverage.v1.schema.json` — the JSON Schema
  for the matrix, following the same draft 2020-12 / closed-object
  conventions as the Kernel and conformance schemas.
- `rust/fslc/tests/fixtures/conformance_coverage.v1.json` — the golden
  matrix.
- `rust/fslc/tests/fixtures/conformance_coverage.v1.md` — the same data
  rendered as a Markdown feature-by-fixture table
  (`fslc_rust::coverage::coverage_matrix_markdown`), for humans skimming
  which fixture backs which feature.

Regenerate both after an intended change:

```bash
FSLC_COVERAGE_UPDATE=1 cargo test -p fslc-rust --test conformance_coverage
```

`rust/fslc/tests/conformance_coverage.rs` enforces the coupled-change
discipline this matrix depends on:

- every `kernel.v1.schema.json` `semantics` key has a matching feature row
  (`semantics_schema_keys_are_all_registered_as_feature_rows`);
- every `outcome.kind` the corpus emits — independently regenerated from the
  fixtures, not read back from the matrix — is registered and exercised
  (`every_outcome_kind_the_corpus_emits_is_registered_and_exercised`);
- every feature row meets its required level
  (`every_feature_row_meets_its_required_coverage_level`, via
  `coverage_matrix()`'s own `Err`);
- the golden JSON/Markdown match (`coverage_matrix_matches_the_v1_golden_json_and_markdown`);
- the schema's `$id` matches `fslc_rust::coverage::COVERAGE_SCHEMA_ID`
  (`published_coverage_schema_id_matches_the_rust_api_constant`).

**Coupled-change rule**: adding a new `semantics` key, a new
`fsl_runtime::Monitor` violation kind, a new partial operation, or a new
value kind to the kernel/conformance contract requires adding a matching
feature row (with a real, non-hardcoded detector) to
`rust/fslc/src/coverage.rs`, adding fixture evidence for it if the required
level is `exercised`, and regenerating the golden matrix above — in the same
change, the same way a kernel-contract shape change requires regenerating
`kernel_contract.v1.json`.

Concrete/symbolic agreement is a separate mechanical gate:
`fsl_verifier::transition_matches_step` pins a Monitor pre-state, action
parameters, and successor into the symbolic transition relation. Tests require
every checked successful Monitor transition to be satisfiable and a deliberately
altered successor to be rejected. `transition_outcome_matches_step` additionally
checks that disabled actions have an actually false guard and distinguishes
representable successful, type-bound, invariant, transition, and ensures
outcomes in native evaluation order. It rejects malformed calls and state
shapes, corrupted attempted/committed states, and substituted outcome kinds.
Because the symbolic value evaluator totalizes partial operators and uses
unbounded integer terms, agreement uses a separate typed definedness predicate
that follows native short-circuit, statement-branch, guard, post-property, and
checked-i64 evaluation order. A non-partial claim is rejected when its reached
path is undefined; an untaken partial expression does not make a valid outcome
unsupported. Agreement-only fallback values let zero-capacity sequence terms
be constructed under those predicates; ordinary BMC and induction retain their
existing fail-closed evaluation. Exact `partial_op` classification and concrete
evidence that the bounded symbolic representation cannot retain, such as an
over-capacity sequence suffix, return an error rather than green until an exact
partial-evidence oracle is available. Concrete conformance vectors remain
authoritative for `partial_op` rollback meanwhile.
The solver-free `fsl-runtime` dependency boundary remains intact.

## CLI and API

```bash
fslc kernel spec.fsl > kernel.json
fslc conformance spec.fsl --depth 4 > vectors.json
```

Both commands return one JSON document and use the normal native exit contract:
0 success, 2 parse/type/semantic/I/O error. `kernel` accepts source dialects that
reach the shared lowering gate and retain truthful source provenance; `compose`
is the explicit v1 exception described above. The output records the original
dialect and requirement origin.

Rust callers can use:

- `fsl_core::public_kernel_contract(&KernelSpec, &KernelModel, path, dialect)`;
- `fsl_core::public_kernel_contract_for_version(..., PublicKernelVersion)` for
  explicit v1/v2 negotiation (the legacy entrypoint remains v1);
- `fslc_rust::conformance_vectors(&KernelModel, depth)`;
- `fslc_rust::testgen_trace_vectors(&KernelModel)` for the versioned,
  fixed-seed single-path harness contract;
- `fslc_rust::coverage::coverage_matrix()` and `coverage_matrix_markdown(&Value)`
  for the fixture corpus's feature coverage matrix (no CLI subcommand in v1;
  see "Conformance coverage matrix" above); and
- `fsl_verifier::transition_matches_step(...)` and
  `transition_outcome_matches_step(...)` for semantic agreement.

## Versioning and compatibility

The public Kernel schema follows SemVer independently of the FSL language and
CLI versions:

- patch: documentation/schema clarification that accepts exactly the same JSON;
- minor: additive optional fields only at schema locations explicitly declared
  extensible, or new explicitly extensible `kind` values;
- major: removal/rename, required-field addition, representation change, or any
  semantic change to evaluation, updates, failures, fairness, or properties.

Consumers must inspect `schema_version`, accept supported `1.x` schemas, and
reject an unsupported major. Objects marked `additionalProperties:false` are
closed in v1; adding fields there is a major change. Existing fields keep
their meaning for the whole major line. Deprecations remain emitted for at least
two minor releases and are documented before removal in the next major.
Conformance vectors, testgen traces, and public replay traces have their own
versions and record the Kernel schema version they exercise. Replay-trace v1 is
shared by Kernel v1/v2 because v2 adds provenance without changing execution
values; its complete tick/state contract is defined by
[`DESIGN-replay-trace.md`](DESIGN-replay-trace.md). A schema change requires coupled updates to the schemas,
golden vectors, Rust tests, this document, `docs/LANGUAGE.md`, the shared skill,
and `CHANGELOG.md`.

Replay observation actions execute through `Monitor::attempt`. Solver-free BFS
also uses Monitor transitions, while
`fsl-verifier/src/agreement.rs::{transition_matches_step,transition_outcome_matches_step}`
checks Monitor successors and failures against symbolic transition semantics.
Stutter performs no transition and requires full equality with the current
Monitor state, so it introduces no second runtime or solver semantics.
Replay schema 1.2 additionally runs the solver-free `BoundedLivenessMonitor` on
the same observations. Its inclusive `leadsTo ... within` deadline is
differentially checked against the native BMC deadline probe; requirements
`deadline` remains an ordinary lowered safety invariant.

Public Kernel v2 is the separately negotiated provenance publication contract
defined by [`DESIGN-kernel-origin-v2.md`](DESIGN-kernel-origin-v2.md). Use
`fslc kernel SPEC --kernel-version 2`; omission remains v1. Its companion
conformance and coverage schemas also report `2.0.0`. A v2 schema does not imply
complete provenance: `provenance.completeness` and assurance counts expose gaps.
Both majors continue to reject compose until component file identities are
truthfully retained.

## Distribution and downstream migration

Native releases attach independently checksummed `fsl-kernel-contract-v1` and
`fsl-kernel-contract-v2` bundles. The v1 bundle contains only v1 artifacts and
continues to contain all six v1 schemas (including the shared replay trace and domain scaffold
metadata bridge), the comprehensive input fixture and
its golden public Kernel JSON, the golden failure vectors, the golden
conformance coverage matrix
(JSON and Markdown), the replay positive/negative fixtures, and this guide. The
v2 bundle carries the same replay schema and fixtures. Target-specific generators remain
supported. General pytest, Vitest, Swift, Kotlin, Dart, and PHPUnit generators
completed their Public Kernel v1 input migration in
[#214](https://github.com/ymm-oss/fsl/issues/214); their shared adapter also
consumes scenario JSON and the versioned fixed-seed testgen trace. Compose retains an
explicit names/order-only producer because both public Kernel majors reject
incomplete multi-file provenance. All target emitters still consume the same
normalized adapter, and that producer is retired when truthful compose export
exists. Domain TypeScript, Python, Kotlin, Swift, and Rust scaffolds completed
their Public Kernel v1 input migration in
[#213](https://github.com/ymm-oss/fsl/issues/213). Because the closed Kernel
contract intentionally omits unused domain declarations, source expressions,
and saga/effect source topology, their shared adapter also consumes the
versioned `domain-scaffold-metadata.v1` public compatibility companion. The
adapter rejects incompatible versions and missing lowered member counterparts;
the companion is authoritative for topology that v1 cannot represent. It is supported for at
least two minor releases and may be removed only in a following major after an
equivalent external generator or negotiated public topology contract exists.
No target emitter receives `DomainSpec`, and `domain testgen` reuses the same
adapter/effect output. The native Rust typestate
analysis and TypeScript output migration completed in
[#215](https://github.com/ymm-oss/fsl/issues/215): the CLI now feeds public
Kernel JSON v1 to the generator, rejects incompatible schema versions, and no
longer uses the private-model adapter. The frozen Python reference is unchanged.
