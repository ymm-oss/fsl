# Canonical Markdown document trace

Issue: #192.

## Format contract

Some projects must keep natural-language Markdown as the canonical contract.
FSL supports that workflow with a deliberately strict normative-section
format:

```markdown
# Product requirements

## REQ-1: A submitted claim can be approved
Approval requires a positive amount.
```

A normative requirement starts at exactly `## ID: title` where ID matches
`[A-Z][A-Z0-9_-]*-\d+`, and ends at the next Markdown heading. Every normative
statement must be inside such a section; text elsewhere is non-normative.
Duplicate IDs are an error.

The canonical tag text is the heading title plus all non-empty body lines,
with whitespace collapsed to single spaces. The FSL declaration/scenario tag
copies that full canonical text. This fixed normalization makes freshness
comparison deterministic while keeping tags single-line.

## CLI and diagnostics

```bash
fslc check spec.fsl --docs requirements.md
fslc verify spec.fsl --docs requirements.md
```

Both commands append warnings:

- `missing_formalization`: a doc ID has no declaration tag, acceptance, or
  forbidden reference;
- `ghost_requirement`: a formal ID is absent from the canonical doc;
- `stale_tag`: a shared ID's copied tag text differs, with both `old_text` and
  `new_text` included as review evidence.

An aligned document adds no warnings. Without `--docs` and without a source
tag, command output is unchanged.

## Source auto-discovery

The existing spec-level metadata tag can declare the document:

```fsl
spec Cart "source: requirements.md" { ... }
```

The path is resolved relative to the spec file. Explicit `--docs` takes
precedence. This convention changes no grammar and remains metadata only.

## Cache and meaning boundary

Verify cache keys include SHA-256 of the exact Markdown bytes. Any doc edit is
a cache miss, so trace warnings are recomputed. The bytes are key material only;
no checksum is embedded into `.fsl` and there is no acknowledgement-hash
ceremony.

The checks establish ID existence and copied-text freshness, not semantic
agreement between prose and formulas. The old/new pair from `stale_tag` is the
evidence passed to a human or explicitly approved external reviewer. Core fslc
does not call a language model or convert that judgment into a proof result.
