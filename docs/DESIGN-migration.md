<!-- SPDX-License-Identifier: Apache-2.0 -->

# Edition-aware lint and migration

Status: accepted

## Contract

`fslc lint PATH... --edition current|next` reports stable diagnostics and never
changes source. Exit 0 means no findings, 1 means findings exist, and 2 means an
I/O or parse/check failure. Every finding includes its code, one of the stable
taxonomies `deprecated`, `non_canonical`, `ambiguous_intent`, or
`unsupported_in_edition`, severity, exact span, edition, canonical replacement,
and whether its edits are machine-applicable.

`fslc migrate PATH... --edition next` is a dry run. It emits the same findings
plus a complete replacement edit for each changed file. `--write` is the only
mutating mode. The command plans every input in memory, parses and checks the
before/after sources, compares location-free Public Kernel JSON, prepares and
syncs sibling temporary files, then replaces the validated set. An I/O failure
during commit restores already-replaced files from same-directory hard-link
backups. A process or filesystem crash cannot be made transactionally atomic
across unrelated files by a portable CLI; recoverable `.bak` files are retained
if the process is interrupted before cleanup.

## Rewrite table

| Legacy form | Canonical form | Automatic boundary |
|---|---|---|
| domain `type E = A \| B` | `enum E { A, B }` | refused when an interior comment has ambiguous attachment |
| domain `\|\|` / expression `->` | `or` / `=>` | domain expressions only; `await ... on E -> E` routing is unchanged |
| declaration string metadata | `@requirement`, `@undecided`, or root `@kind` | refused when moving the suffix would cross an attached comment |
| colon quantifier | brace quantifier | refused when the body boundary is not structurally known |
| requirement action `maps` | action correspondence in the single local `implements` block | refused for branches, duplicates, no block, or multiple blocks |
| implicit field value | explicit initializer selected by current semantics | uses the existing `implicit_initial_value` insertion contract |

`&&` is deliberately not an automatic rewrite. It is a lexical error in the
accepted language, so no pre-migration checked model exists to compare. Lint and
migrate return `unsupported_in_edition`, the exact span, and an `and` suggestion
with `machine_applicable: false`.

## Safety and idempotence

The formatter and migrator share one byte-edit overlap/boundary validator and
one planner for enum, logical-operator, and quantifier canonicalization. The
migrator adds only semantic edits for metadata, mappings, and defaults. A safe
result must parse and check before and after, retain the location-free Public
Kernel, format idempotently, and produce no edits on a repeated migration.
Migration equivalence compares the typed annotation carrier while ignoring the
Public Kernel's closed singular compatibility projection, so replacing a legacy
string spelling does not change the public contract; multiple distinct relations
remain in the typed annotation registry and are not collapsed into that field.
Refused findings prevent every write in the invocation; safe edits are still
returned for review but are not partially applied.

The retained Python implementation is not a second migrator. The native
`fslc-lsp` preserves migration data on diagnostics and exposes
machine-applicable edits as quick-fix Code Actions through the authoritative
Rust planner.

## Bulk update procedure

1. Run `fslc lint PATH... --edition next` and resolve every refusal.
2. Review `fslc migrate PATH... --edition next` machine edits.
3. Run the same command with `--write`.
4. Require `fslc migrate PATH... --edition next` to report zero changes,
   `fslc fmt PATH... --check --edition next` to exit 0, and the project check /
   verification gates to retain their verdicts.
