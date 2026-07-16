<!-- SPDX-License-Identifier: Apache-2.0 -->

# Token-based dialect dispatch

Status: accepted. Implemented by issue #247.

## Decision

The native syntax library lexes each document once and selects its frontend from
one keyword registry. The registry keys are `spec`, `refinement`, `compose`,
`business`, `governance`, `requirements`, `domain`, `dbsystem`, `ai_component`,
and `agent`. Registration validates keyword uniqueness before the first parse;
adding a frontend changes the one registry declaration and its tests.

`parse_document(SourceFile)` is the canonical library entrypoint. It returns a
`ParsedDocument` containing the selected keyword, document-level annotations,
and typed `SurfaceDocument`. Every frontend receives the original `SourceFile`,
the same owned token vector, and the cursor of the declaration keyword. The
specialized DB, domain, and AI adapters may still be called directly, but the
canonical path does not ask them to lex again. `parse_surface_document(&str)` is
the surface-only adapter over this entrypoint.

The native CLI, Kernel lowering, WASM path, and `fslc-lsp` use this dispatch
through `fsl-syntax`/`fsl-core`. The retained Python parser cannot consume Rust
tokens in-process, so its `dialect_registry` adapter mirrors the same
significant-token and registry-key contract and masks dispatch-only prefixes
without changing source coordinates. The Rust registry remains authoritative.

## Significant declaration rule

Processing order is:

1. read the original source (there is currently no Markdown/literate extractor);
2. lex while skipping one leading UTF-8 BOM, whitespace, and `//` comments;
3. parse and retain consecutive document-level annotations;
4. select the frontend from the next identifier token;
5. pass the untouched token stream, declaration cursor, and source to that
   frontend.

Annotation arguments are consumed as a group, so a keyword such as `spec` inside
`@acme.route(spec)` cannot select the frontend. Comments and blank lines do not
break attachment. Document-level annotations use the typed IR and the canonical
forms `@requirement(id, text?)`, `@undecided(reason)`, `@kind(id, text?)`, or a
multi-segment custom namespace with string, integer, Boolean, or symbol-path
arguments. Kernel lowering binds them to the stable `spec` target.

This document supersedes `DESIGN-annotations.md` only for annotations immediately
before the top-level document. Issue #241 continues to own annotation placement
on declarations inside a document, legacy-source migration, and formatter policy.

## Diagnostics

An empty/comment-only document returns `FSL-DIALECT-EMPTY` at EOF. An annotation
without a target returns `FSL-DIALECT-ANNOTATION-TARGET`. An unknown first token
returns `FSL-DIALECT-UNKNOWN` at that exact token and lists registry keywords in
deterministic registration order. The CLI check/verify envelopes expose the code
and `loc`, plus a machine-readable `supported_dialects` list for these dispatch
failures; other syntax errors retain `FSL-PARSE` unless a narrower annotation
code applies.

The BOM contributes its Unicode-scalar column and UTF-8 byte offset to following
spans. It is trivia only at byte offset zero.

## Boundaries

- `agent` is an evidence-only surface document for generic `fslc check`: this
  dispatch frontend validates the declaration header, balanced outer envelope,
  and trailing EOF, but deliberately leaves the body opaque. Structural agent
  grammar/graph analysis remains the `fslc ai check` surface. Kernel
  verification rejects the evidence-only document, as before. The canonical
  formatter accepts an empty envelope and refuses a nonempty opaque body rather
  than guessing its internal structure.
- AI project/statistical block detection remains an AI-frontend feature scan;
  its first significant identifier must belong to the known AI project block
  set, but those evidence blocks are not dialect registry entries.
- Public Kernel v1/v2 JSON schemas do not gain annotation fields or a new dialect
  field. Existing normalized dialect strings are derived from the registry key.
- `SymbolPath` stores its segments, per-segment spans, and complete span
  structurally. `SyntaxQualifiedName` uses the same arbitrary-depth path; its
  frozen two-field Kernel projection is an explicit compatibility adapter.
  Display joins segments only at the presentation boundary, and equality
  compares segments.

## Verification obligations

- every registered dialect selects after BOM/comments/annotations;
- specialized frontends parse the registry-provided token stream;
- duplicate keys fail registry validation;
- annotation argument keywords never affect selection;
- empty/unknown codes, spans, and supported-keyword order are stable;
- CLI, library, Kernel lowering, LSP indexing/diagnostics, and the dialect corpus
  retain their existing successful outcomes, including agent and AI-project
  editor diagnostics.
