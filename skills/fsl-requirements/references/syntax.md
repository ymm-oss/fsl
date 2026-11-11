# Requirements syntax (`fsl 5`)

Every member of a requirements spec of this edition, in the order a spec is
usually written. The fences of this page are one spec, `OrderRequirements`,
and it checks, verifies to depth 8 and proves with `--engine induction` as
written. The words in **bold** in the tables are keywords; each has a Japanese
spelling in `keywords.md`. Names, IDs and quoted text are yours.

## Header and spec

The first line of the file is `fsl 5` (before it only blank lines and `//`
comments). Without it the file is read as the earlier edition. A Markdown
file takes the header as the first line of its first `fsl` fence.

```fsl check
fsl 5
requirements OrderRequirements {
```

| Form | Meaning |
|---|---|
| `fsl 5` | the edition of this file; `fslc migrate --edition 5` inserts it and rewrites the earlier forms |
| **requirements** `Name` `{ … }` | the spec; `Name` is canonical (ASCII) |
| **glossary** `{ … }` | optional, the first item: aliases for canonical names (`glossary.md`) |
| **implements** `Business` `from "file.fsl"` `{ … }` | optional: the seam to the business layer, written in canonical lowered names (`../SKILL.md`, "The seam to a business layer") |

Inside the spec, the edition also holds the earlier stricter rules as errors:
`check` answers `result: "error"`, `kind: "unsupported_in_edition"` with one
entry per finding in `findings[]` (for example `legacy_string_metadata` for a
`"REQ-1: text"` string), each with a `canonical_replacement`.
`fslc migrate --edition 5 file.fsl` plans those edits; `--write` applies them.

## Vocabulary

```fsl check
  number Qty
  const BULK = 2
  const SLA = 2
  actor User, Shop, Courier, Weather

  entity User

  entity Order {
    qty: Qty = 0
    damaged: Bool = false
    lifecycle Empty -> Placed -> Packed -> Shipped -> Delivered
    lifecycle Placed -> Cancelled
  }
  Order belongs to User as buyer via place

  resource stock: Qty = 2
```

| Form | Meaning |
|---|---|
| **number** `Qty` | a bounded integer domain; its range is set in `verify { values Qty = lo..hi }` |
| **const** `BULK = 2` | a named constant |
| **enum** `Channel { Web, Store }` | a finite set of values |
| **actor** `A, B` | who does things; every event and step names a declared actor (`actor_not_declared`) |
| **entity** `User` | an identity with no lifecycle; the number of instances is set by `verify { instances User = N }` |
| **entity** `Order { field: Type = init … }` | a field of every instance; every field needs an initial value |
| **lifecycle** `A -> B -> C` | the stages and the moves between them; one line per path, the first stage is the initial one. The stages become an enum **in the order they first appear**, which an upper layer's stage list must repeat |
| `Order` **belongs to** `User` **as** `buyer` **via** `place` | every `Order` gets the field `buyer` (no one until `place` happens); the event `place` takes the `User` instance that does it as its first input and records it. An event makes at most one relation: a second `Order belongs to User as … via place` is refused (`event_already_makes_relation`); give it its own event |
| **resource** `stock: Qty = 2` | one value that belongs to no entity; events `set` it |

Each name and each requirement ID is declared once. A second `actor` of
one name is refused (`duplicate actor`). An ID belongs to the one `rule`,
`decide`, `scenario`, `forbidden`, `goal` or `policy` written with it, or to a
current-edition `requirement` or `acceptance` block, so a second declaration
of any of these kinds with the same ID is refused at its ID (`duplicate
requirement id`, naming both kinds). A `covers` line is a link, not an owner.
A `covers` of an ID another declaration owns may have any text: the ledger
row and the documents title the ID with the owner's title, and the `covers`
text follows in the documents as a linked statement. The one exception is a
row covering its own table's ID: with another text it is refused as a
conflict. An ID no declaration owns may be covered by any number of events
and rows, in one table or several, but every one of those `covers` must have
the same text; the first that differs is refused. Each was accepted before #1705, and the ledger kept
one row for the two.

The lowering names each field `entity_field` (`order_qty`,
`order_stage`, `order_buyer`); `fslc explain --readable` lists them under
"State". Those are the names a design mapping and an `implements` block use.

## Concepts

```fsl check

  concept Bulk   = Order where qty >= BULK
  concept Single = Order where qty < BULK
  concept Open   = Order in Placed, Packed
  concept Intact = Order where not damaged
```

| Form | Meaning |
|---|---|
| **concept** `C = E` **where** `cond` | the instances of `E` for which `cond` holds; fields are named bare (`qty`), in every expression form, and are never an event input of the same name; a field cannot be a quantifier range bound (#1798, breaking removal) |
| **concept** `C = E` **in** `S, T` | the instances in one of the stages |

A concept is expanded where it is used; it adds no state. Use concepts for the
cases the source names ("bulk orders"), so rules and decision rows read in the
source's words.

## Events

```fsl check

  event place   = User places Order with qty (Empty -> Placed)
    set stock = stock - input qty
  event pay     = User pays Order in Placed
  event pack    = Shop packs Order (Placed -> Packed)
  event cancel  = Shop cancels Order (Placed -> Cancelled)
  event ship    = Shop ships Order (Packed -> Shipped)
  event deliver = Courier delivers Order (Shipped -> Delivered)
    covers REQ-ORD-007 "A shipped order is delivered by the courier"
  fault event damage = Weather damages Order in Shipped
    set damaged = true
  event replace = Shop replaces Order in Shipped
    set damaged = false
```

| Form | Meaning |
|---|---|
| **event** `name = Actor verb Entity (A -> B)` | one move along a lifecycle edge (`event_not_on_lifecycle` if it is not an edge); `name` is the canonical action name implementations see |
| … `A -> B` | the same move without parentheses |
| … **in** `S` | an event that stays in stage `S`; a stay takes no parentheses |
| … **with** `field` | an input stored in the field of that name; `with i: Type` is an input that is not a field; several inputs are a comma list |
| **set** `x = expr, y = expr` | other updates; `input qty` names the input where a field has the same name (a bare `qty` there is `ambiguous_input_field`; `input qty` inside a binder named `qty` is `captured_by_binder`) |
| **covers** `ID "text"` | the requirement this event carries |
| **fault event** | not fair: something that may happen, never something that must eventually happen. The keyword makes it so, not the actor's name (`Weather`) |

The move comes after the phrase and its inputs. In parentheses it reads as a
note on the sentence, which ends with its verb in Japanese:
`出来事 pack = 店舗 が 注文 を 梱包する (受注 -> 梱包済み)`. The bare form
is equally accepted: `event pack = Shop packs Order Placed -> Packed` is the
same event, and both lower to the same model. `fmt` keeps the form you wrote.
The parentheses hold one move only: `(in Placed)` and `(Placed)` are refused
(`kind: "parse"`), and full-width `（ ）` is refused as `fullwidth_symbol`.

The verb may be left out (`Shop Order`); it then is the event name. Every
lifecycle edge should have an event (`lifecycle_edge_without_event` warns
otherwise). Events other than `fault event` are fair. Scenario steps are
matched to events by actor and verb: `unknown_verb`, `unknown_event_phrase`
(with the actor's verbs) and `ambiguous_step` say why a step matched none or
several.

Three of these readings come from a name match, a word count or line order,
so an edit can change them silently: renaming `with qty` (or the field) drops
the store, reordering `lifecycle` lines changes the initial stage and the stage
order, and `Shop Order` makes the event name the verb. `fslc explain` shows
how each was read (`readings` in JSON; `Lifecycles:`, `verb:`, `fair:`,
`input …:` and `guards:` lines with `--readable`), and `fslc lint` notes each
as info in `files[].notes[]`: `input_stored_by_name`, `verb_from_event_name`,
`initial_stage_from_line_order` (only with more than one lifecycle line). A
note is not a finding and never changes `lint`'s exit code; keep the form when
it is what the source means.

## Rules

```fsl check

  rule REQ-ORD-001 "An order is placed for a quantity in stock" {
    allow User places Order only when input qty > 0 and stock >= input qty
  }
  rule REQ-ORD-002 "An order is packed only after it is paid" {
    Shop packs Order only after User pays Order
  }
  rule REQ-ORD-003 "A damaged order is not delivered" {
    never Courier delivers Order when damaged
  }
  rule REQ-ORD-004 "A placed order has a buyer and items" {
    BuyerKnown: always Order not in Empty has buyer
    ItemsKnown: always Order not in Empty has qty > 0
  }
  rule REQ-ORD-006 "A delivered order arrives intact and only from shipping" {
    DeliveredIntact: always Delivered Order is Intact
    DeliveredFromShipping: Order enters Delivered only from Shipped
  }
  rule REQ-ORD-008 "A packed order ships within SLA ticks" {
    deadline packed_age <= SLA
  }
```

A rule is one source requirement: its ID, its verbatim text, and one or more
statements. Each statement is one of these, and they compose by conjunction
in any order:

| Statement | Lowers to |
|---|---|
| **allow** `Actor verb Entity` **only when** `cond` | a guard on the event: it may happen only when `cond` holds |
| **allow** … **only for** `Concept` | a guard: only for instances of the concept; terms join with **and**, and a term may name one stage, `Stage Entity` or `Entity` **in** `Stage` (the same guard) |
| **never** `Actor verb Entity` **when** `cond` / **for** `Concept` | a guard: not when `cond` holds / not for the concept (terms as after **only for**) |
| `P` **only after** `Q` | a hidden history field (`order_did_pay`) that `Q`'s events set and `P`'s require |
| `Name:` `E` **enters** `T` **only from** `S, …` | a transition property `Name` |
| `Name:` **always** `C1` **is** `C2` | an invariant: every instance of `C1` (a concept, or `Stage Entity`) is in `C2` |
| `Name:` **always** `E` [**not**] **in** `S, …` **has** `x` | an invariant: in (or outside) those stages the field condition holds; `has role` means the relation is set |
| **deadline** `age <= bound` | the requirement's deadline, over an `age` of the `time` block below |

- The history field of `only after` is named after `Q`'s events, joined by
  `_or_` (`order_did_pay`, `order_did_pay_or_ship`). When two prerequisite
  sets of one entity would get the same name (the events `a` and `b`
  together, and a single event named `a_or_b`, both give `did_a_or_b`), each
  gets a numbered suffix: `order_did_a_or_b_1`, `order_did_a_or_b_2`.
  The number depends on the entity's other history fields and declared
  fields, not on rule order, so adding a rule or a field elsewhere can
  renumber it, and the field names in counterexamples change with it.
- An `allow` / `never` / `only after` statement rejects calls. Give it a
  `forbidden` trace that tries one (`never_rule_without_forbidden` warns when
  no forbidden trace ends in an event the rule guards).
- `check` proves over the field domains that the guards of an event can hold
  together (`rule_conflict_unsatisfiable_guard` names the rules that cannot).
- A statement the lifecycle already gives is reported as
  `implied_by_lifecycle` (info), as `DeliveredFromShipping` is here. Keep it
  when the source states it; it is then still checked.
- Name every `always` / `enters` statement: the name is the property's name in
  results and diagnostics (otherwise a generated one).
- After `for`, `Order in Placed` and `Placed Order` are one term (`対象 Order 段階
  Placed` in Japanese keywords); `fmt` keeps the order written. Either names one
  stage: for several, declare a concept (`concept Open = Order in Placed, Packed`).
  The `in` stands on the entity's line, as the entity of `Placed Order` stands on
  the stage's: a statement starts on a new line (the stage after `in` may
  wrap, as a term after `and` may). `always` keeps its forms,
  `always Placed Order is …` and `always Order in Placed has …`.

## Decision tables

```fsl check

  decide REQ-ORD-005 "Who may close a placed order" for Placed Order {
    Bulk   => Shop packs
    Single => Shop packs or cancels
  } assuming qty > 0
```

| Form | Meaning |
|---|---|
| **decide** `ID "text"` **for** `Stage Entity` `{ rows }` | who may do what to an instance in that stage, case by case; **for** `Entity` **in** `Stage` is the same table |
| `Concept {and Concept} => Actor verb {or verb}` | a row: in this case, this actor may take these events |
| `Stage Entity => …` | a row by stage |
| … **covers** `ID "text"` (on the next line) | the requirement this row carries |
| **otherwise** `=> Actor verb …` | the case no other row holds |
| `}` **assuming** `cond` | the table is checked only where `cond` holds, and `cond` becomes an invariant at that stage that `verify` proves |

What `check` enforces:

- every event that leaves the stage is named by a row (`decide_event_not_listed`);
- the rows cover every value of the fields they read (`decide_not_exhaustive`,
  with the first uncovered values, such as `qty = 0`);
- no two rows hold together (`decide_overlap`, with the shared values);
- a row that holds for no value is a warning (`decide_row_emptied`).

Here `qty = 0` is covered by no row, but no placed order has it (REQ-ORD-001),
so `assuming qty > 0` closes the gap and `verify` proves
`_decide_REQ_ORD_005_assuming`. Each event of the table gets one `requires`:
the disjunction of the rows that name it.

## Time, terminal states, measures, goals and policies

```fsl check

  time {
    urgent ship
    age packed_age[o: Order] while stage(o) == Packed
  }

  terminal { forall o: Order { stage(o) == Empty or stage(o) == Delivered or stage(o) == Cancelled } }

  measure open_orders      = count Open
  measure delivered_orders = count Order in Delivered

  goal GOAL-ORD-001 "An order can be delivered" some Order can reach Delivered
  policy POL-ORD-001 "Every placed order is eventually packed or cancelled"
    every Order in Placed must eventually be Packed or Cancelled
```

| Form | Meaning |
|---|---|
| **time** `{ urgent e  age a[o: E] while cond }` | kernel time: `a` counts ticks while `cond` holds; time does not pass while an `urgent` event is enabled. `stage(o)` is the instance's stage |
| **terminal** `{ cond }` | the states where stopping is intended. Without it, this edition derives one for requirements: every instance in a stage with no outgoing lifecycle edge. An explicit `terminal` replaces that condition; it is not added to it |
| **measure** `m = count C` / `count E in S, …` | a count of instances, a projection that adds no state (`kpi` is accepted as the same thing) |
| **goal** `ID "text"` **some** `E` **can reach** `S` | a reachability property |
| **policy** `ID "text"` **every** `E` **in** `S` **must eventually be** `T {or T}` | a progress property (`leadsTo`) over the fair events |

Check that a deadline can fail: remove `urgent ship` and `verify` reports the
deadline invariant `violated`. A deadline that no variant can break states
nothing.

## Scenarios and forbidden traces

```fsl check

  scenario AC-ORD-001 "A paid single order is delivered" {
    User places o1 with buyer u1, qty 1
    User pays o1
    Shop packs o1
    Shop ships o1
    Courier delivers o1
    then o1 is Delivered
  }
  scenario AC-ORD-002 "A cancelled order keeps its quantity" {
    User places o1 with buyer u1, qty 1
    Shop cancels o1
    then stage(o1) == Cancelled and o1.qty == 1
  }
  forbidden FB-ORD-001 "An unpaid order is not packed" {
    User places o1 with buyer u1, qty 1
    Shop packs o1
    then rejected
  }
  forbidden FB-ORD-002 "A bulk order is not cancelled" {
    User places o1 with buyer u1, qty 2
    Shop cancels o1
    then rejected
  }
  forbidden FB-ORD-003 "A damaged order is not delivered" {
    User places o1 with buyer u1, qty 1
    User pays o1
    Shop packs o1
    Shop ships o1
    Weather damages o1
    Courier delivers o1
    then rejected
  }
  forbidden FB-ORD-004 "No order is placed beyond the stock" {
    User places o1 with buyer u1, qty 2
    User places o2 with buyer u2, qty 1
    then rejected
  }
}
verify {
  instances Order = 2
  instances User = 2
  values Qty = 0..2
}
```

| Form | Meaning |
|---|---|
| **scenario** `ID "text"` `{ steps  then check }` | a positive trace: every step must be enabled and the check must hold |
| **forbidden** `ID "text"` `{ steps  then rejected }` | a negative trace: the steps before the last are enabled, and the last is rejected |
| `Actor verb instance` [**with** `input value, …`] | a step; `o1`, `o2` are instance names. A relation's role is an input whose value names an instance of the related entity (`with buyer u1`) or gives its number (`with buyer 0`). Names are numbered from 0 per entity in order of first use, a step's instance before its role value (more than `verify` allows is `scenario_instance_overflow`); a name the spec declares as a constant, resource, enum member or stage keeps that meaning, and a name of another entity's instance is refused. Only names are counted: after `with buyer 0`, the first User name is 0 too. `Actor instance verb` is the same step (`User o1 places with buyer u1, qty 1`): of the two words, the verb is the first that is some event's verb |
| **then** `o1` **is** `Stage` | the check after the last step: the stage of one instance. Nothing may follow it: `then o1 is Delivered and …` is refused (`kind: "parse"`) |
| **then** `expression` | any other check after the last step, as one expression joined with `and` (`AC-ORD-002`). `o1.qty` is a field of `o1` and `stage(o1)` (or `Order.stage(o1)`) its stage: the lowered state at `o1`'s number (`order_qty[0]`, which may also be written). A name is read only there, not as a bare value (`o1.buyer == some(0)`); a binder, an `is some(o1)` pattern anywhere in the check, or a declared constant, resource, enum member or stage of the same name keeps its meaning |
| **verify** `{ instances E = N  values T = lo..hi }` | the finite world every command checks in; results hold for these sizes only |

`check` runs every scenario and forbidden trace. Each failure is
`result: "error"` with the trace's `id`: `kind: "acceptance"` for a scenario
(with `failed_step`), `kind: "forbidden"` for a forbidden trace whose last
step was accepted, and `kind: "forbidden_setup"` (with `failed_step`) for one
whose earlier step was already rejected — a forbidden trace must reach its
last step before it can show that the step is rejected.
