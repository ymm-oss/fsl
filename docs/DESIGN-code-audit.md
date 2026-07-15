# FSL code audit projection

Status: Accepted (issue #222)

## Purpose and boundary

`fslc analyze spec.fsl --projection code_audit --code <file-or-directory>`
projects typed requirement relations from the checked Kernel onto explicit code
annotations. It answers which implementation locations claim each exact
requirement/Kernel-target pair. It does not prove that the code implements the
requirement, execute a host-language parser, or assign verifier assurance.

`traceability_graph` maps requirement IDs across FSL project layers.
`code_audit` starts at the executable Kernel targets of one checked spec and
maps them to source locations. They share the checked requirement-target index,
but have different output schemas and evidence boundaries.

## Annotation convention

An emitter writes one compact JSON object after the exact ASCII sentinel
`@fsl.trace ` on a physical source line. The host language may place any valid
comment prefix before the sentinel:

```text
// @fsl.trace {"schema":"fsl-code-trace.v0","requirement_id":"REQ-CHECKOUT","kernel_target":"action:checkout","origin_assurance":"generated_from_source"}
```

The closed object is defined by
`schemas/fslc/analysis/code-trace.v0.schema.json`:

- `requirement_id` is an exact, non-empty FSL requirement ID. No naming regex
  or natural-language interpretation is applied.
- `kernel_target` is an exact checked target: `init`, `action:<name>`, or
  `property:<kind>:<name>` where kind is `invariant`, `trans`, `reachable`, or
  `leadsTo`.
- `origin_assurance` is Public Kernel provenance vocabulary:
  `source_backed`, `generated_from_source`, `generated_only`, or `unknown`.
  These values describe origin, not `proved`/`bounded` verification strength.

One annotation represents one pair. Repeated physical annotations are valid
when several requirements or Kernel targets map to the same implementation.

## Scan and validation contract

The code input is one regular file or a recursively scanned directory. Paths
are sorted deterministically; `.git` directories and symbolic links are not
traversed. All regular files are eligible, so the convention remains
language-independent. A binary file without the sentinel is harmless. A line
containing the sentinel must be UTF-8 and contain exactly one valid closed JSON
object through end of line. Locations are one-based Unicode line/column pairs.

Unknown requirement IDs produce `orphan_code_annotation`; known IDs paired
with another target produce `annotation_target_mismatch`; expected pairs with
no matching annotation produce `missing_requirement_implementation`. These are
review findings with `formal_status: not_a_violation`, so the analyzed envelope
exits 0. Invalid annotations, unreadable inputs, and unsupported CLI
combinations fail with the normal error envelope and exit 2.

The projection is JSON-only and single-spec. It cannot be combined with batch
input, `--focus`, `--profile`, `--export`, DOT, or Mermaid.

## Output and coverage

`code-audit.v0` emits every requirement and expected Kernel target, its
implementation locations, `covered`/`partial`/`missing` status, findings, and
coverage counts. Origin-assurance buckets are non-exclusive at the target
level: two annotations with different provenance can cover the same pair.
Acceptance-only requirement IDs have no executable Kernel target and are
therefore outside this projection.

The output schema is
`schemas/fslc/analysis/code-audit.v0.schema.json`. Ordering is deterministic by
requirement ID, Kernel target, normalized path, line, and column.
