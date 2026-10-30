# Shared syntax (`fsl 5`)

Contents: [structure](#1-top-level-structure), [types](#2-types),
[expressions](#3-expression-catalog), [statements](#4-statements-init--action-body),
[semantics](#5-semantic-rules),
[automatic checks](#6-automatic-checks-checked-even-if-not-written),
[compose/refinement](#compose-and-refinement), [idioms](#8-idioms).
Cross-dialect grammar is in [advanced](advanced.md); business/requirements syntax
belongs to the [layer owners](layers.md). Fragments below are grammar or partial
declarations, not independent programs. The core [Cart](../SKILL.md#minimal-kernel-example)
is a complete checked compilation unit.

## 1. Top-level structure

Put `fsl 5` before the dialect and its annotations (blank lines/comments may
precede it). One dialect per document, optionally a sibling `verify` block.
Native dispatch uses a shared lexer/registry; empty/unknown dialects report
`FSL-DIALECT-EMPTY` / `FSL-DIALECT-UNKNOWN`.

```fsl-fragment
fsl 5
@requirement("REQ-SCOPE-001", "document intent")
spec <Name> {
  const <NAME> = <constant integer expression>
  type <Domain> = <lo>..<hi>
  symmetric type <Identity> = <lo>..<hi>
  enum <Enum> { <Member>, ... }
  symmetric enum <Enum> { <Member>, ... }
  struct <Struct> { <field>: <Type>, ... }
  def <predicate>(<p>: <Type>, ...) = <expression>
  state { <name>: <Type> [= <deterministic initializer>], ... }
  init { <statements> }
  [fair] action <name>(<p>: <Type>, ...) {
    requires <Bool expression>
    let <local> = <expression>
    <statements>
    ensures <Bool expression>
  }
  invariant <Name> { <Bool expression> }
  trans <Name> { <Bool expression using old(...)> }
  reachable <Name> { <Bool expression> }
  leadsTo <Name> { P ~> [within K] Q [helpful act(args)] [decreases <integer>] }
  unless <Name> { P unless Q }
  until <Name> { P until Q }
  terminal { <Bool expression> }
}
```

Action parameters must be finitely enumerable: bounded domain, enum or Bool,
not builtin Int. Inline `action f(p in lo..hi) { … }` is also supported.
`fair` is weak fairness. A leadsTo can have outer unfiltered typed/range
`forall` wrappers; collection domains and `where` on those wrappers reject.
Collection quantifiers inside P/Q remain distinct and supported.
With an outer response wrapper, `helpful` and `decreases` are siblings of the
whole wrapper inside leadsTo, **outside** its forall braces; see the
[ranked-response recipe](#ranked-response-recipe).

### Annotations and reusable predicates

Use `@requirement(id, text?)`, `@undecided(reason)`, `@kind(id, text?)`, or
custom multi-segment namespaces such as `@doc.rationale("reason")` immediately
before supported declarations. Stacking and intervening comments are allowed.
Document annotations attach to the document; dangling annotations report
`FSL-ANNOTATION-TARGET`. Strings stop at a quote/newline; no escape syntax.

Annotations attach to kernel init/actions/properties, business transitions,
requirements declarations, and compose members. Native domain command/decide/
evolve/invariant/projection/effect/saga-step, AI tool/authority/fallback/check,
and DB migration/compatibility-rule declarations also support them. Lowered
domain command/decide/evolve annotations union onto their action; effect/step
annotations broadcast to generated actions. Comments are trivia, not structured
provenance. Generic kind/custom annotations can be queried in-process through
`KernelModel::annotations_for`; do not assume current JSON/LSP/ledger projects them.
Requirement relations and lowering origins are different relations.

`def` is file-local, non-recursive, arity-checked frontend substitution. Put
human requirement tags on the surrounding checked declaration. Caller params,
lets, binders and exported patterns must not capture free names in its body;
rename the caller binding if expansion reports `would capture variable`.
Declaration-level leadsTo binders protect P/Q/helpful/decreases/within too.

Stricter edition findings reject legacy declaration-string metadata, colon/no-
braces quantifiers and inline action maps; prefer typed tags, braced quantifiers
and explicit seam correspondences. Use `migrate --edition 5` to inspect edits,
then owner-approved `--write`; it refuses unsafe/incomplete rewrites. Invalid
expression `||` / `->` means `or` / `=>`, not a blanket rewrite of structural
compose/arrows or the domain expression language.

## 2. Types

| Type | Form / restriction |
|---|---|
| Scalar | `Int`, `Bool`, range type, enum |
| Bounded numeric | `type Qty = 0..5`; inline state `qty: 0..5` also works |
| Identity | `entity Item`, size in `verify { instances Item = N }` |
| Numeric modeling domain | `number Amount`, range in `verify { values Amount = lo..hi }` |
| Enum | `enum Phase { Draft, Done }`; nonempty, members bare |
| Struct | scalar or nested-Option-of-scalar fields only |
| Option | `Option<T>` where T is scalar or another such Option |
| Map | bounded scalar keys; values scalar, nested Option, or supported struct |
| Set | bounded scalar elements |
| Seq | scalar elements and positive constant capacity: `Seq<Job, CAP>` |
| Relation | bounded scalar endpoints: `relation User -> Role` |

Business has no `number`; identity/numeric kinds are also supported in kernel
specs. Both `symmetric type` and `symmetric enum` assert **interchangeable
identities**: model behavior and properties must be equivariant under a single
global renaming of those values. Liveness reduction constrains the lasso loop
head or deadlocked/stalled representative state, not every intermediate state;
do not mark a semantically distinguished identity symmetric merely for speed.
Do not use empty enums for vacuity. Nested structs, nested Maps,
Map/Set/Seq/relation as Map values, and unbounded Map keys are not state types.
Storage restrictions do not imply every composite expression literal is supported
or unsupported: check the actual expression path; do not infer engine coverage
from parsing alone.

Whole Set/Seq/Map/relation values require equal corresponding finite carriers;
Seq capacity must agree and both Map keys/values must agree. Equal-range numeric
aliases remain compatible. Widening a whole collection is not scalar coercion.
Nominal enums do not coerce to ordinals. Int has no finite carrier; do not infer
finite equality rules for inferred Int temporaries.

Inline state initializers normalize to root init assignments. They may use
constants, constructors and deterministic literals, not state reads or another
initializer. The same root cannot also be initialized in init. A relation's
literal initializer is empty `Set {}`; add pairs with reassignment.

## 3. Expression catalog

`true`, `false`, `none` are reserved literals and cannot name declarations or
binders. Other words are contextual. Avoid `__`, reserved for generated names;
the reservation covers requirements relation roles/resources/measures, IDs and
glossary aliases too. Rule-property labels use their own located name diagnostic.
Do not shadow constants with executable locals or binders (including implicit
entity/relation instance names); rename the local/constant.

| Operation | Syntax / limit |
|---|---|
| Arithmetic | `+ - * / %`, unary `-`, `min(a,b)`, `max(a,b)`, `abs(a)` |
| Logic/comparison | `and or not =>`, `== != < <= > >=` |
| Conditional | `if condition then value else value`; Bool condition, compatible arms |
| Finite binder | `x: T`, `x in lo..hi`, `x in set_or_seq`, optional `where BoolExpr` |
| Quantifier | `forall binder { P }`, `exists binder { P }` |
| Aggregate | `count(binder)`, `sum(binder of integerValue)`; empty domain → 0 |
| Cardinality | `unique(binder)` = at most one; `exactlyOne(binder)` = one |
| Option | `none`, `some(e)`, structural `==`/`!=`, `x is some(v)` / `x is none` |
| Struct | `S { field: value, ... }`, `s.field`, field-wise equality |
| Set | `Set {}`, `Set { 1, 2 }`, `.add(e)`, `.remove(e)`, `.contains(e)`, `.size()` |
| Seq | `Seq {}`, `Seq { 1, 2 }`, `.push(e)`, `.pop()`, `.head()`, `.at(i)`, `.contains(e)`, `.size()` |
| Relation | `.contains(a,b)`, `.add(a,b)`, `.remove(a,b)` |
| Graph | `reachable(r,a,b)`, `acyclic(r)`, `functional(r)`, `injective(r)`, `domain(r)`, `range(r)` |
| Two-state | `old(expr)` only in ensures/trans |

Write division with spaces: `a//b` starts a comment. Integer operations require
integer-compatible operands, not Bool/enum ordinals. Both conditional arms,
filters, aggregate values and empty-domain expressions are statically checked;
only a selected conditional arm evaluates. Maps have indexing, not methods.
Method arities are exact. Relation endpoints must match their declared sides;
`reachable`/`acyclic` need identical declared endpoints (`T -> T`), not merely
equal numeric ranges. A path to itself requires ≥1 real edges, not zero-hop
reflexivity. Relations are not indexable Maps, even when functional.
`domain`/`range` return endpoint-typed Sets, never an automatic `Set<Int>`.

Seq binders count duplicate live slots; Set binders visit distinct members.
Computed outside-carrier additions remain visible in temporaries; storing them
back into bounded state exposes a bound violation, not silent truncation.
Keep supported materialization and literal limits separate from type acceptance.

### Option payload scope

Equality binds no name. `o is some(v)` binds a **fresh** payload only where the
match is true: a guarded RHS of `and` or `=>`, and conjunctive requires into
later guards/body. Only `and` exports a binding from a true whole expression.
No export from `or`, comparison results, negation, quantifiers/aggregates, or
expression-if into later clauses. Do not read a condition's binding in an
expression-if arm. Names may remain reserved even where not visible (including
not/if/where and later statements); rename instead of rebinding. Opposite
statement-if arms remain independent. Do not infer general composed-init
binding lifetime guarantees from direct-spec checking.

For “allow none, but payload must be 1 when present”, write
`(o is some(v)) => v == 1` or `o == none or (o is some(v) and v == 1)`.
`(o is some(v)) or v == 1` is undefined, not an equivalent shorthand.

## 4. Statements (init / action body)

- Assign root, index, field or indexed field: `x = e`, `m[k] = e`, `s.f = e`,
  `m[k].f = e`. Methods return new collections: `q = q.pop().push(y)`.
- `if condition { statements } else { statements }`, nesting allowed.
- `forall binder { statements }` for bulk assignment; source/bounds use outer
  scope and filter/body use the local binder scope.
- Action RHS reads the pre-state simultaneously, including locals derived from
  it. No sequential mutation model is implied by textual order.

Init must respect ownership and read order. Disjoint struct/map-entry fields
can be initialized separately when coverage is complete; overlapping path
writes reject. One forall may repeat a location with agreeing values; conflicting
values make init unsatisfiable. Unresolved/dynamic/filtered/collection keys own
the whole root and conflict with other statements writing it.

Read a may-assigned root only after full definite coverage, including all keys
and fields; having written one selected field is insufficient. Never-assigned
roots retain symbolic free-value semantics in BMC/induction, but Monitor/explicit
cannot construct those nondeterministic initial states. Refine also refuses
incomplete assigned roots rather than defaulting missing components.

An init forall cannot read a root the same forall writes. Exception: its own
current binder-key location already written in that iteration. After
`m[k] = value`, `seen[k] = m[k]` is allowed; another key or a read before the
write is not. Field writes allow that field only after required sibling coverage;
whole-entry reads are not that exception. Nested loops and prior partial loops
do not imply a general per-key dependency proof. Init collection sources cannot
read state, even assigned state or an unreachable branch's state.

## 5. Semantic rules

- Duplicate writes on one action path reject; if branches are different paths.
  Bulk-write distinctness must be provable; conservative failures name
  `FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED` and may offer a safe hint.
- Declare all types and variables. Static type checking includes all clauses,
  both conditional arms, trace inputs/expectations and empty aggregate domains.
- A kernel `spec` has kernel properties, not requirements scenario/forbidden
  declarations. Use the [requirements owner](../../fsl-requirements/references/syntax.md)
  for requirements grammar and replay-checked positive/negative traces.
- `within K` is a nonnegative constant response deadline checked by BMC, distinct
  from an unbounded eventual-response ranking proof; adding `decreases` does
  not make the K-step deadline an unbounded induction guarantee. See the recipe below.
- Never weaken a property, domain or trace to hide a counterexample. Decide
  whether intent, model, mapping or code is wrong before repairing.

### Ranked-response recipe

For a confirmed per-case response, the partial declaration shape is
`leadsTo Responds { forall c: Case { level[c] > 0 ~> level[c] == 0 }`
followed by `helpful step(c)` and `decreases level[c]`, then the final leadsTo
closing brace. Both clauses belong **after** the forall's closing brace;
putting them inside the response wrapper is not ranked-forall syntax.

- `check` type-checks helpful arguments in the property's binder scope.
  **Induction additionally requires state-independent arguments:** leadsTo
  binders, constants, enum members, integer/Boolean literals, and integer
  `+`, `-`, `*` combinations (including unary `-`). A well-typed state read
  such as `helpful step(cursor)` can pass check/BMC yet be refused by induction.
- Under proved invariants while P is pending and Q false, the integer rank must
  be nonnegative and progress possible. Without helpful, **every** enabled action
  must reach Q or preserve P and strictly decrease the rank.
- Under interleaving, another case's valid step can leave `level[c]` unchanged.
  Use a matching per-binding `helpful step(c)`: it must be a `fair action`,
  enabled whenever that binding is pending, and strictly decrease its rank when
  it fires. Other instances/actions must preserve the pending obligation unless
  they reach Q, and must **not increase** the rank. Helpful creates no fairness.
- With two or more distinct helpful action names, once a helpful instance is
  enabled while pending, it must stay enabled until firing or Q holds; an
  alternating “some helper is enabled” condition does not satisfy weak fairness
  for any one instance.
- Alternatively use an appropriate global rank such as
  `decreases sum(k: Case of level[k])` when every enabled action reaches Q or
  preserves the pending obligation and decreases the total. This avoids a
  fixed-instance-count sum; it does not waive the progress/decrease obligations.

Without decreases, response evidence remains bounded to depth. A successful rank
proves eventual response at the checked world sizes; the base, coverage and
`within K` deadline retain their own bounded obligations. For an expensive search
use [scoped liveness exploration](commands.md#scoped-liveness-exploration), then
restore full-world safety and unfiltered completion rather than shrinking the claim.

## 6. Automatic checks (checked even if not written)

Type bounds (`_bounds_*`, including Maps, struct fields and Seq live prefix),
partial operations (`_partial_*`, head/pop/at and divisor), action coverage,
deadlock, and response lassos/stalls are automatic. Declare intended stopping
states with `terminal`; business derives sink terminals, requirements does not.

`verify --property P --engine induction` still checks implicit bounds in its
base; sibling user invariants remain unselected (selected trans retains invariant
hypotheses). A selected run is not a completion run for all properties/seams.

Read vacuity/coverage warnings: `never_enabled_action`, `vacuous_implication`,
`vacuous_leadsto`, `always_true_requires`, `tautology_over_frozen`,
`urgency_freeze`, `vacuous_deadline`. Reachability-probe cutoff/evaluation failure
is `vacuity_probe_truncated`, not established vacuity. `--vacuity error` fails
closed; ignore hides evidence and is not a completion policy. Coverage at depth
K is bounded; generated-only actions are not authored coverage obligations.
`scenarios` retains never-enabled diagnostics but has no vacuity mode.

## Compose and refinement

```fsl-fragment
compose <Name> {
  use <SpecName> as <alias> from "<relative.fsl>"
  state { <local state> }
  init { <initialization> }
  fair action <sync>(<p>: <alias>.<Type>) =
    <alias>.<act>(<p>) || <other>.<act>(<p>) {
    requires <additional condition>
    <local statements>
  }
  internal <alias>.<action>
  invariant <Name> { <cross-component predicate> }
}
```

No nested compose. Component references are checked before prefixing; a parent
cannot rescue an undeclared child name. Sync reference arguments read header
params/consts/state, not body lets. Additional sync statements write only local
composition state; glue actions can read/write component `alias.var`. Init can
initialize components but cannot overlap their init writes. Sync does not inherit
fairness: use `fair action` explicitly; otherwise `fair_not_inherited` warns.
Sync numeric arguments use bounded value ranges, not nominal alias names; passing
to a narrower component can check yet violate its bound, so guard the call.

```fsl-fragment
refinement <Name> {
  impl <ImplName>
  abs <AbsName>
  maps auto
  enum conversion <conversion> <ImplEnum> -> <AbsEnum> {
    <ImplMember> -> <AbsMember>
  }
  enum abstraction <abstraction> <ImplEnum> -> <AbsEnum> {
    <ImplMember> -> <AbsMember>
  }
  map <abstract_scalar> = <impl expression>
  map <abstract_map>[<key>: <Type>] = <impl expression>
  action <impl_action>(<params>) -> <abs_action>(<expressions>)
  action <internal_action>(<params>) -> stutter
  preserve progress { respond <AbsLeadsTo> by <impl_action>, <other_action> }
}
```

Explicit entries override optional auto defaults. Auto correspondence matches
parameters **by name**, not position; arity/surplus/unmatched params reject.
Available registered source actors must match for auto pairs; requirement text
does not supply an actor. Explicit correspondence/stutter is not actor-checked.
Typed correspondence formals must agree with the impl action.

Same-named types must agree in kind, resolved bounds, ordered enum members and
struct fields/types, even if unused. Use distinct names for distinct definitions.
Distinct nominal enums need explicit `enum conversion` (exhaustive bijection)
and `convert(name, expr)`, or `enum abstraction` (source-total many-to-one) and
`abstract(name, expr)`. No ordinal conversion. Shared enum member names can be
ambiguous; preserve-progress additionally rejects cross-enum member collisions.
Raw production/causal log mappings lack a typed impl and reject enum mappings.
For abstract Map keys convert the abstract binder toward the impl key type.

Use canonical lowered names from `explain --readable --names canonical` or
Kernel; no glossary in design/mapping. Ordinary refinement carries checked
safety, permits strengthened guards, and is bounded by its reported depth.
Verify each model independently. Progress only crosses when explicitly pulled
through `preserve progress` (or verified at its owning lower layer); `by`
names are metadata/validation, not fairness. A false internal stutter reports
`stutter_changed_abs`; a lost pulled response reports `progress_lost`.

## 8. Idioms

These are partial declarations to incorporate into a confirmed model:

```fsl-fragment
requires stock[i] > 0
requires cart[u] is some(item)
requires stock[item] > 0
action take() { requires q.size() > 0 x = q.head() q = q.pop() }
action drain() { if q.size() > 0 { x = q.head() q = q.pop() } }
invariant EachQueued { forall job in q { jobs[job].st == Queued } }
invariant NoDup { forall i in 0..CAP-1 { forall j in 0..CAP-1 {
  (i < j and j < q.size()) => not (q.at(i) == q.at(j))
} } }
type Idx = 0..CAP-1
invariant Total { balance == sum(i: Idx of log.at(i) where i < log.size()) }
const SLOTS = 4
type Cell = 0..ROOMS*SLOTS-1
state { holder: Map<Cell, Option<UserId>> }
reachable Room1Full { forall c: Cell { c / SLOTS == 1 => holder[c] != none } }
state { ever_locked: Map<UserId, Bool> }
```

The ghost must be initialized/updated on the actual history-producing actions;
an invariant over a frozen ghost is hollow. A mapping can project a tag:

```fsl-fragment
map seats[s: SeatId] = if slots[s].st == Sold then slots[s].holder else none
```
