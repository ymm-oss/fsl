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

Unsupported or incompletely lowered structures fail export with a semantic error.
The exporter never silently omits an unknown type, predicate call, expression,
lvalue, or non-scalar parameter domain.

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
- `fslc_rust::conformance_vectors(&KernelModel, depth)`; and
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
both schemas, the comprehensive input fixture and its golden public Kernel JSON,
the golden failure vectors, and this guide. Target-specific generators remain
supported, but their next versions
should consume public Kernel JSON rather than internal model/AST shapes. Their
migration/deprecation is intentionally tracked as separate follow-up issues:
domain scaffolds [#213](https://github.com/ymm-oss/fsl/issues/213) and general
test generators [#214](https://github.com/ymm-oss/fsl/issues/214). Native Rust
typestate analysis and TypeScript output completed this migration in
[#215](https://github.com/ymm-oss/fsl/issues/215): the CLI now feeds public
Kernel JSON v1 to the generator, rejects incompatible schema versions, and no
longer uses the private-model adapter. The frozen Python reference is unchanged.
