# DESIGN: reproducer artifact (issue #885)

## Purpose

`fslc verify` already returns the shortest bounded counterexample for a violated
specification, but that envelope is not a stable hand-off artifact for later
implementation regression work. **Reproducer v1** closes the verifier
counterexample into a versioned JSON file that stage-2 `testgen --reproducer`
(slice 2) can consume without re-parsing source text, exposing private AST, or
masquerading as `replay-trace.v1` / `testgen-trace.v1`.

## Artifact families (do not mix)

| Family | Role | Input to |
|---|---|---|
| `replay-trace.v1` | Observed production or lab event log | `fslc replay` |
| `testgen-trace.v1` | Fixed-seed clean-spec conformance walk oracle | `fslc testgen` emitters |
| `reproducer.v1` | Bounded verifier failure evidence from a **broken** origin spec | `fslc counterexample export` (slice 1); future `fslc testgen --reproducer` (slice 2) |

Verifier stdout envelopes remain the live inspection surface. A reproducer file is
a **closed export** of one violated inspection, not a replacement envelope.

## Slice 1 (`counterexample export`)

```text
fslc counterexample export unsafe.fsl --depth 8 -o double-writer.reproducer.json
```

Behavior:

1. Run bounded verification with the same engine/deadlock options as `verify`.
2. Accept **only** `violation_kind: "invariant"` (safety invariant) counterexamples.
3. Write `schemas/fslc/kernel/reproducer.v1.schema.json` JSON to `-o`.
4. Print the **same** violated verify envelope on stdout, augmented with
   `reproducer.schema` and `reproducer.exported_to`. Exit code matches `verify`
   (1 for violated, 2 for compatibility/usage errors).

### Required artifact fields

- `$schema` / `schema_version` (`reproducer.v1`)
- `origin`: spec name, source path, `spec_digest` (`fsl-kernel-ast-v1+sha256`)
- `verification`: engine, depth, completeness, checked depth; optional
  `engine_metadata` preserving engine-specific fields from the verify envelope
- `violation`: `trace_type`, `violation_kind`, property identity, step, optional loc/origin
- `trace`: full verifier counterexample trace from the origin inspection
- `canonical_steps`: action name + params projection used by stage-2 oracle replay
- `provenance.scaling_notes`: representation-only slot (empty in v1)
- `do_not_assume`: assurance boundaries (repaired spec is oracle; export ≠ implementation proof)
- `unsupported_v1`: enumerated rejected shapes (see below)

### Explicitly unsupported in v1

| Shape | Rejection |
|---|---|
| `leadsTo` properties or violations | exit 2 before export |
| `refinement` documents | exit 2 before export |
| `induction` / CTI counterexamples | exit 2 (`--engine induction` or CTI trace) |
| Nondeterministic or partial `init` | exit 2 before export |
| Non-invariant violations (`trans`, `reachable`, `deadlock`, …) | exit 2 after verify |
| Clean (`verified` / `proved`) specs | exit 2 — nothing to export |

Future slices may widen support; v1 lists the rejections in every artifact via
`unsupported_v1` and fails closed rather than silently degrading.

## Slice 2 (out of scope here)

`fslc testgen fixed.fsl --reproducer artifact.json` replays the canonical
action/params column against the **repaired** spec Monitor and emits adapter
scaffolds. The origin counterexample state must **not** become the oracle.

## Layer selection (#968 / #643)

Reproducer export is a **same-layer** workflow: the origin and repaired specs
are expected to share action/parameter API at one FSL layer. Cross-layer mapping
(`testgen --via-mapping`, #643) is independent and not part of slice 1.

## Assurance

A reproducer records bounded/proved **failure evidence** from the origin spec.
Passing a future generated regression test is replay-observed / test evidence,
not a proof that the implementation is correct in general.
