<!-- SPDX-License-Identifier: Apache-2.0 -->

# Internal origin chains and diagnostic tracing

Status: implemented by issue #240. Public Kernel v2 publication remains issue #256.

## Contract boundary

The Rust frontend carries provenance in a non-serialized `OriginRegistry`
sidecar from
domain lowering through `KernelSpec`, checked `KernelModel`, verification, and
diagnostic/explain rendering. Read-only Rust accessors expose the sidecar to
in-process diagnostic consumers, while its binding storage remains private.
The registry is the internal source of truth. It is not serialized by
`public_kernel_contract`, and it does not change Public
Kernel v1, its `schema_version: "1.0.0"`, schema, golden files, or default
`fslc kernel` output.

Public Kernel v1's closed `origin` object is a deliberately lossy compatibility
projection. Its historical `declaration` field may still be derived from a
`MetaTag` for byte compatibility. Internal code must not treat that projection
as the provenance model or use `MetaTag.id` as an origin identity. Publishing
the chain requires Public Kernel v2.

## Model

An `OriginChain` contains a deterministic identity, known or unknown source
file and UTF-8-safe full span, dialect, user declaration path, ordered lowering
steps, generated flag, optional primary origin, and secondary origins.

`OriginRegistry` binds chains to stable semantic targets: state declarations,
init statements, actions, action guards/statements, properties, and terminal.
The same identity bound to multiple targets represents one-to-many lowering.
Secondary sites represent many-to-one lowering. `primary: None` represents a
generated-only/source-less node; the implementation does not invent a location.

Requirement traceability is stored independently in `TraceabilityRegistry`.
It may refer to the same semantic target without becoming part of the origin
identity.

## Propagation and diagnostics

Domain expressions and lvalues are matched to direct Surface AST targets by
their preserved spans. The sidecar then moves through checked-model
construction. Model validation attaches a target origin when it reports an
error. Path-aware parsing attaches the root file identity to parse, name, type,
and lowering errors. `can()`, finite membership, and legacy operator rewrites
are recorded as lowering steps.

Verifier and runtime algorithms remain name-based and solver-independent. At
the output boundary, Rust CLI renderers resolve violated properties and trace
actions through the checked model. The user declaration is the primary `name`;
the generated Kernel name remains `generated_name` machine detail. JSON
diagnostics and `explain` include the chain. Unknown/generated-only origins stay
explicit.

Compose retains its fail-closed Public Kernel v1 behavior. Complete multi-file
compose provenance is outside issue #240.

The versioned compatibility surface is Public Kernel JSON, not direct external
construction of the Rust parser/model structs. Retaining full spans and the
non-serialized sidecar adds internal fields to `DomainLoc`, `CoreError`,
`ModelError`, and `KernelModel`; downstream Rust code should obtain these values
through the parser/build APIs and read-only origin accessors rather than struct
literals. This source-level change does not alter the versioned JSON contract.

## Mechanical evidence

- `rust/fsl-core/tests/origin_chain.rs` covers UTF-8 source identity, `can()`,
  membership, legacy operators, state and nested-lvalue propagation,
  one-to-many, many-to-one, generated-only, requirement separation, and
  parse/type/lowering diagnostics, including duplicate state/type validation
  with primary and secondary declarations.
- `rust/fslc/tests/domain_origin_diagnostics.rs` covers verification,
  counterexample, explain, and the closed Public Kernel v1 projection.
- `rust/fslc/tests/kernel_contract.rs` continues to compare the complete v1
  golden contract without regeneration.
