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

The "check failure" half of exit 2 is unconditional per input, shared by
`lint` and `migrate` (#517): every input is checked-model-validated
regardless of whether it happens to contain any legacy syntax for
`plan_migration` to find and rewrite — a spec that fails `fslc check` gets
`kind:"semantics"`/`kind:"type"` and exit 2 from `lint`/`migrate` exactly as
it would from `check`, not a silent exit 0 just because there was nothing to
migrate. As with `fmt --check`, `refinement`/`agent` dialects are not a
standalone check target and are excluded from this validation (a mapping
file has no `state` block by design). A refused plan (a legacy construct
`plan_migration` recognizes but cannot machine-apply, e.g. `&&`) is also
excluded: per the `&&` note below, no pre-migration checked model exists to
compare for such input in the first place, so the check applies only to
input `plan_migration` itself did not already refuse.

Each lint operand may be a file or directory. Directory operands expand
recursively to regular `*.fsl` files; symlink entries and other extensions are
skipped while walking them, and the combined file set is deduplicated and sorted
before diagnostics run. Explicit file operands retain their existing
extension-agnostic behavior.

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

### Manual return example normalization

The return example expresses its amount-dependent correspondence with explicit
fair `submit_small` / `submit_large` actions and local `implements` action mappings.
These replace the old `submit__b1` / `submit__b2` pair one-to-one, preserving the
common guards, threshold equality, updates, requirement annotations and fairness.
The small action still approves; the large action still stutters until a manager
decides. Payment retains `PaidLedger`, and the downstream design mapping and
acceptance trace use the declared action names. Legacy branch/name regressions
remain in dedicated fixtures; this manual rewrite does not make branch `maps`
machine-applicable or alter the migrator's refusal boundary.

### Manual cancellation example normalization

The cancellation requirements example applies the four safe finding edits to
place its action correspondences in the single local `implements` block. Its
checked Kernel, typed requirement metadata, history flag and conservation
counters are preserved. This spelling-only edit does not reformat the file:
`fslc fmt examples/pm/cancel_system.fsl --check --edition next` still exits 1,
as it does for the original example. This example update does not claim to have
passed the bulk procedure's formatting gate.

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

## Amendment: legacy expression operator diagnostics (#251)

In the observed shared expression entries (spec, requirements kernel wrapper and
readable concept, compose invariant, business policy, refinement map), fsl 5
returns the parser code `unsupported_in_edition` at the two-byte `||` / `->`
operator, with manual `or` / `=>` repair and migration-command guidance for
remaining supported edits. Lint/migrate share the same diagnostic but do not
automatically rewrite these invalid expressions. Kind stays `parse` and
exit stays 2. This changes no accepted-source set, typed model or semantics.

Only the expression parser classifies these spellings. Structural composition,
correspondence/transition arrows, quoted strings/comments, Domain's accepted
legacy expressions, and standalone editionless `parse_expr` are unchanged.
Headers select diagnostics in the shared parser; for a headerless CLI request,
a diagnostic-only reparse keeps the original grammar edition and yields no
replacement model. `ParseError::for_edition` is the single classifier used by
check/verify, formatter, lint and migrate. Type-bound range endpoints leave
structural relation arrows to their enclosing grammar. The diagnostic exception
covers the whole bound, including grouped subexpressions:
these remain generic `FSL-PARSE` failures, not accepted legacy operators.
LSP retains the exact span and publishes its UTF-16 range. All Worker check/verify exits pass
through the same edition finalization, including parser/semantic/implements
errors and headerless documents. Governance is not part of that shared exit:
check folds it only on the model path, preserving early successful AI-project
gate envelopes; verify keeps its native outer governance fold before edition.
Implicit-initial-value warnings are owned by
that step rather than also appended by success-only metadata finalization.

## Amendment: current-edition warning exposure (#251)

The shared edition step appends `legacy_string_metadata`,
`legacy_quantifier_colon`, and `inline_action_maps` to current-edition check,
verify, and Worker JSON warnings, including headerless documents. Domain check
shares the adapter, but none of these three legacy forms occurs in an accepted
domain document: string slots/maps are not domain forms and quantified domain
expressions fail closed. Its existing enum-union warning remains unchanged.
All Worker check/verify success, validation-error, and implements-error paths
pass once through the same outer edition finalization step. Check folds
governance only on its model/implements path, never on successful early
AI-project/agent gate results; verify retains its native outer governance fold
before edition finalization. Headerless documents use
current edition; gated explicit headers select their declared edition. Omitted
initial-value warnings are added only by this step, not also by success-only
metadata finalization. Existing warnings,
verdicts, envelopes, and process exits are retained. Lint already exposes these
findings and keeps its 0/1/2 exit contract.

The migration planner remains the single owner of occurrence identity: code,
span, replacement, byte edits, taxonomy, and machine applicability are unchanged.
In particular ambiguous comment/branch/duplicate mappings still refuse automatic
migration; displaying their warning must not make them machine-applicable.
Canonical annotations, braced quantifiers, and explicit action correspondences
do not produce these legacy findings. No accepted syntax or semantic model is
changed by warning exposure.
