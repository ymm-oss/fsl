<!-- SPDX-License-Identifier: Apache-2.0 -->

# Public Kernel v2 origin-chain contract

Status: accepted. Implemented by issue #256.

## Decision and compatibility boundary

Public Kernel v2 publishes the internal origin graph introduced by issue #240. The v1
schema, projection, golden, semantics, and default `fslc kernel` output remain frozen.
Callers opt in through `PublicKernelVersion::V2` or `--kernel-version 2`; unsupported
majors fail with the ordinary JSON error envelope and exit 2.

Every v2 declaration keeps its existing `origin` field, but the field is now only a
closed reference object, `{ "target": "..." }`. It points into the required top-level
`provenance` graph. It never contains a requirement ID. The sibling `requirement`
field remains the independent requirement-traceability relation.

## Portable source identity and coordinates

A source identity is either a repository-relative slash-separated path or a non-file
URI. Platform backslashes are normalized to slashes. Empty paths, parent (`..`)
components, drive-letter paths, `file:` URIs, and developer-machine absolute
filesystem paths are rejected rather than serialized.
The CLI resolves the source path against the nearest ancestor containing `.git`, so
invocation from different directories in the same checkout produces the same identity.
For unpacked release bundles without repository metadata, the canonical current working
directory is the explicit fallback root. A source outside that root is rejected. An
origin whose source file is unknown uses `source: null`.

Coordinates carry both representations because they answer different consumer needs:

- `byte_start` and `byte_end` are zero-based UTF-8 byte offsets in a half-open range;
- `line`, `column`, `end_line`, and `end_column` are one-based Unicode-scalar
  coordinates, with the end coordinate exclusive.

The lexer advances offsets by `char::len_utf8()` and columns by one Unicode scalar, so
the two representations remain mechanically distinguishable for non-ASCII input.

## Cardinality and identity

`provenance.origins` contains canonical target-specific source-chain records. Each
record has an `id`, stable `source_node_id`, dialect, assurance, optional primary site,
ordered secondary sites, ordered lowering steps, and a generated flag. Public source
node IDs namespace the internal stable ID with the portable primary source identity
when one exists. Record IDs additionally include the semantic target and binding index,
because one source node can undergo a different chain of rewrites for each generated
target. This prevents cross-file and per-target collisions without losing source-node
identity.

`provenance.bindings` maps a semantic Kernel target to one or more target-specific
origin IDs. Multiple target-specific origin records sharing one `source_node_id`, and
therefore one reverse-index row with multiple targets, represent one-to-many lowering.
Multiple origin IDs on one target, plus secondary sites on a chain, represent
many-to-one lowering. `provenance.reverse_index` is keyed by `source_node_id`, making
source-node-to-Kernel queries direct rather than dependent on a consumer rebuilding an
index.

Source-node identity is stable only for the exact source revision: the current internal
identity includes declaration path and coordinates, so edits or moves may change it.
`identity_stability: "exact_source_revision"` prevents consumers from treating it as a
cross-revision UUID.

Targets use the internal stable vocabulary (`spec`, `type:Name`, `state:name`,
`init:N`, `action:name`, guard/statement/expression descendants,
`property:kind:name`, and `terminal`). Exported declarations reference the matching
target. Missing internal provenance is not fabricated: the exporter adds an explicit
unknown record for that target.

## Assurance and source-less nodes

Assurance is closed in v2.0:

- `source_backed`: a non-generated chain has a primary source site;
- `generated_from_source`: a generated Kernel node retains its primary source site;
- `generated_only`: generated/source-less, with `primary: null`;
- `unknown`: source identity or span is unavailable and the node is not known generated.

Consumers must not upgrade `unknown` or `generated_only` from a declaration span or
generated name. Both assurances serialize `primary: null`; partial internal sites are
not published as if they were complete source evidence. Compose remains rejected for
both majors until lowering retains
truthful component file identities; the exporter never substitutes the root compose
filename.

The top-level `completeness` is `complete`, `partial`, or `unknown`, accompanied by
known/unknown counts. The carrier is currently richest for the domain dialect; direct
Kernel, business, and requirements targets without an internal binding are emitted as
explicit `unknown` records. Schema major 2 never implies complete provenance.

## Determinism, deduplication, and round trip

Origins are sorted by public ID, bindings by target, origin references within a binding
by ID, reverse-index rows by `source_node_id`, and target lists lexically. Duplicate
identical sites, steps, bindings, and targets are removed. The primary site remains primary;
secondary sites are sorted by source, byte range, dialect, and declaration path.
Lowering steps retain semantic order while exact repeats are removed.

Round-tripping JSON must preserve unknown origin kinds and extension members when a
consumer acts as a transport. A semantic consumer may ignore an unknown kind but must
not treat it as `source_backed`.

## Extensibility

Core v2 objects, sites, coordinates, bindings, and reverse-index rows are closed.
`origin.kind` is an extensible non-empty string: `source_chain` is the only v2.0 kind,
and consumers must apply the assurance rule above to unknown kinds. Each origin record
has an open `extensions` object for namespaced data. Adding optional namespaced members
there or a new origin kind is a minor change; changing required core fields,
coordinates, assurance meaning, ordering, or Kernel semantics is a major change.

## Conformance, coverage, and distribution

Kernel v2 has matching v2 conformance and coverage schemas/vectors whose
`kernel_schema_version` is `2.0.0`. Coverage includes real domain-origin evidence for
one-to-many, many-to-one, generated-from-source, generated-only, reverse lookup,
lowering steps, and requirement separation. Releases publish independently checksummed
`fsl-kernel-contract-v1` and `fsl-kernel-contract-v2` bundles; the v1 bundle contains
only v1 artifacts. The v2 bundle preserves the canonical
`rust/fslc/tests/fixtures/...` source layout so running `fslc kernel` from the unpacked
bundle root reproduces the published golden and its source identities.

Downstream adapters should negotiate the major explicitly, continue consuming v1 until
they need provenance queries, and never infer v2 support from the fslc binary version.
The domain scaffold (#213) and general test generator (#214) migrations remain separate.
