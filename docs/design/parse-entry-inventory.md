# Parse-entry inventory and the published `fsl 5` edition

Status: internal design note for the `fsl 5` syntax edition. The edition was
developed on `main` behind an unstable gate; #1434 removed the gate, so
`fsl 5` is read unconditionally. A document without an edition header is
still `fsl 4`. User documentation of the edition is owed to #1437.

## Parse options (after #1434)

- `fsl-syntax` owns `ParseOptions`, which no longer carries a feature set:
  `UnstableFeature` / `UnstableFeatures`, `ParseOptions::with_unstable(..)` and
  `ParseOptions::unstable()` are gone. `ParseOptions::stable()` (also
  `Default`) is the only value, the published language, which reads every
  edition header. The type and the explicit options argument of every parse
  entry are kept, so the entries below keep their signatures.
- Entries that take options: `parse_document_with`,
  `parse_surface_document_with`, `dialect_keyword_with`, `lex_with`,
  `lexical_warnings`, `IdentifierRule::for_source`, `edition_header`,
  `document_edition`, `lossless_document_with`, `format_source_with`,
  `canonical_rewrites_with`, `legacy_expression_operator_error`,
  `ParseError::for_edition`, and in fsl-core one `_with_options` entry beside
  each function that reads source text (`parse_kernel_source_with_options`,
  `governance_contract_with_options`, ...). The old names (`parse_document`,
  ...) are the `_with` entry under `ParseOptions::stable()` and now read the
  same language. Every registered dialect frontend is reached only through
  `parse_document_with`, on the token stream it lexed with the same options.
- `legacy_expression_operator_error` is a diagnostic-only reparse of an already
  rejected expression under the original header/grammar and caller's options.
  It uses the same shared parser, never returns a model or broadens acceptance,
  and excludes domain, governance, dbsystem and editionless frontends. It serves
  a headerless CLI `--edition next` and formatter errors without silently
  parsing the document as a different grammar edition.
  `ParseError::for_edition` owns the parse-error/edition classification shared by
  check/verify, formatter, lint, and migrate. Range type bounds disable only the
  operator diagnostic because a following relation arrow belongs to the type
  grammar. A line-terminated expression never classifies the next line's token.
- No entry point reads an opt-in any more. `FSLC_UNSTABLE` and
  `rust/fslc/src/unstable_gate.rs` are removed: the native CLI and the language
  server ignore the variable. The browser Worker's `Request` has no `unstable`
  field; serde ignores an old client's `"unstable"` value of any type as an
  unknown field (`Request` does not deny unknown fields), and its other fields
  keep their types.
- Where a command used to ask whether the gate was on, it now asks the
  document's edition or nothing:
  - the requirements solver checks (`spec_loading::reject_refuted_requirements`,
    the language server's requirements diagnostics) run only for a document
    headed `fsl 5`; `requirements_solver::refute` keeps its own
    next-edition check (`requirements.next`);
  - `missing_edition_header` and the process-form conversion depend on the
    requested edition only;
  - the CLI options `--lang` / `--names` (`fmt`), `--names` / `--lowering`
    (`explain`), `--strict-names` (`lint`) and the `--edition` spellings `4`
    and `5` are accepted without an opt-in. `cli-contract.json` and the help
    list the options; `cli_contract.rs` adds only the `--edition` spellings
    `4` / `5` as an overlay (which spelling to advertise is #1437's);
  - `explain` respells through the model's glossary and prints the readable
    lowering steps for every document; a document without a glossary or
    next-edition lowering steps prints what it printed before.
- The #1434 type-boundary sweep ran `fslc check` on all 651 `.fsl` forms
  under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` (631
  headerless, 20 headed `fsl 5`) and on 488 inline sources of the changed
  tests, with the base binary (`63dd3e02c`) with and without
  `FSLC_UNSTABLE=fsl5` and the head binary. Head equals base with the gate on in complete stdout and exit
  for every form; every headerless form equals base without the gate; the
  `ok`-to-`error` set is empty, and the `error`-to-`ok` set is the 17 corpus
  and 16 inline forms headed `fsl 4` / `fsl 5`. The former R0 test
  (`unstable_gate_r0.rs`) and the leak check (`unstable_gate_leak.rs`) were
  removed with the gate.

## Identifier rule (S2)

`IdentifierRule::for_source(source, options)` picks the lexer's identifier
rule. It is `Unicode` only when the first significant
line (after a BOM, whitespace and `//` comments, as the lexer skips them) is
`fsl`, blanks, `5`; otherwise `Ascii`, the published `[A-Za-z_][A-Za-z0-9_]*`.
The rule reads the header through `edition_header` (below). An ASCII-only
document lexes to the same tokens under both rules.

Under `Unicode`:

- an identifier is `_` or `XID_Start`, then `XID_Continue` (UAX #31), read in
  NFC; a non-NFC spelling is the warning `non_nfc_identifier` and reads as its
  NFC form, so both spellings are one name;
- `・` U+30FB and `･` U+FF65 never join a name (`middle_dot_identifier`),
  although Unicode 15.1 made them `XID_Continue`;
- full-width letters and digits are `fullwidth_ascii_identifier`, other
  full-width ASCII forms `fullwidth_symbol`, each naming the ASCII character;
- an identifier beyond UTS #39 "highly restrictive" on scripts alone (one
  script, Japanese/Korean/Han-with-Bopomofo counted as one, or Latin plus one
  of those) is `mixed_script_identifier`. Identifier status is not consulted,
  so CJK Extension B stays usable;
- U+3000 stays whitespace and is the warning `fullwidth_space`;
- a requirement ID with a non-ASCII character is `non_ascii_requirement_id`
  (the parser; unreachable under `Ascii`).

`IDENTIFIER_DIAGNOSTICS` lists these codes; `canonical_name_not_ascii` is
emitted by alias resolution (S3, below), not by the lexer. Errors are lexer/parse errors with the code
as `diagnostic_code`; warnings come from `lexical_warnings` and are published by
the language server. Commands that parse through the shared modules (`check`,
`kernel`, the language server's diagnostics) receive the options and see the
rule. Rename accepts a new name only if it is
one identifier under the rule of every document it edits.

## Edition header (S1)

`edition_header(source, options)` reads the header before lexing (J2). The
first significant line is a header when it is `fsl`, blanks,
decimal digits, and only blanks to the end of the line; `fsl` followed by
anything else is an identifier, so `const fsl = 1` keeps its meaning.

- `fsl 4` and `fsl 5` are the editions (`Edition::Fsl4` / `Fsl5`); no header
  is `fsl 4`. Any other number is `unsupported_edition` at the header.
- `lex_with` drops the two header tokens, so dispatch and every frontend see
  the document without it; the lossless tree (`lossless_document_with`) keeps
  them, and `fmt` prints the header as written.
- `ParsedDocument::edition` carries the edition. A header on a document whose
  declaration has no syntax edition (`causal`, the ai dialects and `agent`) is
  `edition_header_not_accepted` (decisions/049 D8).
- Literate Markdown blanks every non-fsl line, so a header on the first line
  of the first fsl fence is the first significant line; extraction does not
  parse and needs no options.
- `Edition` is the one edition type: the CLI's `--edition`, `fmt`, `lint` and
  `migrate` all hold `Option<Edition>` (`None` = `current`). `--edition 4|5` is
  accepted (without an opt-in since #1434); `next` is `5`, and outputs spell
  the requested edition `current` / `next`. `document_edition` resolves the
  header against an explicit `--edition`; a mismatch is `edition_conflict`
  (exit 2) at the header.
- A document read in `fsl 5` behaves as `--edition next` does: every migration
  finding is an error (decisions/049 D7 keeps the codes). The language server
  publishes migration diagnostics of such a document as errors.
  Consumer coverage is partial; the covered and deferred modes are specified in
  [DESIGN-migration.md](DESIGN-migration.md), not an all-reader guarantee here.
- `lint` / `migrate` for edition 5 report `missing_edition_header` (severity
  `info`) on a document without a header, and `migrate` inserts `fsl 5` before
  the first significant line. Both stay outside `plan_migration`, which
  `check --edition next` turns into errors.
- `check` / `verify` add `fsl_edition` only when a header was read, so a
  document without one prints what it printed before the edition existed. The
  verify cache key adds the exact `--edition` request (`edition_request`, the
  number) when one is given, so `--edition 4` and an omitted flag never share
  an entry; the header itself is part of the hashed source. Before #1434 the
  key also carried the unstable set when the gate was on, so a `fsl 5`
  document's key changes once.
- Worker check/verify validate options first, then admit source before model or
  solver work. Admitted source results, including errors and headerless documents,
  share edition finalization. Check folds governance only after its
  model/implements path, not after an early project/agent gate; verify retains
  its native outer governance fold before edition finalization.

## Glossary and alias resolution (S3)

- `glossary { Canonical = alias  Entity.field = alias ... }` is read only in a
  `requirements` document headed `fsl 5`, and only as its
  first item (`misplaced_glossary` otherwise). Elsewhere `glossary` is still
  `expected requirements declaration`. The parser learns the edition through
  S4's `SourceFile::edition`.
- Resolution starts on the token stream (`fsl-syntax/src/glossary.rs`): the
  block is checked on its own (`canonical_name_not_ascii` for a left side,
  `duplicate_alias`, `alias_collision`, `alias_shadows_keyword` against
  the union of every keyword spelling (`KEYWORD_SPELLINGS` for the English
  words, the keyword table of S5 for the Japanese ones and the particles),
  `alias_shadows_name`
  against the block's canonical names), then every later identifier token
  that is an alias becomes its canonical name (a field alias becomes the
  field name), so every frontend position reads canonical names. The body of
  `implements NAME from "path" { ... }` is not rewritten; an alias there is
  `alias_in_implements`. Resolution runs before S4's members are read and
  desugared, so aliases work in entity, lifecycle, concept, event and
  scenario text too. After the parse, a non-ASCII identifier that is neither
  an alias nor a keyword spelling is `canonical_name_not_ascii`, so nothing
  non-ASCII can reach a Kernel name; a `scenario` / `forbidden` body is not
  scanned, because its instance labels (`申請1`) become numbers. `Glossary::uses` records each
  resolved alias and each canonical spelling of an aliased name.
- `fsl_core::lower_requirements` (`resolve_aliases.rs`) lowers the resolved
  document, rejects an alias spelled like a declared name of the lowered spec
  (`alias_shadows_name`, e.g. a generated `claim_stage`), and publishes the
  table as `NameMap::glossary()` (`fsl_core::Glossary`). No Kernel name and no
  `PublicName` is ever an alias.
- Display only: `check` (shared with the Worker through `check_stages`) shows
  `'alias（Canonical）'` for each quoted canonical name and adds `name` /
  `display` to a rejection; `explain` shows aliases by default for a document
  with a glossary and canonical names with `--names canonical`; `lint
  --strict-names` reports `mixed_names` (warning) where a canonical name is
  also written as itself. `--names` and `--strict-names` are accepted
  without an opt-in since #1434.
- Not yet: the language server (hover, completion, rename of aliases; S11),
  and `fmt --names` (S10). The document (S10) reads the glossary from the
  checked model: its glossary table, and aliases in its views under
  `--lang ja`.

## Keyword table and particles (S5)

- `fsl-syntax/src/keywords.rs` is the one table of next-edition requirements
  keywords: per context (`KeywordContext`), an English spelling (one or more
  words) and a Japanese one (one word), one to one within a context. The
  parser reads a keyword through it (`peek_kw(Kw::…)`) and accepts the
  Japanese spelling only while `Parser::keywords` is set, which only the body
  of a `requirements` document headed `fsl 5` does (and the dispatch reads
  `要件` as `requirements` only in that edition). Elsewhere `peek_kw` is the
  English `peek_ident`, so published documents parse as before (R0).
- Particles が・を・に・で・は are separate words. A phrase (event
  declaration, rule, decision table row, scenario step) with a particle on
  its line is read by role, in any order, and every word but the verb takes
  one (`mixed_particles`); a role of the wrong kind is
  `particle_role_mismatch`; a particle written onto its word (`従業員が`) is
  `particle_needs_space`, found on the tokens before aliases resolve.
- Both directions of the table are public (`english_of`, `japanese_of`,
  `Keyword::lookup_*`, `Kw::english` / `Kw::japanese`, `PARTICLES`) for
  `fmt --lang` (S10) and the language server's keyword index (S11).

## Fail closed (decisions/049 D2), retired by #1434

While the gate existed, a parse that did not receive the process's options
read the published language, where a header was the unknown declaration
`fsl`, so a command never read one file two ways. With the feature set gone,
every entry reads the header the same way, and
`dispatch::tests::edition_header_is_read_by_every_document_entry` replaces the
former `edition_header_fails_closed_under_stable_options`.

## Inventory, retired by #1434

`rust/fslc/tests/parse_entry_inventory.rs` counted every product reference to
an entry that parses without explicit options, and every explicit
`ParseOptions::stable()`, because such a parse read a different language from
a gated one. `ParseOptions` now has one value, so those references read the
same language as the `_with` entries and the count guarded no behavior; the
test and its inventory were removed with the gate.
