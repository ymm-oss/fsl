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

"Definedness" below is three rows on the same site: `PartialDefined` (a
partial operation of `docs/manual/LANGUAGE.md` §6), `NoOverflow` (`i64`
overflow of `+ - * / %`, unary `-`, `abs`, `sum`) and `KeyInDomain` (a `Map`
index outside the finite key domain).

| Family | Rows |
|---|---|
| `init` | `InitSatisfiable@Init` |
| state variable `v` | `Holds@TypeBound(v)`, vacuous when the type has no bound to violate |
| `invariant` | `Holds`, definedness |
| `trans`, `unless`, `until` safety | `Holds@Trans`, definedness |
| `reachable` | `Witnessed`, definedness |
| `leadsTo` (and `until` progress) | `Responds@LeadsTo`; `Deadline@LeadsTo` with `within`; definedness of `Trigger` and `Goal` |
| ranked `leadsTo` (`decreases`) | definedness of `Measure`; `RankLowerBound@LeadsTo`, `RankNoDeadlock@LeadsTo`; `RankStep(L, a)` for every action `a`; with `helpful`, `RankHelpfulFair@LeadsTo` and `RankHelpfulSticky@LeadsTo` |
| `terminal` (when declared) | definedness of `Terminal` |
| action `a` | definedness of `Guard(a)` and `Body(a)`; per `ensures` *i*: `Holds@Ensures(a, i)` and its definedness |
| model | `NoDeadlock@Model` |

Each family has one generator function in `rust/fsl-core/src/obligation.rs`.
The rank rows follow the ranking proof of
[`DESIGN-induction.md`](DESIGN-induction.md) §2.3. Whether a `helpful` action
matches, and so whether stickiness has two instances to compare, is decided per
binding by the engine, so both `helpful` rows exist whenever `helpful` is
declared; `RankStep` is per action, not per action instance, because instances
depend on parameter domains.

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
  are total ([`DESIGN-divmod.md`](DESIGN-divmod.md)) and do not count. A
  `decreases` measure is evaluated without a definedness check today, so it
  uses the action-context predicate, which counts `/` and `%`.
- `NoOverflow` is syntactic: the site contains `+ - * / %`, unary `-`, `abs`
  or a `sum` aggregate -- the operations whose result the verifier checks
  against the `i64` range.
- `KeyInDomain` is typed. Every index read and indexed assignment counts unless
  the collection is a `Seq` (a `Seq` read is a `PartialDefined` site) or the
  collection is a `Map` whose key type contains the index's static type: a
  range inside the key range, the same enum, or `Bool`. A numeric literal
  index counts as the range of its value. Finite key types are exactly ranges,
  enums and `Bool` (`check` rejects `Map<Int, _>` and composite keys). Names
  are typed in the scope the evaluators use -- action parameters, `let`s,
  `requires` and `and`/`=>` pattern bindings, binder variables -- and a name
  that cannot be typed makes its indexes count.
- `Holds@TypeBound(v)` is vacuous for the types `--engine induction` asks no
  type-bound obligation for (`Int`, `Bool`, `Relation`, and composites of them).

## Decisions

- `Corresponds`, `Accepted` and `Rejected` (refinement steps, acceptance and
  forbidden traces) are not P1-a kinds. `catalog` takes only a `KernelModel`,
  which has no trace cases; they arrive with their generator and tests in P1-c.
- A row carries no `claim`, `group` or `catalog_version` field; all three are
  derived from the kind. `PartialDefined` is a required row of its property's
  claim (#1196 reports `_partial_<name>` per property). `NoOverflow`,
  `KeyInDomain` and the `Measure` rows belong to a separate model-definedness
  claim, so an unrelated overflow does not weaken an invariant's claim.
- `TotalDefined` is split into `NoOverflow` and `KeyInDomain` because engines
  support them differently: `--engine induction` asks neither today, and #1221
  is the key-domain half.
- `init` definedness has no row in P1-a.

## Verification

`rust/fsl-core/tests/obligation_catalog.rs` (T2) pins each fixture's catalog as
an exact multiset and checks that ids are unique. The fixtures are the
reproducers of #1189, #1192, #1196, #1217 and #1221, the `helpful` fixture of
#473, a fixture with every non-ranked family, and controls for each overflow and
key-domain case; every fixture except the rewritten #1192 model passes `fslc
check`. A scope override (`verify { instances / values }`) leaves the catalog
unchanged, and a property-selected model owes a subset of the full model's rows
with the kept sites' rows unchanged. Every kind and every site variant occurs in
some fixture, and removing any generator call fails at least one test. Generator coverage is calibrated
with `cargo mutants --no-config --package fsl-core --file
fsl-core/src/obligation.rs` (run from `rust/`), which must leave no surviving
mutant.
