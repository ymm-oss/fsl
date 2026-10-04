# FSL — Obligation catalog (issues #1201, #1202)

## Problem

A verification verdict today is the absence of a failure among the checks an
engine happens to run. Nothing lists the checks a verdict *should* rest on, so a
check one engine performs and another omits is invisible: #1189 (ranked
`leadsTo` without a no-deadlock obligation), #1196 (guard definedness) and
#1217 (`ensures` truth) were each a question BMC asked and `--engine induction`
did not. The C3 assurance matrix
([`DESIGN-assurance-matrix.md`](DESIGN-assurance-matrix.md)) cannot find this
class: its rows come from engine *output* vocabulary (`violation_kind::ALL`, the
Kernel property groups), so a question no engine ever asks is never a row.

The obligation catalog is the missing row set. It is the first stage (P1-a) of
milestone M23's obligation ledger: later stages add per-engine support tables
(P1-b) and a per-run ledger of how each row was settled (P1-c). This document
covers P1-a only.

## Contract

`fsl_core::obligation::catalog(&KernelModel) -> Catalog` lists one row per
authored site and aspect. It is a pure function of the model: no solver, no
property selection, no verification scope. It lives in `fsl-core`, the owner of
the semantic model, so every crate that may read it -- `fsl-verifier`,
`fsl-runtime`, `fsl-tools` -- already depends on it
([`DESIGN-rust-components.md`](DESIGN-rust-components.md)).

**No consumer reads the catalog in P1-a.** Engine, CLI, Worker and Public
Kernel output are byte-identical with and without it; the first consumer is the
P1-c ledger, and any output it adds is additive.

### Row identity

A row's id is `(kind, site variant, name, span offsets, [action, index])`:

- `ObligationKind` is a payload-free `Copy` enum with an `ALL` constant, so a
  support table can match it exhaustively and a test can iterate it.
- `Site` carries the payload. A named site holds a `SiteRef` -- the
  declaration's name and its span's start/end byte offsets. A checked model has
  unique property names (#1192); the offsets keep two same-named declarations of
  a hand-built or rewritten model apart.
- The variant tells apart sites that share a span: an `until` lowers to a
  `<name>_until_safety` trans and a `leadsTo <name>` with the same span, and
  they are `Trans` and `LeadsTo` rows.
- A state variable has no span; its position in `KernelModel::state` stands in.
  `Terminal`, `Init` and `Model` occur at most once per model.

### Rows

| Family | Rows |
|---|---|
| `init` | `InitSatisfiable@Init` |
| state variable `v` | `Holds@TypeBound(v)`, vacuous when the type has no bound to violate |
| `invariant` | `Holds`, `PartialDefined` |
| `trans`, `unless`, `until` safety | `Holds@Trans`, `PartialDefined@Trans` |
| `reachable` | `Witnessed`, `PartialDefined` |
| `leadsTo` (and `until` progress) | `Responds@LeadsTo`, `PartialDefined@Trigger`, `PartialDefined@Goal` |
| `terminal` (when declared) | `PartialDefined@Terminal` |
| action `a` | `PartialDefined@Guard(a)`, `PartialDefined@Body(a)`; per `ensures` *i*: `Holds@Ensures(a, i)`, `PartialDefined@Ensures(a, i)` |
| model | `NoDeadlock@Model` |

Each family has one generator function in `rust/fsl-core/src/obligation.rs`.

### Static vacuity

A row is `statically_vacuous` when no evaluation of its site can fail it. The
predicate is a syntactic over-approximation: it may call a row non-vacuous that
no state can fail, never the reverse, so an engine that skips a vacuous row
skips no real question.

- `PartialDefined` uses the partial-operation inventory of `fsl-core`
  (#1166). An action's guard, body and `ensures` use the same functions as the
  verifier's own skip decision, `action_has_partial_operation_candidate`, which
  moved from `fsl-verifier` into `fsl-core` for this purpose. In property
  context (`invariant`, `trans`, `reachable`, `leadsTo`, `terminal`) `/` and `%`
  are total ([`DESIGN-divmod.md`](DESIGN-divmod.md)) and do not count.
- `Holds@TypeBound(v)` is vacuous for the types `--engine induction` asks no
  type-bound obligation for (`Int`, `Bool`, `Relation`, and composites of them).

## Decisions

- `Corresponds`, `Accepted` and `Rejected` (refinement steps, acceptance and
  forbidden traces) are not P1-a kinds. `catalog` takes only a `KernelModel`,
  which has no trace cases; they arrive with their generator and tests in P1-c.
- A row carries no `claim`, `group` or `catalog_version` field; all three are
  derived from the kind. `PartialDefined` is a required row of its property's
  claim (#1196 reports `_partial_<name>` per property).
- `init` definedness has no row in P1-a.

## Verification

`rust/fsl-core/tests/obligation_catalog.rs` (T2) pins each fixture's catalog as
an exact multiset and checks that ids are unique. The fixtures are the
reproducers of #1192, #1196 and #1217, plus one fixture with every non-ranked
family. Every kind and every site variant occurs in some fixture, and removing
any generator call fails at least one test. Generator coverage is calibrated
with `cargo mutants --no-config --package fsl-core --file
fsl-core/src/obligation.rs` (run from `rust/`), which must leave no surviving
mutant.
