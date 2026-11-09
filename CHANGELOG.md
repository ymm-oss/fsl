# Changelog

The change history of this project. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versioning follows [Semantic Versioning](https://semver.org/). Each version corresponds to an annotated git tag (`v1.0.x`).

## [Unreleased]

## [5.0.0] - 2026-11-07

- Added (#1344): HTML-only `--no-timing` omits execution timing fields from
  stdout and `-o`/`--output` reports for byte-stable comparison of the same
  input path, source, options, and toolchain. Default reports retain timings;
  source, domain state, counts, verification JSON, and exit codes are unchanged.
- Added (#1449): captured-root admission and current-edition migration-warning
  delivery for 21 single-root consumer modes. Each explicit root is captured and
  admitted once before consumption or side effects, and consumers reuse those
  bytes. Open results deliver each complete finding once per slot; successful raw reports,
  documents and domain output deliver findings on stderr without changing stdout.
  HTML/ledger file-output envelopes and approval-only rendering keep their existing
  delivery contracts. Eight explicitly tracked modes remain outside this delivery.
- Added (#1641): independent native regression controls for report child findings,
  first approval overlays, captured prepared-source execution, actual worker and
  fresh-step admission-token transport, and positional manifest/root carriers.
  Observation is binary-private and test-only; public behavior is unchanged.
- Added (#1705): `fslc lint` now warns with `shared_requirement_id` where a current-edition `requirements` document gives one requirement ID two owners: a second `requirement` with the ID of an earlier one, or an `acceptance` and a `requirement` sharing an ID. `check` still accepts these documents, and the ledger keeps one row for the ID. The warning is located at the second owner's ID, names the earlier owner, has taxonomy `ambiguous_intent` and is not machine-applicable.
- Changed (#251): current-edition check, verify, and Worker JSON now
  warn on legacy declaration string metadata, colon quantifiers, and inline action
  mappings, using the existing lint codes and migration-safety data. Verdicts and
  exit codes are unchanged; applicable Worker error envelopes also carry the native
  implicit-initial-value warnings.
  Testgen's verifier-failure envelopes use the same source-edition finalization
  as CLI verify, preserving its warnings without changing failure verdicts or exits.
- Changed (#552): the language server's document index is now checked against
  the parsed declarations (name, role, and owning scope) over the corpus, so an
  index heuristic that gives a name the wrong role or owner fails the tests. The
  index and the server's responses are unchanged.
- Changed (#984): the Rust workspace's `dev` profile (and `test`, which
  inherits it) now emits only line tables for workspace crates and no debug
  info for dependencies, cutting test-executable size and link time. Panic
  messages and backtraces still show `file:line` for workspace code; a debugger
  no longer sees local variables. The `release` profile, command output, and
  exit codes are unchanged.
- Changed (#1167): core diagnostics are classified by a typed error kind instead
  of their message text, so a new kind cannot reach the CLI, the language server,
  or the browser Worker without stating its public `kind` and hint. Diagnostic
  output and exit codes are unchanged.
- Changed (#1170, #1709, breaking): an input file that `fslc` cannot read (missing, a
  directory, no permission, not UTF-8) is now reported the same way on the
  routes below: `kind:"io"`, exit 2, and the message `<path>: <error>`. Routes that
  change: `approval create --kind requirements_document` with an unreadable
  `--artifact`, `--glossary` or `--evidence` (was `kind:"usage"`); the spec of
  `domain analyze`/`check`/`expand`/`generate`/`replay`/`testgen` and
  `db observe` (was `kind:"semantics"`, retiring the #780-era exception); the
  bare OS error without the path from the spec of `ai check`/`compat`/`drift`/
  `eval`/`regress`/`replay`, `db import`, `diff` (both specs and `--mapping`),
  `refine` (the mapping), `fmt`, `replay --trace` and
  `--from-log --mapping`, `approval check`/`diff --record` and `--trust-key`,
  `approval create --artifact` and `--signing-key`, `ledger --impl-log`/
  `--evidence`/`--approval`, `document check` (the document), `mutate --from`,
  `verify --from-state` and `--requirements`, and `causal observe-expectations
  --from-log`/`--mapping`/`--scope` and `causal ledger --plans`; and the causal
  family's `cannot read <path>: <error>` spelling (the spec of every `causal`
  subcommand and `causal analyze`/`ledger --evidence`). A file read from
  a `diff --git` or `approval diff` checkout is named by the caller's path or
  `<commit>:<path>`, not the temporary tree. A missing file that already
  reported `file not found: <path>` keeps that message, and only a missing one:
  any other `chain` manifest read failure is `<path>: <error>`. An unreadable
  import or `compose` component stays `kind:"semantics"` with its `loc`, and `ai` log
  and record inputs, `ai compare --from` and `db observe --trace` still report
  an unreadable file as `kind:"parse"`. A new test pins one case per changed
  route.

  `fslc`'s file-IO error classification has one owner,
  `spec_loading.rs`, and every command arm that reported an `io` error now
  builds it there. A new test counts the hand-written IO, error-envelope,
  and error-exit sites left outside their owners and fails if one is added.
  That ownership change by itself left command output, error `kind`s, messages,
  and exit codes unchanged.
- Changed (#1171): `fslc`'s shared envelope, error, spec-loading, and display
  helpers moved out of `main.rs` into their own modules, so the verification,
  causal, and code-audit modules no longer reach back into `main.rs`; each
  command's option parsing is now a typed option struct with one parser per
  command. Command output, exit codes, usage messages, and help text are
  unchanged.
- Changed (#1255, breaking): `fslc conformance` names a compose component invariant broken
  while another action stepped by its public name, `acct.Inv` (was `acct__Inv`),
  the name `verify` and `replay` give it. `fslc mutate` compares the dead
  actions of the baseline (`action_coverage` keys, `acct.thaw`) with the mutant's
  action in the same spelling, so a never-enabled component action's surviving
  mutants carry `note: "action dead at baseline — survival expected"` in a
  compose as they do in a single spec. Migration: a consumer that matched
  `alias__Name` in a conformance `outcome.name` must match `alias.Name`.
- Changed (#1256, breaking removal): a name declared in a `spec`, `requirements`,
  `business`, or `compose` document may no longer contain `__`. `fslc check`
  (and every command that loads the document) rejects one with `kind: "name"`,
  exit 2, located at the declaration; a compose component's own declaration is
  reported at the parent's `use`. This covers specs, actions, parameters, `let`
  bindings, state variables, properties, types, consts, enum members, struct
  fields, processes, stages, actors, and `use` aliases. `__` is reserved for the
  names lowering generates (a compose component's physical `alias__name`, a
  `branches` split `submit__b1`), so each has one public spelling, `alias.name`
  and `submit.b1`. Before, `action foo__bar` passed check and was published as
  `foo__bar` by the Public Kernel and `foo.bar` by conformance, and
  `use X as a__b` published its state `n` as `a.b__n`. Referencing a generated
  name, as a refinement mapping onto `submit__b1` does, is still accepted. No
  spec or example in the repository declares such a name. Migration: rename a
  declaration that contains `__` (for example `foo__bar` to `foo_bar`) and the
  references to it, including replay traces and stored outputs that name it.

  Also breaking: an action generated by `branches` has one public name,
  `submit.b1`, in every output. `fslc verify` printed it `submit__b1` in
  `cost.properties[].name` while `action_coverage` printed `submit.b1`;
  `fslc conformance` outcome names (`_requires_failed_submit__b1` is now
  `_requires_failed_submit.b1`), `fslc analyze` node and edge IDs
  (`action:submit__b1`, `guard:submit__b1:0`, `effect:submit__b1:0` are now
  `action:submit.b1`, …, and lists keyed by them follow the new order), the
  definedness error of `verify --engine bmc|induction`, the `undecided:` and
  `fslc document` claim keys, and `fslc refine`'s `progress.*.actions` now agree.
  A v1 `fslc replay` trace accepts `submit.b1`, the name conformance prints,
  which was rejected as an unknown action; `submit__b1` is still accepted, and
  `state_mismatch.action` echoes `submit.b1`. `fslc testplan` matches the
  Public Kernel's `submit__b1` to the conformance vectors' `submit.b1`, so a
  branch action's `boundary_accept_*` / `boundary_reject_*` cases are selected
  (none were), and it fails with an error instead of silently dropping the
  cases when no vector names an action that has a boundary guard. The same
  owner now spells every `cost.properties[].name` and conformance outcome name,
  so a database-dialect name no longer leaks the internal separator sentinel
  (`db_not_null_after_backfillQqDbSepqQusersQqDbSepqQid` is now
  `db_not_null_after_backfill__users__id`, as `invariant` already printed it) and
  an ai-dialect invariant matches `invariants_checked`
  (`ai_approval_before_execute.RefundPayment`). The Public Kernel keeps the
  internal name. Migration: a consumer that matched
  `submit__b1` in these fields, or stored analyze IDs or `--focus` arguments
  such as `action:submit__b1`, must use `submit.b1`.
- Changed (#1281): explicit decides clean mutant survivors first, the baseline uses auto, and progress lines go to stderr. The BMC fallback retains its kill labels; stdout is unchanged except for the encoding-only residual in DESIGN-mutate §3, where explicit-clean can survive a BMC-only `build_spec` rejection.
- Changed (#1284, breaking): diagnostics and `fslc explain` spell a compose component
  declaration by its public name. A type-check or model-build error names
  `acct.MAXX` (was `acct__MAXX`) and prints a component type as
  `Named("t.Amount")` (was `Named("t__Amount")`); readable `explain` prints
  parameter and state types as `bank.Amount` (was `bank__Amount`), and the blame
  effect text, `explain`'s JSON and `html`'s embedded copy of it, and the
  `--export tag-review` effect targets print the assignment root as
  `audit.log = audit.log.push(a)` (was `audit__log = …`). A component state or
  property is published from the compose structure rather than by rewriting the
  physical name. Synthetic names such as `_bounds_<state>` are published from
  their parts, so a state that starts with `_` no longer moves the `.`. Single
  specs are unaffected. Migration: a consumer that matched an internal
  `alias__name` in these messages or texts must match `alias.name`.

  The conversion between internal names (compose
  `alias__name`, engine-made violation names such as `_partial_<action>` and
  `_bounds_<state>`) and the names written to public output now has one owner,
  and a test rejects new spellings outside it. That ownership change by itself
  left command output unchanged.
- Changed (#1285): acceptance and forbidden traces reach their consumers only
  through the requirement-trace gate, as a type the gate alone can construct; a
  consumer that reads them ungated names that at the call site. Command output
  and exit codes are unchanged.
- Changed (#1315, breaking): the implementation side violating its own
  contract has one public word, `impl_violated`, wherever it is reported.
  `fslc refine` on an impl spec that breaks its own type bound, invariant,
  `trans`, or `ensures` within `--depth` now reports top-level
  `result: "impl_violated"` (was `"violated"`), and a governance
  `preservation` whose `after` spec does the same reports
  `governance.preservations[].result: "impl_violated"` (was `"violated"`) from
  `check`, `verify`, `scenarios`, `ledger`, and the browser Worker alike. The
  inline `implements` seam and the folded top-level verdict already used
  `impl_violated`, so the nested and top-level values now agree. The evidence
  fields (`violation_kind`, `invariant`, `violated_at_step`, `impl_trace`,
  `note`) and every exit code are unchanged, and `fslc chain`'s own run verdict
  for a failing layer stays `violated`. Migration: a consumer that matches
  `"violated"` on `fslc refine`'s `result`, on a `chain` refine layer's
  `result`/`detail.result`, or on `governance.preservations[].result` must match
  `"impl_violated"` instead; `"violated"` from `fslc verify` keeps its meaning
  (a counterexample of the verified spec itself).
- Changed (#1322): development and test builds now compile the vendored Z3 with
  optimisation (`[profile.dev.package.z3-sys] opt-level = 2`), so cmake builds it
  as `Release` (`-O3 -DNDEBUG`) — the same Z3 configuration release builds ship —
  instead of `Debug` (`-O0 -g`, Z3 internal assertions). Solver-heavy test suites
  run about 4.5× faster and the build directory shrinks from about 26 GB to 11 GB;
  verdicts are unchanged (specs and examples swept with check / verify /
  induction). Release builds are unaffected. No migration is needed.
- Changed (#1334, breaking removal): sync additional statements assigning component
  state through physical names such as `alias__x` now produce an explicit located
  error. Those spellings previously passed `check`; alias-qualified writes were
  already rejected. This removal affects only physical-name component-state
  assignments in sync additions. Binaries built from base
  `9ad07c7ad8327b637d8d419eee7a313dc726e9e5` and implementation head
  `24a807883786e3ad3477c6ecbbdace2d5378441c`, each in its own materialized
  tree, checked all 597 `.fsl` files under `specs/`,
  `examples/`, and `rust/fslc/tests/fixtures/`: recorded exit codes, results, and
  the first 160 characters of each message agree, with zero ok-to-error transitions
  (this write form is absent from that corpus). No pre-existing required tests were
  rewritten, renamed, deleted, or stopped; only new regression tests were added.
- Changed (#1335, breaking removal): compose-level `def` calls now expand with the same source-local
  predicate expander as ordinary specs, before alias resolution and
  sync argument substitution. Definitions are removed before Kernel lowering;
  duplicate, unknown, recursive, wrong-arity, and capturing calls use the
  ordinary spec diagnostics. Unused duplicate or recursive compose definitions
  that previously passed `check` now reject, just as in an ordinary spec.
  Previously valid compose predicates could stop at
  an unlowered predicate call (exit 2) instead of receiving a verdict.
- Changed (#1338, representative decision 2026-10-26): ledger risk rows now include
  every non-`refines` governance preservation, attributed using the model's catalog
  requirement metadata, and every failed inline `implements` as a spec-level
  finding. `unknown_cti` is an unestablished induction finding, with its full
  CTI/hint/rank-failure evidence retained, not a reachable counterexample.
  Direct `ledger` and `html` now return verification `error` as JSON and exit 2/3
  instead of Markdown/HTML or a generated-artifact envelope, and create no `-o`
  artifact; HTML stops before explain. Non-error failure reports still render.
  Approval-only ledger digest rerendering returns the same verification `error`; see #1418.
  Added findings and corrected coverage rendering change affected approved ledger
  and HTML digests (`drifted`, exit 0); corrected declaration attribution also changes
  affected ledger digests. Review and re-approve those artifacts.
  Documents whose verification errors now prevent report generation must be fixed
  before a new artifact can be reviewed. The inline section's noninterference
  claim applies only to that section, not to these separate rendering changes.
  Verdict findings now follow their actual declaration owner, including ranked
  and bounded leadsTo, transitions, action obligations, and synthetic property checks.
  Public compose names, generated identities behind authored labels, and branch
  names resolve through the core name map before metadata lookup; ambiguous names
  remain spec-level rather than guessing a requirement.
  Direct ledger verification errors also precede ancillary replay/evidence errors.
  Each ledger appendix finding omits only raw top-level `cost` and `cache` execution
  metadata, preserving proof evidence and same-named domain fields. Failed-ledger
  approval was already non-reproducible for `violated` reports on main; CTI findings
  exposed the same issue. Their rendered bytes and unchanged approval round trips
  are now reproducible, while genuine reviewed-content changes still cause drift.
- Changed (#1368): the parser's entry points take explicit parse options, and a
  test inventories every parse entry in the workspace so a new one cannot bypass
  them. Parsing, diagnostics, and command output are unchanged.
- Changed (#1371): the language-server index tests build the index with the
  server's parse options. The index and the server's responses are unchanged.
- Changed (#1377): the lexer reads identifiers through one selectable rule whose
  diagnostics carry their own codes, and the language server's rename check and
  the browser harness's import scan use the same rule. The VS Code grammar now
  also highlights a declaration name written with non-ASCII letters. Parsing,
  diagnostics, and command output are unchanged.
- Changed (#1378): the formatter, migration, lint, and language server share one
  edition type, and the core library and the shared command modules take
  explicit parse options. Parsing, diagnostics, and command output are
  unchanged.
- Changed (#1382): the requirements parser can desugar further declaration
  forms into the existing process, acceptance and forbidden items, and the
  names a requirements process generates now come from the core naming module.
  Parsing, diagnostics, and command output of published documents are
  unchanged.
- Changed (#1383): requirements lowering runs a name-resolution step first, and
  the name owner keeps a per-document table of display spellings beside the
  canonical names. Parsing, diagnostics, and command output are unchanged.
- Changed (#1384): the requirements parser can fold a further declaration form
  (decision tables) into the guards of existing process transitions, and the
  core checks that such a table's rows cover and partition the values of the
  fields they read, enumerating finite domains and handing larger ones to the
  solver in `check`. Parsing, diagnostics, and command output of published
  documents are unchanged.
- Changed (#1387): the requirements parser can desugar rule declarations into
  guards and properties of the existing process items, the core can decide
  over finite type domains whether such guards can hold together, and the
  names those properties generate come from the core naming module. Parsing,
  diagnostics, and command output of published documents are unchanged.
- Changed (#1389): the requirements parser reads its keywords through one
  internal keyword table, which pairs each keyword with a second spelling, and
  can read phrases whose words are marked by role, in any order. The checks of
  glossary aliases against keyword spellings read the same table. Parsing,
  diagnostics, and command output of published documents are unchanged.
- Changed (#1390): the migrator can rewrite a requirements `process` block into
  the declarations the requirements desugaring reads back into the same
  process, check that the rewrite keeps the location-free checked kernel, and
  refuse a block it cannot rewrite with a stated reason, writing no file. The
  migration's kernel comparison moved from the command into the migration
  module. Parsing, diagnostics, and command output of published documents are
  unchanged.
- Changed (#1393): the requirements parser can desugar declarations that relate
  one entity to another, state that belongs to no entity, counts over a
  concept, and goals and policies into the existing process items, properties
  and projections; the business dialect's goal and policy lowering is shared
  with it. Parsing, diagnostics, and command output of published documents are
  unchanged.
- Changed (#1394): the formatter can respell a requirements document through
  the parser's record of where each keyword and phrase was written: keywords
  through the internal keyword table, names through the glossary, and the word
  order of phrases whose role markers it adds or removes; a respelled document
  must lower to the same checked kernel. It also indents the continuation lines
  of single-line requirement statements and separates a role marker written
  onto its word. Formatting, diagnostics, and command output of published
  documents are unchanged.
- Changed (#1395): the origins the requirements lowering records for rule
  guards, rule properties and decision-table rows spell the statement in
  canonical names and English keywords, a stage guard records the move it
  came from, and `explain` can show those origins next to each guard and
  property. Parsing, diagnostics, and command output of published documents
  are unchanged.
- Changed (#1396): the language server can read a requirements document
  through the internal keyword table and glossary: aliases resolve to their
  canonical names for navigation, references, hover and rename, keywords and
  role-marking words of either spelling are not indexed as names, and
  completion and hover show each keyword's other spelling. The editor
  behaviour and the command output for published documents are unchanged.
- Changed (#1397): the requirements document renderer reads its source under
  the same parse options as the rest of the command, through new option-taking
  entries beside the projection and rendering entries, and can add reference
  views (a lifecycle diagram per entity, event and decision tables, rule
  statements, relations, resources, measures, goals, policies and the
  glossary) for a further declaration form. Parsing, diagnostics, and command
  output of published documents are unchanged.
- Changed (#1398): the repository holds a draft set of layer skills outside the
  installable skill bundle, and its tests keep that draft checked: every example
  in it must check, its keyword table must match the parser's, and the release
  must not package it. `tools/check-doc-fences.py` gains `--every-fence`, which
  fails an unmarked fence instead of skipping it. The installed skills, parsing,
  diagnostics, and command output are unchanged.
- Changed (#1399): the requirements groundwork derives the terminal states of
  an entity-based requirements document from its lifecycles, as a business
  document does from its process stages (a document that declares its own
  terminal states keeps them); the identifier groundwork's lexer warnings (a
  full-width space, an identifier not in Unicode normalization form C) are
  reported by `check`, the browser Worker and `lint`, as the language server
  already showed them; a name that is not ASCII in a document without a
  glossary is rejected as `canonical_name_not_ascii` in every dialect; and an
  inline `implements` whose layers declare the same enum with other members or
  in another order is reported with both member lists and how to reorder the
  lifecycle lines. Parsing, diagnostics, and command output of published
  documents are unchanged.
- Changed (#1400): for a further requirements form, `fslc mutate
  --by-requirement` credits a killed mutant of a guard to the requirement that
  guard formalizes; `fslc lint` applies the ID policy to the IDs its
  declarations start with, in either keyword spelling; and `fslc explain`
  respells the expression text of blamed guards, effects and conjuncts like the
  other names it shows. Parsing, diagnostics, and command output of published
  documents are unchanged.
- Changed (#1403): the requirements groundwork's process-block rewrite reports,
  as an `info` finding `derived_terminal_added`, when the rewritten document
  gets terminal states derived from its lifecycles' final stages that the
  process block did not declare. The finding names each entity and its final
  stages, says that the deadlock verdict may change, and gives the equivalent
  `terminal { … }`; the exit status is unchanged. Parsing, diagnostics, and
  command output of published documents are unchanged.
- Changed (#1410): Remove the test wording and helper names left from the `fsl5` gate after #1434: the `fsl5_*` runners no longer say they set the gate, `edition_migrator.rs` calls `ParseOptions::stable()` directly instead of `gated_options()`, and the LSP stdio test names its legacy `FSLC_UNSTABLE` values as such. No test assertion and no product behavior changes.
- Changed (#1411): Share one item-level scanner across source censuses that skips only test-only items, so product code after an inline `#[cfg(test)]` item is counted; assertions are unchanged.
- Changed (#1412): a `mutation mutants` CI shard with zero selected mutants now
  seeds its fresh Cargo target with third-party build output, including vendored
  Z3, instead of compiling it again. Every first-party crate still compiles in the
  shard: Cargo's per-artifact verdict is retained and any reused first-party
  artifact fails the shard and the aggregate. Shards that test a mutant, the same
  35 detectors, and the local unsharded gate keep their full build. A failing seed
  command falls back to a full fresh build, a seed Cargo refuses or cannot use
  fails the shard, and the log names the path taken.
- Changed (#1413): `edition_header.rs` refuses `FSLC_UPDATE_EDITION_GOLDEN` under `CI` instead of rewriting the edition golden and passing, through the same guard as the keyword table (only the value `1` updates locally), and the `issue_1351_init_requirement_diagnostics` LSP test reads through the deadline-bound receiver, so a silent server fails the test instead of hanging it. No product behavior changes.
- Changed (#1415): Add unit tests for rule-guard field spelling and requirements-solver refutation.
- Changed (#1417): native verification baselines now delegate to the same ordered
  pipeline as verify, sweep and depth-bearing chain spec layers, with caching disabled
  and the default vacuity probe preserved. Direct ledger and HTML reports check verification errors
  before presentation-model loading: malformed causal, Agent and DB-column inputs
  retain the shared diagnostic envelope; an unknown AI authority tool returns a JSON
  semantics error with exit 2 instead of panicking with exit 101. Passing reports,
  warnings, metadata, public Kernel projections and approval artifact digests are
  unchanged. The error-precedence inventory is in docs/design/DESIGN-ledger.md and
  docs/design/DESIGN-html-report.md; both language references and the command skill
  reference describe the shared boundary.
  Approval create for ledger/HTML, approval check, approval diff and counterexample
  export still lower the spec before verification or rerendering, retaining their
  invalid-input precedence. Unknown AI tools return JSON `semantics` errors (exit 2);
  approval create's diagnostic precedence remains a separate issue.
- Changed (#1418, breaking): Approval artifact failures preserve their complete downstream
  JSON envelopes and exit codes through create and check.
  Their exit moves from 2 to the downstream exit: 1 for scenarios or HTML verification
  failures and 3 for internal failures. `ledger --approval` keeps exit 2 when an `html` or
  `scenarios` record's rerender stops on a failed baseline, now with `result:"error"` naming
  the record instead of that record's envelope, and leaves the `-o` file untouched. Approval status applies to completed evaluations.
  `approved` confirms digest binding, not a verification pass, so `verify` must be gated
  separately; an approval gate requires exit 0, `approval_check` and `approved`. Debug builds check final normal-JSON envelope/exit
  pairs after internal normalization, with finite HTML/ledger report exceptions;
  release and dedicated raw/metadata/fmt delivery retain their existing behavior.
  Aborted scenarios approval results attach each root migration finding and each
  finding from a distinct dependency resource once; successful artifact bytes,
  digests and approval signatures retain their existing form.
  The debug exit check recognizes the existing `migrate_manual_required` result
  at exactly exit 1, preserving its JSON envelope and no-write behavior.
  Batch diff's debug exit check preserves its existing maximum child exit (0/1/2),
  separately from the semantic gate, including operational errors at exit 2.
  The design-citation CI scanner honors Git's declared binary/nontext attributes
  without ignoring malformed UTF-8 text or relaxing heading and tracked-target checks.
  `approval create --kind ledger` and `approval check` on a ledger record now return a
  verification `result:"error"` with its envelope and exit (2, or 3 for internal). On a spec
  that fails `check`, they previously reported an artifact mismatch and `drifted` at exit 0.
  Approving a ledger with a failed or unestablished verdict stays allowed.
- Changed (#1419, breaking): native artifact file output now requires a writable parent
  directory, including to replace a writable existing file, and refuses final
  symlinks and every non-regular destination with exit 2. This is breaking for
  `-o /dev/null`, `-o /dev/stdout` and process substitution such as `/dev/fd/63`.
  Replacement changes the inode and does not guarantee ownership/ACL/xattr
  preservation.
- Changed (#1421, breaking): New approval records use unsigned v5 or signed v6
  with a `path_base` relative to the Git root and the original source label. Checks
  resolve stored inputs and reviewed document targets from the Git root of the spec
  being checked, so a clone, worktree or relocated checkout reads its own files,
  while preserving literal paths, rendering labels and bytes. A document record's
  creation directory, target and inputs must stay inside that root: paths that are
  absolute or lead outside are refused at creation and at check. The root comes from
  the directory the spec's path names, ignoring `GIT_DIR` and similar variables, so
  `document generate --approval` on these records needs Git, and a spec resolving outside
  the root is refused. For solver records `approved` covers the spec and a fresh
  rendering, not the reviewed file on disk; a v5 record is unsigned, so a `path_base`
  edited to reach a same-content copy still reports `approved`, while v6 catches it. Legacy v1–v4
  records retain their closed formats, signatures and invocation-relative paths;
  older binaries reject new records as unsupported schemas. Document source-label
  admission remains unchanged.
  Authoritative document approval checks and ledger evaluations reject a raw
  input-digest mismatch as an I/O error only when the complete existing evaluation
  would approve. Earlier errors and full ordered drift results keep precedence;
  the first mismatch follows glossary then recorded evidence order, using the
  same loaded bytes and literal paths.
  Relative declared AI `dataset.source` paths now resolve from the declaring FSL
  file's directory. Absolute declarations and caller-relative `--records` overrides
  retain their behavior, with no fallback search. The base is the file's lexical
  parent, so a declaring file reached through a symlink resolves beside the link. The documented support-quality
  example uses `support_eval_v3.jsonl`; existing repository-prefixed declarations
  need explicit migration or a records override.
  After replay and artifact construction, causal observation rejects repeated
  actual output destinations before serialization or writes. A key is the canonicalized
  parent plus the file name, or the collapsed spelling when the parent does not exist;
  names differing only by case do not collide. The usage error names the first repeated
  paths. A later write failure now lists the earlier files in `written`. Distinct
  output names, bytes, interleaving and inconclusive assurance remain unchanged.
- Changed (#1434, breaking): the `fsl 5` syntax edition is published. A document headed `fsl 5` (or `fsl 4`) is read in that edition by `check`, `verify`, `migrate`, `lint`, `fmt`, every other command, the language server and the browser Worker without any opt-in. A document without an edition header is still read as `fsl 4`, and this change leaves its `check` verdict unchanged; other 5.0.0 changes refuse some forms in every edition, headerless documents included (see "Upgrading fslc from 4.12 to 5.0" in `docs/manual/LANGUAGE.md`). The unstable gate is removed: `FSLC_UNSTABLE` is no longer read, and the Worker request has no `unstable` field (an old client's `"unstable"` value of any type is ignored as an unknown field). `--edition 4` and `--edition 5` are accepted beside `current` and `next` (`5` is `next`; outputs still spell the edition `current` / `next`), and the five options `fmt --lang` / `--names`, `explain --names` / `--lowering` and `lint --strict-names` are accepted without the gate and are listed in `cli-contract.json`, the help text and the CLI reference page. #1715 (#1437) lists `--edition 4` / `5` there too, so all four spellings are public. The internal `fsl-bfs` comparison tool also reads `fsl 4` / `fsl 5` headers now. Before, even with the gate, all 20 headed corpus forms exited 2: 14 refused as the declaration `fsl`, and 6 Japanese ones at their first Japanese character.

  Behavior that changes for a document without a header: `lint --edition next` (or `5`) reports `missing_edition_header` (severity `info`) and exits 1 where it exited 0, and `migrate`, whose default edition is `next`, plans inserting `fsl 5` before the first significant line and inserts it with `--write`. A document that takes no edition header, such as `agent`, a `refinement` document, which is not a standalone check target, and a `compose` document, whose migration contract cannot keep its components' source files, are not offered one: `lint` and `migrate` give them what they gave before. Default `migrate` also runs the next edition's process-form conversion on a headerless `requirements` document, so one that needs a manual step (`migrate_manual_required`) exits 1 where it exited 0: 11 of the 651 corpus forms, such as `examples/annotations/annotated_claims.fsl`. Use `--edition current` to keep the former result. A document headed `fsl 5` or `fsl 4` that was refused as the unknown declaration `fsl` without the gate is now read in its edition.

  The verify cache key changes: it no longer has an `unstable` field, and it gains an `edition_request` field (the edition number) only when `--edition` is given explicitly. A `verify` result cached before this change under the gate, or with an explicit `--edition`, therefore misses the cache once and is recomputed; a result cached without either is unaffected.

  Type-boundary sweep (`fslc check` over every `.fsl` under `specs/`, `examples/` and `rust/fslc/tests/fixtures/`, 651 forms, plus 488 inline fixtures of the rewritten or removed tests), base `63dd3e02c` with and without `FSLC_UNSTABLE=fsl5` against this change: the `ok`-to-`error` set is empty. The `error`-to-`ok` set against the base without the gate is the 17 corpus forms and 16 inline fixtures headed `fsl 4`/`fsl 5`; against the base with the gate, every output is identical.
- Changed (#1437, breaking): the installed Agent Skills now describe FSL 5. The layer
  skills `fsl-business`, `fsl-requirements`, `fsl-design`, `fsl-design-review`,
  `fsl-from-code`, `fsl-delivery` and `fsl-verify` are replaced by their FSL 5
  versions, which write files headed `fsl 5`. Each one also notes that a headerless
  document is FSL 4 and points to the migration guide in `docs/manual/LANGUAGE.md`.
  `fsl-requirements/references/` adds the FSL 5 requirements syntax, the English /
  Japanese keyword table and the glossary guide.

  The shared `fsl` skill keeps its FSL 4 and kernel guidance. It adds the FSL 5
  route, the header placement, and compact sections on annotations and reusable
  predicates, Option payload scope, the ranked-response recipe, and compose and
  refinement. It also adds the per-file edition rule and the scoped-liveness
  procedure to `commands.md`. The unpackaged `skills-next/` directory is removed;
  `fsl-requirements-document` is unchanged.

  `--edition 4` and `--edition 5` are now advertised beside
  `current` and `next` on every command that takes `--edition` (`check`,
  `verify`, `domain check`, `fmt`, `lint` and `migrate`): in
  `cli-contract.json`'s choices, in each command's `--help` text, in the
  `fmt`, `lint` and `migrate` usage lines, and on the CLI reference page. A
  value outside the four now reports `--edition must be current, next, 4, or 5`
  (it said `--edition must be current or next`), still `kind:"usage"`, exit 2.
  What `--edition` accepts is unchanged, and outputs still spell the edition
  `"current"` / `"next"`.
- Changed (#1439, breaking removal): requirements acceptance `expect <expr>` and FSL 5 scenario
  `then <expr>` now use the invariant's recursive Bool expression checker before
  replay. Enum-vs-integer comparisons such as `c != 1` and `c == 1` fail with the
  existing assignability message, `kind: "semantics"`, exit 2, and the authored
  expect/then keyword location. Valid comparisons remain post-state predicates,
  not invariants; well-typed false expectations retain `kind: "acceptance"`.
  Trace-step arguments now use the same checker for internal well-typedness,
  including unselected branches, without forcing the outer parameter type.
  Errors point to the first authored step token: the action for calls, actor for
  actor-first phrases, or first token (for example the instance) for reordered
  particle phrases. Literal and internally
  consistent conditional wrong-kind values retain `wrong_kind_argument`.
  This fixes false-green traces, including an enum/integer condition selecting a
  guard-rejected value. Breaking removal: these previously accepted, internally
  ill-typed inline forms now fail with `kind: "semantics"`, exit 2 (IDs identify
  the exact tests in `docs/design/DESIGN-requirements-stage.md`):

  | Removal | Ill-typed trace form now rejected |
  |---|---|
  | T01 | acceptance Color argument with condition `c != 1` |
  | T02 | forbidden setup Color argument with condition `c != 1` |
  | T03 | forbidden final: `paint(if c != 1 then Red else Blue)` |
  | T04 | FSL 5 scenario call with condition `claim_stage[0] != 1` |
  | T05 | FSL 5 scenario with input, same malformed stage condition |
  | T06 | FSL 5 scenario particle input, same condition |
  | T07 | FSL 5 forbidden call setup, same condition |
  | T08 | FSL 5 forbidden call final, same condition |
  | T09 | FSL 5 forbidden with setup input, same condition |
  | T10 | FSL 5 forbidden with final input, same condition |
  | T11 | FSL 5 forbidden particle setup input, same condition |
  | T12 | FSL 5 forbidden particle final input, same condition |
  | T13 | acceptance numeric argument: `setn(if c != 1 then 2 else 0)` |
  | T14 | FSL 5 marked/with hybrid input, malformed ClaimStage comparison |
  | T15 | FSL 5 relation-role value with condition `ticket_stage[0] != 1` |
  | T16 | acceptance unselected branch: `paint(if true then Blue else (if c != 1 then Red else Blue))` |
  | T17 | native/shared diagnostic test's `expect c != 1` row |
  | T18 | rewritten Worker diagnostic test's identical `expect c != 1` row |
  | T19 | step: `paint(if o is some(z) then z else Red)` with `o = none` |
  | T20 | expectation: `expect (if o is some(z) then z else Red) == Red` with `o = none` |
  | T21 | reordered particle input: `claim1 を Employee が amount (if claim_stage[0] != 1 then 1 else 2) で submits` |

  Historically, against base `5a539bbe`, all 579 forms per tree under `specs/`,
  `examples/` and `rust/fslc/tests/fixtures/` in both modes still have a measured empty
  ok-to-error transition set. Including the lint-rewritten test's 21 inline
  fixtures, both modes twice give 4,800 checks: the original T01–T16 removals produce
  21 mode/form transitions (T01–T03, T13, T16 in both modes; others only in
  FSL 5), no other full-output/exit differences, no repeat differences or
  excluded fields. Repair comparisons using members of the same enum, including
  in unselected branches. See the design note for commands and measured identity.
  The round 1 supplement checks all sources of the rewritten diagnostic tests and
  changed-oracle consumers plus scope/reordered-particle controls: 20 forms per
  tree, both modes twice, 160 checks against the same base and unchanged
  production code. T17–T20 are additional breaking removals in both modes; T21
  only in FSL 5 (five fixture occurrences / nine mode-form transitions; no other
  differences or exclusions). T17/T18 are identical FSL in two tests. T19/T20
  report `public Kernel cannot type identifier 'z'`: `is some(z)` binds only in
  its true logical continuation (`=>` / `and`), not a conditional's `then` arm,
  even if unselected. Invariants already reject that scope escape; use a supported
  continuation or avoid the escaped name. T21's location is the first token
  `claim1` (8:5), not the actor later in the phrase.

  The shared checker now requires a Bool operand for `not` and an Int-compatible
  operand (including named numeric ranges) for unary minus in every model
  expression and trace. Unselected branches and short-circuited `and` / `or` do
  not bypass typing. `not (c == Red)`, `-(-1)` and numeric `- n` remain valid.
  The fresh sweep against base `5a539bbe` checks 579 corpus forms plus 51 inline
  fixture occurrences per tree, both modes twice (5,040 checks), with an empty
  corpus ok-to-error set. These seven additional inline **breaking removals**
  all change from `ok` / 0 to `error` / `semantics` / 2 in both modes:

  | Removal | Ill-typed unary form now rejected |
  |---|---|
  | U01 | forbidden step `paint(if true then Red else (if not 1 then Red else Blue))` |
  | U02 | forbidden step `paint(if true then Red else (if -Red > 0 then Red else Blue))` |
  | U03 | forbidden step `paint(if c == Red or not 1 then Red else Blue)` |
  | U04 | acceptance `expect c == Blue or not 1` after `paint(Blue)` |
  | U05 | invariant `c == Red or c == Blue or not 1` |
  | U06 | invariant `c == Red or c == Blue or -Red > 0` |
  | U07 | acceptance `expect not (false and not 1)` after `paint(Blue)` |

  These add 14 mode/form transitions to T01–T21 (44 total); there are no other
  full-output/exit differences, repeat differences or excluded fields. Use Bool
  for `not` and numeric operands for unary minus, even in skipped expressions.
  See `DESIGN-requirements-stage.md`, Unary operand typing, for commands and
  the exact measured base/head identities.

  Current rebase sweep: BASE `23eb5e41326ef4cf81a100ad0e011e81ed9fd507`,
  HEAD `1e27d896b69b395ad8e48c570b4ddbca50e442a0`, built in their own trees.
  `BINARY check FORM` covers 594 corpus forms and 65 inline occurrences per tree
  in both modes twice (5,272 checks). The corpus ok-to-error set remains empty;
  the historical 51-inline subset retains all 44 recorded mode/form removals.
  Including every inline form from all five fixed test files additionally records
  acceptance `expect` / FSL 5 `then` predicates `c != 1`, `true and c != 1`,
  `not (c == 1)`, `if c != 1 then true else false` and
  `forall color: Color { color != 1 }` as **breaking removals** for enum `c: Color`:
  acceptance in both modes, scenario only in FSL 5 (15 additional mode/form
  transitions, 59 total). Repair comparisons with same-enum members. The two
  corresponding `c == 1` fixtures remain errors but change their diagnostic from
  `acceptance` to `semantics` at the expectation keyword, with exit 2 unchanged (three mode/form
  changes). These account for all 62 full-output differences, with no others,
  repeat differences or exclusions. S1 calibration passed BASE and rejected HEAD
  twice. See the design note for inventory commands and full measurement scope.
- Changed (#1440): the `issue_1573_pattern_reuse` and `issue_1480` envelope
  tests take the expected `fslc-rust` and `fsl-core` versions from the build's
  package version, and the WASM browser parity test reads `fsl-wasm` and
  `fsl-core` from the `[workspace.package]` version in `rust/Cargo.toml`, instead
  of a literal `4.12.0`, so a release bump no longer fails them. Product output
  is unchanged.
- Changed (#1441, breaking removal): v5.0.0 auto mappings compare source actors on both
  sides (the `by X` label for an edition 4 process transition) independently of
  `covers`, including fsl 4 process/business transitions,
  cross-edition 4/5 pairs and typed/legacy compose pairs. Previously hidden
  mismatches now return the existing located `maps auto actor mismatch` type
  error (exit 2) in check/unscoped verify/refine and Worker; mapping-free diff
  instead reports unknown/automatic_mapping_failed within semantic_diff/exit 0.
  Explicit action correspondences, metadata rules for unregistered actions
  (raw spec/compose actions), and public
  Kernel/JSON schemas are unchanged. The complete four-cell `fslc check` sweep at
  base `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9` and head
  `9ca8630c7f45f1def54c056bf53a852853c7a60c` (579 base / 591 head forms in their
  own trees) measured no corpus ok→error or error→ok transitions. Separately,
  the expansion class is registered transition metadata whose requirement text
  begins with `by ` (covers text and next-edition rule/decide text). Matching actors with
  impl/business covers text `by Manager` (B3/B4) or abstract event rule text
  `by Manager only` without covers (F1/p5) now check ok/0 rather than error/type/2;
  bounded verify returns verified/0 with implements refines and F1 standalone
  refine returns refines/0 instead of error/type/2. There is no general automatic
  semantics-preserving migration; see the measured removals, expansions and migration contract in `docs/design/DESIGN-refinement.md`, section "#1441: source-actor auto mapping and breaking-source migration".
- Changed (#1442, breaking removal): synchronized reference arguments read only
  header parameters, consts, and state, not `let` bindings in the additional items.
  The previously accepted `action s() = b.go(t) { let t = true }`, where that let
  is the only declaration of `t`, now fails `check` with exit 2 and
  `kind: "semantics"`; its cause is
  `public Kernel cannot type identifier 't'`, retaining the original component
  position. As with other component-owned model errors, the diagnostic is wrapped
  at the component's `use` declaration with that inner cause and component
  file, line and column preserved.
  Declare a header parameter and supply its value at the call site, or read
  composition-side state instead; review the changed interface or state semantics.
  There is no automatic migration, compatibility bypass, or grace period.
  The checked repository corpus has zero ok-to-error transitions; this removal
  is demonstrated separately by an inline regression with accepting parameter
  and state controls.
  Composition also no longer rescues originally undeclared component value reads
  or update targets through header parameters, composition state, foreign enum
  members, or generated state/const names. Such inputs now retain the standalone
  component's semantic cause and original component location; late model errors
  retain them inside the existing component-use wrapper. Declare or
  correct the name in its owning component, or pass a value through an explicit
  component parameter; there is no automatic migration or grace period. The
  check sweep comparing base `9ad07c7ad8327b637d8d419eee7a313dc726e9e5`
  and implementation head `24a807883786e3ad3477c6ecbbdace2d5378441c`
  covers all 597 repository forms with zero ok-to-error
  transitions; inline standalone-refusal controls demonstrate this removal even
  though those invalid source forms do not occur in the checked corpus.

  Changed (#1442): `check` now validates each `leadsTo` helpful-action argument
  with the ordinary expression typechecker against the corresponding action
  parameter type, in the property's binder scope. Previously unchecked argument
  expressions with wrong types, unknown names, or forbidden pattern shadowing
  now reject before verification. Name and arity checks remain unchanged.
  Diagnostics identify the helpful clause; individual argument spans require a
  separate follow-up issue. State independence remains induction-only.
  Composition also preserves original-source pattern refusals after aggregate
  normalization: prefixing cannot rescue a nonempty `count` or `sum` whose
  pattern shadows an original global. Empty aggregates remain accepted when
  their standalone source accepts them. No engine semantics or bypass is added.
- Changed (#1446, #1341, #1342, #1343, #1447, breaking): Native
  argv acceptance is validated centrally against the embedded CLI contract.
  Non-owned shared options on `explain`, `typestate`, `mutate`, `testgen`,
  `html`, and `ledger`, non-owned `scenarios` options even at default values,
  and unknown options or excess operands on `version` are usage errors
  (exit 2), rather than silently accepted or interpreted by another command's
  parser. Both `testgen` and `domain testgen` now publish the existing
  `--allow-unwired` flag in their contract actions and help.
  All choices are checked before source I/O, including previously deferred
  checks on `analyze`, `causal analyze`, `db import`, `db check`, `ai check`,
  `domain check`, `domain generate`, `domain testgen`, and `testgen`: invalid
  values now use `kind: "usage"` instead of downstream `io` or `semantics`,
  with the existing messages preserved. Unsupported `--kernel-version`
  values on `kernel` and `conformance` likewise change from `semantics` to
  `usage`, retaining the unsupported-public-Kernel-major message and exit 2.
  Odd `refine` chain operands and malformed `diff --git` ranges already caught
  by shape validation change from `io` to `usage`, preserving their messages
  and exit 2. Three-dot ranges are now rejected before Git ref resolution
  with `--git range must be BASE..HEAD`. The checks retain their branch
  order, and the handling of nonexistent Git refs is unchanged.
  Unknown-option diagnostics now name
  the real command and option on `scenarios`, `ai check`, `domain check`,
  `domain expand`, and `db observe`, and identify dash-prefixed unknown
  options on the `compat` group. Existing root/other-group wording,
  `compat` no-argument/bare-unknown usage and `--include-ai` wording, and
  terminal `ai compare` missing-value wording are preserved.
  `ai compare` no longer accepts literal `--` as an option value, including labels
  and dataset: it reports a missing-value usage error (exit 2) instead.
  An option-position bare `--` now terminates `ai compare`, like other commands;
  following operands remain usage errors.

  Per issue:
  - `explain` and `typestate` reject shared options that do not
    apply to them, and `testgen` rejects its unused `--engine` option. These
    options now report an unknown-option usage error (exit 2), before reading
    any option value (#1341).
  - `db check` validates each `--deadlock` value as `warn`,
    `error`, or `ignore` before loading the source. Invalid values report a
    usage error (exit 2), even when followed by a valid repeated value (#1342).
  - `refine` reports an unpaired additional chain operand as a
    usage error instead of an I/O error, preserving the message, exit 2, and
    literate-input diagnostic precedence (#1343).
- Changed (#1449, breaking removal): Native checking and verification classify the root source's
  edition before consumption. Verify-shaped results share migration-warning
  finalization after governance, including nested verification and report baselines,
  instead of repeating it in the CLI verify and test-generation failure paths.
  Edition-migration ledger rows now say 移行候補 with their code and message instead of vacuity or mutate advice.
  Migration-only rows use status 🟡 移行候補 and risk —; non-migration or mixed rows retain 🔴 要確認 and risk 要確認.
  Kernel, conformance, testplan and typestate consumers also admit their captured
  source before analysis; closed contracts and raw TypeScript deliver migration
  warnings on stderr without changing their output schemas.
  Worker check and verify reject unsupported source editions before model or solver
  consumption, retain verification-option error priority, and deliver admitted
  warnings once after governance.
  Editor diagnostics reject unsupported editions before model or analysis work and
  retain migration finding ranges for UTF-16 publication.
  Replay, explain and single-source analysis also apply admission before consuming
  the captured source, with readable and graph output warnings kept on stderr.
  Domain check now finalizes each result once instead of duplicating migration
  warnings across its specialized result and nested verification result, including
  non-definitive verification errors; requested editions and read-error kinds are retained.
  Causal consumers admit captured roots and explicit observation mappings before
  analysis or output, preserve specialized warnings, and reuse the same source for
  expectation parsing and model construction instead of rereading the root.
  Causal root preparation intentionally reports a later operand/mapping read error
  before an earlier root's parse/model error. Envelope edition metadata now comes
  only from the primary root; every slot's warnings are retained once. Native and
  Worker check reuse the admitted token for their inner diagnostic preflight.
  The single-root extension completes 21 further modes: six AI readers, the five
  non-check domain commands, the three document commands, database observation,
  log replay, counterexample export ordering, mutate success warnings, sweep capture
  and direct HTML/ledger raw-warning delivery. Root and mapping preparation makes
  mapping IO precede an ordinary root parse/model error in log replay. Document
  check admits before artifact IO/frontmatter and projects captured bytes under
  the recorded label without rereading its path. Direct raw reports warn on stderr;
  HTML/ledger file-output envelopes and approval-only stderr remain unchanged.
  The multi-root completion covers analyze batch/project, approval check/create/diff
  and the captured-source ledger --approval overlay, chain, live diff and refine.
  Later direct-input IO now precedes earlier parse/model errors outside chain.
  Newly owned dependency failures use the complete child refusal/exit 2 rather than
  flattened loader errors; successful results receive child warnings once. Approval
  diff keeps approval metadata on success, but a child refusal replaces it. Batch
  diagnostics stay file-local, and chain finishes each detail before its wrapper.
  Git-revision roots (#1513), public-example warning cleanup (#608), lint dialect
  support (#609) and other ledger warning classifications (#1497) are separate.
  Breaking consumer removals: four next-edition migration forms (enum union,
  metadata, colon quantifier, inline maps) now refuse in analyze/conformance/explain/
  html/kernel/ledger/mutate/scenarios/sweep/testgen/typestate, and enum union in
  testplan (45 command/form observations already rejected by check). A named
  BASE/implementation `check` audit covers 597 corpus files plus 309 inline
  occurrences in both materialized trees with an empty ok-to-error set; see the
  migration design amendment for SHAs, command and attribution.
  The latest audit uses BASE 1844fa74a717ba9229e0b292d9421c0939016e5e and Rust
  revision e04df80f35cf3a7a3b8119dc7b6513ac7272ce76, built in separate clean archives.
  Cache-disabled `fslc check FORM`, 597 corpus files +29 required input occurrences
  per tree, both binaries in both trees: 2,504 checks, empty ok-to-error and full
  stdout/stderr/exit difference sets, no excluded fields. The 29 comprise the original
  eight (two domain snapshot includes, two scenario literals, two empty-membership
  inputs, fixed approval source and command-matrix approval baseline), twenty actual
  success-table sources and one replay-order root; repeated/included files remain
  occurrences. Historical BASE a42d4cede3d8d688daff3173de7bd839a6992172 /HEAD
  e2669212315f4db9bec2c0c525ba8d44f77bacfd measured 597+29 per tree /2,504 checks.
  Initial BASE c79016efeb630c717292e9392901cfe65ab40915 /implementation
  ceb03268e567487f6ec080afcf3ab034d4a5ea0c measured 597+8 per tree /2,420 checks.
  Both historical ok-to-error sets were empty; neither substitutes for the latest.
  The latest repeated 21-mode consumer comparison executes 772 observations and records
  17 distinct breaking command/source removals (19 control occurrences per repeat,
  including repeated requirements metadata): domain analyze/expand/generate/replay/
  testgen refuse enum union (5); document claims refuses metadata in spec and
  requirements, colon quantifier and inline mapping (4); document generate refuses
  metadata in spec and requirements (2); document check refuses requirements
  metadata (1); replay --from-log refuses enum union, metadata in spec and
  requirements, colon quantifier and inline mapping (5). These previously non-error
  consumer inputs are not grammar removals or proof claims; the mapping-only control
  was already an error at BASE and is not counted as a removal.
  The sweep grid now uses the same admission owner as the other single-root modes,
  retaining per-cell warning ownership without finishing the aggregate. Success
  controls compare one complete finding where the dialect has migration forms and
  require warning absence for AI/DB dialects without those forms. No planner or
  language behavior is added by those controls.

  The multi-root audit compares main base 796876558b83f2dedd7dafdf04ef77fced3de033
  and Rust/source-test revision 8796f19d9646a4edffbac0719c3693a8f88c1405, each in
  its own materialized tree: 606 corpus files, 35,611 inline occurrences, both
  parser modes and observed edition requests, 72,859 pairs. Additional base
  e16f65bfeae773218d23f425376ea7c23b895bff covers its 603 files and the same inline
  inputs, yielding 218,571 commands overall. Both ok-to-error sets and complete
  base/head JSON/stderr/exit difference sets are empty; no fields are excluded.

  The active-target/single-mapping repair repeats the boundary comparison against
  the same main base with independently materialized head
  e47dce898889e891e9fa82a5fdd8698749e4db80: all 606 corpus files and 35,663 inline
  occurrences including new inputs, 72,963 pairs / 145,926 commands. Fixture hashes
  verified and CLI checks rerun; no ok-to-error or full JSON/stderr/exit differences,
  and no excluded fields.
- Changed (#1450, breaking removal): `check` rejects calls whose caller action
  parameters, lets, expression quantifier/aggregate binders or statement `forall`
  would capture free names in the expanded def body, excluding the def's own
  parameters. The error is exit 2, `kind: "semantics"`, and `would capture variable`
  at the call. Rename the caller binding; no automatic rename or bypass is added.
  Non-global `is some(v)` bindings exported by `requires` and by a binary left
  operand also count as caller bindings and reject these previously accepted calls.
  Direct specs and source-local compose/component defs share the check. Pattern
  collision rules remain in force, including imported enum members; the explicit
  diagnostic-order exceptions below still reject with the same exit and kind.
  Compose-source expansion now receives Common declarations for its
  global-name inventory and runs after component loading, still before rewriting.

  The declaration-level `leadsTo` path is also rejected by #1528: before/after,
  `helpful` arguments, `decreases`, and `within` cannot capture a def-body free
  name through a declaration binder. These calls use the same exit 2 / semantics
  capture diagnostic; rename the declaration binder.
  The declaration binder's own `where` expression is checked for capture; this can
  precede the existing rejection of unsupported wrapper `where` filters. First
  error ordering can also change when both a component load and a compose def are
  invalid, or when both an action body and its parameter range contain invalid calls.
  In particular, **v01** (invalid parameter range and action body) remains
  exit 2/kind `name`, but can report the body error before the parameter error.
  **a01** (a pattern-bound argument colliding with a def-body binder) remains
  exit 2/kind `semantics`, but can report pattern shadowing instead of the earlier
  `would capture` message at a different first location. Restoring those two
  legacy diagnostics is deferred; neither change introduces a successful verdict.

  The final `check` sweep of all 597 repository forms, in the respective
  materialized trees for base `11f01225e244dabde0f8dfd9aaa665bc47dccfd4` and built
  implementation `dd7b47885680076412ab2f0b46ebe8854ff6758b`, has zero ok-to-error
  transitions. Check/analyze outputs agree for all 20 compose corpus files and two
  refactor controls; BMC at depth 4 agrees outside observed elapsed-duration values,
  calibrated with two same-base repeats. No semantic field is excluded. Inline
  capture controls separately demonstrate the breaking removal.
- Changed (Part of #1451, breaking removal): check and ordinary verify reject reads of an init
  may-assigned root before full-root definite assignment with explicit's
  `init references state variable '<name>' before it is assigned`, semantics/exit 2
  and a statement location. This conservatively rejects dead RHS reads,
  already-written components, and free component/path reads in
  `struct-other-partial`, `if-one-branch-read`, and #1481 N2. Never-assigned roots
  (issue-exact/free and choose, #493), permitted same-forall current-key reads
  (#1520 X1/X2, only for unresolved binder values), and unread omitted/legacy
  partial init remain accepted. Resolved partial-domain loops still require
  whole-root coverage. Binder-keyed filtered/collection writes, including nested
  ones, do not establish full coverage for a following read; rewrite with a
  resolved full-domain binder before reading. Explicit's
  free-value support gap remains in #1451. Rewrite by reading after complete
  assignment or explicitly initializing the whole root; the 19 measured-inline
  migration examples (original 13, N2, two subsets and three post-loop cases) are in
  [DESIGN-bridge](docs/design/DESIGN-bridge.md#assigned-root-read-order-1451).
  Breaking-removal evidence: `fslc check <form>` in each own materialized tree,
  base `c79016efeb630c717292e9392901cfe65ab40915` versus head
  `2d05dd63f05ca68c4bd7d06c3b24ba147ce6eda4`, all 597 `.fsl` files under `specs/`,
  `examples/`, `rust/fslc/tests/fixtures/` unchanged in full output/exit (400 exit 0,
  191 exit 2, 6 exit 1); corpus removal set empty. Separately, 42 inline sources
  direct/composed (84 forms) have 38 `ok`→`error` transitions: original 13 cases
  × 2, N2 × 2, two static-subset cases × 2 and three post-loop coverage cases × 2,
  with no other output changes.
- Changed (#1452, breaking removal): compose no longer accepts a child whose own edition rejects a
  legacy colon quantifier. The former `ok`/exit 0 becomes the complete child
  `unsupported_in_edition` refusal/exit 2 with finding message
  `legacy colon quantifier syntax is deprecated`. Its child `loc.file` keeps the
  reference spelling, so the migration refusal matches standalone checking with
  that spelling; canonical filesystem identity is only the deduplication key. The braced canonical twin remains
  accepted. This breaking consumer removal is recorded in the migration design and
  both language references, not a grammar removal or stronger verification claim.
  The materialized-tree check comparison (`bc14d885` → corrected Rust source/test tree `a65eddd5`)
  covered 603 corpus forms in two parser configurations and 368 required-test
  inline/included occurrences per tree (3148 checks). Its 6 `ok`→`error` occurrences
  are only this compose-child form: the
  `edition_dialect_controls::actual_dialects_and_valid_sidecars_are_preserved_twice`
  context and `edition_dispatch_inventory` contexts
  `dialect_controls::actual_dialects_and_valid_sidecars_are_preserved_twice` and
  `deferred_modes_have_live_disagreements_across_the_complete_native_adapters`, each
  twice. Other transitions: 0; corpus transitions: 0. Canonical twins stay accepted.
- Changed (#1456, breaking removal): v5.0.0 automatic mappings use requirement actions'
  own actors, including standalone actions and generated branches on either side,
  not enclosing requirement prose. Hidden own-actor mismatches now fail check,
  unscoped verify, refine and Worker with the existing located type error (exit 2);
  mapping-free diff keeps its semantic_diff/exit-0 envelope and reports affected
  directions as unknown/automatic_mapping_failed. Matching or absent own actors
  previously rejected by prose are intentionally accepted. Typed requirement text
  does not declare an actor. Explicit action entries, inline branch maps and stutter
  stay exempt; public metadata priority, Kernel v1/v2, RCIR, digests and report
  attribution are unchanged. The repeated four-cell corpus check at base
  `e8cf78f0162530e264128f588981d49790f2767f` / head
  `b8daaa8245f374f74448fd983755689f832484cc` (591 base / 592 head forms per binary,
  each own tree) found no corpus ok→error or error→ok transitions. Acceptance
  removals/expansions are separate controls. Confirm policy before correcting an
  action's actor; there is no general automatic migration, compatibility bypass or
  grace period. If problems occur, return to a 4.x binary. See
  `docs/design/DESIGN-refinement.md`, section
  `#1456: requirement-action own actors and breaking-source migration`.
- Changed (#1458, breaking removal): relation `contains`, `add`, `remove`, and `reachable` now
  check source and target arguments against their independently declared endpoint
  types. This breaking removal rejects previously accepted mismatched endpoints
  before evaluation with `kind: "semantics"`, exit 2, the existing assignability
  message and `hint: use endpoints matching the relation's declared source and
  target types`. `domain(r)` is now `Set<A>` and `range(r)` is `Set<B>` for
  `r: relation A -> B`, including binders over those projections and their public
  Kernel projection/binder annotations. Enum-typed projection uses are accepted;
  the enum-binder/cycle idioms `not (exists x in domain(r): reachable(r, x, x))`,
  `forall x in domain(r): not r.contains(x, Blue)`, and
  `forall y in range(r): not r.contains(1, y)` remain compatible with released main.
  The change rejects enum/Int confusion. Enum nominality, numeric/range
  compatibility, public Kernel schemas
  and runtime semantics are unchanged; LSP/Worker inherit the shared checker.
  The restacked boundary sweep used BASE
  `9af1fa5f36f91e415b7ba249888f4ada4cf7a0db` and HEAD
  `3a8a6019a5bed3ae62f960afedf2bdcf6d129341`: zero `ok` to `error` transitions
  among all 597 corpus forms. All 134 inline fixture occurrences were checked;
  #1458 removes 24 occurrences (22 distinct sources) in both modes, 48 mode-form
  transitions. The former 46-transition subset is unchanged; the expanded
  inventory additionally counts numeric Set assignment below. Three counted forms
  (count, enum set-literal equality and matching enum Set assignment) add six
  error-to-ok pairs. Each mode/tree ran twice: 5,848 checks, full outputs without
  exclusions. On this base, source-only revert calibration produces 28 intended
  failures and 24 preservation passes, with exact restoration and 52 fixed tests
  passing afterward; workspace Clippy is warning-free. The workspace suite passed
  3,468 tests on `fc77124b31744f1c1728295db6f09508265b8285`; only a proved
  whitespace-only commit follows it to the measured HEAD (whole-file equality after
  removing whitespace). Builds, CLI conformance, fixed tests twice and fmt pass on
  that HEAD. Since the earlier `8e54e862` measurement, main's #1463 rejects
  `explicit_init_state_collection.fsl` on both BASE and HEAD and adds the authored
  location to the BASE count diagnostic; #1458 removal/addition sets are unchanged.
  See
  `docs/design/DESIGN-nested-option-support.md`, section #1458, for commands,
  identities and measurements. Removed numeric `N -> N` invariants are
  `not reachable(r, Red, Blue)`, `not reachable(r, Red, 1)`,
  `not reachable(r, 1, Blue)`, `not r.contains(Red, Blue)`,
  `not r.contains(Red, 1)`, `not r.contains(1, Blue)`,
  `r.add(Red, Blue).contains(Red, Blue)`, `r.add(Red, 1) != r`,
  `r.add(1, Blue) != r`, `r.remove(Red, Blue) == r`,
  `r.remove(Red, 1) == r`, `r.remove(1, Blue) == r`;
  for `N -> Color`: `not r.contains(1, 2)`, `r.add(1, 2) != r`,
  `r.remove(1, 2) == r`. `Red` and `Blue` belong to enum `Color`.
  LSP and Worker add duplicate target-only contains/reachable occurrences.
  Projection typing additionally removes, for `s: relation Color -> Color` and
  `r: relation N -> N`, `forall x in domain(s): not r.contains(x, x)`,
  `forall x in range(s): not r.contains(x, x)`, `not domain(s).contains(0)`,
  `not range(s).contains(0)`, `forall x in domain(s): x != 0`, and
  `forall x in range(s): x != 0`.
  The expanded inventory also rejects `xs = domain(s)` for
  `s: relation Color -> Color`, `xs: Set<N>` (assignment origin 8:19).
  General breaking rule: projection elements and sets take the endpoint type.
  Enum or Bool elements are rejected anywhere Int is expected; their sets are rejected
  where numeric Sets are expected: comparison, ordering, arithmetic, min/max,
  membership, set-literal equality, Option and pattern payloads, numeric Set state
  assignment and refinement maps. The measured fixture list is not exhaustive.
  Newly accepted families include count over finite endpoint-typed domain and
  range, sum over numeric projections, enum set-literal equality, enum-typed where
  filters and conditional projection membership, let-bound projections consumed
  as matching typed sets, matching enum Set assignments and refinement maps into
  matching enum Sets. Unique over enum domain/range was already accepted on released
  main and remains accepted. These additions do not guarantee invariant truth.
- Changed (#1459, breaking removal): shared static checking now rejects non-Option
  `is none`/`is some` operands; non-integer `abs` arguments, including enum
  domain/range elements; non-relation operands of acyclic, functional, injective,
  domain, range and reachable; and non-self path/cycle relations. Reachable and
  acyclic require exactly the same declared endpoint type, T -> T: mixed
  enum/numeric relations and N -> M are rejected even if numeric N and M have
  identical bounds. Numeric compatibility does not establish self-relation identity.
  Non-relations no longer infer a fallback Set<Int> projection.

  Collection method receivers, names and arities must match the runtime:
  Set contains/add/remove take one item and size takes none; Seq contains/push/at
  take one argument and pop/head/size take none; relation contains/add/remove
  take exactly two endpoints. Map has no methods. Invalid examples now rejected
  include n.contains(0), Set.push, Seq.add, relation.size, s.contains(), q.pop(1)
  and action assignment n = q.at().

  Count/sum check original binders, Bool filters and integer values before
  expansion, even over empty ranges or zero-capacity sequences: count with
  `where 1`/`where not 1` and sum with `of true`/`of -Red` no longer disappear
  into a zero result. Well-typed empty aggregates still return zero.

  These checks cover dead branches, action assignments/requires/ensures and
  trace expressions, reporting authored locations as semantics errors, exit 2,
  before execution or replay. Valid Option bindings, integer abs, supported
  methods, heterogeneous non-path relation operations, endpoint hints and
  non-reflexive reachability remain unchanged. The former abs exception in the
  #1458 projection records is removed. Native LSP and Worker parity tests cover
  the new Option operand error.

  Boundary audit: BASE e8cf78f0, HEAD 33d7faa7; 591 corpus files plus 384 inline
  occurrences (975 forms), stable/fsl5 twice in three independently built trees
  (baseline, immediate parent, head), 11,700 checks. The corpus ok-to-error set is
  empty, with no other corpus output drift and no excluded forms or fields.
  New #1459 removals total 248 mode-forms (124 occurrences/123 distinct sources)
  across all six categories above, including numeric N -> M. Another 60 mode-forms
  remain error/semantics/2 but change from unlocated runtime failures to authored
  static diagnostics. Inherited #1439 (86 removals/11 reclassifications) and #1458
  (56 removals/6 additions) are accounted separately in the expanded inventory,
  not counted as new #1459 behavior. The design amendment records full evidence.

  Review round 1 also rejects non-self selections hidden behind conditional
  then-branch inference, through nesting, old, relation add/remove receivers and
  action let aliases. Projection equality preserves substantive relation errors
  over context-needed empty literals and retains authored locations; range bounds
  and terminal no longer report generated 1:1 origins for authored input. Existing
  numeric/Set compatibility and valid dynamic verdicts are unchanged. At that
  historical baseline, checked positions did not include helpful parameter types;
  this does not add relation-valued temporary collection support. Helpful argument
  typing was separate upstream work at that historical baseline; main's #1527 now
  checks parameter types and is preserved.

  Round-1 delta BASE 33d7faa7 to HEAD 8caf5c2e: 42 additional mode-form removals
  (21 source occurrences) and 24 diagnostic reclassifications (12 occurrences:
  projection inference 16, range-bound origins six, terminal origins two). The
  diagnostic changes retain error/semantics/2. The expanded 591-corpus/447-inline
  inventory (1,038 forms) runs both modes twice in four own-built trees, 16,608
  checks including the separate previous-fix baseline. All 8,304 repeat pairs
  agree in full, with no exclusions or corpus output drift. Inherited/R0 changes
  are accounted separately, not attributed to round 1.

  Current integration: final base `11f01225e244dabde0f8dfd9aaa665bc47dccfd4`,
  implementation measured at `2b7d2e98349505af7dccb7843e41ab659066f021` on
  `b1557a31b77f549d2c48823c3435217f9df4fc53`, carried to `11f01225` across only two
  generated language pages. All 597 corpus + 601 inline inputs (1,198 total), both
  modes twice in three states: 14,376 checks + 24 identity controls, no corpus output
  drift or exclusions. #1459 accounts for 294 mode-form removals (147 occurrences,
  142 distinct sources) and 62 diagnostic reclassifications. Main #1463 already
  owns 48 former removals/four reclassifications; added let/Kernel/CLI/shared-row
  occurrences contribute 26 removals. Historical #1458 attribution from `9af1fa5f`
  remains 48/six on its 134-entry subset, 56/eight on the expanded inventory.

  The shared #1463 row X1/operand-method-types is a breaking removal:
  `invariant Capture { acyclic(done) }` for Bool `done` now fails check at 8:3;
  outcomes become [Error, Error, Error, Error] rather than [Ok, Error, Error, Error].
  For the other 136 row sources plus one resolver dependency (137 inputs),
  `fslc check FORM` at the named base/head, stable/fsl5 twice, shows no further
  transition or output change. The inherited comparison retains 292 embedded-path
  differences and 16 observations from two prior scratch-label extraction errors;
  the real test sources now check on both baseline binaries, with no attribution
  change. Optional replay not run; no sweep output normalized. Full evidence and
  the retained Public Kernel projection-guard controls are in the design amendment.
- Changed (#1463, breaking removal): check now rejects leadsTo collection/where wrapper
  binders, init statement forall collection sources with free state references,
  and relation index reads/writes, with shared core diagnostics and authored
  locations across native CLI, LSP and Worker. These forms previously checked
  without executable semantics. Written aggregate binders, filters and values
  are checked before Count/Sum normalization, so empty expansion cannot hide
  relation indexing or existing type errors. This intentionally also removes
  Bool indexing inside empty literal-range/zero-capacity Seq aggregates
  (the #1463 decision to check aggregate source types regardless of candidate count;
  a #1459-class closure; no new type rules are added).
  Empty Count/Sum now reports every existing type error in the written expressions,
  such as the non-Bool filter in `count(x in 1..0 where x + 1)`; the 6 Bool-index
  removals are examples in the measured corpus/inline inputs, not an exhaustive
  list of type-error kinds.
  Raw validation does not expand dependent inner Count/Sum before outer
  substitution or prematurely reject a pattern name awaiting substitution;
  existing nested and pattern positive/negative controls retain acceptance.
  Map indexing, relation methods, state-independent
  init literals and ordinary assigned-state reads remain supported. Nested init
  forall remains an explicit limitation routed by auto to symbolic BMC.

  Separately fixed the false symbolic/explicit rejection of local binder names
  that shadow state, without narrowing check acceptance. Remaining range/domain
  forms are #1480, init/zero-capacity issues are #1481, typing gaps are #1459,
  and collection-literal shape issues remain #1467.

  Public output changes are intentional: the newly forbidden forms return a
  located semantics error (exit 2) before engine routing; aggregate errors retain
  their authored property location instead of synthetic 1:1. Imported failures
  anchor at the owning root use, retain the component filename/cause location,
  and show public names such as part.xs/part.Capture, not lowered part__ names.
  Wrapper messages say leadsTo wrapper rather than misleadingly bounded leadsTo.
  Model-expression type errors now point to their owning declaration, including
  errors whose base check envelope already had a loc; component type errors,
  including pre-existing ones, anchor at the owning root use and retain their
  component cause.

  The base 5738cc2c711b691a9d453b0190ead1a2ff77fc50 versus the #1463 change
  (exact measured head recorded in the PR) sweep ran twice in their own trees
  using `FSLC_CACHE=off fslc check <form>`. Both runs exited 0: 597 corpus forms +
  1016 inline IDs = 1613, with 110 accepted-to-error removals (1 corpus, 109 inline),
  identical complete reports and no excluded fields. These are 22 wrapper,
  31 init-source, 51 relation-index (18 aggregate), and 6 empty-aggregate Bool-index
  forms. The complete fresh-base sweep includes both added legacy-return fixtures
  without a difference. All 66 nested-aggregate/pattern check controls preserve
  identical accepted outputs; its 15 removals are four quantifier relation-index
  bounds and eleven sync-expanded/root relation-index forms. Complete removal IDs:

  | Removed form ID set | Count |
  |---|---:|
  | `corpus/rust/fslc/tests/fixtures/explicit_init_state_collection.fsl` | 1 |
  | `inline/1457/init-stmt/{empty-body,empty-domain}/{Set,Seq}/state/{positive,negative}` | 8 |
  | `inline/L1/{Set,Seq}/{state,literal}/{false,true}` | 8 |
  | `inline/L1/{utf16-prefix,resolved-component}` | 2 |
  | `inline/L2/{z: K,z in 0..1}/{z == 1,not done,true,false}` | 8 |
  | `inline/L3/z in Set { 1 }` | 1 |
  | `inline/L3/z: K where z == 1` | 1 |
  | `inline/I1/{Set,Seq}/{false,true}/{plain,conditional,unreachable,nested}` | 16 |
  | `inline/S4/{own-source,sibling-restored,aggregate-source}` | 3 |
  | `inline/R1/read/{init,action,invariant,trans,leadsTo,reachable}/{false,true}` | 12 |
  | `inline/R1/write/{init,action}` | 2 |
  | `inline/R1/receiver/{old,conditional,domain-binder}` | 3 |
  | `inline/guard/{StateCollection,ranked-where}` | 2 |
  | `inline/aggregate-and-component-locations/case-{00,04,08,12,16,20,24,28,32,36,40,44,48,52,56,60,64,68}` | 18 |
  | `inline/aggregate-and-component-locations/case-{01,05,09,25,29,33}` | 6 |
  | `inline/aggregate-and-component-locations/case-{72,73,74}` | 3 |
  | `inline/aggregate-and-component-locations/unchecked-init/state-source` | 1 |
  | `inline/nested-aggregates-and-sync-ownership/case-{68,69,70,71}` | 4 |
  | `inline/nested-aggregates-and-sync-ownership/case-{76,77,78,79,80,81,82,83,84,85,86}` | 11 |

  Of 128 full-output differences, the remaining 18 stay error/semantics, exit 2:
  `inline/1457/boundary/nonfinite` gains authored message/loc 1:161;
  `inline/R2/seq-write-remains-rejected` keeps loc 7:17 but requires Map, not Map
  or Relation; `inline/aggregate-and-component-locations/case-{13,17,37,41,49,53,61,65}` moves 1:1 to 6:3
  and changes Map/Relation/Seq repair text to Map/Seq;
  `inline/aggregate-and-component-locations/case-{21,45,57,69}` changes only that text, retaining 6:3.
  `inline/nested-aggregates-and-sync-ownership/case-{72,74}` already failed static aggregate bounds; the relation
  index now reports property 6:3 instead of no location;
  `inline/nested-aggregates-and-sync-ownership/case-{73,75}` anchors the same existing failure at use 2:3
  and retains the part.Capture cause at piece.fsl:6:3.
  Other envelope fields remain unchanged; no output drift is excluded.
  Exit-1 refinement/trace verdicts are not input errors.
- Changed (#1464): the unknown authority tool regression walk also runs
  `approval create --kind html`, which panicked at the AI lowering lookup
  before the #1464 fix. Command behavior is unchanged.
- Changed (#1467, breaking): already-admitted recursive Set/Seq expressions share checked
  expected-type construction, equality, conditional joins, methods and binders,
  including Struct, Option/nested absence, nested collections, relation and existing
  Map values. Sets use logical deduplication; Seq keeps order, duplicates and live
  length. Carrier/capacity, presence, partial-operation and storage-bound rules stay
  unchanged; temporaries are not clipped. No grammar, admission or Map syntax is added.
  **Breaking public Rust library API:** `SymbolicValue::Collection.entries` changes
  from `Vec<(Vec<T>, T)>` to `Vec<(Vec<SymbolicValue<T>>, T)>`; Rust constructor and
  field consumers must adapt to typed recursive keys. The private CollectionEntries
  alias does not hide this public break. The exact `Bindings<T>` BTreeMap alias,
  root export and SymbolicValue variant census remain unchanged, not every Rust API.
  This is not a CLI/Kernel JSON schema or assurance change; existing errors,
  locations, invalid-enum projection and solver-free engine boundaries remain.
- Changed (#1472, breaking removal): whole Set, Seq, Map and relation values
  require matching carriers, including Seq capacity and both Map keys and values.
  This resolves #1627: narrow-to-wide as well as wide-to-narrow whole Set copies
  now fail check with the existing located assignability error (exit 2), rather
  than disagreeing across engines. Equal-range aliases remain compatible through
  symbolic retagging; scalar/Option numeric compatibility and scalar arguments
  are unchanged. Refinement resolves each model's numeric types independently and
  retains its existing type error. Upstream #1642 recovers authored map spans, with
  unknown 1:1 retained only without an authored span. Migrated unchanged-source
  anchors are runtime1:40/1:45 map tokens, standalone4:3 map and inline2:3 implements,
  formerly1:1. Same-name struct-field and enum/range collisions remain open pending
  #1599's shared merge rule and Type-kind handoff. Int is outside the finite-carrier gate;
  existing literal compatibility and inferred Seq capacity are preserved. Static
  implements-map type failures in mutate now use the shared check-invalid path:
  whole-container range mutants move from errored to invalid, outside kill_rate
  and without oracle_errors in the gate. Solver/evaluation errors and budget
  cutoffs retain their existing classifications. Independent archive builds of
  79687655 and 3c99c93b twice checked 606 corpus and 16 original inline
  forms: zero corpus removals, zero error-to-error diagnostic changes; three inline
  ok-to-error removals are FALSE_VACUITY/CondMap (Map keys 0..0 vs 0..2, 20:41),
  the #1522 MAP conditional (keys 0..2 vs 0..1, 7:35), and SET_WIDENING
  (Set 0..2 to 0..4, 6:48). Also removed are admitted whole Seq maps in runtime
  SeqWide to SeqAbs, SeqWider to SeqGuarded, CLI SEQ_WIDE_IMPL to SEQ_ABS and
  inline seq_req(-1): capacity 2, model-local elements -1..2 or 0..3 vs 0..2;
  former refinement failures now reject statically. Another 18 literal-boundary
  forms retain identical check outputs. Set/Seq conditional inference keeps an
  already known finite carrier/capacity across let, without literal materialization.
  Six additional breaking removals, measured with 18 bridge forms, are both arm
  orders of Set0..2→0..4, Seq0..2→0..4 at capacity2 and Seq capacity2→3 with equal
  elements (all8:3). Their standalone/same-carrier controls still check; nine
  total ok-to-error forms were measured. Literal materialization remains #1467.
- Changed (#1473, breaking removal): DB lowering rejects unknown compatibility-rule names with an
  authored semantics error before emitting a model or artifact. Kernel export no
  longer accepts these malformed documents; file-based fmt, lint, and migrate now
  return exit 2 instead of exit 0. Commands retain their established lowering-error
  locations, including positions in the message rather than a loc field for
  fmt/lint/migrate. Check and Worker specialized-validator envelopes, all twelve
  valid rule names, default selection, repair-tool carve-outs, and DB-only findings
  remain unchanged. The rule vocabulary is shared by fsl-core and fsl-tools. Repair: spell each
  `check compatibility` rule as one of the twelve valid names.
  The base/head check sweep covered 595 file-backed forms with no ok-to-error
  transitions or output/exit drift; exact commits, binary hashes, and commands are
  recorded in docs/design/DESIGN-db.md.
- Changed (#1480, breaking removal): `check`, every `verify` engine, LSP and Worker now
  refuse every integer range bound the symbolic engines cannot execute, with a located
  `semantics` error, exit 2. Refused: unplanned dependent bounds (index, field, `size()`,
  `if`, `min`, `/`, `%`, `old`, aggregates); state-dependent bounds in `ensures`,
  `reachable`, `terminal`, `until`, leadsTo P/Q, `decreases` and `helpful` arguments;
  bounds reading a `let` (even literal), pattern binding, Int state, non-literal Seq
  element or non-literal Set literal item; in a scope with no certified read frame,
  bounds reading an element of a state Set that some write can give a key read from
  state; a dependent range under an `old(...)` with no frame left, which includes an `old`
  inside `P` of `P unless Q`; and intervals that do not fit `i64`. Forms that reported a verdict before
  reaching the bound lose it: an earlier violated property, `--property` or
  `--exclude-property`, `--depth 0`, and explicit-engine verdicts on these forms.
  No size cap is added. `conformance`, `analyze`, `explain`, `diff`, `refine`, `testgen`,
  `mutate`, `sweep`, `scenarios`, `html` and `ledger` refuse these forms too. `kernel`, `typestate`,
  `lint`, `migrate`, `fmt` and `document claims` do not, and `replay` keeps loading them
  by design, including a dynamic aggregate form it used to refuse. See
  `DESIGN-check-executable-forms.md`.
  Breaking-removal sweep: `fslc check <form>` with binaries built by Rust 1.98.0, each in its
  own `git archive` tree, base `e3c19a39c9c3bf5c315c6002fd18114aa22cd654`, head
  `f101e7a93cbb50ad85e3531a1d27314559735311`, over 20,179 forms: 646 `.fsl` files (`specs/` 23,
  `examples/` 195, `rust/fslc/tests/fixtures/` 428) and 19,533 models captured while running
  the workspace tests on both sides. `ok`→`error`: 649, none under `specs/` or `examples/`:
  495 `'x' is not an integer const`, 85 `binder bound is not a static integer`, 20 bounds that
  overflow or do not fold, 16 init statement `forall` bounds reading state, 11 declared `Bool`,
  10 typed binders over the builtin `Int`, 8 over a Struct, 4 negative `within`. Corpus files:
  `explicit_init_range_assigned.fsl`, `explicit_init_range_unassigned.fsl`, and seven head-only
  files under `issue_1480_temporal_outer/`. Four of the 85 are a `leadsTo` wrapper bound reading
  a name that is both a state and a const, where the base read the const and returned `violated`
  on bmc and induction.

  Forms the engines used to refuse now run where the rule says they can: a dependent
  bound in a fair guard, in a sticky helpful guard, in a `trans` body with a partial
  operation, under a static outer binder, and in an invariant beside a failing
  `reachable`; binders over several-item Set literals and over state Sets with non-empty
  or written values; and a run with `--property` whose read frame proves the unselected
  bound. A frame that cannot prove it still refuses.

  `Bool` is now a reserved type name. Breaking removal: `enum Bool`, `type Bool` and
  `struct Bool` are refused at the declaration, standalone or in a composed component,
  where a typed binder and an expression used to read the name differently. Breaking
  removal: a typed binder over the builtin `Int`, including a `leadsTo` wrapper, is refused
  at `check` with `type 'Int' is not a finite scalar`, where every engine already refused
  it. A spec that declares `type Int` or `enum Int` keeps its binder in every scope.

  With `--property` on state that really escapes, bmc refuses a read that needs the bound
  with `[eligibility]`, and a binder over a state Set whose frame could not be rebuilt with
  `[planner]: missing-finite-fact`; induction reports the escape, and explicit reports the
  selected property.

  A form that runs out of memory enumerating a range now refuses with a `[resource]` error,
  exit 2, instead of a panic with empty stdout. A panic in the Z3 bindings, which is what
  running out while building solver terms produces, refuses with a `[solver-panic]` error
  carrying the panic text, exit 2; it says no verdict was reached and does not claim a
  cause. No size limit is added.

  Reject two previously check-admitted forms before engine dispatch: init statement
  `forall` integer-range bounds with free state reads, and typed binders over a named
  Struct in an expression, action or `leadsTo` wrapper, which engines already refuse as
  non-scalar. Diagnostics retain owning source locations. Supported init RHS reads,
  Bool, Enum and Range binders and ordinary and collection Struct values are unchanged.
  Range admission runs after existing init ownership and read checks, preserving their
  earlier diagnostics and component provenance. Already-refused init-statement typed
  Struct domains retain their complete unlocated finite-scalar error.

  Static binder ranges share successful checked constant folding across aggregate
  normalization, symbolic verification and static leadsTo and Monitor bindings, including
  Euclidean division and remainder, `min`, `max`, `abs` and typed conditionals with
  selected-arm arithmetic. `KernelModel::constant_integer` is a new Rust API that reads
  the current constant map without caching. A plain `0..n` over a state that shares its
  name with a const now reads the state: it used to prove what the explicit engine
  violated, and now every engine reports `violated`.

  Builtin `Bool` binders keep their `false`, `true` identity in quantifiers, statement
  `forall` coverage and static `leadsTo` wrappers. Breaking admission removal: negative
  constant `within` deadlines fail at the owning property before engine dispatch; zero
  and positive deadlines are unchanged. Property-keyed JSON and bounded unranked liveness
  assurance are unchanged.

  Add the Rust-only integer range planning API under `fsl_core::binder_range` and
  `KernelModel::plan_integer_range`. Plans keep name provenance and prerequisite ownership,
  and full `i64` cardinality uses `u128` without enumerating candidates.

  Unfiltered outer `leadsTo` range binders can use already bound wrapper locals with
  checked integer arithmetic and typed Boolean conditions, keeping declared binding types,
  expansion order and diagnostics. Free state bounds are refused even when a const shares
  the name. `within` remains a nonnegative declaration constant; collection and filtered
  wrappers and explicit liveness remain unsupported.
- Changed (#1481): Init ownership distinguishes disjoint fields of struct roots and map entries, including finite forall and existing scalar or nested Option fields. Shapes requiring the new granularity need full definite coverage of every may-assigned root. In those newly admitted shapes, reads before full coverage are rejected. When field-written roots are complete at the end, the named field-read gate runs before nonfield-read checks, even if the field read appears later. Nonfield refusals retain the legacy ownership error and location unless the field gate refuses first; incomplete field-written roots also retain the saved ownership refusal. Never-assigned roots remain free. Legacy-probe-accepted forms keep their ownership admission and symbolic free components. Before #1521, refinement could default missing components; it now refuses incomplete assigned roots, while bmc and induction retain free-value verdicts. Partial-component enumeration is future work (#1579). Symbolic shadow resolution in #1515 is unchanged. New proofs conservatively leave outer-binder-dependent domains/key bounds unresolved.

  Concrete legacy behavior is not uniformly preserved. Resolved entry-field coverage can make complete single-field-entry maps constructible, add a partial-component suffix, or remove a complete map from missing-root names. This includes constant/enum keys and mixed whole-entry/field branch meets. A complete-by-superset key range now reaches the existing out-of-domain refusal; repeated fixed-entry field writes with conflicting iteration values now reach the existing vacuity refusal instead of missing assignment. Relational repeated-write constraints themselves are unchanged, but concrete refusal stages, kinds and messages can change.

  The effects extend beyond explicit verification and Monitor: deterministic refinement constructors on both sides, `verify --engine auto`, conformance vectors, test generation and requirement-trace validation share concrete initialization. Constructor errors can become correct refinement verdicts, vectors or generated tests. Auto can change from bounded BMC fallback to unbounded explicit closure, and its out-of-domain error can lose BMC's location text. Requirement traces can become executable, changing check/proof/testgen outcomes or exposing a genuine acceptance failure. Kernel replay/conformance output alone does not validate declared acceptance cases.

  Overlapping-write refusals are preserved. On shared ownership refusals, the legacy-concrete preflight preserves earlier read/domain failures; named field-read and #1518 diagnostics retain priority, so not every concrete message is preserved. The runtime remains solver-independent.
- Changed (#1492): ledger ancillary I/O failure tests derive expected messages
  from the platform's `std::io::Error` for the same missing path, so the tests
  also pass on Windows. Command behavior is unchanged.
- Changed (#1498): isolate error-envelope approval fixtures from runner Git
  line-ending defaults, preserving strict stderr assertions on Windows. Run all
  Windows native solver, verifier, and CLI test binaries with `--no-fail-fast` so
  the next main-push product gate reports every remaining failing binary.
- Changed (#1515, breaking removal): executable local bindings with the same
  name as any `const`, including non-integer constants. `check` now rejects
  quantifier and aggregate binders, statement `forall`, `leadsTo` binders, action
  parameters and `let` bindings, and `def` parameters, including their requirements
  and compose-component forms. Rename the local name and its uses, for example
  `forall N in 0..1 { N <= 1 }` to `forall k in 0..1 { k <= 1 }` when `const N`
  exists. Diagnostics name the binding and locate its existing AST owner, not
  the binding-name token; qualified component constants and legal state-name
  shadowing remain available. General dependent range bounds are not added.
  The original pre-migration regression inventory includes 152 removed inline
  forms: 126 compose-capture
  forms (#1442; e.g. `const x = false` with `action go(x: Bool) { requires x }`),
  24 nested-aggregate forms (#1463; e.g. `const x = false` with
  `sum(x in 0..1 of count(y in 0..x)) == 3`), and 2 direct/composed helpful-argument
  forms (#1333; `const c = 0` with local `c` and `helpful step(c)`). Rename the
  local name and its uses; 126 fresh compose controls remain accepted. Five
  partial-init forms (#1481) remain error→error but now report const-shadowing
  before ownership, changing diagnostic priority and owner locations. The
  same rule also rejects documented implicit process c, next-edition entity
  instance first-letter names and relation actor-instance params, at the source
  process/entity/event declaration even when no local declaration was written.
  Rename a conflicting const in those implicit-name cases. Hidden KPI projection
  binders instead freshen only on collision: only then does the projection's
  Kernel AST JSON name change; the public Kernel export is unchanged.
  Nested init with const i=1 and
  `forall i in 0..0 { forall j in 0..i { m[j] = true } }` is an additional removal:
  it checked on base 46031415 and now refuses the outer forall at 5:10. Rename the
  local and its bound uses. Def-call argument binders are checked before expansion
  even if an unused formal erases them, with the same owner/message as retained
  arguments. Fresh native check at base 46031415 versus measured ancestor 65bf2876
  finds zero removals/other output differences across 603 corpus forms, with the
  original 152 inline removals, 126 fresh accepted and five diagnostic changes.
  Another 70 removal witnesses and 70 renamed controls cover the additional
  boundaries; both hidden-KPI cases remain accepted. The measured ancestor has
  identical Rust implementation/corpus sources to the documentation update.
  The design note records this bounded evidence, not every removed program.

  This prevents symbolic and explicit engines from producing opposite
  verdicts when a range bound reads a name shared by a local binding and a global
  constant. The frontend rejects the collision before verification, preserving
  identical complete error envelopes across `check`, BMC, induction and explicit,
  and checking component names before prefixing and `def` parameters before
  inlining. Engine range-bound evaluators are unchanged.
- Changed (#1517, breaking removal): root synchronized arguments are typed against their formals before substitution, including arguments for formals the component action never reads, in root header parameter, state, constant and enum context. Numeric carriers stay compatible, and a range mismatch stays a verification result only when the component action reads the argument. Root errors point at the argument with a bare message; action and Kernel metadata do not change.

  These forms passed `check` before and are now semantic errors (exit 2):

  - Unused formal, bad argument: `true` or `part.h[0]` for `a: K`; an unresolved name (`nosuch`), including one bound only by an additional-item `let`; `part.Red` or `Color.Red` (write `Red`); another component's enum value (`other.tone`) for a `part.Color` formal; a non-Bool condition (`if 1 then 0 else 0`); a root `def` with a badly typed argument (`rp(true)` for `def rp(v: Int)`).
  - Used formal whose substituted body happened to type: `put(a in 0..1) { flag = a }` or `put(a: K) { flag = a }` with `flag: Bool` and `true`; `put(a: K) { m[a] = true }` with `m: Map<C, Bool>` and enum `R`.
  - Duplicate formals `(a: K, a: K)` with `true, 0`.
  - `internal part.put` with `part.put(0)` where the component declares `a: Int`, `a in 1..0`, `a: Nope` or a struct-typed `a: T`.

  Migration: pass a value of the formal's type, using a header parameter when it comes from outside the root, or correct the component. There is no automatic migration.

  The 610-form on-disk corpus has an empty transition set: base `5564b25ccb37e3477311fde1dae5eb6298eb6b2d`, head `d19e9939d8079042ddc768afe8550c4f402743dc`, `fslc check` stable and FSL5 opt-in, and `fslc kernel --kernel-version 1|2`, twice per side. DESIGN-sync-argument-root-validation.md has the command and the exact forms.

  A conditional with a Bool, ranged or named expected type now checks its arms against that type, so `m[if c then 0 else true]` with `m: Map<A, Bool>` names `Named("A")` instead of `Range(0, 1)`. Other conditionals, including an `Int` expectation, still check arms against the first arm's type. This changed no admission result on the measured forms.

  Rust API: public `SyncRef` requires `arg_spans`, and fallible `into_validated_syntax` validates before raw conversion. The three migrated CLI callers each add one full core validation.
- Changed (#1518, breaking removal): check now rejects reads in init forall bodies
  or filters of roots written by the same forall, except locations already written
  at the current binder key in the current iteration. This root-based removal
  includes correct forms with agreeing base-engine results, as well as
  prior-forall other-key and nested-forall reads already refused by explicit and
  Monitor with `before it is assigned` and
  `nested forall in init is not supported`, respectively.
  Fixed targets in empty or not statically provable nonempty domains no longer
  count as assigned, preventing explicit and Monitor from accepting defaults.
  Even after `flag = true`, a fixed-target write under `where flag and k == 0`
  is conservatively refused with `init does not assign` despite a nonempty runtime
  domain, not given an incorrect verdict. The 597-form corpus sweep found 0
  ok-to-error transitions.
- Changed (#1521, breaking removal): `refine` refuses incomplete assigned init roots on both its
  implementation and abstraction free-root paths instead of silently using defaults
  for unwritten fields, map keys, branches, or unproved forall coverage.
  The error explains that bmc and induction still return verdicts using free values.
  Never-assigned scalar roots remain enumerated. Singleton fixed-target assignments
  with a static, state-independent filter proven nonempty remain accepted;
  state-reading filters do not prove nonemptiness and cannot establish full coverage.
  Check, verification engines, and Monitor behavior are unchanged.
- Changed (#1522, breaking removal): Different-bound relation values in conditionals, Set/Seq items,
  action/init/let assignments now fail check/verify/replay (located semantics, exit 2),
  even narrow→wide, due to engine disagreement. At the #1522 measurement,
  equal-bound aliases, scalar/Map compatibility, nominal enums and self-relation
  identity were preserved, and numeric `Set<A>` → `Set<B>` widening still checked
  with symbolic disagreement tracked as #1627. Subsequently #1472 requires equal
  whole-container carriers for Set/Seq/Map (including Map keys and values), rejects
  that Set widening statically, and resolves #1627. Scalar compatibility remains.
  Equal-bound relation literal engine support (#1524) stays with #1467. Corpus
  removals: 0; inline acceptance removals: 10; one #1459 error changes message/location.
  Repair: declare the relations that meet in one conditional, collection or
  assignment with endpoint types of the same bounds.
  See `docs/design/DESIGN-relation-endpoint-compatibility.md`.
- Changed (#1528, breaking removal): `leadsTo` declaration binders now participate
  in def free-name capture checks for before/after expressions, `helpful`
  arguments, `decreases`, and `within`, in direct specs, compose components and root
  properties, and dialect properties lowered to `leadsTo` (such as requirements).
  Previously accepted capturing calls reject with exit 2, `kind: "semantics"`,
  and `would capture variable` at the call. Rename the declaration binder; no
  automatic rename or bypass is added. Binder ranges and binder-where expansion
  are unchanged, as are constant/enum `helpful` arguments from #1580.

  The state-reading `within` control already failed closed: exit 2 / semantics
  is unchanged, but for the acceptance source `unknown constant 'c'` becomes
  `predicate 'Bound' call would capture variable 'c'; rename the binder at the call site at 9:41`.
  The old check loc 4:23 is replaced by the call position embedded in the message;
  the fixed check/BMC/induction error envelopes omit structured loc and agree in full.
  This is an error-to-error diagnostic change, not a false-green baseline.
  When a `leadsTo` also contains an already-invalid pattern-scope escape (#1547),
  `would capture` can likewise precede that rejection; exit 2 / semantics is unchanged.

  Separately built `git archive` trees for base
  `08d9656c5548a7ae5cbd2156880fdf52f9d0b8fc` and implementation
  `c1a7d126071d8ac0600ffcd13d3ea9b3f0b7f286` checked all 601 `.fsl` corpus files
  under specs, examples, and CLI fixtures, plus the one migrated inline source
  (602 forms). All corpus stdout/stderr bytes and exits agree: zero ok-to-error
  transitions and zero error-to-error diagnostic changes. The inline source is
  the only ok-to-error transition, a declaration binder capturing a def-body free
  state name in a decreases call. No unrelated acceptance is removed.
- Changed (#1545, breaking removal): native `check` and the shared Worker check stages validate every
  AI-project component before reporting success. Undeclared authority tools and
  unknown hard-contract rules now produce the same error diagnostic and status 2
  as project `ai check`, rather than an analyzed-project success.

  Shared component validation rejects tool-less
  components, including empty authority blocks, in `ai compat`, `ai replay`,
  project `ai check` and native/Worker project checking. Standalone no-tools
  envelopes from #1546 are preserved on check and specialized-preflight paths;
  the lowering-only duplicate-fallback priority exception is measured below.
  Generic project checking no longer accepts
  the fixtures `error_envelope_ai_project_unknown_tool.fsl`,
  `error_envelope_ai_project_invalid_rule.fsl` and the tool-less Markdown project
  `error_envelope_literate_ai_project.md`. The first two report their existing
  unknown tool/rule semantics errors; the Markdown project reported type,
  `ai_component 'EnvelopeParityLiterateProject' declares no tools`, block loc1:1,
  exit2 (historical #1545 frame; #1574 now reports document loc5:1).
  #1574 corrects project semantic coordinates to document positions;
  LSP project-component diagnostics remain separate (#1575).
  Measured using independently built B
  `d37a215e8df6c00222e6aafd94bc9b556c445521` and H
  `99810256faca086360c6ca4233f935b9b56b1ec8` in their own git-archive trees:
  `<B-binary> check <relative-path>` in B and `<H-binary> check <relative-path>`
  in H, all 597 FSL and 30 Markdown corpus files plus twelve new inline occurrences
  per tree, 1,278 checks, cache/unstable overrides unset and no excluded output
  fields. Four Markdown files have FSL fences; bash-only examples/ai/README.md is
  also covered as a negative control. Corpus ok/exit0 to error/exit2 removals are
  the three named fixtures. Four new test-inline
  occurrences are also breaking removals: tool-less-component projects in CLI and
  Worker-shaped tests (two), an empty-authority-only component in a project (one),
  and an invalid later component after a valid first one (one). The three tool-less
  occurrences reported type at block loc 1:1 (historical frame; #1574 now reports
  document loc2:3 for the indented CLI project forms, while the first-character
  Worker case stays 1:1); the later unknown tool reports
  unlocated semantics. No existing inline FSL was rewritten; other check-only
  output/status observations are unchanged. Lowering-only analyze on 18 AI corpus
  inputs plus four explicit shapes, followed by scenarios (depth2/deadlock ignore)
  and explain on changed shapes and a compose control, produced
  30 command-form pairs × 2 binaries = 60 observations.
  Analyze22/scenarios4/explain4 each have one ok-to-error, zero error-to-ok and two
  changed error envelopes. An undeclared forbidden X hidden by selecting only
  `tool_authority` now reports semantics/`unknown tool 'X' in authority block`/no loc/exit2 instead of
  success. Duplicate fallback x without tools changes
  type/`ai_component 'DupNoTools' declares no tools`/loc1:1 to
  semantics/`duplicate fallback reason 'x'`/no loc; with a tool, unlocated
  semantics/`duplicate action 'fallback_x'` becomes
  semantics/`duplicate fallback reason 'x'`/no loc (both exit2).
  AI-component compose use stays rejected with
  `compose use must reference a spec at 2:3`, loc2:3, exit2.
  These are explicit lowering diagnostic/priority changes, not broad
  envelope preservation. Declare a tool or remove an unneeded component and
  correct unknown authority/rule references.
- Changed (#1546, breaking removal): `check` and every model-loading command,
  including fmt, lint, kernel, diff, all verification engines, scenarios and
  testgen, reject empty spec/Kernel enums even when unused, with kind type,
  `enum '<Name>' has no members` and CLI exit 2. Where the command publishes loc,
  it is the declaration start (symmetric when present); a compose child uses the
  #567 parent-use loc wrapper
  with child path/position in its message. Domain keeps its earlier semantics
  error. Tool-less standalone `ai_component` inputs, including empty authority,
  are rejected at the kernel boundary by check, ai check, BMC, induction,
  scenarios, testgen and Worker checking with exact
  `ai_component '<Name>' declares no tools` and component loc; one tool with empty
  authority remains valid. Existing bad-rule/unknown-authority errors keep their
  priority. This does not change ai compat, ai replay or ai_project paths; their
  tool-less validation gap remains #1545. Column-less dbsystem databases (schema0,
  no tables or only an empty table) are also breaking removals: check, BMC,
  induction, explicit, scenarios, testgen and kernel report exact
  `database '<Name>' declares no columns`, type/exit2 and database loc. db check
  reports that error only when it reaches lowering; static compatibility findings
  still come first (an artifact reading an absent column can yield violated/exit1,
  as on base). DB fmt/lint keep their existing semantics/exit2 rendering with a
  location suffix in the message and no loc field. Empty `EmptyImpl` / `EmptyAbs`
  refinement inputs now
  fail before mapping elaboration: kind/exit are unchanged, message changes from
  `enum conversion endpoint 'X' has no members` or
  `enum abstraction endpoint 'X' has no members` to `enum 'X' has no members`, loc
  moves from the mapping file to the impl/abs declaration, and the later mapping
  span object is absent. See the
  [boundary decision](docs/design/DESIGN-enum-member-identity.md#nonempty-enum-declarations-1546).
- Changed (#1547, breaking removal): native `check` rejects pattern variables
  outside their true continuation, including disjunctive/comparison/implication
  escapes and non-conjunctive `requires` exports. Ten logical scope consumers and
  the DDD frontend share one operator policy. Collision reservations remain
  separate and base-equivalent, including compose guard-name inventories so
  accepted output retains authored names; #1573 remains separate work. Legal
  `and`/`=>` RHS bindings and conjunctive guard exports remain supported. Earlier newly invalid
  references can precede legacy collision diagnostics, still exit 2/semantics.
  Expression-if arm inference can report unbound then/else/nested references at
  `6:32`/`6:39`/`6:48` before the former condition collision `6:12`; renaming only
  the arm reference restores the complete base packet `6:12`. When only an
  out-of-continuation pattern would capture a free name in a def body, the
  predicate-capture error becomes a located unbound-reference diagnostic. This
  class preserves exit 2/kind semantics while changing message/possibly location;
  it is not limited to the three examples measured below. Outer
  not/quantifier/aggregate escapes were already refused, not new removals. For a policy
  allowing `none` but requiring payload 1 when present, use
  `(o is some(v)) => v == 1`, equivalently
  `o == none or (o is some(v) and v == 1)`; `and` alone instead rejects `none`.
  The own-tree `<binary> check <path>` sweep from
  `e0b4cbae39569de8008a8eb049588aa84d8c1a42` to measured
  `3ae20a93296d36089f7860957a0a0b601f96932a`, repeated twice with complete
  stdout/stderr/exit comparison and no excluded fields, found zero removals/output
  changes in 601 corpus inputs (598 baseline plus three new fixtures supplied to
  both trees) and 1,777 existing complete static Rust-test literal occurrences.
  Authored 147 routes have 57 removals; reviewed 110 inputs have 24, totaling 81, enumerated in
  [the scope design](docs/design/DESIGN-pattern-binding-scope.md#3-true-continuation-enforcement-1547).
  The historical 18 message-change packets are unchanged: 15 collision-priority
  cases with mechanical collision-return controls (seven tested location changes),
  plus three def-call capture examples. A nineteenth occurrence is the authored
  copy of the already-reviewed d06 source, with identical before/after packets,
  not another diagnostic class. No acceptance expansions or repeat differences
  were observed in this bounded sweep. Another 1,783 interpolated/incomplete
  document candidates were inventoried, not executed; runtime-generated fixture
  graphs are not covered.
- Changed (#1552, breaking removal): `check` now rejects previously accepted unsatisfiable
  requirements `rule` guards that reuse an entity field's name as an `is` pattern
  inside a quantifier body or aggregate value. The conflict check reads that name
  as the field, not the payload, matching the event's lowering (#1561 tracks the
  pattern-name rule itself). For example,
  `amount == 0 and (exists x: Amount where x == 1 { some(x) is some(amount) and amount == 1 })`
  now fails with exit 2, `kind: "semantics"`, and
  `rule_conflict_unsatisfiable_guard`: the event requires the field to be both 0
  and 1. No spec or example in the repository has this removed shape; the
  597-form corpus sweep showed no acceptance transitions. Migration: to test the
  payload rather than the field, replace `some(x) is some(amount) and amount == 1`
  with `some(x) is some(v) and v == 1`, using a pattern name that is not a field;
  with the outer condition unchanged, this means the field is 0 and the matched
  payload is 1, and the rewritten example passes `check`.
- Changed (#1561, breaking removal): requirements now reject `is some(name)` declarations that collide with an entity
  field or relation role before field substitution and rule-conflict analysis.
  Rename both the pattern and its continuation to a fresh name: replace
  `some(1) is some(amount) and amount == 1` with `some(1) is some(v) and v == 1`.
  This formerly accepted input is a breaking removal; three already-refused #1552
  variants now receive a located pattern-shadowing diagnostic instead of a rule
  conflict. Legacy guards, assignment RHS, rule properties, measures, decision
  assumptions/rows and concepts (even unused) share the
  gate. Event-input and quantifier/aggregate shadowing rules remain unchanged.
  The own-tree `FSLC_CACHE=off <binary> check <form>` comparison
  ran twice on base `f8e3726e2ead231c89cd4f52eef3dd5d652c774d` and measured head
  `13676f0bca83b2433cc974468bbb00c59aef25a9` (binary SHA256
  `0cab1c914a05344d8958a6f9bd5ce5a17618f87f9f7e74e279b449fbca47f314`):
  610 corpus forms unchanged, all 121 supplemental forms included (original63,
  prior49, added fresh1, decision/concept6, exact review2). The 27 case-addressed
  inline removals, not necessarily byte-unique programs, are:

  - `control_is_pattern_field_name_satisfiable_matches_base` (1).
  - `expression-0`, `expression-1`, `expression-2`, `expression-3`, `expression-4`,
    `expression-5`, `expression-6`, `expression-7`, `expression-8`, `expression-9`,
    `expression-10`, `expression-11`, `expression-12` (13).
  - `argument-1`, `argument-4`, `argument-9` (3).
  - `event-set`, `when`, `set`, `never`, `property`, `measure`, `role` (7).
  - `decision-assuming`, `unused-concept`, `review-original-collision` (3).

  The original and/implication/quantifier contradictions plus `argument-7` and
  `decision-row` are five error→error diagnostic changes, not extra removals.
  All 2,924 executions have no repeat differences or excluded fields; no other
  required-test inputs were rewritten or omitted. Later integration commits are
  not this measurement. (Refs #1561)
- Changed (#1569, breaking removal): legacy requirements transition guards such as
  `exists c: Amount { stage(c) == Approved and amount == c }`, with a process field
  `amount`, now fail `check`, `kernel`, and `verify` with exit 2, `kind: "semantics"`,
  and `stage(c) refers to type 'Amount', which has no process`. A field named `c`
  is not required. Previously, `check` accepted the guard and `verify` reported
  `verified`: capture-avoiding substitution renamed the non-entity binder without
  renaming the stage operand, so `stage(c)` silently read the transition's own
  instance. The same invalid guard consistently using binder `d` already failed.
  No checked-in spec or example has this removed shape; the 597-form base
  `1844fa74` / head `7b9b34b3` sweep showed no corpus acceptance transitions.
  Migration: `stage(...)` takes an entity parameter or binder. To read the
  transition's own stage, use
  `exists n: Amount { stage(c) == Approved and amount == n }`, keeping `c` for the
  entity and a different name for the non-entity binder; the rewritten guard passes
  `check`.
- Changed (#1573, breaking removal): pattern-name reuse after matches inside
  `not`, expression-if conditions/arms, where-to-body/of, and statement-if
  conditions or arms flowing into later statements. Rename the later binding
  and its reads; the four standalone/compose #1442 nested-if reuse routes are
  included, with original refusal inputs retained and only the inner `u` renamed
  to `w` for accepted preservation controls. Visibility is not broadened, opposite
  statement arms remain independent, and candidate-local names do not escape.
  Direct-spec init checking applies these rules across all init blocks and inline
  initializers, preserving initializer/const/map-key diagnostics. Composed init
  still has a same-source fragment/inline→init reservation gap; common init can
  inherit unrelated Fresh decisions. Rebinding remains illegal. False verified
  results belong to #1622; compose init-fragment reservations will be tracked in
  a separate issue filed after merge. Expression then-to-else
  reuse is also rejected while the symbolic expression evaluator shares bindings
  (#1577). For sync actions and non-init paths, compose gives independent sources
  distinct hygienic pattern names:
  the guard-output examples change `v` to `__v_substitution_0/1`, preserving their
  explain result and verification verdicts. Two-source statement payloads no
  longer produce the false violated verdict caused by one source's value being
  used by another; check stays accepted and the correct verdict is holds, while
  a genuinely false control remains violated. Evaluator lifetime work remains
  tracked in #1622, including the still-unrepaired separation of component init
  patterns; the general init-source separation guarantee is not made here.
  Refinement map-expression collisions can precede preserved-progress capture
  errors; a later-name-only repair restores the base capture packet. Unknown
  zero-width map-expression error spans now use the available map declaration span,
  without replacing meaningful expression spans.
- Changed (#1595, breaking removal): `fslc refine` preserved progress refuses property pattern names matching free impl names from lexically
  used maps, even outside readable continuations; prior `or` exit 0 `refines` becomes exit 2/type.
  Used-map RHS patterns may not reuse any property binder or sibling used-map pattern name.
  Rename the abstract or mapping pattern binder to repair these located errors at `respond`.
  Ordinary source binding scope is unchanged. Refs #1595.
- Changed (#1599, breaking removal): refinement rejects conflicting same-named type definitions, including unequal
  resolved domain bounds, at the refinement/implements declaration (type, exit 2).
  Symmetry annotations do not participate; equal definitions retain the impl entry.
  With preserved progress, different enums may not share member spellings, even at
  equal ordinals; without progress, typed conversions/abstractions remain accepted.
  Breaking removal: refine_mapping_boundary changes from refinement_failed /
  abs_state_mismatch (exit 1) to error / type (exit 2). Use distinct type names.
  Mutants changing the bounds of a type named identically in the abstraction are
  invalid under #1472's static-type classification, not oracle errors or kills.
  The rule includes entity instances: impl_spec_extra's default check/verify now
  rejects Item=1 versus Item=3 with type/exit2 instead of refinement_failed/exit1.
  Audit a52bef4931adc67ea2b1fdca827e2318b4ae25ab→9eaaea7d1c526ede6a13408ea414616ddadccf69:
  check610 corpus/39 next controls,60 mappings,20 inline/2 expression triples/39 cells,
  full JSON/stderr/exit twice. Existing corpus check ok→error is empty. Inline
  removals: original D-bound issue1296/1302, inline collision and governance collision;
  verify follows. Refine removals: cases1/2a/3a/3b, domain_bounds/domain_lower,
  kind_change/struct_named_type, matching-ordinal/chain and shared-member expression
  progress. Renamed controls pass. Original2 runtime/2 CLI/3 mutation diagnostics
  now prefer declaration rejection; invalid/gates stay unchanged. Authored1:1 is
  the full refinement keyword token, not fallback; map companions retain map spans.
  Existing type errors gain loaded operand identities; boundary/entity change class.
- Changed (#1600, breaking removal): in next-edition requirements, a relation role, a
  resource name, and a measure name may no longer contain `__`, extending
  #1256's `__` reservation to these three kinds of next-edition declaration. `fslc check` rejects
  `Claim belongs to User as __owner via approve`, `resource __r: Amount = 0`,
  and `measure __m = count Claim in Approved` with `kind: "name"`, exit 2,
  located at the declaration, and the message
  `relation role name '__owner' contains '__', which is reserved for the names FSL generates ...`
  (or `resource name`, `measure name`). Before, the declared-name walk missed
  these three, so they passed check. No spec or example in the repository
  declares such a name. Migration: rename the role, resource, or measure so that
  it does not contain `__` (for example `__owner` to `owner`), and update the
  references to it.
- Changed (#1602, #1603, breaking removal): `check` now rejects `__` in goal and policy IDs,
  requirement IDs (`rule` in the next edition), scenario IDs (`acceptance` in
  legacy), forbidden IDs, decision-table IDs (`decide`), and glossary aliases,
  in both editions wherever the construct exists. These previously accepted names
  now produce exit 2, `kind: "name"`, and structured `loc`, using the existing
  reserved-separator message. Next-edition rule IDs use only their exact header
  token spans, even with deadlines or preceding comments. Explicit `requirement`
  blocks are supported and their IDs checked in both editions,
  with or without deadlines; only rule-generated copies lose the duplicate ID check.
  Known legacy-syntax limitation (also for explicit blocks):
  split-spelled requirement, acceptance and forbidden IDs such as `REQ - __1` may
  be located at the declaration keyword or a later repeated normalized spelling
  rather than the ID tokens; rejection is unchanged and that limitation is not
  fixed here. Rename IDs and aliases without `__` and update their references;
  references to generated names remain allowed.
- Changed (#1613, breaking removal): `refine` progress pulls now instantiate indexed mappings
  simultaneously in implementation scope, avoiding identity/swap stack overflow
  and an indexed chain's false `refines`. **Breaking removal:** with per-element
  maps, free abstract whole-map reads in pulled progress properties are now
  `error/type` (exit 2) at `respond`, even when an unrelated implementation state
  shares the name. The `whole-same` witness changes `refines`/0 to type/2;
  already-rejected renamed forms gain location/classification, not new rejection.
  Legal source binders, inserted implementation whole-map expressions, scalar
  maps and no-progress simulation stay supported. Migration must preserve P/Q's
  quantifiers and temporal meaning; moving `forall` across `~>` is not generally
  equivalent. The named f8→2aab own-tree audit has an empty check-removal set over
  610 corpus + 54 inline forms, separately one admitted→error refinement case;
  all 710 check/refine routes ran twice per SHA. Full SHAs, commands, counts and
  the separate crash/verdict corrections are recorded in DESIGN-refinement §4.
- Changed (#1640, breaking): reserved-name diagnostics for legacy requirement, acceptance,
  and forbidden IDs point to the first written ID token, including IDs with
  spaces around hyphens, rather than matching description text or the declaration
  keyword. ID normalization, accepted spellings, zero padding, declaration spans,
  and the public Kernel/JSON projection are unchanged. The public Rust
  `RequirementsItem::{Requirement, Acceptance, Forbidden}` variants gain an
  `id_span: Span` field; downstream constructors and exhaustive field patterns
  must account for this source-compatibility change.
- Changed (#1656): schedule the generic implementation-mutation gate across three round-robin shards while retaining
  the exact GLOBAL mutant roster, all 35 detectors, fresh per-shard builds and real empty-shard
  baselines. Require revision-bound raw outcome inventories and compatible stable-slot artifact
  cohorts before the existing required semantic-mutation aggregate can pass. The default local and
  curated-operator paths remain unsharded/unchanged; an observational same-Rust-tree CI comparison
  recorded a 35.6% lower maximum shard duration with 66.3% more job-minutes, not a causal speedup
  promise (see `docs/design/DESIGN-ci.md` for run identities and evidence limits).
- Changed (#1422, breaking removal): Causal evidence ingestion now rejects present
  `period.valid_until` values unless they are strings with the exact ASCII
  `YYYY-MM-DD` shape and a valid Gregorian date, after checking the artifact
  digest. Missing freshness remains accepted but excluded from current support;
  start/end timing exclusions and review-only assurance are unchanged.

  `causal analyze` and `causal ledger` now refuse an `--as-of` value that is not
  a real Gregorian date written as ASCII `YYYY-MM-DD` with `kind:"usage"` and
  exit 2, before reading the model or evidence. This is a new refusal: values
  such as `2099-02-30`, `2099-13-01`, `garbage`, an empty string, or
  `2099-+4-01` used to exit 0 and count no evidence as stale. Date fields are now
  read as fixed-width ASCII digits everywhere, so a signed `period.start` or
  `period.end` such as `+026-01-01` reports `period_date_unparsable` instead of
  being read as year 26.

  Sweep now checks the normalized inclusive instances/value/depth grid before
  allocating combinations or running cells, with a fixed limit of 10,000 cells.
  Overflow or an oversized grid returns an exit-2 semantics error; admitted grids
  retain their existing verification behavior.

  Dynamic JSON-log negation of `i64::MIN` now returns the existing arithmetic
  refusal instead of panicking. Replay reports `nonconformant` / `log_mapping`
  with exit 1; causal observation reports `observation_replay_failed` with exit 2.
  Representable negation, non-integer type errors, and other arithmetic are unchanged.

  Observation producers now validate generated evidence before writing evidence or
  lifecycle files. Invalid declared freshness returns the existing exit-2 schema
  diagnostic, preserving existing output files.

  `causal observe-expectations` now refuses a `--period-start` or `--period-end`
  that is not a real Gregorian date in ASCII `YYYY-MM-DD` with `kind:"usage"` and
  exit 2 while reading options, writing nothing. This is a new refusal: an invalid
  `--period-start` such as `2026-02-30` used to exit 0 and write an artifact.
- Changed (#1679, breaking removal): the model expressions listed here, whose container carriers symbolic evaluation cannot coerce, now fail `check` with the existing located assignability error (`semantics`, exit 2) instead of `cannot coerce symbolic value` on bmc and induction, evaluated or not. Three routes still pass `check` and report it; they are listed below.

  These forms passed `check` before and are now semantic errors:

  - A Map whose value is `Int` or `Option<Int>` against one with a range, for `m0: Map<K, Int>`, `m1: Map<K, A>`: `m0 == m1`, `m0 != m1`, `m0 = m1`, `m0 == (if c then m1 else m0)`, `m0 == old(m1)`, `(if c then m0 else m1)[0]`; inside a literal (`some(m0) == some(m1)`, `Seq { m0 } == Seq { m1 }`, `Set { m0 } == Set { m1 }`) and between sibling elements (`Seq { m0, m1 }`, `Set { m0, m1 }`); as a method argument (`Seq { m0 }.contains(m1)`, `Set { m0 }.add(m1)`, `Set { m0 }.remove(m1)`, `Seq { m0 }.push(m1)`); and read back out of one (`Seq { m0 }.head() == m1`).
  - A conditional arm that only an `Int` sibling related to the expectation: `a == (if c then t else b)` with `let t = Set { 0 }`, `a: Set<A>`, `b: Set<B>`; the same with a `Seq` literal, a declared `q: Seq<Int, 2>` or a differing capacity; `sa == old(if c then q else sb)`; and such an arm inside `some(...)`, `Seq { ... }`, `Set { ... }` or a method argument.

  Verdicts taken away:

  - A mismatch in a position that is never evaluated returned a verdict on bmc, explicit, induction and `fsl-bfs`, and is refused: `forall i: E { m0 == m1 }` and `exists` over `type E = 1..0`, an arm under such a binder, an action with a parameter over an empty range, and a property left out by `--exclude-property` or `--property`. There are 11 such forms in the audit grids.
  - A mismatch in a `decreases` measure is the same kind of position: bmc does not read a measure, and induction reads one only once it ranks. `leadsTo L { n < 3 ~> n == 3 decreases (if m0 == m1 then 1 else 0) }` returned `violated` on both where `n` never moves, and `verified` on bmc and `cannot coerce` on induction where an action reaches `n == 3`.
  - In the audit grids, other removed forms returned no verdict on bmc or induction. Explicit answered 82 of them, such as an unused `let u = m0 == m1`, and `fsl-bfs` answered 234.

  Migration: declare the two values with one carrier, or compare the entries (`m0[k] == m1[k]`). There is no automatic migration.

  Other forms keep their verdicts, including `a == t`, `q == a` with a differing range or capacity, `some(t) == some(a)`, `Option<Int>` against `Option<A>`, and Maps of equal carriers or equal-range aliases. A collection holding `Int` below an element that no literal expectation types used to check and then report `cannot coerce` on bmc and induction, and on explicit except in a `trans` or a read into state; it now returns a result. That is a `let` or `is` binding (`let t = Set { some(0) }` against `Set { p }`, `some(Set { some(1) }) is some(x) and Set { p } == x`), a method's result or argument (`Set { p, none } == Set { o }.add(none)`, `Set { Set { p } }.contains(t)`, `.add(t)`, `.remove(t)`, `Seq { Seq { p } }.push(t)`) and a read into state (`p = (if c then Seq { p } else t).head()`), in a guard and in an `invariant`, `reachable`, `trans` or `leadsTo`, and it decides inside a `--lemma` candidate and a map under `preserve progress` too.

  A refinement map and a scenario `expect` keep the prior rule. An `expect` is evaluated concretely, and so is a map on the refinement step, but `preserve progress` evaluates a map symbolically: `map same = m == y` for those Maps gives `cannot coerce`, exit 2, under `preserve progress`, and refines without it. A `--lemma` candidate with such a mismatch stays `rejected` with `cannot coerce`, `used` false and the main result unchanged, and one whose mismatch is never evaluated stays usable. The third route predates this change: a range-typed literal under `old(...)` keeps its own range and capacity, so `a == old(Set { vb })`, `sa == old(Seq { vb })` and `sa == old(Seq { va })` for `vb: 0..2`, `a: Set<A>`, `sa: Seq<A, 2>` pass `check`, exit 2 on bmc and induction, and explicit answers.

  The 646-form on-disk corpus has an empty transition set: base `268fdf62e6449c28c7048d04ca0030bd3f23c3b8`, head `d7d35d672e61d5cce8ee3dfea0428f892240b347`, `fslc check` stable and FSL5 opt-in, and `fslc kernel` versions 1 and 2, complete output compared. The one inline change in a rewritten test is `f1` in `issue_1517_conditional_arm_admission`, ok to error.

  Rust API: `fsl_core::CarrierCoercion` and `KernelModel::carrier_coercion` are new; `check` and `coerce` answer from them.
- Changed (#1680, breaking removal): `check` refuses a formal or input name an earlier formal of the same action took, on every route that loads the document: action, requirements action, process transition, event, domain command input, event field an effect or saga action takes as a formal, synchronized root action, inline `implements` abstraction action, and every compose component action whether lifted, synchronized or internal and unreferenced. A component reports `component "piece.fsl" failed to check (part.put: duplicate action parameter 'a' at piece.fsl:1:81) at root.fsl:2:3`, and an inline `implements` reports `implements "abs.fsl" failed to check (step: duplicate action parameter 'x' at abs.fsl:1:77)` with `kind: "type"` and `loc` on the `implements`.

  These forms passed `check` before and are now semantic errors (exit 2):

  - `internal part.put` with `part.put(0, 1)`, in either order, for `put(a: K, a: K)`, `put(a in 0..1, a in 0..1)` or `put(a: K, b: K, a: K)`. The body read the last actual and dropped the first, so swapping the arguments flipped the verdict.
  - `internal part.put` with no reference to it.

  On those component forms the other routes change too: `verify` no longer returns a verdict, `refine` no longer returns `refines`, `replay` no longer returns `conformant` or `nonconformant`, and `lint`, `fmt`, `conformance`, `explain` and `analyze` exit 2 instead of their own results. `domain analyze` and `domain expand` build no model and still exit 0 on a repeated command input.

  Migration: give each formal its own name and pass each actual to the formal it belongs to. There is no automatic migration.

  Elsewhere the message is `duplicate action parameter 'a'` and `loc` is the repeated name, not the first token spelled like it; a domain command or an event no longer adds its declaration's position to the message. A log mapping's `action ev(a, a) -> put(a)` and a domain event field no action takes stay accepted, because no actual is lost. A lifted component action, which reported no location, now uses the component wrapper. `part.put(true, 0)` for such formals now reports the repeated formal instead of the `Bool` actual; `part.put(0)` keeps its arity error. `def` parameters were already refused on every route and are unchanged.

  The 617-form on-disk corpus has an empty transition set: base `e3c19a39c9c3bf5c315c6002fd18114aa22cd654`, head `a7e54a21b390dec8df9cfbfdf6a5f113186b454f`, `fslc check` stable and FSL5 opt-in, and `fslc kernel` versions 1 and 2, complete output compared. The one inline change is the `duplicate_formal_with_erased_bool` row of `issue_1517_breaking_removals`, error to error. The new inputs of `issue_1680_duplicate_formals` change only where they repeat a formal.

  Rust API: `SpecItem::Action`, `SyncAction` and `RequirementAction` require `param_spans`, and `ProcessTransition` requires `input_spans`.
- Changed (#1682, breaking removal): in next-edition requirements, `check` refuses a `scenario` or `forbidden` step that names an input twice, on every route that loads the document, with exit 2, `kind: "semantics"`, `duplicate step input 'amount' at doc.fsl:43:44` and `loc` at the repeated name. Through an inline `implements` the report is `implements "rep.fsl" failed to check (duplicate step input 'v' at rep.fsl:15:30)` with `kind: "type"` and `loc` on the `implements`; `lint` and `fmt` omit `loc`.

  These forms passed `check` before and kept only the last value, so swapping the values changed the result:

  - `Employee submits claim1 with amount 2, amount 1`, in either order, with equal values, or written `Employee claim1 submits with ...`.
  - `従業員 が 申請1 を 申請金額 2, 申請金額 1 で 提出する`, the same input in two `で` groups, or an alias with its canonical name.
  - `Agent takes t1 with assignee 0, assignee 1`, the role of a relation event.

  On those forms `verify`, `sweep`, `refine`, `kernel`, `migrate`, `document claims`, `document generate`, `scenarios`, `testgen`, `mutate`, `html`, `ledger`, `diff`, `explain`, `analyze`, `conformance`, `typestate`, `lint` and `fmt` exit 2 instead of their own results. A step with a parse error keeps that error, different inputs may come in any order, and a later step may name the same input again.

  Migration: keep the one value you mean. There is no automatic migration.

  The 646-form on-disk corpus has an empty transition set: base `1559e8eba90f73615d4efd00fcdccc30cf525b42`, head `596881fc5e944a2548eedfe1b55d52fbea8ee344`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

  Rust API: `NextEditionRequirements` gains `repeated_step_inputs`.

  The design note `docs/design/DESIGN-dialects.md` holds the measured forms and routes. The language references take this text when the next edition's reference is written (#1437).
- Changed (#1683, breaking removal): `check` and the symbolic engines now answer from one carrier rule, `fsl_core::CarrierCoercion`, whose answers are `Retag`, `RebuildInt`, `MapMismatch` and `NotAssignable`, and a literal under `old(...)` that can type itself is also typed against what surrounds it, in `check` and in evaluation.

  Removed: a Map at any depth below `some`, Set and Seq literals and conditional arms under `old(...)`, for `m0: Map<K, Int>`, `m1: Map<K, A>`: `some(m0) == old(some(m1))`, `some(m0) != old(some(m1))`, `Set { m0 } == old(Set { m1 })`, `Seq { m0 } == old(Seq { m1 })`, `Set { m0 } == old(Set { if c then m1 else m1 })`, now fail `check` with the located assignability error (`semantics`, exit 2).

  What goes with it: where such a form is evaluated, bmc and induction reported `cannot coerce`, but explicit and `fsl-bfs` returned a verdict, `violated` included, and that verdict is lost. Under a position that is never evaluated, such as `forall i: E { ... }` over `type E = 1..0`, bmc and induction lose theirs too, as for the forms #1679 refused. The refusal comes before property selection, so `--exclude-property` does not avoid it, and such a form hides another property's counterexample behind exit 2 until it is rewritten.

  Unchanged: `a == old(Set {})`, `sa == old(Seq {})` and `oa == old(none)` are refused, though they pass outside `old(...)`.

  Now returns a result: a range-typed literal under `old(...)`, the third route #1679 recorded. `a == old(Set { vb })`, `sa == old(Seq { vb })` and `sa == old(Seq { va })` for `vb: 0..2`, `a: Set<A>`, `sa: Seq<A, 2>` reported `cannot coerce` on bmc and induction and now agree with explicit. One past capacity, `sb != old(Seq { va, va, va })` for `sb: Seq<B, 2>`, can end in `model sequence length exceeds capacity` on induction instead, as the same form outside `old(...)` does. Two routes still report it: a Map comparison in a refinement map under `preserve progress`, and a rejected `--lemma` candidate.

  Migration: declare the two values with one carrier, or compare the entries (`m0[k] == m1[k]`).

  The 650-form on-disk corpus has an empty transition set, base `b2cdb394fedcba015261df05587c4721583329e3`, head `683d12d99f5b3510101d4571b5e7e2db8fd1a444`, and bmc and induction return the same result on all 428 forms it accepts. The matrix and the positions it does not cover are in `docs/design/DESIGN-finite-container-carriers.md`.

  Rust API: `CarrierCoercion` gains `Retag` and `NotAssignable`, and loses `Other`.
- Changed (#1685, breaking removal): a struct literal that names a field twice is refused at the repeated field, in every position and edition, with exit 2, `kind: "parse"`, `diagnostic_code: "duplicate_struct_field"` and `duplicate field 'P.x' at 5:24`. A component reports `component "piece.fsl" failed to parse (duplicate field 'P.x' at piece.fsl:6:32) at root.fsl:2:3` with `kind: "semantics"`, an inline `implements` reports `implements "abs.fsl" failed to parse (duplicate field 'P.x' at abs.fsl:6:33)` with `kind: "type"` and `loc` on the `implements`, and a refinement mapping reports `duplicate field 'P.x'` with `kind: "type"` and `loc` at the field.

  These forms passed `check` before and kept only the last value, so swapping the values reversed the verdict:

  - `P { x: 1, x: 2, y: 0 }`, in either order or with equal values, in `init`, an action, a guard, a property, a `def`, a map value or a scenario, in any edition.
  - The same literal in a compose component, whether its action is lifted, synchronized, or internal and unreferenced.
  - The same literal in a refinement mapping or an inline `implements` abstraction.

  An input that already failed now reports the repeat, so its kind can change: `P { x: Q { a: 1, a: 2 }, y: 0 }` from `type` to `parse`, and an undefined `Q { a: 1, a: 2 }` from `semantics` to `parse`. In `fslc-lsp` the refusal is a parse diagnostic at the repeated field, and like any parse error it leaves `documentSymbol` returning null until the literal is fixed.

  `verify` no longer returns `violated`, `verified` or `proved` on those forms, `refine` no longer returns `refines`, `impl_violated` or `refinement_failed`, `replay` no longer returns `conformant` or `nonconformant`, `sweep --depth 0..1` no longer returns `sweep_failed` or `sweep_passed`, and `migrate`, `document claims`, `scenarios`, `mutate`, `testgen`, `html`, `ledger`, `diff`, `lint`, `fmt`, `explain`, `analyze`, `conformance`, `kernel` and `typestate` exit 2 instead of their own results. A `domain` value object is refused in `check` and every `domain` command, with `kind: "semantics"`: `domain expand` and `domain analyze` exited 0 before, and `expand` wrote both copies to a file `check` refused. `domain expand` now writes nothing.

  Migration: keep the one value you mean. There is no automatic migration.

  The 651-form on-disk corpus has an empty transition set: base `56354e477544f9be1f4477ddf036e5855d514bb5`, head `0d869b09886c1f627bf21bcc61394477dcceb4e7`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

  Rust API: `fsl_syntax::DUPLICATE_STRUCT_FIELD`.
- Changed (#1686, breaking removal): in a business or requirements document, an `instances` or `values` bound or a `kpi` written twice under one name is refused at the repeated declaration, whether or not the copies are equal, with exit 2, `kind: "semantics"`, `loc` at the second copy and `duplicate values bound for 'V' at doc.fsl:14:3`, `duplicate instances bound for 'E' at doc.fsl:13:3` or `duplicate kpi 'done' at doc.fsl:9:3`. In next-edition requirements a `concept` written twice is refused the same way, `duplicate concept 'SmallClaim' at doc.fsl:24:3`. An inline `implements` reports `implements "1_business.fsl" failed to check (duplicate kpi 'paid_claims' at 1_business.fsl:55:3)` with `kind: "type"` and `loc` on the `implements`.

  These forms passed `check` before and kept one copy, so swapping the copies changed the result:

  - Two `values` bounds of one number: in requirements the last was kept, and `verify` moved between `verified` and `violated`. Lowering a business document does not read `values`, and `check` went from exit 0 to exit 2 there too.
  - Two `instances` bounds of one entity: the last was kept, and `check` moved between `ok` and an error.
  - Two `concept` declarations of one name: the first was kept, and `check` moved between `ok` and a decision-table error.
  - Two `kpi` declarations of one name: `analyze` reported only the last, and no verdict changed.

  `verify` no longer returns `verified`, `proved` or `violated` on those forms, `refine` no longer returns `impl_violated`, `replay` no longer returns `conformant`, and `sweep`, `migrate`, `scenarios`, `testgen`, `mutate`, `html`, `ledger`, `diff`, `document`, `kernel`, `typestate`, `lint`, `fmt`, `explain`, `analyze` and `conformance` exit 2 instead of their own results. A `spec` with an `entity` or a `number` already refused a repeated bound and is unchanged. A `spec` with neither does not read its bounds, and repeating one there stays accepted.

  Migration: keep the one declaration you mean. There is no automatic migration.

  The 651-form on-disk corpus has an empty transition set: base `9c31458678cf6c70e77987c85c95f4bc0d8df503`, head `b2367d61fd8a410f9e1364301f1fab0d6b7c4cb7`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.
- Changed (#1692, breaking removal): a `struct` or `value_object` that declares a field twice is refused at the repeated field, whether or not the two types agree and whether or not anything reads it, with exit 2, `kind: "name"`, `duplicate struct field 'P.x'` and `loc` at the second `x`. A component reports `component "piece.fsl" failed to check (duplicate struct field 'P.x' at piece.fsl:3:20) at root.fsl:2:3` with `kind: "semantics"`, and an inline `implements` reports `implements "abs.fsl" failed to check (duplicate struct field 'P.x' at abs.fsl:3:20)` with `kind: "type"` and `loc` on the `implements`.

  These forms passed `check` before. With different types, `check` admitted a value by the first type and the bounds refused it:

  - `struct P { x: K, x: Bool }` or `struct P { x: K, x: K }`, in a `spec`, in a `requirements` document, or in a composed component, read or not.
  - `value_object C { value: Q = 0; value: Q = 0; }`. `domain expand` and `domain analyze` also exited 0 on a `value_object` whose copies differ, and `expand` wrote it out.

  `verify` no longer returns `violated`, `verified` or `proved` on those forms, `sweep` no longer returns `sweep_failed` or `sweep_passed`, `replay` no longer returns `conformant` or `nonconformant`, and `mutate`, `html`, `ledger`, `diff`, `migrate`, `document claims`, `kernel`, `typestate`, `lint`, `fmt`, `explain`, `analyze`, `conformance`, `domain expand` and `domain analyze` exit 2 instead of their own results. `fmt` reports it as `kind: "semantics"` with no `loc`.

  An input that already failed now reports the repeat, so its kind can change: `struct P { x: Set<K>, x: K }` and `struct P { x: Nope, x: K }` from `type` to `name`.

  Migration: keep the one field you mean. There is no automatic migration.

  The 650-form on-disk corpus has an empty transition set: base `b2cdb394fedcba015261df05587c4721583329e3`, head `66ca55474a05a8aefc9d92b29095505f1f1742af`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

  Rust API: `SpecItem::Struct` gains `field_spans`.
- Changed (#1694, breaking removal): a requirements document declares at most one `implements`. A second clause, naming another contract or the same one, is refused at that clause with exit 2, `kind: "parse"` and `duplicate implements 'Broken' (a requirements document declares one, and 'Held' came first) at 3:3`. The seam took the first clause and dropped the rest without a message, so a later contract was never checked: a broken contract written second passed `check` and written first failed it, in the stable edition and the next one.

  On those documents `check`, `verify` on every engine, `verify --property` and `--exclude-property`, `refine`, `chain`, `mutate`, `ledger`, `lint`, `fmt`, `migrate`, `kernel`, `typestate`, `analyze`, `explain`, `document`, `scenarios`, `testgen`, `html`, `conformance` and `diff` no longer return their own results, and the Worker and the editor report the same refusal. An abstraction file that repeats the clause is reported through the clause that named it, with `kind: "type"` and `loc` on that clause. A document with one clause is unchanged.

  Migration: keep the contract this document refines, and check any other from its own document or with `fslc refine`. There is no automatic migration.

  The 646-form on-disk corpus has an empty transition set: base `1559e8eba90f73615d4efd00fcdccc30cf525b42`, head `805751922a4b93c225beeeb7484b1e80581de809`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared. The 58 Rust string literals that hold a clause did not change either.
- Changed (#1700, breaking removal): a repeated `domain` declaration that loses a copy is refused at the repeat on `check`, `verify` and every `fslc domain` route (exit 2, located), and `domain expand` writes nothing.

  These forms passed `check` before and kept one copy:

  - `kind: "name"`: a second `evolve` for one event, which flipped `verify` from `violated` to `verified` by the order of two copies; a second `command` of one name; an `event` that declares a field twice, which let the order of the copies decide `check`.
  - `kind: "parse"`, a second copy of a clause that holds one value: `implementation_profile`; aggregate `id`; projection `from`; effect `reliable`, `irreversible`, `idempotency_key`, `correlation_id`, `handles`, `request_event`, `success_event`, `failure_event`, `timeout_event`, `retry`, `timeout`, `outbox`, `inbox` or a field of an effect; `max_attempts` or `backoff` in a `retry`; saga `starts_on`; step `awaits` or `timeout`.

  An event field an effect or saga action takes as a formal was already refused; it stays refused at the same place, with the message `duplicate event field 'E.a'` and `kind: "name"` instead of `duplicate action parameter 'a'` and `kind: "semantics"`. An event field no action takes was accepted by #1680 and is now refused. Through an inline `implements` the message names the other file, with `kind: "type"`.

  Kinds that read every copy stay accepted, such as two `requires` or two sagas of one name.

  Migration: delete the copy that was lost, or merge the two. There is no automatic migration.

  The 650-form on-disk corpus has an empty transition set: base `b2cdb394fedcba015261df05587c4721583329e3`, head `7f0b8e43af11f96786e2a72238c70a4a521b4cae`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared. See `docs/design/DESIGN-domain.md`.
- Changed (#1705, breaking removal): in next-edition requirements, `check` and every route that loads the document refuse three repeats, with exit 2 and `kind: "parse"`. For an ID, the ledger used to keep one row, with one title and one status, for the two declarations, or one text for two `covers`.

  These forms passed `check` before:

  - A second `actor` of one name, on its own line or in a list (`duplicate actor 'Agent'`), located at the repeated name. Its output equalled one copy's.
  - A second declaration owning a requirement ID that a `rule`, `decide`, `scenario`, `forbidden`, `goal` or `policy`, or a current-edition `requirement` or `acceptance` block, already owns, located at its ID and naming both kinds (`duplicate requirement id 'REQ-DESK-001': this rule reuses the ID of the rule at 19:8`). That includes two `rule`s of one ID, which lost a title from the ledger, a `rule` reusing a `decide` ID, whose title reached neither the ledger nor the documents, and two `requirement` blocks of one ID. A second `scenario`, `forbidden`, `goal`, `policy` or `acceptance` of its own kind keeps the refusal it had; an `acceptance` reusing a `scenario` ID, refused before as `duplicate acceptance id` with `kind: "semantics"`, now gets this message.
  - Two `covers` of an ID that no declaration owns, with different texts, anywhere in the document: two events, two rows of one `decide`, rows of two tables, or an event and a row. The refusal is located at the `covers` keyword of the later one and names the earlier one (`requirement ID 'REQ-EV-1' is covered as "withdrawing" here and as "closing" at 15:5`). One ID with one text stays accepted, and the ledger shows it once. An event and a row covering one ID with different texts on one event were refused before as a conflicting annotation with `kind: "semantics"`; they now get this message with `kind: "parse"`, at the same location.

  The current edition shares IDs the same way and `check` is unchanged for it. `fslc migrate --edition next` on a current-edition document with two `requirement`s of one ID, or an `acceptance` and a `requirement` sharing one, used to add the header and write a document the next edition refuses; it now stops before rewriting with exit 1 and a `migrate_manual_required` finding, `reason: "shared_requirement_id"`, at the second owner's ID in the source.

  Migration: rename one of the two IDs, merge the two declarations, or give every `covers` of an ID one text; delete the repeated actor. There is no automatic migration.

  The 651-form on-disk corpus has an empty transition set: base `56354e477544f9be1f4477ddf036e5855d514bb5`, head `2286fc51feaf3b49dac235c15e3367eb8346b919`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

  Changed ledger and document output (#1705): in next-edition requirements, a decide row's or an event's `covers` of an ID that a declaration owns stays accepted and linked, whatever its text, and keeps that text in the kernel. The ledger row for the ID now takes the owner's title; it used to take the first `covers` text and drop the owner's. `document claims` now lists the owner's title first among the ID's statements, cited at the owner, so the document's rule section quotes it; the `covers` texts follow as linked statements. With an event's `covers` of a rule's ID, the rule's title used to reach neither `document claims` nor `document generate`. One case stays refused, as before: a row covering its own table's ID with another text (`requirement annotation 'REQ-2-3' has conflicting text`, `kind: "semantics"`). Migrating a current-edition document can therefore retitle a ledger row: a process `covers` naming a `requirement` block's ID with another text migrates with exit 0, and the row that showed the `covers` text now shows the block's title. A ledger approval of the current-edition document reports `drifted` after any migration, retitled or not. Current-edition `ledger`, `document generate` and `document claims` output is unchanged.

  The design note `docs/design/DESIGN-dialects.md` holds the measured forms. The language references take this text when the next edition's reference is written (#1437).
- Changed (#1723): the GitHub Release body is now built by
  `tools/release-notes.sh`. When `docs/release-notes/vX.Y.Z.md` exists, the body
  is that hand-written summary followed by an absolute link to the `## [X.Y.Z]`
  section of the tagged `CHANGELOG.md`. Otherwise the body is that section, as
  before. The `assemble` job builds and checks the body, so a `workflow_dispatch`
  dry run fails on a missing section, an empty body, or a body over GitHub's
  125,000-character limit (checked as 125,000 bytes) before any tag exists. The
  aggregated 5.0.0 section is 225,360 bytes, so 5.0.0 needs a summary.
- Changed (#1727): the WASM browser parity test reads the `[workspace.package]`
  version from `rust/Cargo.toml` table by table, so an array value in that table
  no longer hides it. Product output is unchanged.
- Changed (#1728, breaking): approval records name the target kind's renderer instead of
  the `fslc` release. A new record stores `"<renderer-id>/<semver>"` in
  `target.generator_version`: `fslc-ledger-renderer/1.0.0`,
  `fslc-html-renderer/1.0.0`, `fslc-scenarios-renderer/1.0.0`, or
  `fslc-document-renderer/1.3.0`. `approval check` and `ledger --approval`
  report `renderer_changed` only when that renderer version changes, so
  upgrading `fslc` alone no longer drifts an approval. A 4.x record keeps its
  bare crate version (exactly `X.Y.Z`). For it only the renderer comparison is
  skipped, so it stays `approved` while every specification, rendering,
  claim-set and reviewed-artifact digest matches. Before, it reported
  `renderer_changed` on any other `fslc` version. Other values, such as `4.12`
  or `v4.12.0`, are still compared. Migration: a consumer that read
  `generator_version` as the `fslc` version must read `fslc --version` instead.
- Changed (#1754, breaking): `fslc migrate --write` no longer reports `"written": true` when
  it wrote nothing. The top-level `written` echoed the `--write` flag, so a run
  whose inputs needed no rewrite reported `"written": true` with `"changed": 0`.
  `written` now means a file was replaced: the top level is `true` only when at
  least one input was replaced, and each `files[]` entry carries its own
  `written`, `true` only for a replaced input. Unchanged inputs were already left
  untouched (same inode and mtime); dry runs, refusals and
  `migrate_manual_required` report `written: false` at both levels. A consumer
  that read `written` as "`--write` was given" now sees `false` for a no-op
  write.
- Fixed (#251): next-edition diagnostics for already-invalid expression `||` and
  `->` keep the parse envelope and exit code while reporting
  `unsupported_in_edition`, the operator location, canonical replacement, and
  migration guidance. Structural operators and domain acceptance are unchanged.
  Worker check/verify now finalize successful and error envelopes with native
  edition policy, including declared-edition fields and current enum-union warnings,
  while check preserves early project-gate results instead of folding model-only
  governance over them.
  Type-range endpoints, including parenthesized parts, retain generic parse diagnostics.
- Fixed (#608): 25 more shipped examples now pass `fslc lint`, including the
  teaching examples `examples/pm/cancel_system.fsl`, `examples/pm/cancel_flow.fsl`,
  `examples/layers/return_policy.fsl` and `examples/layers/return_system.fsl`.
  Their requirement, acceptance, forbidden, policy, goal, control and assumption
  IDs use the canonical `PREFIX-SCOPE-NNN` form: `REQ-1` becomes
  `REQ-CANCEL-001`, `goal CanRetain` becomes `goal GOAL-CANCEL-001`, and
  `FB-EARLY-REFUSE` becomes `FB-EARLY-REFUSE-001`. Legacy `"ID: text"` tags
  become `@requirement(...)`, and domain invariants use `=>` instead of `->`.
  Each changed file keeps its `check` result, its `verify --depth 8` verdict and
  property results, and its `mutate` kill rate. A new corpus test,
  `every_corpus_spec_lints_clean_or_is_allowlisted`, fails when a `specs/` or
  `examples/` file adds a lint finding. Each file it still allows names its
  reason, for example a file the frozen Python reference loads, or business
  policy IDs that `preserve progress` must reference as identifiers.
- Fixed (#1091): a requirements document's `terminal_rule` claim is now
  attributed to its `terminal` block. `fslc document claims` reports it
  `source_backed` with the block's `source` location instead of `unknown` with
  no location, so a spec that declares `terminal` keeps
  `provenance.completeness: complete`, and `fslc document generate` no longer
  adds the "provenance information is incomplete" note for it. Claim and claim
  set digests do not change. Migration: a requirements document generated before
  this fix for a spec that declares `terminal` carries that note, so
  `fslc document check` (and `fslc approval` over a `requirements_document`)
  now reports it as drifted (`edit_outside_slot`); regenerate it with
  `fslc document generate`.
- Fixed (#1242): `fslc explain` (and `--readable`) no longer reports
  `result: "explained"` with exit 0 and no witnesses for a spec whose
  acceptance / forbidden traces the requirement-trace gate refuses. It reports
  the gate's own error (`kind: "acceptance"`, `"forbidden"`, or
  `"forbidden_setup"`, the envelope `check` and `scenarios` report) with exit 2.
  `fslc html` stops at the same error: its exit status stays 2, but it now
  prints the gate's error and writes no report instead of a report with an
  empty witness section. Migration: a pipeline that read `explain`'s witnesses
  or `html`'s report for such a spec gets the gate's error instead; fix the
  acceptance / forbidden case it names (the same error `fslc check` reports).
- Fixed (#1243): the commands that report a result about a spec's acceptance /
  forbidden cases run the requirement-trace gate and fail closed on its
  refusal, reporting the gate's error (the envelope `check` reports; a
  `semantics` error when a trace cannot be replayed, e.g. under an `init` that
  leaves a state variable unassigned) with exit 2: `fslc document generate` / `document claims` / `document check`
  (which rendered, projected, or conformance-checked the refused cases as
  claims, exit 0), `fslc ledger` (which rendered a ledger listing the error,
  exit 2), `fslc approval create --kind requirements_document` and
  `--kind ledger` (which recorded an approval, exit 0), and `fslc approval
  check` of either kind (a `ledger` record reported `drifted`, exit 0). The
  language server now publishes the refusal as an error diagnostic with code
  `FSL-REQUIREMENT-TRACE` at the location `check` reports, where it published
  nothing. A spec whose traces pass the gate, or that declares none, is
  unaffected. Migration: fix the acceptance / forbidden case the error names
  (the same error `fslc check` reports) before generating or approving a
  document or ledger; an approval recorded for such a spec can no longer be
  checked until the spec passes `check`.
- Fixed (#1281): `fslc mutate` now checks each mutant on a fresh solver context, so
  a mutant's result no longer depends on the solver queries made for earlier
  mutants in the same run. This can change which property a mutant is reported as
  killed by (and with it the per-obligation and per-requirement tallies), and,
  where an `ensures` counterexample is replayed, whether the mutant is killed;
  each mutant now gets the result it gets when checked alone. Each mutation
  solver call takes about 0.7 ms longer. The faster mutate work tracked in #1281
  is still open.
- Fixed (#1285): `fslc analyze` graphs no longer present the acceptance /
  forbidden cases of a spec whose traces the requirement-trace gate refuses
  as if they had passed it. The graph still projects them (analyze reports
  structure, so it does not fail), and each such `acceptance:<id>` /
  `forbidden:<id>` node now carries `"trace_gate": "unchecked"`. Nodes of a
  spec whose traces pass the gate are unchanged. Migration: a consumer of the
  graph that treats acceptance / forbidden nodes as checked claims skips nodes
  with `trace_gate: "unchecked"`.
- Fixed (#1333): a component's `leadsTo` helpful action now references that
  component's prefixed action, and its arguments are rewritten in the same
  binder scope as the temporal property. Composed ranking proofs can now run
  instead of failing with an undeclared helpful action (exit 2). Internal or
  unknown helpful actions remain errors rather than binding to glue actions.
- Fixed (#1334): ordinary compose glue actions now resolve component aliases in
  parameters, guards, postconditions, lets, statements, and assignment targets,
  allowing direct component-state reads and writes through `alias.var` as designed.
  Assignment indices are resolved in glue actions, sync additions, and compose
  init alike. Sync additions that write component state through `alias.var`
  now report an explicit restriction at the assignment rather than an unknown
  update target. Compose init can initialize component state through `alias.var`;
  overlap with the component's own init is rejected by the existing duplicate
  initialization check, never resolved by picking one assignment. Component
  invariants continue to check glue writes. Alias resolution
  respects parameters, model-normalized guard bindings, quantifiers, aggregates,
  logical/guard pattern continuations, and temporal binders. A pattern payload is
  visible on the right of `and` or `=>`, and a `requires` also exposes it to later
  guards and the normalized body/ensures; neither statement nor expression `if`
  introduces it into a branch. Guards see preceding bindings; body statements
  and ensures see the final guard scope regardless of their source position, so a
  local binding named like an alias keeps its referent.
- Fixed (#1336): `fslc ledger` now shows its folded verification verdict and
  inline `implements` seam (`abs`, `result`) in a new `## inline implements`
  section after the header and approval comparison, before governance and the
  risk lists. Passing seams are listed too; only `refines` is green. Previously
  a failed seam exited 1 without its verdict or seam appearing in the body.
  This section addition alone does not change ledgers without `implements`;
  the separate sentinel-coverage correction and verdict findings (#1338) can
  change them, including governance ledgers. Migration: an approved
  inline-implements ledger's rendering digest changes, so `fslc approval check`
  reports `drifted` (exit 0); review the section and re-approve the ledger.
- Fixed (#1337): `fslc testgen` now refuses failed inline `implements` seams
  and governance preservations on its kernel path as well as its verify fallback
  path. Both return the verify owner's failure envelope and exit 1, or the same
  located contract error and exit 2, or internal error and exit 3, and write no
  harness. The gate shares the scenarios fold and runs before the walk, path
  handling, and generation.
  Migration: these documents previously generated a harness with exit 0 on the
  kernel path; automation must now handle verify exits 1/2/3 and no output artifact. Fix
  the seam or preservation before generating tests. Passing documents and
  non-strict partially reachable scenarios keep their existing generation behavior.
- Fixed (#1338): generated-only sentinel actions no longer appear as dead-action
  coverage findings in `fslc ledger`, or as uncovered badges and coverage
  denominator entries in `fslc html`. Runtime warnings, ledger, HTML, and mutate
  dead-action notes share the existing source-location rule through
  `KernelModel::action_source_span`:
  positive authored primary-origin position, then positive kernel declaration
  position, never a name/prefix test. Authored blocked actions retain their findings.
  The current CLI mutant-enumeration path cannot encounter generated-only dead actions.
  This coverage presentation correction preserves raw coverage,
  verify/induction/Worker JSON, and HTML Raw Data;
  HTML with no eligible coverage entries shows `0/0`. Migration: corrected ledger
  and HTML artifacts have changed rendering digests, so existing approvals report
  `drifted` (exit 0); review and re-approve the artifacts.
- Fixed (#1345): Cache replay preserves stored f64 values and JSON tokens via
  serde_json `float_roundtrip`, without changing cache shape, schema, or keys.
  Document sweep cache provenance as execution metadata: stable comparisons
  exclude root `cost`/`cache` per verification envelope, preserving domain state;
  use `FSLC_CACHE=off` to disable caching rather than a sweep `--no-cache` flag.
  Accurate JSON f64 reading applies workspace-wide, including Worker requests,
  scenario approval normalization, AI eval/JSONL, causal evidence/plan/lifecycle,
  and log replay/implementation-log/event inputs; older scenario approval or causal
  digests containing boundary decimal values can change. The fslc/fsl-tools test
  graphs already enabled this feature via the `jsonschema` dev-dependency, so tests
  and distributed builds previously parsed such values differently.
- Fixed (#1346): requirements acceptance steps now report `wrong_kind_argument`,
  a top-level `message` identifying the argument and expected type and explaining
  that no guard was evaluated, a hint, and the step location for arguments of the
  wrong parameter kind, matching forbidden's diagnostic style without changing
  acceptance's error kind or exit status.
  Correct the language and skill references: enum step arguments take member names,
  not numeric ordinals (`paint(1)` is not `paint(Blue)`).
- Fixed (#1350): `fslc explain` now produces invariant and reachable
  counterfactual candidates for kernel specs using `entity` / `number` domains
  bounded by a sibling `verify` block, just as for directly bounded `type`
  declarations. Invariants without a candidate within the requested depth retain
  their depth-limit note instead of silently disappearing. Kernel specs using
  `def` (named predicates) also regain counterfactual candidates, with authored
  source locations and text preserved. `fslc html` shows the
  same restored candidates without changing the explanation JSON format. A
  weakening that violates several invariants or prevents several reachable goals
  now contributes a candidate to each affected property, retaining the existing
  per-invariant ranking. Counterfactual traces record the actual violating
  successor and its state changes rather than the Monitor rollback state.
  Each invariant is searched in isolation, so its counterfactual trace may pass
  through states that violate another invariant. Normal `verify` and `replay`
  retain all invariants and stop at the first violation instead.
  When a non-user check preempts a target's invariant search, explain now tries
  bounded solver-independent concrete search for a valid target witness. If none
  is found, the existing note identifies the preempting kind, name and step rather
  than claiming absence within depth. Bounded init-assignment removals remain
  skipped; no-candidate depth/preemption notes now append the named state root
  and the reason that nondeterministic initialization was not analyzed. This is
  an intentional note-wording change, with no new JSON field.
  Concrete search also continues past partial guards and erroring children,
  and refuses witnesses starting from an already-violated initial state.
  Preempted no-candidate notes also explain when concrete search could not start,
  naming reported free state roots publicly or preserving another startup error's
  cause. This is diagnostic only; nondeterministic initialization is not searched.
- Fixed (#1351): `fslc analyze` and enabled LSP structural diagnostics no longer
  report an init-only requirement as disconnected, matching `check --strict-tags`
  and lint. Init carrying requirement relations now has an `init` graph node with
  requirement metadata and `declares`/`covers` edges, retained in requirement and
  impact projections, manifest traceability graphs, and DOT/Mermaid exports.
  Init without requirement relations leaves graph output unchanged. Genuine
  orphan requirements remain review findings, not verifier violations.
- Fixed (#1362, #552): in the language server, a name spelled like a word the
  index used to treat as a keyword (`status`, `count`, `age`, `evidence`,
  `owner`, `rejected`, `severity`, `respond`, ...) is now a name like any other
  in `spec`, `requirements` and `compose` documents: a state variable, struct
  field, enum member, parameter or `let` binding so named is declared, and
  go-to-definition, references, rename, hover, document symbols and completion
  reach it and its uses (for example `respond(0)` in an acceptance). The
  document index now takes those dialects' declarations from the parsed AST.
  As a result, the `until` operator inside an `until NAME { P until Q }`
  property no longer declares the first name of `Q` as a property, so uses of
  that state variable resolve to its declaration again. Other dialects and the
  server's other responses are unchanged.
- Fixed (#1391): replaying a requirements document's `acceptance` and
  `forbidden` traces no longer holds memory in proportion to the
  `verify { values }` scope of an action parameter. Each trace step used to list
  every enabled instance of every action, so a published requirements spec with,
  for example, `values Amount = 0..2000000` and a single `forbidden` made
  `fslc check` use several GB until it was killed. Each step now keeps only the
  call it names, and `check` on that spec stays at about 20 MB. Every instance's
  guards are still evaluated, so trace verdicts and errors are unchanged, and
  the replay time still grows with the scope.
- Fixed (#1392): `fslc fmt` and `fslc migrate` print requirement-style IDs
  the way they are written. They used to put spaces around each `-` of an ID
  outside `business` documents, so `requirement REQ-1`, `acceptance AC-1`,
  `forbidden FB-1`, `covers REQ-1` and governance references such as
  `owns CTRL-1` or `require CTRL-1` came out as `REQ - 1`, `AC - 1`, `CTRL - 1`.
  The ID values and the checked model did not change, but the written file no
  longer read as its author wrote it. Subtraction is still printed with spaces
  (`balance - 1`). An ID written with spaces (`REQ - 1`) is now printed as its
  value (`REQ-1`). If the formatter would ever print an ID differently from its
  value, `fmt` and `migrate` now stop with an error instead of writing the file.
  Migration: a file already written by `fmt` or `migrate --write` with spaced
  IDs is reported as changed by `fslc fmt --check`; run `fslc fmt` on it (or
  restore the IDs by hand) to get `REQ-1` back.
- Fixed (#1401): in the requirements groundwork, an event that names no
  requirement with `covers` no longer gets a requirement made from its own name
  and actor (`submit: by Employee`). Its action carries the requirement IDs the
  document attaches to it: the rules that guard it, and the decision table and
  the `covers` of the rows that name it. The kernel's `requirement` of the
  action is the most specific of them (a row's `covers` before its table's ID),
  or none when nothing names the event. `ledger`, `document generate`,
  `explain` and `mutate --by-requirement` no longer show such a note as a
  requirement, and the requirement a rule formalizes gets the same ledger status
  as the same requirement written as a requirement block. Parsing, diagnostics,
  and command output of published documents are unchanged.
- Fixed (#1402): a requirements decision table or rule guard whose check the
  solver decides (because its fields take more than 1000000 values together,
  or because its condition reads a resource or a relation role) was checked
  only by `fslc check`. `verify`, `kernel`, `testgen`, `document`,
  `scenarios`, `ledger`, `explain`, `mutate`, `typestate` and the other
  commands that load the spec accepted it and exited 0. They now reject it
  with exit 2 and the diagnostic `check` reports (`decide_not_exhaustive`,
  `decide_overlap` or `rule_conflict_unsatisfiable_guard`). The language
  server now shows that error, and the browser Worker's `check` and `verify`
  report it too.
- Fixed (#1413): Make LSP stdio tests fail at a deadline instead of hanging, drive a solver test to completion, refuse the `fsl5_keywords` golden-update variable under `CI`, and document the sanitizer exclusion of `requirement_trace_large_domain`.
- Fixed (#1414): `fsl5_lowering_contract.rs` and `fsl5_decide.rs` copy fixtures into scratch directories unique to the process and call, emptied first and removed afterwards, and `issue_1171_command_argv.rs` gives each run its own `FSLC_CACHE_DIR` and removes it. Before, the lowering-contract copies persisted, so a deleted fixture was still read from an earlier run; two concurrent `fsl5_decide` processes on one target deleted each other's inputs; and every argv run left a cache directory in the temporary directory. No product behavior changes.
- Fixed (#1416): preserve lossless token/trivia text and Unicode source positions while computing
  trivia positions incrementally instead of rescanning source prefixes. Make the
  AI project block scanner ignore `//` comment contents during header and brace
  matching, preventing ghost declarations, truncated fields, missing siblings and
  false unterminated-block diagnostics, without repeated prefix-depth scans.
- Fixed (#1417): the HTML and ledger unknown authority tool regression expectations
  now follow the verification-first pipeline's unlocated validator error. Report
  DESIGN notes also reflect #1464's semantics errors instead of the former panics
  on approval and counterexample export paths. Command behavior is unchanged.
- Fixed (#1419): native generated artifacts and approval/causal records are
  written completely to a same-directory temporary file before replacement,
  preserving existing output bytes on publication failure. Existing regular-file
  permission bits and new-file umask are retained; setuid and setgid are not. Partial
  bytes stay owner-only, though abrupt termination can leave that temporary file.
  Multi-file output keeps its existing order without rollback or added power-loss
  durability. Temporary names include a clock component and exclude the destination
  basename, and an entry already at that name fails with an error naming its path.
- Fixed (#1420): native `check`, `verify`, and `scenarios` consume extracted
  literate Markdown source in memory instead of creating an adjacent
  `.literate-<pid>.fsl` file. Extraction no longer requires a writable source
  directory or leaves a new sibling after abrupt termination. Existing
  diagnostic labels, lexical dependency resolution, edition admission and
  Markdown-byte cache identity are retained; verify's `--strict-tags` uses
  the captured extracted source. Existing abandoned siblings are left untouched.
- Fixed (#1423): causal observation log provenance now hashes the same captured UTF-8 input
  used for replay evaluation, rather than rereading its path. Preserve ordinary
  JSON outputs, errors, assurance, replay parsing, and artifact/lifecycle digest
  canonicalization. Observe/verify already use the supplied causal source; this
  fix addresses the remaining log reread in #1423.
- Fixed (#1424): testgen preserves wide Kotlin integer literals and compares integral
  values recursively through lists and maps, emits `PHP_INT_MIN` for i64 minimum,
  rejects lossy Vitest integers with a located error, escapes Python string controls,
  and inserts escaped source basenames only after trusted template slots are filled
  in every target. Ordinary output outside the Kotlin helper and its comment is
  unchanged.
- Fixed (#1425): Built-in mutation candidates are retained as edits and materialized
  one at a time, avoiding a full spec clone per candidate. Residual: each init
  mutant retains a rewritten copy of its entire init block, and whole top-level
  `if` statement mutants retain a rewritten copy of that entire statement;
  their retained memory still scales with that block or statement's size times
  its mutant count. Mutate stdout, catalog order, cap accounting, and verdicts
  are unchanged. No migration is required.
- Fixed (#1426): Causal representative-path projection now prunes dead ends and stops
  after 100,000 DFS expansions. Budget-limited output marks `paths_truncated_exact` as
  `false` and reports the omitted paths counted so far as a lower bound; completed
  walks retain the existing exact counts and output shape. The schema constrains
  the optional exactness marker to `false`; absence remains the exact-count signal.
- Fixed (#1427): Testplan boundary cases now reserve plain names before qualifying
  duplicates across actions and guards, then sort cases by their final unique names.
  They omit unrepresentable integer boundaries without overflow and index conformance
  state values once when constructing shortest setup paths.
- Fixed (#1429): `fslc ledger` and `fslc html` property assurance, and the
  ledger's `保証限界` line, no longer over-claim for a verification envelope that
  does not report a completed check. The header and every property row now read
  completeness the same way (the top-level `completeness`, else the nested
  `kernel.completeness` that `fslc domain check` and `fslc ai check` report). A
  row is `bounded` only under reported `completeness: "bounded"` and `proved`
  only under reported `completeness: "unbounded"`; a missing, unknown or
  method/depth-only envelope, or `result: "proved"` without completeness, now
  shows `not_run`. The guarantee line no longer states BMC coverage of every
  execution for `not_run`, error, replay or statistical input, and a bounded
  report without a valid depth says the depth is unreported instead of printing
  `深さ null`. This includes a native explicit-engine `unknown_budget` result
  (`completeness: "unknown"`) passed to the library renderers, which showed
  `bounded(BMC depth N)` and BMC coverage before and now shows `not_run`;
  `fslc ledger` and `fslc html` accept only `bmc` and `induction`. A rendering of
  a non-error result with top-level bounded or unbounded completeness and its
  depth is unchanged; verdicts, raw findings, locations, exit codes and the
  aggregate/external-evidence token (#994/#995) are unchanged. Migration: an
  approval bound to a ledger or HTML rendering of an affected envelope fails its
  existing drift check; regenerate and re-approve it.
- Fixed (#1430): Graph exports now re-check suffixed Mermaid identifiers to keep
  nodes and edges distinct after sanitization. Analysis and causal exporters share
  one escaping rule per format, including DOT quotes, backslashes and newlines,
  and Mermaid label delimiters.
- Fixed (#1431): TypeScript typestate skeletons now escape ECMAScript and strict-mode
  reserved action and parameter names (including `eval` and `arguments`), keep
  function/signature identifiers distinct, allocate action names once per spec in
  declaration order for reuse across entities, and keep the phantom generic clear
  of every model and generated type name. Parameter types now come from the model
  instead of blanket `number`. Enum types whose public names collide with emitted
  entity interfaces or state aliases use inline member string-literal unions,
  including inside options and sets in parameters and data fields.
  These are native/frozen-reference differences in
  addition to the earlier #680 guarded state-preserving self-loop report entries
  (`state_preserving: true`) and generic signatures preserving the exact input
  state. Native judgment also already scopes enum fields by owning struct and
  drops disjunctive guards when any disjunct does not constrain the entity. JSON
  `typescript` fields carry the TS differences; the command envelope and existing
  report field shapes (including source-name `params`) stay compatible, but report
  values, entity inclusion, applicability and summary counts can differ under the
  earlier judgment rules. Native entity order is alphabetical (the frozen reference
  uses declaration order). Neither JSON nor TS is claimed byte-identical to the
  frozen reference.
- Fixed (#1433): HTML Relation Graphs use declared relation state types rather than
  inferring relations from pair-shaped values. Seq/Set values no longer produce
  spurious graphs; real relation reports, including empty relations, are unchanged.
- Fixed (#1438): in the requirements groundwork, auto-mapped events reject actor
  mismatches against business transitions at check time, using typed actor
  provenance independent of requirement annotations. Parsing, diagnostics, and
  command output of published documents are unchanged.
- Fixed (#1442): component rewriting now preserves model-normalized `let` bindings
  and pattern scope in logical and guard continuations instead of replacing local
  reads with prefixed component-state reads. The tested use-only compositions
  preserve standalone local-binding behavior across BMC, induction, explicit
  exploration, and Monitor replay. Guards see preceding guard bindings; body
  statements and ensures see all guard bindings even when written before a `let`
  or `requires`. A pattern
  payload is visible on the right of `and` or `=>`, and a `requires` also exposes
  it to later guards and the normalized body/ensures; neither statement nor
  expression `if` introduces it into a branch. A pattern
  binding follows the same phase-aware collision rule as the standalone spec.
  Patterns sharing a normalized non-typed `count` or `sum` binder are accepted
  in compose just as they are alone; non-expanding `forall` and `unique` forms
  retain their collision refusal. Other original-source collisions remain
  rejected instead of being hidden by prefixing. The ordinary type checker owns
  this decision for both source rewriting and checked expressions. Component-owned
  late diagnostics retain the original cause and component position inside the
  existing use-site diagnostic wrapper.
  Private lets, patterns, quantifiers, aggregates, and statement-forall binders
  share one hygienic rewrite across synchronized sources. It inventories both
  reads and declarations, including non-exported pattern labels, and preserves
  each source's original collision refusals. Header arguments are inserted in
  their header-parameter scope, not under another source's guards. Ambient state
  or enum names cannot falsely reject private component patterns; an unread
  ambient state alone does not force an ordinary let to be renamed.
  The same hygiene now covers component property, temporal, initialization, and
  inline-state-initializer roots. Bare component enum reads retain their owning
  enum when an unrenamed public value would capture them, while uncontested
  reads retain their existing representation. Reified bare enum reads print their
  source member spelling; conversion, abstraction, and explicitly qualified enum
  literals retain owner qualification through node-local spelling provenance.
  A single inserted argument pattern without foreign interference
  is no longer renamed merely because its name is reserved for freshness.
  When sync composition must rename colliding private bindings, synthesized names
  such as `__u_substitution_0` may intentionally appear in later error messages,
  `explain`, `analyze`, and approval HTML output. Inputs needing no such renaming
  keep their existing output spelling.
- Fixed (#1443): compose state declarations now resolve component aliases in inline
  initializers and type-range expressions. Component constants can initialize a
  compose state field or define its bounds. Inline initializers still cannot read
  any state root: `seen: Bool = a.on` now reports the existing state-read prohibition
  with the resolved public name instead of failing to type the alias identifier.
  Qualified type syntax is unchanged.
- Fixed (#1444): Gate the large-domain trace test's next-edition helper with the same Linux-only condition as its caller, keeping macOS Clippy warning-free without suppressing dead-code diagnostics. Linux test behavior is unchanged.
- Fixed (#1448): induction `--lemma` candidates now receive the same recursive
  Boolean type check as invariants, for all candidates before any candidate proof
  or evaluation. Ill-typed expressions are rejected and unused with a nested
  semantics error and checker message, never proved or recommended. Later valid
  candidates still run, and the target result owns the exit code. Lemma runs
  continue to bypass the persistent verification cache.
- Fixed (#1449): Complete live multi-root and manifest admission for analyze
  batch/project, approval check/create/diff and ledger approval overlays, chain,
  diff and direct/multi-link refinement. Consumers use the same captured bytes and
  positional edition tokens, with once-only diagnostics for each distinct result,
  artifact and baseline. Batch dependency scopes remain file-local; actual chain
  step threads own their detail delivery. Live Git mappings are admitted without
  changing historical mapping selection or admitting historical roots (#1513).
  The deferred-mode set is empty and strict inventory controls remain executable.
  Live Git mapping capture is shared across every selected comparison, including
  existing stream-backed paths, without changing IO precedence or historical selection.
  Project analysis does not prepare mappings for noncanonical, unused target sections.
  Independent HTML/ledger baseline pinning and internal seam controls remain
  tracked in #1641; neither renderer's independent pin is included in this change.
- Fixed (#1451): explicit and Monitor now diagnose init reads of never-assigned
  state roots as unsupported free initial values (nondeterministic init), with
  guidance to use `--engine bmc`, instead of claiming the root is read before
  assignment. Reported names retain compose prefixes. This intentionally changes
  the public message only: semantics/exit 2, assigned-root early-read rejection,
  check acceptance of free roots, BMC/induction symbolic values, and refinement's
  free-root enumeration remain unchanged. Auto still falls back to BMC; for
  non-error BMC results, `engine_fallback.reason` carries the new diagnostic.
  BMC error results still omit fallback metadata; explicit gains no free-root
  enumeration.
- Fixed (#1452): consumed compose, implements, causal and governance dependencies
  are admitted by their own syntax edition before their bytes enter model/solver or output work.
  Canonical mixed editions remain valid, repeated child findings are delivered once
  per outer native owner or explicit local log, and repair tools report child findings without editing children.
  Editor aliases deduplicate each complete child finding at the first parent
  reference, preserving file-backed open-document precedence and child UTF-16 ranges.
  Untitled buffers publish absolute child links without a path-conversion panic.
- Fixed (#1457): symbolic verification now evaluates scalar Set and Seq collection binders in
  expressions and statements using shared membership-aware expansion. Seq binders
  preserve their live prefix, position order and duplicates; Set binders preserve
  distinct members in concrete value order, including dynamically valued literals.
  Symbolic and explicit verification share the existing rejection of state-dependent
  collection domains in init statement forall; check acceptance is unchanged.
- Fixed (#1464): Lowering now returns an authored error instead of panicking
  when the generated kernel needs an undeclared AI authority tool reference,
  including in document-taking CLI commands and the language server. Editor
  diagnostics point at the authority clause that names the tool; existing check
  and Worker error envelopes are unchanged.
  Empty domain membership now renders as false without panicking in domain
  analyze, check, expand, or mutation after a verified baseline; domain check
  still reports a violation. Kernel projection of `at()` without an index now
  returns the existing semantics error instead of panicking, while check
  acceptance and valid `at(0)` projections are unchanged.
- Fixed (#1467): symbolic scalar/enum Set/Seq expressions use shared checked
  inferred/expected types, including empty equality operands and conditional arms.
  Private lexical type metadata preserves raw `none`/composite values and the public
  Rust `Bindings<T>` alias. Carrier, capacity and storage-bound rules are unchanged;
  remaining composite/relation gaps stay tracked by #1467. State/storage restrictions
  do not imply rejection of every composite expression literal. This does not claim
  whole-corpus engine agreement or validation of a later integrated source state.
- Fixed (#1469): `html --no-timing` omits environment-dependent solver statistics
  from the presentation copy on stdout and in `-o`/`--output` files, preventing
  ambient Z3 memory differences from breaking byte-stable report comparisons.
  The exact omitted fields are `verification.cost.elapsed_s`,
  `verification.cost.solver.check_elapsed_s`,
  `verification.cost.properties[].elapsed_s`,
  `explained.counterfactuals[].violation.cost.elapsed_s`,
  `verification.cost.solver.memory_mb`, `verification.cost.solver.conflicts`,
  `verification.cost.solver.decisions`, and `verification.cost.solver.propagations`.
  Solver check counts and property identities/counts remain; default HTML,
  ordinary verify JSON, exit codes, and approval normalization are unchanged.
  One CI failure showed `memory_mb` differing (17.91 versus 17.92); 396 local
  macOS runs showed no solver-statistic variation, including memory. The three
  search counters are classified as ambient based on their Z3-internal source
  and the existing approval normalization of the same four solver statistics,
  not on measured local drift.
- Fixed (#1475): Reject overflowing causal analysis minimum-time sums with the
  contributing claim's location, and evaluate causal time thresholds exactly.
  Malformed generated document frontmatter/markers, missing chain step files,
  and malformed approval timestamps now use their existing error envelopes instead
  of panicking. Normal document and approval bytes/digests and source-check
  acceptance are unchanged.
- Fixed (#1480): avoid reporting a frozen-state tautology when the invariant can be undefined for a typed mutable state. Preserve genuine defined tautologies and partial operations that remain unreachable under the frozen values.
- Fixed (#1481): In ordinary action/property contexts, BMC, induction, and explicit verification now agree with the Monitor's existing `partial_op` for reached `head()`, `at(i)`, and index reads of an empty zero-capacity Seq, instead of symbolic term-construction errors. Unreached reads that previously failed symbolic term construction now pass under short-circuit guards and disabled actions. Ordinary symbolic evaluation constructs a fresh, unconstrained read term outside the declared capacity for each otherwise unrepresentable read, without exposing its value in properties, witnesses, or traces or reusing agreement-only fallback values; temporaries with live elements retain their normal read semantics. Ranked `leadsTo` decreases measures are a narrow exception: a path-sensitive preflight refuses only reached zero-slot reads whose fresh value would otherwise enter rank evaluation, retaining the base semantics error (`head() on zero-capacity sequence` or `cannot select from an empty finite domain`). Unreached conditional and short-circuited measure reads can now prove by ranking where the base reported a semantics error. The following general partial-operation measure probe (#1542, in the same release) also covers physical-slot measures, including capacity-zero push/pop and conditional-merge temporaries, so undefined reads are not used as rank values. BMC's rank fast path discharges nothing on either refusal.
- Fixed (#1496): WASM CI retries each browser probe once only on timeout status
  124, retaining attempt history in the step summary and never retrying parity
  violations or other failures. Only the z3js spike's result-timer expiry now exits
  124; FSL browser completion uses a 360-second wall-clock deadline with timestamped
  phase diagnostics instead of a fixed poll count. Parity comparison and outcome
  JSON contracts are unchanged.
- Fixed (#1497): ledger warnings now use kind-specific summaries, translations and advice instead of treating every non-migration warning as vacuity. Deadlock, coverage and incomplete probes retain their distinct meanings; unknown kinds remain visible generic warnings. Existing vacuity and migration rows, raw evidence, assurance and exit behavior are preserved. Affected approvals report `drifted` for `rendering_changed` (exit 0) and require review and re-approval.
- Fixed (#1501): the generated language reference keeps wrapped line-start issue
  references such as `#1448` and `#1200` as paragraph text instead of turning them
  into headings and dropping the hash. Only the heading separator rule changes:
  one to six hashes require a space, tab, or end of line. The installed
  Python-Markdown version's heading-body and closing-hash behavior, including
  escaped hashes, is preserved; fenced code and already escaped paragraph hashes
  are unchanged. Lines beginning with seven or more hashes now remain prose rather
  than becoming h6 headings, matching CommonMark and GitHub.
- Fixed (#1506): Reject overflowing causal analysis maximum-time sums with the
  contributing claim's location instead of silently saturating. Timeline SCC-internal
  sums only include components reachable from the intervention being evaluated.
  This restriction does not apply to declared feedback cycle sums in graph or review analyses.
  Representable results, output schemas, and source-check acceptance are unchanged.
- Fixed (#1511): Make the #1475 chain regression expect native path separators while retaining full JSON equality, and gate the newline-filename approval regression to Unix because Windows rejects its fixture filename. Product output and Unix refusal coverage are unchanged.
- Fixed (#1513): admit both selected Git diff spec roots and approval baselines,
  with revision:path diagnostics and one complete legacy warning per selected slot,
  including duplicates. Preserve pair-local refusal, old/live primary metadata,
  approval signature/path/digest precedence and historical dependency/mapping scope.
- Fixed (#1519): Symbolic verification retains every live element of full-push and literal sequence temporaries through supported conditional expressions and comparisons, while state assignments remain capacity bounded. Unchecked oversized frames and incomplete non-bound or ranked induction evidence are rejected; checked action-bound diagnostics keep actual attempted values, and genuine bounded leadsTo counterexamples are preserved. Initial values are not reconstructed from actions: symbolic projection fails closed when their elements are unrepresentable, whereas the default concrete pre-pass can report type_bound.
- Fixed (#1520): explicit, Monitor and refinement now resolve static filtered and collection init forall domains for binder-key coverage. Current-key reads after writes are supported when selected keys cover the root; empty and state-dependent domains still cannot fabricate coverage. Check-time read-order rules are unchanged. Refinement now refuses nested fixed-target initialization that depends on an outer binder shadowing a same-named constant, previously incorrectly accepted as refines.
- Fixed (#1531): Verification witness replay honors the run's implicit-bound selection, so selected range-property violations no longer become internal trace mismatches. Explicit verification fails closed with `model sequence length exceeds capacity` (exit 2, `semantics`) when a successor exceeds an unchecked Seq capacity, using BMC's error contract instead of returning `verified`, `proved`, `reachable_failed`, or a replay error. This includes successors that preserve an initial overflow; the initial frame itself is not screened by explicit at any depth, so without an over-capacity successor explicit can still return `proved` where BMC errors. A genuine violating successor may be reported first depending on exploration order: frontier state-value order, then action declaration order within one state. Existing replay APIs and scenarios, mutation, refinement, `fsl-bmc`, and Worker retain all-bounds behavior.
- Fixed (#1541): Retain computed out-of-domain members added to bounded Sets and
  relations through symbolic consumers, state transitions and counterexample
  projection, exposing bound violations when storing the add result back instead
  of false unbounded proofs.
  Relation bounds now appear consistently in selection and proof/report inventories;
  Public Kernel v1/v2 aggregate export retains candidates through collection let
  aliases and shadowed bindings without changing JSON shapes. The existing
  `--exclude-property` limitation for implicit bounds is unchanged.
  Whole-collection assignment from a wider carrier remains unsupported by symbolic
  BMC/induction (exit 2), as on base.
  Relational init copies share complete introduced support, including copy chains,
  conditions and relation pairs. Unchecked Set/relation roots compact only after
  proving finite key support; optional closed-grid ranking preserves lasso search
  when a Set/relation root bound is omitted. Other unselected scalar bounds are not
  assumed by ranking. Static check and concrete init read-order policy are unchanged.
- Fixed (#1542): native induction checks reached partial operations in `leadsTo`
  `decreases` measures before using initial, successor or deadlock-side rank values.
  A possibly undefined measure now produces an exit-2 semantics error naming the
  operation instead of a fabricated rank value or proof. Short-circuit guards,
  zero-slot Seq preflight errors and total-measure solver counts are preserved;
  BMC falls back to its lasso search when ranking refuses the measure. The exit-2
  refusal applies to the rank-proof phase; a real base-BMC counterexample remains
  `violated` (exit 1) if its optional rank diagnosis is refused.
- Fixed (#1543): concrete init evaluation failures now name the same containing
  statement location as BMC, including assignment targets, if conditions, forall
  sources and filters, and inner forall assignments. Explicit and auto no longer
  lose the location of an out-of-domain init Map write.
  CLI and Worker JSON messages for concrete init evaluation
  failures append ` in init at L:C`, including shared Monitor/replay, scenarios,
  and testgen init execution. Existing runtime message stems, error kinds, exit
  codes, and JSON fields are preserved; no structured location is added and
  unsatisfiable-init messages remain unchanged.
- Fixed (#1548): nonconformant ledger implementation logs now add red conformance
  findings. Aggregate assurance stays bounded when induction leaves a bounded leadsTo.
  BMC early safety violations mark tagged reachables `not_run`.
  Unranked leadsTo/reachable rows stay bounded under refinement failure over induction.
  Migration: ledger/HTML rendering digests change for induction with an unranked leadsTo,
  BMC early violations with tagged reachables, a nonconformant `--impl-log`, and
  refinement failure (or an undecided implements seam) over induction. Existing approvals
  report `drifted` (exit 0); review and re-approve affected artifacts.
- Fixed (#1552): Requirements `rule` conditions now accept entity fields inside
  `abs`, `min`, `max`, relation builtins such as `reachable`, indexes, field
  accesses, method calls, `some(...)`, `Set`, `Seq` or struct literals, the operand
  of `is`, quantifier filters or bodies, and aggregate filters or values. Names
  bound by a quantifier or aggregate shadow entity fields within their scope.
  Dynamic range bounds such as `exists x in 0..amount` remain rejected because
  the bounds must be static.

  The rule check now reads these conditions the way the event is lowered. A rule
  previously accepted although its event could never occur, for example one that
  reuses a field name as an `is` pattern inside a quantifier (#1561), is now
  rejected with `rule_conflict_unsatisfiable_guard`.
- Fixed (#1553): Make the #1475 chain regression expect layer-detail warning locations to carry the native path chain passes to check (`.\spec.fsl` on Windows), while the standalone check still expects its argument. Product output and Unix expectations are unchanged.
- Fixed (#1556): A requirements `rule` condition that calls a `def` predicate or uses `stage(...)` of a typed entity parameter or binder, including `<process>.stage(x)`, is now checked as its expanded and lowered form instead of being rejected with `rule_conflict_unsatisfiable_guard`.
- Fixed (#1558): the test-only #1417 preservation control no longer compares the
  process-wide solver memory statistic; stable fields remain compared in full.
- Fixed (#1563): compose regression scratch directories and matching test helpers
  now include a random identity, avoiding collisions when process IDs are reused.
- Fixed (#1568): Concrete explicit verification and replay now treat an empty relation as equal to `Set {}` in either operand order for direct comparisons, and in the direction `some(r) == some(Set {})` for Option-wrapped comparisons, including after removing its last pair. Negated equality and inequality no longer falsely accept an empty relation. Refinement maps using `Set {}` now produce canonical relation-typed abstract state instead of reporting `map_out_of_bounds`.
- Fixed (#1569): A `stage(x)` inside a quantifier or aggregate whose binder is named
  like the implicit entity parameter of a legacy transition, a next-edition event, or a
  property now reads the binder rather than the outer instance. Legacy transitions
  always use `c`; next-edition events and properties use, for example, `c` for `Claim`.
  Previously this could hide violations in transition guards and `set` values, or
  report spurious violations in `always ... has` properties. A process field named
  `c` still leaves `stage(c)` referring to the transition's own entity instance.
- Fixed (#1574): embedded AI component diagnostics now use original-document
  coordinates across AI check, compatibility, replay, native check and shared
  Worker checking. Nested locations and annotation byte spans are translated once;
  standalone locations, parse errors, validity, error kinds and exits are preserved.
- Fixed (#1575): the native language server publishes AI-project component errors
  for undeclared authority tools, unknown hard rules and missing tools at their
  whole-document UTF-16 positions, and clears them after valid edits. Validation
  is shared with CLI/Worker checking without changing their error envelopes.
- Fixed (#1580): induction resolves constants and bare enum members in `helpful`
  arguments, including integer arithmetic, instead of refusing them as unknown names.
  State-dependent arguments remain refused; `check` and BMC verdicts are unchanged; BMC may now skip lasso checks that the ranking discharges.
- Fixed (#1584): the rule-conflict check's copy of a rule guard now renames binders named like generated entity field, relation-role or stage state maps before expanding predicates. The event's own guard already renamed these binders; matching that behavior avoids false rule-conflict rejections.
- Fixed (#1586): the semantic mutation gate's final report schema check reuses
  the initial manifest build instead of rebuilding cargo-mutants' differently
  flagged dependencies. Report validation, the zero-mutant path, and fresh
  per-run mutation build directories are preserved.
- Fixed (#1587): Local expression binders that shadow state names no longer count as state reads
  in frozen-state vacuity warnings, explain trace blame, or document undecided
  requirement associations. This also prevents a spurious symbolic range-bound
  error after explicit verification of nested aggregates with shadowed binders.
- Fixed (#1591): The concrete boundary pre-pass checks only initial type bounds before exploring actions, without evaluating user invariants, so BMC and induction runs entering the pre-pass report an already out-of-bound initial state at step 0 with a one-state trace and no last action, matching explicit even at depth 0. The WASM Worker's verify shares this correction. An over-capacity Seq initial value at depth 0, or an invalid initial value whose first action overflows (including depths 1–2), now returns exit 1 / `violated` / `type_bound` instead of exit 2 / `semantics`. Every `--property` selection bypasses the pre-pass except selected-trans induction; `--exclude-property <user property>` alone keeps it and receives the step-0 correction. Dispatch conditions, supplied-initial-state and nondeterministic-init routes, and the no-actions semantics error are preserved; overflow after a valid initial state still reports the action step. Legacy empty-action-trace replay does not check initial bounds and is unchanged.
- Fixed (#1595): `fslc refine` preserved progress resolves abstract Int/Bool constants in P/Q, binder domains,
  supported nested filters and decreases, retaining local scope. Impl-constant binder
  collisions and read abstract constant/state ambiguities are located type errors;
  unused maps/otherwise legal locals stay admissible. Violations with an abstract constant bound
  and renamed impl constant, or a body constant without an impl namesake, now give
  exit 1 `progress_lost` instead of exit 2 errors. Refs #1595.
- Fixed (#1596): dividing the minimum i64 value by `-1` or taking its remainder
  now reports integer overflow instead of panicking in runtime evaluation and
  constant folding; finite-domain evaluation defers the overflow like other
  checked arithmetic. Init errors retain the containing statement location, and
  zero-division behavior is unchanged, including totalized property contexts.
- Fixed (#1597): BMC and induction report undefined init operations before bounds on init-assigned roots can hide them. An empty-sequence `pop()` flowing into a bounded scalar or sequence now reports `pop() on empty sequence` at its init statement instead of a type-bound violation or a negative model sequence length. Selected bounds on roots init never assigns remain assumptions of the init-definedness query, preventing out-of-type free inputs from inventing init errors. Assigned sequence roots are first queried within their represented capacity; failures found only without that capacity assumption return the unlocated refusal `model sequence length exceeds capacity`, rather than naming a potentially invented failing operation. This includes init that stores an over-capacity sequence and reads beyond its represented slots: it retains the base refusal for many such forms and changes the base step-0 `type_bound` verdict to that refusal for others. Ordinary representable init bound violations and action-definedness checks are unchanged.

  The BMC/induction exception that prioritized a step-0 type-bound violation when init both assigned an out-of-type value and evaluated an undefined operation is removed. For example, `n: 0..1` with `init { n = 5 t = 1 / 0 }` now returns exit 2 / `error` / `semantics` with the located init division-by-zero message, rather than exit 1 / `violated` / `type_bound` for `_bounds_n`. A defined out-of-type assignment alone still returns the step-0 type-bound violation.
- Fixed (#1598): selected induction now establishes every implicit bound assumed by its step in
  the base case, even with `--property P`. This intentionally changes these verdicts:

  - Exit 0 false `proved` / `unbounded` → exit 1 `violated` / `type_bound`, naming
    the failing `_bounds_*`.
  - Exit 2 `error` / `kind: "semantics"` for an overflowing Seq frame → exit 1
    `violated` / `type_bound` (step 0 for deterministic initial overflow).
  - The shared base also changes `sweep --engine induction --property P` from
    exit 0 `sweep_passed` to exit 1 `sweep_failed` (with `v: 0..3` initialized to
    5 and `--depth 2..3`, it reports `_bounds_v`@0), and `verify --lemma` targets from exit 0 `proved` to exit 1
    `violated` / `type_bound`.
  - `unknown_cti` or `violated P` at the same or a later step → `violated _bounds_*`
    at the first failing base step. Same-step examples: `n: 0..1`, init 0,
    increment, `P { n <= 1 }` at depth 2 changes `P`@2 to `_bounds_n`@2; a Set
    initial copy with `K = 0..1`, `init { x = 2 t = Set { x } s = t }`,
    `P { not s.contains(2) }` changes `P`@0 to `_bounds_s`@0 (both remain exit 1
    `violated`, now `type_bound`).

  Sibling user invariants stay unselected; standalone BMC/explicit selection is
  unchanged.
- Fixed (#1599): use one core declaration merge for refinement typechecking, concrete evaluation
  and preserved-progress verification. Pulled-model safety violations now fail
  closed with a non-solver semantics error (exit 2), naming the property and pulled
  model without a trace, instead of being discarded as successful refinement.
- Fixed (#1603): invalid written next-edition rule-property names, including `__`
  and literal names such as `none:`, now report `kind: "name"` instead of
  `kind: "semantics"` and include structured `loc` at the statement's start, not
  the label. Exit 2, `diagnostic_code: "invalid_rule_property_name"`, and the
  diagnostic wording are unchanged. When checking a file, the message suffix
  changes from `at L:C` to `at <source-file>:L:C`, like other located name errors.
- Fixed (#1607): BMC and induction name the first reached concrete failure inside
  quantified and aggregate init expressions, matching explicit and auto. Diagnostics
  respect candidate membership, filters, quantifier short-circuiting, and each
  running sum's overflow while preserving the containing init statement's location.
  Symbolic CLI and Worker JSON messages for binder init failures
  replace `undefined evaluation` and `integer overflow in sum or undefined summand`
  with the concrete source, filter, body, or summand failure, or `integer overflow
  in sum` for accumulation overflow. Runtime messages, the ` in init at L:C` suffix,
  error kinds, exit codes, and JSON fields are unchanged.
- Fixed (#1615): Keep merge-group Rust caches restore-only alongside pull requests,
  while retaining push, scheduled and manual cache saves. Attribute known and generic
  Rust caches on merge-queue refs in the cache-budget audit without duplicate findings
  or changing the physical-budget threshold.
- Fixed (#1622, #1577): pattern payloads now stay within their true continuation
  in symbolic verification and concrete Monitor/solver-free BFS evaluation,
  including raw/expected-type conditionals, guards and composed initializers.
  Closed expressions restore outer values and declared binding types; legal
  conjunctive requires flow and all existing source collision refusals remain.
  Diagnostic evidence retains successful payload matches actually reached in selected
  candidate filters and closed bodies, including non-exporting children, eager
  arguments, optimized indices and nonquantified fallback, in both top-level and
  blame witnesses. Evidence never supplies identifier lookup; skipped children,
  other candidates and nested binder-local captures remain excluded. Original
  binder values and first-observed keys retain their diagnostic identity.
- Fixed (#1628): Non-Seq induction bases skip the redundant concrete boundary
  search only after symbolic BMC verifies every assumed bound. Seq evidence and
  non-success priority are preserved; the depth-20 collection deadline is unchanged.
- Fixed (#1644): the explicit-first unreadable-catalog mutate test now expects the platform's own I/O error text so it also passes on Windows; product output and Unix expectations are unchanged.
- Fixed (#1649): cross-platform chain acceptance evidence: expected manifest-relative
  paths now use the platform path join, including dependency warnings and missing
  input diagnostics. Complete output and topology comparisons remain exact;
  native command behavior is unchanged.
- Fixed (#1652): native error-envelope parity tests now require complete check/verify version metadata independently of cache settings, while preserving its absence on specialized commands. Missing, corrupted and unexpected metadata remain rejecting controls; CLI output is unchanged.
- Fixed (#1654): specialized preflight tests now validate complete native version metadata consistently with cache enabled or disabled.
- Fixed (#1657): removed a Clippy warning in name display without changing prose or alias output.
- Fixed (#1670): keep bool-closure repeat comparisons exact except for validated native-Z3 memory
  allocation telemetry on the observed queried BMC bounded proof route. Add
  test-only accepting and rejecting comparator controls; product semantics are
  unchanged.
- Fixed (#1674): make Windows test fixtures portable by preserving LF bytes in the edition test's
  scratch Git repository and using numeric case labels instead of FSL types and
  expressions in recursive-context fixture paths. FSL inputs and expected results
  are unchanged.
- Fixed (#1675): escape external requirement intent, undecided reasons, approval display fields
  and paths at their Markdown context boundaries; contain quoted lines and ledger
  CR/LF/HTML data without rewriting generated markers or semantic claims. Benign
  Unicode output and glossary formatting are preserved. Affected old documents
  and approvals require regeneration/review; digests always bind actual new bytes.
  The requirements overview now also contains repeated rule intent, authored
  rule/decision comments and accepted CR description data; validated source
  glossary aliases and Mermaid diagram bytes are unchanged.
  Mixed HTML-code fallback values now preserve literal Markdown punctuation, and
  ledger producer results and external finding summaries escape their free inputs
  without rewriting generated syntax or raw evidence. Marker-like first inline
  evidence results remain accepted; subsequent malformed marker lines retain the
  legacy refusal in their actual generated list-row context.
  Literal display also contains GFM strikethrough/autolinks and quoted ordered
  list prefixes. Evidence-source rows retain their original raw-row ordering and
  multiplicity for both marker validation and escaped display.
  Email display also contains underscored domain labels; deeply indented quoted
  intent uses the whole raw line's literal code context, while shallow whitespace
  and the benign single-tab form retain their existing display bytes.
  Deep quoted backtick code preserves raw boundary ASCII spaces with one guard at
  each edge when needed, without padding all-space lines or changing other code slots.
- Fixed (#1678, refs #1281): a `fslc mutate` progress line now reads `engine=secondary, oracle=<route>` when the main oracle was clean and a later check (requirements, implements, or the init-bounds override) decided the mutant's `killed`, `inconclusive`, or `error` status. It used to show only the route, for example `killed acceptance (engine=explicit)`, which read as if explicit had produced that status. Main-oracle lines are unchanged. Stderr only; stdout is unchanged.
- Fixed (#1693, refs #1480): a range admission test no longer pins Linux's wording of a missing-file error, which failed the Windows native Z3 job on `main`. No product behavior changes.
- Fixed (#1698): `rust tests` shards ran out of disk while linking test binaries on some
  hosted runners, which failed the merge queue. Each shard now removes the
  preinstalled Android, .NET and GHCup toolchains before it builds, and prints free
  space before and after the removal and at the end of the job. Tests, Cargo profiles
  and cache keys are unchanged.
- Fixed (#1699): a `sweep` test no longer names its cache directory from the process id and the clock alone, which collided on Windows, where tests run in parallel and the clock is coarse. No product behavior changes.
- Fixed (#1702): the WASM browser test's #1467 recursive repeat comparisons no longer
  compare Z3's ambient allocator footprint (`cost.solver.memory_mb`), which
  differed by 0.01 MB between identical native runs and failed the merge queue
  with `parity_violation`. Each repeat packet must still report a present, finite,
  positive value; every other field, including conflicts, decisions and
  propagations, stays exact, and the cross-runtime comparison is unchanged. No
  product behavior changes.
- Fixed (#1712): tests that named temporary specs and directories from a label, the process id and the clock now also add a per-binary counter. Two tests with the same label, such as the bmc and induction tests of one #1481 cell, could build the same path when the clock read the same value, which happens on macOS, where the clock has microsecond resolution; one test's cleanup then deleted the other's file. No product behavior changes.
- Fixed (#1730): the VS Code extension highlights FSL 5. Its TextMate grammar had not changed since #1377 and knew none of the FSL 5 requirements keywords. It now highlights the `fsl 4` / `fsl 5` header line, and after an `fsl 5` header every active keyword of `rust/fsl-syntax/src/keywords.rs` in English (a multi-word keyword such as `only when` or `belongs to` as one phrase) and in Japanese, the particles が・を・に・で・は, and the FSL 5 declaration heads with their names (`event`, `rule`, `decide`, `scenario`, `出来事`, `規則`, `決定表`, ...). A Japanese keyword or particle is highlighted only as a whole word, never inside a name such as `支払件数`. Headerless and `fsl 4` documents are tokenized exactly as before. A new test, `rust/fsl-syntax/tests/vscode_grammar_keywords.rs`, fails when the grammar misses an active keyword or particle of the table or keeps one the table no longer has.
- Fixed (#1731): `install.sh` no longer keeps the copy of a skill it replaces
  inside `~/.claude/skills/`. Claude Code loads every directory there as a
  skill, so a backup named `<name>.pre-native-v3` was offered next to the live
  skill as a second, stale copy. A replaced copy now goes to
  `<data>/skill-backups/claude/<name>-<UTC time>` under the FSL data directory,
  and the installer prints the path. On its next run with skills enabled, the
  installer moves each `fsl*.pre-native-v3` that an earlier version left in
  `~/.claude/skills/` into the same backup directory, unchanged and under the
  same name, and prints each move. It removes nothing, and it stops before
  linking anything when a backup path already exists. The README describes the
  backup location. Its uninstall command now honours `FSL_DATA_DIR`,
  `FSL_INSTALL_DIR`, `XDG_DATA_HOME` and `FSL_BIN_DIR`. It does nothing unless
  the data directory holds a `current` link and a `releases` directory. It removes
  a command or skill link only when the link points into that directory, then
  removes `current` and `releases`, and keeps `skill-backups/`. It used to delete
  a skill link another tool placed, and a real directory under a skill name, both
  of which the installer itself leaves alone. The README's macOS
  `xattr -d com.apple.quarantine` step no longer fails when the file has no such
  attribute.
- Fixed (#1734): `fslc lint --edition current` and `fslc migrate --edition current` follow the document's edition header, as `check`, `verify`, `domain check` and `fmt` already did. They read `current` as an explicit `4`, so an `fsl 5` document was refused with `edition_conflict` and the message "the document declares 'fsl 5' but --edition 4 was requested", although the user had typed `current` and the upgrade guide recommends it. `current` on a headerless document still reads it as FSL 4 (for `migrate`, instead of its next-edition default), and the explicit conflicts are unchanged: `--edition 4` on an `fsl 5` document, and `--edition 5` or `next` on an `fsl 4` document, remain `edition_conflict`.
- Fixed (#1735): `fslc migrate --write` no longer rewrites a read-only input.
  Since `migrate` was added (3.0.0; observed on 4.12.0), it replaced a changed
  mode-0444 file through a sibling temporary file and a rename, reported
  `"written": true` and exited 0, leaving the new bytes read-only. It now refuses
  such an input with the same `io` error as a read-only `-o` target (#1419):
  `output target is read-only`, exit 2. The check runs over the whole write set
  before any temporary file is prepared, so one read-only input leaves every input
  unchanged; a temporary file prepared for an earlier input is also removed when a
  later input's temporary file cannot be created. Writable inputs are rewritten
  as before, keeping their permission bits.
- Fixed (#1736): `fslc refine` with `preserve progress` is fast again. Since #1467 the symbolic evaluator types every equality, conditional, `let` and collection binder against the model, and each of those calls rebuilt the model's whole type environment (every state root, constant and enum member). The environment is now built once and reused while the model's state, constants and types are unchanged, so every inferred type is the one the rebuilt environment gave. On one machine `refine` of `examples/multi_agent_system` design → requirements at `--depth 8` went from 6.60 s back to 5.02 s (4.58 s in v4.12.0).
- Fixed (#1737): native artifact output refuses a destination that is the same file as an input
  the command reads, before anything is written: `fslc html s.fsl -o s.fsl` no
  longer replaces the spec with the report. The spec, imported files and file
  options such as `--evidence`, `--glossary`, `--approval` and `--artifact`
  count, compared by file identity, so `./s.fsl`, a symlink and a hard link to
  the input are refused too. The refusal is a `kind: "io"` error with exit 2, in
  the envelope of the existing non-regular-target refusals; it covers `html`,
  `ledger`, `testgen`, `counterexample export`, `document generate`/`claims`,
  `db import`, `domain expand`/`generate`/`testgen`, `approval create` and
  `causal observe-expectations`.
- Fixed (#1739): the VS Code extension `.vsix` no longer ships a vulnerable
  `brace-expansion`. The `editors/vscode` lockfile is refreshed with
  `npm audit fix` (no `--force`, no direct-dependency or `package.json`
  change): the runtime `vscode-languageclient` -> `minimatch@5.1.9` ->
  `brace-expansion` moves from 2.1.1 to 2.1.7, clearing its six high
  denial-of-service advisories, and the build-only toolchain under
  `@vscode/vsce` and `@azure/identity` takes in-range updates of `undici`,
  `fast-uri`, `js-yaml`, `markdown-it`, `@azure/msal-node` and `qs`.
  `npm audit --omit=dev` now reports no advisory; the full audit drops from
  11 (7 high, 4 moderate) to 3 moderate, all in `glob`/`minimatch` under the
  build-only `@vscode/vsce@3.9.2`, whose fixed versions lie outside that
  release's dependency ranges. The packaged `.vsix` keeps the same 210 files;
  only the three `brace-expansion` files change.
- Fixed (#1753): `fslc document generate`, `document claims` and
  `approval create --kind requirements_document` refuse an `-o` destination that
  is the abstraction a requirements document names in `implements ... from`, also
  through `./`, a symlink or a hard link, although they never read that file:
  `document claims req.fsl -o abs.fsl` no longer replaces the abstract spec with
  the claims JSON. The refusal is the #1737 `kind: "io"` error with exit 2,
  before anything is written.
- Fixed (#1755): `fslc migrate --write` keeps a document's line endings and byte
  order mark. On a CRLF document, the lines it inserted or rewrote ended in LF
  while the other lines kept CRLF, and a document the formatter rewrote (string
  metadata, inline maps, domain enums, colon quantifiers) came back LF-only
  without its UTF-8 byte order mark. Every line of the written document now ends
  as the input's first line does (CRLF or LF), the mark is kept exactly when the
  input has one, and the dry run's replacement holds the same bytes. LF inputs
  are written as before. `fslc fmt` is unchanged: it still prints LF without the
  mark.
- Fixed (#1756): `fslc --help` and the `root.help` string of `cli-contract.json` list `testplan` and `counterexample`, which already worked and were in `root.commands`. A bare `fslc` reports the root usage line, which names every top-level command, instead of `usage: fslc <check|verify> SPEC [options]`; it stays a `kind: "usage"` envelope on stdout with exit 2. `fslc help [COMMAND [SUBCOMMAND]]` prints the same help as `fslc [COMMAND ...] --help` with exit 0, where 4.12 reported `unknown command 'help'`; a word after `help` that names no command is the usual unknown-command usage error. A bare `fslc document` names `check`, and a bare `fslc domain` names `replay` and `testgen`. A conformance test now derives every root and family command list, and each bare usage line, from `cli-contract.json`, so a command missing from one fails it.
- Fixed (#1757): `fslc lint`, `fslc migrate` and `fslc fmt --check` publish the edition a document was read in, as `check`, `verify` and `domain check` already did, instead of the requested `--edition`. An `fsl 5` document linted without `--edition` reported `"edition": "current"`, `migrate --edition current` on it said `current`, and `migrate` on an `fsl 4` document said `next`; they now report `next`, `next` and `current`, and add `"fsl_edition": 4` or `5` when the document has a header. Over several documents `edition` is `next` only when every one was read in the next edition, and `fsl_edition` appears only when every one declares the same header. These reports still always carry `edition`, so a headerless document reports what 4.12 did. An `edition_conflict` message quotes `--edition` as typed: `--edition next was requested`, where it said `--edition 5`. Exit codes and verdicts are unchanged.
- Fixed (#1764): the `migrate` multi-file refusal test now reaches the refusal.
  Its "safe" input failed `check`, so `migrate` exited 2 before planning the
  refused input and the test passed for the wrong reason. The safe input now
  checks and is rewritten when migrated alone; the test asserts the
  `migration_refused` result, the refused file's `unsupported_double_ampersand`
  finding and `written: false`, and fails when the all-or-nothing guard is
  removed. Test-only; `migrate` behavior is unchanged.
- Replaced (#251): Inline branch maps in the public return example with explicit
  fair actions and local `implements` correspondences. The public refinement
  correspondences intentionally change from `submit.b1` / `submit.b2` to
  `submit_small` / `submit_large` as a result of canonicalizing the example,
  without changing its guards, effects, fairness or typed requirement metadata.
- Required (#251): Preserve legacy return-branch and generated-name regression
  evidence in dedicated fixtures so those controls keep running when the public
  return example is canonicalized.
- Required (#1466): run all three native-CLI Python test modules in the required
  Rust workspace CI job, and reject stale generated injection matrices. Remove
  the repaired quantified-vacuity gap expectations and preserve failing ledger
  exit codes in the native behavior tests.
- Required (#1542): Pin undefined successor decreases measures when both the
  leadsTo trigger and goal become false, and when a non-helpful action leaves a
  helpful obligation pending. Native CLI regression cells require induction to
  refuse undefined rank evidence and preserve BMC and explicit-engine verdicts.
- Unified (#1170): in `fslc`'s `main.rs`, `causal.rs`, `refinement.rs`, and
  `verification.rs`, a pair of `error_output(..)` or `io_error_output(..)`
  with a hand-written `2` or `3` that equals the `Verdict` table's code now
  takes its exit from `fslc_rust::outcome::error_exit` (266 sites). Other
  shapes keep their literal: a bare `(output, 2)`, pairs built from
  `surface_parse_error_output` or `semantic_error_output`, `process::exit`,
  `"exit_code"` fields, and `run_document_claims`' `internal` error with 2.
  The #1170 ratchet counts what is left (`main.rs` 170, `verification.rs` 29,
  `causal.rs` 14, `refinement.rs` 3, and the other files' literals). `chain`
  builds its nested error details through the `cli_output.rs` envelope owner
  and its layer input-error records through `chain_step_input_error`; its
  failed refine-run and `[impl]` command records are unchanged. Command
  output, error `kind`s, messages, and exit codes are unchanged.
- Sharded (#1532): Split Windows native-Z3 tests into long-poles and rest shards without narrowing tests,
  check the skip names against the live test list, and preserve the existing check name
  with an aggregator that requires shard success.
- Sharded (#1562): the test-only compose binding matrix and glossary output checks
  now run independent CLI observations concurrently while preserving coverage and report order.
- Sharded (#1582): the collection binder matrix now runs independent cells
  concurrently while preserving all 349 cells, per-cell checks, and report order.
  Elapsed time remains diagnostic rather than a runner-dependent pass/fail budget.
- Documented (#251): Move the cancellation example's four action correspondences
  into its local `implements` block while preserving its checked Kernel and typed
  requirement metadata. `docs/design/DESIGN-migration.md` records the manually
  applied edits and that whole-file `fmt --check --edition next` still exits 1,
  as it does for the original example.
- Documented (#1335): `LANGUAGE.md`, `LANGUAGE.ja.md`, the `fsl` skill's
  `syntax.md` and `DESIGN-def.md` now record the 5.0.0 compose `def` change as
  a breaking removal. `fslc` 4.12.0 `check` accepted, with exit 0, an unused
  compose-level `def` with a repeated definition or parameter name, direct or
  mutual recursion, or a call in its body to an undefined predicate or with the
  wrong arity; 5.0.0 rejects each with exit 2 and the direct-spec error. The
  4.12 to 5.0 table of refused forms gains a row, and `DESIGN-def.md` records the
  measured transitions. A new regression test in
  `issue_1335_compose_predicate_expansion.rs` covers the forms that existing
  tests did not.
- Documented (#1347): a requirements transition's `by X` is a free label.
  The requirements layer has no `actor` declaration and `check` does not look
  the name up, unlike `business`, which rejects a transition whose actor is not
  declared. The label is compared with the business actor only when a
  refinement auto-maps the transition to the same-named business action (via
  `maps auto`, or via the inline `implements` seam's implicit auto-mapping of a
  transition no explicit inline `action` item names) and that refinement is
  evaluated; a `verify` scoped with `--property` does not evaluate the inline
  seam, and `fslc diff` without a mapping builds its own `maps auto`
  refinement. A transition mapped by an explicit `action` item, or one the
  inline seam maps to `stutter` (no same-named business action), is not
  compared. `docs/manual/LANGUAGE.md` and `docs/manual/LANGUAGE.ja.md` §13.2,
  `skills/fsl/references/layers.md`, and `skills/fsl-requirements/SKILL.md` say
  so; behavior is unchanged.
- Documented (#1432): documented and pinned the existing canonical JSON rules for
  causal evidence digests, including UTF-8 key order, exact integers, serde_json
  float formatting, and string escapes. Serialization and stored digests are
  unchanged; RFC 8785/JCS conformance remains a separate follow-up (#1526).
- Documented (#1437): the published `fsl 5` edition. `docs/manual/LANGUAGE.md` and `LANGUAGE.ja.md` now describe a document headed `fsl 5`, paired English/Japanese. They cover the requirements forms with their English and Japanese keywords: entities, lifecycles, events, all rule forms, decision tables, glossaries and contextual Japanese keywords, relations, resources, projections, goals, policies and scenarios.

  The edition guide states that only the header selects the syntax, while `--edition` (`current` / `next`, or `4` / `5`) selects the edition policy. On a headerless document, `--edition 5` still parses FSL 4 and only applies the next edition's findings. The guide also records how JSON reports the edition (`"edition": "next"`, `"fsl_edition"`).

  The FSL 4 to FSL 5 migration guide covers `fslc migrate --edition 5`, its findings including `derived_terminal_added`, and `migrate_manual_required`. It also states that `causal`, `agent` and other AI-dialect declarations, `refinement` and `compose` documents are not offered a header.

  New sections record the FSL 5 refusals of repeated step inputs (#1682), actors and requirement-ID owners (#1705), and `concept` (#1686). The `verify` blame note now states that `violating_bindings` names the outermost quantifier only. The FSL 4 reference is retained, and the README routes to the new sections. The site renderer keeps checked FSL examples in code blocks and the prose that follows outside them.
- Documented (#1440): `docs/manual/LANGUAGE.md` and `LANGUAGE.ja.md` gain
  "Upgrading fslc from 4.12 to 5.0", for anyone moving the binary from 4.12
  whether or not they adopt FSL 5. It lists the forms 5.0.0 refuses that 4.12
  accepted, headerless documents included, with what `check` or `refine` reports
  and the repair, and the CLI, JSON and exit-code changes with what to search for
  and change. The edition guide no longer implies that a headerless document
  keeps every 4.12 result. The breaking removals #1256, #1452 and #1595 are now
  recorded with their issue numbers in both language references,
  `skills/fsl/references/syntax.md` and their design notes; #1452 is measured
  against pre-release builds, since 4.12 refused every `fsl 5` child. Breaking
  fragments in `changelog.d/` use one of two labels:
  `<Category> (#N, breaking removal):` for a form that was accepted and is now refused, and
  `<Category> (#N, breaking):` for other breaking changes such as CLI, JSON,
  exit-code, record-format or Rust API changes; and
  `docs/release-notes/v5.0.0.md` summarizes the release for a 4.12 user.

  The guide also corrects its 4.12 column:
  plain `fslc migrate` already targeted `next` in 4.12 but planned nothing for a
  headerless document, and 4.12 already refused some foreign options, such as
  `explain --engine`, while it ignored others, such as `version --depth`. The
  guide now says that a 4.12 ledger, HTML or scenarios approval whose artifact
  5.0 renders differently reports `drifted` with `rendering_changed` and needs
  re-approval, with counts over the repository's `examples/` and `specs/`, and
  that `renderer_changed` is not reported for a 4.x record. The FSL 4 to FSL 5
  migration guidance says how to answer the `never_rule_without_forbidden`
  warning a converted process form can add: add a `forbidden … then rejected`
  trace for each guarded rule.
- Documented (#1483): helpful arguments are type-checked against action parameters
  by #1527; state independence remains an induction-only restriction, not a check
  or BMC restriction. Both language references and the induction design record agree.
- Documented (#1624): native `verify --property` and `sweep --property` help
  now state that induction still checks every implicit type bound in its base
  case. The English, Japanese, and skill verify synopses match; verdicts,
  exit codes, and assurance are unchanged.
- Documented (#1630): distinguish user-property selection from induction's
  implicit type-bound base checks in the FSL command reference, and mark the v1
  property-selector description as historical with a link to the current contract.
- Documented (#1766): `examples/fsl5/` ships the first `fsl 5`-headed examples: one
  leave-request requirements model written with the English spellings
  (`leave_request.en.fsl`) and with the Japanese keywords, particles and glossary
  aliases (`leave_request.ja.fsl`). Both check, lint with no finding, verify to
  depth 8 and prove their invariants by induction; their Kernel JSON is identical
  apart from source locations and the quoted requirement texts. LANGUAGE.md,
  LANGUAGE.ja.md, the README tree and the site's examples pages link them.
  The corpus tests that assumed every corpus document is headerless now read a
  headed document in its own edition: `edition_header.rs` copies it unchanged
  instead of adding a second header, and the LSP corpus test expects the next
  lexicon for an `fsl 5` document. `formatter_cli.rs` formats each corpus document
  only under the edition policies `fslc fmt` accepts for it, so an `fsl 5`
  document is no longer formatted under the `fsl 4` policy its header refuses.
- Decided (#1612): Adopt the main merge queue with squash merging and zero required
  approvals so a single maintainer can enqueue. Record the live queue rule in the
  CI contract and fixture, retain full pull-request evidence with
  `FSL_MERGE_QUEUE_CI` unset, reserve any remaining administrator queue bypass for
  emergencies, and record merge-group cache saves as a known open risk (#1615).

## [4.12.0] - 2026-10-17

- Changed (#1263): a `proved` result from `fslc verify --engine induction` now
  says what the proof does not cover. The step reasons over unbounded integers
  and only the base case (BMC to `--depth`) checks i64 overflow, so a counter
  that overflows beyond `--depth` stays `proved`; the envelope now carries
  `not_covered: ["int_overflow"]` whenever an invariant, `trans`, `leadsTo`
  trigger/goal/measure, `terminal`, guard, body or `ensures` uses an operator
  that can overflow (`+ - * / %`, unary `-`, `abs`, `sum`). The test is
  syntactic: arithmetic on small bounded values carries it too, and a proof
  with no such operator omits the key (never an empty array). `--engine
  explicit` checks overflow concretely and does not carry it. Additive output
  only: no verdict, exit code or other field changes, and the envelope schema
  documents the key. Migration: none required; a consumer that needs overflow
  coverage should treat `int_overflow` in `not_covered` as "check with
  `--engine bmc`/`explicit` at a depth that reaches it, or bound the value".
- Fixed (#1197): a compose component's `until` / `unless` is now rewritten like
  its other properties: the name is reported as `alias.name` and its operands
  read the component's own state. Before, a compose using such a component
  failed with exit 2 (`cannot type identifier`), two components declaring the
  same `until` name collided (`duplicate property name`), and when the compose
  declared state with the same name as the component's, the property silently
  checked the compose's variable instead. Migration: a compose in that last
  shape that reported `verified` can now report `violated` (exit 1) for a
  component property that does not hold; the `until` / `unless` result keys
  change from `U` to `alias.U`.
- Fixed (#1221): `fslc verify --engine induction` no longer reports `proved`
  for a spec that indexes a finite-key `Map` outside its key domain (for
  example `m[cur + 1]` once `cur` reaches the top key) beyond `--depth`. The
  step's definedness obligation now also asks, for every invariant, `leadsTo`
  trigger/goal, `trans`, guard, body (reads and indexed assignments) and
  reached `ensures`, whether a state satisfying the proved invariants reaches
  such a miss; a `sat` answer is `unknown_cti` with the new
  `violation_kind: "key_domain"` and `invariant: "_key_domain_<action>"` /
  `"_key_domain_property_<name>"`. Within depth the base case reports the same
  error as BMC and explicit (`map index outside finite key domain` /
  `map assignment index outside key domain`), so the three engines agree.
  Only sites the obligation catalog cannot place inside the key type are
  asked; in-domain reads such as `forall k: K { m[k] >= 0 }` add no query.
  Checked i64 overflow is still not part of the step obligation (#1263).
  Migration: a spec that was `proved` (exit 0) can now be `unknown_cti`
  (exit 1) when an invariant-satisfying state misses the key domain, or
  `error` (exit 2) when the miss is reachable within `--depth`; guard the
  index (`requires cur < 5`, `cur < 5 => m[cur + 1] > 0`) or, if the CTI start
  is unreachable, add an auxiliary invariant that excludes it.
- Fixed (#1239, breaking): `fslc diff` no longer reports an OLD `forbidden` as
  preserved when a NEW guard disables one of its setup steps. NEW then never
  runs the final step, so nothing shows that NEW still rejects it; the forbidden
  is now `unknown` / `forbidden_replay_failed` with `step`, `action`, and
  `detail: "NEW forbidden setup was not enabled"`, like a NEW setup step that
  violates (#1213) and an OLD setup step that is not enabled. Only a NEW final
  step that its guard disables preserves the OLD rejection. The same judgment
  serves `diff`, `diff --git`, and `approval diff`; no other command changes.
  Migration: a diff that passed `--forbid unknown` (or
  `--forbid forbidden_relaxed,unknown`) may now exit 1 — for example, a NEW that
  adds `requires amount != 10` to the setup `withdraw(10)` and drops the
  overdraw guard the forbidden checks. `--forbid forbidden_relaxed` alone still
  exits 0 on it (the final step was never run, so it is not a relaxation); list
  `unknown` in the gate to catch it. Fix the forbidden's setup so NEW can run it,
  or confirm the change and update the forbidden. Corpus sweep: no self-diff
  changes (`docs/design/DESIGN-forbidden.md` §2.1 and §6).
- Fixed (#1241, breaking): a `forbidden` last step whose argument is not of its
  parameter's kind — an enum member or a Bool for an `entity`, `number`, or range
  parameter (`respond(Waiting)` or `respond(true)` with `c: Case`,
  `withdraw(Red)` with `amount: Amount`), or an integer, a Bool, or another
  enum's member for an enum parameter — no longer satisfies the forbidden as
  `bad_call`. It is a spec typo that no guard and no implementation boundary
  decided, so it is now a `kind: "forbidden"` error (exit 2) with `failed_step`,
  `step`, `step_results: []`, `message`, `wrong_kind_argument`
  (`{parameter, value, type}`), a `loc` at that step, and a hint. A value of the
  right kind outside a declared range stays `bad_call` (#1212), and one outside a
  verify scope stays the #1229 error. One classifier serves `check`, `verify`
  (every engine), `sweep`, `chain`, `scenarios`, `testgen`, `mutate`,
  `counterexample export`, `html`, `ledger`, `approval`, `explain`, `fslc diff`,
  and the Worker's `check` / `verify`; `fslc ledger` names the malformed
  argument. Migration: such a spec used to pass `check`, and `scenarios` /
  `testgen` emitted a test asserting `bad_call`; every command that runs the
  forbidden gate now exits 2, except `explain` (exit 0, no witnesses; #1242),
  `approval create --kind ledger` (exit 0, records the ledger listing the
  error; #1243), and `approval check` of a ledger record created before
  (`drifted`, exit 0); `approval check` of a `scenarios` or `html` record created
  before exits 2. In `fslc diff` such an OLD final step is `unknown` /
  `forbidden_replay_failed` where it was preserved, and an OLD `bad_call` whose
  argument NEW's parameter type makes a wrong kind is `unknown` /
  `forbidden_step_unrelatable`. Fix the step to a value of the parameter's type.
  Corpus sweep: no corpus forbidden changes (`docs/design/DESIGN-forbidden.md`
  §2.1 and §6).
- Fixed (#1250, #1301): every native `fslc` subcommand now accepts its options before, between, or after its positionals, as the `--help` usage lines (`rust/fslc/cli-contract.json`) and the frozen Python CLI already did. `fslc verify --depth 3 spec.fsl` no longer fails with `unknown verify option '3'`, `fslc check --no-cache spec.fsl` is a `usage` error instead of an `io` error about a file named `--no-cache`, and `fslc refine --depth 3 impl abs map` runs the refinement instead of returning an `io` error with empty `chain` entries. A value-taking option always consumes its value, and a trailing value-taking option with no value or an unknown option stays a `usage` error (exit 2). The spec-first order is unchanged. Migration: a bare `--` now ends option parsing (argparse semantics), so `fslc verify -- -odd.fsl` names a spec that starts with `-`. Such a token without `--` is now read as an option, so it is reported as an unknown option instead of being opened as a file. `fslc analyze --focus`, `--profile`, and `--export` with no value are now `usage` errors instead of being silently ignored. Option errors are reported before spec-loading errors, and an unknown `approval`, `document`, `ai`, or `domain` subcommand is reported as such even when no spec follows.
- Fixed (#1252): `fslc verify --instances` now reaches the abstract side of an
  inline `implements` whose abstract is a `requirements` spec declaring only a
  `process` (no `entity` of that name). The override was dropped for it, so a
  shrunken impl was compared with the full-size abstract and failed with a
  false `refinement_failed` / `map_out_of_bounds` (exit 1). No migration is
  needed: runs that failed this way now report the seam's real verdict.
- Fixed (#1254): a compose sync action now substitutes its arguments into the
  component action with capture-avoiding scoping. A statement `forall`'s
  `where` and range bounds are substituted (they used to reach the Kernel with
  the unbound parameter name, exit 2), its binding shadows a same-named
  parameter (exit 2 on write-index distinctness before), and neither a
  `forall` binding nor a `let` captures an argument or sync parameter spelled
  like it. Migration: a compose whose sync argument shared a name with a
  component's `forall` / `let` binding, or whose `where` read a compose state
  variable named like the component parameter, could report `verified` for an
  action that breaks the component's invariant; it now reports `violated`
  (exit 1).
- Fixed (#1259): `x is some(v)` whose `v` repeats a state variable could make
  bmc and induction disagree with explicit-state: the symbolic engines bound `v`
  to the absent payload even when `x` was `none`, so `p is some(exact) or exact <= 2`
  was `violated` (or "undefined") under bmc/induction and `proved` under
  explicit. `check` now rejects a pattern variable that repeats a name already
  in scope — a state variable, constant, enum member, action parameter,
  quantifier binder, `let`, or an earlier pattern binding (#1157 item 1) — as a
  located semantics error (exit 2), and the symbolic `is some` keeps the outer
  meaning of the name where the operand is absent, matching the concrete
  evaluator. Migration: a spec that used such a collision now fails `check` and
  every `verify` with exit 2 and `pattern binding '<v>' ... rename the pattern
  variable`; rename the pattern variable to a fresh name. No spec in this
  repository's corpus is affected. The frozen Python reference still accepts
  the collision at `check`.
- Fixed (#1275): `preserve progress` inside an inline `implements { }` block is
  now rejected instead of being accepted and silently unchecked. The inline
  seam checks safety refinement only, so `check` / `verify` used to report
  `implements.result: "refines"` (exit 0) for a mapping whose progress the
  standalone `fslc refine` reports as `progress_lost`. Every command that loads
  the document (`check`, `verify`, `scenarios`, `chain`, the LSP, the WASM
  Worker) now fails with a located `kind: "parse"` error (exit 2) at the
  `preserve` keyword. Migration: a requirements spec with `preserve progress`
  in its `implements` block now exits 2; delete that item from the block, put
  the mapping and its `preserve progress` in a standalone `refinement` file,
  and run `fslc refine <impl> <abs> <refinement>` to check the upper `leadsTo`.
  Inline `implements` blocks without `preserve progress` are unchanged.
- Fixed (#1297): a compose-level `leadsTo`, `until`, or `unless` can now read
  component state through `alias.var`, as compose-level `invariant`, `trans`,
  and `reachable` already could. Before, every such compose stopped with exit 2
  (`public Kernel cannot type identifier 'a'`); it now gets a verdict. No
  migration is needed beyond expecting a verdict (including `violated`, exit 1)
  where the error used to be.
- Fixed (#1314): `fslc scenarios` now folds governance `preservation`
  results and an inline `implements` seam into its envelope exactly as `fslc
  verify` does, through the same mapping (#1303). Before this it answered from
  its own bounded run without reading either one, so a document whose
  preservation or seam failed got `result:"scenarios"` / exit 0; only when its
  own kernel failed did it fall back to the `verify` envelope, which carried
  them. A governance document never takes that fallback, so its preservations
  were never read. Now a failing preservation or seam makes the top-level
  `result` the folded verdict (`refinement_failed`, `impl_violated`, or
  `unknown_budget`, exit 1), every run attaches the `governance` / `implements`
  section `verify` attaches (also when everything `refines`), and a governance
  contract error or inline `implements` error that `verify` reports is the
  same located `error` (exit 2). `fslc approval create --kind scenarios` reads
  the same envelope. Migration: this changes `scenarios` exit codes and output
  for governance documents and requirements documents with an inline
  `implements`, and nothing else. In this repository's corpus, exit 0 → 1:
  `examples/refinement_liveness/governance_detects_safety_loss.fsl`,
  `tests/fixtures/chain/requirements_broken_implements.fsl`, and four test
  fixtures; exit 0 → 2: the three governance error examples under
  `examples/gallery/` and two test fixtures; every other governance or
  inline-`implements` document stays exit 0 and gains the section. Fix the
  preservation or seam, or, to keep it advisory, run it as a separate `fslc
  refine` step. A `scenarios` approval of such a document no longer matches its
  recorded artifact digest (`fslc approval check` reports `drifted`, exit 0);
  re-approve it. `testgen`, `explain`, `testplan`, `diff`, `analyze`, and
  `kernel` are unchanged.
- Fixed (#1316): `fslc ledger` on a governance document now states, in its
  Markdown body, the verdict its exit code reflects. A new `## governance
  preservation` section, between the header and the risk list, gives the
  top-level verdict (`検証判定`, the `verify` result after the #1303 fold) and
  every `preservation` in declaration order with its `before` → `after`, the
  controls it preserves, and its own `result`, 🟢 for `refines` and 🔴 for
  anything else. Before this, a failing preservation made `fslc ledger` exit 1
  while the body showed neither the verdict nor any preservation result, so a
  reader of the ledger alone saw no reason for the failure. Migration: only
  ledgers of governance documents change; every other ledger is byte-identical.
  A governance-document ledger approved with `fslc approval create --kind
  ledger` before this release no longer matches its recorded artifact digest,
  so `fslc approval check` reports it `drifted` (exit 0); review the new
  section and re-approve it.
- Fixed (#1353): an unlocated diagnostic after non-ASCII text on its line (for example an `undefined predicate` after a Japanese invariant title) no longer crashes the language server: its fallback span was a character column used as a byte offset. CRLF line ends no longer shift it either, and the structural-finding location fallback reports UTF-16 positions. No migration is needed.
- Documented (#1361): the distributed FSL skills no longer contradict the parser or
  each other. `number` is documented as requirements/kernel-only (a `business`
  spec that declares one is a parse error); `if c then a else b` is valid in any
  expression position; requirements `terminal` uses `stage(c)`; `@requirement`
  and `@undecided` are the taught tag forms (the `"ID: text"` slot is
  `legacy_string_metadata`); a kernel spec cannot declare `acceptance` or
  `forbidden`; the `decreases`-inside-`forall` diagnostic is quoted as the
  parser emits it; the AI-project examples show only the lines the scanner
  reads; broken `§` cross-references now name the file. Release and issue history that no longer changes what a reader does
  ("before #1229", "used to", "(issue #N)") was removed from the references;
  the current rule is kept. The skills no longer point at repository-only paths (`examples/`,
  `docs/`, `specs/`, `schemas/`) that a mise or install.sh user does not
  have; the needed gist is stated in the skill instead. The business, requirements, and design check loops now include `fslc
  lint`, and `--requirements` is taught as a file of IDs (one per line), not a
  comma list. The `examples/e2e` IDs follow the built-in ID policy (`POL-EXPENSE-001`,
  `REQ-EXPENSE-001`, `AC-EXPENSE-001`, `MODEL-EXPENSE-001`, ...): the business
  and requirements layers are lint-clean, and every verdict is unchanged.
  A new distributed skill, `fsl-verify`, owns verification. It
  gives the command sequence per layer and change kind (`check
  --strict-tags --requirements` → `lint` → `verify --depth 8` → `--engine
  induction` → `refine` / `implements.result` → `diff --forbid` → `mutate
  --by-requirement` → `chain`), a table of results that exit 0 or look green
  without passing (bounded `verified`; `proved` only at the run's
  `instances` / `values`; `deadlock`, `no_user_invariants`, and
  `unreferenced_requirement` warnings; `unknown_cti`, `unknown_budget`,
  `not_evaluated` from `--property`, `not_run`, `closure: false`; `diff`
  without `--forbid`; `mutate` survivors), the fix loop that never weakens
  the upper layer, prohibited shortcuts (lower depth or scope, deleted
  properties, ignore flags or `--property` in the completion run), how to
  read `kill_rate` and `--by-requirement`, and a fixed completion-report
  table with proof classes. The business, requirements, design, and
  delivery skills keep their minimal check loop and point to it.

## [4.11.0] - 2026-10-15

- Changed (#1247, #1174): the concrete refinement check (the impl
  self-consistency pre-pass and the correspondence walk behind `fslc refine`,
  `fslc chain`, inline `implements`, `fslc diff`, and `fslc mutate`'s implements
  oracle) costs less per state; what it explores and reports is unchanged
  (stdout byte-identical). Expression reads such as `design[c].st` no longer
  copy the whole state variable, a step copies the state once instead of four
  to five times, the walk reuses one abstract `Monitor` instead of copying the
  abstract model twice per transition, and both searches keep their visited
  states and parent links packed (one canonical byte string per state instead
  of three full copies). On `examples/e2e` with `instances Claim = 4`,
  `values Amount = 0..3`, `--depth 8`, user CPU time drops from 14.6 s to 3.1 s
  and peak memory from 939 MB to 190 MB; with `values Amount = 0..5` the run
  still stops at the 50,000-state budget (`unknown_budget`), now at 34 MB
  instead of 1.2 GB. No migration is needed.
- Changed (#1282): `fslc mutate --oracle-attribution` reuses the main oracle's
  result (its type-bound killer and its verdict) instead of recomputing the
  boundary probe, the bounds-only check, and the whole main oracle for every
  killed mutant; only the per-property isolated models are checked again. The
  `killers` and `by_obligation` output is unchanged, except that a `build_spec`
  killer on a type-bound kill can appear or disappear, as it already could
  when earlier Z3 calls in the same process changed.
- Fixed (#1303): a governance `preservation` whose refinement does not
  `refines` now fails `fslc check` and `fslc verify`, fail-closed. The first
  non-`refines` preservation in declaration order sets the top-level `result`
  through the same mapping an inline `implements` seam uses —
  `refinement_failed` → `refinement_failed`, `violated` (the `after` spec breaks
  its own semantics) → `impl_violated`, `unknown_budget` → `unknown_budget`, all
  exit 1; any other value is `error`/`internal` (exit 3) — while
  `governance.preservations[].result` keeps each preservation's own value.
  `fslc verify` now runs the preservations (at depth 8, like `check`) and
  attaches the same `governance` section; it is folded outside the verify
  cache, so a cached `verified` cannot hide a preservation broken later. The
  same verdict reaches a `fslc chain` spec layer, `fslc ledger` / `fslc html` /
  `fslc mutate` (exit 1 from the shared verify baseline), and the browser
  Worker's `check` and `verify`. Before this, `check` reported `ok` and `verify`
  reported `verified`, both exit 0, so a CI gating on the exit code passed a
  broken preservation. Migration: this changes exit codes of existing runs —
  `check`/`verify` of a governance document with a failing preservation (for
  example `examples/refinement_liveness/governance_detects_safety_loss.fsl`)
  now exits 1 instead of 0, and its top-level `result` is the preservation's
  failure instead of `ok`/`verified`. Fix the preservation, or, to keep it
  advisory, check it in a separate `fslc refine` step whose exit the CI
  ignores. `fslc verify` of a governance document with any governance
  contract error that `check` reports (a delegated business file, a control, a
  `satisfied_by` mapping, or preservation files) now reports the same located
  `error` as `check` (exit 2) instead of `verified` (exit 0), and a preservation
  refinement that fails internally is `error`/`internal` (exit 3) under both
  commands instead of a located exit-2 diagnostic. Consumers that read `verify` envelopes of governance documents
  should expect the new `governance` key.
- Documented (#1300): the distributed skills now say that a rejection rule the
  source states is carried by a checked declaration and a `forbidden` trace, not
  by prose alone (`skills/fsl-requirements/SKILL.md` Guardrails,
  `skills/fsl/references/layers.md`); that implementation conformance checks the
  implementation against the spec, not the intent, so an agent loop is handed
  over only after the spec is confirmed against intent, classifies each
  divergence before editing code, and does not lower the depth or skip vectors
  to reach zero (`skills/fsl/references/impl.md` §9, "Using conformance as an
  implementation loop for an AI agent"); and that such a divergence against an
  unconfirmed spec is a human decision gate (`skills/fsl-delivery/SKILL.md`).
- Documented (#1304): `skills/fsl-design/SKILL.md` now says to declare, for each
  failure the design must exclude, the property it would break (with the
  construct that fits it), and to model transaction boundaries, queues and
  outboxes, and retries as state and actions; that a bounded `verified` says
  nothing past its depth and domain sizes, with routes to `--engine explicit`
  and "Domain sizing"; and that a design change re-runs `verify` and `refine`
  plus `fslc diff --forbid behavior_removed` (also added to the Review/change
  row of `skills/fsl-delivery/SKILL.md` Stage Gates). `skills/fsl/references/impl.md`
  §9 and fsl-design Workflow 7 add that generated tests and conformance vectors
  do not enumerate interleaving or crash points inside one action, so a
  procedure the design declares atomic needs implementation tests at the
  implementation's own transaction boundaries.

## [4.10.0] - 2026-10-12

- Added (#1201, #1202): `fsl_core::obligation::catalog`, a solver-free list of the
  obligations a model owes -- one row per authored site and aspect (type bounds,
  `invariant`/`trans`/`reachable`/`leadsTo` truth and definedness, `terminal`
  definedness, action guard/body/`ensures` definedness and `ensures` truth,
  `init` satisfiability, deadlock freedom), each marked when it is statically
  vacuous. Rows come from the model's declarations rather than from engine
  output, so a check no engine performs still has a row. Nothing reads the
  catalog yet; CLI, Worker and Public Kernel output are unchanged. See
  `docs/design/DESIGN-obligation-catalog.md`.
- Added (#1202, #1221): the obligation catalog also lists `leadsTo` deadlines,
  the ranking-proof rows of a `decreases` clause (measure definedness, lower
  bound, no deadlock, one step row per action, and the `helpful` fairness and
  stickiness rows), and two model-definedness rows on every site: `NoOverflow`
  for `i64` overflow and `KeyInDomain` for a `Map` index outside its finite key
  domain. A `KeyInDomain` row is marked statically vacuous only when every value
  the index can take lies inside the key type, judged from the index forms a
  declaration bounds rather than from the type `check` infers, and provided the
  state satisfies its type bounds. CLI, Worker and Public Kernel output are
  unchanged.
- Added (#1237): `fslc mutate --fail-on-survivors` and `--min-kill-rate R` make a mutation run usable as a CI gate. Either flag adds a `gate` object (`fail_on_survivors`, `min_kill_rate`, `judged`, `survived`, `kill_rate`, `dropped`, `violations`, `passed`) to the envelope; `result` stays `mutated` and the run exits 1 when `gate.passed` is false. `--fail-on-survivors` fails on any judged survivor; `--min-kill-rate R` (a number in `[0, 1]`, otherwise a usage error) fails unless the published four-decimal `summary.kill_rate` is `>= R`. A run that judged no mutant fails either flag with `no_judged_mutants` rather than passing on no evidence; built-in mutants dropped by `--max-mutants` are recorded as `gate.dropped` and do not fail the gate. Without either flag the output and exit code are unchanged, and a baseline that does not verify is still re-emitted with its own exit code. Equivalent-mutant exclusion is not part of this change.
- Changed (#1226): an invocation that passed `--instances` / `--values` naming
  something a `requirements` / `business` document does not declare as `entity` /
  `number` (including a raw `type X = lo..hi` range) used to run with exit 0/1
  and report the verdict of a model the override did not shrink as asked — the
  name was dropped, or, for a raw range `X` that types a requirements process
  field, the override's `lo` replaced the field's default initial value (so
  `--values X=0..1` could flip the verdict); it now fails with exit 2 before
  verification. Migration: override only declared `entity` / `number` / `process`
  names; to shrink a raw range, declare it as `number X` with
  `verify { values X = lo..hi }` and override that, or edit the `type` bounds.
  Separately, `fslc diff` on a `requirements` document whose `verify` block
  carries `values X = a..b` for a raw `type X = lo..hi` that types a `process`
  field, when the scope changed, no longer reports a false `behavior_removed`
  (`old_to_new` `refinement_failed`). It used to forward `X` to the re-scoped old
  model resolved to the type's `lo..hi`, which moved the field's default initial
  value from `a` to `lo`; the re-scoped old model now keeps the document's own
  bound, so only the real differences are reported. Specs without such a field
  diff as before.
- Changed (#1234), **breaking for stored `fslc analyze` node IDs of compose
  specs**: a compose component action now has one public name, `alias.action`,
  built from the `use` alias and the action name, so `use X as a__b` with action
  `c` is `a__b.c` (it was `a.b__c`, and some outputs printed the internal
  `a__b__c`), matching the frozen Python reference. Every command that names a
  component action uses it: verify on every engine (traces, `action_coverage`,
  `action_profile`, `cost.properties[].name`, vacuity warnings, induction CTIs,
  and the BMC `semantics` error `action 'bank.settle' body evaluation has a
  non-partial failure`, likewise for `guard` and `ensures`; it printed `bank__settle`),
  sweep, scenarios, explain, testgen, conformance, mutate, refine, diff, html,
  ledger, analyze, the `untagged` warnings of `check --strict-tags`, and
  `fslc replay`, including names built from the action such
  as `_requires_failed_bank.settle` and `_partial_bank.settle` (a guard partial
  operation's `_partial_op_` prefix becomes `_partial_` in the same release,
  #1244). Lists keyed by these names (`reachables`,
  `action_coverage`, `action_profile`, induction `k_used`, `cost.properties`,
  scenarios `reach_*` and `respond_*`, the leadsTo warnings of `fslc scenarios`,
  refine `action_map` and `progress`, and the ai-review
  conservation findings) are ordered by the name they print, so `acct.go` sorts
  before `acct2.go`, and a glue action `z__a`, printed `z.a`, sorts before the
  component action `z.b`. `fslc refine` success output now prints `action_map`
  keys and values as `a__b.go` (the keys were `a.b__go`) and `progress.*.actions`
  as `acct.back` (it was the internal `acct__back`). A v1 replay trace can now write
  `bank.settle`, the name testgen emits, so a generated test's actions replay
  `conformant` instead of failing with `bad_call`. v1 replay still accepts the
  older `bank__settle` for the same action, looked up in the model's table of
  component actions rather than rewritten; sync actions and non-compose actions
  such as `foo__bar` keep exact-name matching. `fslc replay --from-log` with a
  `maps auto` mapping matches a log action written `a__b.c` as well as the
  `a.b__c` it matched before. Replay's `state_mismatch.action`
  and `bad_call.action` report `bank.settle` where they used to echo
  `bank__settle`. The breaking part: in `fslc analyze` output (the default graph,
  `--profile ai-review`, and `--export tag-review`), the node IDs of a component
  action and of its requires, effect, and ensures clauses move from
  `action:bank__settle`, `guard:bank__settle:0`, `effect:bank__settle:0`, and
  `ensures:bank__settle:0` to `action:bank.settle`, `guard:bank.settle:0`,
  `effect:bank.settle:0`, and `ensures:bank.settle:0`, and edge IDs and endpoints
  follow. The `name` field of those nodes moves the same way (`bank__settle` to
  `bank.settle`, `bank__settle:0` to `bank.settle:0`), and with alias `a__b` their
  `label` spells the action `a__b.c` instead of `a.b__c` (`a__b.c requires 0`).
  `--export tag-review` prints a component
  action's `name` as `bank.settle`, and an `undecided:` record of a component
  action, shown by `html`, `ledger`, and `--profile ai-review`, has
  `declaration` `action bank.settle` and `node` `action:bank.settle`. In the
  project `traceability_graph` projection of `fslc analyze <manifest>`, a
  refinement's `action_map:` and `stutter_map:` node IDs and labels and its
  `maps_action` endpoints use the same name (`design:action:bank.settle`), so
  every edge ends at a node the graph declares; before, a compose layer left
  edges ending at undeclared `action:bank__settle` nodes, dropped the
  `lower_anchor` edges from requirements covering an abstract component action,
  and reported a spurious `traceability_gap`. All of these IDs now differ from
  the frozen Python reference, which keeps `action:bank__settle`; this is
  intended. To migrate, rewrite stored node IDs and `--focus` arguments from
  `alias__action` to `alias.action`: `--focus action:bank.settle` already
  resolved for an alias without `__` and keeps resolving, while
  `--focus action:bank__settle` now fails with "unknown analyze focus node"
  (exit 2). State and property node IDs are unchanged. `fslc document` claim
  IDs and fingerprints are unchanged, because the document projection rejects
  compose specs. For a compose spec, `fslc testgen` now lists the keys of each
  scenario's expected state in state declaration order (`bank.cleared`,
  `bank.pending`, `audit.balance`, `audit.log`, `withdrawn` for
  `specs/bank_system.fsl`) in the vitest, swift, kotlin, dart, and phpunit
  scaffolds, as pytest already did; they used to list the compose spec's own
  state first and component state after it in name order. The values and the
  partial-match check are unchanged, so a generated test passes or fails as
  before. Non-compose specs change in two corner cases. A keyed list now
  orders a name containing `__` by the `.` form it prints, so `step__a`
  (printed `step.a`) sorts before `step2`; it sorted after. `fslc testgen` now
  accepts a state whose name has two `__` separators, such as `a__b__c`
  (printed `a.b__c`), and generates every target; it failed with "testgen
  concrete vector.initial has unknown state fields: a.b__c" (exit 2). Every other non-compose output is unchanged.
- Changed (#1244), **breaking for stored `fslc conformance` and `fslc replay`
  output**: a partial operation reached in an action's guard (`requires` or
  `let`, such as `requires 2 / x >= 0` with `x == 0`) is now named
  `_partial_<action>` by `conformance` (`vectors[].outcome.name`) and `replay`
  (`violation.name`), the name `verify` already gave it on every engine and the
  name a body partial operation already had everywhere. It was
  `_partial_op_<action>`, so the same failure could not be matched between a
  conformance vector and the verify result for one spec. For a compose
  component action the name is `_partial_alias.action` (#1234), such as
  `_partial_bank.settle`; it was `_partial_op_bank__settle` in `conformance`
  and `_partial_op_bank.settle` in `replay` (`_partial_op_a.b__c` for alias
  `a__b` and action `c`). The `kind` stays `partial_op`. To migrate,
  rewrite stored conformance vectors, replay results, or test expectations
  from `_partial_op_<action>` to `_partial_<action>`.
- Changed (#1251), **breaking for `fslc mutate` consumers that read `status`
  or `summary`**: a mutant the oracle could not judge is no longer
  counted as killed. Z3 not being creatable (previously `killed_by:"internal"`),
  a BMC call the solver could not decide (solver `unknown`, a backend failure, or
  an unreadable model; previously `killed_by:"build_spec"` for built-ins and
  `invalid` for external mutants), and an acceptance/forbidden or implements
  oracle error (previously the error text, or `refinement`, as the built-in
  killer and `invalid` for external mutants) now give `status:"error"`,
  `killed_by:null`, and `error:{stage,message}` with `stage` one of `solver`,
  `bmc`, `requirements`, `implements`. These mutants are excluded from both
  sides of `kill_rate`, counted in the new `summary.errored` (also per source in
  `summary.by_source`; like #1262's `summary.inconclusive`, the key appears only
  when non-zero, so a fully decided run's envelope is unchanged), and listed in a
  note. With `--fail-on-survivors` or
  `--min-kill-rate`, any such mutant adds the gate violation `oracle_errors` and
  the `gate.errored` count (when non-zero), so the gate fails closed (exit 1); without a gate
  the exit code is unchanged. A BMC rejection that is a finding about the mutant
  (for example an action body undefined in a reachable state) is still the
  `build_spec` kill. A built-in mutant that does not lower/build is now
  `invalid` with `invalid:{kind:"semantics",message}`, as an external one
  already was, instead of a `build_spec` kill. `error` stays distinct from #1262's budget `inconclusive`; for one mutant
  `killed` > `error` > `inconclusive` > `survived`, decided in one place for
  built-in and external mutants. An implements oracle error is now `error`
  instead of a `refinement` kill (an impl parameter or key domain that differs
  from the abstraction's is a refinement verdict, `map_out_of_bounds`, not an
  error; see #1295). The `_bounds_<state>` attribution
  for a removed init assignment, which used to overwrite every outcome of such
  a mutant, now applies only to a judged one: a kill (including a semantic
  `build_spec` kill) or a survivor is re-attributed to `_bounds_<state>` as
  before, while an oracle error (including the former `internal`) keeps
  `status:"error"` and a mutant that does not build stays `invalid`. The
  verifier marks solver failures with `VerifyError::is_solver_failure`.
  Migration: read `summary.errored` / `status:"error"` alongside
  `killed`/`survived`/`invalid` (`total` = `killed + survived + invalid +
  errored + inconclusive`, absent counts read as 0); a gated run that now fails with `oracle_errors` had mutants whose
  kills were never established — rerun, or fix the
  oracle error reported in `error.message`.
- Fixed (#1021): `fslc html` drew a `verify` result outside a short hand list
  as a neutral `info` badge, so `unknown_budget` and `error` looked harmless in
  the report. The Result badge (hero summary and status section) now has an
  explicit class for every verdict `verify` publishes: `verified` and `proved`
  are `ok`; `violated`, `reachable_failed`, `refinement_failed`,
  `impl_violated`, and `error` are `bad`; `unknown_cti` and `unknown_budget`
  are `warn`. Any other value, or a missing `result`, is drawn as a `bad` badge
  that reads `<value> (unrecognized verdict)` instead of a neutral one, so a
  verdict added later without a class shows up in the report. A test in `fslc`
  checks each listed verdict's class against `outcome_class`, the vocabulary's
  owner, which `fsl-tools` cannot see. Coverage markers and witness `kind`
  badges are unchanged. The `unknown_budget`, `error`, and unrecognized-value
  badges now differ from the frozen Python reference, which draws them `info`;
  this is intended. JSON output and exit codes are unchanged.
- Fixed (#1190): `fslc kernel` failed with `semantics: public Kernel cannot type
  identifier 'k'` on a statement-level `forall k: K { … }` whose body had a
  partial operation that read the binder (`m[k] = s.at(k)`, `m[k] = 1 / (k + 1)`,
  or a correctly guarded `m[k] = if k < s.size() then s[k] else 0`), although
  `verify` and `explain` handled the same spec. The Public Kernel's
  `partial_operations` now expands a statement-level `forall` like a quantifier:
  one entry per candidate for each partial operation in the body and in the
  binder's `where` (previously not listed at all), with the binder replaced by
  the candidate and the failure condition guarded by the candidate's membership
  and `where`. A `forall` body operation that does not read the binder, which
  was listed once with an unguarded failure condition, is now listed once per
  candidate. A statement-level `forall` with no partial operation in its
  binder, `where` or body is still not expanded and lists nothing, so
  `forall k in 0..x { m[k] = 1 }` with a non-constant bound keeps producing a
  Kernel. One that does have a partial operation now needs constant bounds,
  like a quantifier: `forall k in 0..x { m[k] = 2 / x }`, which used to list
  the division once unguarded, now fails with `'x' is not an integer const`
  (the BMC engine already rejected it the same way). Commands built on the
  Public Kernel (`testgen`, `testplan`, `document`) no longer stop with the
  "cannot type identifier" error. Verification verdicts are
  unchanged, and no spec or fixture in the repository corpus changes its
  `fslc kernel` output.
- Fixed (#1191): a partial operation reached in an action's `requires` or `let`
  (`requires s.head() == 0` or `let h = s.head()` on an empty sequence,
  `requires 2 / x == 0` with `x == 0`) made `verify --engine explicit`, the
  default `auto` engine, and `verify --engine bmc` on a deterministic init return
  `result: error, kind: semantics` (exit 2) with the raw evaluation message. They
  now report `violated` / `partial_op` / `_partial_<action>` (exit 1) with the
  same replayable trace the symbolic engine already gave for a nondeterministic
  init, as `docs/manual/LANGUAGE.md` §6 states. The `loc` of such a guard
  `partial_op` is now, in every engine, the span the Public Kernel's
  `partial_operations` gives that site: the `requires` clause, or the action for
  a `let`. It used to be the action body's first statement, or `null` for an
  action without a body (`rust/fslc/tests/fixtures/replay_trace.fsl`'s `partial`
  now reports line 27). A `partial_op` in the body keeps its location. The concrete
  action enumeration that explicit, Monitor BFS, the BMC/Worker boundary pre-pass,
  refinement's self-violation walk, `scenarios`' cover and response walks, and
  trace replay share no longer aborts on such a guard: it is that action's
  `partial_op`, is not a deadlock, and is skipped by walks that only follow
  successful steps. Commands that take a `verify` baseline follow it: `fslc
  ledger` on such a spec now exits 1 and lists the `partial_op` row instead of
  exiting 2 over a ledger that claimed nothing was detected
  (`rust/fslc/tests/fixtures/replay_trace.fsl`). A spec whose guards never reach
  a partial operation is unchanged.
- Fixed (#1226): `verify` / `sweep` `--instances NAME=N` / `--values NAME=LO..HI`
  on a `requirements` or `business` document accepted a `NAME` the document does
  not declare as `entity` / `number` — a typo, or the name of a raw
  `type X = lo..hi` range — and echoed it under `bounds_overrides`. The name was
  dropped, so `--values X=0..2` returned a verdict over the full declared domain
  of `X`, except that for a raw range typing a requirements `process` field the
  override's `lo` silently became the field's default initial value. The
  native CLI now rejects such a name as `docs/manual/LANGUAGE.md` and the frozen
  Python reference already did: `result: "error"`, `kind: "semantics"`,
  `verify instances references undeclared entity 'NAME' at 1:1` /
  `verify values references undeclared number 'NAME' at 1:1`, exit 2 — the same
  message a kernel `spec` already produced. A requirements `process` name counts
  as an entity (it is one after lowering), and the business dialect, which has no
  `number`, rejects every `--values`. Kernel `spec` behavior and every override
  of a declared name are unchanged. `fslc diff`, which forwards a document's own
  `verify` bounds to the re-scoped old model, now forwards only the declared
  names; this changes its output only for a raw range typing a requirements
  `process` field (see the Changed entry).
- Fixed (#1246): the impl self-consistency precondition that runs before every
  refinement correspondence check (`fslc refine` single and chain, the
  `fslc chain` refine layer, an inline `implements` seam in `check`/`verify`,
  governance preservations, `fslc diff`, `fslc mutate`'s implements oracle) now
  stops at the same 50,000-state budget as the correspondence check (#1041).
  It walked the whole reachable set unbounded first, so #1041's original
  reproducer still aborted above 6 GB; it now reports `unknown_budget`
  (exit 1, `states_explored: 50000`) at about 1.2 GB. A precondition cutoff is
  reported exactly as a correspondence cutoff, through the same consumers.
  Migration: an impl whose reachable set within the depth reaches 50,000 states
  now reports `unknown_budget` even where it used to report a decided result
  because the unbounded precondition happened to fit in memory — a
  self-violation found beyond the first 50,000 states (`violated` /
  `impl_violated`), or a correspondence failure the walk found before its own
  budget (`refinement_failed`). Lower the depth or narrow both layers'
  `verify {}` domains. `fslc diff` and `fslc mutate` read this cutoff as they read a
  correspondence cutoff (#1262): where they used to report a self-violation
  found past the budget (`impl_violated` / killed), `fslc diff` now reports an
  `unknown_budget` direction that fails its gate, and `fslc mutate` reports the
  mutant as `inconclusive`.
- Fixed (#1258): `verify --engine bmc` reported `verified` and `--engine
  induction` reported `proved` (exit 0) for a spec whose `init` reaches a
  failing operation, e.g. `init { d = 0  x = 6 / d }` or a finite `Map` read
  outside its key domain, while `--engine explicit` stops on the same spec with
  `division by zero` (exit 2). The symbolic init encoding totalized those
  operations, and a failing deterministic init also disabled the concrete
  pre-pass; a nondeterministic init never had it. BMC (and so the induction
  base case, every `verify_bounded` caller, and the browser Worker) now asks
  after the search whether init is defined: a `/`/`%` by zero, a partial `Seq`
  operation, a `Map` key outside its domain (read or assignment target), or
  checked i64 overflow that init evaluation reaches is a `kind: "semantics"`
  error, exit 2, whose message names the failure with the explicit engine's
  wording and the init statement, e.g. `division by zero in init at 9:5`. It
  supersedes the search's verdict. An operation on the unreached side of an
  init `if`, conditional, or `and`/`or`/`=>` stays defined, and the step-0
  type bounds the search asserts are assumed, as before. An init overflow that
  used to surface as `integer model value is unavailable` now reads e.g.
  `integer overflow in multiplication in init at 9:5`.

  Migration: a spec that was `verified`/`proved` only because of this defect
  now exits 2; guard the init operation with an init `if` (or fix the initial
  value). No spec under `specs/`, `examples/`, or the test fixtures changes
  verdict, exit code, or message. When init contains an operation that can fail,
  `cost.properties` gains one `{"kind": "init", "name": "definedness"}` entry
  (and `cost.solver.checks` counts it); an init with nothing that can fail builds
  no solver term and asks nothing, so its output is byte-identical.
  `fslc kernel`'s `partial_operations` and `fslc explain`'s `auto_checks` still
  list action sites only.
- Fixed (#1260): `fslc kernel` listed a partial operation in the body of a
  statement-level `if` with an unguarded failure condition, so for
  `if gate { y = 2 / x }` it printed `x == 0` although the division is never
  evaluated when `gate` is false (verify and the runtime already agreed it is
  not). The Public Kernel's `partial_operations[].failure_condition` is now
  guarded by the `if` condition, `gate and x == 0`, as an expression-level
  `if-then-else` already was; an operation in the `else` branch is guarded by
  `not gate`, and nested statement `if`s and `forall`s stack their guards
  (`forall k: K { if k > 0 { m[k] = 2 / x } }` lists one entry per candidate,
  each guarded by `k > 0` with `k` replaced by the candidate). An operation in
  the `if` condition itself stays unguarded by it. Kernel consumers that read
  `failure_condition` (`testgen`, `testplan`, `document`) see the narrower
  condition. Verification verdicts are unchanged, and no spec or fixture in
  the repository corpus changes its `fslc kernel` output.
- Fixed (#1262), **breaking for `fslc diff` / `fslc mutate` consumers**: `fslc diff`
  and the `fslc mutate` implements oracle no longer treat a refinement
  correspondence check cut off by its fixed state budget (50,000 states, #1041)
  as a pass. A `fslc diff` direction that hits the budget now reports
  `result: "unknown_budget"` with `states_explored` (no `checked_to_depth`)
  instead of `refines`, and the diff adds an `unknown_budget` finding (also in
  `summary`) that fails the gate whatever `--forbid` says, like `impl_violated`;
  `fslc diff --git` and `fslc approval diff` report the same and exit 1. Before,
  both directions of such a diff could report `refines`, giving
  `no_semantic_change` / exit 0 even under `--forbid behavior_removed` when the
  difference lay beyond the budget. A `fslc mutate` mutant whose implements check
  hits the budget is now `status: "inconclusive"` with
  `inconclusive: {"reason": "unknown_budget", "states_explored": N}` and
  `killed_by: null`, instead of `survived`; it is excluded from `kill_rate`,
  counted in `summary.inconclusive` and `summary.by_source.*.inconclusive`
  (present only when non-zero), and fails a requested `--fail-on-survivors` /
  `--min-kill-rate` gate with the violation `inconclusive` (`gate.inconclusive`
  carries the count). `RefinementCheck`'s fields are now private: Rust callers of
  `fsl_runtime::check_refinement` read the outcome through `verdict()` and the
  names/depth/action map through accessors. Migration: a diff that used to exit 0
  with `no_semantic_change` on budget-scale specs now exits 1 with
  `unknown_budget`; lower `--depth` or narrow both specs' `verify {}` domains. A
  mutate consumer that assumed every mutant is `killed`, `survived` or `invalid`
  must handle `inconclusive`; runs without a cut-off mutant print exactly the same
  JSON as before.
- Fixed (#1283): when an enum-valued state was left unconstrained (for example
  its `init` assignment removed), `fslc verify --engine bmc` could fail with
  `error` / `semantics` `negative enum ordinal in solver model` (exit 2) instead
  of reporting the `_bounds_<state>` type-bound violation, depending on which
  ordinal the solver picked. The counterexample projection now keeps a negative
  ordinal as a raw integer witness, exactly like an ordinal past the last
  member, so the run reports `violated` / `_bounds_<state>` (exit 1). In
  `fslc mutate`, the same failure no longer surfaces as `build_spec`: the
  built-in `assignment_remove` of such an init line no longer lists
  `"build_spec"` in `--oracle-attribution` `killers`, and the equivalent
  external mutant is now `killed` by `_bounds_<state>` instead of `invalid`, so
  built-in and external verdicts agree. (Which outcomes the built-in
  init-assignment special case re-attributes to `_bounds_<state>` is described
  under #1251.)
  Migration: a spec that relied on the previous `error` exit 2 for this shape
  now gets `violated` exit 1.
- Fixed (#1295): `fslc refine`, `fslc verify`'s inline `implements`, and the
  `fslc mutate` implements oracle stopped with `result:"error"` / `kind:"type"`
  (exit 2) instead of a refinement verdict when the impl's domain differed from
  the abstraction's: an action-correspondence argument outside the abstract
  action's declared parameter domain (an impl parameter type wider than the
  abstraction's: "parameter 'v' does not belong to its declared domain for
  action 'push'"), or a state-mapping expression that reads an impl `Map` at a
  key outside its finite key domain (for example an impl key type narrower than
  the abstraction's: "map index outside finite key domain"). Both are now
  `refinement_failed` / `kind:"map_out_of_bounds"` (exit 1) at the step or
  `init` where they occur, the kind already documented for a shrunken impl
  against a full-size abstract. The documented check order is kept: a false
  abstract guard is still `abs_requires_failed`, and a stutter or action
  mapping's own checks run before the escape is reported. A `Seq` index out of
  range in a mapping, a `Map` read outside its domain in an
  action-correspondence argument, and other abstract-guard evaluation errors
  still stop with an error.
  In `fslc mutate`, a `type_bound_*` mutant that widens or narrows such a type is
  killed by `refinement` as before, and `--oracle-attribution` now lists
  `refinement` among its `killers` (the attribution pass used to drop the error
  and list none). Migration: a spec whose `refine`/`verify` exited 2 with one of
  these messages now exits 1 with a located `map_out_of_bounds` finding; align
  the impl's parameter or key type with the abstraction's, or map it explicitly.
- Fixed (#1296): a refinement whose corresponded abstract action hits a partial
  operation in its own body at the mapped state (`action go() { y = 4 / y }`
  reached at `y = 0`, or `s.pop()` on an empty `Seq`) was reported as `refines`
  (exit 0) whenever the impl step left the mapped state unchanged, and as
  `abs_state_mismatch` with the unchanged pre-state shown as
  `abs_after_expected` when it changed it: the abstract step's `partial_op` was
  recorded and then ignored. It is now
  `refinement_failed` / `kind:"abs_partial_op"` (exit 1) at that step, with
  `abs_before` / `abs_after_actual` and no `abs_after_expected` (the abstraction
  has no defined successor there). Every consumer of the refinement verdict
  reports it: `fslc refine`, `fslc check` / `fslc verify` inline `implements`
  (`implements.violation.kind`), `fslc diff` (the direction whose abstraction is
  partial), governance preservations (native and Worker: only
  `governance.preservations[].result` becomes `"refinement_failed"`; the
  top-level `result` and the exit code of `fslc check` on the governance file
  are unchanged, as for any other failed preservation), and the `fslc mutate` implements oracle, which now kills a mutant
  that reaches such a state (`killed_by:"refinement"`). A false abstract guard
  is still `abs_requires_failed`, and the stutter, guard, and out-of-domain
  checks keep their order. `abs_partial_op` is a new `kind` value; a consumer
  that matches on the documented kind list should add it. Migration: a spec or
  mapping that `refines` today may now fail with `abs_partial_op`; guard the
  abstract action (`requires y != 0`) or make its body total, or keep the impl
  from invoking it in that state; a refinement that failed with
  `abs_state_mismatch` for this reason now reports `abs_partial_op` instead.
  The other violations
  the abstract step reports -- an abstract `ensures` that is false, a partial
  operation inside an abstract `ensures` or post-state check (`Seq` operations; `/` `%` are total there), a post-state
  type-bound violation, and an abstract invariant (or `trans`) violation -- are
  not `abs_partial_op` but `abs_step_violation` (#1302). An abstract guard
  or leading `let` whose evaluation fails (for example `let q = 4 / y`) still
  stops with `result:"error"` (exit 2).
- Fixed (#1302): a refinement whose corresponded abstract step is rejected by
  the abstraction itself — the successor violates an abstract type bound
  (`action go() { y = y + 5 }` with `y: 0..4`), an abstract invariant or
  `trans`, the abstract action's `ensures` is false, or a post-state check
  (`Seq` operations; `/` `%` are total there) or `ensures` hits a partial operation (`ensures 4 / y == 0` at `y = 0`) — was
  reported as `refines` (exit 0) whenever the impl step left the mapped state
  unchanged, and as `abs_state_mismatch` with the rolled-back pre-state shown as
  `abs_after_expected` when it changed it. It is now `refinement_failed` /
  `kind:"abs_step_violation"` (exit 1) at that step, with `abs_violation`
  (`{"kind", "name"}` of the abstract violation: `type_bound` / `invariant` /
  `trans` / `ensures` / `partial_op`), `abs_after_attempted` (the successor the
  abstraction rejected), and no `abs_after_expected`. Every consumer of the
  refinement verdict reports it: `fslc refine`, `fslc check` / `fslc verify`
  inline `implements` (`implements.violation.kind`), `fslc diff` (the failing
  direction, with `abs_violation`), governance preservations (native and
  Worker: only `governance.preservations[].result` becomes
  `"refinement_failed"`; the top-level `result` and the exit code of `fslc
  check` on the governance file are unchanged, as for any other failed
  preservation), and the `fslc mutate`
  implements oracle, which now kills a mutant that reaches such a step
  (`killed_by:"refinement"`). The `refines` note for an abstraction with
  `ensures` now reads "abs ensures are checked only on the abstract steps the
  impl's transitions correspond to within checked_to_depth; verify/prove the
  abstract spec separately". `abs_step_violation`, `abs_violation` and
  `abs_after_attempted` are new; a consumer that matches on the documented kind
  list or on the note text should add or update them. Migration: a refinement
  that `refines` today may now fail with `abs_step_violation`; the abstract-side
  conditions now checked on every corresponded step are the abstract type
  bounds, invariants and `trans` at the abstract successor, and the abstract
  action's `ensures` (including partial operations in any of them; in the invariants and `trans`
  only `Seq` operations can be partial, as `/` `%` are total there). Fix the
  abstract action (or its `ensures` / invariant) or keep the impl from invoking
  it in that state; a refinement that failed with `abs_state_mismatch` for this
  reason now reports `abs_step_violation` instead. A false abstract guard is
  still `abs_requires_failed`, a body partial operation still `abs_partial_op`,
  and an abstract guard whose evaluation fails is unchanged.
- Unified (#1201): the verifier's per-action partial-operation candidate check
  (`action_has_partial_operation_candidate`) moved unchanged into `fsl-core`, next
  to the partial-operation inventory, so the verifier and the obligation catalog
  decide whether an action can reach a partial operation with one function.
  Verification output is unchanged.

## [4.9.0] - 2026-10-07

- Added (#1082): every release now carries a signed packslip manifest, so mise can install the commands and the Agent Skills together. `mise use packslip:github.com/ymm-oss/fsl/fslc` fetches the skills declared there, and `mise skills sync` links them where an agent reads them, per project and per pinned version. The manifest declares every directory under `skills/` at the release commit, so the shipped set cannot drift from the source tree. The release workflow signs it with its own identity, and asserts the platform each artifact name claims before the release is made public.
- Added (#1151): `fslc chain --jobs N` runs up to N of a project's `spec` and refine layers at once, each layer with its own Z3 context. The default stays 1, because memory multiplies with workers. The job count does not change the result: layers are aggregated in manifest order, so stdout, the stderr table, the exit code, and the first-failure attribution with or without `--keep-going` are those of `--jobs 1`; only elapsed times and the per-process solver `memory_mb` can differ. `[impl]` still runs last, and only when the serial chain would reach it. A failure that decides the report ends the run at once: a slower later layer that is already running is not waited for. Two layers whose files have the same bytes stay serial so their verify-cache interaction is unchanged, and cache entries are now written through a temporary file unique to each writer, so concurrent workers never publish a torn entry. The chain path may now also follow its options (`fslc chain --keep-going fsl-project.toml`), as the published CLI contract already allowed.
- Changed (#1089): `fslc sweep` settles depth-limited (`insufficient_depth`)
  reachability cells per `--instances`/`--values` scope instead of over the
  whole grid. A determinate success at any depth of the same scope still
  settles it, but a success in another scope no longer does: with no true failure,
  the grid is `sweep_passed`/exit 0 only when every scope has a determinate
  success at some depth, and otherwise `sweep_inconclusive`/exit 1 with a null
  `minimal_counterexample`. The new `sweep.inconclusive_scopes` array lists each
  all-inconclusive `{instances, values}` scope (`[]` when there is none) under
  every sweep result. This is a breaking CLI contract change for `--instances`/
  `--values` sweeps only; `--depth`-only sweeps are one scope and keep their
  4.7.0–4.8.1 verdicts. Migration: a grid such as `--values Amount=1..5 --depth
  3..3`, where `Amount` 1..3 never witnesses a `reachable` that 1..4 does, now
  returns `sweep_inconclusive`/exit 1 where 4.7.0–4.8.1 returned
  `sweep_passed`/exit 0. Raise `--depth` for the listed scopes or narrow the
  sweep bounds; consumers already gating on the exit code need no change beyond
  expecting exit 1 for such grids.
- Changed (#1129): `docs/` is split by reader. `DESIGN-*.md`, `RUST-PORTING.md` and `RELEASE.md` moved to `docs/design/`; `LANGUAGE*.md`, `GUIDE-*` and `INTRO-*` moved to `docs/manual/`. Filenames are unchanged (the `DESIGN-` prefix stays), so history follows through rename detection. `docs/intro/`, `docs/index.html` and `docs/README.md` did not move, so GitHub Pages URLs are unchanged. About 1,170 references were rewritten in the same commit, and `tools/check-doc-links.py check .` reports 0 findings before and after. Links to the old `github.com/.../docs/DESIGN-*.md` paths in existing issues, pull requests and commit messages are not redirected; `docs/README.md` states the mapping.
- Changed (#1149): `verify` no longer pays the cubic bounded lasso search for a
  `leadsTo` with `decreases` whose ranking obligations hold -- the ranking
  already shows every lasso probe is unsat. The deadlock-stall and `within`
  probes still run, and the verdict, `completeness:"bounded"`, and witnesses are
  unchanged; a ranking that fails, errors, times out (5 s per check), or panics
  falls back to the full search. At
  depth 8/12/16, `tests/fixtures/rust_port/ranked_leadsto.fsl` drops from
  249/741/1649 `leadsTo` checks to 45/91/153 plus 3 ranking checks. Only `cost`
  changes in the envelope: `cost.properties` gains a `leadsTo_rank` row.
- Changed (#1168): removed the unreachable `run_mutate_legacy` and its three
  mutant-count helpers from `fslc` (all behind `#[allow(dead_code)]`, called
  from no command dispatch). `fslc mutate` is unchanged.
- Changed (#1200): a test generated by `fslc testgen` (and `fslc domain testgen`)
  now fails while its adapter is not wired, on every target, with a message that
  starts `adapter not wired`. Before, pytest/Vitest/Swift/Dart/PHPUnit skipped
  such tests and Kotlin returned early, so a forgotten wiring exited 0. The new
  `--allow-unwired` flag restores the skip and produces the pre-#1200 file byte
  for byte; Kotlin rejects it (exit 2), since kotlin.test has no runtime skip.
  `fslc chain`'s `[impl]` layer must now name its evidence and accepts only the
  keys `command`, `report`, and `evidence`; any other key (a typo such as
  `reprot`) is a `kind:"parse"` error at exit 2 naming it. `report` names the
  JUnit XML file (or directory of files) the command writes: exit 0 is `passed`
  only when at least one test executed and none failed, and zero executed tests
  (none collected, or all skipped) or a missing, unchanged, or unreadable report
  is the new layer and top-level result `indeterminate` (exit 1). A report counts
  only if the run created or changed it (length, full-precision modification
  time, and on Unix inode and change time are compared before and after).
  `evidence = "exit_code"` keeps the old exit-status decision for a command that
  is not a test suite, marked `detail.evidence: "exit_code_only"`, shown in the
  table, and reported in a top-level `warnings[]` entry `impl_exit_code_only`. An
  `[impl]` with neither key is `indeterminate` when its command exits 0, and the
  stderr table's Detail column says why (`evidence=missing`, `no test executed
  (E/T)`, `report not read ...`). A malformed `[impl]` table is rejected with the
  manifest, before any layer runs. The frozen Python reference
  (`python -m fslc chain`) is unchanged and still judges `[impl]` by the exit
  code alone, ignoring `report` and `evidence`. This is
  a breaking change of the generated test files and of every existing chain
  manifest with an `[impl]` table. Migration: wire `makeAdapter()` (pytest: the
  `Adapter` class) before running a regenerated suite, or regenerate with
  `--allow-unwired` to keep the skipping scaffold (not available for Kotlin). In
  every `[impl]` table add either `report = "r.xml"` with a command that writes
  JUnit XML (`pytest --junitxml=r.xml`, `vitest run --reporter=junit
  --outputFile=r.xml`, `phpunit --log-junit r.xml`, Gradle with `--rerun-tasks`
  so an `UP-TO-DATE` task does not leave the report unchanged, and for SwiftPM 6
  the `r-swift-testing.xml` file Swift Testing writes beside `--xunit-output
  r.xml` when XCTest also runs), or `evidence = "exit_code"` for a smoke command
  that is not a test suite. A pipe into a JUnit converter reports the
  converter's exit status, so a failing run can exit 0; the report check still
  sees the failing test case.
- Changed (#1212, breaking): a `forbidden` last step that names no action, or no
  variant of that arity, is now a `kind: "forbidden"` error with a `message`
  (`unknown action '<name>' in forbidden` / `arity mismatch for action '<name>'
  in forbidden`), located at that step, instead of a vacuously satisfied
  rejection (frozen-Python parity). Migration, per command: `fslc check` used
  to pass and now exits 2. `verify` (every engine and depth), `sweep`, the
  `[requirements]` layer of `chain`, the `mutate` baseline, `html` and `ledger`
  used to report whatever their engine reached on the rest of the spec (exit 0
  when it verified, exit 1 when it did not) and now exit 2 with no verdict,
  counterexample or mutants: `verify`, `sweep` and `mutate` print this error,
  `chain` carries it as the `[requirements]` layer's `detail`, and `html` and
  `ledger` print the generated result (`result: "generated"`) and still write
  their report, with the verification `not_run` and this error inside. `scenarios`
  and `testgen` now exit 2 with no scenario and no test file; for an arity
  mismatch they used to emit the forbidden scenario as `rejected_by:
  "requires_failed"` and a test asserting it, and for an unknown action they
  stopped with exit 2 (`kind: "semantics"`, `unknown forbidden action
  '<name>'`), in both cases only when the rest of the spec verified (otherwise
  `violated`, exit 1). `counterexample export` returns this error and writes no
  file, where it exported the rest of the spec's counterexample, or exited 2
  with nothing to export. `explain` still exits 0, with empty `witnesses`, and
  the witnesses section of `html` is empty too (a known gap: #1242).
  `approval create` with `--kind scenarios` or `html` exits 2 without a record
  (for `html`, stdout is the generated `html` result),
  and `approval check` of such a record created before exits 2. `approval
  create --kind ledger` now exits 0 and creates a record of a ledger that lists
  the error: exit 0 → 0 on a spec whose rest verifies, and exit 2 → 0 on a spec
  whose rest has a violation, where it now creates a record. The earlier exit 2
  was an accident: that spec's ledger embeds wall-clock `elapsed_s`, so it never
  matched a fresh rendering. Whether a ledger whose forbidden gate fails may be
  approved is tracked by #1243. `approval check` of a ledger record created
  before reports `drifted` (exit 0). Fix the step's action name or arguments. `fslc diff`
  changes two forbidden verdicts: a last step that OLD and NEW both reject as
  `bad_call` outside a range or enum parameter type is now preserved instead
  of `unknown` / `forbidden_step_unrelatable`, so that forbidden no longer
  fails `--forbid unknown`; an OLD last step that names no action, or no variant
  of that arity, is now `unknown` / `forbidden_replay_failed` instead of
  preserved by a NEW guard (in the measured cases `--forbid unknown` already
  failed, because the action NEW adds, or the arity it changes, is itself an
  `unknown` finding). A `bad_call` decided by an `entity` / `number` verify
  scope stays `unknown`. `diff --git` and `approval diff` report the same
  findings. Each command's measured exit status and output before and after,
  and the corpus sweep (no `.fsl` under `specs/`, `examples/`,
  `rust/fslc/tests/fixtures/` or `tests/fixtures/` changes its `check`,
  `scenarios` or self-`diff` verdict), are in `docs/design/DESIGN-forbidden.md`
  §2.1 and §6.
- Changed (#1213, breaking): a `forbidden` case is satisfied only when its last
  step is rejected, that is, not enabled (`requires_failed` or `bad_call`). A
  last step that is enabled and then stops with a runtime violation
  (`invariant` / `trans` / `ensures` / `type_bound` / `partial_op`) used to count
  as satisfied with `rejected_by` set to the violation kind, so a forbidden passed
  while the guard it states was missing. It is now a `kind: "forbidden"` error
  (exit 2) with `failed_step`, `step_results: []`, `violation` (`{kind, name,
  action, params}`), `accepted_trace` (the setup steps), the `state` before the
  last step, a `loc` at that step, and a hint. `fslc diff` follows the same
  rule: a NEW final step that is enabled and then violates is
  `forbidden_relaxed` (its witness carries `violation`, and its `trace` and
  `state` stop before the final step); an OLD final step that violates, or a NEW
  setup step that violates, is `unknown` / `forbidden_replay_failed`. `fslc
  ledger` summarizes the error as a violation reached by an enabled step, not as
  an accepted one.

  Migration, per command (exit status and output measured on small specs;
  `docs/design/DESIGN-forbidden.md` §2.1): the forbidden replay is a gate that
  runs before BMC, induction, and scenario generation. `check` used to exit 0
  and now exits 2. `verify` used to report whatever its engine reached —
  `violated` / exit 1 when `--depth` reached the violation, `verified` / exit 0
  when it needed more steps, and its own verdict under `--engine induction` —
  and now exits 2 at any depth and engine, with no BMC or induction trace; so do
  `sweep`, `chain`, `mutate`, `html` and `ledger` (`html` and `ledger` still
  write their report, with the verification `not_run`: `html` with the
  forbidden hint, `ledger` with the violation under `FB-1`). `scenarios` and
  `testgen` exit 2 with no scenario and no test file; `scenarios` used to exit
  1 when its depth reached the violation and otherwise exit 0 with the
  forbidden scenario as `rejected_by: "invariant"`, and `testgen` exited 1
  (`violated`) on every measured spec, even at `--depth 1`, because its own
  check reached the violation. `counterexample export` exits 2 with no file,
  where it wrote a reproducer with exit 1 when its depth reached the violation
  and otherwise exited 2 with nothing to export. `approval create` with `--kind
  scenarios` or `html` exits 2 without a record (it used to create one when its
  depth did not reach the violation), and `approval check` of such a record
  exits 2. Three commands do not exit 2. `explain` still exits 0, but where
  `scenarios` used to succeed its `witnesses`, and the witnesses section of
  `html`, are now empty (a known gap: #1242). `approval create --kind ledger`
  exits 0 with a record of a ledger that lists the error: exit 2 → 0 when its
  depth reaches the violation, where it now creates a record (the earlier exit
  2 was an accident: that ledger embeds wall-clock `elapsed_s`, so it never
  matched a fresh rendering), and exit 0 → 0 otherwise. Whether a ledger whose
  forbidden gate fails may be approved is tracked by #1243. `approval check` of
  a ledger record created before reports `drifted` (exit 0). In `mutate`, a
  mutant the bounded oracle leaves clean but whose forbidden last step now
  violates is killed with `killed_by: "forbidden"` instead of surviving. In `fslc diff`, a NEW final violation now
  fails `--forbid forbidden_relaxed`, and an OLD final or NEW setup violation
  fails `--forbid unknown`, where that forbidden used to be preserved and failed
  neither. Fix an affected spec by adding the `requires` that rejects the call
  (the guard the forbidden states) and keeping the invariant as its own safety
  property. There is no opt-out: a violation is evidence the guard is missing.
  The strict error reports that violation, but the other properties' verdicts,
  the shortest counterexample, and the reproducer come back only once the
  forbidden or the guard is fixed. No `.fsl` under `specs/`, `examples/`,
  `rust/fslc/tests/fixtures/` or `tests/fixtures/` changes its `check`,
  `scenarios` or self-`diff` verdict (`docs/design/DESIGN-forbidden.md` §6).
- Changed (#1229, breaking): a `forbidden` last step whose argument lies outside
  the `verify { instances / values }` scope of an `entity` / `number` parameter
  (a requirements or business `process` counts as an `entity`) no longer
  satisfies the forbidden. It evaluated no guard, and an implementation, which
  has no such bound, may accept the call (for example `respond(7)` under
  `instances Case = 3`, or `add(9)` under `values Qty = 0..3` when the guard
  admits 9). It used to count as satisfied, with `rejected_by:
  "requires_failed"`, so the generated test asserted a rejection no guard made. It is now a `kind: "forbidden"` error
  (exit 2) with `failed_step`, `step`, `step_results: []`, `message`,
  `out_of_scope_argument` (`{parameter, value, type, scope}`), a `loc` at that
  step, and a hint. An argument outside a declared range or enum type is a
  satisfied `bad_call` (#1212), and a guard refusal inside the scope stays
  `requires_failed`. Under `--instances` / `--values`, a forbidden whose
  final-step argument only the override removed stays `forbidden_skipped` /
  `not_evaluated` (#1218); one outside the declared scope is this error unless
  the override widens the scope to include it, in which case the guard is
  evaluated. The error's `out_of_scope_argument.scope` and the `lo..hi` in its
  hint are the scope of the run, so under a narrowing override they show the
  overridden range (`scope: [0, 1]` for `respond(7)` with `--instances Case=2`),
  not the declared range that makes the step an error, while the hint's "widen
  the scope in verify { instances / values }" refers to the declared block.
  `fslc ledger` summarizes the error as a step outside the verify
  scope.

  Migration, per command (exit status and output measured on small specs;
  `docs/design/DESIGN-forbidden.md` §2.1): the forbidden replay is a gate that
  runs before BMC, induction, and scenario generation, and such a step used to
  pass it. `check` used to exit 0 and now exits 2. `verify` (every engine and
  depth), `sweep`, the `[requirements]` layer of `chain`, the `mutate` baseline,
  `html` and `ledger` used to report whatever their engine reached on the rest
  of the spec (exit 0 when it verified, exit 1 when it did not) and now exit 2
  with no verdict, counterexample or mutants. `verify`, `sweep`, and `mutate`
  print the error (`error` / `kind: "forbidden"`); `chain` prints `kind:
  "chain"` with `failed: ["requirements"]` and the error as that layer's
  `detail`; `html` and `ledger` print the generated result (`result:
  "generated"`) and still write their report, with the error inside and the
  verification `not_run`.
  `scenarios` and `testgen` now exit 2 with no scenario and no test file; when
  the rest of the spec verified they used to exit 0, emitting the forbidden
  scenario as `rejected_by: "requires_failed"` and a test asserting it (otherwise
  `violated`, exit 1). `counterexample export` returns this error and writes no
  file, where it exported the rest of the spec's counterexample, or exited 2
  with nothing to export. `explain` still exits 0, with empty `witnesses` (where the rest of the spec
  verified, it listed the forbidden's witness), and the witnesses section of
  `html` is empty too (known gap #1242).
  `approval create` with `--kind scenarios` or `html` exits 2 without a record (it used to create one
  when the rest of the spec verified); with `--kind scenarios` it prints the
  error, with `--kind html` the `html` result, and `approval check` of such a record
  created before exits 2. `--kind ledger` exits 0 with a record of a ledger
  that lists the error (where the rest of the spec verified, it created one
  before too; where it was violated, it exited 2 only because that ledger embeds
  wall-clock `elapsed_s`; whether such a ledger may be approved is tracked by
  #1243), and a ledger record created before reports `drifted` (exit 0).
  In `mutate`, a mutant that narrows an `entity` / `number` scope so that a
  forbidden final step falls outside it (`type_bound_hi_minus1` /
  `type_bound_lo_plus1` on that type) is killed with `killed_by: "forbidden"`;
  on the measured specs it survived before. `fslc diff` classifies each side with its own `entity` /
  `number` types: an OLD final step outside the compared verify scope is
  `unknown` / `forbidden_replay_failed` instead of `unknown` /
  `forbidden_step_unrelatable` (no `--forbid` exit changes, as it was already
  `unknown`), and an OLD `bad_call` is preserved only when NEW also rejects it
  as `bad_call` outside a declared type. A compose NEW, or a NEW of another
  dialect whose entity types are not in its source, never preserves an OLD
  `bad_call`, even when it rejects the step as `bad_call`, since its component
  may bound that type by a verify scope: such a forbidden is `unknown` /
  `forbidden_step_unrelatable` and fails `--forbid unknown`. `diff --git` and `approval diff` report the same findings.
  Fix an affected spec by widening the scope so it includes the value, or by
  changing the step to an in-scope value the guard rejects (the error's
  `out_of_scope_argument` and hint name the parameter, value, and scope). If the
  call must be rejected as a nonexistent value, state that bound as a range type
  instead, which keeps it a `bad_call`. There is no opt-out. No `.fsl` under
  `specs/`, `examples/`, `rust/fslc/tests/fixtures/` or `tests/fixtures/`
  changes its `check`, `scenarios` or self-`diff` verdict
  (`docs/design/DESIGN-forbidden.md` §6).
- Fixed (#1008): an option that made `verify` skip a declared check removed it
  from the envelope with no reason, so a broken inline `implements` seam passed
  `verify --property X` with exit 0 looking exactly like a spec without one.
  `--property`, `--exclude-property`, and `--from-state` now keep the key as
  `implements: {"abs", "result": "not_evaluated", "reason", "reasons"}`
  (`reason` is `property_selection` / `property_exclusion` / `from_state`), and
  `--instances` / `--values`, under which a requirements `acceptance` /
  `forbidden` scenario can fall outside the overridden scope, report the skipped
  ones as `requirement_traces: {"result": "not_evaluated", "reason":
  "bounds_override", "skipped": [{kind, id, reference}], ...}` (#1218). The top-level `result` and the exit code
  are unchanged (a selected run still speaks only for what it checked), a spec
  that declares no such check keeps the key absent, and an unfiltered `verify`
  envelope is unchanged. A selected run now also reports the same compose
  warnings (`fair_not_inherited`) and `no_user_invariants` suppression as a full
  run; `--property` used to drop them. The Worker has no selection options and
  always runs the full set. `sweep`, whose cells always carry scope overrides,
  copies each skipped section into the cell's `summary` row and reports the
  union over cells as `sweep.not_evaluated: {sections, reasons}`; the grid
  verdict and exit code are unchanged. The frozen Python reference's per-scenario
  `acceptance_skipped` downgrade under overrides is ported (#1218).
- Fixed (#1056): the WASM browser-parity harness no longer reads `from "…"`
  inside a `//` comment as an import. It now collects a spec's imported files
  with a scanner that follows the FSL lexer's comment and string rules, and an
  import string that does not close on its own line fails with the file and
  line instead of producing a multi-line path.
- Fixed (#1082): `install.sh` no longer takes a skill link it did not create. It used to move anything at `~/.claude/skills/<name>` aside to `.pre-native-v3` and link its own payload in its place. `mise skills sync --global` writes that same directory, so a link mise placed was moved aside and left pointing at nothing mise knew about. Such a link is now reported and left alone. A link whose target no longer exists is still replaced, and a real directory is still backed up as before. A link an earlier run of the installer made is still migrated, whether it points into a pre-native clone such as `~/.fsl` or into the `current` payload of a data directory that `FSL_DATA_DIR` or `XDG_DATA_HOME` no longer names.
- Fixed (#1151): each `fslc chain` layer now runs in a fresh Z3 context, so it reports what `fslc verify` (and the verify cache) reports for the same file and depth. Before, every layer shared the context of the layers before it, which could change the witness and solver counts. Measured on `examples/agentic_rag` at depth 8: the design layer's `reachable_failed` witness differed from the standalone `verify`. The verdict was the same.
- Fixed (#1163): the browser Worker's `check` no longer answers `ok` for specs
  native `fslc check` rejects. It now runs the same validity stages in the same
  order as the CLI -- `dbsystem`/`ai_component` validation, the
  source-diagnostic preflight, Agent analysis, and init write ownership, which
  were missing -- through one shared `fslc_rust::check_stages` function, so the
  two stage lists cannot drift apart again. The Worker's `verify` now runs
  native `verify`'s pre-solver stages in native order: it rejects an Agent
  document, validates `dbsystem`/`ai_component` documents, reports an
  `implements` failure before solving, and rejects an init that writes one
  variable twice. `examples/ai/recursive_support_agent.fsl` is now compared
  native-to-Worker in the browser parity harness instead of being excluded.
  Native CLI output is unchanged.
- Fixed (#1164): `verify` (every engine: bmc, induction, explicit, auto) and
  `check` no longer abort with a stack overflow (exit 134, no JSON envelope) on
  deeply nested expressions. Six recursions over the expression tree ran
  without the #620 stack guard and now enter `recursion::guard` at their cycle
  entry: the concrete evaluator `fsl_runtime::eval`, the verifier's definedness
  walk `evaluation_status_with_policy`, induction's `helpful`-argument fold
  `eval_state_independent`, and `fsl-core`'s `extend_pattern_binding`,
  `reserved::check_expr`, and `expr_text_with_origins`. On a release build a
  left-nested `x + ... + x` invariant previously aborted every engine at 4000
  terms and `check` at 15000; it now verifies at 20000. The `deep_nesting`
  regression gains witnesses that force full evaluation (`+`, `and`, and `not`
  chains) under all four engines and `check`, each shown to fail with its guard
  removed. A still deeper tree aborts in the parse tree's derived `Clone`
  (#1186).
- Fixed (#1165): when a spec's `verify { }` block bounds several undeclared
  `entity`/`number` names, `check` now reports the first offending bound in
  source order, with its location, instead of a name that varied from run to
  run. CLI `--instances`/`--values` overrides were already deterministic and
  are unchanged.
  Source order now also applies across the two kinds: an undeclared `values`
  bound written before an undeclared `instances` bound is reported first. The
  frozen Python reference (`src/fslc/dialects.py`) still reports every
  `instances` bound before any `values` bound, so the two differ when both
  kinds are undeclared.
- Fixed (#1166): `fslc explain` now lists a `partial_op` auto-check for every
  partial operation `verify` checks. It previously missed `Seq` index reads
  (`s[0]`), operations inside quantifier and aggregate binders, and operations in
  an indexed assignment target, although `verify` reported `_partial_<action>`
  violations for all of them. The six partial operations are now defined once in
  `fsl-core` and shared by `explain`, the verifier, the runtime and the Public
  Kernel; the Public Kernel's `partial_operations` now also includes an indexed
  assignment target's index expression (`m[s.head()] = …`). The runtime classifies
  a partial-operation failure by a typed field rather than by its error message;
  a model-build `division by zero`/`remainder by zero` (constant folding, rejected
  by `check`) is deliberately not tagged as one.
  Verification verdicts are unchanged.
- Fixed (#1196): `verify --engine induction` reported `proved` for a spec whose
  guard reached a partial operation in a reachable state (e.g. `requires x > 0
  and x / d < 100` after another action sets `d = 0`), because the step case
  evaluated guards with a totalizing evaluator; BMC and the explicit engine fail
  the same spec once the state is within `--depth`. Induction now adds a
  definedness obligation over every state that satisfies the proved invariants:
  action guards, enabled bodies, invariants, `leadsTo` expressions, `trans`
  properties, and reached `ensures` must not reach a partial operation, with the
  same short-circuit semantics as BMC (`d != 0 and x / d < 100` stays
  `proved`). A failure is `unknown_cti` with `violation_kind: "partial_op"` and
  `invariant: "_partial_<action>"` / `"_partial_property_<name>"`; when the state
  is unreachable, an auxiliary invariant that excludes it (e.g. `d != 0`)
  restores `proved`. The k-induction chain premises are now asserted in their
  own solver scope, which is popped before the solver is reused for this
  obligation and for the ranked `leadsTo` proof, so an unsatisfiable chain no
  longer makes those queries vacuous. `--lemma` adjudication still
  proves only the lemma's truth; definedness is checked in the target run.
  `--property <invariant>` (including `sweep --property`) narrows the premise to
  the selected invariant, so a division only a dropped invariant protects is now
  reported there as `_partial_<action>`. The frozen Python reference still
  reports `proved`.
- Fixed (#1212): `forbidden`: a last step whose argument lies outside its
  parameter's declared range or enum type is now reported as `rejected_by: "bad_call"`
  (`docs/design/DESIGN-bridge.md` 1.2), and the generated negative test asserts
  `bad_call`; only a call inside that domain that a `requires` guard refuses
  stays `requires_failed`. On the spec measured in
  `docs/design/DESIGN-forbidden.md` §2.1, no command that runs the forbidden gate
  changes its exit status, but an `approval check` of a `scenarios` record
  created before reports `drifted` (exit 0) for such a forbidden; `fslc diff`
  changes its verdict (below).
  An argument outside the `verify { instances / values }`
  scope of an `entity` / `number` parameter evaluates no guard, and an
  implementation may accept the call, so it is not a satisfied `bad_call` but an
  error (#1229, listed in the #1229 `changed` entry). `fslc ledger` no
  longer summarizes every forbidden error as an accepted final step with
  "ガードを追加": a final step naming no action and a broken setup
  (`forbidden_setup`) get their own summary, translation, and next action. The
  `fslc diff` verdict changes of #1212 are breaking and listed in the #1212
  `changed` entry.
- Fixed (#1217): `verify --engine induction` reported `proved`/`unbounded` for a
  spec whose action `ensures` is false on a reachable step beyond `--depth`
  (e.g. `ensures x != 6` on a counter from 0 to 10, proved at `--depth 2`, while
  BMC at depth 8 reports `violated ensures`): the step case never asked
  `ensures`. Induction now checks every `ensures` as a one-step obligation over
  states that satisfy the proved invariants (and `trans`), with BMC's notion of
  a reached `ensures` (selected, enabled, body defined, earlier `ensures` defined
  and true). A failure is `unknown_cti` with `violation_kind: "ensures"`,
  `invariant: "<action>"`, and `last_action`; an `ensures` that follows from an
  invariant stays `proved`, and an unreachable CTI start is excluded with an
  auxiliary invariant. The frozen Python reference still reports `proved`.
- Fixed (#1218): `fslc verify --instances` / `--values` (and every `fslc sweep`
  cell) now replays the requirements `acceptance`/`forbidden` scenarios instead of
  skipping them all. Previously the native CLI did not replay any scenario under bound
  overrides, so a false acceptance yielded `verified` / exit 0 — even when the
  override equalled the declared range — and `sweep` always passed it. The frozen
  Python reference's per-scenario skip (#89), as `docs/manual/LANGUAGE.md`
  specifies it, is now native: only a scenario that references a value outside
  the overridden scope — one the override removed: inside the declared bounds,
  outside the overridden ones (an action argument, an `expect` map index whose
  `expect` evaluates in the declared world, or a forbidden final step whose
  "rejection" tested no guard) — is skipped, with an
  `acceptance_skipped` / `forbidden_skipped` warning whose `reference` names the
  out-of-range reference; every other failure is the same exit-2 error
  (`trace_type: "acceptance"` / `"forbidden"`) an unscoped run reports.
  `requirement_traces` (#1008) is now built from those skips: absent when every
  scenario was replayed, otherwise `not_evaluated` / `bounds_override` with
  `skipped: [{kind, id, reference}]` instead of the scenario counts, and `sweep`
  lists the union as `sweep.not_evaluated.skipped`. Native is deliberately
  stricter than the frozen Python reference, which also skips references outside
  the declared bounds, excuses any `expect` error that mentions an out-of-range
  index, and counts an out-of-range forbidden final step as a rejection.
  Migration: a scoped `verify` or a `sweep` that exited 0 only because its
  scenarios were never replayed now exits 2 with the failing scenario — fix the
  scenario or the spec (an unscoped `verify` already reported the same error).
- Fixed (#1223): the `Result<Option<..>, _>` signature census (#868) now tokenizes Rust char and byte literals (`'"'`, `'\''`, `b'"'`), lifetimes and labels, raw, byte and C strings, and comments as whole units. Before, a `'"'` char literal flipped its string tracking and hid every signature up to the next `"`, so `run_chain_steps` in `fslc` went unclassified while the census passed. It is now classified as ordinary: it returns one report slot per chain step, not an optional verdict.
- Fixed (#1231): the `fsl-design-review` skill no longer claims that `fslc refine` proves the Liskov substitution principle. refine checks trace inclusion (mapped effects equal the abstract actions' effects, abstract invariants hold through the map) but lets the detail layer strengthen a guard, as `specs/bank_impl.fsl` does, so it cannot show that preconditions were not strengthened. LSP and OCP are now rated "Partial", and a refine success is reported as the safety half only. The skill names what can see strengthening today: `never_enabled_action` under `verify --vacuity error` for a fully dead action, `fslc diff --forbid behavior_removed` for a before/after pair with shared names, and otherwise guard review until enabledness preservation (#1232) exists. The `abs_requires_failed` row now says the detail layer is more permissive than the contract, not that it strengthened a precondition. `examples/validation/README.md` no longer presents the windowed variant as LSP-substitutable.
- Fixed (#1240): `fslc verify` (BMC, and the base case of `--engine induction`)
  reported `verified` / `proved` with exit 0 for a spec whose reachable action
  guard or body overflowed checked i64 arithmetic or read or wrote a finite `Map`
  outside its key domain, whenever that action contained none of LANGUAGE.md
  §6's partial operations and `init` was nondeterministic (a deterministic `init`
  already failed through the concrete pre-pass). BMC now asks every action
  instance's guard and enabled-body definedness at every step below `--depth`,
  as it already did for invariants, `ensures`, and the other properties, and
  reports the exit-2 `semantics` error an action with a partial operation already
  produced (`action '<name>' guard evaluation has a non-partial failure`, or
  `body`). Because actions are checked in declaration order, such an error in an
  earlier action now precedes a later action's `partial_op` in the same step, as
  in the explicit engine. Specs without such a failure keep their verdicts: none
  of the 552 corpus specs changed verdict, exit code, or message under `bmc`,
  `induction`, or `explicit`. The check runs as a second pass on a reset solver
  after the search has produced its evidence, so witness values (deadlock,
  reachable, and CTI traces) are the ones the search produced before, natively
  and in the browser Worker alike. `cost.properties` gains a
  `{"kind":"partial_op","name":"actions"}` row that answers the all-defined case
  once per step, and the per-action `partial_op` rows also count that pass's
  typed probes (`docs/design/DESIGN-verification-cost.md`). The row also
  counts the range-lemma queries that keep this check fast: each `Int` state
  leaf's bound at a step is kept only when the solver proves it entailed, and is
  conjoined only to the action definedness queries, whose answers it cannot
  change. `fslc mutate` uses
  the same check, so a mutant that removes an `init` assignment of an unbounded
  `Int` can now be reported `killed_by: "build_spec"` (the definedness error)
  instead of by an invariant. The induction step obligation (#1196) still
  excludes non-partial undefinedness (`docs/design/DESIGN-induction.md` §2.6), so
  an overflow or out-of-domain `Map` access first reachable beyond `--depth` is
  still not reported by `--engine induction`; the frozen Python reference still
  reports `verified`. Migration: a spec that now exits 2 reaches an overflow or
  an out-of-domain `Map` access in the named action; guard the access (e.g.
  `requires i <= 3 and m[i] == 0`) or bound the state that feeds it.
- Fixed (#1245): `fslc refine` (single and chain), the `fslc chain` refine layer,
  and governance preservations (native and browser Worker) no longer report a
  refinement correspondence check cut off by its fixed state budget (50,000
  states, #1041) as `refines`. They now report `unknown_budget`: `fslc refine`
  exits 1 with `states_explored` and a `hint`, a chain stops at that link, a
  `fslc chain` refine layer fails, and a preservation's `result` is
  `unknown_budget` — the vocabulary and exit the inline `implements` seam
  already used. The Worker governance path also reports a self-violating
  `after` spec as `violated` instead of `refines`, matching native. Migration:
  a refinement whose correspondence check visits more than 50,000 states
  previously reported `refines` / exit 0 and now reports `unknown_budget` /
  exit 1; lower `--depth` or narrow both layers' `verify {}` domains.
  `fslc diff` and `fslc mutate` do not read this cutoff yet (#1262): on a cut-off
  check `fslc diff` still reports `no_semantic_change` (exit 0) and
  `fslc mutate` still counts the mutant as survived.
- Documented (#1082): the README now shows how to install `fslc` and `fslc-lsp` with mise, including the `[tool_alias]` pair one repository needs and the anchored `matching_regex` that keeps `fslc` from selecting the `fslc-lsp` asset.

## [4.8.1] - 2026-10-01

- Fixed (#1189): `verify --engine induction` no longer reports a ranked
  `leadsTo ... decreases M` as `proved` / `unbounded` when the obligation can be
  pending in a state where nothing that must fire is enabled. The ranking proof
  asserted the transition relation on its pre-state, which forced some action
  to be enabled there, so the `no_deadlock` obligation of
  `docs/design/DESIGN-induction.md` §2.3 never saw such a state: without
  `helpful` it was not checked at all, and with `helpful` a pending state in
  which no action at all was enabled was skipped. The obligation is now asked
  over an unconstrained invariant state and fails as `unknown_cti` /
  `violation_kind: "leadsTo_rank"` with `rank_failure: "deadlock"` (or
  `helpful_action_not_enabled`), as in the frozen Python reference. It is a
  proof obligation over invariant states, so `--deadlock` does not disable it,
  and an unreachable deadlocked pending state blocks the proof until an
  invariant excludes it.
  Visible change: at a depth large enough for the base-case BMC to find the
  stall, such a spec now reports `unknown_cti` / `rank_failure: "deadlock"`
  instead of `violated` / `leadsTo` (the existing rule prefers the rank failure
  when the base case shows a leadsTo violation, as Python does); the exit code
  stays 1.
- Fixed (#1192): `check` accepted a spec that declared two properties with the
  same name, and every result keyed by that name collapsed them — with two
  `leadsTo L`, `verify --engine induction` reported `proved`/`unbounded`
  although the second `L` is false. Property names (`invariant`, `trans`,
  `unless`, `reachable`, `leadsTo`, `until`, and the `<name>_until_safety` trans
  an `until` lowers to) now share one namespace; a reuse, within a kind or
  across kinds, is a located `kind: "semantics"` error at the later declaration
  that names the earlier one's location, so `check` and `verify` exit 2. No spec
  under `specs/`, `examples/`, or the test fixtures reused a property name. The
  frozen Python reference still accepts such specs.
  Still accepted: a user invariant named `_bounds_<var>` shares its name with
  the generated type-bound invariant for `<var>`. Both are still checked
  separately (a false one is still reported `violated`), but the name appears
  twice in `invariants_checked`.

## [4.8.0] - 2026-09-29

- `fsl check` fence marking and `tools/check-doc-fences.py` gate the FSL in complete-specification fences under `docs/`, and report how many `fsl` fences remain unchecked (#1137).
- Changed (#1116), **breaking for runtime logs that were passing**: `fslc domain replay` accepts a narrower set of JSON values than 4.7.0 did. A parameter of a declared numeric type takes a JSON integer only — `"2"`, `2.0`, `true`, and `null` replayed as 2, 2, 1, and 0 and reported `conformance_checked`/exit 0, and are now `nonconformant`/exit 1 with `command_rejected_by_model`. A `Bool` parameter takes `true`/`false` only; the integer `1` is not read as `true` on this path, unlike `fslc replay`'s hand-written mapped-action inputs. Note the cliff this puts in front of specification work: while an identity type is only referenced and never declared, an opaque token such as `"p1"` maps onto its `external` placeholder and the log replays, but declaring it (`type PaymentRequestId = 0..999;`) makes every non-numeric token for it nonconformant. See `docs/DESIGN-domain.md`, "Runtime Replay".
  Migration: replay your existing logs against 4.8.0 before upgrading. Where a row now reports
  `nonconformant`/`command_rejected_by_model`, fix the log emitter to write each parameter at its
  declared type — a JSON integer for a numeric type, `true`/`false` for `Bool`, an enum member for
  an enum — rather than a string, a float, or `null`. Where the rejected parameter is an opaque
  token for a type you have since declared numeric, either keep that type implicit or change the
  runtime to emit the numeric identity.
- Fixed (#828): VS Code installation instructions now use the published VSIX asset name and document checksum availability.
- Fixed (#979): The e2e README command test now matches the documented invocations without `--deadlock ignore`.
- Fixed (#991): Document that adjacent refinement links imply composed refinement, but direct endpoint success does not establish intermediate-layer contract compliance.
- Fixed (#1037): `ledger::evidence_verdict`'s doc comment now cites the design section that documents it ("Verdict mapping — `ledger::evidence_verdict` (issue #508)") as a quoted heading, so the design-citation check verifies it; the old `classify_source` name no longer exists anywhere.
- Fixed (#1040): FIFO cleanup joins a finished writer before reading its final outcome, avoiding a race between outcome delivery and the writer's finished state.
- Fixed (#1069): issue_250 test fixture paths now remain unique when tests construct fixtures concurrently.
- Fixed: the daily ruleset drift audit no longer reports the intended `strict_required_status_checks_policy: false` on `main safety and CI` as drift. The contract and fixture now record `false`, and the checker compares against the contract's value instead of a hardcoded `true`.
- Fixed (#1090): the skill reference and the intro syntax pages now recommend the depth-only `fslc sweep file.fsl --depth 1..8 [--property Name]`, which works on every spec, and present `--instances <Entity>=…` / `--values <Number>=…` as an extension for specs that declare them. The old `--instances Case=1..3` form exited 2 on any spec without entity/number declarations.
- Fixed (#1109): the business dialect now rejects `with`, `when` and `set` on a
  transition instead of silently dropping them. `lower_business` never read a
  transition's inputs, guard or assignments, so `check` answered `ok` for a
  guard that was never applied and an assignment to a field that was never
  declared, and `verify` then answered for that other model. The error is
  positioned at the transition and names the `requirements` dialect, whose
  `process` gives the three clauses meaning; a control confirms the same
  process without them, and every existing business example, still passes. `covers` is unaffected: `lower_business` does consume it into a requirement
  annotation, so it keeps working.
- Fixed (#1116): `fslc domain replay` now reads a runtime log's `params` at the target action's declared types instead of forcing every value to `Int` and reading an unparseable one as `0`. `{"value":"garbage"}` on an integer input no longer reports `conformance_checked`/exit 0 (false conformance), and enum and `Bool` inputs — which the forced `Int` made permanently `command_rejected_by_model` — now replay, an enum member from its domain, qualified, or Kernel spelling. An *implicit identity type* (`PaymentRequestId`, only referenced and never declared) keeps its documented `external` placeholder mapping for an opaque token such as `"p1"`, but a JSON object, array, or `null` there is rejected rather than folded onto the placeholder, so the exception does not re-open the same false conformance for the parameter shape most logs carry. A correlation id filled in from `effect_completion.correlation_id` is converted from the JSON the log wrote rather than from a re-parse of the stringified pairing key, so `"1"` and `1` get the same verdict there as in `params`. `examples/domain/order_async_effect_replay.jsonl` and the `issue_518` replay corpus are byte-identical to 4.7.0.
- Fixed (#1117): `fslc domain replay` now checks each `domain_event` log row
  against the event the model actually raised at that point — the one-hot
  `event_<Event>` flag left by the last accepted `command`/`effect_completion`
  transition — instead of only checking that the name is declared somewhere in
  the domain. An event the preceding command's `decide` does not emit, a
  `domain_event` with no preceding transition, and a stale event re-logged
  after a later accepted transition are all reported as
  `unknown_domain_event`/`runtime_event_emitted_by_preceding_transition`
  (`nonconformant`, exit 1) rather than returning `conformance_checked`.
  Conformant logs, including outcome events raised by an `effect_completion`,
  are unchanged. A mismatch names its cause rather than guessing: an event only
  a saga step, step timeout, or compensation emits gets
  `failed_rule:"runtime_event_reachable_by_replayed_transition"` and repair
  text saying no log can match it and the FSL model must not be changed; a
  mismatch after a transition the model **rejected** names that rejection; and
  only a genuine ordering or model gap proposes adding the event to the
  transition the log ran. The finding `kind` enum in
  `schemas/fslc/domain/finding.v0.schema.json` is unchanged. What the check
  does **not** cover, and what therefore keeps #1117 open: **payloads are not
  compared** — `command SetN {v:1}` followed by `domain_event NSet {v:0}` still
  conforms, because only the event name is matched; the **`aggregate` field of
  a `domain_event` row is not read**, so a row naming the wrong aggregate
  conforms as long as the event name matches (the flag is keyed by event name
  across the whole domain, and a domain where two aggregates declare the same
  event name is already rejected at lowering with `duplicate state variable`);
  and a **rejected** `command`/`effect_completion` rewrites nothing, so a stale
  event row after one still conforms — the rejection itself is the reported
  finding. `events_observed` changes meaning accordingly: a `domain_event` row
  enters it only once its occurrence flag matched, so it lists the events the
  model raised rather than every declared name the log mentioned.
- Fixed (#1119): The explicit runtime no longer copies an `exists` witness scope into the caller's bindings, so a quantifier binder that shadows an action parameter can no longer overwrite it -- `explicit`/`auto` returned `proved` for a violated invariant, `bmc`/`induction` failed witness replay, and the conformance vectors recorded the witness index.
- Fixed (#1124): `docs/DESIGN-approval.md`'s worked example now names `examples/e2e/2_requirements.fsl` -- a specification that exists and carries requirement IDs -- instead of the nonexistent `specs/order.fsl`, so the `fslc ledger` / `fslc approval create` / `fslc document generate` sequences it shows run as written. The same section's prose claim that a specification without requirement IDs is rejected is now demonstrable against the specification it names.
- Fixed (#1133): `fslc domain replay` no longer reports a **type-mismatched log
  parameter** as a guard rejection that blames the FSL model. Since #1116 a
  value that is not of its declared type fails closed, but the conversion
  failure was folded into the same verdict the model's own refusal produces, so
  `{"value":"garbage"}` on an integer input came out as
  `command_rejected_by_model`/`runtime_command_must_be_enabled_by_domain_model`
  with the repair *"change the implementation command path or update the FSL
  decide/evolve model"* — advice to edit a `decide`/`evolve` that was never
  consulted, since the row was refused before any guard ran. The conversion's
  own message was computed and discarded. A conversion failure now takes its
  own `failed_rule` on the same `kind` —
  `runtime_command_parameters_match_declared_types` for a `command` row,
  `effect_completion_parameters_match_declared_types` for an
  `effect_completion` row, including the correlation value filled in from
  `correlation_id` — and the discarded message is carried in the witness as
  `parameter_error` (`parameter 'value' must be an integer`). The repair names
  the row and the two things that can actually disagree: the value the
  implementation logged, and the parameter's declared type. A parameter the
  command does not declare takes the same rule, for the same reason. Genuine
  guard rejections and genuine `effect_completion` lifecycle mismatches keep
  their existing rules and repair text, and every conformant log is unchanged:
  `examples/domain/order_async_effect_replay.jsonl` and the `issue_518_*`
  fixtures produce byte-identical output to v4.7.0. The finding `kind` enum in
  `schemas/fslc/domain/finding.v0.schema.json` is unchanged; a consumer that
  branches on `kind` alone still cannot tell the two causes apart, which is the
  new-`kind` option deliberately left to #1133's own follow-up.
- Fixed (#1134): the business dialect now checks its control catalog instead of
  discarding it. `BusinessItem::Control` fell into the catch-all arm of
  `lower_business`'s item loop, so a `control` declaration --- and every
  `satisfies` naming one --- was parsed, accepted and dropped, and `check`
  answered `ok` for a policy pointing at a control the document never declared.
  An unknown reference is now a located error at the policy or goal that wrote
  it, and a control no policy or goal satisfies raises the `unused_control`
  warning `docs/DESIGN-dialects.md` already promised, located at the
  declaration. Both were already implemented in the Python front end
  (`src/fslc/dialects.py`), so this closes a port gap rather than narrowing the
  language; a control confirms a declared-and-satisfied catalog still checks
  clean, and `check` exit codes are unchanged for all 537 tracked `.fsl` files.
- Fixed (#1135): `tools/check-merge-readiness.sh` now runs every step of a lane and fails afterwards, instead of aborting at the first failure and hiding the other thirty steps of `check_automation`. A step whose tool is absent is reported as a skip, listed separately from failures, and still exits the lane non-zero -- a skip is not a pass -- and in CI (`GITHUB_ACTIONS`, `CI`, or `FSL_READINESS_CI`) a missing tool is a hard failure with no way to re-enable skipping, so a CI image that loses a tool cannot go green. `--fail-fast` restores the previous stop-at-the-first-failure behaviour. See `docs/DESIGN-ci.md`, "Run every step, then fail".
- Fixed (#1136): `docs/DESIGN-approval.md`'s walkthrough now runs end to end. Its opening `approval create` no longer signs the record the later `approval check`, `ledger --approval`, and `approval diff` examples consume without a trust anchor, which made those three blocks exit 2 with `no trusted Ed25519 public key matches`. The signed flow keeps its own block, which now creates the v2 record it verifies with `--trust-key`, and the "Record contract" examples report the current `generator_version` (`4.7.0`, not `2.7.0`).
- Fixed (#1147): `fslc chain` now verifies each layer through the same options path as `verify` and `sweep`, so it reads and writes the verify cache; before, it bypassed the cache entirely and re-solved every layer on every run (0 entries after two runs). An unchanged layer is a cache hit, an edited layer alone is re-verified, and a hit adds only the documented `cache` field to that layer's `detail`. Regression controls in `rust/fslc/tests/chain_cli.rs` fail on the old code.
- Fixed (#1148): the verify cache key no longer embeds the checked spec's canonicalised absolute path, as `docs/DESIGN-incremental-verify.md` already promised. The same spec bytes at another path, in another worktree or under another file name now hit the cache (13.32 s miss against a 0.02 s hit before), while a changed spec, a changed dependency or a changed option still misses. The key prefix moves from `v3` to `v4`, so entries written before this change are never matched and a stale `verified` cannot be replayed; they age out through normal eviction. The design document now states the native key and its storage layout as implemented.
- Fixed (#1153): An `x is some(v)` pattern binding no longer overwrites an action parameter, binder or let of the same name in the native runtime, verifier and type checker, restoring the guard the frozen reference has in `_eval_requires`. Before, the native `fslc` and the Python reference returned opposite `verified`/`violated` verdicts for the same spec (`act(r=0)` wrote `w[1]` natively), `and` was not commutative over such a binding, an `Option<Bool>` matched into `r: R` retyped `r` and was rejected, and a failed shadowing match ended `bmc`/`induction` in `trace state mismatch at step 1`. A pattern binding with a new name still reaches the action body.
- Required (#1127): Markdown link targets now resolve or the merge-readiness lane
  fails. `tools/check-doc-links.py` runs repository-wide (214 documents, 396
  in-repository link targets, about 0.1s, no network and no build) alongside its
  slug selftest and the new accepting/rejecting controls in
  `tests/test_doc_links.py`, which reproduce issue #1127's measured table in CI --
  including a `.fsl#kind:name` fragment naming an element the specification does
  not declare (the forward direction of #1124) and a `other.md#anchor` whose
  anchor exists only in a different document. What the gate does not check (a bare
  path in prose) and does not claim (that every FSL element is discussed by some
  document, #1138) is recorded in `docs/DESIGN-ci.md`. The one pre-existing
  finding, `CONTRIBUTING.md`'s link to a `CODE_OF_CONDUCT.md` that has never
  existed in this repository, was removed rather than answered by inventing a
  policy document.
- Documented (#1124): `docs/README.md` now states the convention that separates a broken FSL reference from a deliberate example -- a `specs/`- or `examples/`-prefixed path in `docs/**.md` names a file that exists, while an illustrative path is written bare (`spec.fsl`, `<spec.fsl>`) -- together with the one-liner that checks it, a note that no CI gate enforces it yet, and an explicit statement that the backward direction (every FSL element being discussed by some document) is not claimed. `DESIGN-layers.md`'s `fsl-project.toml` example and `DESIGN-changelog-fragments.md`'s rename experiment were brought into line.
- Documented (#1128): `docs/DESIGN-claim-binding-spike.md` records the spike that binds one hand-written claim to one named FSL element and measures whether the claim is raised for re-verification when the bound element changes (it is) and when another part of the same specification changes (it is not). It decides that a prose-only rewrite must raise the claim under its own reason, compares how GitHub renders the three annotation syntaxes, and names where the mechanism overlaps the existing document checks. `tools/spike-1128-claim-binding/` holds the projector; nothing is wired into a gate.
- Decided (#1126): mystmd is not adopted for `docs/`. The spike record `docs/DESIGN-myst-spike.md` measures both stages. Stage 1: every one of five reference breaks exits 0 under `myst build --site --strict` until `error_rules` and `--check-links` are configured, and no single switch closes the table. Stage 2: all four stage-1 objections are configurable — `error_rules` entries take a `keys` glob list, a document-stage plugin re-identifies headings with GitHub's slug algorithm, a local `site.template` path builds the whole gate offline, and the one prose failure is a nested-code-span bug GitHub renders wrong too (fixed here). Configured that way all 105 documents build clean and a typed FSL reference plugin closes the forward check, with the measured caveat that a plugin's report only reaches the exit code from a transform, never from a role's `run()`. Adoption is still declined: the transforms that close the reference rows reimplement GitHub slug and per-file anchor resolution in JavaScript against files on disk, stock mystmd resolves a wrong-file `other.md#anchor` silently, and the notation that makes the typed reference work in both renderers is an ordinary Markdown link that needs no AST. `tools/spike-1126-myst/` holds the configuration and both plugins for reproduction; `tools/check-doc-links.py` is the counter-proposal, extended here to resolve `.fsl#kind:name` fragments, and is deliberately not wired into a gate.
- Decided (#1132): `x is some(v)` is a binding, never a match against an
  already-bound `v`, and the binding is lexically confined — a collision with
  an action parameter does not survive into the action body. The Python
  implementation already does this; the Rust port does not, and the two return
  opposite verdicts on the same specification. `docs/DESIGN-pattern-binding-scope.md`
  records the decision, the measurements, and the port fixes it implies
  (including a Rust-only `trace state mismatch` internal error on a failed
  shadowing match).
- Decided (#1138): the backward documentation direction -- every FSL element being discussed by at least one hand-written document -- is now measured and reported, and deliberately **not** gated. `tools/report-doc-backward-coverage.py` is the report and `docs/DESIGN-backward-doc-coverage.md` is the spike record. The denominator is every FSL element declared under `examples/` except `examples/gallery/` -- 516 elements in 120 files -- with `tests/`-style fixtures (323 files, 702 elements), the `specs/` conformance corpus (23 / 134), the deliberately-invalid `examples/gallery/` (71 / 213) and the generated site's `docs/intro/specs/` (3 / 19) excluded, each with its reason; `examples/self/` and the feature examples are kept in scope even though keeping them worsens every ratio. Measured against 178 hand-written Markdown documents, 0 of 516 elements are discussed at element granularity (`<path>.fsl#kind:name`, a notation used three times in the whole repository), 267 at non-index file granularity, and 444 at file granularity, with 35 of 120 in-scope files named by no hand-written document at all. That 0/516 is why the outcome is a periodic report: a gate would stop all work on day one, and an element-level ratchet over a zero baseline is a gate in disguise. A file-level ratchet is deferred until the 35 unnamed files are named. What is still not claimed: a reference is a reference, not an assurance that the prose is correct or current (that is #1128).
- Decided (#1139): the `def` capture check stays as a semantics error with its
  current message. It guards capture-avoiding substitution at expansion time,
  not the runtime binder leak #1119 removed, so #1119 could not have made it
  redundant. `tests/test_named_predicates.py` now pins the two expansions of a
  rejected call to opposite verdicts; see `docs/DESIGN-pattern-binding-scope.md`.

## [4.7.0] - 2026-09-24

- Added (#1055): examples now require an adjacent rationale whenever a teaching command uses `--deadlock ignore`, preventing unexplained flag regressions.
- Changed (#852): Cache-budget audit reports now separate listing-derived, superseded-generation recoverability from unattributed net growth while retaining the physical-byte budget verdict and non-atomic-observation caveat.
- Changed (#1082): the release skill bundle and the installer now carry every directory under `skills/`, so `fsl-from-code` and `fsl-requirements-document` ship for the first time. The six-name list that had to be edited in both places is gone.
- Fixed (#587): the WASM browser probe now reports why it failed instead of
  folding every failure into one exit code. A stalled CDP call or completion
  poll exits 124 (`probe_timeout`, with `mode` `cdp` or `poll`), a refuted
  native/WASM parity comparison -- including a malformed WASM envelope, a stale
  corpus exclusion, or agent fail-closed drift -- exits 65 (`parity_violation`),
  and anything else, including a malformed native envelope, stays a harness
  failure (exit 1). All three still fail the `WASM` job; nothing is retried or
  downgraded. A failure before the browser starts now exits instead of hanging
  on the open HTTP server.
- Fixed (#998): example commands no longer suppress default deadlock warnings where that suppression was unnecessary.
- Fixed (#1080): `fslc sweep` no longer treats depth-limited-only reachability
  cells as counterexamples. The public CLI contract is breaking: a grid made
  entirely of such cells now returns `sweep_inconclusive` with exit 1 and a null
  minimal counterexample, while grids with a determinate success and no true
  failure return `sweep_passed` with exit 0.
  Some grids that previously returned `sweep_failed`/exit 1 now return
  `sweep_passed`/exit 0, and `minimal_counterexample` is null when no true failure
  exists; consumers should gate on the exit code rather than
  `result == "sweep_failed"`.
- Decided: `main safety and CI` no longer requires its six contexts to have run against current `main`. The six remain required on the pull request's own head; what is given up is the guarantee that the head included whatever landed in the meantime, and why that trade was taken here is recorded in `docs/DESIGN-ci.md`.

## [4.6.0] - 2026-09-16

- Fixed (#980): `approval create`/`check`/`diff` now reject a `.md` positional
  with `kind:"usage"`/`FSL-INPUT-LITERATE-UNSUPPORTED` (exit 2), the same
  input-kind guard `check`/`verify`/`db`/`ai`/`causal`/`domain` already apply
  (#694), instead of parsing it as an FSL spec and reporting a false
  `FSL-PARSE` syntax error at `1:2`. `--kind requirements_document`'s
  legitimately `.md`-shaped input is `--artifact`, never the positional, so
  `--kind` does not change this. The guard runs immediately after the
  positional is resolved and before `check`/`diff` read `--record`, so a
  mismatched record no longer masks the same defect. `kind` and
  `diagnostic_code` change for this previously-incorrect input; `result` and
  exit code (2) do not.
- Fixed (#992): refresh stale design notes for Public Kernel v2 publication,
  refinement `preserve progress` checking, and cross-layer traceability (replace
  the nonexistent `fslc trace` subcommand with `fslc analyze --projection
  traceability_graph`).
- Fixed (#996): `docs/DESIGN-assurance-classes.md` now documents the native Rust
  assurance classifier (`ledger::assurance_token`, `formal_assurance`,
  `evidence_verdict`, and related APIs) as the authoritative contract; the frozen
  Python `src/fslc/assurance.py` surface is retained as a reference-only parity
  map. Rust/Python `formal_assurance` vs `classify_element` evaluation-order
  divergence on `result:"error"` with `completeness` set remains explicitly
  unadjudicated (issue #995).
- Fixed (#997): `fslc diff` no longer silently drops a `verify { values X = lo..hi }`
  bound from `scope.old`/`scope.new`/`scope.applied_to_old` when `lo`/`hi` is a
  declared const or a compound expression (e.g. `-1..HI`) instead of a bare
  integer literal. The bound is now resolved against the same evaluated
  domain `check` reports (`TypeDef::Domain`), so a const-only bound change is
  recorded as `scope_changed`, applied to OLD, and rejected by
  `--forbid scope_changed` the same way a literal bound change already was.
- Fixed (#1009)!: `fslc html` now exits according to the embedded verification
  verdict instead of always returning 0 after generating the report. Pipelines
  that gated only on the process exit code while ignoring `result` will see exit
  1 for `violated`, `reachable_failed`, `unknown_cti`, `unknown_budget`,
  `refinement_failed`, and `impl_violated`; successful verification still exits
  0 and the HTML artifact is still written on failure.
- Fixed (#1011): `civil_days` now rejects impossible calendar dates such as
  `2026-02-31` and non-leap `2026-02-29` instead of normalizing them, so timing
  eligibility cannot admit unparsable evidence periods; leap-year and valid-date
  controls confirm the fix does not over-reject.
- Fixed (#1015): `fslc kernel` no longer panics on an `ai_component` whose
  `authority` block names an undeclared tool; it now returns the same
  `{"result":"error","kind":"semantics"}` envelope and exit 2 that `fslc check`
  already returned for this input, instead of exiting 101 with no JSON. A
  negative control in `error_envelope_parity.rs`'s `kernel` coverage pins this
  envelope so the lookup panic cannot be reintroduced silently; `fslc check`'s
  envelope for the same input is unchanged.
- Fixed (#1018): documented the native-vs-frozen-Python divergence for inline
  `implements` fail-closed folding into `check` / `verify` top-level verdicts
  (#1002, PR #1026 `246f0987`). Native is authoritative; the frozen reference
  retains the pre-folding `ok` / `verified` primary result with the seam under
  nested `implements`.
- Fixed (#1020): `--strict-tags`'s `Declared` side now auto-collects requirement-block
  IDs from the requirements dialect, not only the lines of an optional
  `--requirements ids.txt` file. `docs/DESIGN-strict-tags.md` section 2 defines
  `Declared` as `--requirements` union the requirements dialect's requirement-block
  IDs, and calls that union "essential" for catching an empty requirement block --
  one declared but never formalized, whose trace disappears after expansion. The
  native implementation collected only the file half, so without `--requirements`
  the `Declared` side never ran at all and an unformalized empty requirement block
  passed silently. `check`/`verify` (independent option-parsing paths for
  `--strict-tags`/`--requirements`) now both report such a block as
  `unreferenced_requirement` with no `--requirements` flag needed. Exit codes are
  unchanged: no path in the codebase branches exit status on the `warnings` array's
  contents, so a spec that starts warning here still exits the same as before. An
  existing `--requirements` file's already-referenced IDs keep their original
  file-line warning order; only IDs that reach `Declared` solely through
  auto-collection are appended afterward, so established exact-match warning-array
  expectations are undisturbed.
- Fixed (#1023)!: `fslc verify`'s cache key is now built from the dependencies
  actually read while resolving `use`/`from`/inline `implements` (including
  across a symlinked alias), not from walking the checked spec's parent
  directory. A `from "../x.fsl"` dependency outside that directory used to be
  invisible to the key, so a stale `verified`/exit 0 cache entry kept being
  served after the dependency started violating the refinement contract;
  editing an unrelated sibling `.fsl` file that nothing depended on used to
  invalidate the entry too. Both are fixed: the key's dependency domain now
  matches what the verdict actually depends on. Because a cache hit now has to
  read those dependencies to know its key is still valid, a spec whose
  dependency has since become unreadable now fails closed (`error`, exit 2) on
  what used to be a cache hit, instead of replaying the stale pre-removal
  verdict. The on-disk cache generation moved from `verify/v2` to `verify/v3`
  (orphaning any existing `verify/v2` entries on disk; nothing removes them).
- Fixed (#1027, #1029): `docs/RELEASE.md` §2 promotion branch names now match the
  `production-policy` required check (`release/vX.Y` instead of
  `release/vX.Y.Z-candidate`), and pre-merge pin verification requires tree
  equivalence plus candidate identity as either HEAD or first parent (covering both
  a direct pin and a sanctioned `-s ours --no-ff` preparation). The v4.4.1
  carry-forward section is removed; production (`99f150e9`) again contains
  `docs/DESIGN-nested-option-support.md` natively after the v4.5.0 promotion
  completed the carry-forward. The release skill no longer points at the removed
  carry-forward steps.
- Fixed (#1031): the release workflow now runs `cargo test --release --locked -p fslc-rust
  -p fsl-lsp` on the MSRV toolchain (`1.88.0`) before building release binaries, so shipped
  artifacts are no longer built from a toolchain that never exercised their tests; library crates
  remain covered by PR/main CI at `1.98.0`.
- Fixed (#1032): split the `rust-native-z3` matrix job into two statically named
  jobs so skipped checks no longer appear as unexpanded `native Z3 4.16 (${{
  matrix.os }})` in the pull request checks list.
- Fixed (#1041)!: `check`/`verify`'s inline `implements` correspondence search
  (`check_refinement`) is now bounded by a fixed internal state-count budget
  (`IMPLEMENTS_SEARCH_BUDGET`, 50,000 states; no CLI flag, and not shared with
  `--explicit-budget`, a different search). Exceeding it reports top-level
  `unknown_budget` and exit 1 on both entry points instead of running
  unbounded. Observable change: an inline `implements` search over a large
  enough domain now exits 1 with `unknown_budget` where it previously grew RSS
  without bound (P1 measured ~3.8 GB on one corpus-scale domain, with a flat
  control run confirming the growth was specific to the search). A search over
  50,000 states now ends in `unknown_budget` / exit 1 where an environment with
  enough memory could previously have run it to completion and exited 0, so this
  is recorded as breaking even though no spec in this repository's corpus reaches
  the budget: the population that would be affected is users' own specs, which
  cannot be measured from here. The budget
  constant is included in `verify`'s cache key so a future change to it cannot
  replay a stale verdict.
- Fixed (#1043): `outcome_class`'s `semantic_diff` arm now reads the
  `gate.passed` field `run_diff` actually publishes, instead of a top-level
  `violations` array `run_diff` never writes (it only appears nested under
  `gate`). This is a latent-defect fix in the shared classifier: no in-tree
  caller currently folds a `semantic_diff` envelope through `outcome_class`,
  and `fslc diff`'s own process exit already computed its status independently
  and correctly. There is no observable CLI behavior change.
- Fixed (#1044): exclude intentionally malformed gallery examples with path-component matching on Windows and prevent three latent LSP fixture URI portability issues with filesystem-derived URIs.
- Fixed (#1051): Updated `fslc check` documentation to describe its inline `implements` checking scope and depth-bounded, solver-free behavior.
- Fixed (#1061): the #1023 cache-key regression test no longer collides with
  itself. Its temporary fixture roots were separated only by a `SystemTime`
  nonce, and `{name}` (`inside`, `outside`) is shared by a matrix column's
  tests while `process::id()` is constant within a test binary. The clock has
  microsecond resolution, so two tests that start concurrently read the same
  value, build the same root, and -- because `create_dir_all` succeeds on an
  existing directory -- share one tree silently: one test's write overwrites
  the other's input and the first `Drop` deletes the other's spec mid-run,
  surfacing as an unrelated `io` error. The root now carries a process-wide
  `AtomicU64` counter, so uniqueness comes from construction rather than from
  clock resolution, and it is created with `create_dir`, so a future
  collision fails where it happens instead of several assertions later.
  Measured on the same binary and harness: 3 failures in 30 runs before, 0 in
  30 after, with the distinct-root count observed from outside the process
  staying at the expected 2 (`inside`) and 3 (`outside`) in every run after
  the fix. No product code changed.
- Fixed (#1065)!: `analyze --projection code_audit` no longer misreads a prose
  mention of the `@fsl.trace ` sentinel (for example, a comment describing the
  annotation convention itself) as a broken annotation. A line is now treated
  as an annotation attempt only when the text following the sentinel, after
  stripping leading whitespace, starts with `{`; any other line is skipped
  without a finding or an error. A line where the sentinel is followed by `{`
  but the JSON is malformed, or where the JSON parses but fails
  schema/kernel-target/`origin_assurance` validation, still fails with the
  same `CodeAuditError::Semantics` exit-2 error as before -- only the
  discriminator for "is this an annotation attempt at all" changed. Recorded
  as breaking because it changes the CLI's exit code for an existing
  observable input: a source tree containing a prose mention of the sentinel
  with no following JSON object, which previously exited 2 (`error`), now
  exits 0 (`analyzed`) with the mention simply absent from `findings` and
  `coverage`. This is a known cost of the `{`-prefix discriminator, not an
  unverified gap: a malformed annotation whose payload does not start with `{`
  (for example a typo like `@fsl.trace requirement=REQ-1`) previously failed
  with the same `malformed code trace` error and is now skipped as well --
  the rule cannot distinguish it from a prose mention, so the fix trades
  detecting that narrower class of broken annotation for not misreading prose.

## [4.5.0] - 2026-09-11

- Added (#316): explicit-state verification now reports deterministic per-action
  `enabled`, `fired`, and `no_op` execution-profile counts without changing the
  existing action-coverage contract.
- Added (#841): native typed-state support for nested `Option` values around scalar payloads, with matching symbolic, Monitor, and explicit-state semantics.
- Added (#844): `fslc testplan SPEC [--depth N]` selects the bounded
  `conformance` vectors — the accepting ones and the `requires_failed` ones no
  test-generation path consumed — into a closed `test-plan.v1` JSON document
  (`schemas/fslc/kernel/test-plan.v1.schema.json`). Kernel and conformance JSON
  are built from one checked model, so a plan cannot pair two snapshots. A plan
  is a selection, never a verdict: it carries `formal_result: "not_run"`,
  `assurance_effect: "none"`, and a `do_not_assume` list, and it records the
  requirement to pass a spec at the implementation's layer granularity.
- Added (#848): add opt-in `--oracle-attribution` to `fslc mutate`, exposing per-mutant `killers`
  arrays and `by_obligation` sole/shared counts keyed by oracle display names while
  leaving default output unchanged.
- Added (#885): add `fslc counterexample export` and the versioned `reproducer.v1` JSON artifact
  for bounded safety-invariant counterexamples (issue #885 slice 1).
- Added (#1007): the intro site manual gained a sitewide orientation layer. A
  four-stage correctness backbone (business, requirements, design, implementation)
  renders on every manual chapter and names each stage's intent, seam, evidence,
  and *limitation*, highlighting the current chapter's position without claiming
  whole-system proof. Category hubs gained an audience/problem to FSL-contract to
  evidence-readout to next-action journey, and the reference hub states the four
  canonical sources in authority order so the frozen Python reference under
  `src/fslc/` is never presented as the CLI authority. The home page gained a
  bounded correctness-chain route into the example gallery with commit-pinned
  example links. The shared top bar now carries the five official sections, and
  every page gained a localized skip link to a focusable `<main>` landmark.
- Changed (#798, slice 2): retired the #796 CLI post-validation on `domain
  analyze` and `domain expand` now that scope-aware AST normalization makes the
  renderer agree with direct lowering; rejecting controls in
  `issue_796_domain_command_validation` still pass without the guard.
- Changed (#800)!: `fslc ai check`, `fslc ai compat`, and `fslc ai replay` now
  reject invalid `ai_component` declarations (undeclared authority tools and
  unknown `check hard` rule names, plus checked project parsing for `ai replay`)
  with exit 2 instead of returning `ai_project_analyzed`,
  `compat_profile_generated`, or `replay_conformant` on false greens.
- Changed (#841): reject nested Map state values that native `check` previously
  accepted but explicit-state execution could not initialize; the shared CLI/LSP
  type hints now describe the recursive nested-Option boundary for struct fields
  and state values.
- Changed (#926): the Actions cache budget audit reports a `generation-coexistence-partial-explanation`
  finding alongside `budget-exhausted` when at least one `refs/heads/main` `{sharedKey, platform}` pair
  holds more than one live generation (measured 2026-09-04: `rust-native-z3` Darwin/arm64 held two,
  driven by `Swatinem/rust-cache` hashing the runner's entire installed-toolchain list rather than
  anything this repository controls). This is diagnostic only: `budget-exhausted` itself continues to
  judge the same raw physical total it always has (`Math.max(usageBytes, rawSummed)`, unchanged),
  because GitHub's budget and least-recently-used eviction act on physical bytes regardless of which
  generation this repository considers current. An earlier version of this change instead subtracted
  the superseded generation's bytes from judgment; independent review executed two counterexamples
  against it (a same-identity pair physically filling the whole budget judged as half; a listing-derived
  subtraction applied to an independently-observed usage total already higher than the listing, on the
  unproven assumption the two non-atomic observations share the same bytes) and it was reverted before
  merge. A `generation-coexistence` entry in the always-reported, non-gating `informational` array
  separately states the raw entry count and superseded-generation byte count whether or not the budget
  is exhausted. Every non-main-branch entry, including any `refs/pull/*` cache, is still counted
  individually and in full for every existing rule, so the original #747 incident shape (many
  different refs each holding their own cache) remains fully detected.
- Changed (#941, #763): Split the FSL skill reference into topical, indexed resources and route authoring skills to load only the needed guidance.
- Changed (#961): the `no_user_invariants` model warning is suppressed only for
  safety-bearing declarations (`invariant`, `trans`, `forbidden`, `implements`);
  `reachable`, `leadsTo`, and `acceptance` alone no longer suppress it. Shared
  finalization in `fsl-core` replaces frontend message-string filters.
- Fixed (#694): literate Markdown FSL's dialect-command diagnostic gap: 18 `db`/`compat`/`domain`/`ai check`/`ai replay`/`ai compat`/`causal` spec-path commands that used to hand raw `.md` input to their dialect frontends and report the Markdown's own first character as a spec syntax error now fail closed with `FSL-INPUT-LITERATE-UNSUPPORTED` (`kind:"usage"`, `loc` naming the input file only), matching `lint` and the other registry-gated commands via `fslc_rust::literate_access::literate_access`. `ai eval`/`ai regress`/`ai drift` keep their existing `load_ai_project` literate `.md` frontend and are unchanged. No exit code changed.
- Fixed (#698): distinguish proven duplicate writes from conservative `forall` index-distinctness
  rejections in native `check`/`verify`, the browser Worker, and LSP diagnostics.
  Unproved injectivity now reports `FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED` with
  `loc` and a safe repair `hint` when one exists; acceptance is unchanged.

  Known limit: an affine write inside an `if` within the `forall` is still reported with the previous duplicate-write message, because `assignment_for_target` only scans top-level assignments. That shape is unchanged from before this fix, not a regression.

  The quick-fix is withheld rather than guessed: the RHS scan is built on the shared `expr_children`/`binder_exprs` walk instead of a second hand-written match, a binder whose domain does not start at zero contributes its own lower bound, a filtered binder is refused outright, and an RHS that already names `k` blocks the rewrite. The classification, code and location still arrive in every one of those cases.

  The hint is emitted only for a shape it can render correctly: a named key type (an inline `Map<lo..hi, _>` key would render as `forall k: lo..hi`, which the grammar does not accept after `:`), and an offset that is bracketed unless it is already a single token.
- Fixed (#773): `domain generate` now preserves the typed source-diagnostic
  location for invalid domain guards, matching the checked-Kernel envelope.
- Fixed (#798, slice 1): `domain_kernel_source` now normalizes domain expressions
  through scope-aware AST composition instead of blind `str::replace`, so path B
  agrees with `lower_domain` on generated-name misuse and command-input shadowing.
  `KNOWN_DIVERGENT_DOMAIN_FIXTURES` is empty; slice 2 retires the legacy string
  path and #796 CLI suppression.
- Fixed (#801): prevent error-envelope parity coverage from silently dropping AI project input shapes by separating the semantic input-shape population from cell classification.
- Fixed (#808): native `fslc domain generate`, `domain replay`, `domain testgen`, and `domain check` now derive their AST, checked Kernel/model, generated scaffolds, replay monitor, and edition metadata from one captured root-source snapshot, preventing concurrent atomic replacements from mixing document revisions in one result.
- Fixed (#831): compose diagnostics no longer invent a `1:1` component location
  for unreadable files; alias-qualified type resolution and synchronized-reference
  errors now carry authored source spans into the public envelope.
- Fixed (#841): ordinary trace, replay, testgen, and Worker JSON no longer collapse
  `none` and `some(none)` for a nested `Option` state value; the tagged form
  `{"kind":"some","value":…}` is introduced only where the declared payload is
  itself an `Option`, so every previously supported `Option<scalar>` byte is
  unchanged, and trace changes are computed from typed values so a struct's own
  `kind`/`value` fields are never mistaken for an `Option` tag.
- Fixed (#928): `fslc check` now returns the AI authority unknown-tool semantic envelope instead of panicking during its diagnostic preflight.
- Fixed (#932): native `fslc check`, `fslc db check`, `fslc explain`, and `fslc html` now derive their validation, checked Kernel/model, verification, rendering, and post-processing from one captured root-source snapshot, preventing concurrent atomic replacements from mixing document revisions in one result.
- Fixed (#945): routed FSL skill citations to the split references and aligned the changelog-fragment surface contract with its enforced glob.
- Fixed (#946): the Codex Cargo lock wrapper no longer waits for the lock its own
  parent holds. The lock-owning wrapper marks its child with
  `FSL_CARGO_LOCK_HELD=<absolute path of the lock file>` -- in the environment and
  again inside the shell, after the login profile runs, specifically so that a
  login profile which unsets the marker cannot reintroduce the self-deadlock: the
  re-export inside the shell restores it before the command runs, so a nested call
  still bypasses acquisition instead of waiting on its own parent. Acquisition is
  bypassed only when the marker names *this* lock and the lock is still held. A
  marker from another repository falls back to ordinary serialization, because it
  never matches this lock's path. A marker left behind after the owner released
  the lock is stale: the next call finds the lock free, takes it, and becomes the
  new holder like any ordinary invocation. A `flock` failure that is not
  contention is still reported at once rather than waited out. The outer
  invocation keeps the Git common-directory lock for the whole command, so
  serialization across worktrees is unchanged. Known limitation: a descendant
  that outlives its own wrapper can still bypass while an *unrelated* invocation
  holds the same lock; closing that needs owner identity, which is not attempted
  here.
- Fixed (#947): keep Codex edits unblocked when the changelog advisory checker cannot run, while preserving fragment-violation blocking.
- Fixed (#956): zero-argument function citations now require an actual item
  definition instead of accepting registration string literals, and P2's missing
  `corrupting_state_step_kind_or_location_cuts_a_p2_edge` rejecting control is
  implemented.
- Fixed (#989): causal evidence whose timing eligibility cannot be established
  is now excluded from current support, as the applicability rule already
  required. An unconvertible observation window, or a claim whose `lag` is
  `unknown` so there is no `lag_min` to compare against, previously recorded a
  `not_evaluable` entry while the same evidence/claim edge stayed
  `applicable: true` and cast its vote, so an artifact whose timing could not be
  checked could still decide a claim's `causal_support` between `supported`,
  `mixed` and `challenged`. The edge now excludes with
  `evidence_timing_not_evaluable`, and the record and the vote agree.

  Each independent cause is named rather than collapsed into one sentence:
  `period_start_missing`, `period_end_missing`, `period_date_unparsable`,
  `period_end_before_start`, `timebase_not_convertible`,
  `week_window_not_whole_weeks`, and `claim_lag_unknown`. A window that cannot
  convert for a claim whose lag is also unknown reports both, which the previous
  single branch dropped. The records now also travel on the evidence/claim edge
  itself in `causal_evidence_graph` output, not only in the envelope-level
  `not_evaluable` array.

  Unchanged: `w == lag_min` still passes and votes, `w < lag_min` still excludes
  with `evidence_window_shorter_than_lag`, and no support-vocabulary value is
  new -- a claim left with no applicable artifact is
  `unsupported_by_current_evidence` exactly as before.

  Known limit, unchanged by this fix: a period date is parsed as `YYYY-MM-DD` with a month in
  1..12 and a day in 1..31, and day-of-month validity against the month is not checked, so an
  impossible date such as `2026-02-31` converts as its normalized value instead of reporting
  `period_date_unparsable` — the edge then votes on a window computed from that normalization.
  That is pre-existing `civil_days` behaviour, it also governs `valid_until` staleness, and it is
  tracked as #1011 rather than widened into this fix.
- Fixed (#993): `fslc document claims` no longer rejects a `verify { values N =
  lo..hi }` bound written with compile-time consts. The projector now reads the
  evaluated `KernelModel::types` domain bounds the model already built, so RCIR
  v1's `analysis_scope.values[].lo/hi` carry plain integers and the renderer
  shows `-1` to `2`. This aligns the projector with `docs/LANGUAGE.md`, which has
  always written the bound as `values <Number> = <lo>..<hi>` without restricting
  it to integer literals — it does not widen a documented contract. RCIR v1's
  schema is unchanged, and existing literal bounds project byte-identically. A
  `values` bound naming a number the spec never declares is not rejected either,
  so no `values` bound makes `document claims` refuse a `requirements` spec that
  `fslc check` accepts; a dialect RCIR v1 has no adapter for is still refused, as
  a scope boundary. Rejection of a
  non-constant bound is unchanged and now carries the model's own message
  (`unknown constant 'LO'` rather than `must be an integer literal`); a
  preservation control pins that, and separate detectors cover the evaluated,
  literal-fallback, and omitted cases. `fslc diff` still drops such a bound
  silently — that surface is tracked in #997, not fixed here.
- Fixed (#1000): `fslc kernel` now projects relation state fields initialized with
  `Set {}` as `set_lit` with relation-typed `type` and empty `items`, matching
  `fslc check`; non-empty relation literals remain rejected.
- Fixed (#1001): `fslc check|verify --strict-tags --requirements` now counts a requirement
  ID linked by a typed `@requirement(...)` annotation on an `init` block or on an
  `acceptance`/`forbidden` block as referenced, instead of reporting it as
  `unreferenced_requirement`; a genuinely unreferenced ID still warns.
- Fixed (#1002)!: a requirements spec can declare an inline `implements Abs from
  "..." { ... }`, which makes `check` and `verify` refine that spec against the
  upper layer as part of the ordinary run. A failed refinement used to be
  reported only under the nested `implements` key: the top-level `result` stayed
  successful and the process still exited 0, so a gate reading either of them
  passed a spec whose refinement had failed. The failing seam verdict
  (`refinement_failed` or `impl_violated`) now becomes the top-level `result`
  verbatim and the process exits 1, while `implements.violation` keeps the
  seam-specific evidence. Five subcommands change their exit code from 0 to 1 on
  such a spec: `check`, `verify`, `sweep` (whose `sweep_passed` becomes
  `sweep_failed`), `mutate`, and `ledger`. `mutate` is worth naming separately
  because its loss is silent rather than wrong: it re-emits the failing baseline
  envelope and generates no mutants at all, so a spec with a broken seam stops
  having a kill rate rather than having a lower one. `db check` and `domain
  check` are unaffected because neither reaches verification on this input: both
  reject a requirements document by kind (`expected a dbsystem document` /
  `expected a domain document`, exit 2) before any kernel projection. A `verify`
  scoped with `--property`, `--exclude-property`, or `--from-state` still omits
  `implements` entirely and therefore still cannot gate the seam (#1008).

  Those seven exit codes were measured, not derived: with a binary built from
  `81b40e3b` (the previous `main`) and one built from `71613598`, each run in
  its own checkout, against
  `tests/fixtures/chain/requirements_broken_implements.fsl` -- blob `71510f81`
  at the former and `e1129ccb` at the latter, differing by two added comment
  lines and nothing else -- whose `implements ... from "business.fsl"` import
  resolves to blob `ce15ac77` in both. The five went 0 -> 1; `db check` and
  `domain check` returned exit 2 with the quoted messages on both binaries.

  `fslc html` is worth one more line, because its report was actively misleading
  rather than merely incomplete: on the same spec its top-level Result went from
  a green `verified` badge to a red `refinement_failed` one. Its exit code is
  unchanged -- `html` still exits 0 over a failing spec, which is #1009 and is
  not fixed here -- so the badge and the exit code now disagree deliberately,
  and only the badge has been corrected.
- Fixed (#1003): propagate `fslc verify --instances` / `--values` overrides into inline `implements`
  abstract specs, filtered to names the abstraction declares, so refinement keeps
  running at the same world size on both sides.

  Scoped runs now apply the same model-warning finalization as unscoped runs, so a
  spec's `warnings` no longer depend on whether `--instances` / `--values` were
  passed. Measured on the #1003 reproducer, this removes a `no_user_invariants`
  warning that only scoped runs emitted.
- Fixed (#1007): the business-layer manual pages
  (`docs/intro/business-layer.{en,ja}.html`) no longer print the flagship e2e
  chain with `--deadlock ignore`. The flag was suppressing a diagnostic that
  passes: measured with `--no-cache`, twice, `verify examples/e2e/1_business.fsl
  --engine induction` and `verify examples/e2e/2_requirements.fsl --engine
  induction` both exit 0 with `result: "proved"` without it, and `2_requirements`
  checks a real `deadlock` property while doing so. The pages sat directly under a
  callout telling the reader not to weaken business intent to make a check pass.
  This covers the site pages only -- the same suppression survives in
  `examples/e2e/README.md` and twelve other `examples/` documents, tracked in #979
  and #998.
- Replaced (#688): site CLI reference pages (`docs/intro/cli.{en,ja}.html`) now
  render from the native `rust/fslc/cli-contract.json` contract instead of
  frozen Python reference argparse introspection (`src/fslc/cli.py`), so the
  public site stops describing the native CLI from a surface that is not its
  authority. The move surfaces `fslc testplan`, `fslc counterexample`, and
  `fslc mutate --oracle-attribution`, and drops no command: 29 of the
  contract's 30 prog entries render on the page, `fslc version` staying
  deliberately excluded as before.
- Required (#761): `tools/check_ci_validator_inventory.py` now discovers and requires classification for
  `tools/check_rust_*.py` in addition to `tests/test_*.py`, reserved as future scope by slice 1
  (`docs/DESIGN-ci-validator-inventory.md`, "Scope boundaries"). A new, unclassified harness now fails
  `check` closed with `untracked validator module`, exactly as an unclassified `tests/test_*.py` module
  already did -- the shape #761 itself demonstrated is otherwise possible (17 harnesses existed with no
  record of why any were unwired). Three new `exempt_reason` values (plus the existing
  `frozen-python-compatibility`, reused) distinguish the actual reasons found in issue #761's
  classification table instead of collapsing them into one:
  `manual-developer-run` (5 self-declared "Optional developer-run" harnesses), `frozen-python-compatibility`
  (reused; the 8 F1-F8 parity harnesses, whose precise pipeline stage remains `docs/RUST-PORTING.md`'s
  record, not this inventory's), `parked-pending-unrelated-work` (1 harness blocked on an unrelated,
  currently-parked feature), and `pending-native-migration` (3 harnesses blocked on a tracked,
  not-yet-complete migration). This tool establishes only that a classification was recorded, not that
  it is correct; `docs/DESIGN-ci-validator-inventory.md` states that guarantee boundary explicitly. The
  17 harnesses in the inventory were seeded via explicit `--exempt path:reason` pairs matched to #761's
  own classification table, not via the filename-pattern default (`default_exempt_reason`), which was
  not relied on here. That default is reached both via `--bootstrap` for a genuinely new module and,
  pre-existing and unrelated to this change, via an ordinary `generate` for a module whose `wiring`/
  `prior` tier falls through every more specific branch (for example a previously `required` module
  that is no longer wired anywhere) -- confirmed directly, not merely inferred from the source.

  16 of the 17 were seeded in one `--bootstrap --exempt` call against a clean prior. `full_envelope`
  was classified separately, one commit later in review: an earlier version of this pull request also
  deleted `tools/check_rust_full_envelope.py`, reasoning that its `_diff`/`_normalize` helpers had no
  other consumers after #913; independent review found that answers "is the helper still used," not "is
  the comparison it performs still needed," and that `test-browser.mjs`'s native-vs-WASM parity is a
  different edge that does not observe the frozen Python side either -- so it was restored, classified
  `frozen-python-compatibility` like the other seven F-numbered harnesses, and its retirement decision
  moved to a dedicated follow-up issue (#988) instead of being bundled with this change.
- Required (#810): native/Worker error-envelope comparisons now cover AI-project and
  surface parsing, guard, and name failures with calibrated one-sided rejecting controls.
- Required (#841): calibrated nested-Option cross-engine, diagnostic-location,
  and fault-operator controls, including exact nested-state comparator detection.
- Required (#922): serialize Codex Cargo launches across repository worktrees, and use shared snapshot and SPDX enforcement detectors in agent hooks and merge readiness.
- Required (#932): `explain` and `html` each carry a FIFO control that fails when the CLI opens a second root source, plus a platform-neutral control without `#[cfg(unix)]` so Windows holds the same guarantee; the HTML parity comparison's two wall-clock exclusions are themselves asserted present in both rendered reports, so the exclusion cannot go dead unnoticed.
- Required (#937): Worker `check` and pre-solver `verify` error routes now have
  full native-envelope comparisons, an explicit route registry, and calibrated
  detectors for requirement traces, build failures, boundary violations, and
  implements finalization.
- Required (#962): add a machine-generated CI validator inventory and
  reachability metatest so new `tests/test_*.py` modules fail closed unless they
  are wired into a required gate or explicitly classified as exempt; slice 1
  records the existing 98 unwired modules without wiring them.
- Required (#1007): the `site reference freshness` context now also enforces
  static manual-route integrity (`tests/test_site_manual_integrity.py`) and the
  sitewide refresh contract (`tests/test_site_refresh_contract.py`) alongside the
  existing generated-reference freshness snapshots. The context keeps its name,
  ruleset membership, job, triggers, permissions, timeout, and concurrency, and
  still runs on every pull request with no path filter; `fetch-depth: 0` was added
  so the pinned-blob check can read local Git history. None of the three checks
  establishes Rust/solver behavior, native CLI parity, browser rendering, or
  assistive-technology behavior -- they are bounded documentation-artifact checks.
- Unified (#932): `ledger` and `analyze` now derive their root-level reports from one captured input snapshot instead of multiple independent root-path reads.
- Documented (#643): conformance-test layer-selection norm — generate from the
  implementation's layer granularity; reuse upper-layer `forbidden` negatives only,
  not upper positive scenarios (forward-simulation rationale and
  `examples/refinement_chain` example in `docs/LANGUAGE.md` §12 and
  `skills/fsl/references/impl.md` §9).
- Documented (#841): accepted the implementation design for full recursive nested `Option` support, including fail-closed type boundaries, lossless JSON/replay, and cross-engine agreement.
- Documented (#915): a release-gate failure-classification rule that distinguishes procedure defects, environment differences, cancellations, and reproducible defects before retrying.
- Documented (#920): release promotion pull requests now use a branch pinned to the approved candidate SHA, with tree-identity checks before and after merge and an explicit carry-forward for the v4.4.1 revert.
- Documented (#925): recorded the nested Map state-value removal as a breaking change in `docs/DESIGN-nested-option-support.md`, corrected its coupled-surface inventory to the fragments and surfaces that exist, and added the contributor rule that a type-boundary gate change measures `check` verdicts over `specs/`, `examples/`, and `rust/fslc/tests/fixtures/` at the named base and head SHAs, plus the inline fixtures of any required test the change rewrites, renames, removes, or stops from running.
- Documented (#926): recorded the 2026-09-04 re-measurement of the Actions cache budget audit
  (9.07 GiB / 91%, above the 85% threshold) in `docs/DESIGN-ci.md`, "Actions cache budget" --
  the issue's originally-reported generation coexistence (`semantic-mutation`/`rust-workspace`/`wasm`)
  no longer reproduces, but the same mechanism now affects `rust-native-z3` Darwin/arm64 instead,
  driven by the GitHub-hosted runner's ambient installed-toolchain list rather than by anything this
  repository controls. Recorded why a coexistence-detecting audit rule was rejected (its easiest
  passing implementation is the prohibited manual deletion), why `shared-key` does not apply (only one
  job builds this cache), and that the cache's size reflects the vendored Z3 C++ build rather than
  accumulated debt.
- Documented (#929): Codex hook activation now records machine-local entry trust,
  the resulting enforcement boundary, and a non-failing local diagnostic for
  untrusted checked-in hooks.
- Documented (#961): `docs/DESIGN-no-user-invariants-warning.md` records the
  suppression contract that supersedes the `CHANGELOG.md` v1.5.0 note on this
  warning.
- Documented (#990): align `docs/DESIGN-assurance-classes.md` classifier rules
  with the keys and values `assurance.py` / `ledger.rs` actually accept; record
  that FSL core classifies external evidence for display only and does not verify
  it (confirmation rules remain issue #994).

## [4.4.1] - 2026-08-26

- Fixed (#761): the legacy replay trace object wrapper is now exactly `{"events": [...]}` —
  a misspelled key (for example `eventz`), a non-array `events` value, or extra keys are
  rejected with the wrapper diagnostic instead of being silently tolerated. Focused native
  accepting and rejecting contracts pin the wrapper and its full public projection, and a
  new liveness-witness replay matrix rejects isolated state, action, and loop corruptions
  of every `leadsTo` lasso case the retired Python comparison covered.
- Fixed (#786): the frozen Python compatibility reference now lets an explicit
  inline `implements` action correspondence override the same requirements-process
  auto map, so the manual dialect-conformance corpus no longer rejects a valid
  arity-changing refinement mapping.
- Fixed (#826): unresolved indexed `init` writes now conservatively collide with every concrete key on the same logical map root instead of silently accepting aliasing writes with equal values.
- Fixed (#832): native `fslc check` now fails closed for compose alias and typed
  binder resolution gaps that `verify` already rejected. A declared alias no
  longer authorizes a nonexistent qualified type such as `core.NoSuchType`, and
  an undeclared alias-shaped expression in an `init if` condition receives the
  same type check as an assignment right-hand side. Unknown typed binder names
  are also rejected uniformly in properties, action postconditions, and init.
  Errors retain the author's spelling and source location; valid imported types,
  binder types, and state conditions remain accepted.
- Fixed (#904): legacy solver-free BFS now excludes intended `terminal` states from deadlock reporting while preserving true deadlocks, matching explicit BFS and symbolic BMC.
- Required (#742): merge readiness now rejects Git-tracked citations to missing
  `docs/DESIGN-*.md` sections, including colon and explicit-section forms, without
  letting untracked documents satisfy them; calibrated folded, list-tail, historical,
  and accepting controls preserve the boundary.
- Required (#746): the five frozen-Python and DESIGN-index coupled-change metatests now run in the required pre-merge automation lane while remaining outside the Rust-native product gate.
- Required (#761): native Rust tests now bind every corpus `verify` result class to
  its exact public process exit code; pin the business, requirements, and governance
  induction CLI contracts; exercise all nine leadsTo case-by-corruption replay cells
  with exact diagnostics; compare the complete legacy object-wrapper envelopes
  while rejecting wrong keys, value types, and extra root keys; compare the
  complete stable `refine` envelope projection with a live, reasoned exclusion for
  solver-selected implementation traces; and own all sixteen stable induction
  envelope/exit cases with live exclusions and negative controls before the Python
  parity harnesses are retired.
- Required (#898): Merge readiness now runs ShellCheck with masked-return analysis and fail-closed tracked-script enumeration, plus a calibrated direct-syntax Bash-version guard lint that scans expansions in unquoted heredocs, ignores literal heredoc data, and documents its dynamic-execution boundary.
- Required (#761): the sixteen induction CLI cases the retired Python parity harness compared
  are now owned by a native golden contract (`induction_cli_contract.rs`) — full stable
  envelopes with reason-coded, live-hit-checked exclusions, a calibrated cache-mode control,
  and an idempotent golden regeneration path.
- Required (#761): refinement corpus parity now compares the full stable projection of every
  corpus envelope against a checked-in golden with reason-coded exclusions and a
  dead-exclusion check, replacing the result/kind/exit-only assertion the retired Python
  harness was stronger than.
- Documented (#757): recorded the first natural production partial re-run — a GitHub-side
  artifact-service transient failed one test shard on a docs-only PR, `gh run rerun --failed`
  re-ran only that shard, and the aggregator accepted the mixed-attempt cohort
  (`attempts=1,1,2`), recovering in one shard's time instead of a full three-shard re-run.
- Documented (#761): recorded all 17 Python/Rust parity-harness dispositions and deletion preconditions without deleting a harness, including seven native-owner gaps, the full-envelope helper dependency, the parked Phase-3 comparison, three native-migration candidates, and five bounded manual controls.
- Documented (#904): accepted the #761 native BFS/BMC migration plan, including the
  checked 20-model decision matrix, bidirectional witness validation, terminal-aware
  deadlock repair, rejecting controls, and safe Python-harness and helper-binary
  retirement order.
- Required (#906): the induction contract golden is now part of the release-commit
  regeneration set. It records each envelope's `versions` block, so a version bump made the
  complete product gate fail comparing the previous version against the new one; `docs/RELEASE.md`
  step 6 now names its `UPDATE_INDUCTION_CLI_CONTRACT` regeneration and
  `is_release_bump_path` names the golden, keeping the release commit exempt from a
  fragment for that path.

## [4.4.0] - 2026-08-25

- Changed (#760): native CLI tests now cover deterministic DOT and Mermaid
  analysis export, DOT escaping from valid FSL input, and rejection of non-graph
  export requests.
- Fixed (#617): Normalize Windows source-path separators in the optional-verdict census so classifications and consumer checks remain live across platforms.
- Fixed (#741): `tools/build_site_reference.py` now rejects a `docs/LANGUAGE.ja.md` that
  reorders `## ` sections relative to `docs/LANGUAGE.md` while keeping the section count equal.
  `render_language_tree()` previously compared only `len(ja_sections) == len(en_sections)` and
  then paired the two files positionally, so a same-count reorder passed generation silently and
  attached a Japanese section body to the wrong English anchor and blurb — exactly the drift
  `docs/DESIGN-docs-site.md` D7 and this tool's own docstring already claimed was caught. The
  fix checks each positional pair's leading numeric section prefix (`"2"` from `"2. Types"`/
  `"2. 型"`) and raises `SystemExit` naming both headings and the position on a mismatch, needing
  no new maintained ja/en heading-name table. That per-position check is only sound if
  `docs/LANGUAGE.md`'s own numbers are unique, so a second assertion now rejects a duplicated
  English section number directly, naming the duplicate, instead of leaving uniqueness an
  unstated precondition. Rejecting controls (a same-count reordering fixture and a duplicate-number
  fixture) and an accepting control (the real, untouched files) are all in
  `tests/test_site_reference_snapshot.py`.
- Fixed (#757): sharded Rust and semantic-mutation artifacts now use stable logical names so a failed
  shard can be rerun without discarding successful peers. Provenance, checksums, shard identity, and
  exact-union controls reject incomplete or incompatible mixed-attempt cohorts.
- Fixed (#762): restore the native root CLI-help contract formatting while retaining concise summaries for every top-level command.
- Fixed (#786): Prove the native-only `enum abstraction` refinement exclusion by neutralizing only its registered block and reject stale or shadowed registry entries.
- Fixed (#795): failed trusted Actions cache-budget audits now maintain one deduplicated issue and close it after recovery.
- Fixed (#818): a compose document with an undeclared component alias inside a
  top-level `init` block's `forall` binder no longer panics past the public
  API boundary. `rewrite_compose_statements` now returns `Result` and its
  caller in `lower_compose` propagates with `?`, so `fsl_core::parse_kernel_source`
  returns `Err(CoreError)` with the "unknown alias" message instead of the
  process aborting. A rejecting control exercises the reproduction through
  `parse_kernel_source`; an accepting control confirms a correctly declared
  alias used in a `forall` binder still lowers successfully.
- Fixed (#821): the explicit-engine init duplicate-write check now rejects a `forall i { m[i]
  = ... }` write that overlaps a later flat `m[K] = ...` write to the same concrete key `K`,
  whether or not the two values agree. It previously tracked forall-indexed writes at
  whole-variable granularity while flat writes were tracked per key, so an overlapping flat
  write with an agreeing value was silently accepted and one with a conflicting value fell
  through to the unrelated "init constraints are unsatisfiable" diagnostic instead of the
  duplicate-write rule's own message.
- Fixed (#822): Offer `use` in LSP completion and enforce reasoned parity with rename-validation keywords.
- Fixed (#840): Cache-budget audit wiring validation now reports malformed sibling workflows without a traceback.
- Fixed (#843): `fslc testgen` no longer bakes a Monitor rollback as a conformance expectation. The
  fixed-seed walk is a concrete Monitor run capped at 100 steps and independent of
  `--depth`, so it could reach a violation the bounded verification `testgen` runs
  first proved absent within `depth`; the violating `StepResult` was discarded and
  the rolled-back (unchanged) state was recorded as that step's `expected`. A
  conforming implementation failed the generated test, an implementation that
  silently did nothing passed it, and `--target pytest` -- which drives the walk live
  -- reached the opposite conclusion from the five baked targets on the same input.
  The walk now fails closed, reporting the violation with the same
  `result:"violated"` envelope, exit code, property, step, and replayable trace
  `verify` reports, and writing no harness.
- Fixed (#846): the Rust workspace is clean again under `clippy` 1.98.0, whose
  new deny-by-default `manual_is_variant_and` and `chunks_exact_to_as_chunks`
  lints turned `main` red with no repository change, because every CI job uses
  the unpinned `dtolnay/rust-toolchain@stable`. Both rewrites are
  semantics-preserving. CI named only two sites, since cargo aborts at the first
  failing crate; enumerating with `--keep-going` found four, and the fix was
  also checked against the pinned 1.88.0 MSRV that `release.yml` uses so
  `as_chunks` cannot break the release build after PR CI passes on stable.
- Fixed (#847): Scheduled and manually dispatched product-gate failures now reconcile through the post-merge CI reporter.
- Fixed (#851): Cache-budget audit workflow-name validation now rejects case-only,
  surrounding-whitespace, and compatibility-Unicode collisions with the reporter source.
- Fixed (#855): map-key diagnostics now explain when a `const` shadows a member of
  the map key enum and advise renaming one of the colliding declarations.
- Fixed (#856): Replaced workflow line scanners with required PyYAML-backed audits that preserve source diagnostics, reject ambiguous YAML, and pin the privileged post-merge reporter checkout and execution shape.
- Fixed (#857): `rust tests` and `rust checks` have cold-build margin. A cold
  `rust tests` shard was measured at 30m04s against its 30-minute budget and was
  cancelled, turning the aggregate `rust workspace` required context red;
  `docs/DESIGN-ci.md` had recorded that outcome as an unobserved risk. The budgets
  move to 45 and 30 minutes, sized from measured cold durations (24m59s / 27m12s /
  30m04s for the shards, 16m35s for `rust checks`) against warm baselines of
  9-11m and 2m. #747 records why cold builds recur: two concurrent pull requests
  exceed the 10 GiB Actions cache ceiling and evict each other, so a budget with
  no cold margin turns a routine eviction into a failed required context.
- Fixed (#858): the public types that carry a verification outcome are now `#[must_use]` on the
  type rather than only on the functions returning them. `Result` is already
  `#[must_use]`, so `monitor.step(x).map_err(..)?;` satisfied the compiler while the
  inner `StepResult` -- which owns `violation` -- was dropped as a statement value;
  annotating the function is redundant against `Result` and closed nothing. With the
  attribute on the type, `cargo clippy -- -D warnings` rejects the discard **where
  the outcome is returned directly**, which is the shape behind the `testgen` walk
  bug. It does not reach an outcome wrapped in `Option`: `#[must_use]` does not
  propagate through `Option`, and `Option` is not itself `#[must_use]`, so a
  `-> Result<Option<Outcome>, E>` signature stays undetectable by this mechanism
  (issue #868).
- Fixed (#860): recognize integer constants as distinct concrete keys when checking `init` coverage for integer-keyed maps.
- Fixed (#861): make the release Linux linkage guard reject binaries dynamically linked to libz3, with accepting and rejecting ldd fixtures. The separate GLIBC behavioral-control finding is tracked in #865.
- Fixed (#862): `Map<Int, V>` is rejected at check time in every grammar-reachable type position, preventing verification from materializing an unbounded key domain.
- Fixed (#868): Detect discarded optional verification verdicts through direct and source-declared alias return types, and assert expected bounded-liveness observations.
- Fixed (#871): Corrected the release-linkage calibration to name both conditions
  behind the former hole: `!` exempted the failing pipeline from `errexit`, and
  the final clean iteration then supplied the loop's status.
- Fixed (#873): generated cargo-mutants liveness exclusions now derive their
  line-scoped expressions from the maintained source anchors instead of requiring
  hand-copied line numbers.
- Fixed (#875): causal expectation replay now reports a `within 0` liveness
  violation found in the initial state instead of returning a false `pass`.
- Fixed (#882): the generated cargo-mutants configuration's freshness check no longer fails unconditionally on a CRLF checkout. `mutants.toml` and its template are pinned to LF via `.gitattributes` and the comparison normalises line endings, so the gate's verdict does not depend on checkout configuration. Found by the promotion-only Windows solver lane after #880 merged, because that lane is skipped on pull requests.
- Fixed (#883): text checkouts are pinned to LF and #858's source-inspection control normalises CRLF reads, so its adjacent-line assertions do not depend on checkout configuration.
- Required (#802): cache-budget audit workflow wiring now has parsed-YAML calibration controls for every validator rejection and diagnostic CLI input failures.
- Required (#812): `mutate` now preserves malformed causal parser diagnostics through an executable error-envelope parity control.
- Required (#845): cache-budget audit workflow-name uniqueness now blocks main
  merges through merge readiness.
- Required (#850): Native CLI regression coverage now detects incorrect failed-link
  identities and composed action maps in multi-link `fslc refine` chains.
- Required (#865): Added accepting and rejecting fixture controls for the Linux
  release GLIBC ABI guard. The required product gate now exercises the deployed
  guard with compliant GLIBC_2.39 and rejecting GLIBC_2.40 `readelf` fixtures.
- Required (#879): `AGENTS.md` now requires four pieces of control evidence beyond a negative control's existence: each control labelled as a detector or a preservation control and the label proven by executing the mutation, one mutation at a time with the revert proven by count; a full-output comparison whose every exclusion carries a reason it cannot be compared, is built from observed output rather than a type's field names, and fails when an excluded key is absent from both sides; ambient observables checked for membership rather than equality, with a new or changed control run at least twice before being reported green; and the commit, built binary, and any applied mutation named before an observation about repository state is reported.
- Unified (#819): the two-inode FIFO read-count oracle used by all six #796/#808
  CLI-level snapshot controls now lives once, in
  `rust/fslc/tests/support/fifo_snapshot.rs`, carrying the cleanup hardening
  `issue_796_domain_command_validation.rs`'s independent copy grew during its
  own review: nonblocking cleanup reader descriptors, a
  `WriterOutcome`/`WriterMode`-driven writer thread, and a `ReapedChild` guard
  that reaps unconditionally even when `kill` itself fails. The previously
  unhardened shared copy could hang instead of fail when the CLI under test
  exited before ever opening the FIFO path -- the exact incident during #813's
  development that cancelled a CI job after 30 minutes and hung a local run for
  1h33m. A new negative control in `fifo_snapshot_hardening.rs`,
  `release_writer_completes_when_cli_never_opens_the_path`, reproduces that
  scenario directly and proves cleanup now terminates instead of hanging.
- Documented (#746): Index every accepted design record in docs/README.md.
- Documented (#821): corrected `docs/DESIGN-bridge.md` §1.3's init-determinism
  bullet, whose limiting clause ("... rather than a `forall`-bound variable")
  described the pre-fix explicit-engine behavior instead of the per-concrete-key
  contract `docs/LANGUAGE.md` already stated. The bullet now says a later flat
  `m[K] = ...` write to a key a `forall` bulk assignment already covered is a
  duplicate too, matching the corrected runtime check.
- Documented (#827): the root `README.md` rebuilt around the native Rust
  workspace (`rust/`) as the authoritative implementation, replacing the stale
  "Lark + Z3" / `src/fslc/` product framing, a 47-line directory tree with no
  `rust/` entry, a `pytest` / "Library API" contract that `AGENTS.md` no longer
  treats as the product gate, and three overlapping, self-contradicting install
  paths. In their place: one consolidated install section with a chooser, a
  `rust/` crate layout, the native CLI surface that was missing (`kernel`,
  `document`, `ledger`, multi-target `testgen`, `replay --from-log/--mapping`,
  `--engine explicit|auto`, `analyze` projections), editor integration, and the
  Public Kernel JSON contract as the supported programmatic surface in place of
  a Python library import. The `specs/`/`examples/` per-file inventories are
  deliberately not restored -- a hand-maintained list is what went stale.

## [4.3.0] - 2026-08-18

- Fixed (#753): `git apply` silently skips every file in a patch, and exits zero, when the
  fault-operator scratch checkout is not its own git repository root -- git resolves the
  scratch to the enclosing repository, where everything under it lives beneath `rust/target/`
  and is git-ignored (`Skipped patch '<path>'.`, exit 0). The scratch then compiled
  **unfaulted**, every detector passed because there was nothing to detect, and the harness
  recorded that as a detector gap. `sync_scratch` guarded this with `[ -e "$scratch/.git" ]`,
  which tests the wrong property: an empty or partial `.git` from a restored CI cache
  satisfies it and suppresses the repair, which is why the failure appeared only in CI and
  varied run to run. The guard now requires `git rev-parse --show-toplevel` inside the scratch
  to equal the scratch, and `git apply --verbose` turns a `Skipped patch` line into a nonzero
  status.
- Fixed (#753): the implementation fault-operator harness now **witnesses** that a fault
  reached what it measured instead of inferring it. A `primary still passed under the
  fault` verdict has two causes with different owners -- the detector does not cover the
  seam (a real gap) or the detector never saw the fault (a harness defect) -- and
  `tools/run-fault-operators.sh` previously reported the first whenever the second was
  true, because it treated a zero-exit `git apply` plus a clean build as proof of arrival.
  The symptom was the same operator returning different verdicts on different runs of one
  revision, which made unrelated pull requests unmergeable through the `semantic mutation`
  required context. Two fail-closed witnesses now run before any verdict: every file a
  patch names must differ byte-for-byte from the pristine copy after the patch applies,
  and, of the two artifacts a detector can execute -- the test harness binary, read back
  from cargo's own `Executable <target> (<path>)` line, and the `fslc` executable a
  detector may spawn via `env!("CARGO_BIN_EXE_fslc")` -- at least one must differ from the
  digest recorded for it under the no-op control. Both are checked because an operator's
  fault normally reaches exactly one: a patch under `rust/fslc/tests/**` changes the test
  binary, a patch under `rust/fslc/src/**` changes `fslc`. A byte-identical pair cannot
  arise from compilation nondeterminism, so the binary witness fires only on real artifact
  reuse.
  The source witness carries its own negative control,
  `rust/fslc/tests/fault_operators/controls/identical-after-apply.patch`, a hunk that
  applies cleanly while leaving the file unchanged and must be refused; the binary witness
  is calibrated by live mutation and has no fixture, recorded as such in
  `docs/DESIGN-conformance-harness.md`.

- Decided (#737): the shared-edit merge-conflict spike closes as **GO for CHANGELOG
  fragments only (C1)** and **NO-GO for fragmenting the contract documents (C2)**, recorded
  in the new accepted `docs/DESIGN-changelog-fragments.md`. The load-bearing evidence is a
  replay of history at `d72ac8d`: of 22 genuinely concurrent branch pairs (interval overlap
  plus `git merge-base --is-ancestor` false both ways, conflicts detected with
  `git merge-tree --write-tree`), 16 conflict, `CHANGELOG.md` is a conflicting file in 16 of
  16, and the three current-era conflicts have it as the *only* conflicting file — while
  fragmenting `docs/LANGUAGE.md`/`docs/LANGUAGE.ja.md`/`skills/fsl/reference.md` resolves
  zero additional pairs (every contract-document conflict co-occurs with an implementation
  conflict in one early-Python cluster), which is #737's own no-go condition, and would put
  a generated layer upstream of the required `site reference freshness` check. The record
  carries the six fail-closed negative controls (missing or empty fragment, duplicate id,
  order determinism plus conformance on **both** the category and id sort components,
  unaggregated-at-release plus bidirectional direct-edit, aggregation conservation against
  silent entry drops **and against truncation**, and fragment-name conformance), each with a
  calibrated rejecting fixture and named diagnostic, the six migration sites, the rollback
  method, and the reversal condition. `<category>` follows **this repository's** bullet
  lead-word convention, not Keep a Changelog's: the measured `[Unreleased]` body has zero
  `### ` subheadings and already uses `Documented` and `Decided`, so the aggregator emits
  bullets rather than introducing subheadings. Implementation is deliberately not included. The `LANGUAGE.md` /
  `LANGUAGE.ja.md` section-alignment enforcement is untouched by C1; a pre-existing
  count-only gap in that enforcement found during the evaluation is tracked as #741.
- Documented (#738): the command-family extraction spike's result as re-evaluation evidence in
  `docs/DESIGN-rust-components.md`. Section 10's first trigger — three unrelated command changes
  failing on `fslc` orchestration coupling — has fired (the exit-code chain alone supplies five)
  and is adjudicated: the harmful coupling is one contract policy implemented at more than one
  site, which a vertical command-family split cannot remove, so further family extraction is
  no-go and a new crate is rejected for lack of independent version/dependency/consumer evidence
  (C1 remains the selected candidate). The one executed extraction (`causal.rs`, #393 / PR #440)
  produced zero confirming samples and two counter-shaped ones in 13 days — both workspace-wide
  refine changes, so the falsifier section 8 already named appeared in shape while the effect
  stays unconfirmed, not disproven; the response that measurably worked is policy single-owner
  extraction (`spec_load.rs`, `outcome.rs`, `literate_access.rs`): after `743bc7a` gave spec-load
  classification one owner, `main.rs` involvement fell 379 → 179 → 16 lines across its follow-up
  series. Section 8 records the discriminator the spike leaves behind, and it is stated in terms of
  **implementation sites, not call sites** — one contract policy implemented at more than one site is
  harmful coupling; an arm that only calls into a single library owner is cohesion, however many arms
  call it. That distinction is the whole rule: counting call sites makes every centralized policy look
  coupled, because centralizing is what produces callers, and counting only recurrence misses a policy
  whose arms were mismatched from birth, does not capture Case C, and returns both verdicts on a
  history where a duplicated policy was later centralized. Touch rate and line count discriminate
  nothing (post-extraction `main.rs` churn median 29 lines, 26 of 50 commits at wiring scale). The
  negative control is **`c455e50` (#697)**, which the rule rejects: it touches `main.rs`, spans three
  crates and changes concrete-verification behaviour, yet its policy — `CONCRETE_PROBE_BUDGET`
  governing the single `find_boundary_violation` — is defined once and merely referenced from four
  call sites, with the budget value appearing nowhere else in `rust/` outside a test comment, an
  11-line wiring change in `main.rs`, and no later fix landing on a second arm. `64bfb55` is
  explicitly **not** the control: two of its three defects are multi-site policies, so by this rule it
  contains coupling — the rule working, not failing — and two claims made for it are withdrawn, the
  single-file pickaxe over a symbol the broken copy never contained, and the false assertion that it
  deleted a duplicated hint string (`git show 64bfb55^:rust/fslc/src/main.rs` has no `coverage_hint`
  occurrence). Also recorded: the accepting/rejecting controls a future extraction would have to
  calibrate, and the measurable changed-owner rollback rule any future extraction PR must carry.
  Documentation only; no Rust behavior changed.
- Fixed (#720 Finding 1): `rust-tests`' `cargo-nextest --partition count:K/3` balanced pre-merge
  shards by test count, not wall clock, so five binaries holding ~77% of the suite's sequential time
  (`refine_corpus_parity`, `explicit_engine`, `injection_detector_matrix`, `corpus_check_sweep`,
  `issue_226_auto_engine`) could land unevenly across shards — measured spreads of 2.2x and 3.1x
  between the fastest and slowest shard on two runs of the same commit, now both
  recorded in `docs/DESIGN-ci.md` (previously only the 2.2x run was). `check_rust_tests` in `tools/check-native-integration.sh` now
  pins those five binaries to specific shards via a checked-in
  `tools/rust-test-shard-groups.txt` and `cargo nextest`'s `binary_id(=…)` filterset, unpartitioned,
  while every other test still goes through the original count-partition, scoped to exclude every
  pinned binary. Coverage cannot silently drop: an unlisted binary simply falls into the
  count-partitioned leftover as before, and `tools/check-shard-union.sh`'s existing full.txt/shard.txt
  guard needed no shape change to keep validating the result. Added `check-shard-union.sh
  check-groups`, a narrower guard that fails closed if the grouping file pins a binary-id the live
  workspace no longer has or pins one binary to two shards, plus new accepting/rejecting `selftest`
  cases (including a whole-binary-dropped fixture) wired into the existing `merge readiness /
  automation contracts` lane. `.github/workflows/ci.yml` is unchanged — the required `rust workspace`
  context, its `if: always()` aggregator, and the shard-union contract all keep their exact shape.
  **Measured, and the second form delivers**: the slowest shard — the only quantity
  `rust workspace` waits on — fell from **15.6 min to 12.2 min** and the spread from 3.1x to 1.17x
  (warm-cache runs 31076668077 and 31081427765 attempt 2; shard wall clocks 12.2 / 10.85 / 10.46 min,
  clean shard union on both). The first form delivered ~0.1 min and this entry previously said so;
  two measured defects explain the gap. The cost model was wrong twice: packing by each binary's
  sequential total, then by its slowest single test, which underestimates a shard holding several
  long binaries by 52%. The model that fits all three shards is
  `1.11 × max(slowest single test, sequential sum / 3)` — error −0.2% / +12.7% / +0.1%, the outlier
  being the shard holding `issue_697_all_properties_memory`, which is memory-bound by construction
  (`CONCRETE_PROBE_BUDGET`, #697) and so overlaps less than the model assumes. Adding a pin is
  therefore **not** free. And the pinning file was stale before the first form merged:
  `issue_697_all_properties_memory`, whose 371.8s test is the workspace's second slowest, arrived with
  the #739 merge and was unpinned, putting that test in shard 2's count partition — which is what took
  shard 2 from 5.0 to 14.6 min. The assignment now pins eight binaries by
  `max(slowest, sum/3)`, which also cut the duration-blind leftover's spread from 5.3x to **1.75x**
  (phases 75.4s / 43.0s / 63.9s). Remaining floor ≈10.4 min: `refine_corpus_parity`'s indivisible
  458.8s test plus a measured ≈113s of fixed cost per shard, so going materially lower needs that test
  split or the two phases run concurrently rather than serially. Every comparison here is warm-cache;
  an eviction-induced cold build adds 6–12 min per shard and is tracked as #747.
- Fixed (#747): two concurrent pull requests exhausted the repository's 10 GiB Actions cache
  allowance and evicted `main`'s Rust build caches, after which every run built cold. Actions caches
  are ref-scoped — a pull request's cache is readable only by that same pull request — while
  `ci.yml`'s four shared keys store about 6.9 GiB per ref (`semantic-mutation` 2.72 GiB,
  `rust-workspace` 1.50, `fsl-logic` 1.37, `wasm` 1.35), so two pull requests exceed the limit on
  their own. Measured on 2026-08-06: usage 9.96 GiB across 12 entries, every large cache on a
  `refs/pull/*` ref, and `refs/heads/main` holding only 26 MiB of tool binaries — while a `main` push
  four hours earlier had restored a cache in 25–50 s, so one had existed and been evicted. The cost
  is measured too: two runs of the same commit on the same branch gave 27.88 / 20.81 / 20.61 min cold
  against 12.2 / 10.85 / 10.46 warm, i.e. **+8 to +16 min per shard**, independently on each of `rust
  workspace`'s three shards, `WASM`, `FSL Logic Test` and `semantic mutation` — and each cold run
  saved a fresh ref-scoped copy, evicting more. Every `Swatinem/rust-cache` step in `ci.yml` now
  carries `save-if: ${{ github.event_name != 'pull_request' }}`; pull requests still restore, because
  `main` is the default branch and readable from every ref. `merge-readiness.yml` is deliberately
  unchanged — its two keys total ~131 MiB and its lanes are the sub-minute fast path. New
  `.github/scripts/audit-cache-budget.mjs` fails closed on usage at or above 85% of the limit, on a
  missing `refs/heads/main` cache for a critical-path shared key, and — the rejecting control for the
  `save-if` guard itself — on any pull-request-scoped cache for one of `ci.yml`'s shared keys, which
  can only appear if that guard is removed; an unreadable listing or absent usage total also fail
  closed rather than reading as headroom. `.github/workflows/cache-budget-audit.yml` runs it on a
  schedule, on dispatch, and on `main` pushes touching it or `ci.yml`; it is not a required context
  because the shared cache state can change after a pull request's checks pass. The 11-case
  calibration suite, including a fixture reproducing the 2026-08-06 listing verbatim, is wired into
  `tools/check-merge-readiness.sh`'s `check_automation` lane. No job, trigger, or required-context
  name changed: the parsed `ci.yml` differs from `main` only inside its cache steps. This also
  qualifies this repository's earlier finding that cache hit rates have no headroom — true **on a
  warm cache**, which is the premise concurrency breaks. #720's Finding 2 adds a cache and therefore
  depends on this budget holding first.
  Eviction started the problem; `cache-on-failure: false` made it unrecoverable. The
  `semantic mutation` lane fell into a closed loop: a cold scratch build exceeds the job budget, the
  job is cancelled, `rust-cache` skips saving from a failed job, and the next run is cold again — so
  the cache could only be created by a run that succeeds while a run could only succeed once the cache
  existed. Both semantic-mutation cache steps now carry `cache-on-failure: true`, and both budgets are
  raised past a measured cold run: `mutation operators` 30 → 50 min (warm 18.0–19.5, cancelled at 30.2
  cold) and `mutation mutants` 60 → 90 min (warm 17.2–34.2, cancelled at ~61 cold). Raised rather than
  narrowed, for the reason this repository already gives for the promotion-only native-Z3 job: a gate
  that runs out of wall clock reports a failure it did not observe. This is the most likely explanation
  for `main`'s standing #721 and #678, whose cancellations sit exactly at the old budgets; whether they
  clear once this lands is the test of that reading.
- Fixed (#736): `.github/workflows/ci.yml` cited a `docs/DESIGN-ci.md` section,
  `"Merge queue (planned, not yet enabled)"`, that does not exist and asserted the
  opposite of the accepted decision — the design document records that a merge
  queue was configured on the `main` ruleset on 2026-08-05 and rejected the same
  day (`### The merge queue was tried, measured against this repository's
  workflow, and rejected`), not merely planned. The `merge_group:` trigger
  comment's "no merge queue exists on `main` **yet**" carried the same stale
  implication. Both comments, plus the `pull_request:` trigger comment's
  reference to the same dangling section, now cite the real heading
  (`## Required pre-merge contexts, and why the merge queue was rejected` and
  its subsection) and state the rejection accurately. `tools/check-product-gate-scope.sh`
  carried the third instance of the same dangling citation and additionally
  described the inert `queue-entry-stub` branch as "implemented ahead of that
  rollout"; it now records that no rollout is pending and that reviving the queue
  is a human-review-policy question, not a CI one. The same file's
  `FSL_MERGE_QUEUE_CI` branch comment said enabling the variable was "the only
  remaining step", contradicting both the rejection record and the file's own
  header eighty lines above; it now states that reaching that branch needs the
  policy change first, and why enabling the variable alone would land changes
  with no pre-merge Linux evidence. `tools/check-product-gate-scope.sh`'s
  `diff_scope` comment said the in-job check is "the only place the exemption can
  apply **once** a merge queue exists", presupposing one will; it now says
  "if a merge queue ever existed" and states that none does. And
  `docs/DESIGN-ci.md` itself — the authority the other five sites cite —
  recommended a merge queue as a mitigation "worth using ... once `ci.yml` gains
  the same trigger", while its own `## Non-goals` lists running a queue as a
  non-goal; `ci.yml` had already gained that trigger, so the paragraph read as an
  available next step. It is corrected rather than deleted, so the sequence stays
  legible.

  **Seven sites across three files, of two signatures, found over four passes** —
  not one clean sweep. Three quote the removed section name; four state a stale
  premise without naming any section. The section-name sites are findable
  mechanically, but only with a comment-prefix-aware detector: the string wraps
  across lines behind `#` prefixes, so a single-line `git grep` matches one of the
  three. The reliable form is
  `sed 's/^[[:space:]]*#[[:space:]]*//' | tr '\n' ' ' | tr -s ' '`; calibrated
  against the pre-repair tree it finds all three, and over all 1,436 tracked files
  here it leaves only the two `CHANGELOG.md` entries that quote the string as
  history. **The four stale-premise sites are not findable that way at all** — they
  contain no section name — and they include both the most dangerous one (an
  operation the #717 canary proved unsafe, described as one flag away) and the one
  inside the accepted decision record. #742 tracks mechanising the citation check;
  it must also cover this second signature, or the recurrence continues. Audited
  every other `docs/DESIGN-ci.md` citation in
  `ci.yml`, `ruleset-drift-audit.yml`, and `site-reference-freshness.yml`: all
  resolve to real headings. Comment/citation fix only — no trigger, job, or
  required-context name changed; the parsed YAML structure is unchanged before
  and after, and `./tools/check-product-gate-scope.sh selftest` still passes.
- Documented (#722): the implicit domain aggregate initializer enumeration in
  `docs/LANGUAGE.md`, `docs/LANGUAGE.ja.md`, and `skills/fsl/reference.md`
  covered only Bool `false`/enum first-member/range lower-bound/external-
  placeholder `0`, omitting the container defaults `Context::default_for_type`
  (`rust/fsl-core/src/domain.rs`) and its `domain_lowering.rs` counterpart
  already select: `Option<T>` -> `none`, `Set<T>` -> `Set {}`, and a top-level
  `Map<K, V>` -> dense per-key `forall k: K { field[k] = <value default> }`
  (#691 / PR #708). Confirmed empirically with `fslc check` on
  `rust/fslc/tests/fixtures/domain_characterization/container_defaults_surface.fsl`
  and `.../lvalues_surface.fsl` that `implicit_initial_value` does **not**
  fire for these container shapes -- that warning's `omitted_domain_value`
  (`rust/fslc/src/frontend_output.rs`) only ever recognizes the four scalar
  shapes, a real (documented, not fixed) gap between the warning's coverage
  and the renderer's total default dispatch. Also documented the two
  `Map`-specific rejections: an explicit whole-`Map` default ("whole-Map
  domain defaults are not supported") and a `Map` nested as another `Map`'s
  value ("Map state requires explicit initialization through supported
  semantics"). No Rust behavior changed. See `docs/DESIGN-domain.md`.
- Added (#707): a ruleset drift audit, closing the drift-detection half of #707 (the
  required-context half landed separately). `.github/ruleset-contract.json` is a checked-in,
  schema-versioned record of the `main safety and CI` ruleset (id `19090811`): identity,
  enforcement, conditions, rule types, strict/`do_not_enforce_on_create` policy, required
  contexts as `(context, integration_id)` pairs, `bypass_actors`, and two explanatory lists
  (`deferred_contexts` — the `FSL_OPTIMISTIC_CI`-gated cross-platform matrix and aggregate that
  can never require pre-merge without deadlocking; `constituent_contexts` — the sharded lane
  names whose `if: always()` aggregator alone carries the required-context name). The new
  `.github/scripts/audit-ruleset-drift.mjs` fetches the live ruleset and compares it against the
  contract through pure, unit-testable `compareRuleset`/`validateContract` functions, rejecting
  on `ruleset-identity`, `ruleset-missing`, `api-unreadable`, `enforcement`, `conditions`,
  `rule-type-missing`/`-unexpected`, `empty-rules`, `strict-policy`, `enforce-on-create`,
  `required-context-missing`/`-unexpected`/`-duplicated`, `required-contexts-empty`,
  `bypass-actors-unobserved`, `bypass-actor-added`, and `contract-invalid`. Required contexts are
  keyed on `(context, integration_id)`, not context name alone, so a same-named check reported by
  a different GitHub App is treated as impersonation, not satisfaction. `pull_request` rule
  parameters (review counts, `allowed_merge_methods`, …) are deliberately unaudited — human
  review policy is ruleset `19090821`'s separate accepted decision — and the calibration suite's
  blind control proves an edit there stays invisible to this audit. `bypass_actors` is in scope
  and its **absence** (not just an added actor) is a failure: GitHub returns that field only to a
  caller with write access to the ruleset, the workflow's `GITHUB_TOKEN` has no `administration`
  permission at all, and a new `RULESET_AUDIT_TOKEN` fine-grained-PAT secret is required to
  observe it — without the secret the audit still runs and fails closed with
  `bypass-actors-unobserved` rather than treating absence as an empty (safe) list. The raw
  observation is written and digested to the step summary *before* classification. Failure-issue
  lifecycle (`ci/ruleset-drift` label, marker `<!-- ruleset-drift:19090811 -->`, occurrence
  markers, reopen-on-recurrence, close-on-clean-with-recovery-comment) imports and reuses
  `report-post-merge-ci.mjs`'s exported `GitHubRestClient` rather than adding a second REST
  client. `.github/workflows/ruleset-drift-audit.yml` runs on `schedule`, `workflow_dispatch`,
  and a path-filtered `push` to `main`; it deliberately carries no `pull_request` trigger, since a
  fork PR has no access to the elevated token. `.github/scripts/audit-ruleset-drift.test.mjs`
  derives every case from a verbatim capture of the live ruleset
  (`.github/scripts/fixtures/ruleset-19090811.json`) by `structuredClone` plus one mutation, and
  is wired into `tools/check-merge-readiness.sh`'s `check_automation` lane. `site reference
  freshness` joins the `main` ruleset's required contexts, now six instead of five — it reports on
  every pull request with no path filter, so requiring it cannot deadlock a pull request the way
  the deferred cross-platform matrix would. `docs/DESIGN-ci.md` gains a "Ruleset drift audit"
  subsection and corrects two sentences that had gone stale after the five-context requirement
  landed (2026-08-05): "None of the four contexts are required status checks today" is now scoped
  to the time the exemption mechanism moved, and the agent-configuration-exemption's "merges on
  `merge readiness` (plus `site reference freshness`) alone" is restated for the six-required-
  context reality. `docs/DESIGN-docs-site.md`'s D7 addendum and
  `.github/workflows/site-reference-freshness.yml`'s header now say the workflow's own context is
  enforced (not merely visible) and why that is deadlock-safe; the `#688` frozen-`cli.py`
  asymmetry it separately flagged is unchanged and stays open. The live ruleset edit adding
  `site reference freshness` is applied out of band; until then, the audit correctly reports a
  `required-context-missing: site reference freshness` finding — the designed pre-rollout state,
  not a bug.
- Fixed (#713): saga `compensation { when Trigger after After { ... } }` now
  lowers to a kernel action guarded by BOTH the trigger event flag and the
  `after` event flag on both lowering paths (`lower_saga_actions` in
  `rust/fsl-core/src/domain_lowering.rs` and `render_saga_actions` in
  `rust/fsl-core/src/domain.rs`); previously only the trigger flag was
  required, so a compensation could fire on a trace that never observed its
  `after` event. Because generated `event_*` flags are one-hot per
  transition, a compensation whose trigger and after events differ (the
  common shape, e.g. `examples/domain/order_fulfillment_saga.fsl`) is now
  structurally disabled and surfaces as a `fslc verify` never-enabled action
  warning instead of silently accepting the bad trace; this is the accepted
  interim state pending the correlation-indexed saga history follow-up
  (issue #662, `docs/DESIGN-saga-history.md`). No spec migration is required:
  an existing `compensation` block's syntax and semantics are unchanged
  (only its guard strength, previously unsound). See `docs/DESIGN-domain.md`.
- Fixed (#712): top-level `await` blocks (e.g.
  `await PaymentResult { waits_for one_of [...] on X -> Y }`) are now
  rejected fail-closed by both lowering paths through a new
  `validate_lowerable_constructs` gate (`rust/fsl-core/src/domain_lowering.rs`,
  called from both `lower_domain_surface` and `domain_kernel_source`):
  `DomainAwait` was consumed by nothing outside the parser, and cross-
  aggregate routing proofs remain explicit Future Work in
  `docs/DESIGN-domain.md`. **Migration**: remove the top-level
  `await { ... }` block and use a saga step's `awaits` instead; the
  top-level form never had executable meaning under `check`/`verify`, so
  removing it changes no verification outcome.
  `examples/domain/order_async_effect.fsl` had its now-rejected `await
  PaymentResult` block removed as part of this fix (its `projection
  OrderObservedState` block is unaffected and out of this fix's scope). See
  `docs/DESIGN-domain.md`. Tracked mechanism for future migration
  diagnostics: #702, #703.
- Fixed (#711): aggregate `on_stale` policies (e.g.
  `on_stale Approved when ... { emits ApprovalRejected }`) are now rejected
  fail-closed by both lowering paths through the same
  `validate_lowerable_constructs` gate: nothing in `docs/DESIGN-domain.md`
  pins `on_stale` semantics, and the finding it references
  (`late_completion_without_stale_policy`) is itself unimplemented in native
  Rust (#724). **Migration**: remove the `on_stale { ... }` block; it never
  had executable meaning under `check`/`verify`, so removing it changes no
  verification outcome. See `docs/DESIGN-domain.md`. Tracked mechanism for
  future migration diagnostics: #702, #703.
- Fixed (#710): `value_object` invariants (e.g.
  `value_object AuditStamp { ... invariant nonNegative { attempts >= 0 } }`)
  are now rejected fail-closed by both lowering paths through the same
  `validate_lowerable_constructs` gate. No accepted design pins a
  `value_object` instance set, and the frozen Python reference only emits
  direct-state-field instances while skipping `Option<VO>`/`Set<VO>`/
  `Map<_,VO>`/command-input/event-field/nested-VO positions -- adopting that
  partial coverage in the verifier would leave most instances unconstrained
  while an author believes every instance is checked. **Migration**: remove
  the `invariant { ... }` block from any `value_object`; it never had
  executable meaning under `check`/`verify`, so removing it changes no
  verification outcome. `value_object` field defaults without invariants
  remain fully supported. See `docs/DESIGN-domain.md`. Tracked mechanism for
  future migration diagnostics: #702, #703.
- Fixed issue #697: verifying **all** properties at once (no `--property`)
  could blow past 11.4 GB RSS while every property verified fine in seconds
  in isolation, on both `--engine bmc` and `--engine induction`. The cause
  was not the solver — a Z3 session's own `cost.solver.memory_mb` during a
  blowing-up run measured 27.8 MB — it was an **unbudgeted concrete
  (solver-free) BFS** in `fsl-runtime`: `find_boundary_violation`, the
  pre-pass `prepare_bmc` runs to catch a concrete boundary outcome (a
  reachable over-capacity `Seq` successor) the bounded symbolic value cannot
  represent. That pre-pass runs only when no `--property`/
  `--exclude-property _bounds_*` narrows the request, which is exactly the
  asymmetry the issue reported. Its `Monitor` held the whole model by value
  and was cloned per visited BFS node, and its trace was cloned per node
  too, so memory grew with branching factor and path length on top of raw
  state count; a history-recording `Seq` (records push order, so the same
  set of values in a different order is a different value) defeated the
  BFS's own `visited` dedup, making the frontier grow like branching^depth.
  `find_boundary_violation` (`rust/fsl-runtime/src/lib.rs`) now takes the
  model by reference plus a `budget: usize`, returns a `BoundaryProbe`
  (`finding`, `exhausted`, `states_explored`) instead of a bare
  `Option`, terminates at exactly `budget` distinct states, stops cloning
  the model per node (one scratch `Monitor` is re-pointed at each popped
  state instead), and stops cloning the trace per node (a lifted parent-link
  `reconstruct_trace` — shared with, not duplicated from, the explicit
  engine's own pattern — replaces the per-node `Vec<TraceStep>` clone). All
  four production call sites (`rust/fslc/src/verification.rs`'s
  `prepare_bmc`, `rust/fsl-wasm/src/lib.rs`'s identical Worker pre-pass —
  same constant, so native/Worker strategy parity holds by construction —
  and `rust/fslc/src/main.rs`'s weakening-counterfactual and
  `mutation_oracle_for_model` call sites) now pass the new
  `CONCRETE_PROBE_BUDGET` and treat `exhausted && finding.is_none()`
  identically to today's empty result: this pre-pass is an evidence detour,
  not a verdict authority, so exhaustion falls through to the symbolic
  engine, which finds every symbolically representable violation on its
  own, and the one outcome class the pre-pass alone covers still fails
  closed downstream (`rust/fsl-verifier/src/value.rs`'s "model sequence
  length exceeds capacity") rather than passing — confirmed directly with a
  `Seq<Int, 2>` overflow fixture: the default path still reports
  `violated`/`type_bound` with a full concrete trace, while isolating to a
  property that must render the overflowed value still fails closed with
  that exact error, both unchanged by the budget. `CONCRETE_PROBE_BUDGET =
  50_000` is calibrated, not guessed: a full sweep of `specs/` + `examples/`
  at their default `--depth 8` found a maximum `states_explored` of 23,409
  (`examples/named_predicate.fsl`), with every file reaching normal BFS
  closure well short of even a 500,000-state ceiling; 50,000 keeps a >=2.1x
  margin over that observed maximum (>=3x over every other corpus file). On
  the diagnosis's validated reproducer (a label-workflow spec whose
  `audit: Seq<LabelId, 10>` is the load-bearing history-recording
  ingredient), the joint all-properties `--depth 8` run went from
  OOM-killed at an artificial 4 GB cap and still growing (extrapolating past
  the reported 11.4 GB) to completing `verified` in ~8s (release) / ~82s
  (debug) at ~2.2 GB peak RSS on both engines, with the per-property and
  per-engine verdict sets unchanged and a mutated (genuinely violated) copy
  of the same fixture still reporting `violated`. See
  `docs/DESIGN-kernel-contract.md`, "Concrete boundary pre-pass budget", for
  the full measurement and the budget contract, and
  `rust/fslc/tests/issue_697_all_properties_memory.rs` /
  `rust/fsl-runtime/tests/boundary_probe_budget.rs` for the regression
  controls (verdict/attribution agreement, fail-closed preservation, and a
  structural exhaustion assertion independent of any resource cap).

- Sharded the two heaviest pre-merge product-gate jobs to cut PR wall clock
  from a measured 38m15s (`semantic mutation (changed)`, run 30968645971) to a
  measured **20.7 min** (run 30989320577, all lanes green) — 1.85x, ~17.5 min
  saved per pull request — without weakening any check or changing the `main`
  ruleset. The gain is under 2x, not 3x, and `docs/DESIGN-ci.md` records why
  from the same run: `cargo-nextest --partition count:` balances by test count,
  not duration, so the three test shards took 18.1/8.3/12.6 min and
  `rust workspace` waits on its slowest; and sharding the curated operator lane
  three ways bought only ~10% (22.5 min → 20.3 min) because each shard pays a
  fixed cold build in its own scratch checkout. The two remaining levers,
  neither attempted here, are a duration-aware test assignment (~5 min) and
  caching `rust/target/fault-operators` so that scratch build starts warm. `rust workspace` (32m43s, of which ~29 min was `cargo test`
  running 176 test binaries strictly sequentially) is now `rust-checks`
  (formatting, Clippy, doctests, the full build, and the boundary/stack-
  parity controls, run once) plus `rust-tests` (a 3-way `cargo-nextest
  --partition count:K/3` matrix), aggregated by a `rust-workspace` job.
  `semantic mutation (…)` is now `semantic-mutation-operators` (the curated
  fault-operator table, round-robin sharded three ways by
  `tools/run-fault-operators.sh --shard K/N`) plus `semantic-mutation-mutants`
  (the generic cargo-mutants half, kept **complete and unsharded** — at its
  measured ~14.3 min it is already close to the curated lane's post-split
  floor, and cargo-mutants sharding would add real risk for no measured
  gain), aggregated by a `semantic-mutation` job. Doctests move explicitly to
  `rust-checks` because `cargo-nextest` cannot run them at all; silently
  dropping them would have been exactly the "weaken a gate for speed" move
  this repository forbids. Both aggregators keep the exact required-context
  job id and name (`rust-workspace` / `rust workspace`,
  `semantic-mutation` / `semantic mutation (${{ ... }})`) and run
  `if: always()`, because GitHub treats a *skipped* required check as
  satisfied — without `always()` a failing shard could skip the aggregator
  into a false-green required context. Each aggregator also downloads its
  shards' inventories and proves the split is a true partition of the
  unsharded set before trusting it: `rust-workspace` requires the three
  `rust-tests` shards' `full.txt` listings to be byte-identical and their
  `shard.txt`s to union back to exactly one of them; `semantic-mutation`
  requires the three operator shards' manifests to agree on
  `base_revision`/`table_operators` and their `executed_operators` to union
  back to `table_operators` exactly. Both checks reuse a new generic
  `tools/check-shard-union.sh` (subset/pairwise-disjoint/union-equality,
  fail-closed and naming the offending entries), whose `selftest` — an
  accepting 3-way split plus four rejecting cases (uncovered entry,
  duplicated entry, invented entry, empty shard list) — is wired into
  `tools/check-merge-readiness.sh`'s automation lane next to
  `check-product-gate-scope.sh selftest`. `tools/run-semantic-mutation-gate.sh`
  gained `--lane operators|mutants` and `--shard K/N`; with neither flag its
  behavior, order, and output are unchanged from before this change.
  `tools/check-native-integration.sh` gained `rust-checks` and
  `rust-tests K/N` phases; `rust` (the single unsharded local entrypoint
  named in `AGENTS.md`) is untouched. See `docs/DESIGN-ci.md`, "Sharded
  pre-merge Linux evidence", and `docs/DESIGN-semantic-mutation-gate.md`,
  "CI scheduling: two lanes, one aggregator".

- Required the complete Linux evidence on `main`, closing the gap #707
  opened: the `main safety and CI` ruleset (id `19090811`) now requires
  `rust workspace`, `WASM`, `semantic mutation (changed)`, and
  `FSL Logic Test (pr)` alongside `merge readiness`. `bypass_actors` stays
  empty and `strict_required_status_checks_policy` stays `true`, so no
  account — administrators included — can merge past a failing or missing
  required context. Requiring these four was previously unsafe: under the
  retired `paths-ignore` exemption an agent-configuration-only pull request
  emitted none of them, leaving them permanently `Expected`. The in-job scope
  check makes them always report, which is what unblocked this.

- Reverted a GitHub merge queue that was configured on `main` earlier the
  same day (2026-08-05), after direct measurement showed it cannot function
  in this repository's workflow: `gh pr merge --admin` bypasses the queue
  entirely (observed on #717 — merged directly, no `merge_group` event in the
  run history), and the normal `enqueuePullRequest` path is refused by
  ruleset `main review for non-admins` (one approving review plus
  `require_last_push_approval`), which a single-maintainer workflow cannot
  satisfy because GitHub does not allow self-approval. Setting
  `FSL_MERGE_QUEUE_CI=enabled` on top of that state would have made every
  pull request report cheap stubs with no `merge_group` run ever replacing
  them, landing changes on `main` with no pre-merge Linux evidence at all;
  the canary pull request found this before the variable was created.
  `ci.yml`'s `merge_group` trigger and the scope script's
  `queue-entry-stub` branch remain in place, inert, documented as such in
  `docs/DESIGN-ci.md`.

- Replaced `ci.yml`'s `paths-ignore`-based agent-configuration exemption with
  `tools/check-product-gate-scope.sh`, run as the first step of each of the
  four heavy product-gate jobs (`rust workspace`, `WASM`, `semantic
  mutation`, `FSL Logic Test`). Same accepted exemption scope
  (`.claude/**`, `.agents/**`, `CLAUDE.md`, `AGENTS.md`, `CHANGELOG.md`),
  but a workflow-level path skip never emits its job's context at all — if
  that context is ever made a required status check (issue #707) or needed
  to satisfy merge-queue entry, the skip leaves it permanently `Expected`,
  unfixable even by an admin merge (observed while merging PR #715); an
  in-job diff-checked early exit cannot get stuck that way. `ci.yml` also
  gains a currently dormant `merge_group` trigger and per-job scope checks
  on those four jobs, as inert groundwork for a future merge-queue-gated CI
  architecture — no live ruleset or repository-variable change ships with
  this PR; see `docs/DESIGN-ci.md`'s new "Merge queue (planned, not yet
  enabled)" section. The script's `selftest` subcommand is wired into
  `merge readiness / automation contracts` as its accepting/rejecting
  control.

- Exempted agent-configuration-only pull requests (`.claude/**`, `.agents/**`,
  `CLAUDE.md`, `AGENTS.md`, and `CHANGELOG.md`, whose coupled entry would
  otherwise defeat the exemption) from the pre-merge product gate via
  `paths-ignore` on `ci.yml`'s `pull_request` trigger. No product-gate lane
  reads those files; the paths that do (`merge readiness / automation
  contracts` runs the `.claude/` environment contract test, `release.yml`
  reads the changelog at tag time) keep unfiltered coverage. One changed
  file outside the list restores the full run, and the unfiltered `main`
  push trigger still gives every merged state the complete product
  evidence; `docs/DESIGN-ci.md` records the exemption contract and why
  `skills/**` and `docs/**` must never join the list.

- Added the `pr-review` agent skill (`.claude/skills/pr-review/SKILL.md`), a
  two-layer review orchestrator for pull requests that resolve issues: the
  skill reconstructs the issue contract from repository evidence, dispatches
  the specialized read-only reviewer agents (`fsl-coupled-change-reviewer`,
  `fsl-soundness-reviewer`, `fsl-vacuity-reviewer`, with
  `fsl-test-diagnostician` for failure triage) in parallel, audits the diff
  for green-faking (weakened specs, hand-edited snapshots, allowlist growth,
  loosened tests, missing negative controls, substring-strength assertions),
  reproduces the PR's verification claims into an explicit claim ledger
  (executed / read / unverified), adversarially verifies
  soundness-critical changes with reviewer-authored fixtures and a live
  mutation of the claimed negative control, and delivers a severity-ranked
  verdict without merging. `CLAUDE.md`'s verification checklist now points
  to it.

- Fixed a false green in `domain expand`/`check_domain`'s `can(Command)`
  rendering: `Context::normalize`
  (`rust/fsl-core/src/domain.rs`) joined a `decide`'s `requires` clauses and
  negated `rejects` conditions with literal `" and "` without
  parenthesizing each piece individually, so a piece containing a
  top-level `or` misgrouped once two or more pieces were present (`and`
  binds tighter than `or` in FSL's grammar). This could invert a verifier
  verdict between the directly-lowered typed model (`check`/`verify` on
  domain source) and the rendered-then-reparsed kernel (`domain expand`'s
  output): a `violated` invariant rendered as a tautology and reported
  `verified`. Each piece is now individually parenthesized before joining
  (#690, symptom 1 of 3; symptoms 2 and 3 remain open). Added
  `rust/fslc/tests/fixtures/domain_characterization/can_expansion_precedence.fsl`
  and `rust/fslc/tests/issue_690_can_precedence_false_green.rs` as the
  reproducing fixture and verdict-agreement regression control, and moved
  the fixture into `domain_render_agreement.rs`'s `VALID_DOMAIN_FIXTURES`;
  `expressions_valid.fsl` remains in `KNOWN_DIVERGENT_DOMAIN_FIXTURES` for
  the still-open name-shadowing symptom.

- Fixed `domain check`/`domain expand` producing an
  ill-typed `= 0` initializer for domain state fields whose accepted container
  type had no explicit default. The rendered lowering path now dispatches
  exhaustively over the structured type expression: `Option<T>` initializes to
  `none`, `Set<T>` to `Set {}`, and top-level `Map<K, V>` to the same dense
  per-key `forall` initializer as the typed lowering path. New corpus fixtures
  keep all three variants in the checked two-path agreement gate. Both paths
  now also share enum declaration validation, so an empty enum nested anywhere
  in those containers fails at its declaration instead of producing invalid
  text or panicking. Renderer failures now retain their source location and
  name-resolution classification in the CLI JSON envelope (#691).
- Fixed native induction's property selector so
  `verify --engine induction --property <trans>` reaches the already-supported
  transition base/step obligations instead of returning a usage error. The
  selected transition is the only transition obligation, while the complete
  user-invariant and implicit-bound set remains proved in the base case and
  available as the induction hypothesis; this avoids the false negative caused
  by treating proof dependencies as unselected obligations. Existing selected-
  invariant semantics are unchanged, and selected `leadsTo`/`reachable` plus
  unknown property names remain rejected as before (#701).
- Added a `property_selection` C3 assurance-matrix axis
  (`rust/fslc/tests/assurance/property_selection.rs`) so the pre-existing
  `properties` axis's `(property group, engine)` cells are no longer the only
  inventoried dimension: the `properties` axis records capability for an
  *unfiltered* run only, which is exactly how #701 went undetected --
  `transitions x induction` was `Exercised` there while the CLI's
  `--property`-selected run of that same cell was independently rejected.
  The new axis's rows (`"{property group}x{engine}"`) are generated from the
  Kernel-schema-synced `properties::ROWS` crossed with `properties.rs`'s own
  declared engine columns, never a hand-copied list, so a new property group
  or engine automatically gains rows here too and an unclaimed cell fails
  `every_declared_cell_across_every_axis_has_a_claim` until reviewed. Columns
  are the `--property Name` and `--exclude-property Name` selector lanes;
  `terminal` is `NotApplicable` on both for every engine because it has no
  name `select_properties` can resolve at all. Includes the `trans x
  induction x selected` positive control and the `reachable x induction x
  selected` rejecting control named in #705, plus the `leads_to x explicit x
  selected` rejecting control (explicit's leadsTo incapability holds under
  isolation too, unlike its accepted `leads_to x explicit x excluded` lane)
  (#705).

- Added an opt-in, CI-independent, Store-first Referance semantic-drift pilot for
  the bounded domain lowering/rendering slice (#709). The content-addressed
  Python/Rust generation probe retains the full JSON envelope and exit code with
  a failing mutation control, FSL verification bindings expose stale code/adapter
  evidence, and an auxiliary scoped CodeReferance profile indexes Rust enum
  variants and exact symbols. Referance
  output remains shadow evidence: the accepted procedure forbids automatic
  grounding, promotion, issue creation, and use in product or release gates.
  Authority triage filed the independent accepted-but-hollow findings as
  #710–#713 rather than treating Python parity or symbol presence as proof.

- Added a standalone `site reference freshness` CI workflow
  (`.github/workflows/site-reference-freshness.yml`) that runs the
  pre-existing but previously unwired `tests/test_site_reference_snapshot.py`
  on every pull request into `main`, so a stale `docs/intro/language.{ja,en}.html`
  or `cli.{ja,en}.html` (regenerated from `docs/LANGUAGE.md`/
  `docs/LANGUAGE.ja.md`/`src/fslc/cli.py` by `tools/build_site_reference.py`)
  now fails loudly on this workflow's own run (#630). Kept out of both
  `ci.yml`'s Rust-native-only product gate and `merge-readiness.yml`'s
  deliberately stdlib-only fail-fast lane; see `docs/DESIGN-docs-site.md`'s D7
  addendum for the full rationale, including the stated, unresolved
  frozen-Python-vs-native-CLI asymmetry this gate now enforces for
  `cli.*.html` (#688).

- Fixed literate Markdown FSL's diagnostic-quality gap: the 19 commands that
  read a spec path but do not extract ` ```fsl ` fences from a `.md` input
  (`lint`, `migrate`, `fmt`, `kernel`, `conformance`, `explain`, `mutate`,
  `typestate`, `testgen`, `html`, `ledger`, `analyze`, `diff`, `refine`,
  `replay`, `sweep`, and `document generate`/`claims`/`check`) used to hand
  the raw Markdown to the parser and report the user's spec as a syntax
  error at the position of the Markdown's own first non-fsl character
  (`fmt` reported it under `kind:"format"` with no `diagnostic_code` at all).
  They now fail closed with a dedicated `FSL-INPUT-LITERATE-UNSUPPORTED`
  `kind:"usage"` diagnostic naming `check`/`verify`/`scenarios` as the
  commands that do support literate input, with `loc` naming the input file
  rather than a spec position. No exit code changed. The decision and
  materialization are now owned by one function
  (`fslc_rust::literate_access::literate_access`), driven by a total,
  test-gated registry checked against `rust/fslc/cli-contract.json`'s real
  command surface (#665).

- Unified the nested `verify` kernel projection that `run_db_check` and
  `run_domain_check` each carried as an independent, hand-copied key list:
  `fslc_rust::outcome::{classify_kernel_key, project_kernel}` is now the
  single owner both commands call. `db check`'s copy previously dropped
  every AGENTS.md-protected replayable-evidence key (`loc`, `trace`,
  `blame`, ...); it now carries the same evidence `domain check` has since
  #515/#641. Both commands also gain `cti`/`hint` (the `unknown_cti`
  counterexample and its guidance), `unreached` (`reachable_failed`'s list
  of properties that could not be reached), and `trace_type`/
  `requirement`/`requirements`. Additive only: no existing key, top-level
  field, `result` value, or exit code changes; `db check`'s `kernel` object
  gains keys and loses none (#663). `rust/fsl-tools/src/ai.rs`'s independent
  third copy (`fslc ai check`) is a known, deliberately out-of-scope
  boundary — it cannot call the new owner because `fslc-rust` depends on
  `fsl-tools`, not the reverse — tracked separately as #687.

- Added a corpus-wide agreement gate between `domain`'s two independent
  lowering paths: `lower_domain` (typed `KernelSpec`, used by `check`/
  `verify`) and `domain_kernel_source` (rendered `.fsl` text, used by
  `domain expand` and `check_domain`).
  `rust/fsl-core/tests/domain_render_agreement.rs` projects both paths
  through `public_kernel_contract` for every domain spec in `examples/` and
  `rust/fslc/tests/fixtures/` and diffs the two structurally, excluding only
  source spans (a gated, single-entry exclusion whose classifier defaults
  to comparing every field unless one is explicitly named, so a future
  contract field cannot silently escape comparison the way #689 describes).
  Building the gate found three pre-existing, previously undetected live
  divergences between the two implementations -- not hypothetical, and not
  fixed here, but pinned as regression fixtures and filed as #690 (an
  internal generated-name check gap, plus a `quantity` name-shadowing bug
  and an unparenthesized `can(...)` macro expansion that can invert a
  verified/violated verdict on independent `Bool` state) and #691 (a
  `Map`-typed state field with no default renders an ill-typed literal `0`)
  (#664).
- Added (#727): `fslc mutate` now accepts `domain` documents. It renders the
  domain document through the same textual kernel path `fslc domain expand`
  uses (`fsl_tools::domain_kernel_source`) and mutates the re-parsed kernel
  spec instead of the direct-lowering path `check`/`verify` use for domain.
  Direct lowering does propagate real spans into the domain source file
  (measured: 409 spans, 16 null, same on both paths), but resolves to only 23
  distinct non-null source positions against 90 for the rendered path, so it
  collapses many distinct mutants onto the same witness location; the
  rendered path is chosen for that ~4x finer discrimination, at the cost that
  its `loc` points into rendered kernel text rather than a domain source
  line, which is why that text is embedded in the output envelope as
  `kernel_source`, matching `domain expand`/`domain check`. A saga whose
  compensation actions are structurally dead at baseline now reports every
  compensation-targeting mutant surviving with the existing "action dead at
  baseline" note, giving domain specs the same kill-rate self-check evidence
  channel other dialects already have. Unlowerable domain constructs are
  still rejected by the shared lowering guard before any mutant runs; other
  non-spec-like dialects (`governance`, `db`, …) are unaffected. As
  out-of-scope defensive hardening in the same dialect-lowering match this
  change adds an arm to, one call site (`requirements_trace_contract`'s error
  handler; unreachable for domain, whose only parse there always succeeds)
  and a second in the pre-existing `Business|Requirements|Compose` branch
  (never reached by `domain`, a separate match arm) stopped collapsing a
  typed `CoreError` into an untyped `semantics` message and now render
  through `core_error_output`, preserving `loc` and diagnostic
  classification (the #780 defect class).
- Added (#737): checked-in `changelog.d/` fragments for `CHANGELOG.md`'s
  `[Unreleased]` entries, closing the shared-edit merge-conflict spike's
  GO-C1 decision (`docs/DESIGN-changelog-fragments.md`). `tools/aggregate_changelog.sh`
  (stdlib-only bash, matching the `merge readiness / automation contracts`
  lane's dependency contract) implements all six fail-closed controls the
  record requires, each proven with both an accepting and a calibrated
  rejecting fixture executed in `selftest`: nonconforming fragment name (now
  also rejecting a conforming-looking name hidden anywhere
  `list_fragment_files` will not enumerate it -- a subdirectory or a
  top-level dotfile, both of which earlier releases of this fragment left
  invisible to every check but control 1's own shallow path match), duplicate (id, category),
  nondeterministic or nonconforming order (both the numeric-id and the
  category-order sort components, each pinned against a golden a
  lexicographic sham fails), unaggregated-at-release plus bidirectional
  direct-edit-forbidden (diffed against the merge base, matching this
  repository's own three-dot convention, so a release landing on `main` does
  not misname every open pull request's untouched `[Unreleased]` body as a
  direct edit; the single remaining release exclusion is proven not to let a
  bullet silently drop under cover of a release move), missing-or-empty
  fragment (with a release exclusion keyed directly to control 4's own
  validated "the `[Unreleased]` body was actually emptied under a matching
  new version heading" classification, computed once and shared by both
  controls so they cannot disagree -- not merely CHANGELOG.md's git status,
  which a fragment deletion plus any unrelated CHANGELOG.md edit, down to a
  trailing newline, would also have satisfied -- and no longer requiring a
  deleted fragment at all, so a version-only release with nothing accumulated
  in `changelog.d/` also merges. The exclusion exempts only the fixed, measured
  set of *product-surface* paths a release commit actually bumps
  (`is_release_bump_path`: `rust/Cargo.toml`, `rust/Cargo.lock`, and the domain
  characterization baseline, byte-identical across all four releases in this
  repository's history), not every product-surface path in a release-move diff
  -- a validated release move that also carries an unrelated product change
  still demands a fragment for that change, so a forged, empty version heading
  in a repository whose `[Unreleased]` body is otherwise always empty buys an
  exemption for those three paths and nothing beyond them, a bound
  `docs/DESIGN-changelog-fragments.md` records and accepts explicitly. A
  product-surface path arriving by rename or copy is classified by its
  destination, so moving a file into `rust/`, `specs/`, or any other product
  surface owes a fragment exactly as a plain addition does), and aggregation
  conservation checked by per-bullet identity, not substring containment
  (calibrated against a fragment-dropping sham, a first-line-only-truncating
  sham, and a sham that drops one fragment while duplicating another whose
  entire rendered block is a byte-for-byte prefix of the dropped one's).
  Fragment bodies are also checked for hygiene (a stray carriage-return byte,
  a leading list marker, or a leading ATX heading marker -- `#` through
  `######` followed by a space, not a bare issue-reference `#`) before
  aggregation. `changelog.d/.DS_Store`, `.gitkeep`, and editor swap files are
  exempted from the nonconforming-name/enumerability check by exact name,
  alongside `README.md`, since none of them can ever be an attempted fragment.
  The category vocabulary is eleven
  words, not ten: `changed` joins the set, measured 12 times across
  `CHANGELOG.md`'s full history (more than seven of the other ten words),
  where an earlier version of this mechanism's own documentation called it
  "unmeasured" and told authors not to add it. The existing 620-line,
  27-bullet `[Unreleased]` body is left untouched rather than converted: of
  its bullets, 10 are in `- <Lead> (#NNN):` form, 3 more carry an id outside
  that position, and 14 carry no id of any kind, so control 6 would reject
  every fragment a conversion tried to produce for those 14; the body moves
  under a version heading the ordinary way at the next release
  (`docs/RELEASE.md` step 7), in the same step that aggregates this
  directory's fragments. Every migration site moves together: `AGENTS.md`,
  `CONTRIBUTING.md`, `docs/DESIGN-kernel-contract.md`, and
  `docs/DESIGN-saga-history.md`'s coupled-change guidance,
  `.claude/hooks/changelog_reminder.py`, `.claude/agents/fsl-coupled-change-reviewer.md`,
  `.claude/skills/add-language-feature/SKILL.md`, `.claude/skills/release/SKILL.md`,
  `tools/check-product-gate-scope.sh`'s `changelog.d/` exemption prefix (with
  the `changelog.dx/y` near-miss still classifying as product),
  `docs/DESIGN-ci.md`'s exemption amendment, `.github/workflows/release.yml`'s
  stale-fragment guard, and `.github/workflows/merge-readiness.yml`'s new
  pull-request-level fragment/direct-edit check. This entry is itself the
  first fragment.
- Fixed (#723): effect `retry { backoff ... }` clauses (e.g. `retry { max_attempts
  3; backoff exponential }`) are now rejected fail-closed by both lowering
  paths through the same `validate_lowerable_constructs` gate
  (`rust/fsl-core/src/domain_lowering.rs`) that #710/#711/#712 established:
  no accepted design pins a backoff strategy's execution meaning in the
  finite model, and the parsed `backoff` value was never read by either
  lowering path or by the frozen Python reference
  (`src/fslc/domain_parser.py`, `src/fslc/domain_ir.py`), which only stores
  it in the IR. **Migration**: remove the `backoff ...` line from any `retry`
  block; it never had executable meaning under `check`/`verify`, so removing
  it changes no verification outcome. `retry { max_attempts N }` without a
  `backoff` clause is unaffected and still lowers the `attempts < N` guard.
  `examples/domain/order_async_effect.fsl` and
  `rust/fslc/tests/fixtures/issue_518_domain_replay.fsl` had their now-rejected
  `backoff exponential` lines removed as part of this fix, and
  `docs/intro/domain.{en,ja}.html`'s matching code sample was updated to
  match. See `docs/DESIGN-domain.md`. Tracked mechanism for future migration
  diagnostics: #702, #703. Remaining #723 items (`projection`, effect
  `compensation`, saga `outboxes`/`inboxes`) are out of this fix's scope.
- Fixed (#726): `fsl_tools::analyze_domain` now calls `domain_kernel_source`
  before projecting a domain document, instead of projecting the raw
  `DomainSpec` directly. `fslc domain analyze`'s accepted/rejected spec set is
  now identical to `domain expand`'s, since both share `domain_kernel_source`:
  this rejects not only the three #710/#711/#712 unlowerable constructs (a
  top-level `await`, an `on_stale` policy, a `value_object` invariant), which
  `check`/`verify` (including the wasm Worker's) already reject transitively
  through `lower_domain`, but also a conflicting explicit effect-outcome role,
  a duplicate or empty enum declaration, and any failure `domain_kernel_source`'s
  own kernel-text rendering raises (for example an unsupported Map/container
  default shape, or a reference to an unknown domain type). `analyze` previously
  accepted all of these with `result:"analyzed"`/exit 0; it now matches `domain
  expand`'s diagnostic byte-for-byte for every one of them. A well-formed
  domain spec still analyzes successfully.
- Fixed (#728): typed bounded never-enabled action coverage as `never_enabled_action`, so native `verify` and `sweep` select it with `--vacuity error|ignore`, while WASM and mode-less scenarios preserve the diagnostic.
- Fixed issue #729: `verification_warnings`'s `vacuous_implication`/
  `vacuous_leadsto` reachability probe (`fsl-runtime`) ran one **unbudgeted**
  concrete BFS per antecedent/trigger, cloning the whole `KernelModel` per
  explored node. On the `LabelCoreRepro` reproducer
  (`rust/fslc/tests/issue_697_all_properties_memory.rs`), isolating a single
  affected property (`--property PublishedWasReviewed`) consumed ~1,095 MB
  (measured control, before this fix; the issue reported ~1,096 MB), against
  ~47 MB for an unaffected property, and `--vacuity ignore` did not help —
  `apply_vacuity_mode` only filtered the already-computed output. Every
  implication antecedent and leadsTo trigger now shares one budgeted BFS
  (`fsl_runtime::expression_reachability`, budgeted at the same
  `CONCRETE_PROBE_BUDGET` `find_boundary_violation` uses and reusing its
  established scratch-`Monitor` pattern from issue #730/#776), which drops the
  same isolated-property case to ~348 MB (no vacuity option change) or ~44 MB
  (`--vacuity ignore`, which now genuinely skips the probe instead of
  filtering its output) and removes the per-property multiplier the shared BFS
  was designed to eliminate. Reachability is now tri-state
  (`Reachability::{Reachable, Unreachable, Exhausted}`): a candidate that hits
  the budget before resolving reports the new `vacuity_probe_truncated` kind
  (added to `fsl_core::VACUITY_KINDS`, 6 → 7) instead of silently becoming
  `Unreachable` (fail-open) or being dropped (also fail-open under `--vacuity
  error`) — depth-bounded non-closure is unaffected and still reports
  `vacuous_implication`/`vacuous_leadsto` exactly as before. `skip_vacuity_probe`
  is threaded as an explicit argument from the one CLI derivation point
  (`--vacuity ignore` in `verify`/`sweep`); the `ledger`/`html`/`mutate`
  baseline and the wasm Worker surface (neither has a `--vacuity` option)
  always compute it. Negative controls: the `--vacuity ignore` envelope equals
  the `--vacuity warn` envelope with vacuity-kind warnings filtered out
  (`issue_729_vacuity_ignore_skip.rs`); `ledger`/`html` output for an ordinary
  vacuous finding is unchanged, and a `vacuity_probe_truncated` finding cannot
  move a requirement's assurance class since `assurance_token` never reads
  `warnings` (`issue_729_vacuity_probe_truncated_ledger.rs`); a
  corpus-conservation test confirms no maintained spec exhausts the shared
  budget (`issue_729_vacuity_probe_corpus_budget.rs`).
- Fixed issue #730: `fsl-runtime`'s `bfs` (the solver-free agreement oracle),
  `first_self_violation` (`check_refinement`'s self-consistency precondition),
  and `verify_explicit_selected` each held a `Monitor` -- and therefore the
  whole `KernelModel` by value -- per queued/frontier node, so exploring `n`
  states cloned the model `n` times; `first_self_violation` additionally
  cloned its whole accumulated `Vec<TraceStep>` per node, worse than
  `find_boundary_violation` before issue #697's fix. Measured directly on the
  `LabelCoreRepro` reproducer (release build, depth 3, 16,290 states): `bfs`'s
  peak memory footprint dropped from ~946 MB to ~236 MB with the state count
  unchanged. All three now carry only a `State` (plus a scratch `Monitor`
  re-pointed at each popped state, `find_boundary_violation`'s established
  pattern) and reconstruct a trace from parent links only when one is
  actually needed. `first_self_violation`'s multi-root case (issue #493:
  nondeterministic init) is handled by `trace::reconstruct_trace` discovering
  each state's actual root by walking parent links to whichever parentless
  state the chain terminates at, rather than assuming a single passed-in
  initial state. No public contract changed: verdicts, JSON envelopes, and
  state counts are identical before and after on the reproducer and on the
  `specs`/`examples` corpus.
- Fixed (#731): `implicit_initial_value` now fires for every domain aggregate
  state field the renderer already gives an implicit default -- `Int` (`0`),
  `Option<T>` (`none`), `Set<T>` (`Set {}`), a top-level `Map<K, V>` (the dense
  per-key `forall` init), and `value_object`-typed fields (their struct-literal
  default) -- not just the original four scalar shapes (`Bool`, enum, range,
  external-placeholder). `rust/fslc/src/frontend_output.rs`'s warning now
  reads the selected value from `fsl_core::domain_type_default`, the same
  total dispatch `domain_kernel_source` already used
  (`rust/fsl-core/src/domain.rs`'s `Context::default_for_type`), instead of a
  second, non-exhaustive copy of the dispatch that silently excluded any type
  name containing `<`. An enum default is rendered in domain-source form (the
  bare declared member, e.g. `Pending`) rather than `domain_kernel_source`'s
  kernel-scoped mangled identifier (`Status_Pending`); a `DefaultForm`
  threaded through `Context::default_for_type`/`default`/`normalize` selects
  the right form at every enum-rendering site, including a value_object's own
  explicit default field and an enum nested inside a value_object's struct
  literal or a top-level Map's per-key value, so nesting depth cannot bring
  the mangled form back. New regression tests
  (`rust/fslc/tests/issue_250_initialization.rs`) cover container/value_object
  coverage, an explicit default still suppressing the warning, bare `Int`
  warning like every other scalar shape, and a nested enum inside a
  value_object/Map staying bare (never `domain_kernel_source`'s mangled form)
  at any depth; a `domain_expand`-vs-warning string-level check confirms the
  selected value matches for every shape that renders identically in both
  domain-source and kernel form -- deliberately excluding enum-bearing values,
  where the warning's bare member and `domain_kernel_source`'s mangled
  identifier now intentionally differ. A top-level `Map<K, V>` field (no
  whole-field initializer syntax exists at all) and a `Set<T>`/`value_object`
  field whose brace-literal default cannot yet round-trip through `fslc
  fmt`'s reformat-and-reparse pass (issue #770, found and reproduced
  independently while implementing this fix) both still warn under `check`
  but omit the machine-applicable insertion -- chosen by an allowlist over
  the field's type shape (`Option` only; everything else, including any
  future brace-literal-rendering constructor, fails closed until reviewed),
  not the rendered value's text -- and keep `edition_severity.next` at
  `warning` rather than `error`, since `migrate --write` is fail-closed (it
  would not write a corrupted file) but attempting that insertion would trip
  #770's reformat failure and fail migration for the whole file, dropping
  every other, otherwise-safe edit in it too. A "defect witness" test pins
  #770's `fslc fmt` symptom directly so a future fix to #770 turns it red;
  its failure message also flags issue #785, a second pre-existing defect on
  the same `Context::normalize` enum-mangling guard (found independently
  during review, not reachable through this PR's own withheld insertion) that
  must be fixed before the `value_object` insertion can be safely re-enabled.
- Fixed (#747): `main`'s scheduled product gate is no longer cancelled by a
  self-perpetuating cache-budget loop. PR #752 restricted `Swatinem/rust-cache`
  saving in `ci.yml` to non-pull-request events so pull requests could not keep
  evicting `main`'s caches, but `merge-readiness.yml`'s two `rust-cache` steps
  (`rust-compile`, `core-contracts`) carried no such guard. Because that
  workflow runs on every pull request, each one kept saving a fresh
  ref-scoped copy of both keys. Measured on 2026-08-11: 15 pull-request refs
  (#743-#792, all closed or merged by then) still held 29 entries (~1.90 GiB)
  on those two keys alone, part of 38 entries / 10.03 GiB total against the
  repository's 10 GiB budget -- retained not because anything kept restoring
  them, but because GitHub only evicts an unused cache after 7 days without a
  read, and measurement landed inside that window. The LRU evictor does not
  distinguish "small but many" from "few but large": it had removed
  `refs/heads/main`'s `native Z3 4.16 (windows-latest)` cache down to zero
  entries. `merge-readiness.yml` has no `push` trigger, so the `ci.yml`-style
  `save-if: ${{ github.event_name != 'pull_request' }}` guard would not close
  this path -- it would leave every pull request permanently cold, since
  nothing would ever save a `main` copy under this workflow's own key for it to
  restore. Both `Swatinem/rust-cache` steps instead go restore-only against
  `ci.yml`'s own `rust-workspace` key (`shared-key: rust-workspace`,
  `save-if: false`). The shared toolchain, runner, and checkout predict an exact
  derived-key match, and four direct pull-request restore logs confirm it: run
  `31581715093` attempt 1 logged `full match: true` for core contracts at 09:11:00.77Z
  and Rust compile at 09:11:04.76Z; run `31583381471` attempt 1 logged the same result at
  09:33:03.06Z and 09:33:05.55Z. Each identified
  `v0-rust-rust-workspace-Linux-x64-e8b3ee54-09fbaf53`; the logs, rather than
  the mechanism, establish the observed full-key match.
  `tools/run-semantic-mutation-gate.sh`'s
  mutants lane also moves its per-run scratch `CARGO_TARGET_DIR` out from under
  `rust/target` (to `${RUNNER_TEMP:-${TMPDIR:-/tmp}}`), and the script now
  clears any `rust/target/semantic-mutation.*`/`semantic-mutation-build` left by
  a restored cache unconditionally, right after mode validation, before either
  lane runs. That placement matters because the current
  `semantic-mutation-mutants` `save-if: false` actively makes the operators
  shards the key's only configured saver; it does not merely record an
  operators-only historical race. The counterexample is run `31086907528`,
  attempt 1: mutants job `92568586155` restored `No cache found.` and its
  successful post step uploaded 2,922,378,363 B while all three operators shards
  were cancelled with their post steps skipped. An earlier version of this
  cleanup ran only in the mutants lane's own path, after the operators lane's
  early exit, so the then-save-enabled operators lane never ran it.
  `semantic-mutation-mutants`'s `Swatinem/rust-cache` step is now
  `save-if: false` (with `cache-on-failure` removed, since `save-if: false`
  already makes it inert), closing its historically observed save path; the
  operators shards remain the only save-enabled path. **This is a
  closed-ingress-path fix, not a size fix**: measured directly (product-gate run `31210570118` attempt 1, job
  `92972117510`, `mutation operators (3/3)`), the `semantic-mutation` entry's
  current 2.719 GiB was created by a *cold* operators run (`No cache found.` at
  19:14:17Z, saved at 19:48:12Z). That operators job did not run the mutants
  lane; the same run's successful mutants job `92972117519` ran on a separate
  runner and could not contribute to this cache archive. Neither the mutants
  scratch build nor its evidence paths therefore contributed to this entry's
  size, so there was no dead weight to recover, and two earlier size predictions
  in this fragment and in `docs/DESIGN-ci.md` (~0.9-1.4 GiB from an assumed
  accumulating scratch tree, then ~2.2 GiB from `rust/target/fault-operators`'
  deliberately persistent build tree treated as a designed minimum size) were
  both wrong about what this entry actually is.
  This one cold-start save is evidence of what this key can legitimately hold,
  not a proven minimum across every shard and revision. `semantic-mutation` is
  not resized by this change and is not touched by the budget lever below.
  Separately, six observed `rust-native-z3` **`windows-latest`** scheduled runs
  timed out at the configured 40-minute limit from 2026-08-07 through 2026-08-11.
  They span four revisions and test volumes: `0590975` / `31133932323`,
  `bcb0a4d` / `31210570118`, `1050a76` / `31273202771`, and `60705fd` /
  `31330456273`, `31423201873`, `31527197290`. Each observed cancelled job had
  `Post Run Swatinem/rust-cache@v2` skipped; cache absence is a material
  correlate, not evidence that one deadlock or one change caused every timeout.
  That step now carries `cache-on-failure: true`. On run `31565897267` attempt 1, the cancelled
  Windows job's `Post Run Swatinem/rust-cache@v2` step succeeded and created
  `v0-rust-rust-native-z3-Windows_NT-x64-af4551b0-09fbaf53` at
  2026-08-12T06:16:19.271024Z (619,429,238 B), four seconds before the job
  completed; this directly contrasts with run `31527197290` attempt 1, whose cancelled
  job logged `9 skipped Post Run Swatinem/rust-cache@v2`. This is an observed
  cache save after timeout cancellation under the combined change, not isolated
  proof that `cache-on-failure` alone caused it: that changeset also raised this
  job's timeout from 40 to 60 minutes, and commit `877fe8c` (#752) likewise
  coupled the flag with timeout increases for the semantic-mutation lanes.
  macOS recovery is not established by this Windows observation. The direct
  scheduled recovery record is run `31632094255` attempt 1 (`event: schedule`):
  both `native Z3 4.16 (windows-latest)` and `product gate` concluded `success`.

  The predecessor `.github/scripts/audit-cache-budget.mjs` `sharedKeyOf` regex
  matched the GitHub Actions `runner.os` platform spellings (`Linux`/`macOS`/`Windows`), but
  `Swatinem/rust-cache` derives its key from `os.type()`, which reports
  `Linux`/`Darwin`/`Windows_NT` -- so `rust-native-z3`'s cache, on either
  platform, was invisible to every rule in this audit, including the one that
  should have reported `main`'s Windows entry evicted to zero during this
  incident. The current `entryIdentity`, which replaced `sharedKeyOf`, accepts
  only the observed `os.type()` spellings, and
  the default-branch requirement is now
  per-`{key, platform}` pair rather than per-key, so `rust-native-z3` must be
  present on both `Windows_NT` and `Darwin` independently -- one platform's
  cache can no longer hide the other's absence. A new general rule also flags
  any `v0-rust-*`-prefixed cache on a pull-request ref regardless of whether its
  shared key is one `ci.yml` declares, closing the same blind spot for any
  future workflow's unguarded `Swatinem/rust-cache` step (and, retroactively,
  for `merge-readiness.yml`'s own now-removed per-job keys, which this audit
  never flagged for the same reason). Its runner treats usage as conservative
  bytes-only evidence (`max(usage bytes, listing sum)`); GitHub refreshes its
  count approximately every five minutes, so `active_caches_count` is not an
  identity condition, but is rejected when supplied as an invalid numeric
  envelope field. It collects the full `created_at`-ascending listing twice,
  including a validated empty sentinel each time, and requires the ID set plus
  each ID's `key`, `ref`, and `size_in_bytes` to agree. One disagreement retries
  the paired collection; a second fails closed. This detects page-boundary
  mixing whenever it changes the two observed sets, but does not claim an atomic
  snapshot: two collections could still receive the same mixed state. Sentinel
  counts must be zero (the observed out-of-range envelope) or repeat the first
  count, never an arbitrary valid integer. A disagreement waits one second before
  the one bounded retry: this is pacing for a later request, not a claim of a
  fresh or independent backend snapshot, which GitHub does not guarantee. The retry-safe request bound counts
  HTTP-successful usage plus listing requests: `1 + 4 × (pages + sentinel)`, capped at 900
  to reserve 100 of the standard 1,000-request Actions-token quota, with
  missing/empty/invalid `x-ratelimit-remaining` rejected before conversion and
  headroom checked before every request and against the current collection's
  remaining requests, never a later small collection's bound minus cumulative
  requests. Malformed listing envelopes or usage bytes exit as `api-unreadable`;
  absent usage bytes become non-PASS `usage-unobserved`. GitHub documents
  `created_at` only as the primary sort key: the repeated live sample had unique
  timestamps, so it establishes no tie order. A tied boundary reorder can make
  the listing unauditable through a duplicate ID, rejected immediately, or
  different paired observations, which retry once and fail closed if still
  different; an identical mixed state can repeat as an undetectable residual.

  An earlier revision of this change added an `entry-oversized` finding
  (`SINGLE_ENTRY_WARN_BYTES = 2.5 GiB`) calibrated against the wrong ~2.2 GiB
  size above; since the observed clean size is 2.719 GiB, that control would
  have fired on a healthy `semantic-mutation` entry and has been removed rather than
  recalibrated, since no measured defect signature exists to calibrate it
  against (raising the threshold to paper over this would only be guessing at
  an unmeasured defect). The actual budget lever is `ci.yml`'s `fsl-logic` job,
  which now goes restore-only against `rust-workspace`
  (`shared-key: rust-workspace`, `save-if: false`) instead of saving its own
  key: its entire build (`cargo test -p fslc-rust --test typed_agreement
  --locked`) is a strict subset of what `rust workspace` already builds, and
  `Swatinem/rust-cache` prunes workspace-member artifacts at save time regardless
  of which job saves, so every lane's cache is substantively the same external
  dependency set. Measured main-branch entries (2026-08-12): `rust-workspace`
  1,605,761,517 B, `fsl-logic` 1,470,489,603 B, `wasm` 1,452,450,563 B,
  `rust-native-z3` Darwin 1,239,235,056 B, `semantic-mutation` 2,919,716,751 B,
  plus ~41 MB of tool-binary caches -- 8.130 GiB total. Deleting the now-orphaned
  `fsl-logic` entry (separate, human-authorized) and re-adding Windows
  native-z3 (historical 0.577 GiB) estimated 7.338 GiB (73.4%), under the
  8.5 GiB warn threshold. The 7.469 GiB listing was measured only after the
  six audit failures and was never itself audited. Four scheduled audits
  (`31239888526`, `31295386890`, `31357678690`, and `31459843075`, each attempt 1,
  2026-08-08 through 2026-08-11) reported only `budget-exhausted`; the two
  2026-08-12 failures (`31565897238` attempt 1, push, and `31566055925` attempt 1,
  schedule) reported both two orphaned `refs/pull/793/merge` Rust
  entries and `main-cache-absent` for `rust-native-z3`. Recreating the Windows
  entry at 2026-08-12T06:16:19.271024Z resolved the latter; human-authorized deletion of
  the two PR entries on 2026-08-13 resolved the remaining findings. The listing
  was then 7.337 GiB and audit run `31654305398` attempt 1 succeeded. `CI_SHARED_KEYS` and
  `REQUIRED_MAIN_ENTRIES` both drop `fsl-logic` accordingly, while the latter now explicitly
  requires the independently saved Linux `semantic-mutation` entry; the generic
  pull-request-rust-cache rule covers any regression the same way it already
  covers `merge-readiness.yml`'s former per-job keys. Separately, the shared-key
  regex now parses from the tail of the cache key (anchored at the end) instead
  of lazily from the head: a reviewer reproduced a case where a shared key
  containing a platform-like substring earlier in its name (e.g. a hypothetical
  `foo-Linux-bar`) misparsed into the wrong shared key, which caused a real,
  present main-branch entry to be reported `main-cache-absent`. See
  `docs/DESIGN-ci.md`, "Actions cache budget".
- Fixed (#723): `fslc domain check`/`analyze`'s `reliable_effect_without_outbox_boundary`
  finding now honors `DESIGN-effect.md`'s accepted "outbox on the effect *or
  owning saga*" contract instead of only the effect's own `outbox`. A saga owns
  an effect when one of its steps or `compensation` blocks emits the effect's
  request event; an unrelated saga's outbox boundary no longer silences the
  finding, and a partially-covered set of owning sagas still fires with the
  uncovered saga names recorded in the finding's `witness.uncovered_sagas`.
- Fixed (#779): a saga step, timeout, or compensation action that emits an
  event no longer leaves that event's declared `evolve` unapplied. Both native
  `domain` lowering paths (`lower_saga_actions` in `domain_lowering.rs`,
  `render_saga_actions` in `domain.rs`) called `event_assignments` for these
  three action kinds without the paired `evolve_items`/`saga_emit_evolve` call,
  so an action could raise its emitted event's one-hot flag while the aggregate
  state that event was declared to evolve stayed frozen at its initial value
  forever — an accepted-but-unreachable-transition soundness defect in the same
  class PR #725 (#713) closed for the compensation guard. Both paths now apply
  the declared evolve in the same action, in emit order, matching the pairing
  already correct for command/decide, effect-completion, and saga-observe
  actions. A new corpus-wide sweep
  (`rust/fsl-core/tests/domain_saga_evolve_pairing.rs`) asserts, for every
  action any domain fixture lowers to on either path, that an action setting an
  event flag true also applies that event's declared evolve, so a future
  lowering rewrite (e.g. #679's saga-history rewrite) cannot silently drop the
  pairing again. Negative controls on `examples/domain/order_fulfillment_saga.fsl`
  confirm both the fix (`inventory_status != ReservationPending` and
  `payment_status != PaymentPending`, previously `proved` under
  `--engine induction`, now correctly `violated`) and its boundary
  (`inventory_status != ReleaseRequested` stays `proved`, and the saga's
  never-enabled compensation warning is unchanged, because the compensation
  action's dual event-flag guard is untouched by this fix and remains
  structurally disabled until #679).
- Partially fixed (#780): domain, DB, AI-component, AI-project check/evidence,
  approval, and causal command boundaries preserve parse kind, location, and `FSL-PARSE`.
- Fixed issue #783: `fsl-runtime`'s `check_refinement` correspondence walk,
  `action_cover_traces`, and `leadsto_response_traces` were the three
  remaining lanes still holding a `Monitor` -- and therefore the whole
  `KernelModel` by value -- per queued/frontier node, the same defect class
  issue #730 fixed in `bfs`, `first_self_violation`, and
  `verify_explicit_selected`; the first two additionally cloned a growing
  `Vec<TraceStep>` per node. Measured directly on the shared `LabelCoreRepro`
  refinement fixture (release build, depth 3): `check_refinement`'s peak
  memory footprint dropped from ~1,728 MB (consistent with the issue's own
  reported ~1.72 GB) to ~491 MB, with `refines`/`refinement_failed` verdicts
  identical before and after. `check_refinement` and `action_cover_traces`
  now carry only a `State` per frontier node (plus a scratch `Monitor`
  re-pointed at each popped state) and reconstruct a trace from parent links
  only when one is actually needed, using the same `trace::ParentLink`/
  `reconstruct_trace` machinery issue #697 established; both are now locked
  onto a shared `LeanFrontier` type so a future per-node `Monitor`/
  `Vec<TraceStep>` clone in either is a consumer-side type error, not only a
  review-time judgment call. `leadsto_response_traces` is a partial fix
  scoped to the `Monitor` clone alone: its walk has no `visited` dedup (a
  `leadsTo` pending/response history is path-dependent, so two routes
  reaching the same state must stay distinct path-trees), so its per-node
  `Vec<TraceStep>` clone is unchanged -- but it is now locked onto a second
  shared type, `PathFrontier` (`(State, Vec<TraceStep>, usize)`, no
  `Monitor`), so a future per-node `Monitor` clone there is also a
  consumer-side type error, closing the one lane an independent review found
  without either a ceiling or a type guard. A negative control
  (`rust/fsl-runtime/tests/issue_783_refine_memory_ceiling.rs`, Linux-only,
  its `LabelCoreRepro` fixture shared with the `issue_730_*` ceiling tests via
  `tests/support/mod.rs`) adds a calibrated memory ceiling for
  `check_refinement`'s walk and, as PR #776 deferred to this issue, one for
  `first_self_violation`'s own frontier. Both ceilings are calibrated from a
  direct Linux measurement (`rust:1-bookworm` Docker, `aarch64`; exact
  commands, mutant source commits, and reasoning are recorded in the test
  file's own top comment, not only here): a binary-search sweep of
  `CEILING_KB` against the fixed build and a mutant confirmed the `ulimit
  -v`-capped assertion actually passes on the fixed build and actually fails
  (`SIGABRT`, `status=...unix_wait_status(134)`, `stderr="memory allocation
  of N bytes failed"`) on the corresponding mutant, with both PASS/FAIL
  boundaries reproduced twice. The two mutants differ in precision: commit
  `6012c00` (all three #783 lanes reverted) stands in for `check_refinement`'s
  mutant, sound teeth there because the measurement's driver never reaches
  `action_cover_traces`/`leadsto_response_traces`; `first_self_violation`'s
  mutant restores only that function to its pre-#776 form (commit
  `20d0a9b^`). `check_refinement`: fixed boundary `(500, 502]` MiB,
  `6012c00` boundary `(2000, 2020]` MiB, `CEILING_KB = 950 * 1024`
  (1.89-1.90x headroom, 2.11-2.13x margin). `first_self_violation`: fixed
  boundary `(500, 505]` MiB, pre-#776-clone boundary `(2120, 2150]` MiB,
  `CEILING_KB = 1000 * 1024` (1.98-2.00x headroom, 2.12-2.15x margin). The one
  remaining assumption is architecture: this measurement is `aarch64`, while
  this repository's CI runs `x86_64`; if either ceiling flakes on CI, that
  cross-architecture gap is the most likely cause, and widening `CEILING_KB`
  is the fix, not tightening the test's claim. No public contract changed:
  `RefinementCheck`/`LeadstoResponse` fields, JSON envelopes, exit codes, and
  verdicts/trace/state-count output are identical before and after, checked
  directly across every `specs`/`examples` corpus refinement mapping (`fslc
  refine`, 27 registered triples) and every `spec`-dialect corpus file (`fslc
  scenarios`, 93 files, default and `--depth 6`).
- Fixed (#796): `domain analyze` and `domain expand` now reject unresolved domain
  identifiers with the same located semantic diagnostic as `check`, instead of
  returning a false-green analysis or an unusable generated Kernel.
- Fixed: CLI test fixtures now check out with line-feed endings on every runner.
  `.gitattributes` pinned `*.fsl` and four named files, so the `.md` and `.json`
  fixtures added with the error-envelope parity matrix were checked out CRLF on
  `windows-latest`. `error_envelope_document.md`'s leading `---` then stopped
  parsing as document frontmatter and three parity cells failed on that runner
  alone while Linux and macOS stayed green (run `31759050211`). The directory now
  carries the rule, `.z3-trace` stays binary, and
  `rust/fslc/tests/fixture_line_endings.rs` fails with the offending path if a
  fixture escapes it again.
- Fixed (#808): native `fslc` verification now binds validation, lowering,
  requirements metadata, and engine execution to one captured root-spec snapshot.
- Fixed (#808): native `fslc mutate` now binds its baseline verification, Kernel/model load,
  requirements-trace contract, and mutant enumeration to one captured root-spec snapshot, so a
  concurrent edit of the input file can no longer mix one revision's baseline with another
  revision's kill-rate.
- Fixed (#808): native `fslc testgen` and generic `scenarios` now bind requirement-trace
  validation, the BMC fallback on a genuine violation, and requirement-trace scenario
  generation to the same captured root-spec snapshot as `verify` (#811).
- Required (Refs #781): native CLI error-envelope parity now pins measured
  `check`/`verify` Parse envelope differences for AI-project and causal input
  shapes under #780, classifies the actually invoked `approval create --kind
  requirements_document` frontend with its generated reviewed artifact, and
  records the compose nested-component Parse boundary. #801 tracks only the
  remaining work to derive the hand-authored input-shape population.
- Documented (#723): `docs/DESIGN-effect.md`'s effect lifecycle diagram no
  longer lists `Compensated` as a reachable status. `effect_outcome_member`
  (`rust/fsl-core/src/domain_lowering.rs`) is a total function over exactly
  `{Succeeded, Failed, TimedOut, Cancelled}`, and the renderer
  (`rust/fsl-core/src/domain.rs`) mirrors the same set; an effect's
  `compensation { emits ... }` block only feeds the presence-only
  `missing_compensation_for_irreversible_effect` finding
  (`rust/fsl-tools/src/domain.rs`) and never writes effect status, so
  `Compensated` was unreachable in both v0 lowering paths. The generated
  `EffectStatus` enum keeps the `Compensated` member (reserved) because
  `rust/fslc/tests/issue_450_sibling_enum_conversion.rs` and the domain
  characterization baseline already depend on it for refinement enum
  conversion. A new negative control,
  `effect_compensated_status_is_never_assigned_by_either_lowering_path`
  (`rust/fsl-core/tests/domain_render_agreement.rs`), pins the current
  unreachability across both lowering paths so a future writer for
  `Compensated` is forced to update this test and `docs/DESIGN-effect.md`
  together; real compensation semantics are tracked against #679's
  correlation-indexed `SagaPhase` history, a precondition for defining which
  compensation cancels which specific effect instance.
  `docs/intro/domain.en.html` and `docs/intro/domain.ja.html` carried the same
  `Compensated` transition in their effect-lifecycle callout and are corrected
  identically.
- Documented (#724): `docs/DESIGN-domain.md`'s Findings section restated to
  match the native `fslc domain check`/`domain replay` implementation instead
  of the 7-kind list it previously claimed as implemented. Native
  `fslc domain check` implements exactly 4 finding kinds
  (`rust/fsl-tools/src/domain.rs`'s `effect_findings`):
  `irreversible_effect_without_idempotency_key`,
  `pending_effect_without_timeout_or_fallback`,
  `missing_compensation_for_irreversible_effect`, and
  `reliable_effect_without_outbox_boundary`. `aggregate_boundary_violation` and
  the static form of `uncorrelated_async_completion` were never findings: an
  `evolve` writing outside its aggregate and an async effect with no
  `correlation_id` both fail Kernel lowering itself
  (`rust/fsl-core/src/domain_lowering.rs`) with a top-level `result:"error"`,
  `kind:"semantics"` envelope outside the finding schema, a stronger
  (fail-closed) guarantee than a warning/error finding would give; only the
  `domain replay` form of `uncorrelated_async_completion` is a real finding
  (`rust/fslc/src/main.rs`, `rust/fslc/tests/issue_518_domain_replay_detection.rs`).
  `missing_decide_for_command`, `missing_evolve_for_event`, and the
  previously-undocumented `cross_aggregate_update_without_event` exist only in
  the frozen Python compatibility reference (`src/fslc/domain_expand.py`), not
  in native. `late_completion_without_stale_policy` is removed rather than
  restated: reviving it would recommend the `on_stale` syntax native already
  rejects fail-closed (#711). `saga_dead_end` and `process_wait_cycle` are
  removed with no native replacement; whether to implement them is tracked in
  #769. `docs/DESIGN-effect.md`'s effect-lifecycle summary and its
  timeout/retry/fallback vs. stale-policy warning claim are corrected to match.
  `docs/intro/domain.en.html` and `docs/intro/domain.ja.html` carried the same
  7-kind over-statement in their own findings table and prevention cards and
  are corrected identically.
- Documented (#747): Recorded recovery observations after #794: on run `31565897267` attempt 1, a cancelled
  Windows native-Z3 job's post step saved 619,429,238 B, although the same changeset also raised its
  timeout from 40 to 60 minutes; runs `31581715093` attempt 1 and `31583381471` attempt 1 logged four
  merge-readiness full matches; the Windows entry's 2026-08-12T06:16:19.271024Z recreation resolved
  `main-cache-absent`, and removing two human-authorized orphaned #793 caches resolved the remaining audit
  findings before run `31654305398` attempt 1 succeeded at 7.337 GiB. The direct scheduled recovery
  record, run `31632094255` attempt 1 (`event: schedule`), has both `native Z3 4.16 (windows-latest)`
  and `product gate` concluding `success`. The restore-only FSL Logic Test logged shared-key full
  matches on run `31565897267` attempt 1 (2m54s) and on run `31570480618` attempt 1 (3m02s). Also recorded that the current
  `save-if: false` guard, rather than historical timing, makes operators the only configured
  `semantic-mutation` saver: run `31086907528` attempt 1's mutants job `92568586155` uploaded
  2,922,378,363 B after `No cache found.` while all operator posts were skipped.
  The recovery listing's repeated `created_at` order was observed only across distinct timestamps;
  GitHub documents no secondary tie order, so a future tied page boundary can produce a duplicate ID,
  rejected immediately, or different paired observations, which retry once and fail closed if still
  different; an identical mixed state can repeat as an undetectable residual.
- Documented (#787): `AGENTS.md`, `CONTRIBUTING.md`, `docs/DESIGN-conformance-harness.md`,
  `docs/README.md`, `tests/test_dialect_conformance.py`, `tests/dialect_registry.py`,
  `rust/fsl-syntax/src/causal.rs`, and `docs/DESIGN-causal.md` now state plainly that the
  frozen-Python conformance harness and coupled-change parity checks (`tests/test_dialect_conformance.py`,
  `tests/test_coupled_change_meta.py`) are developer-run manual/reference checks with no CI or
  `tools/check-native-integration.sh` lane invoking them, instead of describing them as machine-enforced
  gates. The underlying coupling rules (register a new dialect construct in `tests/dialect_registry.py`)
  are unchanged; only the enforcement-mechanism claim was corrected. The `docs/LANGUAGE.ja.md` freshness
  claim, which genuinely is a required CI check (`site-reference-freshness.yml`), is untouched.

## [4.2.0] - 2026-08-03

- Fixed native Rust business-dialect precedence policies so `every <Entity>
  reaching ... must have passed through ...` lowers to deduplicated history
  state, transition updates, the attributed no-bypass invariant, and its
  dominated-set stability invariant. Native BMC, induction, and explicit-state
  regressions now fail if the lowering is hollowed back into a no-op (#681).
- Hardened semantic migrations against another hollow accepted construct:
  behavior-bearing variants now require executable or fail-closed controls,
  the precedence bypass gallery and a curated implementation fault operator
  calibrate native detection, all four business policy variants have native
  rejecting controls, and both agent harnesses retain soundness findings until
  they are fixed or linked to an issue; the merge-readiness automation lane
  executes that harness contract on every pull request (#681).
- Accepted the measured correlated-saga-history design: retain one-hot event
  flags as current-transition observations and add a dedicated
  correlation-indexed six-phase saga map in a follow-up. The maintained
  two-correlation fixture measures the three candidates as ×1 reused effect
  state, 32 sticky-event valuations versus 6 one-hot valuations, and 36
  dedicated saga-progress valuations; a negative control proves global sticky
  flags alias correlations (#662).
- Reached `Seq` `pop`/`head`/`at`/index operations are now uniformly
  `partial_op` violations in property context across Monitor BFS, explicit
  verification, native/browser symbolic BMC, and CLI rendering. The contract
  includes `terminal` and `leadsTo`, so neither can turn an undefined Seq read
  into a solver-selected deadlock decision or vacuously false antecedent.
  Symbolic phantom slots can no longer fabricate a property verdict or
  reachable witness; property-context `/` and `%` zero totalization is
  unchanged (#650).
- Native BMC now distinguishes structurally impossible `reachable` targets
  from depth-limited ones. `over_constrained` entries carry an irreducible
  `blocking_requires` set computed in an independent solver session, so path
  UNSAT cannot poison the classification and diagnostic queries cannot perturb
  native/browser witness projection (#634).
- Fixed `fslc typestate` silently omitting locally guarded read/query actions.
  They now appear as explicit state-preserving self-loops, and generated
  TypeScript preserves the caller's exact phantom state instead of widening a
  multi-state guard (#635).
- Bounded LSP workspace discovery now scans only direct siblings for a document
  opened outside every registered workspace root, preventing an arbitrary
  parent such as `/tmp` from becoming an unbounded recursive index while
  preserving explicit workspace and sibling cross-file behavior (#623).
- `fslc explain` counterfactual weakening now evaluates only the invariant and
  reachable outcomes it actually reports, and explicitly marks liveness as
  skipped. Quantified `leadsTo` checks no longer multiply every safety mutant's
  runtime while their results are discarded (#633).
- Added a calibrated `vacuous_deadline` lane that proves a generated deadline's
  age remains zero across every transition, catching state-changing urgent
  handlers that evade the narrower global `urgency_freeze` proof. The SLA worker
  example now consumes slack and fails when its deadline is tightened (#632).
- Added the M13 FSL Logic Test (#673): deterministic valid finite models now
  carry stable seed/case IDs, a single-case replay command, machine-coupled
  generation/dialect/metamorphic inventory, and named concrete-versus-symbolic
  agreement edges over verdicts, clean-run reachables/action coverage, exact
  witnesses, replay, and successor admission. PR and scheduled tiers emit a
  fail-closed completeness report; scheduled evidence spans three seeds and
  three depths (1,152 cases), delayed violations carry nontrivial actions, and
  a comparator corruption control exercises structural shrinking and minimized replay, while a deduplicated corpus owns
  future real disagreements. Generation volume does not promote public
  assurance or change product output.
- Added the M13 soundness-critical semantic mutation gate (#672). A pinned
  `cargo-mutants 27.1.0` pilot now mutates exact P2 bounded-verification,
  witness projection/replay, identity/location, and result-fold decisions;
  reviewed semantic operators cover unknown/backend failures, inclusive
  depth, witness omissions, replay bypass, and false-success folds that
  generic token mutation cannot express. Pull requests run changed-scope
  mutants plus every curated control, with the detector package named
  explicitly even when a diff selects only `fsl-verifier`; product events run
  the complete accepted scope. Survivors, timeouts, stale seams, missing output,
  and stale equivalence records fail closed; this calibrates detection power
  without promoting any public assurance class or changing product output.
- Added CI-internal Triangulated Assurance (#670): a federated, fail-closed
  registry now requires one raw observation, two explicitly independent
  semantic lineages, all three executable agreement edges, positive/negative
  calibration, and honest scope. Initial pilots cover compound native outcome
  conservation, symbolic-witness/concrete-replay identity (including
  state/step/kind/location corruption controls), and token-based dialect
  dispatch across the syntax library, CLI, and LSP. A calibrated common-mode
  fault executes a production-classifier substitution for the registered P1
  independent observer and declares its shared model lineage; no public
  assurance class, JSON field, or exit code changes.
- Fixed raw `fsl_verifier::verify_bounded*` calls bypassing the automatic
  action-context partial-operation check. Symbolic BMC now reports replayable
  `partial_op` evidence for ordered guards, reached action bodies, and
  `ensures`, including selected-property, supplied-state, and nondeterministic
  init paths; native CLI and Worker no longer source `partial_op` from their
  separate concrete boundary pre-scan (#651).
- Fixed `domain` saga lowering so effect-owned outcome events cannot bypass the
  effect correlation guard through weaker generated observation actions. Three
  observation actions disappear from generated kernels (#640).
- `fslc domain check` now preserves the nested kernel's `warnings` and
  `action_coverage`, so vacuity and generated-action coverage diagnostics remain
  visible to domain users instead of being dropped by the stable projection
  (#641).

## [4.1.0] - 2026-07-30

- Semantic Assurance Matrix slices 2/3 add source-derived `expr` (24
  `Expr` + 4 `AggregateKind` rows), `types` (9 `TypeRef` + 3 `TypeDef`
  rows), and `dialects` (all 10 `frontends!` keywords) axes for issue #537
  C3. A 25-model deterministic C6 family now checks all evaluator-reachable
  expression variants and all type rows across Monitor BFS / explicit / BMC,
  with a known-violating paired control for every model, while `Call`/`Stage`,
  standalone refinement checks, and the Worker's missing agent path are
  recorded with fail-closed evidence.
- `rust/fslc/tests/typed_agreement.rs` introduces the C6 typed generative /
  metamorphic cross-engine agreement suite (issue #537 C6 slice 1, issue
  #648, `docs/DESIGN-conformance-harness.md` "Typed generative / metamorphic
  agreement"): a deterministic structural generator (15 domain-axis models
  + 4 operation-axis models, no randomness) builds checked `KernelModel`s
  and compares Monitor BFS / explicit / BMC verdicts, replay, and successor
  admission, plus seven metamorphic relations (alpha rename, BOM/trivia,
  inline-vs-explicit init, disjoint assignment reorder, domain-size
  boundary, short-circuit/partial/Euclidean duality, entity/number sugar vs
  lowered type) each with a positive test and a negative control. Two
  confirmed cross-engine findings are recorded as re-measured, self-retiring
  exclusions rather than normalized away: `verify_bounded`'s symbolic Seq
  encoding treats an out-of-range `head()` read in property context as
  *defined* with an out-of-bound value instead of undefined (a spurious
  `violated`/`invariant`, disagreeing with the concrete engines' correct
  error); and `verify_bounded` alone never performs LANGUAGE.md's automatic
  "Partial operations" check in action context at all (the CLI's
  `--engine bmc` classification comes from a separate concrete pre-scan,
  `find_boundary_violation`, not the solver).
- `corpus_expectation_manifest.rs` and `evidence_corpus_manifest.rs` give every
  declared `examples/gallery/{valid,errors,adversarial}` fixture and every
  `causal`/`agent`/`ai_component` evidence-only document exactly one native
  owner (issue #645, #537 C4 residual). 12 of ~38 declared gallery fixtures had
  never been run by any native test — `tests/test_gallery.py` was the only
  oracle, and it is frozen-Python, outside `tools/check-native-
  integration.sh` — and nothing walked the corpus for an unregistered
  `causal`/`fsl-ai` document. All reproduce their declared `result`/`kind`/exit
  natively; no defect surfaced. `corpus_check_sweep.rs`'s
  `every_corpus_spec_checks_ok_or_declares_its_error` no longer duplicates the
  `check`-targeted half of that claim, now owned by the new manifest instead.
  A shared `rust/fslc/tests/support/mod.rs` replaces the corpus-walk helpers
  `corpus_check_sweep.rs` and `refine_corpus_parity.rs` each carried their own
  copy of.
- `rust/fslc/tests/assurance_matrix.rs` introduces the federated Semantic
  Assurance Matrix skeleton (issue #537 C3 slice 1,
  `docs/DESIGN-assurance-matrix.md`): axis modules under `tests/assurance/`
  own their rows (read from existing single-owner registries, never copied)
  and declared columns; every `(row, column)` cell carries a
  machine-rechecked citation (path + anchor, re-read from the working tree
  each run); blank required cells and stale/fabricated citations fail CI;
  negative controls prove both checks can actually fail. Slice 1 axes:
  `outcome_kind` (7 Monitor kinds × Monitor/CLI), `violation_kind` (12
  engine kinds × BMC/induction, #646), and `properties` (5 KernelModel
  property kinds × BMC/explicit/induction/replay).
- `fsl-verifier`'s free-form `Violation.kind` strings (`BmcViolation`/
  `InductionCti`/`RankFailure`) are now single-owned in
  `rust/fsl-verifier/src/violation_kind.rs` instead of scattered string
  literals (#646). Reading the emission sites directly (not transcribing
  issue #646's own citation list) found 12 live values, not the 9 the issue
  named — `pending_not_preserved`, `non_decreasing_helpful_action`, and
  `non_helpful_action_increases_measure` had no exercising fixture anywhere
  in the corpus before this change. No emitted JSON `violation_kind` value
  changes by a single byte. Part of #537 C3 slice 1
  (`docs/DESIGN-assurance-matrix.md`).
- The kernel AST projection no longer re-serializes every subtree, and is named
  for what it is: `python_ast` is now `kernel_ast_v1` (#622). `json!` calls
  `serde_json::to_value` on each interpolated operand, and the operands were
  already-built `Value` children, so every level deep-copied the subtree below
  it — O(depth) stack and O(n²) time per level, spent *inside*
  `recursion::guard` rather than around it, which is why #620's guard could not
  see it. Nodes are now built with `Value::Array(vec![...])`. The name was
  history, not contract: the tagged-array shape came from the frozen Python
  reference, but the shape is now the input to `fsl-kernel-ast-v1+sha256`, which
  **two** digests hash (`approval::spec_digest`,
  `document_digest::spec_digest_from_kernel`). Deleting or reshaping it would
  drift every approval record already in a user's repository; renaming the
  function changes nothing a user can observe.
- `PredicateExpander::expand_expr` and `public_kernel::expr_json` are now
  guarded too, bringing the #620 table to ten cycles (#622). Neither was
  reachable by the #620 witness, whose depth lived in a refinement mapping; a
  second generator that puts the same depth inside an `invariant` — the shape
  the digests actually project — aborted both immediately. `expr_json` is the
  more interesting of the two: it called the already-guarded `infer_type` once
  per level, so the guard ran every level, and the crash still landed in
  `stacker::_grow` itself, because the check happens on entry and this frame is
  large enough that the stack could fall from above the red zone to below what
  growing it costs. A guard on a called function is not a guard on the calling
  cycle. `fslc check`, `document claims`, and `kernel` now complete on a
  2000-deep invariant.
- Byte-identity was proven by differential sweep, not self-consistency: the
  pre-change and post-change binaries were run over all 211 corpus specs under
  `document claims`, `analyze`, and `testgen`, comparing stdout byte for byte —
  633 comparisons, 0 mismatches. The sweep's own control came first and mattered:
  run against itself the old binary disagreed with itself on 14 files, because
  `testgen` stamps wall-clock `elapsed_s`/`check_elapsed_s` into its envelope.
  Those two keys are blanked; nothing else is normalized.

- `tools/run-fault-operators.sh` no longer measures a *previous* run's fault.
  The scratch checkout shares one `CARGO_TARGET_DIR` across steps, and `rsync -a`
  restores the reverted sources with the worktree's mtimes -- which can be older
  than rlibs that an operator step compiled *with its patch applied*. Cargo then
  judged those crates fresh and linked the faulted rlib into the no-op control's
  binary, so a warm run reported a detector red for a fault that was no longer
  in the source. Every `sync_scratch` now touches the files any patch names
  (taken from the diffs' `+++ b/` headers), which puts cargo's mtime comparison
  on the side of rebuilding without discarding the rest of the warm cache.
  Reproduced deterministically -- cold run green, every warm run red,
  `cargo clean -p fsl-syntax` green again -- and verified by restoring clean
  sources into a fault-built scratch: before the fix cargo recompiled nothing
  and the rebuilt binary still aborted on the #620 witness (exit 134); after it,
  `fsl-syntax` recompiles and the witness exits 0. This is the second
  scratch-fidelity defect in this harness after the `.git` marker (#611), and
  the first to be caught by the no-op control rather than by inspection --
  a harness that trusted its cache would have called the operator calibrated
  while measuring the fault itself.
- Recursion over a spec's expression structure now grows the stack instead of
  aborting the process (#620). #617 made every platform agree on an 8 MiB
  stack; it did not make the recursion bounded, and a generated 160-stage
  refinement trio still killed `fslc` with `has overflowed its stack` — exit
  134, no JSON envelope, no exit code the outcome contract could read. The
  fix is `stacker::maybe_grow` at each recursion cycle entry, not a depth
  limit: a limit would put an arbitrary constant into the language contract
  and reject legitimate machine-generated specs, and this class was found by
  exactly such a spec. Every current answer is preserved and none is added.
  The measurement in #620 named two sites; a debugger's innermost frame only
  shows whichever site ran out first, so guarding one exposed the next, six
  rounds over. Eight cycles across three crates are now guarded —
  `SyntaxParser::expression`, `SyntaxExpr::into_kernel`,
  `SyntaxExpr::render_source`, `Expr::kernel_ast_v1` (`fsl-syntax`),
  `elaborate_enum_conversions`, `infer_type`, `validate_expression`
  (`fsl-core`), and `eval` (`fsl-verifier`) — behind one
  `fsl_syntax::recursion::guard` that owns the red zone and segment size, so a
  future site cannot pick its own constants. Six of the eight are
  crash-witnessed; `render_source` and `infer_type` are not, and each says so at
  its definition together with the N at which it was observed to survive
  unguarded, so a reader can tell a measured guard from a speculative one.
  `refine` now proves a 1000-stage trio (a 2000-long right-nested `if` chain)
  that previously aborted at 160, and `check`/`fmt` return their ordinary
  envelopes on the same file.

  The invariant is **not** fully met, and #622 tracks the rest. `Expr::kernel_ast_v1`
  builds its result with `json!`, which calls `serde_json::to_value` on each
  already-built child and re-serializes the whole subtree — O(depth) stack per
  level, inside the guard rather than around it. `fslc analyze` still aborts
  above ~200 stages. This is not analyze-only: the same projection feeds both
  spec digests (`approval::spec_digest`,
  `document_digest::spec_digest_from_kernel`), the requirements document's claim
  projection, and testgen's `expect`, so a deep enough invariant in a spec
  reaches them too. The witness exposed only `analyze` because its deep
  expression lives in a refinement mapping. Because that projection is digest
  input, the repair has to prove byte-identical output and is a separate
  reviewable change. See `docs/DESIGN-rust-component-internals.md` 4.4.
- `fslc` now runs on an explicitly sized 8 MiB stack on every platform
  instead of whatever the OS gives the main thread (#617). Windows gives
  1 MiB where Linux and macOS give 8, and `fslc refine` needed more than 1
  on `examples/agentic_rag` and `examples/multi_agent_system`: the same
  command on the same bytes answered `refines` on Linux and aborted with
  `has overflowed its stack` on Windows, returning neither a JSON envelope
  nor an exit code. Those two mappings are among the 22 that #593 counted as
  never executed, so #616's manifest ran them for the first time and the
  crash surfaced on its first post-merge Windows run. The recursion is still
  unbounded — it grows with the spec's structure, not with `--depth` — which
  is #620; this makes the platforms agree, it does not add a limit.
- `examples/layers/return_impl_refines.fsl` now declares an `enum abstraction`
  instead of a bare-member if-chain, so the command `examples/layers/README.md`
  documents answers `refines` again rather than `error`/`kind:"type"` (#615).
  `da003eb` began rejecting members that name a position in more than one enum,
  which is correct -- `New`, `AutoApproved`, `MgrQueue`, `MgrApproved`, and
  `MgrRejected` each belong to both `DSt` and `SSt`, so the chain read as an
  identity while the compiler had to guess. The corpus was never migrated to
  the `convert`/`abstract` requirement that change introduced. `DSt`'s
  `PaySubmitted` and `PayConfirmed` both collapse to `SSt`'s `Paid`, making this
  many-to-one and `enum abstraction` rather than `enum conversion` the right
  form. The mapping's #616 exclusion went stale on the fix and is replaced by a
  live manifest row, which is the self-retirement working on a real repair
  rather than a rehearsed one.
- `run_db_check` and `run_domain_check` now share one
  `outcome::is_definitive_kernel_verdict` instead of each carrying the rule
  that only a kernel status of 0 or 1 may be folded into a dialect verdict
  (#612). #600 existed because they did not: `run_domain_check` had folded
  correctly since #515 while `run_db_check` tested only `== 2`, so an internal
  error was absorbed as a `dbsystem` verdict. Behaviour is unchanged; the point
  is that a third dialect check cannot now be written without the rule.
### Added
- `rust/fslc/tests/deep_nesting.rs`: the #620 witness as a Rust generator rather
  than a checked-in fixture, so the regression carries its own scale knob and
  needs no interpreter. `WITNESS_STAGES = 200` sits above the measured 140-160
  crash threshold, and a `const _: () = assert!(WITNESS_STAGES > 160)` makes
  lowering it to speed the test up a compile error rather than a silent
  downgrade to a test that asserts nothing. The assertion is that `fslc` exited
  at all — `Output::status.code()` is `None` exactly when a stack overflow kills
  the process — alongside the verdicts, because a guard that changed an answer
  would be worse than the crash.
- `unguarded-recursion` #537 C5 fault operator: patches
  `fsl_syntax::recursion::guard` back into a direct call and requires
  `deep_nesting` to fail while the parse-diagnostic matrix stays green. The
  guard was consolidated into one function partly so this operator is one line;
  an operator that had to patch eight call sites would calibrate the patch
  rather than the defect.
- `rust/fslc/tests/refine_corpus_parity.rs`: a command-owned manifest binding
  every corpus refinement mapping to native `fslc refine` (issue #593, #537 C4,
  `docs/DESIGN-conformance-harness.md` "Refinement mapping manifest"). 22 of the
  28 mappings had never been executed by any test, and the script covering the
  other 6 (`tools/check_rust_refinement_parity.py`) was invoked by no workflow and
  no `tools/check-native-integration.sh` lane. `corpus_check_sweep.rs` cannot
  close this: a mapping has no `state` block, so `fslc check` answers
  `semantics`/"spec has no state block" for one whether or not the mapping is
  sound — a green corpus said nothing about `refine`. The roster is walked from
  `specs/` + `examples/` rather than listed, so an unregistered mapping fails
  instead of silently joining the 22, and a registered path that no longer exists
  fails as a stale entry; a hard-coded list is the shape #577 retired 28 stale
  instances of. Each of the 26 live rows carries `declared_by`, the `path:line` of
  the README row or fixture header that **declares** its verdict, and compares
  `result`, `kind`, **and** the process exit code. Expectations are transcribed
  from those declarations, never from observed output: pinning a measurement would
  make a future defect look like the contract, which is exactly the state
  `examples/layers/return_impl_refines.fsl` is in. Where a mapping declares its
  own expectation in the gallery `expected-command`/`expected-result`/
  `expected-kind` header convention, the row is checked against it rather than
  trusted, `--depth` included — that caught a row running at depth 4 against a
  header declaring `--depth 3`. 22 of the 26 rows additionally anchor their
  `declared_by` to the cited file, and the harness requires that file to still
  contain a line stating the row's verdict, so a citation cannot outlive the
  declaration it names. That keeps each declaration single-owner:
  `governance_semantic_mapping.fsl` is declared by the broken implementation it
  maps, and the citation is checked there rather than copied onto the mapping as
  a second header to keep in sync. The two exclusions are self-retiring in the #568
  sense: the harness re-measures each recorded premise and fails, naming the row
  that must replace it, once the premise stops holding.
- `specs/bank.fsl` and `specs/seat_booking.fsl`: the `fslc refine` command line
  each header already implied. Both headers asserted that the paired
  implementation "refines this" but documented no way to run it, so the manifest
  row above had no depth to inherit. This promotes an existing assertion to a
  runnable claim; it introduces no new expectation and deliberately uses prose
  rather than the `expected-command:` key, which `tests/test_corpus_snapshot.py`
  and `tools/check_rust_cli_snapshot.py` consume.
- `rust/fslc/src/outcome.rs`: one definition of the success and failure
  classes over the CLI's `result` vocabulary (issue #537 C2,
  `docs/DESIGN-rust-component-internals.md` "Outcome classification"). The
  crate previously had none — each command arm compared `result` against
  literals at its own return point, so the class was re-derived, and
  re-forgotten, once per family. `outcome_class` takes the whole envelope
  rather than the `result` string because five values carry their verdict in
  a sibling field (`approval check`'s `status`, `fmt --check`'s `changed`,
  `lint`'s `finding_count`, `diff`'s `violations` and `gate.passed`) — the
  same reason `docs/LANGUAGE.md`'s exit-code table excludes
  `approval_check`/`approval_diff`. Unknown values classify as `Failure`,
  following `mutate_exit_status`'s `_ => 3`: a result nobody registered now
  fails loudly instead of exiting 0, which is how #554 arose. Cacheability
  stays a separate predicate (`verify_cache_admits`), because `violated` is
  failure-class *and* cacheable.
- `rust/fslc/tests/fault_operators/` and `tools/run-fault-operators.sh`:
  implementation fault operators (issue #537 C5,
  `docs/DESIGN-conformance-harness.md`). `injection_detector_matrix.rs`
  calibrates detectors against defective *specs*; this answers the other
  question — would the test suite notice if the *verifier* started lying?
  Every escaped defect in the 2026-07 batch was of that kind (#601's
  `_ => 0`, #600's `violated`-only fold, #496's dropped explicit input), and
  none of them lives in a `.fsl` file. Each operator is a **patch file, not
  code**: the harness copies the working tree to a scratch checkout, applies
  one minimal diff, rebuilds `fslc` there, and requires that operator's
  primary detector to fail while its blind detector still passes. No
  fault-injection hook exists in the shipped binary, under a feature flag or
  otherwise — a switch that makes a verifier lie about verdicts must not exist
  in the codebase whose purpose is to prevent exactly that. Two controls make
  the matrix readable rather than decorative: a no-op patch must leave every
  named detector green, and a stale-seam patch must be *refused*, which keeps
  "a patch that no longer applies is a loud failure, never a skip" verified
  instead of merely intended. Wired into `tools/check-native-integration.sh`
  as its own `fault-operators` phase, part of `all` and deliberately not part
  of `rust`, which every pull request runs. CI runs it as its own post-merge
  `fault operators` job, required by the `product gate` aggregator: a matrix
  that never runs is worse than one that skips, because it rots into decoration
  while reporting green. Adding an operator is a patch file plus one row in
  `operators.txt` — data, no harness code, and `CONTRIBUTING.md` now asks for
  one whenever a fixed escaped defect's class can recur at a sibling seam.
  One blind detector is the public Kernel v2 golden contract, whose
  `spec.source.file` is repository-relative; naming it makes the no-op control
  prove the scratch checkout is a *faithful* copy, not merely a compiling one.
  It caught the first such infidelity immediately: `portable_cli_source_path`
  finds the repository root by walking ancestors for a `.git`, and a scratch
  tree without one escaped upward into the enclosing worktree, prefixing every
  such path with `rust/target/fault-operators/checkout/`. The harness now gives
  the scratch tree a repository root of its own.
- `rust/fslc/tests/issue_600_db_check_folds_kernel_verdict.rs` and
  `rust/fslc/tests/fixtures/issue_600_db_inconclusive_kernel.fsl`: the
  rejecting control for #600 below, plus a guard test that fails loudly if
  the fixture stops producing an inconclusive kernel — otherwise the control
  would start passing vacuously.
- `rust/fslc/tests/refine_corpus_parity.rs`: a native port of
  `tools/check_rust_refinement_parity.py`'s 6 corpus refinement-mapping
  regressions (`specs/cart_refines.fsl`, `specs/seat_refines.fsl`,
  `examples/gallery/errors/refinement_failed_map.fsl`,
  `examples/gallery/adversarial/refine_mapping_boundary_map.fsl`, and 2
  `examples/refinement_liveness/*_progress_refines.fsl` files). The Python
  script checked both the `fslc refine` stdout envelope and the process
  exit code, but was never called from any CI workflow or
  `tools/check-native-integration.sh`, so the native `refine` path's corpus
  coverage ran nowhere — a structural gap that let #466, #483, #493, #494,
  and #512 escape as false greens on this path (issue #593). The new test
  is picked up automatically by `cargo test --workspace`, which both
  `tools/check-native-integration.sh` and CI already run.
- `rust/fslc/tests/corpus_check_sweep.rs::check_result_and_exit_status_never_contradict`:
  an oracle-free Verdict Conservation Law check (issue #537 C2) over every
  `.fsl` file under `specs/` + `examples/`. It holds no per-file expectation;
  it only requires that a success-class `fslc check` result exit 0, that
  every other (including any future, unlisted) result exit non-zero, and
  that the stdout envelope is a JSON object carrying a string `result`
  field. `corpus_check_sweep.rs`'s existing sweep only ever inspected the
  JSON body, never the process exit status, so a `result:"error"` envelope
  that exited 0 would previously pass unnoticed.
- `rust/fslc/tests/corpus_check_sweep.rs::ledger_exit_status_agrees_with_its_verify_baseline`:
  extends the same Verdict Conservation Law (issue #537 C2) to `fslc ledger`.
  Unlike `check`, `ledger`'s JSON envelope always reports
  `result:"generated"` regardless of the verification baseline it embeds, so
  there is no `result`-string vocabulary to allowlist; instead this runs
  `fslc verify` at matching `--depth`/`--deadlock ignore`/`--engine bmc`
  arguments as an independent process and requires the two commands'
  exit-code class (0 vs non-zero) to agree for every corpus file.

### Fixed
- `--strict-tags`' `untagged` hint proposed the non-canonical form. It read
  `add a declaration tag such as "REQ-1: original requirement"; use "MODEL: ..."
  or "ASSUME-1: ..."` — the `"ID: text"` string slot that
  `docs/DESIGN-id-policy.md` classifies as migration input and that `fslc lint`
  reports as `legacy_string_metadata` with a machine-applicable
  `@requirement(...)` replacement. The diagnostic that fires at the exact moment
  an author is choosing a tag was therefore teaching the form the next gate
  rejects, which no amount of documentation elsewhere can outweigh. The hint now
  names `@requirement("REQ-SCOPE-001", "original requirement")`, the
  `MODEL-`/`ASSUME-`prefixed id for modeling intent, and process `covers` for the
  business/requirements dialects. Native only; the frozen Python reference keeps
  its wording, and no parity gate, test, or snapshot compares this string.
- `tools/run-fault-operators.sh` applies operator patches with `git apply`
  instead of the system `patch`. The matrix's first CI run failed (#613):
  BSD `patch` on macOS accepted the no-op control's end-of-file hunk while
  GNU `patch` on `ubuntu-latest` rejected the identical hunk against
  identical bytes, so the calibration was green locally and red on the
  runner. A harness whose verdict depends on where it runs calibrates
  nothing; `git apply` is one implementation wherever git is.
- The agent skills now state that typed link as the only canonical form.
  `skills/fsl/SKILL.md` had a worked example writing
  `invariant OnePerUser "ASSUME-1: ..."`, `skills/fsl-design/SKILL.md` — the
  design layer's own skill, where a design declaration links back to a
  requirement ID the requirements spec owns — said nothing about requirement
  links at all, and `@requirement` appeared nowhere outside the on-demand
  `reference.md`. The skills' tagging gate was `--strict-tags` alone, which only
  asks whether a tag exists: it counts a legacy string as tagged. `fslc lint`,
  the command that actually rejects the non-canonical form, was named in no
  skill; it is now part of the gate in `fsl/SKILL.md`, `fsl-design`,
  `fsl-requirements`, and `fsl-delivery`.
- `fslc db check` reported `result:"verified_under_assumptions"` alongside a
  non-zero exit whenever its nested kernel `verify` came back
  `unknown_cti`/`reachable_failed`/`unknown_budget` (issue #600).
  `run_db_check` folded only `violated` into the top-level verdict, so the
  exit code was right and the envelope contradicted it — the half of the
  Verdict Conservation Law a gate reading the JSON sees. Its
  `run_domain_check` sibling has folded every non-passing kernel verdict
  since #515; `db` now matches it, and its status guard admits both
  definitive codes (`status != 0 && status != 1`) instead of testing `== 2`,
  which had let a `kind:"internal"` kernel envelope be absorbed as a
  `dbsystem` verdict.
- `wrap_specialized` (the `ai`/`domain`/`db`/`agent` aggregation, 15 call
  sites) ended its status match in `_ => 0`, so every specialized-dialect
  result nobody had added to its failure list exited 0 (issue #601) — the
  shape #554 arose from, and the opposite default to `outcome_class`'s
  `_ => Failure` in the same crate. `check_ai` forwards its nested kernel's `result` verbatim when
  it has no finding of its own, so an inconclusive kernel verdict reached
  that arm. It now derives the status from `outcome_class`. A differential
  sweep of the pre- and post-change binaries over `examples/` + `specs/`
  found no corpus input that reaches it, so this omission is latent, unlike
  its `db check` sibling above. Unlike #594 and #600, which are each a single
  missing value, this was the mechanism that loses them: with the default on
  the success side, any failure value added to any specialized dialect exits
  0 from the moment someone forgets to register it.
- `fslc sweep`'s minimal-counterexample selection carried a hand-written
  failure list that had silently lost `unknown_budget` (issue #594), so a
  grid whose only failing scope reported it would collapse into
  `sweep_passed`/exit 0. It now defers to `outcome_class`. The omission was
  latent rather than a live false green: `sweep` restricts `--engine` to
  `bmc|induction`, `unknown_budget` is produced only by the explicit engine,
  `--engine auto` falls back to BMC before it escapes, and the verify-cache
  key includes the engine — so the control for this one is at the classifier
  (`outcome.rs`), not end-to-end.
- `rust/fslc/tests/corpus_check_sweep.rs` no longer carries
  `CHECK_SUCCESS_RESULTS`, its own two-value copy of the success class
  (issue #596). `check_result_and_exit_status_never_contradict` now calls
  `fslc_rust::outcome::outcome_class`, so the law is stated against
  production code instead of against a list in the test — the stale-check
  shape #577 retired 28 instances of. `chain_layer_passes`,
  `verify_cache_store`, and `mutate_exit_status` were migrated to the same
  definition; `chain` and `analyze` batch now each state their
  empty-workset choice (reject vs allow) as a named local, with the
  asymmetry recorded rather than removed.
- The agent skills now state that a requirements spec's inline
  `implements Abs from "business.fsl"` seam is the one exception to the
  exit-code contract: `check`/`verify` run the refinement to the business
  layer but report the verdict only in the `implements` field, so a
  `refinement_failed` / `impl_violated` seam still returns `result: "ok"` /
  `"verified"` and exit 0, while the same failure is exit 1 through
  standalone `fslc refine` and through `fslc chain`. `skills/fsl/SKILL.md`
  previously stated the exit contract with no exception, and its diagnostic
  table keyed the row on `implements.result: violated` — a value the verdict
  vocabulary never produces (`refines` / `refinement_failed` /
  `impl_violated`), so an agent searching the JSON for `violated` would miss
  a broken seam entirely. `skills/fsl-delivery`'s Requirements stage gate and
  both skills' "green `implements`" claim-discipline lines now say to assert
  `implements.result == "refines"` rather than relying on the exit code. Only
  `docs/DESIGN-design-family.md` ("treat `implements.result != refines` as
  failure even when the top-level process exits zero") had carried this, and
  it never reached the skills or `docs/LANGUAGE.md`. `docs/LANGUAGE.md` and
  `docs/LANGUAGE.ja.md` now state it too, beside the `implements` description
  and naming it the one exception to their exit-code table: the skills derive
  from the language contract, so recording it only in the skills would leave
  the canonical source still claiming the contract holds without exception.
- `rust/fslc/tests/corpus_check_sweep.rs`'s module doc claimed (per issue
  #483) that "the gallery fixtures and `examples/refinement_liveness/*`"
  are refined by a test; only 6 of the 28 `refinement`-dialect corpus files
  are, and that exercise was never wired into CI. Reworded to name the 6
  covered files and their owning test, `refine_corpus_parity.rs` (issue
  #593; same "comment claims a contract the implementation does not back"
  shape as #585).
- `fslc ledger` now exits non-zero when the verification baseline it embeds
  is not itself success-class (`violated`, `reachable_failed`,
  `unknown_cti`, `unknown_budget`, or `error`), instead of unconditionally
  reporting `result:"generated"`/exit 0 regardless of the child verdict
  (issue #592). The rendered ledger content is unaffected -- a violated spec
  still produces a full audit ledger with the violation embedded -- only the
  process exit code changes, reusing `mutate_exit_status`'s existing
  `docs/LANGUAGE.md` exit-code mapping rather than adding a second one. This
  only changes `fslc ledger`'s own direct exit code; `approval create`/
  `check`'s internal reuse of the same rendering (`generate_unapproved_ledger_report`,
  which exists purely to reproduce a target for a digest comparison) is
  unaffected, since a verdict mismatch there is legitimate drift evidence,
  not a hard error.
- A CDP timeout in the browser parity gate now reports the call ordinal, the
  socket's `readyState`, how many other requests were outstanding, and Chrome's
  own stderr, instead of only the method and the elapsed budget. The stall it
  catches is intermittent — one failure in five full local runs, none in CI —
  and is not reproducible on demand, so the single message an occurrence prints
  is all the evidence there will ever be, and the previous message could not
  separate "Chrome never became usable" from "the page stopped answering
  mid-poll". Call id 1 is `Runtime.enable`, so an id-1 timeout indicts startup
  while a later id indicts the page; an `OPEN` socket means nobody answered
  while `CLOSING`/`CLOSED` means the transport went away and the close handler
  lost the race. No retry was added: retrying an unexplained stall would
  suppress the event this exists to capture, and a genuine hang would return as
  a slow pass rather than a failure (#587).
- `test: add native FSL self-conformance anchors` (#537 C7) connects real
  native CLI JSON and process exits to the `fslc_session` and `fslc_monitor`
  replay models, and adds `fslc_fold.fsl` for sticky compound-verdict
  aggregation across sweep, chain, and analyze batch. Independent,
  fail-closed mapping tables never import `outcome.rs`; real passing/failing
  observations, synthetic contract violations, mutation/vacuity evidence, and
  an issue #554 exit-status sensitivity assertion provide both positive and
  negative controls.

## [4.0.0] - 2026-07-27

### Changed
- Pull requests into `main` now run the complete Linux product evidence — `rust workspace`
  and `WASM` — instead of deferring it to after the merge. Only the cross-platform
  `native Z3 4.16` matrix (macOS, Windows) stays post-merge under `FSL_OPTIMISTIC_CI`. The
  bounded `merge readiness` lane is kept as a sub-minute fail-fast check; adding
  `--all-targets` to its `cargo check` was tried and reverted after measuring 12m42s in
  CI, since `rust workspace` now compiles and runs those targets on the same pull
  request. 103 of the repository's 124
  integration test files live in `fslc`, `fsl-tools`, `fsl-verifier` and `fsl-lsp`, none of
  which `merge readiness` runs, so that evidence only reached `main` post-merge
  (`docs/DESIGN-ci.md`).

### Fixed
- `fslc ai check` now rejects duplicate `dataset` and `evaluator` declaration
  names, which the frozen reference has always rejected
  (`src/fslc/ai_project.py:237-241`). Native validated only three of the five
  declaration kinds, so a project declaring `dataset Shared` twice exited 0 and
  reported `datasets: ['Shared', 'Shared']`. Because a `dataset X` reference
  resolves by name, two declarations sharing one made resolution depend on
  declaration order — the false green was silently picking a winner, not merely
  echoing a name twice (#571).
- Native now emits `kind:"name"` for name-resolution failures instead of
  collapsing them into `semantics`, making every member of the
  `docs/DESIGN-v1.md` §7.2 closed set reachable. `duplicate state variable`,
  `duplicate enum member`, `duplicate def`, `duplicate parameter in def`,
  `def ... parameter is shadowed by binder`, and `undefined predicate` were all
  reported as `semantics`, sending an agent following the §8 repair protocol
  down the wrong branch. `CoreError` and `ModelError` now carry the
  classification the frontend already determined, and it survives through
  `SpecLoadError` to the one renderer — the same direction as #484, rather than
  adding patterns to the message-string classifier, which has no text these
  diagnostics share and would silently reclassify them on any wording change.
  Messages are byte-identical and no other `kind` moves.
  `examples/gallery/errors/name_duplicate_state_variable.fsl` is the corpus
  golden the classification never had; its absence is why this survived #484 and
  #555. Its `loc` now names the redeclaration rather than the first binding:
  the diagnostic carried no span of its own, so it fell back to the
  message-derived heuristic, which finds the *earlier* `x` and pointed a repair
  agent at the innocent declaration (#565).
- A `duplicate enum member` diagnostic now points at the repeated member rather
  than the first declaration of that name. `SpecItem::Enum` carried no
  per-member span, so the report fell back to `source_diagnostic`'s
  message-derived heuristic — the first token matching the quoted name — which
  for a *duplicate* is the earlier, innocent declaration by construction. For
  `enum E { A, B } enum F { B, C }` it named `B` in `E` (2:15) instead of the
  redeclaration in `F` (3:12). A `loc` that exists but names the wrong construct
  is worse than none: `docs/DESIGN-v1.md` G2 assumes the position is correct.
  `SpecItem::Enum` now carries `member_spans` positionally parallel to
  `members`, the same shape `SpecItem::Struct` gained in #555 and the same the
  domain surface already used, and the diagnostic attaches the offending
  member's own span (#576).
- The native<->Worker parity corpus's `unsupportedDocuments` exclusions in
  `rust/fsl-wasm/test-browser.mjs` are now self-retiring. The map excluded 32
  refinement/agent/causal documents from the comparison and recorded only a
  document type, so if the Worker ever gained a verb for one of them the
  exclusion would keep suppressing the comparison forever -- the shape that
  left #556's divergent path with zero corpus coverage. Each entry now carries
  the measured reason it holds, and every excluded document is probed on the
  Worker alone: the recorded premise is that the Worker cannot analyze it, and
  the day that stops being true the harness fails and names the entry to
  remove. The compared-pair count is unchanged at 351 -- no exclusion was
  retired and no document newly compared -- and the run now also reports
  `exclusionProbes`. This stays a capability exclusion, not a
  tolerated-difference allowlist: the envelopes are still not compared for
  these documents and no verdict, location, or exit-code difference is
  allowlisted (#568).
- The 28 `refinement`-typed entries in `rust/fsl-wasm/test-browser.mjs`'s
  `unsupportedDocuments` exclusion map are retired; the compared native<->Worker
  parity corpus grows from 359 to 415 cases and `exclusionProbes` drops from 32
  to 4 (agent + 3 causal only). The exclusion premise was measured stale: #574
  gave native and the Worker one shared classifier (`kernel_load_error`) for a
  document whose top level parses but is not Kernel-shaped, so `check`/`verify`
  on every refinement document in the corpus now produce byte-identical
  `semantics`/"spec has no state block" envelopes on both surfaces -- the
  exclusion was suppressing zero divergence. `specs/cart_refines.fsl` is now
  asserted by name to remain a compared parity case, the same way
  `duplicateWriteCase`/`governanceErrorCase` are, so a future silent
  re-exclusion fails loudly instead of only showing up as a quiet drop in
  `parityCases.length` (#577).
- Every parity-corpus exclusion is now checked to have native answer
  non-`error`, before the Worker launches. This closes the blindness that let
  the 28 retired entries go stale: #568's probe tests only the *Worker* side
  (`result !== "error"`), which detects agreement only when native answers
  non-`error` — agreement then forces the Worker non-`error` too. Native
  answered `error` for all 28 refinement documents, so two sides erroring
  looked identical whether or not the errors matched, and the probe stayed
  green for weeks. Native answers non-`error` for all 4 surviving exclusions
  (`ok` for agent, `causal_model_checked` for the 3 causal), so the cheap
  Worker-only probe is sound for them — and the day an exclusion is added
  whose native side errors, this assertion names it instead of silently
  reverting the corpus to that blind state. Full envelope comparison was
  evaluated and rejected: the causal envelope carries no `versions` block, so
  `validateEnvelope` throws before any comparison, which is a normalizer
  rejection rather than a verdict. The assertion runs in the candidate loop
  rather than beside the Worker probe because it depends on nothing but
  native, turning a ~6-minute failure into a sub-second one (#577).
- The browser parity harness no longer embeds a developer's absolute path.
  `rust/fsl-wasm/test-browser.mjs` hardcoded one machine's
  `/Users/<name>/Library/Caches/ms-playwright/chromium_headless_shell-1208/...`
  Chrome, the only committed line in the repository containing an absolute home
  path, which `AGENTS.md` forbids. Playwright installs each build under its own
  version directory and has changed the internal layout between them — both
  `chrome-mac/headless_shell` and
  `chrome-headless-shell-<platform>/chrome-headless-shell` exist on a machine
  with two builds installed — so the version, the platform directory, and the
  binary name are all discovered rather than spelled out, newest build first
  (#583).
- A dropped or half-open Chrome DevTools Protocol connection no longer hangs
  the browser gate forever. `cdp()` returned a promise that only the message
  listener could ever settle, with no timeout and no socket `close`/`error`
  handler, so a response that never arrived hung Node indefinitely; the
  surrounding `for (attempt < 360)` loop is not a bound when a single `await`
  inside it never returns. Because the hang left the process to be killed
  externally, the `finally` that terminates Chrome and removes the profile
  directory never ran: an orphaned `chrome-headless-shell` (parent gone,
  4h13m old) and seven leaked profile directories were measured on one machine,
  and an orphan over two hours old blocked a later gate. `cdp()` now times out
  naming the method, and both socket `close` and `error` reject every
  outstanding request, so the failure is loud and the cleanup runs (#584).
- `rust/fsl-wasm/web/cases.mjs` no longer claims its smoke cases are compared
  against native. The native comparison keyed on them (`nativeVerdict`) was
  removed in `d30f456` when the corpus-wide parity run replaced it, but the
  now-unused `import { cases }` in `test-browser.mjs` and the header comment
  promising "the parity comparison stays honest" both survived, so the file
  documented a check that no longer existed. The import is removed and the
  comment states what is true: these 9 inline sources are Worker smoke cases
  checked in the browser against their own `expected`, and the 415-case corpus
  comparison supersedes the removed one (#585).
- A kernel-stage failure inside a `use ... from` component now reports the
  parent's `use` declaration as its `loc`, and names the component's own path
  and position in the message. The two used to be mixed: the path came from the
  parent (`source_file`) while the line and column came from the component's
  parser, so `check` on
  `examples/gallery/errors/semantics_compose_component_parse_failure.fsl`
  reported `…:7:18` — and line 7 of that file is a comment. A location in the
  wrong file is worse than none, and `docs/DESIGN-v1.md` G2 requires the output
  JSON alone to say where the problem is. `loc` is `{line, column}` with no
  `file` in all five of its `docs/DESIGN-v1.md` examples, so it can only mean a
  position in the file the envelope is about; the `use` declaration is that
  position, and it is also the line to look at first. The message now reads
  `component "<component>" failed to parse (<inner> at <component>:<l>:<c>) at
  <parent>:<use line>:<use column>`, naming both files explicitly. An
  unreadable component is re-anchored the same way, instead of reporting `1:1`.
  Native and the browser Worker share the loader, so both return the identical
  string; the file is a parity corpus case, so
  `./tools/check-native-integration.sh` checks that on every run (#567).
- The fsl-ai project check's unexecutable-`require` spec error (#542) now
  carries a `loc` pointing at the offending clause's own line and column.
  `rust/fsl-syntax/src/ai_project.rs` tracked no positions at all, so the error
  named the declaration and slice but no position, breaking
  `docs/DESIGN-v1.md` §7.2's guarantee that every `parse` error has one. Block
  bodies and their statement lines now carry their char offset through
  `top_blocks`/`top_lines`, each metric/observed requirement records the
  position of the clause that produced it, and both `fslc ai check` and the
  `fslc check` dispatch emit it. A clause nested in a `slice` reports its own
  line, not the enclosing block's. Which projects are accepted is unchanged
  (#562, partial -- see below).
- **Not fixed, recorded:** an unknown *non-*`require` line inside a declaration
  body is still silently ignored, so acceptance still turns on the line's first
  word. Closing that requires deciding a closed body grammar, and no current
  source specifies one: the frozen `src/fslc/ai_project.py` ignores unrecognized
  lines by construction (an `if`/`elif` chain with no `else`) and downgrades an
  unparseable `require` to `kind="inconclusive"` rather than erroring;
  `docs/LANGUAGE.md` and `skills/fsl/reference.md` state only the `require` rule
  #542 added; and `skills/fsl/reference.md` documents unvalidated block bodies
  as intended for `ai_action`/`retriever`/`trust_boundary`/`authority`. The one
  corpus project file additionally uses free-form predicate lines
  (`language in ["ja", "en"]`, `condition output.claims not_supported_by
  retrieved.sources`) that both implementations store verbatim and never parse
  (#562).
- **Breaking (input).** `true`, `false`, and `none` are now rejected as
  declaration names; a specification that used one will stop checking. They
  always resolve to the literal in an expression, so a declaration with one of
  those names was unreadable from every expression — and the misreading was
  silent, not an error. `state { true: Bool }` with
  `invariant AlwaysHolds { true }` returned `ok` from `check`, `verified` from
  `verify`, and **`proved`** from `verify --engine explicit`, while
  `init { true = false }` assigned a variable nothing could read: the author
  read "the variable holds" and the verifier proved the literal. All three
  engines agreed, so symbolic/concrete/BFS agreement did not catch it.
  `none` appeared to be rejected already, but only incidentally — the error came
  from Option-type inference failing on a bare `none`, so
  `state { none: Bool, slot: Option<Bool> }` with `slot = none` checked `ok` and
  silently assigned the literal. The check now covers the specification name,
  `const`, `def` and its parameters, `type`, `enum` and its members, `struct`
  and its fields, state variables, actions and their parameters, properties
  (including `leadsTo`), quantifier and aggregate binders, and `is some(x)`
  pattern bindings. The reserved set is derived from the native expression
  parser rather than copied from the frozen reference: only those three
  identifiers are matched unconditionally as literals, so `count`, `sum`,
  `stage`, `in`, `is`, `where`, `old`, `abs`, `and`, and `or` remain valid names
  and every `.fsl` under `specs/` and `examples/` still checks unchanged.
  The diagnostic is `kind: "semantics"` with a `loc`; `docs/DESIGN-v1.md` §7.2
  puts it in the `name` class, which native does not yet emit anywhere (#565)
  (#570).
- `fslc analyze` on a `.toml` project manifest now emits the same TSG
  vocabulary a standalone file emits for the same source. The manifest loop
  built each layer with `fsl_tools::build_tsg` alone, which only sees the
  lowered `KernelModel`, so none of the source-only kinds ever appeared on that
  path — `acceptance`/`forbidden` scenario nodes with their `covers` and
  `starts_with`/`precedes` edges, and `control` catalog nodes — and the graph
  vocabulary silently depended on which input form named the spec. Each layer
  now runs the same source-level enrichment, on its own file
  and its unprefixed graph, so `<layer>:` prefixing still applies to
  enrichment-added nodes and two layers declaring the same id stay two nodes.
  No ordering change was needed: `Builder::build` drops a `covers` edge whose
  target has no node yet and runs first, but the edges it computes come from
  `KernelModel::requirement_targets`, which enumerates only `init`, `action:*`,
  and `property:*` targets and can never name a scenario or a control, and
  every enrichment-added edge has both endpoints created by the enrichment
  itself.
  Verdicts and exit codes are unaffected, and no review finding changes — the
  manifest path accepts only `--projection traceability_graph`, so no structural
  detector runs there at all (#558).
- Native `fslc ai check` on an fsl-ai project now emits the six fields the
  frozen reference's `analyze_ai_project` emits and native omitted:
  `ai_project`, `assumptions`, `datasets`, `dialect`, `evaluators`, and
  `failure_modes`. `evaluators` and `failure_modes` were not merely
  unprojected — the Rust project parser did not descend into `evaluator` /
  `failure_mode` blocks at all, so the data did not exist; it now records their
  names, the only thing the reference projects. `skills/fsl/reference.md`
  already documented `failure_mode` as listed under `failure_modes`, so the
  skill AI agents read promised output native had never produced; the skill was
  right and the implementation moved to match it. Native's `ai check` output
  now equals the frozen reference on all 13 of its keys, and
  `tools/check_rust_phase3_commands.py`'s `ai-project-check` projection — which
  compared only six of them, and is why the gap went unnoticed — now compares
  the whole set (#563).
- `fslc html`'s property table now renders the "Deadline" column
  `docs/DESIGN-html-report.md` specifies. `grep -ci deadline
  rust/fsl-tools/src/html.rs` was 0: the caption half of that design paragraph
  landed with #525 and this was its unimplemented remainder, so a
  `leadsTo ... within` deadline was invisible in the report. The column is
  conditional, as the same paragraph's no-`none`-filler rule requires — it
  appears only when some property declares a deadline, a property that declares
  none gets an empty cell rather than a filler, and a spec with no deadline
  anywhere renders no column at all. `explain`'s `skeleton` properties now also
  carry `within` for a `leadsTo ... within`, additively and only when declared,
  matching the frozen reference's `_property_skeleton` (#564).
- **Breaking (exit code).** `fslc mutate` on a spec whose baseline already
  fails now exits 1 instead of 0. `mutate` re-emits the baseline `verify`
  envelope verbatim when the baseline does not verify, but derived its exit
  code from `result == "error"` alone, so every other non-success verdict fell
  through to 0 and `result:"violated"` returned a green exit — a mutation score
  is meaningless over a spec that already fails, and a gate reading only the
  exit code saw a pass. `docs/LANGUAGE.md`'s exit-code table maps `violated`
  to 1 with no per-command exemption, and `scenarios`/`testgen` re-emit the
  same envelope and already exited 1. The status is now a total match over the
  results a mutation run can carry (`mutate_exit_status`): `mutated`/
  `verified`/`proved` → 0, `violated`/`reachable_failed`/`unknown_cti`/
  `unknown_budget` → 1, `error` → the code the envelope was already classified
  with, and anything unmapped → 3 rather than silently 0. Mapping the whole
  vocabulary also closed a second reachable false green in the same command:
  `--from <unreadable file>` returned `result:"error"`, `kind:"io"` with exit 0
  and now exits 2 (#554).
- Native `fslc mutate` now defaults to 200 built-in mutants instead of 100,
  matching the `max_mutants` default its own published CLI contract
  (`rust/fslc/cli-contract.json`) advertises and the 200 fixed by
  `docs/DESIGN-mutate.md`, `skills/fsl/reference.md`, and the frozen
  `src/fslc/mutate.py` (`DEFAULT_MAX_MUTANTS`). For a model with more than 100
  candidates the smaller default silently evaluated a different mutant set and
  reported a different kill count, making native reports incomparable with the
  documented baseline and skewing hollow-spec triage. Explicit `--max-mutants`
  behavior is unchanged, and the runtime default is now a single
  `DEFAULT_MAX_MUTANTS` constant rather than a literal that can drift from the
  contract (#524).
- **Breaking (output).** `fslc html` no longer labels every property row with
  one report-wide assurance class. `html.rs::assurance` derived a single class
  from root completeness and `properties_section` reused it for every row, so
  under `--engine induction` a `reachable` and an ordinary unranked `leadsTo`
  both rendered `proved(induction)` even though k-induction ranks only
  invariants and transitions -- a reachability witness comes from the bounded
  base BMC. In an audit artifact "proved" claims all-depth universal evidence,
  so this was an assurance misstatement. Property rows now classify per element
  through the ledger's existing `formal_assurance` rule
  (`docs/DESIGN-assurance-classes.md`) instead of a second local
  classification, and the report-wide status row now shares the ledger's
  `assurance_token`/`assurance_label` pair as well. For
  `examples/pm/cancel_flow.fsl --engine induction`, `CanRetain`, `CanChurn`,
  `POL-1`, and `POL-2` change from `proved(induction)` to
  `bounded(BMC depth 8)`. The same renderer also never called
  `requirement_caption` for property rows, so invariant/reachable/leadsTo rows
  dropped the requirement text actions and counterfactuals keep; each row now
  renders its own caption under the declaration name, escaped, and omits it
  entirely when the property carries no requirement tag
  (`docs/DESIGN-html-report.md`) (#525).
- **Breaking (output).** `fslc ai check` and the `fslc check` dispatch for
  fsl-ai project files no longer accept a declaration whose `require` clauses
  are unparseable. Both ran `ai_project_summary`, a line scanner that
  collected declaration *names* by string prefix and never read a clause body,
  so a project whose clauses match no known evidence grammar returned
  `ai_project_analyzed` / `findings: []` / exit 0 from `check` and was then
  rejected by `eval`/`regress`/`drift`/`compat` -- the confidently green false
  negative `AGENTS.md` calls more dangerous than a crash. The check stage now
  reports from `fsl_syntax::parse_ai_project`, the parser those commands
  already execute, and an unexecutable `require` clause is a spec error
  (`result:"error"`, `kind:"parse"`, exit 2) naming the declaration, slice, and
  clause text, matching `docs/LANGUAGE.md`'s exit-code table and the exit 2 the
  same dialect already returns for non-AI `ai compat` input and an unknown
  `--property`/`--migration` selection. Reporting from the parser also corrects
  two `ai check` output fields: `components` is now the declared `ai_component`
  names (`[]` when none, previously the scanner's placeholder `[""]`), and
  `raw_blocks` entries carry `{kind, name}` per block as
  `skills/fsl/reference.md` specifies (previously `{kind}` only, deduplicated
  by kind). Raw blocks stay unvalidated: garbage inside `ai_action`,
  `ai_contract`, `authority`, `retriever`, or `trust_boundary` still passes
  `check`, because those are contractually block boundaries only (#542).
- `type` and `semantics` errors now carry `loc` on every spec-reading command,
  completing the `docs/DESIGN-v1.md` §7.2 clause that guarantees a location for
  `parse`/`name`/`type`/`semantics`. Issue 484 delivered the `parse` half; the
  other half returned `loc: null` everywhere, including `check`, so the §8
  repair protocol could not mechanically locate a `type`/`name` failure from
  the output JSON alone. `SpecLoadError::Semantic` now carries the structured
  diagnostic instead of a flattened `String`, and `ModelError` carries the span
  of the construct that failed — a kernel `spec` registers no lowering origins,
  so the three corpus goldens previously had no location to report at all. The
  reported position is the offending construct, not the enclosing declaration:
  the state field for an unknown type, the `struct` declaration for a
  non-scalar field (`TypeExpr` carries no span of its own), and the *second*
  write for a duplicate assignment. Messages are unchanged — the span is a new
  field that `ModelError`'s Display deliberately ignores — and no `kind`
  moves. A diagnostic that names no construct in the reported file still omits
  `loc` rather than emitting a `{line: 0, column: 0}` placeholder;
  `docs/DESIGN-v1.md` now states that exception explicitly instead of leaving
  the clause overstating the implementation (#555).
- The browser Worker no longer classifies a kernel-stage load failure
  differently from the native CLI. `rust/fsl-wasm`'s `build()` hard-coded every
  `fsl_core::parse_kernel_source_with_file` failure to `kind: "parse"`, while
  native carried the same `CoreError` through `kernel_load_error` and reported
  `semantics`/`type`/`name` — the same input classified two ways depending on
  which surface ran it, which `AGENTS.md` forbids allowlisting and which makes
  `docs/DESIGN-v1.md`'s closed `kind` set meaningless. The classifier and its
  render dispatch moved out of the `fslc` binary into `fslc_rust::spec_load`
  (`SpecLoadError`, `kernel_load_error`, `surface_parse_failure`,
  `render_spec_load_error`) so both surfaces run the same one; the Worker gained
  no classifier of its own, since a parallel classifier is what diverged.
  Native's classification is unchanged.
  Detection power was zero: `examples/gallery/errors/` had no input that fails
  at the kernel stage while its own top level parses — every such file in
  `specs/`+`examples/` is a refinement, agent, or causal document the parity
  harness excludes as unsupported — so the divergence was invisible to
  `./tools/check-native-integration.sh`.
  `examples/gallery/errors/semantics_compose_component_parse_failure.fsl` and
  its `semantics_compose_broken_component.fsl` helper close that hole, taking
  the native↔Worker parity corpus from 351 to 355 cases, and
  `rust/fsl-wasm/test-browser.mjs` now fails loudly if the case ever leaves the
  corpus (#556).
- Every spec-reading native command now classifies a syntax error the way
  `check` does: `kind: "parse"` with `diagnostic_code: "FSL-PARSE"` and a
  `loc`. `rust/fslc/src/main.rs`'s `load_kernel_model` flattened the frontend's
  structured failure into a `String`, so `kernel`, `conformance`, `scenarios`,
  `explain`, `analyze`, `typestate`, `ledger`, `testgen`, `document generate`,
  `diff`, `replay`, and `html` re-classified an unparseable file as
  `kind: "semantics"` with `loc: null`, and `verify`/`sweep` reported
  `kind: "parse"` with the `loc` dropped — breaking the schema guarantee in
  `docs/DESIGN-v1.md` §7.2 that `parse` always carries a location, and sending
  the §8 repair protocol into the wrong branch. The loader now returns a typed
  `SpecLoadError` (`Io`/`Parse`/`Semantic`) and every consumer renders it
  through the same `frontend_output::render_surface_parse_error` `check` uses.
  Exit codes and `result: "error"` were already correct and are unchanged.
  The browser Worker's `verify` moved with the CLI so the native/Worker parity
  contract still holds (#484, #497).
- A missing or unreadable specification is now `kind: "io"` for every command
  instead of falling through the message-string classifier to
  `kind: "semantics"`. `fslc analyze` disagreed with itself: one missing input
  reported `semantics` and the same missing input alongside a second file
  reported `io`. `check` and `verify` also no longer silently continue when the
  spec cannot be read (#497).
- `fslc mutate` on a specification that fails to load now exits 2 instead of 0.
  A non-`verified` baseline still scores 0, but a spec error keeps its own exit
  code, as `docs/LANGUAGE.md`'s exit-code table requires with no per-command
  exemption; exit 0 on an unparseable spec was a green gate over a
  specification that was never analysed (#484).
- `rust/fslc/tests/testgen_contract.rs`'s
  `symlink_source_name_and_canonical_pytest_path_remain_distinct` no longer
  panics with `create output-parent symlink: AlreadyExists` on a repeated
  local run whose previous invocation was interrupted before its own
  trailing cleanup ran: the fixture directory and both symlinks it creates
  now live under a fresh, uniquely named scratch directory (same idiom as
  `rust/fslc/tests/chain_cli.rs`'s `scratch_dir`) instead of a fixed, reused
  path, so there is nothing left over to collide with on the next run
  regardless of how the previous one ended (#539).
- `rust/fslc/tests/domain_codegen_contract.rs`'s
  `all_five_public_kernel_domain_targets_match_pre_migration_goldens` and
  `every_valid_domain_corpus_entry_generates_all_five_targets` no longer
  intermittently fail (a golden digest mismatch or a
  `clear generated directory: No such file or directory`) when cargo runs
  them in parallel in the same test binary, the default: both used to
  `exists()`-then-`remove_dir_all()` a directory under a shared
  `rust/target/domain-codegen-contract/` parent before generating into it,
  leaving a window one test's cleanup could race against the other's
  read/write. Each generation call now gets its own uniquely named scratch
  directory (same idiom as `rust/fslc/tests/chain_cli.rs`'s `scratch_dir`),
  which needs no `exists()`/`remove_dir_all()` step at all — closing the
  race window rather than narrowing it (#546).
- Native `reachable(r, a, a)` is now non-reflexive on every engine: true
  only via a real path of one or more edges back to `a`, never a free
  zero-hop `a == a` step. `rust/fsl-runtime/src/lib.rs`'s concrete
  `relation_reachable` (used by the `--engine explicit` BFS oracle, and by
  the default engine's own trace-replay consistency check) started its BFS
  frontier at `[source]` and checked `current == target` before traversing
  any edge, so it reported self-reachability as trivially true for *any*
  relation, including an empty one -- a free zero-hop step the symbolic
  evaluator's convention never took. Neither `docs/LANGUAGE.md` nor
  `docs/LANGUAGE.ja.md` previously stated whether `reachable` was
  reflexive; both now document the non-reflexive contract explicitly.
  Matches the frozen Python reference's `_relation_reachable`
  (`src/fslc/runtime.py`) and native BMC exactly -- confirmed by direct
  code reading and by running the frozen `verify`/`scenarios` commands
  against the same repro spec, both `violated_at_step:1`, not the reflexive
  `0` the pre-fix native concrete engine reported. Before the fix an empty
  relation disagreed between engines (`violated_at_step` 0 vs. 1) and a
  multi-hop-cycle spec made the default engine's own trace-replay
  consistency check fail outright (`kind:"internal"`,
  `"trace state mismatch"`, exit 3), since BMC's concrete replay of its own
  symbolic counterexample used the same disagreeing evaluator (#502).
- `relation A -> B` state is now usable end to end in the native symbolic
  verifier: `r = Set {}` types and evaluates as the empty relation (both
  `fslc check` and the concrete Monitor previously rejected or mistyped it),
  and `fslc verify`/`fslc mutate` implement all seven relation operations
  (`.contains(a, b)`, `.add(a, b)`, `.remove(a, b)`, `reachable(r, a, b)`,
  `acyclic(r)`, `functional(r)`, `injective(r)`, `domain(r)`, `range(r)`),
  matching the frozen Python reference's semantics (`reachable`/`acyclic`
  require a self-relation and are rejected otherwise; a memoized bounded-hop
  closure avoids the unmemoized-blowup class of bug already fixed in the
  Python reference) (#467).
- Native `leadsTo` lasso and deadlock-stall search now applies the documented
  `symmetric type` / `symmetric enum` liveness symmetry reduction: the
  designated representative state (lasso loop head, or the stalled state) is
  constrained to the canonical permutation of each symmetric type's
  per-entity rows, built from `Map<SymmetricType, V>` and
  `Set<SymmetricType>` state (skipping any `V` that itself mentions a
  symmetric identity type), matching the frozen Python reference. A genuine
  per-entity violation (only one identity ever stalls) is still found under
  both `Map`- and `Set`-shaped symmetric state (#461).
- `verify --engine induction` now implements `leadsTo ... helpful` per-binding
  ranking proofs natively, matching the documented idiom for per-entity
  progress under interleaving. `fslc check` rejects a `helpful` action that
  names an undeclared action or has the wrong arity; ranking induction
  additionally proves the four `helpful`-specific obligations (matching
  action `fair`, an enabled matching instance whenever pending, stable
  enabledness across two or more `helpful` actions, and non-helpful actions
  neither dropping the pending obligation nor increasing the measure),
  reporting `progress_action_not_fair`, `helpful_action_not_enabled`,
  `helpful_action_enabledness_not_sticky`, `non_decreasing_helpful_action`,
  `non_helpful_action_increases_measure`, or `pending_not_preserved` on
  failure and echoing `helpful` on both the proof and CTI (#473).

### Added
- Native `verify` now reports the three remaining `docs/DESIGN-vacuity.md` §2
  lanes — `always_true_requires`, `tautology_over_frozen`, and
  `urgency_freeze` — closing the last of issue 465. A spec whose only emptiness
  was one of them previously came back `result:"verified"` / exit 0 even under
  `--vacuity error`; the three-line reproduction (a `Bool` set by `init`,
  assigned by no action, asserted as an invariant) now exits 2 with
  `kind:"tautology_over_frozen"` and `trace_type:"vacuity"`. All three are
  proved in `fsl-verifier`, so `fsl-runtime` stays solver-independent, and all
  three report identically on `--engine bmc`, `explicit`, and `induction`.

  Unlike the frozen Python reference, the native lanes decide "always true over
  all reachable states" over the **declared type space** rather than over the
  states an unrolling happened to witness, so no verdict moves with `--depth`.
  Python reports `examples/causal/funnel.fsl`'s `requires visits < 100` as dead
  at depth 8 because `visits` only reaches 8; with `visits: 0..100` declared,
  native correctly stays silent at every depth. The lanes are sound and
  deliberately incomplete: an unproven obligation, an `unknown` backend
  verdict, a compose-synchronized action, a generated declaration, and an
  action that was never enabled all yield no finding.
- Added an optimistic CI lane with one stable, parallelized `merge readiness`
  context for pull requests and merge queues, while preserving the complete
  Rust/WASM/macOS/Windows product gate for every merged `main` state and
  production promotion. Failed post-merge jobs now create or update
  deduplicated `ci/post-merge` issues and close them only after observed
  recovery; rollout is fail-safe behind the `FSL_OPTIMISTIC_CI` repository
  variable. The readiness compile lane deliberately excludes native-Z3 and
  default-feature builds, which remain mandatory post-merge evidence.
- Native `analyze`'s TSG projection now emits `requirement`/`acceptance`/
  `forbidden`/`kpi` nodes and `covers` edges for a standalone `.fsl`/
  requirements spec, not only for a `.toml` project manifest.
  `requirement`/`covers`/`kpi` are model-only (`build_tsg` itself now
  projects them from `KernelModel::requirement_targets`/`.projections`, so
  every dialect gets them); `acceptance`/`forbidden` scenario nodes need
  requirements-dialect source text (`fsl_core::requirements_trace_contract`)
  since they have no Kernel-lowered form, and gain a `covers` edge from any
  `@requirement(...)`-annotated requirement that names them. A requirement
  connected to nothing the graph represents still gets its own node with no
  `covers` edges, rather than being silently dropped, so
  `disconnected_requirement` (`--profile ai-review`) can detect it.
  `requirement_property_graph` and `--focus requirement:ID` previously had
  zero edges/always failed for a standalone spec; both now work (#495).
- Native `analyze`'s TSG projection now also emits the remaining documented
  vocabulary: `starts_with`/`precedes` step edges and `control` nodes. An
  acceptance/forbidden case gains a `starts_with` edge to the action its first
  step calls and a `precedes` edge to each later step's action, running
  scenario → action and carrying the 0-based `step` index (the direction and
  split the frozen reference fixes in `src/fslc/analysis/tsg.py`); the index is
  part of the edge id, so a case that calls one action twice keeps both edges.
  `control` nodes project governance/business `control ID "text"` catalog
  entries, which have no Kernel-lowered form. Because scenario nodes
  previously had no outgoing edge at all, `unanchored_property`'s
  scenario-anchor suppression (`scenario_actions && kind == "reachable"`) could
  never apply, so a `reachable` anchored by an acceptance scenario was reported
  as unanchored — a structurally guaranteed false positive that the step edges
  remove. No review finding reads `control` nodes yet; the edge vocabulary has
  no `satisfies` kind, so a control node carries only its `declares` edge
  (#495).
- Added an evidence-gated `fsl-design-family.v0` sidecar prototype with three
  maintained three-variant dogfood families, native check/verify/refine/diff
  orchestration tests, raw producer and deterministic digest controls, and an
  accepted agent/workflow-only decision without new language or CLI semantics
  (#427).
- Refinement mappings can declare a source-total `enum abstraction` and invoke
  it with `abstract(name, expr)` for nominal many-to-one mappings. Repeated and
  unused targets are allowed, while missing, duplicate, unknown, or
  wrong-nominal sources fail closed; concrete, symbolic, progress, CLI, Worker,
  Public Kernel projection, and raw-replay guards share the checked semantics
  without weakening bijective `enum conversion` (#455).
- Native `verify` now reports the `vacuous_leadsto` vacuity lane: a `leadsTo`
  whose trigger never becomes reachable within `--depth` is a hollow
  property, the same class BMC already reports for an implication invariant's
  unreachable antecedent (`vacuous_implication`). Detected via the same
  solver-free existential-reachability BFS `fsl-runtime::verification_warnings`
  already uses, so `fsl-runtime` gains no new solver dependency (#465, partial;
  `always_true_requires`, `tautology_over_frozen`, and `urgency_freeze` remain
  unimplemented on native — see `docs/DESIGN-rust-port.md` "Shared semantic
  diagnostics").

### Fixed
- The native LSP's document index no longer lets an `@` annotation swallow the
  declaration that follows it. `@requirement("REQ-COMMAND", "…")` shares its
  name with the `requirement NAME { … }` declaration keyword, and the index
  handled no `@` at all, so the annotation armed a pending declaration that
  nothing between there and the next identifier disarmed — the *next line's*
  construct keyword was consumed as the declaration name. In
  `examples/annotations/annotated_domain.fsl` that registered a Property symbol
  literally named `command` and left `command Place { … }`'s real name `Place`
  with no declaration at all, so definition/references/rename returned nothing
  and `documentSymbol` listed a construct keyword. 13 sites across the four
  `examples/annotations/*.fsl` files were affected, every one of them from
  `@requirement`. Nothing from `@` through the annotation's closing `)` may now
  declare anything or consume a pending declaration, in any dialect; non-keyword
  names inside an annotation stay references. Resolved by position, not by
  removing `requirement` from the declaration keywords, which would have broken
  the real declaration form (#551).
- `rust/fsl-lsp/tests/corpus.rs` now also asserts, over every corpus file, that
  a declaration keyword owning a name has actually declared that name
  (`DocumentIndex::misprojected_declarations`). The previous whole-corpus check
  only required every identifier to have *some* entry, which a swallowed name
  satisfies as a reference to nothing, so it passed while roles and scopes were
  wrong. This is the drift detector the `#504`/`#551` class of index bug had
  been missing (#551).
- The native LSP's document index now recognizes `def` declarations and their
  parameters, aggregate/quantifier binders written with a `name: Type` form
  (`count(c: Id where ...)`, `sum(...)`, `unique(...)`, `exactlyOne(...)`,
  `forall`/`exists`), `is some(v)` pattern binders, named `preservation NAME {
  ... }` governance blocks, and every name in a comma-separated `actor A, B`
  list — each previously indexed as an ordinary reference with no matching
  declaration, so `textDocument/definition`/`references`/`rename` returned
  nothing and `documentSymbol` omitted them. Also fixed a `reachable`/`domain`
  collision: both name a top-level declaration keyword (`reachable NAME {
  expr }`, `domain SpecName { ... }`) and a relation builtin call
  (`reachable(r, a, b)`, `domain(r)`); an unconditional keyword match
  previously consumed the identifier immediately following either builtin
  call as a brand-new self-defining declaration, corrupting the referenced
  state variable's definition/reference set (#504).
- `fslc db check` no longer reports a confidently green `verified_under_assumptions`
  over a genuine compatibility violation. `validate_db` now rejects a `check
  compatibility` rule name outside the closed vocabulary and an `environment
  schema lo..hi` the declared migration plan never reaches (exit 2 instead of
  silently checking nothing). `findings()` is rewritten around a materialized
  per-migration column-state simulation (mirroring the frozen Python
  reference's `_static_findings`) instead of syntactic pattern matching, which
  fixes: `set_not_null` without a prior `backfill` (previously made the
  violating kernel transition unreachable instead of reported);
  `rollback_not_equivalent` never firing for a lossy rollbackable
  split/merge (only `drop` was checked); `data_preservation_loss` never
  firing for dropping an existing column (only split/merge were checked);
  reads/writes checked against "a drop op exists somewhere in the source"
  instead of actual per-snapshot column state (missed an initially absent
  column with a live reader, and false-fired on a column dropped then
  re-added); and an offline payload's TTL window never extending its
  acceptance obligation past the emitting schema snapshot. An `active`
  artifact's own `calls`/`expects`/`emits_offline`/`requires` declarations
  are now checked too — previously only artifacts other than `active` were
  treated as consumers (#490).
- `fslc db check` no longer false-fires against already-annotated migrations
  and supported providers. A `supported` (not just `active`) artifact's
  `accepts`/`responds`/`provides` now counts toward compatibility (`may_exist`
  still does not); `drop ... destructive` is accepted alongside
  `irreversible`, and dropping an already-absent column no longer fires
  `destructive_migration_unannotated`; `split`/`merge ... irreversible` is
  accepted as a preservation classification alongside `lossless`/`lossy`; and
  every finding kind is now gated by its documented rule (`rule_enabled`,
  centralized instead of scattered per-finding conditionals), so an opt-in
  `data_preserved`/`rollback_equivalent` rule no longer fires when it was not
  selected (#491).
- `api_response_field_missing`'s `failed_rule` is now the documented
  `api_responses_expected` instead of the undocumented
  `api_response_fields_available` (#492).
- `fslc db observe` validates its observation envelope and every event
  against `schemas/fslc/db/observation.v0.schema.json` before evaluating them
  — a declared `schema_version` must equal `fsl-db-observation.v0`, and each
  event must be an object with typed required fields and a `capability` from
  the closed vocabulary — and fails with a located exit-2 error instead of
  defaulting a missing/mistyped field into a fabricated `unsupported_artifact_observed`
  witness (#505).
- `fslc db observe` now matches an event's `flags` snapshot against artifact
  window conditions the same way `fslc db check` does; an artifact observed
  under the wrong feature-flag variant is `unsupported_artifact_observed`
  instead of silently `observed_conformant` (#506).
- `fslc db import`'s SQL importer now lowers `ALTER TABLE ... DROP COLUMN`
  into a `drop ... irreversible` migration op as documented (previously
  unimplemented — the construct fell through to `unsupported_sql`), and a
  malformed `CREATE TABLE` (unbalanced/missing parentheses) now produces an
  `unsupported_sql` warning instead of being silently skipped with no warning
  and no table (#507).
- `fslc ai compat` no longer line-scans any readable file and reports
  `compat_profile_generated` with a syntactically empty `dbsystem` fragment
  (`artifact  { requires ; provides ; }`) for non-AI input or an fsl-ai
  project that declares no `ai_component` at all — indistinguishable from a
  genuine clean result. Native now parses either a single `ai_component`
  document or a full fsl-ai project (`fsl_syntax::parse_ai_project`,
  `AiComponent.tools[].schema` preferred over the tool name, matching the
  frozen reference) and rejects wrong-dialect input and a component-less
  project with exit 2 (#511).
- `fslc ai eval`/`regress`/`drift` now execute the selected
  `statistical_property`/`ai_migration`/`observed_property` declaration
  instead of aggregating records against two hardcoded example
  metrics/thresholds. `eval` applies every declared slice's
  `min_samples`/`ci_lower`/`ci_upper` gate and evaluator-trust check
  (`fsl_tools::evaluate_statistical_property`); `regress`/`drift` read the
  spec path and the selected `ai_migration`/`observed_property`'s declared
  metric clauses (`evaluate_migration`/`evaluate_observed_property`) instead
  of ignoring it; an unknown `--property`/`--migration` selection is now a
  check-time error (exit 2) instead of a fabricated statistical/observed
  verdict. `ai eval` also honors the documented `--records`-less invocation
  by falling back to the declared `dataset`'s `source` file. As part of this,
  `fslc ai drift`'s success result changes from `observed_conformant` to the
  documented `observed_supported` (`docs/LANGUAGE.md`,
  `docs/DESIGN-assurance-classes.md`); `observed_conformant` remains correct
  and unchanged for `fslc db observe`, which is a different command with its
  own, still-valid result vocabulary (#509).
- `fslc ai eval` results now carry every field
  `schemas/fslc/ai/statistical-result.v0.schema.json` requires
  (`schema_version`/`status`/`slice`/`metric`/`n`/`estimate`/`threshold`/
  `evaluator`/`assumptions`, not just `result`/`interval`/`checks`), and
  every non-`statistically_supported` terminal status (`dataset_invalid`,
  `evaluator_untrusted`, `slice_missing`, `insufficient_samples`,
  `inconclusive`) now exits 1 instead of 0 (#510).
- Native `verify`'s `vacuous_implication` lane now unwraps a
  `forall`-quantified implication before checking antecedent reachability,
  not only a bare top-level `Binary{op: "=>"}`. `docs/DESIGN-vacuity.md:22`
  already documented "a single `=>` directly under `forall*`" as the
  primary shape, but `verification_warnings` matched only
  `property.expr` itself being `=>`, so any invariant of the ordinary form
  `forall x: T { P(x) => Q(x) }` was invisible to the lane: a hollow
  invariant of this shape verified clean instead of being flagged, and
  `--vacuity error` had nothing to select. Every leading `forall` is now
  peeled (nested foralls included, matching the frozen Python reference's
  `_implication_antecedent_candidate`) and the antecedent is existentially
  closed over the collected binders (reusing the existing `exists_wrap`
  helper already used for `leadsTo` triggers) before the reachability
  check; with zero leading foralls this is a no-op, so the previous
  top-level-`=>` shape still works unchanged (#486).
- `fslc refine` now classifies a zero divisor in a refinement
  action-correspondence *argument* expression (`impl_action(a) ->
  abs_action(a / c)`, where `c` is an impl state variable that can be zero)
  as a located `refinement_failed`/`kind:"map_partial_op"` finding, joining
  the existing closed kind set (`abs_requires_failed` / `abs_state_mismatch`
  / `stutter_changed_abs` / `map_out_of_bounds`). Constructing an abstract
  action call is action context (`docs/DESIGN-divmod.md` §2.2), not the
  read-only "mapping expression" §2.3 exempts (a refinement *state* map,
  distinct from an action-correspondence argument), so it gets the same
  `partial_op` treatment a division inside the abstract action's own body
  would. Before this fix `check_refinement` propagated the divisor's raw
  `RuntimeError` unclassified, surfacing as `result:"error"`,
  `kind:"type"`, `message:"division by zero"` — neither of the two
  documented `/0` treatments, and not a member of the refinement contract's
  kind set. A correspondence whose divisor is always guarded on every
  reachable impl step is unaffected and still `refines` (#512).
- `analyze`'s TSG no longer leaks the internal db-dialect `QqDbSepqQ`
  separator sentinel into node labels: a db-dialect invariant/action label
  now matches the display name `verify` reports for the same target (both
  now go through the shared `fsl_core::display_name`; two independent local
  reimplementations in `fsl-tools` — `analysis.rs`'s `display` and
  `typestate.rs`'s `display_name` — were each missing the sentinel
  replacement `fsl_core::display_name` already does). Node ids keep their
  raw, guaranteed-unique internal form, but `--focus` now also accepts a
  node's displayed name (resolving it to the matching raw id), so a caller
  no longer needs to know the internal sentinel to reference a target
  `verify` already named for them.
- `action_dependency_graph` no longer collapses an action pair connected
  through more than one shared read/write state bridge down to a single
  bridge. `enables`/`conflicts_with` edges are deduplicated by
  `(from, kind, to)`, and the projection previously pushed one edge object
  per bridge state for the same pair, so the dedup silently kept only
  whichever state happened to be processed last (alphabetically, since the
  underlying map iterates that way) — renaming a state variable could flip
  which bridge a pair reported, and `progressless_cycle`
  (`--profile ai-review`) attached its leadsTo/terminal check to only that
  one surviving state, potentially missing progress attached through a
  different shared bridge. Every bridge state for a pair is now aggregated
  into `states` (plural) before emitting one edge, and the
  `progressless_cycle` consumer reads the full `states` array instead of
  the legacy singular `state` field (#498).
- `analyze` batch mode no longer silently drops an explicitly-named input
  just because it does not end in `.fsl`. `collect_analysis_files`'s
  `*.fsl`-only filter applied even to files named directly on the command
  line (not only to directory expansion, where it belongs), so
  `analyze a.toml b.txt` returned `files:[]`, `errors:[]`,
  `result:"analyzed"`, exit 0 — a batch that analyzed nothing reported
  success. Explicit files are now always kept regardless of extension: a
  `.toml` project manifest routes through the same handling single-file
  mode already uses, and anything else that cannot be analyzed is a real
  error in `files[]`/`errors[]`. Directory expansion is unaffected and
  still filters to `.fsl` only (#496).
- `fslc refine`! `maps auto` no longer guesses a binding for an incompatible
  same-name action pair. Auto-matched action parameters are now resolved by
  name only, never by position: with impl `go(a: K, extra: K)` and abstract
  `go(wanted: K)` under a bare `maps auto`, native previously bound
  `wanted <- a` positionally and silently dropped `extra`, returning
  `refines`/exit 0 for a mapping the frozen Python reference correctly
  rejects. Now a same-name action pair with a different arity, a surplus
  impl parameter, a renamed parameter, or an abstract parameter with no
  same-named impl counterpart is a located `kind:"type"` error, matching the
  contract `docs/DESIGN-refinement.md` already documented. A legitimately
  auto-mappable pair — including a pure parameter reorder — still auto-maps.
  `examples/e2e/2_requirements.fsl`'s `submit` correspondence (a genuine
  arity-changing map between the business and requirements layers) now
  declares its correspondence explicitly instead of relying on the removed
  guess (#494).
- `fslc refine`! init-state handling now reasons over *every* concrete
  initial valuation a nondeterministic `init` permits (an `init if` reading
  a state variable init never assigns, on either the impl or abs side)
  instead of comparing against the single default state a solver-free
  `Monitor` happened to materialize. This closed two opposite false
  verdicts: a nondeterministic impl `init` no longer lets native silently
  check only the default branch and miss both that branch's own
  initial-correspondence violation and the reachable set below the other
  branch (previously `refines`/exit 0 where the frozen reference correctly
  returns `refinement_failed`/`abs_state_mismatch@init`/exit 1); a
  nondeterministic abs `init` is now checked as set membership rather than
  equality against one materialized abs default, so a correct refinement
  whose impl deterministically starts in a different, still-valid, abs
  initial branch is no longer wrongly rejected (previously
  `refinement_failed`/exit 1 where the frozen reference correctly returns
  `refines`/exit 0). The step-0 impl self-consistency precondition (#466)
  shares the same enumeration, so a self-violation reachable only from a
  non-default initial branch is no longer missed either. A state variable
  assigned on only some init paths (not any) is unaffected and keeps the
  prior single-valuation behavior (#493).
- `fslc chain`'s project-manifest reader now fails closed instead of silently
  discarding malformed input: an unrecognized top-level section (including a
  plain typo), zero recognized `[business]`/`[requirements]`/`[design]`/
  `[impl]` sections (including an empty manifest), or a present-but-unparseable
  `depth`/`refine_depth` value (for example one followed by a TOML inline
  comment) is now a `kind: "parse"` error at exit 2 instead of a silently
  dropped layer, a vacuous `verified` over zero executed layers, or a silently
  substituted default depth that understates a declared depth (#489).
- `fslc chain`'s documented default and bare-filename invocations (e.g.
  `fslc chain` or `fslc chain fsl-project.toml` from the manifest's own
  directory) no longer fail the `[impl]` layer with an io error: an empty
  manifest parent directory now normalizes to `.` before resolving files and
  launching the implementation command (#500).
- `fslc ledger --impl-log` no longer discards a replay error (missing file,
  malformed JSON, wrong-spec trace, schema-invalid trace): it now fails the
  whole `ledger` command through the standard error envelope and exit code,
  the same as `--evidence`, instead of silently rendering a ledger with the
  implementation-log conformance row missing (#499).
- `fslc ledger --evidence` now surfaces a failing external evidence source
  (a definitive nonconformant/mismatch/unsupported verdict) as a 🔴 要確認
  finding for every requirement it attaches to — recursively, via its root
  `requirements`/`requirement.id` or a `requirement.id` nested inside a
  `findings`/`checks` array item — or as a spec-level finding when it fails
  with no requirement attribution at all. Previously a failing source only
  ever affected the assurance-class column, so a requirement explicitly
  attached to failing evidence still rendered green with no finding; the
  assurance-class computation itself is unchanged (class and verdict stay
  orthogonal, so a failing source still never lowers an independently
  proven requirement's class) (#508).
- **Breaking:** `fslc typestate` no longer treats a state comparison in one arm
  of an `or` as sufficient to pin a transition's from-state when the other arm
  says nothing about the entity. Previously `enum_guard_states`/
  `option_guard_states` unioned the states extracted from both arms of `and`
  **and** `or` alike, so `requires e.status == A or bypass` reported
  `derivable` with `from: ["A"]` and `--ts` emitted `self: E<"A">` even though
  the action is also reachable from every other state whenever `bypass` holds
  — a generated ghost type that excluded behavior the checked model accepts.
  `or` now unions the states each disjunct implies (as `and` already did),
  computed per entity, but only when **every** disjunct actually constrains
  the entity: a disjunct that says nothing about it (e.g. an unrelated flag)
  drops the guard entirely rather than leaving the other disjunct's state
  unioned in on its own. `status == A or status == B` — where both disjuncts
  constrain the entity, just to different states — correctly remains
  `derivable` with `from` covering both; only a mix of a real state
  constraint and an unconstrained disjunct (like `status == A or bypass`)
  stops being derivable. `and` is unchanged (#521).
- **Breaking:** `fslc typestate` struct-field state machines are now scoped by
  field name **and** owning struct type, not field name alone. `EnumLocation::Field`
  previously carried only the field name, so `enum_expr_location`/`enum_lvalue_location`
  matched any struct field with that name regardless of which struct declared it;
  two structs that both declare a same-named field backed by the same enum type
  (e.g. `struct Order { status: St }` and `struct Ticket { status: St }`) had each
  entity's report and `--ts` output absorb the other's transitions — a method
  emitted against the wrong host type. The same field-name-only matching also let
  a whole-struct-literal reassignment (`ticket = Ticket { status: Closed }`, as
  opposed to `ticket.status = Closed`) leak into an unrelated same-named-field
  entity's report, up to fabricating a `derivable` transition it never had.
  Field accesses/writes and struct literals now carry the public Kernel `named`
  type of their base/own declared type (already present on every public Kernel
  v1 expression/lvalue/struct-literal node), and a struct-field location matches
  only when the field name and that owner type both agree. Two entities with the
  same field name and different owning structs now stay fully independent in the
  JSON report and in `--ts`; an entity with multiple actions over its own field
  still aggregates into one machine, unchanged (#520).
- `fslc scenarios --deadlock error` now preserves the same `violated` /
  `deadlock` / exit 1 verdict `fslc verify --deadlock error` already reports
  for the same spec, instead of silently discarding the promoted failure and
  returning `result:"scenarios"` / exit 0. `--deadlock warn` still generates
  a `deadlock_terminal` scenario, now carrying the required explanatory
  `note: "after these steps no action is enabled"` (#522).
- `fslc scenarios` no longer describes a never-enabled action (a genuinely
  unsatisfiable `requires` conjunction, per verify's own `action_coverage`
  verdict) as "was enabled but no cover trace could be built": it now says
  the action is never enabled, with the same hint and `blocking_requires`
  verify already computes for it. The original wording is reserved for an
  action verify found enabled whose cover trace scenario generation still
  failed to build (#523).
- `fslc scenarios` now warns for every quantified `leadsTo` binding without a
  response witness individually, instead of collapsing completeness to one
  warning per property name: a witnessed binding for one binding value no
  longer hides the missing-response warning for every other binding. A
  binding whose antecedent never held within `--depth` is now worded
  distinctly ("antecedent never holds within depth K") from one whose
  antecedent held but never closed with a response ("has no response
  scenario within depth K") (#526).
- `fslc explain --readable` no longer prints a branch-lowered action's
  internal `name.bN` form: a `branches { when P { … } maps Q }` action now
  resolves back to its authored name (via a new `OriginChain` bound at the
  branch-splitting site), with a `branch:` line naming each branch's guard
  and `maps` correspondence, and an `Implements:` section when the source
  declares a refinement mapping — restoring the branch-lowering and
  synthesized-refinement-mapping detail `docs/DESIGN-explain.md` §2
  documents. The JSON skeleton's `actions[].name` for the same branch
  actions is corrected the same way, with the lowered form preserved as
  `generated_name` (#528).
- `fslc explain`'s JSON skeleton restores three fields the native
  implementation had dropped versus the documented contract: `spec_kind`
  (was hard-coded `null`), `auto_checks` entries of `kind:"partial_op"` for
  every syntactic `pop`/`head`/`at`/`/`/`%` site (previously `type_bound`
  only), and `generated:true` origin provenance on the SLA-synthesized
  `tick` action and `_deadline_*` invariants (previously indistinguishable
  from authored declarations) (#530).
- `fslc domain check` now folds a nested kernel's non-`verified`/`proved`
  result (`violated`, `reachable_failed`, `unknown_cti`, `unknown_budget`)
  into the top-level verdict and exit code instead of unconditionally
  reporting `result:"verified_under_assumptions"`/`formal_result:"verified"`/
  exit 0 regardless of what `kernel.result` actually said — a confidently
  green false negative on a domain whose aggregate invariant the kernel
  proves violated. `run_domain_check` also now returns a non-{0,1} `verify`
  status (spec error, internal error) verbatim instead of letting it reach
  `check_domain` and be misread as a kernel result, and the internal
  `stable_kernel_projection` allowlist gained the violated-path evidence
  keys (`loc`, `violated_at_step`, `violating_bindings`, `blame`,
  `last_action`, `trace`) so the nested `kernel` stays a replayable
  counterexample rather than only the bare verdict string (#515).
- `fslc domain replay` now actually drives a concrete Monitor over the
  lowered domain/effect model — stepping `command` and `effect_completion`
  log entries as real action calls — instead of only maintaining a
  `(effect, correlation_id)` bookkeeping set. Previously only 1 of the 4
  documented detection categories (completion without a prior request)
  could fire; a rejected command, a duplicate irreversible effect commit,
  and a lifecycle ordering mismatch (e.g. a completion observed after the
  aggregate moved into a state that rejects it) all silently passed as
  `conformance_checked`/exit 0. `unknown_domain_event`, `unknown_effect`,
  `effect_completion_event_not_declared`, and `unknown_runtime_event_kind`
  (including a `{"kind": ...}`-keyed log entry, which previously evaluated
  zero log lines and still reported success) are also now detected, and
  `final_state`/`assumptions` are populated from the same Monitor and
  assumption computation `domain analyze` already uses instead of always
  returning `{}`/`[]` (#518).
- `fslc domain replay` and `fslc domain analyze` now reject an unrecognized
  trailing argument (`result:"error"`/`kind:"usage"`/exit 2), matching
  every other `domain` subcommand and the rest of the CLI, instead of
  silently discarding it and returning a result computed without it (#516).
- `fslc lint`/`fslc migrate` now checked-model-validate every input,
  unconditionally, instead of only validating a file `plan_migration` found
  legacy syntax to rewrite. Previously an input with no legacy tokens for
  `plan_migration` to fix was never checked at all — a spec with a genuine
  type error, or a canonical requirements spec whose `implements ... from`
  target had moved, could still report `lint`/`migrate` exit 0 purely
  because it happened to have nothing to migrate, while the identical defect
  with one unrelated legacy token present correctly failed. The new
  pre-flight (shared by both commands through `load_migration_plan`) mirrors
  `fslc check`'s own kind/location convention exactly, so the same input
  gets the same verdict from both commands. `refinement`/`agent` dialects
  are excluded (same carve-out `fmt --check` already uses: a mapping file
  has no `state` block by design), and a refused plan (a legacy construct
  `plan_migration` recognizes but cannot machine-apply, e.g. `&&`) is also
  excluded, since no pre-migration checked model exists to compare for such
  input in the first place (#517).
- `Monitor::new` (the solver-independent concrete interpreter `replay` and
  BMC's concrete pre-scan build on) now runs the same deterministic-init gate
  the explicit engine's own construction check already had, instead of
  silently default-filling any state component `init` leaves free and
  treating that one arbitrary value as the specification's initial state.
  Previously `replay` compared an observed trace's initial state against
  that default and falsely reported `initial_state_mismatch` on a BMC-valid
  trace whose free component held a different, equally admissible value. A
  caller that already has its own complete concrete initial state (an
  observed replay trace's own step 0, an explicit `--from-state`/
  `--initial-state` snapshot, or a BMC witness's first state) is unaffected:
  it now builds through the new `Monitor::from_state`, which has nothing to
  ask `init` to compute and so is not subject to the gate.
  `initial_state_mismatch` is still fully checked whenever `init` determines
  every state variable; when it does not, the whole state is trusted from
  the caller rather than compared component-wise (#519).
- Native semantic diff now evaluates OLD forbidden arguments in the OLD typed
  model and reports missing actions, incompatible arity, or incompatible NEW
  argument domains as explicit `unknown` findings instead of a false
  `no_semantic_change` result (#460, prerequisite for #427).
- `fslc testgen` (and `domain testgen`) no longer converts a genuine
  `violated`/`reachable_failed` counterexample from the underlying scenarios
  machinery into an unrelated exit-2 `kind:"semantics"` spec error. The
  guard that only short-circuited on `status == 2` let a real `violated`
  invariant/`leadsTo`, or a `--strict` `reachable_failed`, fall through to
  `fsl_tools::validate_scenarios`, which found no `scenarios` array in what
  was actually a `verify`-shaped envelope and reported a generic error —
  changing the exit code from 1 to 2, replacing `result` with `"error"`,
  and destroying the trace/blame evidence a repair loop depends on. Both
  guards (`run_testgen`, `run_domain_testgen`) now propagate any non-zero
  status verbatim; the success path (`result:"generated"`, exit 0) is
  unaffected (#472).
- `fslc refine` now verifies the impl spec's own internal consistency (type
  bounds, invariants, `trans`, `ensures`) before checking any correspondence
  against the abstraction. An impl that violates itself within `--depth` —
  e.g. a dropped `requires` that lets a state variable step outside its
  declared type bound — is reported `result:"violated"` with a `note`
  explaining this is a property of the refinement input, never `refines`
  and never folded into `refinement_failed`. Previously `check_refinement`'s
  BFS silently discarded (`continue`d past) any violation the impl produced
  while stepping, so a guard weakening that broke the impl's own bounds
  could pass as `refines`/exit 0 with no counterexample at all. `fslc diff`
  gains the same detection as a new `impl_violated` finding kind that fails
  its gate unconditionally (unlike other finding kinds, not subject to
  `--forbid`), since a self-violating side makes the comparison untrustworthy;
  the `implements`-clause mutation oracle and the `implements:` verify
  metadata (`requirements_implements_output`) are also corrected to stop
  reporting the impl-violation case as a clean/`"refines"` refinement (#466).
- `--vacuity {error,ignore}` now selects over the complete documented 5-kind
  vacuity lane set (`fsl_core::VACUITY_KINDS`) instead of only the two kinds
  spelled `vacuous_*`. `always_true_requires`, `tautology_over_frozen`, and
  `urgency_freeze` previously could not be promoted to `--vacuity error` or
  suppressed by `--vacuity ignore` because none of the three names start
  with `vacuous_`; native `apply_vacuity_mode` matched by name prefix rather
  than the closed kind set (#465, the CLI half; the lanes for these three
  kinds are still unimplemented on native, see above — this fix prevents a
  *second*, independent bug from compounding the first once they land).
- `sweep` no longer folds a spec `error` (parse / type / semantics / io /
  vacuous / a mistyped `--instances`/`--values` name / a missing file) into
  the positive `sweep_passed`/exit-0 verdict. Any scope in the grid that
  errors now short-circuits the sweep with that error's envelope and exit
  code (2, or 3 for `kind:"internal"`) returned verbatim, instead of being
  discarded as an unrecognized grid-cell result while the top-level verdict
  fell back to "no counterexample found" — previously a one-character typo
  in `--instances` could turn a sweep over a genuinely violating spec from
  exit 1 `sweep_failed` into exit 0 `sweep_passed` (#464).
- `verify --engine explicit` now agrees with symbolic BMC on a contradictory
  `init`: a `forall` binder that writes different concrete values to the same
  non-indexed location across binder values (e.g. `forall k: K { x = k }`
  with `|K| > 1`) is detected by the concrete Monitor without a solver and
  reported as `result:"error"`, `kind:"vacuous"`,
  `message:"init constraints are unsatisfiable"`, exit 2 — matching BMC
  exactly instead of silently running the forall as a last-write-wins loop
  and returning `result:"proved"` / exit 0 for a spec with no valid initial
  state (#480).
- `fslc testgen`'s `pytest` target now emits the `forbidden`-scenario rejection
  assertion (`result = adapter.step(...)` / `_assert_rejected(...)`) that every
  other testgen target already emitted, restoring byte-identity with the
  frozen Python reference and giving the generated conformance harness a real
  negative control against a guard-weakened implementation (#471).
- The `examples/gallery/injected/` negative-control detector matrix and its
  primary/blind calibration are now measured on the authoritative native CLI,
  not only the frozen Python reference: 17 injected specs and the
  `agentic_rag` `backported_constraints_slice.fsl` mutation slice carried a
  `requirement` annotation text conflict between their outer block and an
  inner legacy tag (checked-model error since #237) that the corpus never
  followed; the redundant inner tag is removed and the outer block's already-
  correct requirement text is the sole source of truth. Adds
  `rust/fslc/tests/injection_detector_matrix.rs` (the native primary/blind
  matrix, run by `tools/check-native-integration.sh`) and
  `rust/fslc/tests/corpus_check_sweep.rs` (an exhaustive `specs/`+`examples/`
  `check` sweep closing the gap left by `rust/fsl-lsp/tests/corpus.rs`, which
  never builds a checked model) (#485).
- `preserve progress` now works with an indexed (per-element) refinement state
  map (`map a[i: K] = expr`), not only scalar maps. `fsl-core::substitute_expr_indexed`
  substitutes each pulled-back read `a[e]` with the map's own expression, its
  binder replaced by `e` (DESIGN-refinement.md's "substituted on the read"
  rule), matching capture-avoidance already applied to scalar substitution.
  `rust/fsl-verifier/src/refinement.rs`'s `check_refinement_progress` no
  longer rejects the mapping outright the moment *any* indexed map exists
  (previously `VerifyError("indexed progress map for '<name>' is not
  implemented")`, `kind:"semantics"`, exit 2) — including when the pulled
  `leadsTo` does not even read that map. This restores the documented
  DESIGN-refinement.md:20-40 canonical shape (indexed map + `preserve
  progress` in the same mapping) and the `examples/agentic_rag` and
  `examples/multi_agent_system` cross-layer `refine` commands, which
  previously reported the verifier's own missing feature as a spec error
  instead of `refines`/`refinement_failed` (#483).
- Zero-division is now totally defined in property-context expressions
  (`invariant`, `trans`, `reachable`, `leadsTo`, and refinement state mapping),
  matching `DESIGN-divmod.md` §2.1/§2.3: `a / 0` and `a % 0` evaluate to `0`
  instead of raising a concrete `RuntimeError` there, in both the Z3 symbolic
  encoding (`div`/`modulo` are now pinned with `ite(divisor == 0, 0, ...)`)
  and the concrete Monitor/BFS evaluator. Previously an unrelated `/0`/`%0`
  inside an invariant could mask a genuine, independent invariant violation
  behind a misattributed `violation_kind:"partial_op"` / `_partial_<action>`
  counterexample under the default (BMC) and `--engine induction` engines,
  and made `--engine explicit` return a raw `result:"error"` / `kind:"semantics"`
  instead of the real verdict — breaking the documented symbolic/concrete/BFS
  agreement invariant. Action-context (`requires`/body/`ensures`) division by
  a divisor that can reach zero is still reported as `partial_op` (§2.2 is
  unchanged); Euclidean negative-number division/modulo semantics were
  already correct and are unaffected (#477).
- The recursive `agent` dialect (`docs/LANGUAGE.md` §13.6) now has a real grammar and structural
  analyzer in native, matching the frozen reference's `src/fslc/ai_parser.py`/`ai_agent.py` exactly
  (confirmed by byte-identical JSON, including `agent_ir`/`graph_summary`, on
  `examples/ai/recursive_support_agent.fsl` and every documented finding-kind fixture). Previously
  `rust/fsl-syntax`'s `parse_agent` only balanced braces and discarded the entire body, so any token
  soup that lexed cleanly parsed as an empty agent and `fslc ai check` on a recursive `agent` document
  returned `"expected an ai_component document"` (native rejected the dialect entirely) while `fslc
  check` unconditionally reported the hardcoded constant `agent_analysis_result: "agent_analyzed"`
  with no analysis behind it — a confidently green false negative on AI agent authority-delegation
  safety, and native's own CLI contract (`rust/fslc/cli-contract.json`) already advertised "check an
  ai_component hard contract or recursive agent structure" as a capability it did not have. A `grant
  authority`/`grant context` that exceeds the immediate parent's declared boundary is now a check-time
  `kind:"semantics"` error from `ai check` and `check` alike; the six documented
  `agent_structural_violation` finding kinds (`child_authority_exceeds_parent_authority`,
  `child_context_exceeds_parent_context`, `irreversible_operation_without_human_approval_path`,
  `visibility_leak_across_sibling_agents`, `low_trust_agent_path_to_high_authority_tool`,
  `policy_review_bypass_in_orchestration`) are all computed by `fslc ai check`. `fslc verify`'s
  rejection of agent documents and `fslc fmt`'s refusal to reformat a well-formed agent body (no
  native pretty-printer exists yet) are both unchanged (#468).
- Native `ai_component` lowering (`fslc check`/`verify`/`ai check`) no longer collapses to a
  one-boolean catalog sentinel with an unsatisfiable no-op action. It now generates the documented
  `Tool` enum, `human_approved`/`tool_executed`/`tool_suggested: Map<Tool, Bool>` and
  `fallback_required: Bool` state, `suggest_*`/`approve_*`/`execute_*`/`fallback_*` actions (no
  `execute_*` action is ever generated for a forbidden tool; an approval-required tool's `execute_*`
  action always carries `requires human_approved[tool]`), and the
  `ai_forbidden_tool_not_executed__<Tool>` / `ai_approval_before_execute__<Tool>` invariants. `check
  hard { rule <Name>; }` now rejects an unknown rule name as a check-time `kind:"semantics"` error
  (previously silently accepted) from `ai check`, `check`, and `verify` alike. `fslc ai check` also
  implements the four previously-unchecked hard rules (`tool_authority`, the two static findings
  were entirely missing; `tool_schema_declared` was missing; `human_approval_required` used a
  narrower `irreversible && may_execute && !approved` predicate than the documented
  `irreversible && !requires_human_approval && !forbidden` rule, so a tool only in `may_suggest`
  passed silently) and populates `repair_candidates` instead of always emitting `[]`. `fslc ai
  replay` gains the matching `tool_authority` findings for `suggest`/`execute` calls outside
  authority and now flags a declared precondition with no `preconditions` evidence object at all
  (previously only an explicit `false` value was caught, so missing evidence passed silently). This
  was AGENTS.md's "confidently green false negative" on the AI dialect's tool-authority /
  human-approval safety claims (#470).
- Native `fslc db check` now evaluates `rule all_active_writes_exist` the same way it already
  evaluated `rule all_active_reads_exist`: dropping a column that is still declared as an active
  artifact's write capability now yields a `column_removed_while_still_written` finding with
  `witness`, `minimal_conflict_set`, `repair_candidates`, and `artifact_version` populated, and the
  top-level JSON `result` is reconciled to `"violated"` whenever the attached `kernel` projection
  reports a violation. Previously the write branch was silently missing (a regressive port relative
  to the frozen Python reference), so a write-drop incompatibility returned `verified_under_assumptions`
  with an empty `findings` array — a confidently green false negative, and one that escaped a
  hardcoded kernel depth-8 default entirely for deep migration histories, since the findings layer
  is depth-independent once the write branch exists (#469).
- Native `fslc check`/`verify`/`db check` no longer reject two writes indexed by distinct enum
  members as a possible alias (`"an action may not assign the same state location more than once"`).
  The write-aliasing analysis now resolves enum-member indices (both the typed `Expr::EnumMember`
  literal and a local constant bound to an enum value) to their nominal `(type_name, member)`
  identity, matching the same-index detection already applied to `Int`/`Bool` constants. This
  restores `fslc check`/`db check` on the four `examples/db/` `dbsystem` `rename`/`split`/`merge`
  preservation fixtures, which previously exited 2 against a golden corpus snapshot of `"ok"` (#475).
- Native `fslc check`/`verify` now emit the documented `fair_not_inherited` `compose` warning: when a
  non-fair synchronized action references a `fair` component action, `warnings` includes a
  `kind: "fair_not_inherited"` entry naming the composite action and fair constituent(s), matching the
  frozen Python reference's message and `loc` exactly. Compose lowering previously discarded
  constituent `fair` markers with no warning at all (`rust/fsl-core/src/compose.rs` had no warnings
  channel), so a declared `fair` constituent silently stopped contributing fairness through
  synchronization with no diagnostic signal — the same failure mode issue #16 fixed in the frozen
  Python reference, regressed by the native port (#474).
- The frozen `tests/test_dialect_conformance.py` corpus-conformance harness (`docs/DESIGN-conformance-harness.md`)
  is green again (0 failing of 212, up from 201 passed / 6 failed). Four intentional-error `governance`
  gallery fixtures gained the `// expected-result: error` front matter that reclassifies them as
  `DECLARED_ERROR` instead of `CONFORMANCE`; the no-action `governance_semantic_before.fsl` "before"
  fragment and the three `examples/causal/*.fsl` files (a dialect the frozen Python reference does not
  implement at all) are now registered exclusions (`MONITOR_EXCLUSIONS` and a new `causal` entry in
  `EVIDENCE_CONSTRUCTS`/`is_causal_source`) instead of falling through to `UNKNOWN`. The always-red
  harness had made a new registration gap indistinguishable from a pre-existing one (#476).
- Refinement typechecking now rejects an unshadowed bare enum member shared by distinct
  implementation and abstraction enums, preventing checked and evaluation
  merge order from assigning different nominal identities. Existing identifier
  shadowing by implementation inputs is preserved, unevaluable abstraction
  constants are excluded, bijections use explicit typed conversion, and
  many-to-one mappings use explicit source-total abstraction (#454, #455).
- Refinement mappings can now declare an exhaustive, type-safe member-wise
  conversion between distinct nominal enums and invoke it from state maps or
  action arguments. This includes requirements `process` stage enums, rejects
  unknown/duplicate/non-total mappings and implicit ordinal conversion, and
  fails closed in raw replay modes that lack a typed implementation model
  (#450).
- The embedded native CLI contract and exact help tree now include all six documented causal
  command leaves, while the deliberately absent `causal verify` path remains rejected (#442).
- Verifier transition-outcome agreement now proves actual guard failure and
  representable defined post-update failure phases in native evaluation order,
  rejects malformed, relabelled, partial, or checked-arithmetic evidence, and
  fails closed for `partial_op` or concrete evidence outside the bounded
  symbolic representation (#428).

### Changed
- The shared parser evaluation retains one cursor, annotation, location, diagnostic, and standard
  grammar owner. The Public Kernel evaluation likewise retains its v1/v2 projector, but selects the
  six independently evolving testgen/replay trace schema constants as one neutral-owner child slice
  with unchanged crate-root symbols, byte-stable values, and schema drift negative controls (#399;
  follow-up #448).
- `dbsystem` executable lifecycle lowering now constructs typed Kernel surface IR
  directly instead of rendering and re-parsing generated FSL. The migration preserves
  types, state, action/property semantics, metadata, annotations, and deterministic
  ordering, removes the obsolete public source generator, and binds generated targets
  to authored DB declarations so diagnostics and Public Kernel v2 provenance no longer
  cite fabricated generated-source lines (#410).
- The `fsl-runtime` touch-driven extraction evaluation retains the current public facade and
  physical eval, Monitor, liveness, refinement, search, and replay owners. Evidence is insufficient
  to select a move-only slice after the short migration/feature burst, so no implementation child
  is created; future extraction remains gated by owner change, dependency direction, solver-free
  production dependencies, and positive/rejecting semantic agreement controls (#398).
- The `fsl-core` dialect/domain lowering evaluation retains the existing syntax, checked dispatch,
  structural lowering, and generated-source projection owners. Evidence is insufficient to select
  a naming-policy slice or establish a current naming local optimum, so it creates no implementation
  slice and records the exact evidence and controls required before sharing the duplicate private
  core naming policy (#397).
- The native CLI output-boundary evaluation inventories all 50 command leaves and rejects a typed
  `Command`/`CommandOutcome` framework. A future, separately scoped change may centralize only
  regular-command JSON/raw process delivery, with exact byte, envelope, stderr, exit-status, and
  Worker-separation controls (#394; follow-up #441).
- The native causal command parser, I/O preparation, tool calls, and JSON/status projection now
  have one explicit `causal.rs` module owner, while top-level dispatch, shared process helpers, raw
  output, serialization, and exit normalization remain in `main.rs` with unchanged CLI contracts
  (#393).
- Native BMC, explicit/auto, and browser verification now share the single
  `verification_output` renderer owner, while native engine selection and cache policy remain in
  `verification` with explicit dependencies and unchanged output and exit contracts. Explicit
  witnesses are replayed before rendering, and malformed or cross-family cache entries degrade to
  misses instead of producing a verdict (#396).
- The pure `conservation_candidate` classifier now belongs to the existing `fsl-tools` analysis
  family, while the CLI retains ai-review orchestration, envelope formatting, mode validation, and
  transport with unchanged findings, ordering, and exit codes (#395).
- Test generation now receives an explicit delivery-normalized path context,
  removing filesystem and CWD observation from the pure generator while
  preserving existing, missing, and symlinked pytest paths and every other
  target's output bytes (#423).
- Requirements-document locale parsing and selection now has one private
  neutral presentation owner shared by rendering, markers, glossary, and
  document checking, while preserving the `fsl_tools::Locale` facade and
  generated output bytes (#422).
- BMC and induction now share neutral private liveness and trace-projection
  owners, preserving verdicts, evidence, trace output, and solver boundaries
  (#421).
- Checked-model type validation now has one private neutral `fsl-core` owner
  shared by model construction, refinement, and Public Kernel export, with
  unchanged Kernel schemas, ordering, diagnostics, and fail-closed controls
  (#420).
- The duplicated Domain naming transform now has one private neutral owner used
  by analysis, Kernel grounding, and code generation, with mixed-case, digit,
  consecutive-underscore, and one-sided divergence controls (#419).
- Claude's path-scoped Rust rule now links the accepted component and internal
  architecture, names all crate owners and hard delivery boundaries, rejects
  eager or unscoped C2 rewrites, and has positive/negative loading controls for
  Rust versus frozen-Python paths (#392).
- Migration-era porting, Python bridge, and sequence design text now marks its
  historical authority explicitly and links to the maintained native Rust
  contracts. The Kernel contract now records native outcome priority, while
  future C2 ownership candidates are explicitly non-authorizing and require
  separately scoped, audited, independently revertible changes (#430).
- The authoritative Rust workspace now has one evidence-backed component design
  record covering all eleven crates, their state and responsibility ownership,
  dependency gates, public contracts, design pressures, and reevaluation
  triggers, plus a companion internal design covering directed value flow,
  mutable-state and failure ownership, targeted dependency normalization, and
  touch-driven extraction. Migration-era documentation now consistently treats
  Python as frozen compatibility evidence rather than the active product
  authority, and concrete/symbolic failure agreement now includes a
  corrupted-evidence negative control.

## [3.1.0] - 2026-07-20

### Fixed
- Native induction CTIs now emit the documented heuristic `suggested_invariants` for monotone
  scalar and uniformly initialized Map counters without changing the proof verdict; quantified Map
  suggestions choose a collision-free binder so the emitted expression remains reusable (#337).
- Native induction lemma handling accepts candidates for selected invariant and `trans` properties,
  proves each candidate without original user properties, then uses and recommends only the first
  independently proved candidate excluding each successive CTI (#336).
- `fslc scenarios` now emits the shortest replayable action-coverage trace even
  when the covered action reaches a terminal state before the requested depth,
  including requirements transitions guarded by `Bool` inputs (issue #405).
- `fslc mutate --by-requirement` now attributes acceptance and forbidden kills
  through explicit requirement annotations on the failed trace declaration
  instead of reporting the linked requirement as `empty_formalization` (issue
  #407). Explicit requirement relations are preserved even when their ID equals
  a trace-case ID, while synthetic case relations remain excluded. Duplicate
  acceptance or forbidden IDs are rejected so attribution is unambiguous.
- `fslc mutate` now adjudicates singleton bound mutants whose empty domain
  produces no action instances instead of panicking in bounded verification;
  overflowing integer and bound neighbors are omitted (issue #406).
- Explicit domain-effect `success_event`, `failure_event`, and `timeout_event`
  roles now override event-name heuristics, ambiguous cross-role assignments
  fail closed, and completion, retry eligibility, Monitor/BFS execution, and
  symbolic verification share the same lowered status (issue #409).
- `fslc lint` now classifies business `policy`/`goal` `satisfies CTRL-*`
  operands as control references, recursively expands directory inputs into a
  sorted list of unique physical `.fsl` files even when operands repeat or alias
  one another, and continues to report nested FSL parse errors instead of silently
  skipping them (issues #385 and #386).
- `fslc fmt` now preserves canonical hyphenated IDs in business control,
  policy, goal, and `satisfies` positions while retaining normal spacing around
  arithmetic subtraction (issue #387).
- `docs/LANGUAGE.ja.md` now carries the Japanese translation of the
  "Literate Markdown FSL" subsection added to `docs/LANGUAGE.md` §7 by the
  literate Markdown feature, so `docs/intro/language.ja.html` documents the
  `.md` input support instead of silently omitting it (issue #346).
- Documentation for the depth-4 underspecification probe (`divergent_choice`,
  `unconstrained_effect`) now matches the native implementation: a solver-free
  explicit-state BFS over the runtime Monitor, not symbolic BMC/Z3.
  `docs/DESIGN-underspecification.md`, `docs/DESIGN-analysis.md`,
  `docs/LANGUAGE.md`/`docs/LANGUAGE.ja.md`, and `skills/fsl/reference.md` are
  corrected; the `evidence_basis:"bounded_bmc"` schema enum value is retained
  unchanged as frozen v0 vocabulary for "backed by a bounded reachability
  witness", so no output or schema contract moves (issue #318).
- Mutation documentation no longer overstates the score: `kill_rate =
  killed / (killed + survived)` is now defined everywhere as bounded
  mutant-set sensitivity — dependent on the operator mix, `--max-mutants`
  cap, `--depth`, and the verify/acceptance/forbidden/refinement oracle — not
  a production defect-detection rate, spec-correctness probability, or
  completeness measure. Survivors are documented as a review queue
  (equivalent mutants, dead-at-baseline behavior, beyond-depth effects, or
  genuine under-constraint) instead of "a place where an invariant is
  missing", and `empty_formalization`/per-requirement kills as observed lower
  bounds within the chosen mutant set and depth. Aligned across
  `docs/LANGUAGE.md`, `docs/LANGUAGE.ja.md`, `docs/DESIGN-mutate.md`,
  `README.md`, `skills/fsl/reference.md`, and the generated site reference; a
  new `rust/fslc/tests/mutation_docs_contract.rs` regression keeps the
  stronger survivor claim from returning (issue #338).

### Changed
- The native installer no longer clones the repository into `~/.fsl`. It now
  installs an exact-tag, checksummed CLI/LSP pair under the user data directory,
  atomically activates the complete payload, safely migrates recognized Python
  2.7 links, and rejects binaries whose reported version differs from the
  Release tag. Agent Skills are packaged as a checksummed Release asset; the
  v3.0.0 compatibility path verifies a pinned tag-archive checksum and extracts
  only the six installed skills.

### Added
- Causal portfolio ledger (Phase 5, issue #364): `fslc causal ledger
  model.fsl [--plans ... --evidence ... --lifecycle ... --as-of ...]`
  integrates claims, validation plans, evidence, and observations into
  a per-claim projection with 12 deterministic attention reasons
  (`validation_plan_missing`, `current_evidence_missing`,
  `observation_not_directional_support`, etc.). New
  `fsl-causal-validation-plan.v0` schema for immutable plan artifacts
  with claim pins, design, scope, observation window, measurements,
  and opaque external refs. Plan lifecycle reuses
  `fsl-causal-evidence-lifecycle.v0`. CLI envelope:
  `causal-ledger.v0` (inventory 38). Every claim appears regardless of
  plan/evidence; retired claims have no attention; output is byte-stable.
- Causal observation bridge (Phase 4, issue #360): `fslc causal
  observe-expectations model.fsl --from-log events.jsonl --mapping
  log_mapping.fsl --scope scope.json --period-start ... --period-end ...`
  replays compiled expectations against a production JSONL log using the
  solver-free `BoundedLivenessMonitor`. Generates per-expectation
  `fsl-causal-evidence.v0` artifacts with `design: "observational"`,
  `support: "inconclusive"`, `assurance: "replay-observed"`, and matching
  lifecycle records. Pass/violated verdicts never change claim
  `formal_assurance` or `causal_support`. Nonconformant logs,
  missing scope/period, and mapping failures are fail-closed.
  CLI envelope schema: `causal-observation.v0` (inventory 36).
- Causal expectation lowering (Phase 3, issue #323): explicit `expectation`
  declarations (trigger action or inline predicate, response predicate,
  `within N clock <name>`, `derived_from_claim`) compile — fail-closed on
  missing/foreign clocks, fractional tick conversion, unresolved references,
  or non-kernel targets — into ordinary `leadsTo ... within ticks` properties
  on the imported spec; action triggers become one-step pulse ghosts that
  never change guards. `fslc causal verify-expectations model.fsl [--depth
  K]` (`causal-expectations.v0`) reports `pass`/`violated` with
  `assurance: "bounded"`; pass and violated goldens both hold the central
  invariant that the derived claim's `formal_assurance` stays `"not_run"`
  and its `causal_support` is untouched — `derived_from_claim` is
  traceability only, and the legacy `supports` field is a parse error. A
  generated property is semantically identical to the hand-written pulse
  encoding (asserted by test), and there is deliberately still no
  `causal verify` alias.
- Causal evidence plane (Phase 2, issue #322): `fslc causal analyze --evidence
  artifact.json [--lifecycle chain.json] [--as-of YYYY-MM-DD]` validates
  `fsl-causal-evidence.v0` artifacts (closed design/support vocabularies,
  claim ID + content-version pins, scope tokens, ISO-8601 periods, canonical
  sha256 `artifact_digest`) and independent append-only
  `fsl-causal-evidence-lifecycle.v0` digest chains, fail-closed on
  schema/digest/chain violations. A new `causal_evidence_graph` projection
  (and the `causal-review` profile) overlays a deterministic per-claim
  `causal_support` — `untested | supported | challenged | inconclusive |
  mixed | unsupported_by_current_evidence` — counting only artifacts that pin
  the current claim version with `subsumes` scope, declared freshness, an
  active lifecycle, and an observation window at least the claim's minimum
  lag; one source lineage is one vote and staleness is judged only against an
  explicit `--as-of` date. Sixteen evidence/measurement review findings
  (`evidence_scope_mismatch`, `evidence_window_shorter_than_lag`,
  `duplicate_evidence_source`, `unobserved_mediator`,
  `unsupported_assumption_chain`, ...) join the causal-review profile, all
  `formal_status: "not_a_violation"`. Evidence never changes a claim's
  `formal_assurance: "not_run"` — the two axes stay orthogonal by contract
  and by test. Ships three new schemas under `schemas/fslc/causal/`, a
  digest-stamped example artifact + lifecycle chain under
  `examples/causal/evidence/`, and docs/skills updates.
- `fslc causal check|analyze|diff` (Phase 1 of the review-only causal profile,
  `docs/DESIGN-causal.md`): a standalone `causal <Name> { ... }` document —
  parsed outside the dialect registry via a pre-dispatch sniff, so the frozen
  Python `DIALECT_KEYWORDS` parity gate does not move — is typed into a
  `CausalModel` (roles, measurement bindings resolved against imported
  kernel/business specs, closed scope vocabulary, claim content versions and
  the `active|retired` lifecycle, clock mappings, fail-closed well-formedness
  diagnostics including `causal_instantaneous_loop` and
  `causal_unacknowledged_feedback`). `causal analyze` emits deterministic
  `causal_graph` / `causal_timeline` / `causal_traceability_graph`
  projections (JSON/Mermaid/DOT; SCC-condensed, Dijkstra-exact earliest
  first-pass, upper-bounded latest through feedback, capped representative
  paths with truncation metadata) and a `causal-review` profile with 15
  structural/temporal findings, all `formal_status: "not_a_violation"` with
  `do_not_assume`. `causal diff` compares stable claim IDs and content
  versions, flags missing version bumps, retired-claim reactivation, and
  retired-hypothesis re-proposals. `causal-diff.v0` accepts no evidence inputs,
  so `support_transition` remains `"not_available"`; evidence aggregation is a
  separate analyze/ledger projection. Evidence/plan scope comparison treats a
  dimension present on only one side as `unassessable`, never universal. No
  output path attaches `proved`/`verified` to a causal claim and
  there is deliberately no `causal verify`; `fsl-runtime`, `fsl-solver*`,
  `fsl-verifier`, Public Kernel v1, and the frozen Python reference are
  untouched. Ships `schemas/fslc/causal/` (check/graph/findings/diff v0),
  three dogfood models under `examples/causal/`, LSP sniff handling,
  `docs/LANGUAGE.md` §17 + `docs/LANGUAGE.ja.md` §17,
  `skills/fsl/reference.md` §12 with the agent-facing "never present causal
  claims as proved/verified" hard rule, and a docs-contract regression
  (issue #321).
- Accepted `docs/DESIGN-causal.md`: the design contract for the review-only
  `causal` profile (typed causal hypothesis graphs). It fixes the three-plane
  architecture (causal specification / formal expectation / external
  evidence), surface syntax and field contracts, scope-token declaration and
  three-valued containment, claim content-versioning and retired lifecycle,
  discrete time semantics (lag/persists Minkowski sums, fail-closed clock
  mappings), delayed-feedback polarity classification, the typed CausalModel
  IR boundary, and versioned JSON contracts under `schemas/fslc/causal/`.
  Causal claims never receive `proved`/`verified`; Public Kernel v1, runtime,
  solver, and verifier are unchanged by design (issue #320).
- Added an explicit dialect boundary to `fslc document` (issue #334). RCIR v1
  supports exactly `spec` and `requirements` (`fsl_tools::
  RCIR_SUPPORTED_DIALECTS`); every other dialect (`business`, `governance`,
  `domain`, `dbsystem`, `ai_component`, `compose`, `refinement`, `agent`) was
  already rejected before projection, but the rejection surfaced as a generic
  `semantics` error indistinguishable from a defect in the input. The
  projector now returns a typed `DocumentProjectionError`, and
  `generate`/`claims`/`check`/`approval create --kind requirements_document`
  all report `kind: "document"`, `code: FSL-DOC-DIALECT-UNSUPPORTED` with
  machine-readable `dialect` and `supported_dialects` fields — an explicit
  unsupported report, never a partial or plausible-looking document.
  Negative controls now cover all eight rejected dialects (previously only
  `business`), and a coupled-change test derived from
  `fsl_syntax::DIALECT_KEYWORDS` pins the supported set as a scope-change
  tripwire. It does not replace the still-required adapter and dialect-scoped
  coverage row. `--view business`/`--view
  design` remain reserved usage errors: "design" is a refinement-chain
  position, not a dialect keyword, and a cross-layer view is gated on the
  recorded activation contract (explicit `refinement`/`implements` metadata
  only; liveness re-verification shown as a separate fact; `compose`
  rejected until truthful multi-source provenance). See
  `docs/DESIGN-document-dialect-adapters.md`.
- Added a `requirements_document` target kind to `fslc approval` (issue #333),
  binding a reviewed `fslc document generate` artifact to a digest-bound
  approval record without implicitly widening the existing closed `kind`
  enum: a new schema revision, `fslc.approval.v3` (unsigned) /
  `fslc.approval.v4` (signed), constrains `kind` to `requirements_document`
  only, and existing v1/v2 records for `ledger`/`html`/`scenarios` stay
  byte-shape compatible. `target.digest_algorithm` is
  `fsl-rendered-requirements-document-v1+sha256` — the same plain-`sha256`
  value `fslc document generate`'s own `artifact_digest` reports — and
  `target` additionally records the literal reviewed-document digest and the RCIR claim-set digest
  (`claim_set_digest_algorithm`/`claim_set_digest`), so `fslc approval check`
  can report `artifact_changed` and `claim_set_changed` drift reasons distinct from
  `spec_changed`/`rendering_changed`. Unlike the other three kinds,
  `approval create --kind requirements_document` does not require the
  reviewed artifact's bytes to match a fresh rendering exactly — it reuses
  `fslc document check`'s own structural conformance gate, tolerating an edit
  inside the one editable slot (issue #329) while still rejecting a tampered
  claim block. `--glossary`/`--evidence` (rejected for the other three
  kinds) are recorded as `{path, digest}` reproducibility inputs instead of
  `--depth`/`--deadlock`/`--engine` (rejected for `requirements_document`),
  since a deterministic RCIR projection has no solver-depth concept at all.
  `fslc document generate --approval RECORD` (repeatable, plus `--trust-key`
  for a signed record) newly displays a verified approval record as an
  "Approval records" section — always paired with a fixed disclaimer that
  approval is an organizational record, never proof of intent fidelity —
  and fails closed (`FSL-DOC-APPROVAL-DRIFTED`) if the record does not match
  the current rendering or the literal reviewed file has changed. The section
  displays the reviewed-file digest rather than the canonical rendering
  digest; `fslc document check --approval RECORD` reproduces
  the same section for structural comparison (never verifying a signature)
  via a new `approval_digest` frontmatter key and `approval_changed`/
  `FSL-DOC-APPROVAL-CHANGED` drift reason. See `docs/DESIGN-approval.md`.
- Added an evidence/assurance overlay to `fslc document generate`/`check`
  (issue #332): a repeatable `--evidence evidence.json` flag accepts the same
  saved verification-evidence envelope shape `fslc ledger --evidence` already
  does, and overlays a per-requirement assurance class
  (`proved`/`bounded`/`replay-observed`/`statistical`/`not_run`) using the
  exact classifier `fslc ledger` already established (issue #171) — no new
  judgment is invented. Assurance renders as residue *after* every
  requirement's claim blocks, never inside a `<!-- fsl:claim -->` marker, so a
  claim's own digest never depends on whether evidence was supplied; a
  `violated` BMC run still shows `bounded`, never silently downgraded, since
  assurance class (method coverage) is orthogonal to pass/fail verdict. A
  liveness requirement (linking a `leadsTo`/`reachable` claim) gets an extra
  caveat when its only formal evidence is `bounded`. v1 is evidence-file-only
  — `generate` never runs a live verify pass itself, preserving the
  byte-identical-per-spec determinism contract. A new frontmatter key,
  `evidence_digest` (order-independent across repeated `--evidence` flags),
  lets `fslc document check --evidence ...` detect evidence that differs from
  generation time as its own drift reason
  (`evidence_changed`/`FSL-DOC-EVIDENCE-CHANGED`); unlike a renderer or
  glossary change, an evidence-only change skips only residue comparison, not
  per-claim-body comparison, so a genuine hand-edit inside a claim is still
  caught regardless of the evidence given to `check`. An evidence file naming
  an unknown requirement ID is a warning by default, an error under `--strict`
  (`FSL-DOC-EVIDENCE-UNMATCHED`). See `docs/DESIGN-document-evidence-overlay.md`.
- Added a presentation-only glossary sidecar to `fslc document generate`/`check`
  (issue #330): `--glossary glossary.json` (schema `fslc.document-glossary.v1`)
  maps a target string (`action:NAME`/`state:NAME`/`enum:Type.Member`, validated
  against the checked `KernelModel`, not RCIR's own claim vocabulary) to a display
  label. A label can only ever change identifier *display* — an action's own claim
  heading, and a new "Glossary" reference section listing every accepted label —
  never modality, negation, or conditional structure; v1 does not substitute a
  label inside rendered expression/condition text. A duplicate label target is
  always `FSL-DOC-LABEL-CONFLICT` (detected via a hand-written `serde` `Visitor`
  that preserves every JSON object entry including duplicates, since ordinary
  deserialization into a map silently collapses a repeated key to its last
  occurrence before the code could ever see the conflict); an unresolvable target
  is `FSL-DOC-LABEL-UNKNOWN` (warning by default, error under `--strict`). The
  glossary is threaded only into rendering, never into RCIR projection, so
  `claim_set_digest`/`spec_digest` are provably unaffected by a glossary edit
  while `artifact_digest` changes; a new frontmatter key, `glossary_digest`,
  lets `fslc document check --glossary glossary.json` detect a glossary that
  differs from generation time as its own drift reason
  (`glossary_changed`/`FSL-DOC-GLOSSARY-CHANGED`), gating per-claim-body and
  residue comparison the same way a changed renderer already does. See
  `docs/DESIGN-document-glossary.md`.
- Added an explicit RCIR coverage registry and a no-silent-omission gate (issue
  #328). `fsl_tools::RCIR_TARGET_KIND_REGISTRY` names every semantic-target kind
  the RCIR v1 projector (#325) can classify (`action`, four `property:*` kinds,
  `terminal`, `acceptance`, `forbidden`, `init`, `projection`, `refinement`) and
  whether it renders as a claim or reports as unsupported. Two coupled-change
  tests keep it honest: one cross-references the Kernel-native rows against
  `kernel.v1.schema.json`'s own required-key lists (catching a wholly new
  Kernel-level construct that the projector was never updated to handle at all,
  which the projector's existing runtime completeness invariant cannot see on its
  own), the other cross-references every kind actually observed across the
  fixture corpus. A third test pins the projection-completeness guarantee itself
  (`rendered + unattributed + unsupported == authored`, as a disjoint-union-of-
  sets check, not just a count). `--strict` erroring on an unattributed or
  unsupported authored target was already implemented in #327 and needed no
  change. See `docs/DESIGN-document-coverage-registry.md`.
- Added the `fsl-requirements-document` Agent Skill (issue #331):
  `skills/fsl-requirements-document/SKILL.md` (symlinked under `.claude/skills/`
  and `.agents/skills/`, per the existing skill convention) documents the
  `fslc document generate`/`claims`/`check` workflow (issues #325-#329) for an
  agent used as a non-normative editor and review-support assistant, never a
  compiler: permitted operations (editing the `background` slot, drafting
  glossary candidates, flagging a source-text/formalized-meaning mismatch as
  advisory) and forbidden ones (rewriting a generated claim block, inventing
  normative language, rewording an assurance class, collapsing a many-to-many
  requirement relation), a CI example that fails a build on `document_drifted`,
  and cross-references from `fsl`/`fsl-requirements` clarifying the handoff.
- Added generated block markers and `fslc document check` (issue #329). Every
  `fslc document generate` artifact now carries a frontmatter block
  (`fsl_document_schema`/`view`/`lang`/`source`/`renderer`/`renderer_version`/
  `normative_scope`/`spec_digest`/`claim_set_digest`) and wraps each claim's rendered
  block in `<!-- fsl:claim begin id="..." digest="sha256:..." --> ... <!-- fsl:claim
  end -->` markers, where the digest is a new `fsl-doc-claim-block-v1+sha256` hash of
  the exact rendered text (`fsl_tools::framed_text_digest`) — distinct from RCIR's own
  semantics-only `claim_digest`, which never sees rendered prose and so cannot by
  itself detect a hand-edited sentence. A new fixed, non-normative `background` slot
  (`<!-- fsl:slot begin name="background" normative="false" --> ... <!-- fsl:slot
  end -->`) is the only place free text may be edited. `fslc document check <spec.fsl>
  <document.md>` re-projects and re-renders the spec (reading the locale and source
  label back from the artifact's own frontmatter, so a check run from a different
  working directory than generate doesn't report false drift from a path-spelling
  difference) and reports `document_conformant` (exit 0) or `document_drifted` (exit
  1) with a `reasons` array — `claim_changed`/`claim_missing`/`claim_duplicate`/
  `claim_unknown`/`claim_reordered`/`slot_missing`/`slot_duplicate`/`slot_unknown`/
  `marker_malformed`/`edit_outside_slot`/`renderer_changed`/`spec_digest_mismatch`/
  `claim_set_digest_mismatch`, each carrying its `FSL-DOC-*` diagnostic code. No
  natural language is interpreted; only frontmatter values, marker structure, and
  claim-block digests/text are compared. See
  `docs/DESIGN-document-generated-markers-and-check.md`.
- Added `fslc document generate` and `fslc document claims` (issue #327), the CLI entry
  points for the RCIR v1 projector (#325) and its ja/en controlled-language renderer
  (#326). `document generate <spec.fsl> [--view requirements] [--lang ja|en] [--strict]
  [--strict-rendering] [-o requirements.md]` renders a deterministic requirements
  document; `document claims <spec.fsl> [-o requirements.claims.json]` emits the raw
  RCIR claim set as schema-conformant JSON so agents/tools consume the public contract
  instead of re-parsing `.fsl`. Both follow the existing `ledger`/`html`/`testgen`
  convention: with `-o`, the artifact is written to disk and a JSON envelope
  (`spec_digest`/`claim_set_digest`/`artifact_digest`, coverage counts, provenance
  completeness) goes to stdout; without `-o`, the raw content is printed directly.
  `--strict` fails closed (`FSL-DOC-UNTAGGED-TARGET` / `FSL-DOC-UNSUPPORTED-TARGET`) on
  any authored target the projector could not attribute to a requirement or could not
  project; `--strict-rendering` fails closed (`FSL-DOC-FORMULA-FALLBACK`) on any
  expression the renderer could not phrase in natural language. A specification with no
  requirement IDs at all (`FSL-DOC-NO-REQUIREMENTS`) is always an error. See
  `docs/DESIGN-document-cli.md`.
- Added the controlled-language renderer (issue #326):
  `fsl_tools::render_requirements_document` compiles an RCIR v1 claim set (issue #325)
  into deterministic Japanese/English Markdown, using one fixed template per claim kind
  and a safe-pattern expression recognizer (equality/comparison, `and`/`or`/`not`,
  `forall`/`exists`, `count`/`sum`/`unique`/`exactlyOne`, `old(...)`, option `is none`/
  `is some`) that falls back to the canonical FSL text (never a paraphrase failure) for
  anything outside that whitelist, counted in `formula_fallback_count`. `requires` always
  reads as an enablement condition, `not`/negation is never dropped, weak fairness is
  always "a scheduling assumption" and never "immediately", `acceptance`/`forbidden`
  always carry a non-generalization disclaimer, and a progress/reachability claim never
  claims established evidence (RCIR v1 carries none). The renderer verifies the embedded
  Public Kernel, reprojects and exactly matches the complete RCIR renderer role sidecar against
  its checked Kernel/Model/trace and original source inputs, and requires every semantic target to
  resolve exactly once before emitting Markdown, failing closed on any mismatched input.
  `fsl_core::expr_text`/
  `source_expr_text` (the `explain --readable` canonical-text renderer) moved from the
  `fslc` binary crate into `fsl_core` so `fsl-tools` could reuse it without a
  second implementation; `fslc` re-exports both names unchanged. CLI wiring is issue
  #327. See `docs/DESIGN-document-controlled-language-renderer.md`.
- Added the Requirement Claim IR (RCIR) v1 schema and native projector (issue #325),
  the semantic foundation used by the `fslc document` requirements-document workflow.
  `fsl_tools::project_requirement_claims_from_source` compiles a checked `spec` or
  `requirements` dialect model into `schemas/fslc/document/requirement-claims.v1.schema.json`:
  nine claim kinds (`operation`, `state_rule`, `transition_rule`, `progress_rule`,
  `reachability_goal`, `acceptance_trace`, `forbidden_trace`, `deadline_rule`,
  `terminal_rule`), full many-to-many requirement relations (no singular compatibility
  projection), a fail-closed `rendered`/`unattributed`/`unsupported` coverage
  partition over every authored semantic target, `spec_digest`/`claim_set_digest`/
  `claim_digest` (mutation-sensitive, comment/formatting-stable), and provenance
  reusing the Public Kernel v2 assurance vocabulary (falling back to the checked
  declaration's own span, never a guess, when the origin registry has no chain for
  a target). RCIR is not a second semantics: it embeds the schema-validated Public
  Kernel v2 artifact, while claims expose typed subjects and stable target references
  instead of a second, unconstrained Python-shaped AST. Trace-only expressions reuse
  the Public Kernel expression contract. The controlled-language renderer, CLI, drift
  checking, no-silent-omission gate, glossary, evidence overlay, and approval integration
  are included in the same release through issues #326-#334. See
  `docs/DESIGN-document-requirement-claim-ir.md`.
- `fsl_tools::undecided` gains `undecided_records`, a typed variant of
  `undecided_declarations` (issue #189) that additionally carries the annotation's
  source span; the existing JSON-producing function is now a thin projection over it
  with unchanged output.
- Documented that the persistent verify cache is safe under concurrent
  processes: writes are atomic, so parallel `fslc verify` invocations over many
  files (`xargs -P`, CI job matrices) at worst duplicate solving and never
  corrupt or poison the cache. `docs/DESIGN-incremental-verify.md` §3 and
  `skills/fsl/reference.md` now state this explicitly so users and AI agents
  choose process-level parallelism instead of a sequential per-file loop
  (issue #353). No implementation change.

## [3.0.0] - 2026-07-17

### Changed
- Native v3 releases now assemble and validate all four CLI/LSP pairs, checksums,
  VSIX, and Kernel bundles before one tag-only publication step. Release actions,
  the Rust toolchain, and the VSIX packager are pinned; publication verifies a
  remote draft inventory before making it public, and manual dry runs validate
  the same assembled unit without publishing it. Native crates and release
  builds require Rust 1.88 or newer.
- The installer rejects unsupported Intel macOS, pins repository content and
  binaries to the same latest Release tag, and stages both checksummed native
  binaries before replacing either installed command. Linux artifacts now have
  an explicit Ubuntu 24.04 / glibc 2.39 baseline. All native targets build the
  vendored Z3 4.16 source so the distributed binaries do not require `libz3` or
  `z3.dll` at runtime. macOS builds use the Clang 17 runner required by Z3 while
  retaining a macOS 14 deployment target, and CI timeouts accommodate clean Z3
  source builds on every supported runner.
- Release distribution checks now run in the authoritative native Rust gate;
  the redundant Python-only release workflow test was removed.
- Native `fslc verify` cache keys now use a build-time implementation fingerprint instead of
  hashing the full executable on every invocation, and CLI preparation reuses its validated
  `KernelModel` across property selection and engine dispatch. Cache schema v2 invalidates old
  entries fail-closed while removing the engine-independent fixed cost found in issue #349.

### Added
- Documented a rationale convention for preserving a declaration's "why" in
  the checked model instead of only in `//` comments (lexer trivia): use the
  existing `@kind(id, text?)` to classify and explain a declaration in one
  line, and the recommended custom namespace `@doc.rationale("...")` for a
  short rationale that isn't a classification. No grammar, IR, or schema
  change; multi-sentence narrative keeps living in comments, and generic
  annotations are not currently projected through JSON, LSP, or the audit ledger. See
  `docs/LANGUAGE.md` §13.1.2 (mirrored in `docs/LANGUAGE.ja.md`),
  `docs/DESIGN-annotations.md`, and `skills/fsl/reference.md`.
- `fslc lint` now enforces a built-in, kind-aware canonical ID policy and accepts
  an explicit `--project fsl-project.toml` override. Requirement, acceptance,
  forbidden, policy, goal, control, model, and assumption IDs have distinct
  default templates; project tables can replace individual kinds while retaining
  all other defaults. Results record the resolved policy, invalid configuration
  fails closed, zero-padded numeric ID components retain their exact source
  spelling through parsing/lowering, and ID findings are deliberately non-machine-applicable so
  `migrate` never guesses cross-artifact renames. Typed `@requirement` annotations
  are the canonical declaration-link syntax; legacy `"ID: text"` metadata remains
  migration input.
- `docs/intro/language.ja.html` now renders from a new, section-aligned Japanese
  translation, `docs/LANGUAGE.ja.md`, instead of reusing the English `docs/LANGUAGE.md`
  body verbatim. FSL keywords, `fslc` commands/flags, diagnostic/result identifiers,
  JSON, and fenced code blocks stay untranslated and byte-identical to the English
  source; only headings and prose are Japanese. `tools/build_site_reference.py` fails
  loudly if the two files' `## ` section counts drift out of 1:1 alignment, and page
  anchors/`SECTION_BLURBS` lookups stay keyed off the English heading so cross-page
  links are unaffected. This supersedes the earlier "no translation" stance (see
  `docs/DESIGN-docs-site.md` D7); `docs/LANGUAGE.ja.md` now moves alongside
  `docs/LANGUAGE.md` under the "a language feature moves all of its files together"
  rule (`AGENTS.md`).
- Literate Markdown FSL: `fslc check`, `fslc verify`, and `fslc scenarios` now
  accept `.md` files containing ` ```fsl ` fenced code blocks directly. Lines
  outside fsl blocks are blanked in place so that diagnostic positions (line
  numbers, columns, counterexample locs) point to the original Markdown
  document. Multiple fsl blocks form one compilation unit; files without fsl
  fences are rejected with a clear diagnostic. Fence detection follows the
  CommonMark grammar (backtick or tilde runs of length >= 3, matched by
  character and length), so a non-fsl fence can safely contain a literal
  ` ```fsl ` example. The verify cache key is stable across repeated runs of
  the same document (issue #193).
- `fslc-lsp` is now a native Rust language server backed directly by the authoritative
  syntax, core, and analysis implementation. Existing diagnostics, navigation, symbols,
  rename, semantic tokens, completion, hover, and code actions no longer require Python;
  corpus/index coverage and stdio lifecycle tests run in the Rust workspace (issue #310).
- Required product CI now has one Rust-native integration entrypoint covering the workspace,
  dependency boundaries, CLI/schema contracts, native/browser agreement, and WASM. The redundant
  Python CLI parity job and its frozen-surface exception snapshot are removed; compatibility Python
  tests remain explicit manual evidence. FSL source locations, testgen templates, and generated
  artifact digests are portable across the supported native runner matrix (issue #307).
- `fslc analyze --projection code_audit --code <path>` now maps checked
  executable Kernel requirement targets to a closed, language-independent
  `@fsl.trace` annotation convention. Deterministic JSON reports complete,
  partial, missing, orphan, and target-mismatch coverage without presenting
  structural claims as proof; versioned input/output schemas and a compliant
  fixture define the public contract (issue #222).
- Replay-trace schema `1.2.0` opts into solver-free bounded `leadsTo`
  monitoring over initial, action, and stutter observations. Inclusive deadline
  failures carry property/binding/timing evidence; successful output separates
  safety from bounded liveness, preserves finite-prefix `pending`, and names
  unchecked unbounded properties. Native BMC and a test-only Python oracle
  cross-check the monitor, and positive/overdue NFR fixtures ship in both Public
  Kernel release bundles (issue #225).
- Replay-trace schema `1.1.0` adds explicit `action: null` observation points.
  Equal-state stutters preserve the projected action trace; reported transient
  implementation states are nonconformant while unreported intermediates are
  outside invariant judgment. Replay now delegates action outcomes, partial
  guards, and rollback to `Monitor::attempt`, with differential fixtures in both
  Public Kernel release bundles (issue #224).
- `fslc replay --trace` now consumes the closed `replay-trace.v1` external
  compiler contract: trace/Kernel versions, exact spec action/parameters,
  canonical ticks, complete typed initial and post-action state, and optional
  opaque producer timestamps. Typed observation differences return leaf-level
  nonconformance while malformed/incomplete contracts fail closed. Positive and
  negative goldens plus the schema ship in both Public Kernel release bundles;
  legacy action-only arrays/`{events}` remain an explicit unversioned adapter
  (issue #221).
- `fslc lint` now reports edition-aware diagnostics with stable taxonomy,
  severity, spans, canonical replacements, and machine applicability. `fslc
  migrate` is dry-run by default and `--write` atomically applies only a fully
  parsed, checked, location-free Public-Kernel-equivalent file set. Shared
  formatter edits cover legacy enums/operators and quantifiers; typed metadata,
  unambiguous local action correspondences, and implicit defaults use semantic
  planners. Unsafe comment movement, branch/duplicate mappings, and invalid
  `&&` are explicit refusals. The native LSP exposes applicable diagnostics
  as quick-fix Code Actions (issue #249).
- `fslc fmt` now formats one registered FSL document or stdin to canonical
  stdout without mutating it; `--check` accepts multiple paths and reports a
  machine-readable 0/1/2 result. A lossless token/trivia tree preserves line
  comments, blank lines, raw spelling, spans, and annotation attachment while
  domain enum/logical and quantifier legacy forms normalize to accepted
  equivalents. Corpus-wide idempotence and location-free semantic round trips
  cover every registered dialect; opaque agent bodies and ambiguous
  comment-bearing structural rewrites fail at exact spans (issue #248).
- `fslc verify --engine auto` composes the explicit-state and BMC engines:
  explicit runs first (faster, and can prove `closure: true`) and falls back
  transparently to BMC exactly when explicit cannot decide the spec on its
  own (an unsupported feature such as `leadsTo` or nondeterministic init, or
  `unknown_budget`). Every result carries `engine: "explicit"`/`"bmc"` naming
  whichever engine decided, and a fallback additionally carries
  `engine_fallback: {from, reason, kind}` so a caller can distinguish a
  bounded BMC verdict from an unbounded explicit one, and a permanent
  unsupported-feature gate from a transient budget one, without parsing
  prose. `auto` shares verdict-cache entries with plain `--engine
  explicit`/`bmc` runs of the same spec — the cache key is always the engine
  that actually decided, never `auto` itself — and does not change the
  default engine or extend to any other subcommand's `--engine` option
  (Rust-only, issue #226).
- Finite quantifiers, `count`, `sum`, `unique`, and `exactlyOne` now share one
  Binder/Aggregate IR across typed, range, Set, and Seq domains. Optional
  filters use one scope/type-check path; empty aggregates are zero and Seq
  duplicates preserve positional multiplicity. Legacy colon quantifiers remain
  detectable as non-canonical input, business/requirements KPI declarations
  survive as typed metadata projections, and collection aggregates normalize to
  existing Public Kernel v1/v2 expression shapes (issues #242 and #217).
- Requirements expressions now accept `stage(entity)` and qualified
  `<process-path>.stage(entity)` through the same structural node and resolver
  as business expressions. Entity-typed binders/parameters select the process;
  qualified process declarations disambiguate shared entity types; generated
  stage maps remain Kernel detail while public origins, explain output, and
  violations retain the source accessor (issue #243).
- Conditional expressions (`if condition then a else b`) are now accepted in
  every expression context. The shared parser replaces the refinement-only
  path; concrete, explicit, symbolic, analysis, mutation, formatter, LSP, and
  Public Kernel paths use one node, and partial operations are evaluated only
  on the selected branch (issue #245).
- `Option<T>` now supports structural `==` / `!=` between complete Option
  values, including `some(expr)`. Concrete and symbolic evaluation compare the
  presence tag first and ignore the payload when absent; `is some(binding)`
  remains the distinct binding form (issue #246).

### Fixed
- Concurrent `fslc check`, `verify`, and `scenarios` commands on the same
  literate Markdown source now use process-owned materializations, so one
  command cannot delete another command's source mid-read. Verification cache
  identity remains the original Markdown path, preserving cross-process hits,
  while dependency invalidation follows the resolver base for symlink aliases.
- Native `check` now propagates malformed governance-preservation diagnostics
  as a located type error with exit 2 instead of silently returning `ok`. A
  resolver-backed core contract also rejects missing dependencies, unknown or
  unsatisfied controls, duplicate declarations, unknown artifacts, empty
  preservations, and referenced-name mismatches without last-write-wins data
  loss. Native CLI and browser Worker now share these checks and progress-
  preservation verification, including process-exit and browser parity cases.
- Model construction now rejects duplicate action names, duplicate action
  parameters, and empty inline parameter ranges before name-based runtime
  dispatch or finite-domain enumeration can become ambiguous.
- The native concrete `Monitor` now revalidates a previously enumerated action
  against the current state, rejects parameters outside their declared
  Bool/enum/range/domain, and shares one guard-evaluation path across
  enumeration, direct attempts, and selected steps. Builtin Bool action
  parameters are accepted as documented while unbounded/non-scalar parameters
  fail during model construction. A two-stage FSL requirements model captures
  fresh selection, stale reuse, and raw-input rejection; native
  symbolic/concrete differential tests and mutation evidence supply its
  implementation anchors and negative controls.
- Python compatibility envelopes now scope faithfulness routing to actual
  diagnostics, so user state fields named `kind`, `classification`, or other
  diagnostic discriminators neither crash nor receive routing metadata (issue
  #278).
- `fslc verify --engine auto` no longer silently drops its `engine_fallback`
  annotation, or returns a stale/completeness-downgraded BMC verdict, when a
  warm cache entry was written by an unrelated plain `--engine bmc` run: the
  fallback gate is recomputed for the current invocation instead of being
  read back off the cache entry, so a warm hit always matches what a fresh
  `auto` run would report, and the persisted cache entry itself carries the
  same fields as a plain engine run's, with the `engine_fallback` sibling no
  longer written at all (issue #226 follow-up).

### Changed
- Required product CI now runs the Rust workspace and WASM integration phases in parallel while
  preserving `tools/check-native-integration.sh` as the complete local gate. The pinned
  `wasm-bindgen-cli` binaries are cached by runner OS and version instead of being rebuilt on every
  run.
- The internal `release` Skill now operates a GitLab Flow-inspired
  `short-lived branch -> main -> production -> vX.Y.Z` lifecycle. It separates
  integration evidence from release readiness, omits permanent `develop` and
  `pre-production` branches, defines promotion, stabilization, and hotfix
  procedures, and keeps component behavior independent of branch names. Codex
  and Claude share the same internal Skill definition.
- Releases now follow one documented, agent-operable procedure that gates the
  Rust version/changelog PR, exact-SHA `main` to `production` promotion,
  four-target workflow dry runs, confirmed annotated tag push,
  changelog-derived GitHub Release notes, and post-release asset/version checks.
  Intel macOS binaries are no longer built or advertised (issue #335).
- Chronological field-trial reports were distilled into authoritative design
  contracts, language/skill rules, maintained examples, and executable regression
  tests, then removed as a parallel documentation source. The native pytest
  generator now also sanitizes composed scenario display names with deterministic
  collision suffixes while preserving the original name in the docstring. Literate
  Markdown commands now use process-isolated materializations while normalizing
  cache identity to the source document, preventing concurrent checks from deleting
  one another's input.
- Native Rust internals now separate top-level CLI routing from command-family parsing and base
  ledger generation from approval evaluation. Observable CLI, ledger, and approval digest
  contracts are unchanged.
- `fslc verify --engine auto` (issue #226) now dispatches through a
  `VerificationEngine::Auto` variant alongside the other engines instead of
  resolving before the shared parser, and loads the model once per fresh
  attempt (the static fail-closed gate and the real explicit-state run share
  it, removing a redundant load/gate re-check on the common decide-by-explicit
  path). A cache hit recomputes that same fail-closed gate for the current
  invocation rather than persisting a fallback trace on the BMC cache entry
  (see the Fixed entry above), so a warm `auto` hit always reproduces the
  exact `engine_fallback` a fresh run would. Output contract, cache-sharing
  with plain `--engine explicit`/`bmc` runs, and the `engine_fallback: {from,
  reason, kind}` shape are unchanged.
- Native domain TypeScript, Python, Kotlin, Swift, and Rust scaffolds now share
  one versioned input adapter over Public Kernel v1 and the public
  `domain-scaffold-metadata.v1` compatibility bridge. Target emitters no longer
  receive `DomainSpec`; incompatible schemas and missing lowered member
  counterparts are rejected, all five outputs match pre-migration goldens, the
  valid domain corpus generates for every target, and `domain testgen` reuses
  the same TypeScript adapter/effect emitter instead of a duplicate
  implementation (issue #213).
- Native pytest, Vitest, Swift, Kotlin, Dart, and PHPUnit test generation now
  shares one fail-closed adapter over Public Kernel v1 metadata, scenario JSON,
  and the new versioned `testgen-trace.v1` fixed-seed conformance trace. Target emitters no longer
  receive the private model/AST, and their byte output is unchanged. Compose
  uses an explicit checked names/order bridge until Public Kernel can publish
  truthful multi-file provenance (issue #214).
- Standalone refinement files, requirements `implements` blocks, action-level
  `maps` clauses, and synthesized auto/identity mappings now lower through one
  typed `ActionCorrespondence` IR. Impl parameter annotations, target arity and
  argument types, actor compatibility, duplicate origins/spans, progress lookup,
  and concrete refinement execution share that validation path; requirements
  auto-mapping no longer has a separate indexing path (issue #238).
- Native/WASM parity now runs every Worker-supported surface document from the
  shared `specs/` and `examples/` corpus and structurally compares complete
  envelopes. Only schema-validated timing/backend identity and replay-validated
  witness state values and commuting-step order are normalized while state
  keys, action/source identity, and change shapes remain exact; structured failures retain difference paths,
  both envelopes, and version metadata. Native and browser Z3 seeds are fixed,
  and the gate includes a pre-#267 duplicate-write regression (issue #273).
- Distribution is now fail-closed: every Rust workspace crate is marked
  non-publishable, the dormant PyPI workflow and obsolete Python/PyInstaller
  binary path are removed, and GitHub Releases are documented as the only
  official distribution surface (issue #272).
- Native, induction, explicit, and browser verification results now share one
  fixed `cost` schema. Z3 backends report check count/time, nullable common
  conflict/decision/propagation/memory statistics, and deterministic
  per-property check attribution; native and Worker parity tests enforce the
  same keys and nullability (issue #271).
- Native CLI and browser Worker result envelopes now identify the verifier,
  `fsl-core`, and loaded Z3 versions through one `versions` schema. Native
  verdict-cache keys and entries use the linked Z3 runtime version instead of a
  hard-coded release string (issue #268).
- Domain finite variants now use canonical `enum Name { Member, ... }` syntax,
  while bounded numeric domains remain `type Name = lo..hi`. The 2.x legacy
  union spelling remains compatible with a stable
  `deprecated_domain_enum_union` diagnostic and loss-aware replacement;
  `--edition next` rejects it. Canonical and legacy forms share the same typed
  model, Kernel lowering, and verdict, and public examples now use the
  canonical spelling (issue #244).
- Native Rust domain lowering now carries a non-serialized origin chain from typed
  domain nodes through Surface/Kernel models into validation, verification,
  counterexamples, and `explain`. Source identity/full spans, declaration
  paths, lowering steps, primary/secondary sources, one-to-many/many-to-one,
  and generated-only nodes remain distinguishable; requirement traceability is
  stored separately. Diagnostics prefer user declarations and retain generated
  Kernel names as machine detail. Public Kernel v1 schema/version/goldens and
  default `fslc kernel` output remain unchanged (issue #240; v2 publication is
  tracked by #256). Rust consumers should construct parser/model values through
  the supported parse/build APIs rather than struct literals because the
  internal span and origin carriers add fields to those implementation types.
- Native Rust domain lowering (issue #239) now resolves typed domain symbols,
  enum members, `can()`, finite membership, state reads, and nested lvalues
  structurally, then builds Kernel AST directly without generated-source
  render/reparse or substring semantics. Resolver/type failures report original
  domain spans; empty membership and map-state initialization are explicit;
  characterization now records direct-lowering origins and fail-closed checks.
- Native Rust domain parsing (issue #236) now retains defaults, ranges, guards,
  rejection conditions, assignments, invariants, stale policies, effect
  correlation/idempotency expressions, and saga guards as unresolved typed
  syntax nodes with exact source spans. Field identifiers/type references and
  declaration nodes are span-aware as well. Domain-only membership and legacy
  logical spellings are structural, effect paths retain their prior restricted
  grammar, existing lowering remains compatible through a rendering adapter,
  and malformed expressions now report their original domain-source location
  instead of a generated Kernel coordinate.

### Fixed
- Native and browser verification now share duplicate-write validation and
  verification warning generation. Duplicate writes are rejected while building
  the checked Kernel model for `check` and `verify`; model, vacuity, deadlock,
  and action-coverage warnings come from solver-independent shared crates rather
  than frontend-local implementations. The Worker no longer accepts ambiguous
  write order or emits an unconditional empty warning list. Indexed writes that
  may alias are rejected unless constant indexes prove them distinct, and
  induction selects typed warning kinds instead of message substrings (issue
  #267).
- The bounded `leadsTo ... within N` deadline check no longer misses a
  violation when the path deadlocks after the deadline. The deadline probe ran
  as a single post-hoc pass after the BMC unrolling loop, by which point every
  non-final step had a permanently-asserted forward transition, so a
  missed-deadline path that subsequently deadlocked was excluded from the
  solver context and the spec falsely verified at any `--depth` past the
  deadlock. The probe now runs inside the per-step loop, anchored at each
  window's deadline step before that step's forward-transition assertion —
  the same restructuring issue #260 applied to the stagnation check
  (issue #266).
- Bounded `leadsTo` deadlock-stagnation detection no longer requires `--depth`
  to land exactly on the stalling step. The BMC unrolling loop permanently
  asserted a forward transition out of every non-final step before the
  deadlock-stagnation check ran as a single post-hoc pass, so a deadlocked
  path below the requested depth was excluded from the solver context and the
  spec falsely verified. The check now runs inside the per-step loop, before
  that step's forward-transition assertion, matching the frozen Python
  reference's existing behavior (issue #260).
- The browser Worker now reports `leadsTo` violations. The Worker envelope
  previously ignored the bounded verifier's leadsTo verdict, so a spec whose
  `must eventually` policy is violated was reported as `verified` in the
  browser while the native CLI reports `violated` — a confidently green false
  negative. The Worker now emits the native CLI's leadsTo fields (bindings,
  `pending_since`, `stutter`, `hint`, and the counterexample trace).
- The browser Z3 bridge now builds if-then-else terms through the z3-solver
  `If` API and resolves constant sorts from their FSL sort descriptors instead
  of a TypeScript-only `__typename` marker that does not exist at runtime.
  Previously any spec with `Bool` state or `count` aggregation failed in the
  browser Worker with an internal error while verifying natively.
- Windows native CI now keeps the conformance coverage and approval Markdown
  snapshots on LF and canonicalizes both the specification and repository-root
  paths before binding approval records to Git, avoiding false snapshot drift
  and false "outside its Git repository" errors without weakening exact checks.
- Rust CLI contract validation (issue #220) now preserves exact compatibility
  with the frozen Python surface while checking native-only commands and
  options against an explicit structural allowlist. CI runs the focused
  contract and help-parity suite, including a mutation regression that rejects
  unlisted choice drift and runtime probes for invalid choices and help paths.

### Added
- Native Rust now accepts `@requirement(id, text?)`, `@undecided(reason)`,
  `@kind(id, text?)`, and multi-segment custom-namespace annotations directly
  on a declaration (`init`, `action`, `invariant`/`trans`/`reachable`/`until`/
  `unless`/`leadsTo`, a process `transition`, and `requirement`/`acceptance`/
  `forbidden` blocks) in the spec/business/requirements/compose dialects, not
  only before a document's dialect keyword. A shared parser helper backs both
  attachment points — no per-dialect copy. The new syntax and the legacy
  string-metadata/`covers` forms desugar to the same typed relation and union
  when both target one declaration; a requirement block's annotations fan out
  to every action/property it contains. A malformed or stray annotation
  (nothing to attach to) reports a precise coded diagnostic
  (`FSL-ANNOTATION-TARGET`/`-ARGUMENTS`/`-PATH`/`-SYNTAX`). Public Kernel v1/v2
  JSON and `python_ast` projections remain unchanged (issue #241).
- Native Rust now also accepts the same declaration-level `@...` syntax on
  `domain` (aggregate `command`/`decide`/`evolve`/`invariant`, `projection`,
  `effect`, saga `step`), `ai_component` (`tool`, `authority` and its
  individual rule lines, `fallback` and its `when` items, `check`), and
  `dbsystem` (`migration`, `check compatibility` rule lines) nested
  declarations. A `command`/`decide`/matching-`evolve` group unions onto the
  one action they generate together; an `effect` or saga `step` broadcasts to
  every action it generates. `AiAuthority`'s `may_suggest`/`may_execute`/
  `requires_human_approval`/`forbidden` rule lists are now span/annotation-
  carrying `AiAuthorityRule` nodes while `python_ast()` still projects the
  original plain string-array JSON shape. A `dbsystem` migration or
  compatibility-rule annotation reaches the checked model and TSG/ledger by
  rendering back to `@...` source rather than through the lossy legacy
  `quote_meta` string convention. Public Kernel v1/v2 JSON and `python_ast`
  projections remain unchanged (issue #281).
- Approval records can now opt into detached Ed25519 signatures with
  `approval create --signing-key`. The strict v2 schema binds the complete
  canonical record, while `approval check`, `approval diff`, and `ledger`
  require explicit repeatable trust anchors and distinguish signed, unsigned,
  and signature-invalid evidence. Unsigned v1 behavior is unchanged (issue
  #269).
- Native dialect selection now lexes each document once and dispatches through a
  duplicate-checked keyword registry shared by Kernel, CLI, WASM, and mirrored
  Python/LSP entrypoints. Leading BOM/comments/whitespace and typed top-level
  annotations are handled before selection without mistaking annotation arguments
  for dialects; specialized frontends consume the original token stream. Add
  exact empty/unknown diagnostic codes, spans, and supported-keyword lists, an
  evidence-only `agent` surface, and span-retaining multi-segment `SymbolPath`
  values. Qualified syntax shares the same arbitrary-depth path and per-segment
  spans, dispatch errors expose machine-readable supported dialects, and LSP
  diagnostics route annotated agent/AI-project sources consistently (issue #247).
- Native Rust now carries ordered, typed `Requirement`, `Undecided`, `Kind`,
  and namespaced `Custom` annotations through a common target-keyed IR. Legacy
  declaration strings, spec badges, requirement blocks, process `covers`, and
  acceptance/forbidden IDs adapt into the same validated relation; identical
  requirements deduplicate, conflicting text fails at its annotation span, and
  semantic queries are order-independent. Explicit requirement forms retain
  exact spans and reject reserved `undecided` IDs. Strict tags, analysis/TSG,
  scenarios, verifier diagnostics, and ledgers consume every relation while
  retaining lexical singular projections. Public Kernel v1/v2 JSON remains
  unchanged (issue #237).
- Opt-in Public Kernel v2 publishes the internal domain origin chain as a
  deterministic provenance graph with portable typed source identities,
  UTF-8 byte and Unicode-scalar coordinates, primary/secondary origins,
  lowering steps, generated-only/unknown assurance, exact-revision source-node
  IDs, target bindings, reverse lookup, and machine-readable completeness.
  Rust and CLI callers explicitly negotiate `PublicKernelVersion::V2` /
  `--kernel-version 2`; unsupported majors fail closed. Add matching v2 Kernel,
  conformance, and provenance-coverage schemas/goldens plus an independently
  checksummed v2 release bundle. Public Kernel v1, its goldens, default CLI,
  and compose rejection remain unchanged (issue #256).
- The browser Worker verify envelope now carries the counterexample `trace`
  (per-step state, action, and changes) for invariant and type-bound
  violations, matching the native CLI trace shape. Browser clients previously
  only received the violated property name and step.
- Kernel state fields now accept deterministic inline initializers and normalize
  them to the existing `init` assignment semantics across Monitor, explicit
  exploration, BMC, induction, and Public Kernel v1. State-reading/order-dependent
  expressions and inline/explicit root overlap fail with source diagnostics.
  Existing domain enum/Bool/range/external defaults and requirements number
  lower-bound defaults now emit the edition-aware `implicit_initial_value`
  warning with the selected value, reason, and machine-applicable insertion edit;
  requirements Bool/enum fields remain explicitly initialized (issue #250).
- Domain-expression characterization corpus (issue #235): Rust CI now freezes
  pre-typed-AST surface expressions and locations, normalized semantic model
  structure, representative public Kernel expression/origin output, generated
  Kernel fragments, CLI diagnostics and verification traces, and concrete
  Monitor-to-symbolic agreement. The corpus covers logical operators, enums,
  membership, `can`, aggregate state, root/index/field lvalues, defaults,
  invariants, stale policies, effects, sagas, invalid expressions, and a
  deterministic AI-native prompt/spec attempt baseline. This is migration
  evidence only and intentionally changes no language semantics.
- Codex CLI, IDE, and desktop environment: bounded repository instructions, worktree-local
  task packets, explicit checkpoint/task-start skills, shared FSL skill discovery,
  read-only exploration and review agents, concise SessionStart context, and CI-tested
  configuration contracts.
- Rust-authoritative Claude Code environment: concise shared instructions, path-scoped rules,
  worktree-local task packets, checkpoint/task-start skills, native verifier hooks, focused
  exploration/diagnostic/review agents, publication permission prompts, and CI-tested hook contracts.
- Conformance corpus feature coverage matrix (issue #223): native Rust
  `fslc_rust::coverage::coverage_matrix()` structurally cross-references the
  fixed `kernel_contract.fsl`/`conformance_failures.fsl` fixture manifest
  against their generated contracts and conformance vectors, reporting for
  every kernel `semantics` key, `outcome.kind`, value encoding, partial
  operation, quantifier, finite parameter domain, and requirement ID whether
  it is `exercised` (a real vector demonstrates it) or `declared` (the
  contract states it but no vector in the corpus can fire it — reserved for
  `terminal_deadlock`, `fairness_weak`, and the six `partial_op_*` rows). No
  row is hardcoded; `coverage_matrix()` itself returns `Err` naming every
  feature that falls short of its required level, so a newly uncovered
  feature fails CI loudly instead of shipping silently. Closes real gaps the
  matrix caught: the corpus never fired a `trans` violation, never
  distinguished Euclidean from truncating integer division, and never
  carried a `Bool` value — `conformance_failures.fsl` gained a
  `regress`/`Monotone` pair, a literal negative-operand `euclid_divide`
  action, and a `flag`/`flip` pair, and its golden vectors were
  regenerated (also updating `fsl-verifier`'s failure-fixture agreement test
  to check the new successful transitions with the same Monitor↔symbolic
  path used for `kernel_contract.fsl`, since not every action in the fixture
  is a deliberate failure anymore). Add
  `schemas/fslc/kernel/conformance-coverage.v1.schema.json`, golden JSON/
  Markdown fixtures, `rust/fslc/tests/conformance_coverage.rs` (schema/
  outcome-kind sync, required-level enforcement, golden equality, schema ID
  sync), a release-bundle update, and a "Conformance coverage matrix"
  section in `docs/DESIGN-kernel-contract.md`. No new CLI surface; the
  frozen Python reference remains unchanged.
- Public-Kernel-backed native typestate generation (issue #215): Rust
  `fslc typestate` now performs applicability analysis and TypeScript scaffold
  generation from versioned public Kernel JSON v1 instead of private
  `KernelModel` structures. The adapter validates schema identity/version,
  restores declaration order from public spans, and preserves existing report
  and `--ts` bytes with golden tests. The old private-model adapter is retired
  from the native CLI path; the frozen Python reference remains unchanged.
- Explicit-state exploration engine (issue #212): native Rust
  `fslc verify --engine explicit` enumerates the concrete state space on the
  Z3-free path (`fsl-runtime` BFS with `BTreeSet` dedup and parent-link
  traces). Closure — no new states within `--depth` — returns `proved`
  (`closure: true`), a complete unbounded proof that needs no lemmas even for
  true-but-not-inductive invariants where k-induction reports `unknown_cti`;
  depth exhaustion returns bounded `verified`; exceeding `--explicit-budget`
  (default 1,000,000 visited states) returns the new `unknown_budget` verdict
  (exit 1), never a silent `verified`. Violations reuse the BMC
  shortest-counterexample trace schema, and results carry `states_explored`,
  `max_frontier_width`, and `depth_reached`. Fail-closed rejections keep the
  engine sound: `leadsTo` properties, nondeterministic `init`, and
  `init forall` binder domains referencing state variables (both reference
  engines require compile-time-constant domains). Verdict-cache keys include
  the engine and budget. Includes corpus verdict-agreement and trace-replay
  integration tests, `docs/DESIGN-explicit-engine.md`, shared FSL skill
  guidance, and `tools/bench_explicit.py`; the frozen Python reference
  implementation is intentionally unchanged.
- Versioned normalized public Kernel contract (issue #208): native Rust
  `fslc kernel` exports checked/lowered models as typed, source-traceable JSON
  without the Python AST, while `fslc conformance` emits deterministic
  language-neutral success/disabled/rollback vectors. Add v1 JSON Schemas,
  requirement/lowering origins, explicit partial-operation conditions, atomic
  Monitor rollback for all failed steps, a Monitor↔symbolic transition agreement
  gate, golden fixtures, compatibility policy, release bundle, and external
  compiler guidance. The frozen Python reference remains unchanged.
- Digest-bound approval records (issue #190): add native Rust `fslc approval
  create|check|diff` for reviewed ledger, HTML, and scenario artifacts. Versioned
  sidecars bind a location-insensitive lowered-kernel digest, normalized rendered
  artifact digest, generation inputs, requirement IDs, approver, timestamp, and
  reconstructable Git baseline. `fslc ledger --approval` now shows per-requirement
  `approved` / `drifted` state with the complete baseline digest and a direct
  semantic-diff command. Includes JSON Schema, snapshot/integration coverage, and
  `docs/DESIGN-approval.md`; the frozen Python reference remains unchanged.
- Intentional undecided declarations (issue #189): reserve the
  `"undecided: reason"` metadata tag, including tagged `init`, and add
  dependency-derived affected requirement IDs to new ledger and HTML undecided
  sections. Bounded underspecification findings remain visible and now distinguish
  exact declaration matches with additive `acknowledged` / `acknowledged_by`
  fields. Includes native Rust integration/snapshot coverage and
  `docs/DESIGN-undecided.md`, shared FSL skill guidance, and bilingual manual-site
  coverage for the syntax and acknowledged-review workflow. The frozen Python
  reference implementation is intentionally unchanged.
- Complete the native Rust migration for issue #195: generic `check` and bounded
  `verify` now agree with the Python reference across all 181 FSL corpus files,
  database documents lower to executable compatibility kernels, collection
  binders and `unique` preserve language semantics, multi-fragment initialization
  is retained, and AI project/agent routing plus parse-error classification match
  the public CLI contract. Port full domain scaffolding and requirement-oriented
  ledger rendering (including implementation logs and external evidence) without
  a Python command fallback. The installer now places the checksummed native Rust
  release on the public `fslc` path; Python remains an optional LSP and reference
  implementation.
- Rust port Phase 0 (issue #195): add a deterministic Python AST JSON exporter
  that serves as the reference oracle for cross-language parser differential
  tests. It covers every `.fsl` file under `specs/` and `examples/`, records
  evidence-only frontends explicitly, preserves source-relative resolution, and
  never silently drops parse/lowering failures. Add the initial typed Rust
  expression parser, typed kernel `spec` and refinement surface parsers with
  178/178 parseable surface-corpus AST parity, and `fsl-core` spec/compose
  lowering with 81/81 kernel AST parity. Add a typed kernel model, independent
  concrete Monitor/BFS, backend-neutral async-check solver interface, pinned
  native Z3 4.16.0 backend, and incremental symbolic BMC. Python BFS, Rust BFS,
  and Rust BMC agree on the decision surface for all 20 monitorable `specs/`
  programs at depth 3. Native `check`/`verify` match the existing stable snapshot
  projection for all 23 `specs/` files at depth 5; native scenario identities
  match 20/20 and all 86 generated scenarios cross-replay. BMC witnesses are
  replayed in both implementation directions. Full `check`/`verify` envelopes
  match 43/43 command cases with a reviewed witness-only allowlist, and bounded
  `leadsTo` checking covers deadlock stutter, deadlines, and fair lassos with
  3/3 focused liveness-envelope and cross-replay parity. Phase 2 starts with
  native k-induction (11/11 focused base/CTI/ranking envelopes) and bounded
  refinement including progress preservation (6/6 focused envelopes). Add exact Python↔Rust differential runners, plus a
  pinned `z3-solver` 4.16.0 disposable-Worker round-trip/throughput spike and CI
  gate. Add native Phase-2 sweep/refinement-chain/project-chain commands, typed
  DB/AI/domain tooling and Phase-3 report command entry points with a 107/107
  differential gate covering exact typestate and full built-in/external mutation
  adjudication (including requirement and refinement attribution), full
  explain/counterfactual output, byte-identical pytest and five alternate
  testgen targets (including forbidden rejection cases), core analysis graph
  JSON plus DOT/Mermaid exports (including standalone refinement mappings),
  declaration-level tag-review export, mixed spec/refinement batch analysis,
  project traceability and missing-anchor findings, structural AI-review
  findings (tag drift, unanchored properties, unread/unwritten state, and
  unguarded actions, progress-cycle detection/suppression, bounded unconstrained
  effects, conservation candidates, and acceptance-backed divergent choices),
  focused byte-identical HTML/ledger artifacts, raw CLI output parity for
  version, typestate, testgen, explain, domain expansion/test generation,
  HTML, and ledger output, and broad HTML tag/attribute
  structure parity across workflow, inventory, vending, and forbidden specs. Add
  the production Rust WASM Worker, official npm Z3
  bridge, COOP/COEP playground assets, and a headless-browser gate that proves
  forced cancellation recovery and native/WASM verdict parity. Fix stale
  successor-expression caching that produced false-positive divergent-choice
  findings when CPython reused short-lived state-object ids. Broad HTML parity
  compares byte-identical static content plus complete tag/attribute structure;
  solver-selected dynamic witnesses remain covered by bidirectional replay. See
  `docs/RUST-PORTING.md` for the repeatable rewrite method and current evidence.
  Complete the native CLI option contract, persistent verification cache,
  snapshot and implementation-log replay, auxiliary-lemma proofs, semantic
  diff (including Git tree inputs), expression fuzz agreement, and native
  runtime/tool/CLI regressions. Release artifacts now include Intel macOS.
- Rust port architecture (issue #195): an accepted, phased design for a
  repository-local Cargo workspace that targets a native single-binary CLI and a
  browser Web Worker/WASM kernel backed by the official `z3-solver` npm package.
  The design makes the Monitor/oracle solver-independence a crate-graph invariant,
  freezes the JSON/exit-code contract, and defines parser, envelope, trace-replay,
  and corpus-parity gates for every migration phase. This is a design milestone,
  not a claim that the Rust implementation exists. See
  `docs/DESIGN-rust-port.md`.
- Declaration tag drift review (issue #188): `analyze --profile ai-review`
  gains deterministic `tag_stale_reference` and `tag_formula_disjoint`
  findings based only on exact code-shaped identifiers. `analyze --export
  tag-review` emits tagged actions/properties with rendered formal definitions
  under the new `tag-review.v0` schema for declaration-local external review;
  it never calls a model or promotes wording judgments to violations. See
  `docs/DESIGN-tag-drift.md`.
- Named predicate frontend sugar (issue #187): file-local `def name(p: Type) =
  expr` declarations can be used in kernel, requirements, and compose
  expressions. Calls are inlined before model construction, keeping
  model/BMC/runtime unchanged and equivalent to hand expansion. Unknown calls,
  arity mismatch, direct/mutual recursion, shadowing, and capture-changing
  substitution are explicit diagnostics. Includes a tagged flagship example;
  see `docs/DESIGN-def.md`.
- Git-aware semantic diff (issue #186): `fslc diff --git BASE..HEAD [SPEC]`
  materializes both complete tracked trees before delegating to the
  VCS-independent #176 comparison, so imports resolve from their own revision.
  Omitting `SPEC` compares every changed `.fsl` file. JSON records both commit
  hashes and `git_archive_full_tree`; batch gates aggregate child `--forbid`
  violations. See `docs/DESIGN-diff-git.md`.
- Bounded underspecification findings (issue #179): `fslc analyze --profile
  ai-review` now uses a fixed depth-4 BMC probe to emit `divergent_choice` when
  two distinct actions are enabled in the same reachable state and split an
  invariant/acceptance outcome, and `unconstrained_effect` when an unread state
  can receive different next values from two enabled actions. Findings include
  `evidence_basis:"bounded_bmc"`, reachable branch/successor evidence, and a
  question-form `spec_question`; they remain review-only
  `formal_status:"not_a_violation"`. Strong semantic findings suppress duplicate
  `unread_state`/`unguarded_action` approximations, while structural findings
  remain when no bounded witness exists. The v0 schema gains additive optional
  question/evidence fields. See `docs/DESIGN-underspecification.md`.
- External mutation adjudication (issue #178): `fslc mutate SPEC --from
  mutants.jsonl` appends externally generated full-spec or exact-replacement
  mutations to the built-in catalog and judges valid records through the same
  BMC/acceptance/forbidden/refinement oracle. Malformed JSON/instructions,
  duplicate ids, parse/name/type/construction errors, and spec-name changes are
  first-class `invalid` generation-quality findings rather than false kills.
  Mutants now carry `source:"builtin"|"external"`; combined and per-source
  summaries include `invalid` and kill rates over killed+survived only.
  `--max-mutants` caps the built-in catalog, enabling external-only runs with
  `--max-mutants 0`. See `docs/DESIGN-mutate.md`.
- Verified auxiliary lemma candidates for k-induction (issue #177): repeatable
  `fslc verify --engine induction --lemma "EXPR"` independently proves each
  candidate using the original transition system and implicit bounds, without
  assuming the original user invariants. False/non-inductive/invalid candidates
  are rejected with their own counterexample, CTI, or parse/type result; only
  `proved` candidates can strengthen the target proof. The retry loop evaluates
  candidates in each target CTI model, records which lemma excluded which CTI
  and steps under `lemma_cti_exclusions`, and on final `proved` emits source
  declarations under `auxiliary_invariant_recommendation` without rewriting the
  file. Lemma text/order is included in the persistent verify cache key, and
  BMC use is rejected as a usage error. See
  `docs/DESIGN-induction-lemmas.md`.
- Semantic specification diff (issue #176): `fslc diff OLD.fsl NEW.fsl
  --depth K` classifies bounded changes through bidirectional auto-refinement
  (`behavior_added` / `behavior_removed`), SMT implication of user-invariant
  conjunctions (`invariant_weakened` / `invariant_strengthened`), and replay of
  OLD negative scenarios against NEW (`forbidden_relaxed`). Directional
  differences include counterexample witnesses; incompatible names are
  explicit `unknown` unless a one-direction `--mapping` is supplied. Changes
  to `verify` entity/number bounds are first-class `scope_changed` findings and
  comparisons record/use NEW's scope. Findings are analysis (exit 0); only an
  explicit `--forbid kind,...` gate exits 1. See
  `docs/DESIGN-semantic-diff.md`.
- Predictive BMC from a state snapshot (issue #175): `fslc verify SPEC
  --from-state state.json` replaces the declared init with a complete
  Monitor/replay logical-state JSON and searches for violations from that point.
  Snapshot runs are BMC-only, bypass the verdict cache, preserve concrete
  identities by disabling symmetry reduction, and stamp
  `faithfulness.scope:"bounded_from_snapshot"` with `spec_init:"not_used"`.
  Full type/shape validation covers every kernel state type; partial snapshots
  are rejected rather than treated as unconstrained. See
  `docs/DESIGN-from-state.md`.
- Production-log replay mappings (issue #174): `fslc replay SPEC --from-log
  events.jsonl --mapping log_mapping.fsl` reuses the refinement parser and
  mapping expressions to translate external action/parameter names and observed
  post-action state into the target spec. The Z3-free Monitor stops at the first
  rejected action, mapping failure, or state mismatch and reports both the
  zero-based record index and one-based JSONL line. The initial contract requires
  complete observed state; missing fields are nonconformant rather than silently
  unconstrained. See `docs/DESIGN-log-replay.md`.
- Assurance classes (issue #171): a shared `fslc.assurance` classifier turns
  every command's result dict (BMC `verify`, k-induction `prove`, and the
  fsl-ai/fsl-db/fsl-domain `formal_result:"not_run"` producers — replay,
  observe, eval/regress/compare/drift, compat, agent/project analysis) into
  one of `proved(induction)` / `bounded(BMC depth k)` / `replay-observed` /
  `statistical(Wilson c%)` / `not_run`. `fslc ledger` gains a 保証クラス
  column (per requirement id, from the weakest of its tagged elements) plus a
  new `--engine bmc|induction` flag and a repeatable `--evidence
  <result.json>` flag to fold external evidence into the classification; a
  `## 外部エビデンス` section lists evidence not tied to a requirement id.
  `fslc html` gains an Assurance row/column and the same `--engine` flag.
  Presentation-only: no change to verification semantics, the JSON envelope,
  or exit codes. See `docs/DESIGN-assurance-classes.md`.
- Counterexample blame assignment (issue #170): a `violated` result with
  `violation_kind` `invariant`/`type_bound` now carries top-level
  `blame.conjuncts[]` (which AND-conjunct of the invariant is false, with
  violating bindings) and per-step `blame: {guards[], effects[]}` on the
  trace (a backward slice naming the `requires` clauses and state-writing
  statements that fed the blamed conjunct(s), verified to exclude untouched
  sibling variables, not just list every write). `fslc explain`'s
  counterfactuals inherit both automatically with zero explain-side logic.
  `vacuous_implication`/`vacuous_leadsto` findings gain `classification`
  (`insufficient_depth`/`over_constrained`) and `blocking` (the invariants
  actually making the antecedent/trigger impossible) — reusing the existing
  reachable-diagnosis unsat-core machinery, with a fix for a self-reference
  bug found while wiring it up (a vacuous_implication's own invariant was
  always appearing in its own blocking core). All additive; no change to
  existing fields or exit codes. Moved the AST-to-text renderer out of
  `explain.py` into a new leaf module `src/fslc/render.py` so `bmc.py` can
  render blame text without an import cycle. See
  `docs/DESIGN-blame-assignment.md`.
- Persistent verdict cache for `fslc verify` (issue #169, `src/fslc/verify_cache.py`):
  a sha256 key over the post-desugaring kernel AST, raw entry-file text, every
  verify option, and an implementation fingerprint (fslc/z3/lark/Python
  versions + a hash of the installed package's source) makes an unchanged
  re-run in the write→verify→repair loop return instantly instead of
  re-solving. A `violated` result is additionally reused at any deeper
  requested depth (a counterexample's earliest step does not depend on the
  search bound). Hits add one additive JSON field
  (`"cache":{"hit","key","source"}`); misses are byte-identical to today's
  output, and any cache-layer failure degrades to an ordinary uncached run —
  never affects the verdict. New `--no-cache` flag / `FSLC_CACHE=off` env var
  to opt out; `FSLC_CACHE_VERIFY=1` re-runs the engine on every hit and
  reports a divergence as an internal error. Stage 1 of the design
  (whole-verdict cache + cross-depth reuse) only — property-level
  differential re-verification is explicitly deferred. See
  `docs/DESIGN-incremental-verify.md`.
- Dialect corpus conformance harness (issue #167): a declarative
  `tests/dialect_registry.py` (dialect construct → example corpus, plus
  documented exclusions for fsl-ai project/agent files and one Monitor
  edge-case fixture) backs a new `tests/test_dialect_conformance.py` CI gate
  that runs `parse -> desugar -> build_spec -> Monitor load -> BMC/Monitor
  expression agreement -> verify-vs-oracle verdict agreement` over every
  `.fsl` under `specs/`/`examples/`, closing the gap the 2026-07-08 fsl-db
  audit found (an entire dialect corpus silently sat outside the dual-
  evaluator safety net while `pytest -q` stayed green). No `pytest.skip`
  anywhere — every non-conformance file is a classified, asserted case.
  Along the way, hardened the shared expression-agreement check (now in
  `tests/agreement.py`, reused by `tests/test_evaluator_agreement.py`): array
  state is pinned as a fully-determined term instead of per-key equalities,
  and agreement is proved by solver unsat-check rather than `Model.eval`,
  fixing spurious disagreements on `Set<domain>` bound checks (a real z3
  quantifier-evaluation gap the broadened corpus scan surfaced, not a
  bmc.py/runtime.py semantics bug — see `docs/DESIGN-conformance-harness.md`).
- Coupled-change metatests (issue #168): `tests/test_coupled_change_meta.py`
  mechanizes the "grammar/dialect/CLI command moves its LSP index entry and
  DESIGN doc together" discipline that was previously only a human checklist.
  A corpus-wide, two-stage check (structural scan + position cross-check)
  verifies every grammar production's NAME/REQ_ID tokens reach
  `src/fslc/lsp/index.py`'s symbols/references, with a reviewed, staleness-
  checked allowlist for genuine free-form labels; a second check verifies
  every kernel dialect and CLI command maps to an existing
  `docs/DESIGN-*.md`, and that `docs/README.md`'s DESIGN-doc map is
  bidirectional. Prototyping this metatest re-found the same class of bug
  `d1770c4` fixed, twice more, fixed in this PR: fsl-domain sources crashed
  the LSP entirely (`_parser_for_source` never dispatched to the domain
  grammar), fsl-ai project files (multi-block bundles that happen to start
  with `ai_component`) crashed it too (now indexed via
  `ai_project._top_blocks` directly, matching how that dialect is actually
  parsed), and several fsl-db/fsl-ai productions
  (`env_flag`/`database_def`/`delegation_edge`/`agent_event`/
  `agent_output_def`) were silently unindexed -- including a pre-existing
  bug in `_visit_env_artifact` that dropped every `when flag F=V` condition
  on a database artifact. See `docs/DESIGN-coupled-change-metatest.md`.
- Rebuilt the `docs/intro/` manual site's information architecture around 4
  fixed categories (Get Started / Guides / Reference / Examples & Background),
  designed with the Relational Design plugin (decisions in
  `docs/DESIGN-docs-site.md`). Added a categorized sidebar with
  real `<h2>`/`aria-labelledby` groups, a breadcrumb, 4 category hub pages, and
  7 new chapters: `quickstart` (a 5-minute install-to-proof walkthrough),
  `examples` (a guide to `examples/`/`specs/`), `errors` (the JSON envelope,
  exit-code contract, and a failure-mode index), `glossary`, and `design-notes`
  (an index of authoritative `DESIGN-*.md` documents). Added `tools/build_site_reference.py`,
  which generates `intro/language.*.html` and `intro/cli.*.html` — exhaustive,
  disclosure-tree references reproduced verbatim from `docs/LANGUAGE.md` and
  introspected from `src/fslc/cli.py` — plus `tests/test_site_reference_snapshot.py`
  to catch a forgotten regeneration. `syntax.*.html` is now a 30-minute reading
  primer that points to the generated language reference rather than a second
  hand-maintained grammar table. Fixed `docs/LANGUAGE.md` §7 missing the
  `fslc ledger` command and `docs/README.md` missing 5 `DESIGN-*.md` entries.
- fsl-ai stochastic/migration/drift evidence commands: `fslc ai eval` now checks
  precomputed eval JSONL against `statistical_property` Wilson-bound thresholds,
  `fslc ai regress` checks aggregate `ai_migration.no_regression` metric
  drop/increase clauses, `fslc ai compare` reports metric deltas,
  `fslc ai drift` checks runtime telemetry thresholds/drift, and
  `fslc ai compat` emits shared DB artifact capability profiles. All return
  external evidence with `formal_result:"not_run"` rather than kernel proof.
- fsl-domain / fsl-effect v0 for Functional DDD and async lifecycle modeling:
  `domain` files now parse aggregate state, command/event/error, pure
  `decide`/`evolve`, projections, saga/process-manager steps, async effects
  with correlation/idempotency/retry/timeout/compensation/outbox boundaries, and
  lower the checkable slice to the existing kernel. Added
  `fslc domain check|analyze|expand|generate|testgen|replay`, TypeScript
  Functional DDD scaffolds with process-manager output, simple
  Python/Kotlin/Swift/Rust scaffolds, domain adapter/conformance scaffolds,
  stable `fsl-domain-finding.v0` findings, examples under `examples/domain/`,
  and docs/skills/manual updates. Runtime replay is observation evidence; the
  v0 boundary keeps external API/queue/wall-clock and production exactly-once
  guarantees outside formal proof.
- Documented the agent-side natural-language review boundary for
  `fslc analyze`: `fslc` remains deterministic and language-agnostic, while AI
  agents may add non-authoritative review suggestions only when they cite source
  text and graph nodes, keep `formal_status:"not_a_violation"`, and respect
  explicit privacy opt-in for external model calls. (#117)
- fsl-ai recursive `agent` composition: `fslc ai check` now parses nested
  agents as ordinary scoped agents, emits deterministic AI-readable
  `agent_ir`/graph summaries, validates explicit authority/context grant
  boundaries, and reports structural findings for unsafe delegation,
  visibility, review-gate, and tool-reachability patterns. Existing
  `ai_component` hard-contract behavior remains backward-compatible. (#157)
- fsl-stochastic external evidence layer and AI compatibility integration:
  `docs/DESIGN-stochastic.md` fixes statistical evidence semantics as an
  external layer over precomputed eval JSONL, Bernoulli/proportion metrics, and
  Wilson intervals only (`formal_result:"not_run"`; no `fslc verify` semantics
  change). `dbsystem artifact` now supports generic `requires` / `provides`
  capability profiles so AI model/prompt/retriever/tool-schema/output-schema
  compatibility is checked in the same environment/schema/flag snapshots as
  DB/API/mobile/server artifacts, with `required_capability_missing` findings
  and `DB-ASSUME-AI-CAPABILITY-PROFILES`. (#139, #140)
- (Documentation) Team-facing Japanese practical guide for `fslc analyze`
  (`docs/GUIDE-analyze.ja.md`): a plain-language walkthrough of the structure
  map, graph projections, `--profile ai-review` findings (with real command
  outputs), DOT/Mermaid exports, batch mode, project traceability, and the
  "not a verifier" boundary, plus review/PR/CI workflow integration. Linked from
  the doc map in `docs/README.md`. Includes a prior-art positioning section
  (Petri net structural analysis, SPARK/GNATprove flow analysis, Frama-C
  PDG/slicing, Event-B/Rodin, proof-assistant dependency graphs, mCRL2 static
  LPS analysis) stating plainly which parts of `analyze` are borrowed ideas and
  which parts of the combination are uncommon.
- fsl-ai hard-contract dialect (`ai_component`) for AI tool-boundary
  safety. The dialect parses declared tools, authority, human approval,
  forbidden tools, symbolic business preconditions, and fallback metadata,
  lowers the deterministic hard-contract slice to the existing kernel, and adds
  `fslc ai check` / `fslc ai replay` with stable AI-readable findings
  (`fsl-ai-finding.v0`). Runtime replay is explicitly observation evidence
  (`formal_result:"not_run"`), while evaluator-backed and statistical AI claims
  remain out of the kernel. Includes fixtures in `examples/ai/`, schemas under
  `schemas/fslc/ai/`, docs in `docs/DESIGN-ai-hard.md`, bilingual site pages,
  and updated language/skill references. (#135, #136, #137, #138)
- fsl-db MVP (`dbsystem`) for database migration compatibility across schema
  versions, artifacts, and environments. The dialect parses typed DB IR, expands
  to the existing kernel via `Map<Column, Bool>` lifecycle state and generated
  migration actions/invariants, and adds `fslc db check` for stable
  AI-readable findings (`fsl-db-finding.v0`) such as
  `column_removed_while_still_read`, `column_removed_while_still_written`, and
  `not_null_before_backfill`. Includes golden fixtures in `examples/db/`,
  docs in `docs/DESIGN-db.md`, and updated language/skill references. (#122,
  #123, #124, #125, #126, #127, #128)
- fsl-db post-MVP compatibility extensions: destructive/irreversible migration
  annotations, bounded data preservation and rollback-equivalence findings,
  rename/split/merge transforms, API response and offline payload compatibility,
  runtime observation evidence via `fslc db observe`, and a minimal SQL DDL
  importer via `fslc db import`. The new checks stay under explicit assumptions
  for finite rollout windows, offline TTL ticks, bounded row models, and
  observability coverage. (#129, #130, #131, #132, #133, #134)
- fsl-db DB/multi-environment follow-ups: finite feature-flag variants in
  `environment` blocks with `when flag name=value` artifact windows and
  `DB-ASSUME-FINITE-FLAG-STATE`, a minimal Prisma schema importer
  (`prisma-schema-minimal.v0`) alongside SQL DDL import, and external
  preservation/engine evidence JSON schemas plus fixtures that keep sampled,
  audited, and dry-run evidence separate from formal proof
  (`formal_result:"not_run"`). (#144, #145, #146, #147)
- Issue #104 follow-ups: typed `relation A -> B` state with relation helpers
  (`.contains/.add/.remove`, `reachable`, `acyclic`, `functional`,
  `injective`, `domain`, `range`); `helpful action(args)` metadata for
  per-binding ranked `leadsTo` induction under interleaving; opt-in
  `fslc sweep` with `sweep.results` and `sweep.minimal_counterexample`; richer
  `preserve progress` diagnostics (`progress_failure` classification and
  lower-layer `fair action` repair hints); and `fslc html` relation/refinement
  visual evidence.
- `fslc analyze` structural observation JSON. `--projection tsg` emits a stable
  Typed Semantic Graph over requirements, actions, state variables, properties,
  acceptance/forbidden scenarios, and traceability metadata. Graph projections
  (`action_state_graph`, `requirement_property_graph`, `property_state_graph`)
  add deterministic components, SCCs, representative cycles, and degree
  summaries. `--profile ai-review` emits AI-readable review findings
  (`disconnected_requirement`, `unanchored_property`, `progressless_cycle`) with
  `formal_status:"not_a_violation"` so structural observations are not confused
  with proof failures. See `docs/DESIGN-analysis.md`.
- `fslc analyze` now supports batch file/directory analysis, DOT/Mermaid graph
  exports, standalone `refinement_graph`, project-manifest
  `traceability_graph`, versioned JSON Schema files under
  `schemas/fslc/analysis/`, two additional AI-review findings
  (`unwritten_state`, `unguarded_action`), and opt-in LSP informational
  diagnostics for analysis findings via `FSLC_LSP_ANALYSIS_DIAGNOSTICS=1`.
- `fslc analyze` structural follow-ups for AI-assisted review: graph projections
  now include additive `metrics` (cycle rank, fan-in/fan-out hubs, and graph
  counts), `action_dependency_graph` exposes structural `enables` and
  write/write `conflicts_with` action relations, and `impact_graph --focus
  NODE` emits an upstream/downstream slice around a TSG node. The `ai-review`
  profile adds `unread_state` using a transitive relevance closure over effect
  reads, and `conservation_candidate` proposes weighted-sum invariants from
  restricted counter-like `Int` effects without treating them as proof evidence.
  (#149, #150, #151, #152, #153)
- Optional **spec-level tag** classifying a whole spec: an intent string right
  after the spec name, `spec ReturnUI "ui: screen flow" { … }`. Metadata only —
  it desugars to nothing and is never verified (corpus snapshot unchanged) — and
  is surfaced by `fslc explain` (`skeleton.spec_kind = {id, text}` + a `Kind:` line
  in `--readable`) and by `fslc html` (a neutral badge next to the spec title).
  It carries the machine-readable and at-a-glance "this is a UI spec" classification
  the fsl-ui spike (issue #9) identified, without the `expand_ui` dialect; see
  `docs/DESIGN-ui.md`. Touches `grammar.py`, `model.py`, `explain.py`,
  `html_report.py`. (The surfaced field is `spec_kind`, not `kind`, because the
  enveloped result tree reserves `kind` for the diagnostic discriminator.)

### Fixed
- Rust BMC witness replay now starts from the solver-projected step-zero state,
  so partially initialized aggregate components remain legitimately free and
  invariant counterexamples return `violated` instead of an internal Monitor
  init mismatch. Explicit `--from-state` snapshots remain authoritative, and
  top-level `kind: "internal"` envelopes now consistently exit 3 per the CLI
  contract. The frozen Python reference is unchanged. (#219)
- **CI**: `tests/test_site_reference_snapshot.py` (added with the doc-site
  rebuild) was missing from every `.github/ci-shards/shard-*.txt`, so
  `pr-shard-coverage` was failing on any PR touching the test suite. Added it
  to `shard-2.txt`.
- **Soundness**: `fslc verify --engine induction` could report a `leadsTo
  ... decreases ... helpful` property `"proved"` when it was genuinely false.
  With two or more distinct `helpful` action declarations,
  `_prove_leadsto_rank_no_deadlock` only checked that *some* helpful match is
  enabled at every pending state (a disjunction); if which instance is
  enabled alternates (e.g. two helpful actions gated on opposite parity of
  some other variable), no single instance is ever *continuously* enabled,
  so its `fair` declaration never actually obligates it to run under weak
  fairness, and an unrelated action can stall the obligation forever. Added
  `_prove_leadsto_rank_helpful_sticky` (`src/fslc/bmc.py`), a new induction
  proof obligation requiring each helpful instance to stay enabled (or `Q`
  resolve) until it fires, reported as `rank_failure:
  "helpful_action_enabledness_not_sticky"` when it can't be shown. Wired into
  both the explicit-`decreases` path and the auto-synthesized-measure path
  in `_prove_ranked_leadstos` (the latter was missed in an earlier pass of
  this fix and reached the same false "proved, synthesized: true" outcome
  through a separate candidate loop). The common single-helpful-action idiom
  is unaffected (`tests/test_helpful_leadsto.py`, `docs/LANGUAGE.md`,
  `skills/fsl/reference.md`).
- **Soundness**: a second, independent false-`"proved"` gap in the same
  `helpful`/`decreases` ranking rule, reproducible even with a single
  `helpful` action — `_prove_leadsto_rank_progress` only required a
  non-helpful action to keep the leadsTo obligation pending
  (`Or(q_next, p_next)`), not to avoid *increasing* the measure. A fair,
  always-enabled helpful action decreasing the measure by a bounded amount
  each time it fires does not guarantee eventual convergence if an unrelated
  action can increase the measure by more than that in between — the
  helpful action still fires under weak fairness, but the measure can
  diverge instead of reaching zero. `pending_preserved` now additionally
  requires `measure_next <= measure` for the non-helpful branch, reported as
  `rank_failure: "non_helpful_action_increases_measure"` when it can't be
  shown (`src/fslc/bmc.py`, `tests/test_helpful_leadsto.py`,
  `docs/LANGUAGE.md`, `skills/fsl/reference.md`).
- `runtime.py`'s concrete `Monitor` rejected every fsl-db-generated spec
  (15/18 `examples/db/*.fsl`) with a spurious `state variable 'column_exists'
  assigned more than once in init`, because the duplicate-init-assignment
  check keyed on the base Map variable alone; `db_expand.py` legitimately
  writes one flat `column_exists[Col] = ...` statement per column, each to a
  different key of the same map. This silently disabled the dual-evaluator
  agreement safety net (oracle/agreement/trace-soundness tests, `replay`,
  `testgen`) for the whole dbsystem dialect, since BMC accepted these specs
  fine. `_check_deterministic_init` now disambiguates a Map/index target by
  `(base, key)` when the key is a concrete (non-forall-bound) value, so
  distinct keys of the same map no longer collide; a genuine same-key
  duplicate is still rejected, and a `forall`-keyed init is unaffected.
- The FSL language server's raw-tree index (`src/fslc/lsp/index.py`) no
  longer hard-codes the kernel-only grammar: `build_index` now picks
  `ai_parser.AI_PARSER`/`db_parser.DB_PARSER` for `ai_component`/`dbsystem`
  sources (same dialect-sniffing already used by `parser.parse_src`), so
  go-to-definition/references/hover/semantic-tokens no longer go dark with an
  uncaught `UnexpectedCharacters` for those files (`fslc check` diagnostics
  were unaffected, since they go through a different path). Added indexing
  for `tool`/`table`/`column`/`artifact`/`migration`/`environment`
  declarations and their references (`authority` tool lists, `col_ref`,
  `env_artifact`). Also fixed two kernel-grammar indexing gaps found in the
  same audit: a `leadsTo ... helpful NAME(...)` action name was a bare Token
  the generic child walk skipped, so it never resolved or showed up in
  find-references; and `deadline NAME <= expr` registered `NAME` as a new
  `property` symbol instead of a reference to the already-declared
  `age NAME[...]` variable it targets. Added `helpful`/`relation`/`acyclic`/
  `functional`/`injective`/`domain`/`range` to the completion keyword list.
- `ai_component` rejects two `fallback` entries that share the same `reason`
  (they would silently collide into one generated `fallback_<reason>`
  action). `docs/DESIGN-ai-hard.md` now also documents explicitly that
  `fallback` is structural-only in the hard-contract layer — no invariant is generated over
  `fallback_required`, since `target` has no corresponding kernel action to
  check it against yet (`src/fslc/ai_expand.py` `validate_ai_component`).
- `fslc refine` no longer silently merges a same-named impl/abs enum (or
  struct) that has a different member list (or field set): it now rejects
  the pair as a `kind: "type"` static error instead of letting an impl-only
  member get reinterpreted as whichever abs member sits at the same ordinal
  index, which could previously turn a real refinement violation into a
  false `"refines"`. Domain types (`lo..hi`) still may share a name with
  different bounds (`src/fslc/refine.py` `_merge_types_meta`/
  `_type_defs_conflict`; `docs/DESIGN-refinement.md`, `docs/LANGUAGE.md`,
  `skills/fsl/reference.md`).
- The `acyclic`/`reachable` relation helpers no longer unroll an unmemoized
  O(n^n) Z3 expression tree, which made verification of a self-relation with
  as few as 7 domain values effectively hang; `_relation_reachable_expr`
  (`src/fslc/bmc.py`) now memoizes the bounded-path recursion into an
  O(n^2)-node shared DAG.
- `fslc db observe` and `fslc db import` no longer return exit code 3
  (reserved for internal errors) for their normal result values
  (`observed_conformant`/`observed_mismatch`/`imported`/
  `imported_with_warnings`); `exit_code()` in `src/fslc/cli.py` now maps them
  to the documented 0/1 contract.
- Restored `docs/index.html` to a language selector only and added bilingual
  `docs/intro/db.{ja,en}.html` content pages for the fsl-db DB /
  multi-environment compatibility manual entry.
- `fslc html`/`fslc explain` now render a declaration's full logic instead of a
  truncated first source line: action `requires`/`ensures` and property bodies
  (`invariant`/`reachable`/`trans`/`leadsTo`) are rendered from the AST via the
  existing `_expr_to_text` pretty-printer, so multi-line predicates (e.g. a
  `reachable` block with a `forall` body) show completely, and a
  component-origin/composed action gets a real guard string instead of the old
  "source unavailable" sentinel (`src/fslc/explain.py`). Dialect-generated
  declarations (a `deadline`-derived invariant, the time-block-generated
  `tick` action) are now tagged `"generated"` in the explain skeleton
  (additive field) and routed out of the human Actions/Properties tables into
  the auto-checks table in the HTML report, with a neutral "generated by
  verifier" label replacing the old sentinel; the report also drops a
  `_Age*`-style generated domain type out of the entity/domain listing.
  `fslc html` further surfaces business-layer content that reached the
  skeleton but never the page: enum member lists in the State Model type
  column, an Entities & Domains panel, a KPI table, and a derived
  stage-transition ("process flow") panel reconstructed from enum-valued
  state plus guard/assignment shape — dialect-agnostic, so any hand-written
  enum state machine gets one too. `ID: sentence` requirement/policy meta
  moves from a trailing "Requirement" column (which showed literal "none"
  cells) to an inline caption under each row's own name, dropped entirely
  when absent; the same treatment now applies to a new optional "Actor"
  column. Fixed `_type_text` rendering `Bool`/`Int` state as a raw Python
  tuple. Sections are reordered model-first (State Model, Actions,
  Properties, *then* Verification Status), and the hero subtitle is now a
  one-line summary derived from the spec's own counts instead of a fixed
  string. Two more "none" filler spots dropped with the same principle: the
  property table's "Deadline" column now only appears when at least one
  property has a `leadsTo ... within`, and an action's Ensures cell is left
  empty rather than a "none" chip when the action has no `ensures` clause.
  Fixed a correctness bug in `_expr_to_text` (`src/fslc/explain.py`) that the
  above rewiring exposed: it never parenthesized a child operand, so e.g.
  `not (pending[r] and served[r])` rendered as `not pending[r] and
  served[r]` — which re-parses as `(not pending[r]) and served[r]`, the
  inverted formula (seen live in `sla_worker_design`'s
  `PendingServedExclusive`). `_expr_to_text` now tracks each operator's
  precedence tier (mirroring the grammar's `implies < or < and < not < is <
  cmp < sum < product < unary` chain) and parenthesizes a child exactly when
  its own precedence is looser than what its position requires — covering
  `not (A and B)`, `(A or B) and C`, and non-associative arithmetic like
  `a - (b - c)`/`a / (b / c)`, while still rendering `A and B or C` without
  spurious parens. This is the function `explain --readable` also uses, so
  the fix applies to both surfaces. Also suppressed the redundant
  `<actionname>: by <actor>` requirement caption on actions now that a
  business-dialect transition's actor has its own Actor column (real
  `REQ-n`/`POL-n` prose captions are unaffected).
- (LSP) `textDocument/references` with `includeDeclaration=true` now returns the
  declaration even when the cursor is on a *cross-file* reference (e.g. a
  `use`-imported `alias.member` in another spec). Previously the declaration was
  dropped in that case: `references_at()` only emits the declaration when it lives
  in the current document, and the workspace loop in `_workspace_references`
  (`src/fslc/lsp/server.py`) scans other files' references, never their
  declaration symbol, so a symbol declared in a different workspace file was
  omitted from the results. `go-to-definition` was unaffected.

### Changed
- `skills/` reframed around the doc-substitution philosophy: a `.fsl` spec
  replaces the prose spec/design doc, not just proves a property, so the
  self-check in `skills/fsl/SKILL.md` now scores both verification payoff and
  documentation payoff (a linear-path/CRUD feature that would be documented
  anyway is in scope), `--strict-tags` and `explain --readable` moved from
  high-risk-only to defaults for every contract (`skills/fsl-delivery/SKILL.md`,
  `skills/fsl-design-review/SKILL.md`), and `skills/README.md`,
  `skills/fsl/SKILL.md`, `skills/fsl-delivery/SKILL.md` now state the
  corpus-as-living-source-of-truth framing.
- `docs/intro/` (`concept`, `when-to-use`, `guide`, `index`, `design-layer`;
  `.en.html`/`.ja.html`) now state the same spec-as-documentation philosophy:
  `concept`, `index`, and `design-layer` say the `.fsl` source replaces the
  prose spec/design doc rather than supplementing it; `when-to-use` reframes
  from verification-ROI triage to a "write it wherever you'd document
  anyway" default, and narrows "out of scope" to genuine inexpressibility
  (continuous/probabilistic/free-text) rather than low payoff; `guide` keeps
  high-risk-first as adoption sequencing, not the end state; `concept`,
  `guide`, and `when-to-use` add the living-corpus/CI framing — the spec
  corpus stays continuously re-verified, not proved once.
- CI now treats Python as a frozen reference implementation and validates only
  the primary Rust workspace, dependency boundary, native Z3 matrix, official
  `z3-solver` Worker probe, and production WASM browser surface. Remove the
  Python pytest matrices, Python/Rust differential gate, and obsolete shard
  manifests; differential tools remain available for manual migration audits.

## [2.7.0] - 2026-07-04

### Added
- (LSP) `fslc-lsp` now serves four more `textDocument/*` features as thin
  lsprotocol adapters over the existing raw-tree index
  (`src/fslc/lsp/server.py`, `src/fslc/lsp/index.py` unchanged): `hover`,
  `references` (same-file plus cross-file, following `use`-imported aliases
  and workspace spec-name resolution), `completion` (triggered on `.` for
  alias-member completion, alongside local symbols and keywords), and
  `semanticTokens/full` (legend-registered token types/modifiers).

### Fixed
- (LSP) The raw-tree symbol/reference indexer (`src/fslc/lsp/index.py`) now
  covers three parse-tree shapes it previously fell through to generic
  child-recursion for: a `policy ... every <Case> reaching ... must have
  passed through ...` precedence policy's case name (`type` reference), a
  struct literal's field keys (`field` references), and every `field_suffix`
  beyond the first in a multi-level `a.b.c` postfix chain (`field`
  references) — the pre-existing `a.b` alias.member `value` reference is
  unchanged.

## [2.6.3] - 2026-07-03

### Fixed
- `fslc verify --instances`/`--values` overrides (#86) now propagate into an
  inline `implements` abstract spec, restricted to the entity/number names the
  abstract declares. Refinement is a same-world-size forward simulation, so a
  shrunken impl (`--instances Claim=1`) against a still-full-size abstract
  previously failed the refinement with `map_out_of_bounds` (surfaced only in
  the `implements` sub-field, with the overall `result` staying `verified`).
  Both sides now shrink together; an impl-only carried number (e.g. `Amount`,
  absent from a business abstract) applies to the impl only. (#94)

## [2.6.2] - 2026-07-03

### Fixed
- `docs/LANGUAGE.md` and `skills/fsl/reference.md` wrongly claimed there is
  no `sum()` aggregate usable in a `decreases` ranking measure, steering
  readers toward hand-written sums (`decreases level[0] + level[1]`) that
  only scale to domains small enough to enumerate by hand. The kernel's
  bounded `sum(x: T of expr [where expr])` aggregate has always been usable
  there (`decreases sum(k: Case of level[k])`), proves `unbounded`
  completeness under `--engine induction`, and is instances-count
  independent (composes with `--instances` overrides, #86). Corrected both
  docs and added regression coverage in `tests/test_sum_decreases.py`. (#91,
  see also the still-open per-entity fairness gap, #72)

## [2.6.1] - 2026-07-03

### Fixed
- `fslc verify --instances`/`--values` overrides (#86) no longer hard-error
  when an `acceptance`/`forbidden` scenario hardcodes an id/number from the
  spec's original (larger) world. When an override is active, a scenario
  whose replay fails purely because it references a value outside the
  overridden bounds (an out-of-range action argument or an out-of-range
  `expect` index) is downgraded per-scenario to a skip, reported via
  `warnings` (`kind: "acceptance_skipped"`/`"forbidden_skipped"`); other
  scenarios still replay normally. Without an override, or for any other
  failure reason, behavior is unchanged (hard error). This makes
  `--instances Case=1 --property <Liveness>` usable without editing
  acceptance/forbidden scenarios written for the full-size model. (#89)

## [2.6.0] - 2026-07-03

### Added
- `fslc verify --instances NAME=N` / `--values NAME=LO..HI` (both repeatable)
  override a `verify { ... }` block's `entity`/`number` bounds from the CLI,
  so liveness/induction runs can shrink the model without editing the spec.
  An undeclared `NAME` or a malformed value is a spec error (exit 2); the
  effective override is echoed back as `bounds_overrides` in the JSON
  envelope. (#86)

### Fixed
- Business-dialect no-bypass precedence policies (`every <Entity> reaching
  <Targets> must have passed through <Waypoints>`, #75) now auto-synthesize a
  second, stabilizing auxiliary invariant (`<PolicyId>_stability`) alongside
  the history flag: `forall c { stage[c] in dominated(Waypoints) =>
  visited[c] }`, where the dominated set is computed by one reachability
  pass over the process's transition graph with the waypoint nodes removed.
  Previously a compliant precedence policy verified under BMC but stalled at
  `unknown_cti` under `--engine induction` on a ghost CTI (`stage[c] ==
  Waypoint && visited[c] == false`, unreachable in practice but not provable
  by induction alone); it now proves at k=1, including for cyclic process
  graphs and waypoint disjunctions. Docs: `docs/DESIGN-precedence-policy.md`,
  `docs/LANGUAGE.md`, `skills/fsl/reference.md`. (#85)

## [2.5.0] - 2026-07-02

### Added
- Requirements-layer `process E with f: T { ... }` carried fields now accept
  `Bool` and enum types declared in the same spec, not just `number`. Number
  fields keep the original default (the domain's `lo` bound) and gained an
  optional explicit initializer (`f: T = <const-expr>`); `Bool`/enum fields
  have no invented default and **require** an explicit initializer
  (`f: Bool = true/false`, `f: T = Member`) — a missing one is a check-time
  error. Docs: `docs/LANGUAGE.md`, `docs/DESIGN-dialects.md`,
  `skills/fsl/reference.md`. (#70)
- The requirements-dialect inline `implements Abs from "file" { }` block now
  accepts `action <impl>(<params>) -> <abs>(<args>) | stutter` items (grammar.py
  `?implements_item` gains `refinement_action`), the same action-correspondence
  syntax as a separate refinement file, including an arity change between the
  impl and abs action's parameters. The inline desugar already merged the
  block's items into the same mapping AST a separate-file `refinement` parses,
  so `dialects.py`/`refine.py` needed no changes — including duplicate-map
  detection (`kind: "type"`, `"duplicate action map for '<name>'"`) when an
  inline `action ...` item and a requirement action's `maps` clause both target
  the same impl action. (#73; docs: `docs/DESIGN-refinement.md` §1.2,
  `docs/LANGUAGE.md`, `skills/fsl/reference.md`)
- k-induction `unknown_cti` results now suggest auxiliary invariants for the
  common monotone-counter idiom: when the CTI trace shows an `Int`/domain
  scalar or a `Map<K, Int>` counter moving in only one direction and starting
  on the unreachable side of the concrete initial value (a ghost/huge/negative
  start), the result gains `"suggested_invariants": ["<expr>", ...]` and the
  matching sentence is appended to `hint` (e.g. `"audit >= 0"` or, for a
  uniformly-initialized map, `"forall k: Case { audit[k] >= 0 }"`).
  Post-processing only (trace diff against `runtime.Monitor(spec).reset()`) —
  no solver/engine semantics change, so verdicts are unaffected; the field is
  additive and absent when no such counter is found. (`bmc.py`, `docs/LANGUAGE.md`,
  `docs/DESIGN-induction.md`, `skills/fsl/reference.md`, `tests/test_cti_suggestions.py`) (#74)
- Business-layer no-bypass precedence policy: `policy ID "text" every <Entity>
  reaching <Stage> [or <Stage> ...] must have passed through <Stage> [or
  <Stage> ...]`. Synthesizes an invisible `Map<Entity, Bool>` history flag
  (dedup'd across policies over the same process/waypoint-set), sets it on
  the transition(s) landing on a waypoint, and compiles to a kernel invariant
  carrying the policy's REQ-ID — closing "no-bypass" controls at the business
  layer without descending to `requirements`. See
  `docs/DESIGN-precedence-policy.md`. (#75)
- `terminal { }` now works in the `requirements` dialect (`terminal_def` is a
  `requirements_item`; it passes through unchanged to the kernel spec, same
  one-block-per-spec rule as the kernel). The `business` dialect gets no new
  syntax: `terminal { }` is instead derived automatically from each process's
  sink stages (stages with no outgoing `transition`) — if every process has
  at least one sink, the generated predicate is the conjunction, over
  processes, of "every entity of that process is at one of its sinks". A
  process with no sink at all (a genuine cycle) still generates no terminal,
  matching prior behavior exactly. Previously, neither dialect could declare
  intended stop states, so `--deadlock ignore` was the only way to silence
  the deadlock check at a completed stage — discarding detection of
  unintended deadlocks along with it. (#69)
- Allow the builtin `Bool` as an action parameter type (`p: Bool`), matching
  its existing use as a state `Map` value/key. `Bool` params are first-class
  z3/concrete booleans in expressions — usable bare as a guard
  (`requires b` / `requires not b`) or assigned into `Bool`-typed state
  (`flag[i] = b`) — not a 0/1 int carrier, keeping BMC and the concrete
  `Monitor` in agreement. `Int` stays rejected (unbounded, can't be
  enumerated); the error now hints at a range parameter
  (`p in <lo>..<hi>`). (#68)
- `acceptance`/`forbidden` action arguments now accept enum member names (and
  const names) in addition to numeric ordinals, matching the name resolution
  already used by `requires`/`invariant`/`expect` expressions
  (`_is_enum_member` in `values.py`). An undefined name is still a
  `kind: "acceptance"`/`"forbidden"` check-time error, now reported as
  "undefined const or enum member". (#67)

### Documentation
- Documented `leadsTo ... decreases` placement and ranking discipline in
  `docs/LANGUAGE.md` and `skills/fsl/reference.md`: `decreases` sits outside
  the `forall` wrapper (nesting it inside is a parse error, not a "ranking
  doesn't work under forall" limitation); a per-entity measure
  (`decreases level[c]`) always fails under interleaving
  (`rank_failure: "non_decreasing_action"`) because every enabled action must
  strictly decrease it; the working idiom is a global sum measure over a
  fixed small domain (`decreases level[0] + level[1]`), since there is no
  `sum()` aggregate to generalize it (fairness-aware per-entity ranking is
  tracked separately as #72). Also added a targeted parse-error hint in
  `cli.py` for `decreases` nested inside a `forall` body. (#71)
- Updated the workflow skills (`skills/fsl/SKILL.md`, `skills/fsl-business/SKILL.md`,
  `skills/fsl-requirements/SKILL.md`) to track the v2.5 feature set:
  auto-derived business `terminal` and the requirements-layer `terminal`
  passthrough (#69), `Bool`/enum carried fields with initializers (#70),
  `unknown_cti`'s `suggested_invariants` (#74), the no-bypass precedence
  `policy` form (#75), inline `implements { action ... }` correspondence with
  arity changes (#73), `Bool` action params (#68), and enum member names in
  `acceptance`/`forbidden` arguments (#67). `skills/fsl/reference.md` already
  covered these; the workflow skills had not. (#67 #68 #69 #70 #73 #74 #75)

## [2.4.0] - 2026-06-29

### Documentation
- `skills/fsl/SKILL.md` repair protocol: added the over-constraint mirror of the
  hollowing warning — after fixing a `forbidden`/`violated` by tightening a guard,
  re-run `verify` and confirm the action's `action_coverage` is still `true` (and
  affected `reachable`s still witnessed), since an over-tight guard surfaces as a
  *new* `reachable_failed`/`covered:false`. (Surfaced by the #22 repair validation run,
  where fslc-arm agents did exactly this self-check.)
- Documented the cross-layer discrete-time SLA rule: a `deadline` is a safety
  property of the clock that declares it, so a refinement carries it only across a
  *shared* clock (a finer-clock design fails `fslc refine` with
  `abs_requires_failed` — the same non-propagation as liveness). Added a worked
  shared-clock refinement (`examples/nfr/sla_worker_design.fsl` +
  `sla_worker_refines.fsl` → `refines`), corrected the `examples/nfr/README.md`
  framing (the kernel and requirements specs are *different machines*, not two
  encodings of one), and recorded the analysis and the deferred options in
  `docs/DESIGN-nfr.md` §6 / `docs/DESIGN-layers.md` §6 / `docs/LANGUAGE.md` /
  `skills/fsl/reference.md`. No kernel/grammar/refine change. (#56)
- Clarified in `docs/LANGUAGE.md` and `skills/fsl/reference.md`: the `implements { }`
  block takes only state `map` / `maps auto` / `preserve progress` (action↔action
  correspondence goes on the requirement-level action's `maps` clause, not inside
  `implements`), and the `time`-block `tick` is generated (declaring `action tick` is a
  check error), advances age only, auto-maps to `stutter`, and is referenced as
  `tick()`. (#58)
- Reframed the "When to Use FSL" manual chapter (`docs/intro/when-to-use.{ja,en}.html`)
  and the `skills/fsl/SKILL.md` self-check toward FSL's distinctive edge: cross-layer
  alignment is a **primary use**, not an optional "second lens." The page now leads
  with two payoffs — ① a single spec's way of breaking (the classic island check) and
  ② layer-spanning alignment (write the contract layers in FSL and the machine keeps
  catching cross-layer drift) — so it no longer reads as generic formal-methods
  adoption guidance. Brakes kept intact (abstraction tax, hollow-spec / mutate
  kill-rate, judgment-aid-not-mandate, "contract spine, not the entire product
  process").

### Added
- `fslc ledger <spec.fsl> [--impl-log run.json] [-o ledger.md]`: a business
  **audit ledger** (Markdown) that re-organizes `verify` / `scenarios` / `replay`
  findings **by requirement id** so a PM / governance / internal-audit reader can
  decide approve/reject/risk-accept per requirement from the ledger alone. A
  presentation layer over the verifier (no new evaluation): the `trace_type`
  discriminator (#23) drives a per-finding business translation, governance
  columns (risk/decider) come from `control` metadata when present (fill-in
  otherwise), the guarantee limit is stated in positive form, and raw JSON is
  demoted to a collapsed appendix. Docs: `docs/DESIGN-ledger.md`,
  `skills/fsl/reference.md` §7. (#24)
- `fslc` JSON results now carry a `trace_type` repair-routing discriminator on
  every counterexample/failure (`invariant` | `sla` | `type_bound` | `trans` |
  `ensures` | `partial_op` | `deadlock` | `leadsTo` | `leadsTo_rank` | `reachable`
  | `refinement` | `acceptance` | `forbidden` | `vacuity` | `conformance` |
  `induction_cti`), so an agent can route a fix by channel and tell an SLA
  deadline from a structural invariant. Derived in the CLI envelope from existing
  fields (no engine change); passing/spec-error results carry none. `requirement`
  is now also hoisted to the `refinement_failed` root, for parity with verify
  violations. The other repair inputs the issue proposed already exist (`trace`,
  `checked_to_depth`+`completeness`, `hint`/`recommended_action`,
  `unreached[].blocking_requires`) and are documented as the repair-field map in
  `skills/fsl/reference.md` §7. Backward compatible (additive). (#23)
- `init` blocks now support statement-level `if`/`else` in both the symbolic
  verifier and concrete runtime monitor. (#55)
- State declarations may now use inline anonymous range types such as
  `state { x: 0..3 }`, desugared to the existing domain type machinery. (#57)
- `fslc testgen --target phpunit`: a PHPUnit emitter (PHP 8.1+ / PHPUnit 10+,
  `declare(strict_types=1)`), the sixth harness on the pluggable emitter from #43.
  Same `reset`/`step`/`observe` `Adapter` contract and same baked-walk design. Leaves
  compare with `assertSame` (`===`), keeping `int`/`float`, `bool` and `null` from
  coercing (PHP's loose `==` would conflate `0 == "0"`); `_php_literal` renders `int`
  (`1`) distinct from `float` (`1.0`). `assertPartial` recurses by the expected keys
  (maps match order-independently — sidestepping PHP's numeric-string-key coercion —
  and list-shaped values also pin length). `setUp()` calls `markTestSkipped` until
  `makeAdapter()` is wired. Output defaults to `<SpecName>ConformanceTest.php` (PSR-4
  class = file name). Tests gate syntax with `php -l` (skips when php is absent).
  Docs: `docs/LANGUAGE.md` §12, `skills/fsl/reference.md` §9,
  `docs/DESIGN-bridge.md` §3.5. (#47)
- `fslc testgen --target dart`: a `package:test` emitter (also runs under
  `flutter test`), the fifth harness on the pluggable emitter from #43. Same
  `reset`/`step`/`observe` `Adapter` contract and same baked-walk design. Dynamic
  state is `Map<String, dynamic>`; Dart's `==` is reference-based on collections, so
  the bundled `assertPartial` recurses by the expected keys and compares leaves with
  the `equals` matcher (re-exported by `package:test`, keeping the generated file's
  only dependency `package:test`). `package:test`'s `skip:` is static, so a top-level
  probe runs once in `main()` and conditionally skips every `test()` until
  `makeAdapter()` is wired. Strings escape `$` (interpolation). Output defaults to
  `<spec_name>_conformance_test.dart` (snake_case). Docs: `docs/LANGUAGE.md` §12,
  `skills/fsl/reference.md` §9, `docs/DESIGN-bridge.md` §3.4. (#46)
- `fslc testgen --target kotlin`: a kotlin.test emitter (multiplatform; the JVM
  delegates to JUnit), the fourth harness on the pluggable emitter from #43. Same
  `reset`/`step`/`observe` `Adapter` contract and same baked-walk design. Dynamic
  state is `Map<String, Any?>`, where Kotlin's structural `==` is deep on `List`/`Map`
  and distinguishes `Int` from `Double`, so the partial-match helper is a plain
  recursion. The one framework-forced divergence: kotlin.test has no portable runtime
  skip, so `makeAdapter()` returns `null` until wired and each `@Test` returns early
  rather than reporting "skipped". Strings escape `$` (Kotlin string templates).
  Output defaults to `<SpecName>ConformanceTest.kt`. Docs: `docs/LANGUAGE.md` §12,
  `skills/fsl/reference.md` §9, `docs/DESIGN-bridge.md` §3.3. (#45)
- `fslc testgen --target swift`: a Swift Testing emitter (`import Testing` / `@Test` /
  `#expect`; not XCTest), the third harness on the pluggable emitter from #43. Same
  `reset`/`step`/`observe` `Adapter` contract and same baked-walk design as Vitest:
  deterministic and forbidden-rejection scenarios translate directly; the random walk
  is baked at generation time so the file needs no `fslc`/Python at runtime. Dynamic
  state is `[String: Any]` with a bundled `fslEqual`/`assertPartial` deep-equality
  helper (`Int`/`Double` kept distinct); an Option `None` bakes as the self-contained
  `FSLNull.instance` sentinel (no Foundation). Tests are disabled via
  `@Test(.enabled(if: isAdapterWired()))` until `makeAdapter()` is wired. Output
  defaults to `<SpecName>ConformanceTests.swift` (output naming is now a per-target
  registry). Docs: `docs/LANGUAGE.md` §12, `skills/fsl/reference.md` §9,
  `docs/DESIGN-bridge.md` §3.2. (#44)
- `fslc testgen --target {pytest,vitest}` (default `pytest`): the emitter is now
  pluggable. `testgen.py` separates the language-independent scenario-collection
  core (`scenarios()`) from per-target emitters (`emit_pytest`/`emit_vitest`), so a
  new harness is a backend, not a redesign. The first new target is **Vitest**: a
  self-contained TypeScript file with the same `reset`/`step`/`observe` `Adapter`
  contract. Deterministic and forbidden-rejection scenarios translate directly; the
  `random-walk` oracle is **baked at generation time** — the Python `Monitor` runs the
  fixed-seed (`Random(0)`) walk and the `(action, params, expected_state)` trace is
  embedded as a static fixture, so the generated tests require no `fslc`/Python at
  runtime (the single independent oracle stays in Python). pytest output is unchanged
  (byte-for-byte on identical scenario input). Vitest output defaults to
  `<spec>.test.ts`; `--target` and `target` flow through `cli.py`/`run_testgen`.
  Docs: `docs/LANGUAGE.md` §12 and `skills/fsl/reference.md` §9. (#43)
- Kernel `spec` now accepts `entity`/`number` declarations (previously dialect-only),
  desugared to `type X = lo..hi` via the `verify` block (`instances`/`values`). This
  separates domain identity from the verification world size so a design-layer spec
  reads as documentation instead of asserting a domain size that is only a model
  bound. Reuses the requirements desugar path (no new semantics); see
  `docs/DESIGN-spec-domains.md`. A new "Authoring specs as readable documentation"
  section in `skills/fsl/reference.md` records the doc-first conventions, and
  `examples/e2e/3_design.fsl` / `examples/e2e/2_requirements.fsl` are rewritten to it.
- (Skill) `skills/fsl-from-code/` — reverse-engineers an FSL design-layer spec from
  existing source code. Encodes the three-zone extraction (mechanical skeleton vs.
  human-confirmed invariants vs. mechanical truth-check), a formalization-memo
  question set that forces cross-action invariant discovery, and a two-axis
  anti-hollow gate (`fslc mutate` for invariant teeth + the `testgen` harness
  replayed against the real code for fidelity). Anchored downward via conformance,
  not upward via refinement; specgraphen evaluated and not adopted.
- examples/structural: Step 1 demand-validation specs for issue #35 (Alloy-style structural discovery via the populate+reachable idiom)
- (Docs) New manual chapter "When to Use FSL" (`docs/intro/when-to-use.{ja,en}.html`,
  wired into the chapter nav as #2, after Concept): criteria for deciding whether to
  use FSL — the interaction-not-size test, three gates, the high-yield / low-priority
  / out-of-scope split, the spec-not-product preconditions, per-feature vs
  per-project altitudes, and a layer selector. Concept's closing CTA now leads here.
- (Skill) `skills/fsl/SKILL.md` gained a "First, decide whether FSL fits" self-check
  (the one test + three gates, framed as a recommendation rather than a gate) so the
  agent filters out non-FSL-shaped tasks and recommends tests instead of writing a
  hollow spec.
- (Skill) `skills/fsl-requirements/SKILL.md` now defines an explicit faithfulness
  gate (definition of done): a step-1 coverage map from every source requirement to
  an FSL element, mandatory provenance tags (`covers` or `MODEL:`/`ASSUME-n:`) on
  every declaration, and a `fslc check --strict-tags` gate that must report zero
  `untagged` and zero `unreferenced_requirement`. Closes the gap where specs derived
  from a requirements document came out both thin (dropped requirements) and
  over-reaching (invented rules).

### Changed
- (Skill) Reframed the FSL skill set from "single-spec verification" toward
  "verifying cross-layer connection" (issue #48) — framing/routing only, no kernel
  or verifier change. `skills/fsl/SKILL.md` gains a one-paragraph connectivity lead,
  a connectivity-value lens in the fit self-check (a recommended lens with an
  abstraction-tax warning, explicitly *not* a fourth gate), a "Connected workflow
  (across layers)" juxtaposed with the single-spec workflow with `implements` /
  `refine` / `preserve progress` / `chain` as first-class operations, and a
  heading-level "Two soundness facts about connection" (safety descends but liveness
  does not; a chain is only as strong as its refinement soundness). Role boundaries
  are now stated as refinement seams (contracts), not plain baton passes, and the six
  role skills (`fsl-business` / `fsl-requirements` / `fsl-design` /
  `fsl-design-review` / `fsl-delivery` / `fsl-from-code`) are audited to express the
  handoff consistently. `skills/README.md` promotes cross-layer alignment
  (`fsl-delivery`) to a primary use, and the same connectivity-value criterion is
  synced into the manual's "When to Use FSL" chapter
  (`docs/intro/when-to-use.{ja,en}.html`).
- Removed the maintainer contact email from `SECURITY.md` and `pyproject.toml`;
  vulnerability reports are now routed solely through GitHub Security Advisories
  (Private Vulnerability Reporting).

## [2.3.0] - 2026-06-22

### Added
- (LSP) Added a Python `fslc-lsp` language server with diagnostics, document
  symbols, and go-to-definition over raw Lark parse trees, including compose
  `use ... from` cross-file resolution. Added a VSCode extension scaffold with
  `.fsl` language registration, TextMate highlighting, and an LSP client.
- (Install) `install.sh` now installs the LSP server (the `[lsp]` extra) and
  links `fslc-lsp` into `~/.local/bin` alongside `fslc`, so the VSCode extension
  finds it on `PATH`.
- (VSCode) The extension is now packageable as a `.vsix` (`vsce package`): added
  `.vscodeignore`, a `vscode:prepublish` compile step, and a `repository` field.
  The `release-binaries` workflow builds the `.vsix` and attaches it to the
  GitHub Release on a `v*` tag.

### Fixed
- (LSP) Refinement mapping files are parsed without the standalone-spec state
  block diagnostic, and their `impl`/`abs`, mapped action, and state map
  references resolve across workspace `.fsl` files by spec name.

## [2.2.0] - 2026-06-21

### Changed
- (Grammar) Eliminated ~50-line duplicate `ref_expr` grammar by collapsing it to
  a thin `?ref_expr: ite | expr` wrapper; `if/then/else` remains accepted only in
  the four mapping/acceptance positions (map_def, mapped_action_target,
  req_mapped_action_target, acceptance_arg) and is rejected in ordinary spec
  expressions.
- (Grammar) Removed the dead `glue_action` rule that was never referenced.
- (Grammar/model) Extracted module-level `_args()` helper in `grammar.py` to
  remove three identical `maybe_placeholders` sentinel-filtering list comprehensions
  in the `Ast` transformer.
- (Model) Added `_check_reserved` to `model.py` to reject user-defined state
  variables, consts, and action parameters whose names collide with FSL bare-atom
  literals (`true`, `false`, `none`), giving a clear `kind=name` error instead
  of silent parse ambiguity. Keywords that require an immediately following `(`
  (`count`, `sum`, `stage`, `min`, `max`, `abs`, `old`, `unique`, `exactlyOne`,
  `some`) or a binder (`forall`, `exists`) are contextual and parse unambiguously
  as bare identifiers, so they are not reserved.

### Added
- (Governance/Business) Business specs can declare `control` metadata and attach
  it to checkable `policy`/`goal` declarations with `satisfies`. Violations now
  carry satisfied controls in the requirement payload. New top-level
  `governance` catalogs validate cross-business control delegation and run
  preservation refinements during `fslc check`.
- (HTML report) `fslc html <file.fsl> [-o report.html]` generates a self-contained
  review report from `explain` + `verify`: status summary, state/action tables,
  an action-to-state SVG graph (arrows show the write direction), traces, witnesses,
  counterfactual evidence, escaped source, and raw JSON payloads. Counterexamples
  surface a verdict banner and highlight the violating trace step, with violation
  facts humanized in the status table; reachable runs are labelled as such. Without
  `-o`, the HTML is written to stdout.
- (Refinement) Mapping files can opt into liveness-preserving refinement with
  `preserve progress { respond <AbsLeadsTo> by <impl actions...> }`. The check
  pulls the named abstract `leadsTo` through the state mapping and reports
  `refinement_failed / progress_lost` when the lower layer can stall or spin
  while the abstract response remains pending.

## [2.1.0] - 2026-06-18

### Added
- (Induction) Automatic synthesis of `leadsTo` ranking measures (follow-up to
  #11). When a `leadsTo` has no author-supplied `decreases`, `fslc verify
  --engine induction` tries a small set of candidate measures (stage-rank sum
  derived from the action graph, integer distance-to-target, pending count) and,
  if one satisfies the existing ranking obligations, proves the response
  unboundedly and reports `"proof": "ranking", "synthesized": true`. Sound by
  construction — candidates are validated by the unchanged proof obligations, so
  a wrong candidate can only fail, never yield a false proof; if none works the
  leadsTo keeps its bounded result. Branching multi-action flows with no global
  ranking measure fall back to bounded checking as before.

## [2.0.0] - 2026-06-18

Theme: **human-readable business/requirements dialects** (issue #21) — verification
bounds, KPIs, and refinement mappings stop masquerading as model facts, and the
requirements layer gains a readable process+data profile close to the business surface.

### Added
- (Dialects) `entity <Name>` / `number <Name>` type-kinds and a top-level
  `verify { instances E = N ; values X = lo..hi }` block that holds the bounded-model
  sizes in one honest place. Kernel `type X = lo..hi` is unchanged.
- (Requirements) A `process <Entity> with <field>: <T> { ... }` profile: business-style
  `stages`/`initial`/`transition ... by`, extended with `with <input>`, `when <guard>`,
  `set <field> = <expr>`, and `covers REQ-n "text"` traceability. It lowers to a kernel
  state machine; an empty `implements X from "..." { }` body auto-generates the identity
  refinement when names match. Acceptance gains a readable `expect <Entity> <id> in <Stage>`.
- (Requirements) `maps auto` is now allowed inside an `implements { }` block.
- (Safety) Auto-mapped process transitions are statically actor-checked against the
  business layer (the verifier cannot — actors are not refinement state), so an actor
  mismatch is a check-time error instead of a green-but-wrong refinement.
- (Explain) `fslc explain --readable` renders a deterministic text view that surfaces
  verification bounds, fairness, KPI projections, branch lowering, and the synthesized
  refinement mapping (auto-mapped entries flagged for actor/intent review).

### Changed
- (Dialects) `kpi NAME = count ENTITY in STAGE` is now a declarative derived projection
  (available in business and requirements) carried as metadata — no ghost counter, no
  per-transition increment, no auto `_kpi_*` consistency invariant.
- The "spec declares no user invariants" warning no longer fires when the spec has
  leadsTo/reachable/trans properties, acceptances, forbidden flows, or an `implements`
  refinement (those are checked too).

### Removed
- **BREAKING:** business `case X = lo..hi` (use `entity X` + `verify { instances X = N }`).
- **BREAKING:** `kpi ... counts ... in ...` (use `kpi ... = count ... in ...`); the
  KPI counter-consistency auto-invariant and the "decrement KPI unsupported" error are
  gone (a projection is exact by construction).

## [1.5.0] - 2026-06-18

Theme: **honest verification bounds, AI-legible diagnostics, and tractable liveness** —
verify/induction results now declare their `completeness`, `checked_to_depth`, and `cost`;
diagnostics gain a `faithfulness_class` routing tag, split-action `display_name`s, sharper
`insufficient_depth`/`over_constrained` reachable classification, and a new `urgency_freeze`
vacuity lane; liveness gains ranking-function (`decreases`) proofs and `symmetric type`/`enum`
reduction; and the workflow adds `fslc chain`, partial `testgen`, `--exclude-property`, and
`maps auto`, plus a compose `fair`-not-inherited warning and friendlier identifier parse errors.

### Added
- (Temporal) FSL now supports `symmetric type` and `symmetric enum` declarations
  for interchangeable entity identities. `leadsTo` lasso and stall checks use a
  canonical representative for per-entity `Map<SymmetricType, ...>` / `Set`
  rows, reducing symmetric liveness search without changing the JSON envelope.
- (Induction) `leadsTo` declarations can now include `decreases <int expr>`.
  Under `fslc verify --engine induction`, fslc proves the response unboundedly
  with a ranking argument: the measure is non-negative while `P` is pending and
  `Q` is false, pending states cannot deadlock, and every enabled action either
  establishes `Q` or keeps `P` pending while strictly decreasing the measure.
- (Refinement) Mapping files now support `maps auto`, which synthesizes identity
  mappings for same-named compatible state variables and same-named compatible
  actions unless an explicit `map` or `action ... ->` entry overrides it.
- (Testgen) `fslc testgen` now generates partial pytest scaffolds when some
  `reachable` targets are not witnessed at the requested depth. Witnessed
  scenarios are still emitted, unwitnessed targets appear in `warnings[]` with a
  depth hint, and `--strict` restores the previous `reachable_failed` abort.
- (Vacuity) `fslc verify --vacuity` now emits `kind:"urgency_freeze"` for the
  requirements time/deadline trap where Z3 proves the generated urgent condition
  holds initially and is preserved by every action, so generated `tick` is dead
  and deadline invariants are vacuous.
- (Verifier CLI) `fslc verify --exclude-property <Name>` is repeatable and
  skips named invariants, `trans`, `leadsTo`, and `reachable` properties in both
  BMC and induction runs. It mirrors the 1.4.0 cross-kind `--property` resolver;
  exclusion wins when both options name the same property.
- (Verifier JSON) `verify` / induction results now expose boundedness metadata:
  `completeness`, `checked_to_depth`, and `cost.elapsed_s`. BMC `verified` is
  explicitly `completeness:"bounded"`, induction `proved` is
  `completeness:"unbounded"`, and bounded `verified` adds a saturation hint when
  normal exploration first witnesses a reachable/vacuity/coverage fact at depth K.
- (Reachability diagnostics) `reachable_failed.unreached[]` now classifies each
  target as `insufficient_depth` or `over_constrained`; over-constrained targets
  include a `blocking_requires` unsat-core-style list naming the blocking type
  bounds/invariants. The same classification is emitted by `fslc scenarios`.
- (Diagnostic routing) Diagnostics can now carry additive `faithfulness_class`
  and `recommended_action` fields for `partial_op_unguarded`,
  `frozen_only_invariant`, `intent_unexercised`, and the reserved
  `liveness_not_refined` route.
- (Diagnostics UX) Branch-split action diagnostics keep the internal action name
  and add `display_name` such as `submit[a <= AUTO_LIMIT]`. Coverage
  `blocking_requires` hints now summarize the blocking factors after a cheap
  core-minimization pass.
- (Chain CLI) `fslc chain [fsl-project.toml]` runs a manifest-defined
  business -> requirements -> design -> implementation pipeline, reusing the
  existing `check`, `verify`, `refine`, and shell implementation commands. It
  writes a consolidated status table to stderr, JSON to stdout, supports
  `--keep-going`, and exits non-zero when any layer fails.

### Changed
- (Parser/UX) Invalid identifier characters such as `foo$bar` now produce a
  focused parse diagnostic that states the identifier rule instead of leaking raw
  Lark terminal expectations.
- (Refinement) Mapping-file action formals now accept `name: Type` annotations,
  validated against the implementation action's declared parameter types.
- (Documentation) Clarified compose synchronized-argument compatibility as structural
  over bounded value ranges rather than nominal type names, including same-range
  and narrower-target repro results. Also documented action-level
  `maps stutter` in requirements/refinement docs and clarified that distinct
  fields of the same `Map<K, Struct>` element may be updated independently in
  one action.

### Fixed
- (Explain JSON) Counterfactual violation diagnostics no longer emit redundant
  raw `internal_invariant` compose names when the public `invariant` field already
  carries the dotted display name.
- (Compose) Non-fair synchronized actions that reference fair component actions
  now emit a `fair_not_inherited` warning instead of silently hiding the dropped
  composite-level liveness assumption.

## [1.4.0] - 2026-06-17

Theme: **probing single properties and friendlier IDs** — `verify --property`
becomes a general property probe across all declaration kinds, requirement-style
IDs accept underscores, and the liveness/safety scaling trade-off is documented.

### Added
- **`verify --property <Name>` now targets any property kind**, not just
  invariants. The name is resolved across `invariant`, `trans`, `leadsTo`, and
  `reachable` declarations and checked in isolation while the full action model
  still steps, so a single property can be probed on its own (e.g. iterating on a
  slow `leadsTo` without gating the safety checks).
- **Underscores are accepted in requirement-style IDs** (`REQ_ID`): `acceptance`,
  `forbidden`, `requirement`, `policy`, and `goal` IDs now allow `AC_DONE` in
  addition to `AC-DONE`, matching the underscore already permitted in
  action/invariant/trans names. Purely widens the accepted set — existing
  hyphenated IDs are unchanged.

### Changed
- **`--property` not-found diagnostics** now read `no such property: X
  (available: …)` and list every property kind. Under `--engine induction`
  (k-induction proves safety invariants only), naming a `trans`/`leadsTo`/
  `reachable` now reports that the induction engine cannot prove it and to use the
  default `bmc` engine, instead of a misleading "no such invariant".
- **Documented the liveness/safety scaling difference** (`skills/fsl/reference.md`
  §7): `leadsTo` cost grows roughly exponentially in the number of concurrent
  entities (the textbook BMC-liveness state explosion), while safety stays cheap.
  Added the practical strategy — verify liveness on a reduced model and safety
  separately at full size, and use `--property` to isolate one liveness property
  while iterating.

## [1.3.1] - 2026-06-17

Theme: **FSL delivery orchestration skill** — making the business → requirements →
design → implementation-conformance workflow directly invokable as a lifecycle
skill, while also adding readable business-stage syntax.

### Added
- **`fsl-delivery` Agent Skill**: a lifecycle coordinator that routes multi-layer
  work across `fsl-business`, `fsl-requirements`, `fsl-design`, and
  `fsl-design-review`, keeps layer boundaries explicit, and reports business,
  requirements, design/refinement, and implementation-conformance proof states
  separately. The install script and skill documentation now include it.
- **Readable fsl-biz stage syntax for PM/consulting-facing policies and goals**:
  `policy ... every Case in Stage must eventually be Target [or Target ...]`,
  `goal ... some Case can reach Stage`, and
  `goal ... all Case can be Stage [or Stage ...]`. These are AST sugar for the
  existing `responds` / `reachable` forms, so kernel semantics and JSON output
  remain unchanged while common business-flow rules no longer require reading
  `stage(c) == ... ~> ...` formulas.

## [1.3.0] - 2026-06-16

Theme: **propagation review for layer chains (fsl-design-review)** — establishing that
refinement propagates safety but not liveness, and adding end-to-end chain checking.
Also unifies the two FSL expression evaluators behind a shared, domain-parameterized core.

### Changed
- **Unified the symbolic (`bmc.py`, Z3) and concrete (`runtime.py`, Monitor) FSL evaluators**
  behind a single shared core (`src/fslc/values.py`) parameterized by a per-evaluator domain
  object (`_SymDomain` / `_ConcDomain`). The two evaluators previously re-implemented the same
  expression semantics, a drift hazard where the verifier and the replay Monitor could disagree.
  Unified: count, sum, quant, the Option/Seq/struct comparisons, `is`-patterns, field/index access,
  and map access. Behavior-preserving — the verdict-level output is byte-identical across the whole
  spec corpus, guarded by two new safety-net tests (`tests/test_corpus_snapshot.py`,
  `tests/test_evaluator_agreement.py`). Genuinely divergent pieces (Seq/Set method evaluators,
  `compute_updates`, `_eval_requires`, display) are intentionally left per-evaluator. Internal
  refactor only — no change to the CLI, JSON output, exit codes, or grammar.
- Split the over-long `cli.main`, `dialects.expand_business`, and `compose.expand_compose` into
  named private stages (no behavior change).

### Added
- **`fslc refine` chain mode (mapping composition)**: when you line up successive `(spec mapping)`,
  it composes adjacent mappings (states α_AC = α_BC ∘ α_AB, actions a→b→c / stutter) and
  checks **bottom ⊒ top directly**. On success it returns the composed `action_map` and `chain`; on failure it returns
  the first broken link, `failed_link`. Because bounded refinement is transitive at the same depth,
  the composition check is equivalent to all adjacent links holding (`DESIGN-refinement` §7, example `examples/refinement_chain`).
  State mappings are composed at the Z3 level, and indexed maps, Option, and structs are handled by the existing eval.
- Examples `examples/refinement_liveness` (safety propagates, liveness does not, resolved with fair) and
  `examples/refinement_chain` (chain checking), each with its own checking test.
- **A set of self-specs** in `examples/self/`: three specs that model fslc's own design contracts
  in FSL (`fslc_session` = CLI result classification and exit-code severity,
  `fslc_monitor` = stickiness of replay-runtime rejection, `refinement_algebra` = safety
  propagates, liveness does not). All are proved. Pinned-result test `tests/test_self_examples.py`.
- **`terminal { <predicate> }` block**: declares a halting state satisfying the predicate
  as an "intended terminal" and excludes it from deadlock checking. Whereas `--deadlock ignore`
  uniformly ignores all halting states, this lets you single out only the intended halts, while unexpected deadlocks are
  still detected. Used by `examples/self/fslc_session` and `fslc_monitor` (LANGUAGE §1/§6).
- **`fslc verify --property <Name>`**: checks just a single invariant.
  This makes it easier to confirm a violation of a targeted invariant with a non-vacuous probe (a nonexistent name is a usage error = exit 2).
- **Vacuity detection of dead-ghost tautologies**: `--vacuity`
  now statically detects with Z3 an "invariant that, when a frozen state variable assigned by no action is pinned to its init value, becomes
  always true regardless of the values of dynamic variables" (kind `tautology_over_frozen`). It warns at verification time about
  hollow (always-true) invariants that previously both verify and vacuity missed, surfacing only via mutate's survival rate.
  Invariants that do not reference a frozen variable / do not reference state are out of scope. Confirmed zero false positives across the existing corpus.
- **Transition invariant `trans { }`**: `trans Name { old(x) => ... }`
  lets you directly declare cross-action two-state safety. BMC checks each reachable transition, induction checks it in the step case,
  successful output includes `transitions_checked`, and a violation returns `violation_kind:"trans"`.

### Fixed
- **Test suite runs without a `.venv` (CI portability)**: the subprocess-based tests invoked the
  CLI through a hardcoded `ROOT/.venv/bin/python` and a macOS-only `/private/tmp` scratch path,
  which failed on the CI runners. Now use `sys.executable` and `tempfile.gettempdir()`.
- **Include the state in the deadlock warning**: the `--deadlock warn` warning
  message now shows which state it halted in (e.g. `deadlock reachable at step 1
  (state: status=ToolFault, ...)`). The state was previously only in the JSON `deadlock.trace`.
- **Soundness bug in `fslc refine`**: when an impl's violating transition reached a terminal (deadlock) state within the bound,
  forcing a full-length trace excluded the violation from all models, so it was missed
  (a non-monotonic behavior where raising the depth reduced detection). Resolved by switching to a dedicated solver that
  checks each prefix with only the constraints up to step t. Added a regression test (a residual case of the
  "vacuous refines" regression class in `examples/gallery/`).

### Documentation
- **Rescoped the layer-chain propagation claim to safety**: in `DESIGN-layers` §1/§6 and `LANGUAGE` §10,
  made explicit that refinement propagates safety (invariants, control guards, behavioral inclusion) but not liveness
  (`leadsTo`/`responds`), because of stuttering, and that liveness must be re-verified at each layer
  with `fair` required on progress actions.
- Documented the self-spec findings in their authoritative surfaces: dead-ghost
  vacuity, intended terminal states, transition invariants, finite-state
  expressiveness limits, deadlock diagnostics, and single-property selection.

### License / distribution (preparing for OSS release)
- **Finalized the license as Apache License 2.0** (rights holder: Copyright 2026 Ryoichi Izumita).
  Added `LICENSE` (full text) and `NOTICE`. Updated pyproject's license to the SPDX form `Apache-2.0`
  (previously only the `MIT` label with no LICENSE file), and tidied up authors, urls, classifiers, and keywords.
  Added an `SPDX-License-Identifier: Apache-2.0` header to all Python sources. The dependencies (lark / z3-solver) are
  both MIT and compatible with Apache-2.0.
- Updated the public repository URL to `github.com/ymm-oss/fsl` (links in README / install.sh / CHANGELOG,
  and reworded private-assumption phrasing for public release). Removed the generated search index `docs/index.bleve/` from tracking,
  added it to `.gitignore`, and also added Claude Code's local settings to the ignore list.

## [1.2.10] - 2026-06-15

Theme: **audit triage (issue #12) — settling two design decisions (doc alignment)**. We analyzed that
keeping the code as-is is appropriate and aligned the DESIGN documents with the actual state and intent.

### Documentation
- **Aligned the check ordering in DESIGN-refinement §2** with reality. For t>0 (between steps), the transition correspondence is checked
  before the type-bound check; for t=0 (initial state), the type-bound check (`map_out_of_bounds`) is done before the init correspondence.
  Because at t=0 a range escape almost always accompanies an init mismatch, we prioritize `map_out_of_bounds`, which can directly point at
  a mapping-expression bug — reflecting the design intent (the purpose in §2), and resolving the
  self-contradiction in the previous ordering description.
- Made explicit in **DESIGN-seq §5** the cross-engine difference for invariants containing unguarded partial Seq operations
  (`head`/`pop`/`at`). `verify`/`prove` (BMC) read don't-cares symbolically, while the runtime
  `Monitor` concretely returns `partial_op`. Because there is essentially no guarantee that a don't-care matches between symbolic and concrete,
  we strongly recommend the size-guarded idiom (the guarded version is verified to agree across both engines).

## [1.2.9] - 2026-06-15

Theme: **audit triage (issue #12) — settling design-decision items (continuation of Batch E-c)**.
Items previously treated as deferred were addressed in line with the recommendation, after verifying on real hardware.

### Fixed
- **A `push` to a full `Seq` was reported by the runtime (Monitor) as `partial_op`**; changed to report it as
  **`type_bound`** (a violation of the implicit `_bounds_*` length invariant), to match BMC / DESIGN-seq
  (`runtime.py`). This resolved the conformance fidelity gap where the same operation split into BMC=`type_bound` / runtime=`partial_op`.
  push always appends as a total function, and exceeding capacity is detected by the post-store bounds invariant.
- Added a note about the case where **`fslc refine` returns an impl's own invariant violation as-is**
  (`refine.py`). Clarified that this is a property of the refinement *input* (the impl spec), not the refinement verdict,
  so it is not confused with `refinement_failed` (LANGUAGE §10).
- (Documentation) Added to `parse()`'s docstring that if you need compose's display names you should use `parse_src`
  (`parse()` discards `display_names`, so dotted aliases appear under their physical names).

### Kept as-is by design decision (recorded in issue #12)
- The t=0 check ordering for refinement: there is tension between the ordering description in DESIGN-refinement §2 and
  `map_out_of_bounds`'s usefulness for "directly pointing at a mapping-expression bug," and existing tests expect bounds-first.
  We keep the current behavior, which precisely points at mapping bugs, and leave the §2 interpretation to maintainer judgment.
- Don't-care handling of Seq head/pop/at in invariant context: a guarded invariant is protected by short-circuiting,
  so the practical harm is small, and `in_invariant` propagation would be a broad change, so we keep the current behavior.

## [1.2.8] - 2026-06-15

Theme: **audit triage (issue #12) — runtime/refine/doc alignment batch (Batch E-c)**.
Items requiring design interpretation were addressed selectively after verifying on real hardware.

### Fixed
- Fixed an issue where **`Monitor.step()` evaluated let before requires and, when the guard did not hold, returned
  `partial_op` instead of `requires_failed`** (`runtime.py`).
  Calling an action that writes `let h = q.head()` after `requires q.size() > 0` on an empty queue
  produced a partial_op rather than a guard failure. Now let and requires are interleaved in source order, and
  on the branch where the guard fails, the subsequent let's partial op is never reached
  (DESIGN-v1 §5: let is usable only in subsequent requires).
- Fixed an issue where **`fslc refine` did not type-check the argument expressions of action mappings**
  (`refine.py`). Per DESIGN-refinement §3, it now matches the abs action's parameter types against
  the static types of the mapping argument expressions (when a type is unknown it skips the check to avoid false positives).
- (Documentation) Added to `parser.py`'s docstring that compose/requirements expansion raises `FslError`
  directly (not wrapped in VisitError). Clarified the DESIGN-bridge §3 statement that "generated artifacts
  import only runtime and pytest" to match the implementation, which also allows `random` for fixed-seed walks
  and `pathlib` for path resolution.

### Remarks (deferred this time by design decision; continued in issue #12)
- The t=0 check ordering for refinement (`map_out_of_bounds` and init correspondence): there is tension between
  the ordering description in DESIGN-refinement §2 and "the usefulness of map_out_of_bounds for directly pointing at a mapping-expression bug" (§2),
  and existing tests expect bounds-first. We keep the current behavior and leave it to maintainer judgment.
- Don't-care handling of Seq head/pop/at in invariant context (alignment with BMC): a guarded
  invariant is protected by short-circuiting, so the practical harm is small, while `in_invariant` propagation would be a broad change, so we
  defer it.
- `parse()`'s base_dir fallback / discarding display_names: this affects the compatibility of the library API,
  so we defer it (the CLI already uses parse_src + the parent directory).

## [1.2.7] - 2026-06-15

Theme: **audit triage (issue #12) — model/grammar alignment batch (Batch E-b)**.
Fixed 4 drifts between LANGUAGE.md / DESIGN-v1 and the code.

### Fixed
- Fixed an issue where **`Set<Bool>` / `Map<Bool, ·>` were rejected as type errors**. LANGUAGE.md §2 includes
  Bool among the bounded scalars. Added Bool to `is_bounded_scalar_type`, and made the Z3 encoding,
  `domain_range`, the implicit bounds invariant, the runtime concrete evaluation, refinement mappings, and display
  support Bool keys/elements (`_z3_domain_value`/`_map_domain`/`_display_map_key`).
- Fixed an issue where **`fslc check` rejected a spec with zero actions as a semantic error**. In LANGUAGE.md
  §7.1, check is syntax/name/types only. Introduced `build_spec(semantic_check=...)`, and the rejection of zero
  actions is now done only on the verify/prove/scenarios side (check is ok, verify is as before).
- Fixed an issue where **the business-dialect-only `stage(...)` passed check in a regular spec and then could not be evaluated by verify**.
  Added `check_stage_usage` to build_spec's semantic check, rejecting `stage(...)` in a kernel spec as a type error
  (since it does not remain in the spec after business expansion, business is unaffected).
- Fixed an issue where **the deprecation warning for `Map<Int, ·>` was only emitted for a spec with a domain type**. Removed the condition so that
  a deprecation warning (with a rewrite hint) is emitted for every spec using an Int-key Map.
- (Test) Hardened the warning assertion in `test_cart::test_fixed_verifies`. Because the Map<Int> warning now
  always fires, the vacuously-true premise that "all warnings are Map<Int" had become fragile. Changed it to confirm the presence of
  the expected 2 Map<Int deprecation warnings (which can coexist with verify's advisory warnings).

## [1.2.6] - 2026-06-15

Theme: **audit triage (issue #12) — explain/testgen batch (Batch E-a)**.

### Fixed
- Fixed an issue where **`fslc explain` uniformly excluded weakening of the init block from the counterfactual search**
  (`explain.py`). A user invariant established in init (e.g. BalanceNonNegative in audit_log.fsl)
  was misreported as "no counterfactual." Now init weakening is also included in the search, and
  the diagnostic is tagged with `origin: "init"` / `label: "init weakening"`.
- Fixed an issue where **the `requirement` of `fslc explain`'s acceptance/forbidden witnesses became None**.
  Registered the id/text of acceptance/forbidden in reqs, and included the scenario's
  `acceptance`/`forbidden` fields in requirement resolution.
- Fixed an issue where **`fslc testgen` did not generate the rejection check for forbidden scenarios**
  (`testgen.py`). In addition to replaying the setup steps, it now generates a pytest that runs the final step and asserts that it is rejected
  (`ok==False`, and the `rejected_by` kind matches when required).
- Added a regression test for compose expansion (Batch D) (verifying the const prefix for type/binder_range/param_range
  and the alias-access rewriting of sync arguments).

## [1.2.5] - 2026-06-15

Theme: **audit triage (issue #12) — compose expansion batch (Batch D)**. Fixed an issue where, when a component's
`const` was referenced in a range/binder/param/sync argument, at expansion time the const was prefixed with `alias__`
but the reference in the expression was not rewritten, leaving it unresolved.

### Fixed
- Fixed an issue where **the domain bound expression of a component's `type T = 0..MAX` was not rewritten and went unresolved**
  (`compose.py` `_prefix_component_items`). After expansion it was inconsistent with `alias__MAX`.
- Fixed an issue where **the lo/hi expressions of `binder_range` (`forall k in 0..MAX`) and `param_range` (`action f(n in 0..MAX)`)
  were not rewritten**. `_rewrite_binder`/`_rewrite_params` now receive the component's const set and
  prefix const references in range bounds.
- Fixed an issue where **`alias.x` references in sync argument expressions were not rewritten and remained in the post-expansion AST**
  (`_expand_sync_action`). They are now resolved to physical names with `_rewrite_expr` before assignment.
- The static check for "type mismatch of synchronized arguments" (DESIGN-compose §2) is not implemented this time, since the compose layer
  has no type inference; it is left to the type check after `build_spec` (the arity check is still performed as before).

## [1.2.4] - 2026-06-15

Theme: **audit triage (issue #12) — acceptance/forbidden/mutate batch (Batch C)**.
Fixed 6 items in acceptance/forbidden scenario replay and mutation aggregation, plus 1 parse bug for
zero-argument steps found during verification.

### Fixed
- Fixed a bug where **a zero-argument call (`noop()`) in an acceptance/forbidden step was parsed as `[None]` and
  caused an arity mismatch** (`grammar.py`). Removed the None coming from `maybe_placeholders`
  (same kind as the refinement zero-argument mapping in commit cca8627). It went undetected because existing examples always used calls with arguments.
  Found while writing the Batch C tests.
- Fixed an issue where **a `const` reference in an acceptance/forbidden step argument was not resolved and was passed as a bare string,
  causing `bad_call`** (`acceptance.py`). Changed `_literal_value` to resolve `spec["consts"]`
  (an undefined const is a structured error).
- Fixed an issue where **when the `expect` expression was non-bool, `_EvalError` propagated and `run_check` crashed without returning an envelope**.
  Now expect evaluation is caught and turned into a structured acceptance-failure result.
- Fixed an issue where **unknown actions / arity mismatches in forbidden setup/final steps raised an exception instead of returning a structured failure dict**.
  Now it returns a result with `failed_step`, and the kind is also split into
  `forbidden_setup`/`forbidden` (resolving the shared `_err`'s fixed `kind="acceptance"`).
- Fixed an issue where **the aggregation of `fslc mutate --by-requirement` mixed in acceptance/forbidden ids and kills**
  (`mutate.py`). Per DESIGN-mutate §4 it now targets only the formalization of the requirement block,
  excluding AC/FB ids (also resolving the erroneous `empty_formalization` attached to AC-2, etc.).

## [1.2.3] - 2026-06-15

Theme: **audit triage (issue #12) — typestate batch (Batch B)**. Fixed 2 from-state
extraction misses in `fslc typestate`. Both misjudged a sound transition as `relational` and unduly dropped the entity's
applicability to `none`.

### Fixed
- Fixed an issue where **the from-state could not be extracted from a conjunction in `requires`, leading to a misjudgment as `relational`**
  (`typestate.py`). For a binding like `requires e.st == A and q > 0`, the `and` node was
  unhandled, so the from-state `e.st == A` was not picked up. Extended `_enum_guard_states` /
  `_opt_guard_states` to recurse over `and` the same way as `or`.
- Fixed an issue where **the from-state in an `if` condition was not extracted, leading to branch transitions being misjudged as `relational`**.
  For a branch like `if light == Red { light = Green }`, it now derives each branch's from-state from the enclosing condition
  (a status-only `else` is treated as the complement). With this,
  `tiny_traffic_light.tick` is correctly classified as `branching` (with a from-state), and
  applicability becomes `full`. `typestate:325` (emitting branching to `_emit_ts`) is also resolved accordingly.

## [1.2.2] - 2026-06-15

Theme: **starting triage of the automated code audit (issue #12) — soundness batch (Batch A)**.
We triaged 44 items (42 unverified + 2 verified-but-unfixed) via 7-way parallel verification and real reproduction, and fixed the 5
that directly bear on soundness/correctness. All confirmed against real CLI behavior.

### Fixed
- Fixed an issue where **a `Set<bounded scalar>` got no implicit type-bound invariant, so out-of-range elements were missed**
  (`model.py`/`bmc.py`/`runtime.py`). `s.add(99)` into a `Set<Id>` (Id=0..3) stayed
  `verified` (a false verification success). Introduced the `set_bounds` AST node, which checks via Z3 ForAll / concrete evaluation that all elements are
  within the element type's range (no false positive for an explicitly initialized set).
- Fixed an issue where **the value-bound invariant for `Map<Int, bounded value>` was not generated, so out-of-range values were missed**.
  `m[0] = 99` into a `Map<Int, Qty>` (Qty=0..5) stayed `verified`.
  Introduced the `map_value_bounds` AST node, which checks the value type's bound over the Int-key Map's effective domain
  (the existing convention `0..max(consts)` of `_map_domain`).
- Fixed an issue where **`fslc explain`'s `--max-mutants` terminated early before the weakening search**
  (`explain.py`). Changed the cutoff to count actually-processed weakening mutants rather than the index across the whole
  `enumerate_mutants` (resolving the loss of counterfactuals).
- Fixed an issue where **a `_PartialOp` (partial operation) during invariant evaluation leaked from `step()` as an exception**
  (`runtime.py`). When an invariant expression hit a division by zero, the head of an empty Seq, etc., the exception propagated,
  violating the DESIGN-bridge §1.2 contract that "step() always returns a result dict." It now returns a structured result as a `partial_op`
  violation.
- Fixed an issue where **`fslc testgen` completely broke with a `NameError` (`parse` not imported) when `-o` was omitted**
  (`testgen.py`). `default_output_name` was calling the unimported `parse`.
  Like `generate_test_file`, it now uses `parse_src(src, base_dir)` and passes the spec file's parent directory as base_dir
  for relative-path resolution of compose specs.

## [1.2.1] - 2026-06-15

Theme: **fixing verified bugs detected by the automated code audit (composer-2.5)**. Fixed 3 items
including 1 bearing on the verifier's soundness (issues #13/#14/#15). All confirmed against real CLI behavior.

### Fixed
- Fixed an issue where **the `where` clause of a `leadsTo` binding was discarded and a wrong `violated` was reported**
  (issue #13). Because `expand_leadsto_bindings` (`bmc.py`) dropped the binder's `where` and enumerated all
  domain values, even `forall p: T where p > 0` checked `p = 0` as a separate binding,
  potentially producing a spurious counterexample at a value not satisfying where (verifier soundness). Fixed to concretely evaluate `where`
  and filter the enumeration. Also fixed the over-constraint (unsat) where `init_constraints`'s `run_collect`
  ignored the `where` of a nested `forall`.
- Fixed an issue where **a missing spec file was reported as `kind:"internal"`/exit 3**
  (issue #14). In `run_check`/`run_scenarios` (`cli.py`), the `except FileNotFoundError`
  came after `except Exception` and was unreachable. Reordered them so io errors become
  `kind:"io"`/exit 2 per LANGUAGE.md §7.
- Fixed an issue where **compile-time integer division `/` was unimplemented** (only `+ - *`) (issue #15).
  Per the "four arithmetic operations" in DESIGN-v1 §3.1, added division to `eval_const` (`model.py`). Its semantics
  match the runtime (`_euc_div`, Euclidean division), and division by zero is a compile error of `kind:"type"`.
  So that a range bound like `type K = 0..(MAX / 2)` passes.

## [1.2.0] - 2026-06-14

Theme: **the validation suite for AI formalization** (roadmap #1 completed). Added a set of detectors that close the gap
between the "internal consistency of the spec" that the verifier guarantees and "whether the spec is faithful to the original intent,"
casting a detection net over the errors AI writes into a spec (under-constraint, vacuity, omission/fabrication, mix-up).
The discipline before writing (formalization memo, recommended practices) goes into the skill, and the effect is measured on an error-injection bench.

### Fixed
- Fixed an issue where **a refinement zero-argument abstract action mapping** (`action foo() -> bar()`)
  failed with the spurious error `expects 0 arguments` (removing the None coming from `maybe_placeholders` in `grammar.py`'s
  `mapped_action_target`/`req_mapped_action_target`). It went undetected because existing specs all mapped zero-argument impls
  to `stutter`. A byproduct of the fsl-ui spike (#9).

### Added
- **The fsl-ui spike** (#9): a study of a screen-transition dialect. We hand-wrote the screen flow of a return-request flow in plain fsl,
  and confirmed it is verified + proved and that the refine into the requirements layer also holds
  (`examples/ui_spike/`, findings in `docs/DESIGN-ui.md`). Screen flows can be expressed without changing the kernel semantics,
  and the dialect looks viable as AST sugar (go/no-go in DESIGN-ui).
- Added **`fslc explain` (issue #7)**. It turns a spec's skeleton (state/action/requires/writes/
  properties/implicit type bounds and partial_op checks) into JSON via loc-based source excerpting and structural traversal, and, per
  user invariant, generates a counterfactual trace of removing requires/assignment/fair, reusing the
  `mutate`/`verify` machinery. For invariants where no counterfactual is found at depth K, it does not error but states so explicitly,
  and shapes reachable/scenarios witnesses into a staged narration.
- Added **`fslc typestate` (design spec → typestate / applicability check for phantom types + TS scaffold)**.
  For each `(entity, action)`, it judges `derivable` if the from-state is a **local guard on the entity's own state**
  (`requires e.status == S`), `branching` if it is a data-dependent to-state inside an `if`, and `relational`
  if it **assigns a state but has no local guard** (the premise lives in an external structure such as a queue).
  `relational`/`branching` are not exposed in the type; they remain as runtime/verification obligations along with reasons (diagnostics)
  and the action's requirement IDs (the business-layer `transition ... by <actor>`, etc.). An entity's `applicability` is
  `full` only when all transitions are `derivable`/`branching` (it does not claim full while dropping transitions it could not understand).
  The corresponding state machines come in three forms: **enum-valued struct fields, enum-valued state variables (the business
  `process`/stages), and `Option<_>` slots (none/some ≈ Empty/Filled)**.
  `--ts` emits to stdout the TypeScript only for derivable entities. The output is the same JSON envelope as the other commands
  (`result:"typestate"`, exit 0).
- Added **`fslc mutate`** (issue #6). It applies a deterministic single mutation
  (delete/negate requires, delete an assignment, swap enums, ±1 on integer/type bounds, swap then/else,
  delete fair) to the post-dialect-expansion kernel AST, re-runs `build_spec` per mutant, and reports in JSON whether it can be killed
  by BMC/acceptance/forbidden/refinement. A spec whose baseline is not clean is not mutated and returns the
  baseline result. `--by-requirement` aggregates by the requirement tag of the killed property and warns on a
  zero kill as `empty_formalization`. Survivors are review data, and `mutate`'s exit is always 0.
- Added the **`--strict-tags` lint** (issue #5) to `fslc check` / `fslc verify`.
  Only for ok/verified/proved successful results, it emits as warnings the untagged action/invariant/reachable/leadsTo and
  unreferenced requirement IDs (`--requirements ids.txt` and the requirements-dialect `requirement`
  block). The dialect-generated `tick` is excluded by an explicit marker, and the default (no flag)
  output is as before.
- Added **vacuity checks** (issue #4) to `fslc verify`. On the verified/proved path it emits as warnings
  `vacuous_implication` (the unreachable antecedent of an implication invariant), `vacuous_leadsto`
  (an unreachable leadsTo trigger), and `always_true_requires` (a requires clause that is always true
  under the preceding requires context). Added `--vacuity warn|error|ignore`
  (default warn), where error makes it `result:"error"` / exit 2.
  Actions with coverage false and compose synchronized actions are out of scope for `always_true_requires`
  (a synchronized action's clauses are inherited copies from its components — an identical guard across components is by design,
  with each component defending its own contract, and is checked by verifying the component spec alone).
- Added **`forbidden` (negative acceptance criteria / must-forbid)** (issue #3) to the requirements dialect.
  `forbidden FB-1 "source text" { <steps> expect rejected }` writes an "operation sequence that should be rejected,"
  and at check time the concrete Monitor verifies that all setup steps are ok and **the final step is rejected**
  (either not-enabled or an invariant/type_bound/partial_op/ensures violation). If it is
  accepted, `kind: "forbidden"` (detecting under-constraint = a missing guard that a safety invariant would stay silent on);
  if the setup is not enabled, `kind: "forbidden_setup"`. It outputs `forbidden_<ID>`
  to scenarios (with `rejected_by`) and flows into testgen's negative tests. The verification engine and
  Monitor are unmodified.

### Documentation / workflow
- Added the **validation workflow for AI formalization** (issue #2) to the skill.
  The discipline that closes the gap between the "internal consistency" the verifier guarantees and faithfulness to the original intent:
  a **formalization memo** before writing (chat output, with only assumptions folded into the `.fsl` as `// ASSUME-n:`
  comments/tags), a **natural-language→syntax reverse-lookup table**, the discipline of appending to the assumption ledger during repair,
  and **recommended practices** (positive-example pairs, one requirement = one declaration, domain sizing,
  cross-verification of high-risk specs — all optional; heavy procedures are not mandated).
- Added `examples/validation/order_refund.fsl` (proved) as the maintained example
  of the workflow. Demonstrated how the positive-example pair
  `reachable FullyRefunded` catches with `reachable_failed` a first version where "the safety invariant passes but the refund path is dead."
- Expanded the documentation index for the validation artifacts available at that release.

## [1.1.0] - 2026-06-12

### Added
- **Integer division `/` and remainder `%`** (added to arithmetic, at the same precedence as `*`). Division by zero is
  defined totally as 0 in both evaluators (and explicitly pinned in the Z3 encoding), and in an action context the divisor != 0 is
  checked as an implicit `partial_op`. Negatives are Euclidean (`0 <= a%b < |b|`).
  → This lets you write axis recovery (`c / SLOTS`, etc.) when 2D data is flattened into a single key.

### Documentation / idioms
- Added to LANGUAGE.md and the skill the **idiom for flattening 2D data** (Maps cannot be nested → a single product-domain type + `/` `%`).
- **Codifying discrete-time SLAs**: the placement rules for `time`/`deadline`, the semantics of `age`, and the meaning of
  `urgent` = freezing time. In particular, made explicit the **trap of making an always-enabled action urgent, which makes the
  deadline hold vacuously**, and the correct **deadline-urgency pattern**
  (make urgent only a guarded action that is enabled only when the deadline is reached).
  Added the official example `examples/nfr/support_sla.fsl` (proved).
- External authoring trials motivated the above documentation improvements for
  independently producing a proved spec from the skill alone.

## [1.0.3] - 2026-06-12

### Added
- Added `CHANGELOG.md` (this file), so the changes per release can be seen at a glance.

## [1.0.2] - 2026-06-12

### Fixed
- **BUG-020**: Fixed an issue where `Monitor.enabled()` raised a `_PartialOp` exception on a `let` containing a guarded partial operation
  (e.g. `let j = queue.head()` after `requires queue.size() > 0`).
  Now `requires` is evaluated first and short-circuits, so an action that does not satisfy its guard is simply treated as not enabled. The
  `partial_op` detection during `step()` execution is maintained as before. `fslc verify` was already correct, and
  the impact was on the runtime Monitor / replay / testgen.

### Quality assurance (tests)
- Added a **Z3-independent brute-force ground-truth oracle** (`tests/oracle.py`). It BFS-exhausts the bounded reachable states with the Monitor's
  concrete semantics and cross-checks the ground truth of invariant violations, reachability, and
  deadlock against the BMC verdict (detecting false negatives = misses).
- Added test suites for **concrete-replay soundness** of counterexample traces and witnesses, an **independent refinement oracle**,
  **metamorphic** properties (guard removal → violation, rename invariance, depth monotonicity), and
  **robustness** (JSON serialization, exit-code consistency, no leakage of internal names).
- Total tests 208 → 301 (+69 skipped, about 260 seconds).

### Documentation
- Updated the README to the current state with test count, docs list, examples tree, etc.

## [1.0.1] - 2026-06-12

### Fixed
- **A soundness bug in refine**: when the impl deadlocked just short of the search depth,
  the full expansion became unsatisfiable so all violation checks were missed, and it wrongly
  returned `refines`. Fixed by switching to incrementally checking each reachable prefix and cutting off
  at the depth where it became unsat (resolving the miss of control violations).

### Added
- `fslc version` / `fslc --version` / `-V` (version display).

## [1.0.0] - 2026-06-11

The de facto first release. FSL (AI-native formal specification language) and the verifier `fslc`.

### Core verification
- **BMC** (bounded model checking, shortest counterexample) / **k-induction** (`--engine induction`,
  infinite-depth `proved` and `unknown_cti`→auxiliary-invariant loop).
- `invariant` / `reachable` (witness) / `leadsTo` + weak fairness (`fair`,
  lasso counterexamples). Automatic checks: type bounds, partial operations (`partial_op`),
  action coverage (unsat-core diagnostics), and deadlock.
- Type system: domain types, enum, struct (`Option<scalar>` fields allowed),
  `Option<T>` / `Map` / `Set` / `Seq<T, N>`.

### Implementation bridge, composition, refinement
- `fslc scenarios` (integration-test scaffold), `fslc replay` (log conformance),
  `fslc testgen` (pytest conformance scaffold), `fslc.runtime.Monitor` (concrete execution).
- `fslc refine` (fidelity checking via refinement mapping, with conditional expressions in mapping expressions).
- `compose` (namespaced composition, synchronized actions, `internal`).

### The three-layer dialects and traceability
- `business` (consulting) / `requirements` (requirements, with `branches`, `acceptance`,
  `implements`) / `spec` (design), chained via refinement.
- The declaration tag `"ID: source text"` propagates requirement IDs into all diagnostics (counterexamples, CTIs, coverage, scenarios).

### Non-functional requirements
- Behaviors for authorization, audit, capacity, and reliability are done with idioms, while SLAs/timeouts are checked with
  discrete time (`time` / `urgent` / `age` / `deadline`).

### Distribution and use
- A case-study gallery (valid examples / invalid-example catalog / adversarial), examples for PM/consulting/three-role integration, and
  an example conformance test against a plain Python implementation.
- A one-liner installer (with ZIP-download support) and an Agent Skill for AI agents.

[Unreleased]: https://github.com/ymm-oss/fsl/compare/v5.0.0...HEAD
[5.0.0]: https://github.com/ymm-oss/fsl/compare/v4.12.0...v5.0.0
[4.12.0]: https://github.com/ymm-oss/fsl/compare/v4.11.0...v4.12.0
[4.11.0]: https://github.com/ymm-oss/fsl/compare/v4.10.0...v4.11.0
[4.10.0]: https://github.com/ymm-oss/fsl/compare/v4.9.0...v4.10.0
[4.9.0]: https://github.com/ymm-oss/fsl/compare/v4.8.1...v4.9.0
[4.8.1]: https://github.com/ymm-oss/fsl/compare/v4.8.0...v4.8.1
[4.8.0]: https://github.com/ymm-oss/fsl/compare/v4.7.0...v4.8.0
[4.7.0]: https://github.com/ymm-oss/fsl/compare/v4.6.0...v4.7.0
[4.6.0]: https://github.com/ymm-oss/fsl/compare/v4.5.0...v4.6.0
[4.5.0]: https://github.com/ymm-oss/fsl/compare/v4.4.1...v4.5.0
[4.4.1]: https://github.com/ymm-oss/fsl/compare/v4.4.0...v4.4.1
[4.4.0]: https://github.com/ymm-oss/fsl/compare/v4.3.0...v4.4.0
[4.3.0]: https://github.com/ymm-oss/fsl/compare/v4.2.0...v4.3.0
[4.2.0]: https://github.com/ymm-oss/fsl/compare/v4.1.0...v4.2.0
[4.1.0]: https://github.com/ymm-oss/fsl/compare/v4.0.0...v4.1.0
[4.0.0]: https://github.com/ymm-oss/fsl/compare/v3.1.0...v4.0.0
[3.1.0]: https://github.com/ymm-oss/fsl/compare/v3.0.0...v3.1.0
[3.0.0]: https://github.com/ymm-oss/fsl/compare/v2.7.0...v3.0.0
[2.7.0]: https://github.com/ymm-oss/fsl/compare/v2.6.3...v2.7.0
[2.6.3]: https://github.com/ymm-oss/fsl/compare/v2.6.2...v2.6.3
[2.6.2]: https://github.com/ymm-oss/fsl/compare/v2.6.1...v2.6.2
[2.6.1]: https://github.com/ymm-oss/fsl/compare/v2.6.0...v2.6.1
[2.6.0]: https://github.com/ymm-oss/fsl/compare/v2.5.0...v2.6.0
[2.5.0]: https://github.com/ymm-oss/fsl/compare/v2.4.0...v2.5.0
[2.4.0]: https://github.com/ymm-oss/fsl/compare/v2.3.0...v2.4.0
[2.3.0]: https://github.com/ymm-oss/fsl/compare/v2.2.0...v2.3.0
[2.2.0]: https://github.com/ymm-oss/fsl/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/ymm-oss/fsl/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/ymm-oss/fsl/compare/v1.5.0...v2.0.0
[1.5.0]: https://github.com/ymm-oss/fsl/compare/v1.4.0...v1.5.0
[1.4.0]: https://github.com/ymm-oss/fsl/compare/v1.3.1...v1.4.0
[1.3.1]: https://github.com/ymm-oss/fsl/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/ymm-oss/fsl/compare/v1.2.10...v1.3.0
[1.2.10]: https://github.com/ymm-oss/fsl/compare/v1.2.9...v1.2.10
[1.2.9]: https://github.com/ymm-oss/fsl/compare/v1.2.8...v1.2.9
[1.2.8]: https://github.com/ymm-oss/fsl/compare/v1.2.7...v1.2.8
[1.2.7]: https://github.com/ymm-oss/fsl/compare/v1.2.6...v1.2.7
[1.2.6]: https://github.com/ymm-oss/fsl/compare/v1.2.5...v1.2.6
[1.2.5]: https://github.com/ymm-oss/fsl/compare/v1.2.4...v1.2.5
[1.2.4]: https://github.com/ymm-oss/fsl/compare/v1.2.3...v1.2.4
[1.2.3]: https://github.com/ymm-oss/fsl/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/ymm-oss/fsl/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/ymm-oss/fsl/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/ymm-oss/fsl/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/ymm-oss/fsl/compare/v1.0.3...v1.1.0
[1.0.3]: https://github.com/ymm-oss/fsl/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/ymm-oss/fsl/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/ymm-oss/fsl/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/ymm-oss/fsl/releases/tag/v1.0.0
