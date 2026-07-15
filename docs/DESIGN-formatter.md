<!-- SPDX-License-Identifier: Apache-2.0 -->

# Lossless syntax and canonical formatter

## Contract

`fslc fmt FILE [--edition current|next]` formats one registered FSL document to
stdout. It never changes the input file. `FILE` may be `-` for stdin.
`fslc fmt PATH... --check` accepts one or more paths, emits a JSON
`format_check` envelope, and exits 0 when every path is canonical, 1 when any
path would change, and 2 for I/O, syntax, type, or unsafe-format errors. Stdin
cannot be repeated or mixed with paths. There is deliberately no in-place
option; mutation and multi-file migration belong to the edition migrator.

Both formatting editions select an accepted, meaning-equivalent canonical
surface. `next` does not authorize migrations, implicit defaults, declaration
movement, or other changes that require judgment. In particular, `&&` remains
invalid FSL; accepting or migrating it requires a separate language decision.

## Lossless boundary

The ordinary lexer remains the sole token authority. `lossless_document`
combines its significant token spans with the untouched source slices between
them:

```text
source bytes
  |-- shared lexer --> token kind + span --> semantic parser / lowering
  `-- span gaps ----> whitespace / line-comment nodes --> formatter
```

Concatenating every lossless node is byte-identical to the input. Raw token
text therefore retains legacy spelling and delimiters while trivia retains
line comments, blank lines, BOM/whitespace, UTF-8 bytes, and annotation
attachment. A lexical failure produces an error node and keeps the complete
source; formatting refuses it at the original span. The current language has
`//` comments only, so this contract does not invent block comments.

The semantic AST is not used as a pretty-print tree. Doing so would lose
comments and source spelling. Formatting first parses through the shared
dialect registry, applies only accepted equivalent source edits, formats token
layout, and parses the result again. For file paths, the CLI additionally
type-checks both the original and result when the dialect has a Kernel lowering
gate. Stdin has no resolver base, so it receives the same syntax and
safe-rewrite checks but not resolver-backed type checking.

## Canonical and preserved forms

The token layout fixes two-space indentation, brace line breaks, comma and
colon spacing, optional semicolon line breaks, at most one preserved blank
line, and no spaces around member/range paths. Stacked typed annotations and
comments retain source order and attachment. Qualified `SymbolPath` segments
remain adjacent.

Accepted legacy domain enum unions become `enum Name { ... }`; legacy domain
logical `||` and implication `->` become `or` and `=>` only in domain
expression context. Domain `await ... on Event -> Event` routing keeps its
arrow. Legacy quantifier separator colons become the common brace form. These
edits do not rewrite abstractions such as `.size()` into `count`.

## Refusal boundary

The formatter returns `FSL-FMT-UNSAFE` without changing source when it cannot
prove comment attachment or semantic ownership. The public refusal range is:

- a nonempty opaque `agent` body, because the registry intentionally has no
  native grammar for that body;
- registry-external multi-declaration AI project files;
- a legacy domain enum with an interior comment;
- an unbraced quantifier whose body boundary cannot be identified from the
  accepted separator form;
- any lexical, parse, or checked-model error.

The corpus gate covers every registered dialect. For the supported range it
checks comment sequence preservation, byte-idempotence, and a location-free
Public Kernel comparison; compose/refinement use exact token-kind comparison
where a truthful Public Kernel export is unavailable. Declared malformed
fixtures and the refusal range must leave their original bytes untouched.

## Formatter and migrator ownership

The formatter owns whitespace and already-accepted equivalent spellings. The
edition migrator owns diagnostics, dry-run/write workflows, implicit-default
insertion, inline-mapping movement, and any conversion that is not already an
accepted semantic equivalence. This keeps one source-edit engine without
turning formatting into an unsafe migration command.
