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
| `init` | Typed normalized statements plus requirement/origin metadata. |
| `actions` | Finite typed parameters, ordered `guards` (requires/let binding scope), compatibility projections in `requires`/`lets`, simultaneous updates, ensures, partial-operation conditions, requirement IDs, origin, and span. |
| `properties` | Invariants, transition properties, reachability, progress, and terminal expression. |

Every expression has a `kind`, structural `type`, and source `span`; consumers do
not reparse expressions or infer types. Statement and declaration origins state
the source dialect, originating declaration or requirement ID, whether lowering
occurred, and whether the node was generated. Action/property/statement spans
include start and end positions. The current internal syntax tree does not retain
direct spans for scalar/type/state declarations, so those declaration spans are
explicitly `null`; their origin remains present. This is an intentional v1
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
checks disabled actions, concrete partial-operation rollback, and the attempted
symbolic successor plus unchanged committed state for type-bound, invariant,
transition, and ensures failures. The solver-free `fsl-runtime` dependency
boundary remains intact.

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
- `fslc_rust::conformance_vectors(&KernelModel, depth)`;
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
Conformance vectors have their own version and record the Kernel schema version
they exercise. A schema change requires coupled updates to both schema files,
golden vectors, Rust tests, this document, `docs/LANGUAGE.md`, the shared skill,
and `CHANGELOG.md`.

## Distribution and downstream migration

Native releases attach a checksummed `fsl-kernel-contract-v1` bundle containing
all three schemas, the comprehensive input fixture and its golden public Kernel
JSON, the golden failure vectors, the golden conformance coverage matrix
(JSON and Markdown), and this guide. Target-specific generators remain
supported, but their next versions
should consume public Kernel JSON rather than internal model/AST shapes. Their
migration/deprecation is intentionally tracked as separate follow-up issues:
domain scaffolds [#213](https://github.com/ymm-oss/fsl/issues/213) and general
test generators [#214](https://github.com/ymm-oss/fsl/issues/214). Native Rust
typestate analysis and TypeScript output completed this migration in
[#215](https://github.com/ymm-oss/fsl/issues/215): the CLI now feeds public
Kernel JSON v1 to the generator, rejects incompatible schema versions, and no
longer uses the private-model adapter. The frozen Python reference is unchanged.
