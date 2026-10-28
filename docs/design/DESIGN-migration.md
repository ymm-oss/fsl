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
LSP retains the exact span and publishes its UTF-16 range. Worker check/verify
source results share edition handling, including parser/semantic/implements
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
All admitted Worker check/verify success, validation-error, and implements-error paths
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

## Amendment: captured-root admission, partial delivery (#1449)

Source-edition classification is shared by native `check`, `verify`, `db check`
and `compat check`, `domain check`, `scenarios`, generic `testgen`, `kernel`,
`conformance`, `testplan`, `typestate`, `explain`, single-FSL-source `analyze`,
trace replay, and all six causal commands. These consumers classify a captured
root before model/solver work or output effects and consume those same bytes.
The single-root extension completes 21 additional modes: the six AI readers,
domain analyze/expand/generate/replay/testgen, document check/claims/generate,
database observation, log replay, counterexample export ordering, mutate success
warnings, sweep capture and direct HTML/ledger raw-warning delivery.
Existing header/gate/conflict rules and command-specific read-error kinds are
retained; this adds no CLI flags or accepted dialect/header combinations.
The single-input analyze entry also applies edition preflight to `.toml` text;
this does not admit the referenced FSL roots or complete project-mode intake.

`run_admitted_roots` accepts caller-owned read results and classifies each input
slot once. A single-slot adapter only reshapes that call. Causal diff admits both
operands; observation admits its explicit FSL mapping too. Expectation parsing
and model construction share the captured causal root rather than rereading it.
Slot identity is retained: the same file in two explicit slots may contribute
one warning per slot, two in total; completing live diff/refine intake is deferred.
Envelope `fsl_edition` and `edition` come only from the primary root (first slot);
secondary slots contribute warnings without overwriting that root's metadata.
Preparing all direct causal roots also intentionally changes error precedence:
a later unreadable operand or observation mapping is reported before an earlier
root's parse/model error, because consumption starts only after preparation.
Log replay likewise prepares its root and explicit mapping before model work:
an unreadable mapping now precedes an ordinary root parse/model error.
Document check admits the captured root before artifact IO or frontmatter parsing,
then projects those same bytes under the artifact's recorded display label;
that label is not another path to read. Ordinary projection still follows label
discovery. These two error-precedence changes are intentional.
JSON traces, evidence, plans, records and trust keys are not FSL roots. Recursive
compose `use`, `implements from`, causal `uses` and governance dependencies are
outside this root-admission change. Git-revision roots are tracked by #1513.

The planner still owns complete finding identity and migration safety. Each
result is moved to one warning-finalization site; a borrowed `SourceEdition` can
finish distinct results without classifying the source again. Admitted
verification folds governance before finalization. Domain/database checks,
scenarios and generic testgen own their outer finish and suppress an inner one.
This fixes domain check's duplicate migration warning in both definitive and
non-definitive verification results. Worker check/verify admit before model or
solver use, preserving option-error priority. Editor refusals retain UTF-16
ranges and prevent model construction and follow-on analysis.
Native and Worker check pass the admitted token through `run_check_stages` to
`diagnostics_admitted`; that inner diagnostic preflight does not classify again.

Implemented open JSON outputs receive findings in `warnings`. Closed successful
Kernel/conformance/testplan output and implemented raw TypeScript/readable/graph
output instead use stderr `warning: <finding-json>`, retaining stdout schemas and
formats. Their error envelopes remain open.
Direct raw HTML/ledger, document generate/claims and domain expansion/test generation also
deliver migration warnings on stderr. HTML/ledger file-output envelopes remain
unchanged; their distinct verification baselines retain one finished warning.
Approval-only rendering retains its own baseline admission without new stderr
delivery. Mutate finishes the authored source once, even when it later uses a
synthetic mutation source; sweep captures once and lends its token to each cell.
All 21 extension modes use the shared `run_admitted_roots`/`run_admitted_root`
owner; the sweep grid consumes the captured string and borrows the owner's token
without finalizing its aggregate envelope.
Existing non-migration warnings, verdicts, assurance and exits are preserved on
admitted current-edition input;
next-edition migration findings refuse with the existing edition error and exit 2.
Ordinary parser/type errors keep their command-specific diagnostics. Repair tools
`lint`, `migrate` and `fmt` retain their planner/formatter projections, validation,
safety and exit contracts rather than treating migration targets as consumer errors.

The measured boundary comparison uses BASE
`5637daf2ce8ce3e1a2c3b1266a8442c3df7887e4`, warning-exposure parent
`25f68db7fe214ed2ed66034701ed1bf06b5fbe7c`, and built implementation
`33e3fcc5606249a097814d91cb09b506add8e97d`. With cache disabled,
`fslc check FORM` checks all 597 tracked `.fsl` files in `specs/`, `examples/`,
and `rust/fslc/tests/fixtures/`, plus 309 inline occurrences, using BASE and
implementation binaries in both materialized trees: 3,624 checks. Only forms
with an fsl 5 header enable the existing fsl5 gate; fixture edition requests are
retained. The ok-to-error and error-to-ok sets are empty. Full JSON, exit and
stderr comparison has no excluded fields: 57 corpus forms and 22 inline
occurrences change only `warnings`, not another output field or exit.

The separate 52-native-leaf × nine-form comparison runs all three binaries
(1,404 observations). Parent-to-implementation records these **breaking consumer
removals**, not new grammar removals: the next-edition enum-union form and the
three next-edition metadata/colon-quantifier/inline-mapping forms change from a
non-error consumer result to edition refusal for `analyze`, `conformance`,
`explain`, `html`, `kernel`, `ledger`, `mutate`, `scenarios`, `sweep`, `testgen`,
and `typestate`; `testplan` additionally changes for the enum-union form only.
That is 45 command/form observations, not 45 distinct sources. Existing check
already refuses these forms. No canonical control changes result, exit or
artifact presence. That measurement did not complete HTML/ledger raw warnings,
mutate success warnings or sweep capture; the single-root extension now does.
Upstream #1450/#1459 restrictions, including the located Bool `acyclic(done)`
refusal in the shared #1463 row, belong to BASE and are not attributed here.

The initial single-root comparison uses BASE
`c79016efeb630c717292e9392901cfe65ab40915` and implementation
`ceb03268e567487f6ec080afcf3ab034d4a5ea0c`, each built in its own clean archive.
Cache-disabled `fslc check FORM` runs both binaries in both trees over all 597
tracked corpus forms and eight input occurrences from rewritten required tests
(including included files and the approval baseline used by a helper):
2,420 checks. The full
stdout/stderr/exit difference set and ok-to-error set are empty, with no excluded
fields. The 21-mode consumer inventory executes 772 observations across two
repeats. Both produce 17 distinct breaking command/source removals (19 control
occurrences per repeat, including repeated requirements-metadata probes):

| Consumer | Previously non-error next-edition sources now refused | Pairs |
|---|---|---:|
| domain analyze/expand/generate/replay/testgen | enum union | 5 |
| document claims | metadata in spec and requirements, colon quantifier, inline mapping | 4 |
| document generate | metadata in spec and requirements | 2 |
| document check | requirements metadata | 1 |
| replay --from-log | enum union, metadata in spec and requirements, colon quantifier, inline mapping | 5 |

These are consumer-intake removals, not grammar removals or proof claims. A
mapping-only control already errored at BASE and is not counted as a removal.

This is not all-reader completion. `DEFERRED_TO_SECOND_PR` now names eight modes
across seven leaves: analyze batch and project, approval check/create/diff, chain
admission, live diff and refine. Approval-check transport explicitly includes the
ledger `--approval` overlay, whose root rereads remain deferred. Existing report
baselines, ledger migration rendering and per-layer warning behavior are retained.
Deferred cells still execute, and each named mode must retain a rejecting
observation; an unclassified leaf or a fully green mode left deferred fails the
inventory. `OUT_OF_SCOPE_1513` separately identifies historical revision cells.
The remaining modes must be completed and the deferred set emptied to close #1449.

Earlier refusal can remove next-edition inputs accepted by an ungated consumer;
it does not remove current-edition syntax. The final-base corpus and consumer
comparison above records the exact revisions, form counts and per-command
transitions for this extension. Public-example
warning cleanup (#608), AI/causal lint-planner support (#609) and unrelated ledger
warning classification (#1497) are not part of this delivery.
