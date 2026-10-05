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
authored site and aspect. It is a pure function of the model: no solver and no
property selection. A verification scope (`verify { instances / values }`,
`--values`) adds or removes no row, but it resizes the model's types, and
static vacuity compares resolved types: a `Map` index of one type into a key of
another can be in domain at one size and not at another. It lives in
`fsl-core`, the owner of the semantic model, so every crate that may read it
-- `fsl-verifier`, `fsl-runtime`, `fsl-tools` -- already depends on it
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
| state variable `v` | `Holds@TypeBound(v)` |
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

A row is `statically_vacuous` when its site holds no candidate for it.
Candidates over-approximate: a row no state can fail may keep a candidate,
never the reverse, so a vacuous row is one no evaluation of its site can fail
and an engine that skips it skips no real question. `KeyInDomain` and the value
types it uses carry one premise: every state variable the site reads satisfies
its type bound. Each engine checks a state's type bounds before it evaluates
a site on that state (explicit, bmc and induction all report `_bounds_<v>`
first when an out-of-type value reaches an index in an invariant, a `trans`,
an `ensures`, a guard or a body), so the premise holds wherever the
`Holds@TypeBound` rows are checked. An indexed assignment writes the map the
state held before the action: `check` rejects an action that assigns the same
state location twice, so no earlier statement can replace that map. A run that does not check them (`--property` selects a property
and drops the bounds) cannot rely on a key row's vacuity: there an out-of-type
value reaches the index and explicit reports a key-domain miss. The P1-c fold
must keep that order.

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
- `KeyInDomain` is typed by values, not by `check`. Every index read and
  indexed assignment counts unless the collection is a `Seq` (a `Seq` read is
  a `PartialDefined` site) or a `Map` whose key type holds every value of the
  index: a range inside the key range, the same enum, or `Bool`. Finite key
  types are exactly ranges, enums and `Bool` (`check` rejects `Map<Int, _>`
  and composite keys). `check` types some forms from one operand -- a
  conditional from its `then` branch, `s.add(e)` and `q.push(e)` from the
  receiver -- so its type is no bound. An index is bounded only by an allowlist
  of forms: a numeric literal (the range of its value), `true`/`false`, a name
  typed in scope, an enum member, a struct field, a `Map` or `Seq` element
  (`m[i]`, `head`, `at`), `old` of one of these, and a conditional whose
  branches join (the hull of two ranges, or one type). Every other form --
  arithmetic, `abs`, `size`, `add`, `push` -- counts, even where its value
  happens to fit.
- A name's type holds every value the evaluators can bind to it. Action
  parameters come first, then each `let` in clause order; a binder variable
  is typed inside its `where` filter and body, and a binder over a collection
  takes the item type of a bounded collection. An `is some(v)` pattern never
  rebinds a parameter, `let` or binder (`or_insert` in both evaluators), but
  once it matches it can bind `v` for what is evaluated after it in the same
  context (an action's guard, body and `ensures` are one; each property
  expression is one, and a `leadsTo` trigger and goal are evaluated from
  separate copies of the binder bindings), whether or not the path there
  required the match. The catalog also lets a pattern inside a binder's scope
  reach the rest of the context, which the evaluators do not: an
  over-approximation. So outside a parameter, `let` or binder, `v`
  is typed as the join of every payload of its patterns in the context and its
  base meaning (a state variable, constant or enum member); without a join it
  is untyped, and every index through an untyped name counts.
- `Holds@TypeBound(v)` is vacuous only for `Int`, `Bool` and `Option`s of
  them, which the type-bound check (`value_conforms` in `fsl-runtime`) accepts
  whatever they hold. Every other type keeps a live row, including the two
  `check` does not protect: a `Map` assigned a map over a narrower key range
  breaks its key-set bound, and a relation's `add` admits an out-of-type pair
  (explicit and bmc report `_bounds_<v>` for both, and induction for the
  relation).

## Decisions

- `Corresponds`, `Accepted` and `Rejected` (refinement steps, acceptance and
  forbidden traces) are not P1-a kinds. `catalog` takes only a `KernelModel`,
  which has no trace cases; they arrive with their generator and tests in P1-c.
- A row carries no `claim`, `group` or `catalog_version` field; they are
  derived from the kind and the site variant, since the kind alone does not
  decide the claim. `PartialDefined` is a required row of its property's
  claim (#1196 reports `_partial_<name>` per property), except at a `Measure`.
  `NoOverflow`, `KeyInDomain` and the three `Measure` definedness rows belong
  to a separate model-definedness claim, so an unrelated overflow does not
  weaken an invariant's claim.
- An invariant owes one `Holds@Invariant` row, not a base row and a step row
  (#1202 names both for `--engine induction`). Base and step are how one
  engine discharges that row; the catalog lists what the model owes, and how
  each engine covers a row is the per-engine support table of P1-b.
- `TotalDefined` is split into `NoOverflow` and `KeyInDomain` because engines
  support them differently: `--engine induction` asks neither today, and #1221
  is the key-domain half.
- `init` definedness has no row in P1-a.

## Verification

`rust/fsl-core/tests/obligation_catalog.rs` (T2) pins each fixture's catalog as
an exact multiset and checks that ids are unique; `catalog` also
`debug_assert`s unique ids, and an exhaustive match whose arms look each kind
up in `ObligationKind::ALL` at compile time keeps `ALL` complete (a kind
missing from it fails `cargo check`). The fixtures are:

- the reproducers of #1189, #1192, #1196, #1217 and #1221, the `helpful`
  fixture of #473, and a fixture with every non-ranked family;
- controls for each overflow and key-domain case, and one site per bounded
  index form against the forms left unbounded;
- the false-vacuity reproducers of the first independent review, each of which
  `fslc verify --engine explicit` and `--engine bmc` fail, with in-domain
  controls (a conditional of two members, a fresh pattern name, a parameter a
  pattern cannot rebind) that stay vacuous, and two pattern bindings only the
  join over a whole context types soundly (also failing in both engines);
- a template that puts a live key candidate (`m[i]`), a live overflow
  candidate (`x + 1`) and a control (`0`) in every operand position the walk
  recurses through -- each `Expr`, `Statement`, `Binder` and `LValue` operand
  and each of an action's guard, body and `ensures` -- and requires the live
  rows to be exactly the candidate's sites. An `EnumMember` index, a
  predicate call and a stage access, which no built model holds, are written
  into a built model by hand.

Every fixture except the rewritten #1192 model passes `fslc check`. A scope
override adds or removes no row and, where every index has its key's own type,
changes none (`a_scope_override_keeps_every_row`); an index of another type
can turn live at a larger size (`a_scope_override_can_make_a_key_row_live`). A
property-selected model's catalog drops the unselected sites' rows, so the
P1-c ledger must call `catalog` on the full model. Every kind and every site
variant occurs in some fixture.

The tests are calibrated against faults injected by hand, one at a time:
deleting each `push_*` call, each row a generator pushes, each candidate
predicate, each recursion of the walk and each value-typing rule fails at
least one T2 test. `cargo mutants --no-config --package fsl-core --file
fsl-core/src/obligation.rs` (run from `rust/`) must leave no surviving
mutant; it mutates operators and bodies but deletes no call, which is why the
injected faults are needed.
