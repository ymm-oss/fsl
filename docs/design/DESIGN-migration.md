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

## Amendment: captured-root admission, completed live-root coverage (#1449)

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
Project analysis captures the TOML manifest once and prepares every direct FSL
layer and active refinement mapping before projection. The manifest is not an
FSL root and contributes no artificial edition metadata.

`run_admitted_roots` accepts caller-owned read results and classifies each input
slot once. A single-slot adapter only reshapes that call. Causal diff admits both
operands; observation admits its explicit FSL mapping too. Expectation parsing
and model construction share the captured causal root rather than rereading it.
Slot identity is retained: the same file in two explicit slots may contribute
one warning per slot, two in total. Live diff and refine use positional captured
sources too; a multi-link refinement's intermediate specification remains one slot.
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
JSON traces, evidence, plans, records and trust keys are not FSL roots. The
dependency extension below admits actually read compose, implements, causal and
governance sources separately from these explicit root slots. Git-revision roots
are tracked by #1513.

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
Approval-only rendering borrows captured source and its token; distinct artifacts
and verification baselines retain their standalone diagnostics without new stderr
delivery. Requirements-document rendering does not read an `implements` child and
does not scan that reference to invent a warning. Mutate finishes the authored source once, even when it later uses a
synthetic mutation source; sweep captures once and lends its token to each cell.
The existing `run_admitted_roots`/`run_admitted_root` owner shares its execution
boundary through `run_prepared_roots`; preparation alone does not extend a
dependency log across batch files or worker threads. The sweep grid consumes the captured string and borrows the owner's token
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

The latest single-root sweep uses BASE
`1844fa74a717ba9229e0b292d9421c0939016e5e` and implementation Rust revision
`e04df80f35cf3a7a3b8119dc7b6513ac7272ce76`, each built in its own clean archive.
Cache-disabled `fslc check FORM` runs both binaries in both trees over all 597
tracked corpus forms and 29 required-test input occurrences: **2,504 checks**.
The 29 comprise the original eight (two domain snapshot includes, two scenario
literals, two empty-membership inputs, the fixed approval source and the
command-matrix helper's approval baseline), twenty actually executed success-table
sources and one strengthened replay-order root. Included-file and repeated-source
occurrences are retained. The full stdout/stderr/exit difference set and ok-to-error
set are empty, with no excluded fields.

Both earlier measurements remain historical evidence: BASE
`a42d4cede3d8d688daff3173de7bd839a6992172` and HEAD
`e2669212315f4db9bec2c0c525ba8d44f77bacfd` also covered 597+29 occurrences per
tree /2,504 checks; the initial BASE `c79016efeb630c717292e9392901cfe65ab40915`
and implementation `ceb03268e567487f6ec080afcf3ab034d4a5ea0c` covered 597+8
per tree /2,420 checks. Their ok-to-error sets were empty too; they are not
relabeled as the latest sweep.

The latest BASE/Rust-revision pair also reruns the 21-mode consumer inventory,
772 observations across two
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

Live-root coverage now also includes analyze batch/project, approval
check/create/diff and ledger `--approval`, chain, live diff and direct/multi-link
refine. `DEFERRED_TO_SECOND_PR` is empty. The complete-adapter inventory requires
agreement for every nonhistorical observation, rejects a reintroduced deferral,
and still executes all three `OUT_OF_SCOPE_1513` controls. Historical Git old/new
and approval-baseline roots remain outside explicit admission; their consumed
dependencies still pass the dependency boundary below.

Diff's old operand and refinement's first implementation supply primary metadata;
approval diff's sole live root is primary. A live Git `--mapping` is a secondary
slot, whereas a mapping selected from the materialized tree remains historical.
The existing absolute/relative mapping selection is unchanged.

Batch preserves its display sort and adjacent canonical deduplication before
assigning slots. Every root is prepared before any file runs, but dependency
execution and delivery remain file-local: a rejected child in one file cannot
replace another file or the batch envelope. Project results carry direct-root
warnings only, not a TOML edition. Empty project/batch inputs retain their existing
success without indexing a nonexistent primary token.

Chain prepares readable roots before layer work or implementation effects and
groups steps from captured bytes. Immutable source/tokens travel in the existing
shared scheduler payload. Each actual step thread opens the existing dependency
scope and finishes its detail before the layer wrapper; no parent thread-local
log is inherited or made thread-safe. Ordinary read failures are retained as
results for the established layer and cutoff, not promoted to top-level refusals.

The intentional consumer-output changes are:

| Route | Before | After |
|---|---|---|
| live diff/refine mapping, multi-link later spec | earlier parse/model failure could win | later direct-input IO precedes consumer work |
| project later layer, mixed batch/project | earlier model failure or sequential batch envelope | later direct-input IO at the top-level preparation boundary |
| newly owned diff/refine/approval dependency reads | flattened read/semantic error | complete child refusal, findings and edition, exit 2 |
| approval diff rejected child | flattened error could carry approval metadata | complete refusal replaces that metadata; successful diff retains it |
| chain refinement dependency | flattened detail or missing child warning | complete refusal or one child warning inside the original layer wrapper |
| batch dependency diagnostics | compact entries dropped warnings/findings | complete per-file diagnostics; other files and compact causal errors retained |
| project direct roots/mapping | no direct-root warnings | one warning per slot, no artificial primary metadata |
| approval and its distinct artifacts | outer warnings missing; nested baseline could lose diagnostics | outer delivery once, standalone artifact/baseline diagnostics retained, no new stderr |

Earlier root refusal can remove previously accepted consumer inputs; these are
consumer-intake removals, not new grammar or proof strength. Native check's raw
governance/refinement boundary is retained. Further independent HTML/ledger
baseline pinning and the three internal seam controls are tracked in #1641;
neither renderer's independent baseline pin is included here. Existing complete
artifact roundtrips remain live, but do not rule out correlated loss in both paths.

Earlier refusal can remove next-edition inputs accepted by an ungated consumer;
it does not remove current-edition syntax. The final-base corpus and consumer
comparison above records the exact revisions, form counts and per-command
transitions for this extension. Public-example
warning cleanup (#608), AI/causal lint-planner support (#609) and unrelated ledger
warning classification (#1497) are not part of this delivery.

## Amendment: dependency edition admission (#1452)

The multi-root completion's type-boundary audit uses base
`796876558b83f2dedd7dafdf04ef77fced3de033` and Rust/source-test revision
`8796f19d9646a4edffbac0719c3693a8f88c1405`, each built in its own materialized
tree. `fslc check FORM` covers all 606 corpus files and 35,611 captured inline
occurrences (including rewritten dependency tests), both parser configurations
and observed edition requests: 72,859 configuration pairs. The additional
`e16f65bfeae773218d23f425376ea7c23b895bff` binary covers its 603 corpus files
and the same inline occurrences, for 218,571 commands overall. Both ok-to-error
sets and the complete base/head JSON/stderr/exit difference set are empty;
no output fields are excluded. Duplicate fixture occurrences remain counted.

Every FSL dependency actually read by a consumer is classified by its own header
or the current default before its bytes reach lowering, a model, a solver or an
output producer. The parent's requested edition is not inherited; canonical mixed
editions remain valid. This covers compose `use`, `implements from`, causal
`uses`, and governance delegates, before/after documents and refinement mappings,
including nested reads and rereads. It does not pre-scan unused virtual files,
introduce a graph walk, or add semantic checks to consumers that do not read a
declared dependency.

One resolver decorator reuses `admit_source`. Native captured-root, check and
verify owners share outer-owner-local thread-local records; nested owners retain
refusal precedence but only the outer owner delivers dependency warnings. Worker
and editor calls pass local records explicitly rather than using thread-local
transport. In owned commands, a child's complete refusal envelope and exit 2
replace an ordinary wrapped component error, including when a helper otherwise
swallows that error. Outside an owner the read still fails closed through the
existing read-error renderer, with the child's path and finding codes in a
single-line cause; no unowned rejection record survives the read.

Dependency findings use warnings-only finalization, without replacing the primary
root's edition metadata. Repeated findings for the same provider resource are
delivered once per outer native owner (or explicit local log), not once across all
owners a command may run. Native deduplication keys use real canonical filesystem
paths; child `loc.file` retains the reference spelling, not that absolute key.
Virtual file lookup keeps its provider keys. Distinct explicit root slots retain their existing
warning cardinality. Closed/raw outputs keep their stdout contracts and existing
stderr delivery rules. The editor builds an absolute, lexically normalized path
for its own complete-finding key and related link; it does not normalize the
Store provider's lookup or resolve symlinks. Labels without a filesystem path are not
filesystem directories. Findings anchor at the first parent use in read order and relate the
child's own source range and severity; child edits are not parent quick-fixes.

Repair roles `lint`, `migrate` and `fmt` only report child classifications and keep
their existing validation, root-edit and exit contracts; they never apply a child
edit. Parse-class child reports are findings (`code`, `kind`, `severity`, `message`
and `loc`), not nested command envelopes. Strict consumption still refuses without
an owner. Explicit root-intake slots are completed separately by the captured-root
boundary above.
A child admission is not standalone verification of all its properties.
Admission is a boundary on child bytes, not a global preflight of every declared
edge: native lemma-mode inline-implements reads and governance reads can follow
root solver work (and the latter the root-kernel cache store). Refused child bytes
still cannot enter a model, solver, lemma or cache. Worker governance pre-evaluation
returns early only for a recorded dependency refusal; ordinary governance errors
are folded after the kernel result, preserving its `error` precedence.

This is a **breaking consumer removal**, not a grammar removal: a headerless
compose root could previously consume a child whose own edition rejects a legacy
colon quantifier. It now returns the full child `unsupported_in_edition` envelope
and exit 2, with finding message `legacy colon quantifier syntax is deprecated`,
instead of `ok`/0 with no message. The independently checked braced canonical twin
still returns the complete successful check envelope. The same inline form is
retained in all three required-test contexts:

| Target | Required test | Before → after |
|---|---|---|
| `edition_dialect_controls` | `actual_dialects_and_valid_sidecars_are_preserved_twice` | composed legacy child `ok`/0 → complete child refusal/2; canonical twin `ok`/0 |
| `edition_dispatch_inventory` | `dialect_controls::actual_dialects_and_valid_sidecars_are_preserved_twice` | same form and full rejection/canonical controls |
| `edition_dispatch_inventory` | `all_nonhistorical_cells_agree_across_the_complete_native_adapters` | same helper's former bypass-preservation cell is now rejecting |

The materialized-tree comparison used base
`bc14d8855963487185cbe2608970492ff8638ba7` and corrected Rust source/test
tree `a65eddd54e38eac34331a70beb3360f8b98901e2` (the `rust/` subtree), each
built and run in its own materialized Git-root tree. The source-tree identity
remains stable when fixups are folded; final-head commit IDs belong to the PR's
verification record. The check command was that tree's native `fslc check <path>`. All **603**
`.fsl` forms under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` ran in
both stable and opt-in parser configurations (**1206 checks per tree**).
The **368 required-test inline/included occurrences per tree** include each
context above twice, its canonical twin, and the unchanged inline inputs of the
rewritten Worker tests. Across **3148 checks**, there were **6 `ok`→`error`
occurrences**: exactly the legacy-compose cell in those three contexts, twice
each. There were **0 other `ok`→`error` transitions**, including **0 in the
corpus**; all canonical twins remained accepted. This counts repeated test
occurrences, not six distinct language constructs. Documentation-only edits
do not change that Rust source/test identity or the checked corpus inputs.

Removing only a later causal original-model read or Worker preservation reread's
decorator does not change stable-file public fixtures: those exact paths are
necessarily read earlier through admission. Their coverage is the prior-read
table plus a site-specific static provider-type inventory, not a claimed isolated
runtime detector. A raw late provider makes that inventory fail; full-route
controls still preserve their observed result. No runtime fault-injection hook or
new production test seam was added.
