# FSL — coupled-change metatests (LSP index coverage + DESIGN-doc coverage)

Motivation: issue #168. CLAUDE.md/CONTRIBUTING.md state the discipline "a language
feature moves all of its files together", but it was a human checklist. `d1770c4`
showed the failure mode: `src/fslc/lsp/index.py` was outside the list, so the
`ai_component`/`dbsystem` dialects shipped with the LSP entirely dark. Prototyping
this metatest re-found the same class **twice more**: `_parser_for_source` still
does not dispatch `domain` sources (all 4 `examples/domain/*.fsl` crash
`build_index` with `UnexpectedCharacters`), and `is_ai_project_source` files
(`examples/ai/support_answer_quality.fsl`) sniff as `ai` but fail `AI_PARSER`.
The structure that produced d1770c4 is intact; these tests mechanize the check.

One test module, `tests/test_coupled_change_meta.py`, no Z3 dependency (lark +
file scans only; runs in seconds).

## 1. LSP index coverage (grammar production ↔ visitor handler)

`_IndexBuilder.visit` dispatches by reflection: `getattr(self, f"_visit_{node.data}")`,
falling back to a generic child walk that **skips bare Tokens**. So a production
whose NAME/REQ_ID token is a direct child of an unhandled `node.data` silently
drops that token from symbols/references — exactly the `helpful`/`deadline` bugs
of d1770c4. The dispatch mechanism makes coverage introspectable:
`hasattr(_IndexBuilder, f"_visit_{data}")`.

### Corpus and parser selection

Corpus = `specs/*.fsl` + `examples/**/*.fsl` (~180 files; every dialect is
exercised), minus `examples/gallery/errors/` (intentionally invalid sources).
Each file is parsed with the LSP's own `_parser_for_source(source)` — not a
shadow copy — so a dialect the LSP cannot dispatch surfaces as a loud
"corpus file unparseable by LSP" finding instead of being skipped.

### Two-stage check (per file)

1. **Structural scan** — walk the raw tree; for every subtree whose direct
   children include a `NAME`/`REQ_ID` Token, record whether
   `_visit_{node.data}` exists. Stage 1 alone over-reports: e.g. `field_suffix`
   has no handler but its tokens are consumed by the parent `_visit_postfix`
   (50 files, zero missing tokens). So stage 1 only *attributes* findings.
2. **Position cross-check** — `idx = build_index(source, path)`; collect
   `{sym.selection_range.start_tuple} ∪ {ref.range.start_tuple}`; every
   NAME/REQ_ID token in the raw tree must have `(line-1, column-1)` in that set
   (lark is 1-based, LSP 0-based). A missing token is a finding keyed by
   `(parser_id, node.data)` where `parser_id ∈ {kernel, ai, db, domain}`.

A finding fails the test unless its key is in the allowlist. Stage 1 enriches
the failure message ("no handler at all" vs "handler exists but drops tokens").

### Allowlist

`INTENTIONALLY_UNINDEXED: dict[tuple[str, str], str]` — `(parser_id, node.data)`
→ human-readable reason — lives at the top of the test module, one entry per
recorded design decision (issue #167's "loud, not silent"). **Staleness rule**:
every allowlisted `node.data` must still occur in the corpus scan for that
parser; a stale entry fails the test. Initial contents (verified against the
current corpus):

| key | reason |
|---|---|
| `(kernel, control_severity)` | `severity high` — free severity word, not an in-file symbol |
| `(db, column_type)` | `column x: int` — engine scalar-type words; dbsystem has no in-file type decls |
| `(db, check_item)` | `rule <name>` names built-in compatibility rules |
| `(ai, check_item)` | `check hard { rule … }` names built-in hard-check rules |
| `(ai, tool_precondition)` | free-form semantic label (`precondition order_paid`) |
| `(ai, tool_effect)` | free-form effect label |
| `(ai, trust_def)` | `trust medium` — free trust-level word |
| `(ai, atom_name)` | `model gpt_5_5` / `prompt …_v8` — external artifact ids |
| `(ai, failure_target)` | `-> HumanReviewPending` — policy-outcome label, not an in-file symbol |

`(kernel, preservation_refinement)` was considered but dropped: its grammar
(`"checked_by" "refinement" STRING`) has no NAME/REQ_ID child at all, so it
structurally never reaches stage 1 and an allowlist entry for it would always
be flagged stale.

### Real gaps the first run found — fixed in this PR, not allowlisted

- `_parser_for_source` lacked `is_domain_source` → `DOMAIN_PARSER` dispatch
  (every `examples/domain/*.fsl` crashed `build_index`). Added the dispatch
  plus ~35 domain handlers (`domain_def`, `aggregate_def`, `command_def`,
  `event_def`, `effect_def`, `saga_def`, `state_field`, `decide_def`/
  `evolve_def` (reference the command/event, not fresh declarations), …).
- `is_ai_project_source` sources start with `ai_component` (so a naive
  `startswith` check claims them) but are a bundle of independently-parsed
  blocks (`ai_project._top_blocks`), not one Lark tree; `build_index` now
  special-cases them, indexing each top-level block by name/kind directly
  instead of crashing.
- db: `database_def` is now a symbol; the feature-flag family (`env_flag`,
  `flag_variant_list`, `flag_default`, `env_flag_condition`) is now indexed.
  Finding this also surfaced a **pre-existing, unrelated bug**: `_visit_env_artifact`
  (added in `d1770c4`) only ever read the artifact's own direct-child NAME
  token and returned, silently dropping every `env_window`/`env_flag_condition`
  Tree child — every `when flag F=V` condition on a database artifact was
  unindexed. Fixed by visiting Tree children after the artifact ref.
- ai: orchestration `delegation_edge` (`A -> B`), `agent_event`
  (`Agent.failed` — the agent half is a reference, the status half is
  recorded as a non-exported informational symbol, same treatment as
  `_visit_fallback_item`), and `agent_output_def` (visibility list entries
  are agent references, collected directly rather than through the shared
  `name_list` handler, which would mislabel them as `"tool"`) are indexed.

A node.data whose fix is out of scope for the PR that finds it may still land
as a `(parser_id, node.data) → "known gap, #NNN"` entry, but the entry text
must carry an issue reference — an allowlist reason is a design decision, not
a parking lot.

## 2. DESIGN-doc coverage (dialect/feature ↔ docs/DESIGN-*.md)

Three assertions, from mechanical sources of truth (CHANGELOG prose was
rejected as unparseable; README-only was rejected because a feature absent from
README is silent by omission):

1. **README map is bidirectional** — `re.findall(r"DESIGN-[a-z0-9-]+\.md", docs/README.md)`
   must equal `{p.name for p in docs.glob("DESIGN-*.md")}` exactly. Catches both
   "linked but missing on disk" and "on disk but unlinked" (currently 34 = 34;
   this very file must be added to the README map when the test lands).
2. **Kernel dialects** — the alternatives of the `top_def:` rule extracted from
   `fslc.grammar.GRAMMAR` (`spec_def | refinement_def | compose_def |
   requirements_def | business_def | governance_def`) must each be a key of
   `TOP_DEF_DESIGN_DOCS`, whose values are existing doc names
   (`spec_def → DESIGN-v1.md`, `refinement_def → DESIGN-refinement.md`,
   `compose_def → DESIGN-compose.md`, `requirements_def`/`business_def`/
   `governance_def → DESIGN-dialects.md`). A new `top_def` alternative fails
   until mapped.
3. **CLI commands** — enumerate subcommands by introspecting
   `cli._build_arg_parser()` (`argparse._SubParsersAction.choices`); every
   command must be a key of `COMMAND_DESIGN_DOCS: dict[str, tuple[str, ...] | str]`.
   A tuple lists required docs (all must exist on disk); a `str` is an explicit
   waiver reason (e.g. `version`, and `sweep` — a scope-grid driver over
   `verify` with no standalone semantics, documented in LANGUAGE.md). Unknown
   command → fail; stale map key (command removed) → fail. The raw-dialect
   parsers are reached through their commands (`db`, `ai`, `domain`, `compat`),
   so a new dialect needs both its command mapping and its doc.

Gap reporting is a plain assertion diff: the failure message lists the unmapped
keys / missing files / stale entries, nothing else.

## Non-goals

- No check that every grammar production is *exercised* by the corpus (alias
  rules and token-only rules make that noisy); coverage is corpus-driven, which
  matches how the repo treats `specs/`+`examples/` as the behavioral corpus
  (`test_corpus_snapshot.py`).
- No semantic check that a handler indexes tokens with the *right* role/scope —
  that stays in `tests/test_lsp_index.py` unit tests.
- No enforcement that DESIGN docs are up to date, only that they exist and are
  mapped; content freshness remains review territory.

## CI placement

`tests/test_coupled_change_meta.py` belongs to the frozen Python reference
implementation and is no longer run by `.github/workflows/ci.yml`. Run it
manually only if the retained reference must be inspected; active CI coverage
is provided by the Rust workspace tests.
