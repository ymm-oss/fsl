<!-- SPDX-License-Identifier: Apache-2.0 -->

# Token-based document frontend registry

Status: accepted. Implemented by issue #247.

## Decision

Every native document entrypoint constructs one `SourceFile`, lexes it once,
and selects a frontend from the first significant declaration token. The
ordered registry keys are:

```text
spec refinement compose business governance requirements domain dbsystem ai_component agent
```

Whitespace, `//` comments, and a UTF-8 BOM are lexer trivia. Before the
declaration, the dispatcher consumes zero or more document annotations; tokens
inside annotation arguments never participate in dialect selection. An empty
document reports `FSL-PARSE-EMPTY-DOCUMENT`. An unknown first identifier reports
`FSL-PARSE-UNSUPPORTED-DIALECT`, its exact token span, and the ordered supported
key list. Registry construction rejects duplicate keys with
`FSL-PARSE-DUPLICATE-DIALECT-KEY`.

The selected frontend receives the original `SourceFile` and a borrow of the
same token slice. Public dialect-specific parsers remain compatibility adapters:
they may lex a standalone source once and then call the token frontend, but a
registry dispatch never re-lexes. `ai_component` owns both standalone hard
contracts and project evidence files; the latter is a semantic variant selected
only after the registry has chosen that frontend. Recursive `agent` documents
likewise use the registry rather than a CLI prefix exception.

## Document annotations and paths

This design supersedes only the `DESIGN-annotations.md` non-goal that excluded
`@...` parser syntax. A document annotation is metadata attached to the selected
top-level declaration:

```fsl
@requirement("REQ-247", "dispatch contract")
@acme.review.owner(team.platform)
spec RegistryDemo {}
```

The grammar is `@` plus a dotted symbol path and an optional parenthesized,
comma-separated argument list. Arguments are strings, integers, Booleans, or
symbol paths. `requirement`, `kind`, and `undecided` map to the existing typed
annotation variants; other paths are `Custom`. They do not execute macros or
change verifier semantics.

`SymbolPath` is the shared loss-aware path primitive. It stores multiple
`SyntaxIdent` segments and the complete source span; parsing, equality, display,
and diagnostics operate on that structure. Legacy one-namespace APIs may use an
explicit adapter, but are not the authoritative path representation.

## Entrypoints and preprocessing

Native CLI, library, tools, and Worker paths call `parse_document` directly or
through the compatibility `parse_surface_document` adapter. Any future literate
or Markdown extraction occurs before `SourceFile` construction; the extracted
source and its source map then enter this pipeline, and dialect frontends do not
perform extraction.

The retained Python implementation is frozen except for its LSP surface and
cannot link the Rust crate. Its LSP frontend selection is therefore a
compatibility adapter: it mirrors the ordered native keys and significant-token
rules, masks BOM/annotation prefixes without changing offsets, and is locked by
parity tests. It is not language authority; disagreement is a failing
compatibility test and is resolved in favor of the native registry contract.

## Compatibility and evidence

- Existing unannotated AST projections, Kernel v1/v2 JSON, and CLI success
  envelopes are unchanged.
- Document annotations enter the existing target-keyed annotation registry at
  target `spec`; specialized frontends retain them in `ParsedDocument`.
- Corpus tests cover all registered dialects. Focused tests cover BOM/comments,
  annotations, keyword-like arguments, duplicate keys, exact diagnostics,
  agent/project dispatch, and LSP offset preservation.

## Non-goals

- one giant grammar for all dialects;
- removal of dialect-specific declaration parsers;
- macro execution or annotation-selected verifier behavior;
- redesign of Markdown/literate extraction;
- changes to Public Kernel v1/v2 schemas.
