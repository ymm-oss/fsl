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
  actor User, Shop, Courier, Fault

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
| `Order` **belongs to** `User` **as** `buyer` **via** `place` | every `Order` gets the field `buyer` (no one until `place` happens); the event `place` takes the `User` instance that does it as its first input and records it |
| **resource** `stock: Qty = 2` | one value that belongs to no entity; events `set` it |

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
| **concept** `C = E` **where** `cond` | the instances of `E` for which `cond` holds; fields are named bare (`qty`) |
| **concept** `C = E` **in** `S, T` | the instances in one of the stages |

A concept is expanded where it is used; it adds no state. Use concepts for the
cases the source names ("bulk orders"), so rules and decision rows read in the
source's words.

## Events

```fsl check

  event place   = User places Order with qty      Empty -> Placed
    set stock = stock - input qty
  event pay     = User pays Order                 in Placed
  event pack    = Shop packs Order                Placed -> Packed
  event cancel  = Shop cancels Order              Placed -> Cancelled
  event ship    = Shop ships Order                Packed -> Shipped
  event deliver = Courier delivers Order          Shipped -> Delivered
    covers REQ-ORD-007 "A shipped order is delivered by the courier"
  fault event damage = Fault damages Order        in Shipped
    set damaged = true
  event replace = Shop replaces Order             in Shipped
    set damaged = false
```

| Form | Meaning |
|---|---|
| **event** `name = Actor verb Entity A -> B` | one move along a lifecycle edge (`event_not_on_lifecycle` if it is not an edge); `name` is the canonical action name implementations see |
| … **in** `S` | an event that stays in stage `S` |
| … **with** `field` | an input stored in the field of that name; `with i: Type` is an input that is not a field; several inputs are a comma list |
| **set** `x = expr, y = expr` | other updates; `input qty` names the input where a field has the same name (a bare `qty` there is `ambiguous_input_field`) |
| **covers** `ID "text"` | the requirement this event carries |
| **fault event** | not fair: something that may happen, never something that must eventually happen |

The verb may be left out (`Shop Order`); it then is the event name. Every
lifecycle edge should have an event (`lifecycle_edge_without_event` warns
otherwise). Events other than `fault event` are fair. Scenario steps are
matched to events by actor and verb: `unknown_verb`, `unknown_event_phrase`
(with the actor's verbs) and `ambiguous_step` say why a step matched none or
several.

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
| **allow** … **only for** `Concept` | a guard: only for instances of the concept |
| **never** `Actor verb Entity` **when** `cond` / **for** `Concept` | a guard: not when `cond` holds / not for the concept |
| `P` **only after** `Q` | a hidden history field (`order_did_pay`) that `Q`'s events set and `P`'s require |
| `Name:` `E` **enters** `T` **only from** `S, …` | a transition property `Name` |
| `Name:` **always** `C1` **is** `C2` | an invariant: every instance of `C1` (a concept, or `Stage Entity`) is in `C2` |
| `Name:` **always** `E` [**not**] **in** `S, …` **has** `x` | an invariant: in (or outside) those stages the field condition holds; `has role` means the relation is set |
| **deadline** `age <= bound` | the requirement's deadline, over an `age` of the `time` block below |

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

## Decision tables

```fsl check

  decide REQ-ORD-005 "Who may close a placed order" for Placed Order {
    Bulk   => Shop packs
    Single => Shop packs or cancels
  } assuming qty > 0
```

| Form | Meaning |
|---|---|
| **decide** `ID "text"` **for** `Stage Entity` `{ rows }` | who may do what to an instance in that stage, case by case |
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
| **terminal** `{ cond }` | the states where stopping is intended. This edition derives none for requirements: without it, a run that ends with every instance in a final stage is a `deadlock` warning |
| **measure** `m = count C` / `count E in S, …` | a count of instances, a projection that adds no state (`kpi` is accepted as the same thing) |
| **goal** `ID "text"` **some** `E` **can reach** `S` | a reachability property |
| **policy** `ID "text"` **every** `E` **in** `S` **must eventually be** `T {or T}` | a progress property (`leadsTo`) over the fair events |

Check that a deadline can fail: remove `urgent ship` and `verify` reports the
deadline invariant `violated`. A deadline that no variant can break states
nothing.

## Scenarios and forbidden traces

```fsl check

  scenario AC-ORD-001 "A paid single order is delivered" {
    User places o1 with buyer 0, qty 1
    User pays o1
    Shop packs o1
    Shop ships o1
    Courier delivers o1
    then o1 is Delivered
  }
  forbidden FB-ORD-001 "An unpaid order is not packed" {
    User places o1 with buyer 0, qty 1
    Shop packs o1
    then rejected
  }
  forbidden FB-ORD-002 "A bulk order is not cancelled" {
    User places o1 with buyer 0, qty 2
    Shop cancels o1
    then rejected
  }
  forbidden FB-ORD-003 "A damaged order is not delivered" {
    User places o1 with buyer 0, qty 1
    User pays o1
    Shop packs o1
    Shop ships o1
    Fault damages o1
    Courier delivers o1
    then rejected
  }
  forbidden FB-ORD-004 "No order is placed beyond the stock" {
    User places o1 with buyer 0, qty 2
    User places o2 with buyer 1, qty 1
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
| `Actor verb instance` [**with** `input value, …`] | a step; `o1`, `o2` are instance names, numbered in order of first use (more than `verify` allows is `scenario_instance_overflow`); a relation's role is an input (`with buyer 0`) |
| **then** `o1` **is** `Stage` [**and** …] | the check after the last step |
| **verify** `{ instances E = N  values T = lo..hi }` | the finite world every command checks in; results hold for these sizes only |

`check` runs every scenario and forbidden trace. Each failure is
`result: "error"` with the trace's `id`: `kind: "acceptance"` for a scenario
(with `failed_step`), `kind: "forbidden"` for a forbidden trace whose last
step was accepted, and `kind: "forbidden_setup"` (with `failed_step`) for one
whose earlier step was already rejected — a forbidden trace must reach its
last step before it can show that the step is rejected.
