# Pattern binding scope, and why the `def` capture check stays

Issues: #1132, #1139. Related: #1119 (quantifier binder leak), #1120.

Two questions were raised together because both were found while reviewing
the #1119 scoping fix. They are answered separately here, because neither
answer follows from the other.

All measurements below were taken with a `fslc` built from this branch
(`cargo build -p fslc-rust --bin fslc`, reporting version 4.7.0 — the #1119
fix is on the branch but not in the 4.7.0 release artifact) and with the
Python implementation via `python -m fslc`. `--no-cache` was used for every
`verify`.

## 1. `x is some(v)` (#1132)

### Decision

`x is some(v)` is a **binding**, not a match, and the binding is
**lexically confined**.

1. The form always binds `v` to the payload. It never compares against a `v`
   that is already in scope. Its truth value is exactly "the operand is
   present".
2. When `v` is a fresh name, the binding is available in the logical
   continuation of the match and, when the match occurs in `requires`, in
   the action body. That is the existing `docs/manual/LANGUAGE.md` §9 idiom and it
   is not changed.
3. When `v` collides with an enclosing binder — an action parameter, a
   quantifier binder, a `let` — the shadowing is **confined to the
   expression in which the match occurs**. The enclosing name is restored
   outside it. This is the rule `docs/manual/LANGUAGE.md` already states for
   quantifier binders: *"when it shadows an enclosing name (an action
   parameter, say) the shadowing is confined to that scope"*.
4. The collision is additionally a **located `check`-time error**
   ("pattern variable `v` shadows an action parameter; rename it"), for the
   reason recorded in `docs/design/DESIGN-def.md`: this repository prefers an
   explicit local repair over silently changing meaning. Implemented in #1259;
   see "Work this decision implies" below for its full collision scope.

### Why not "match"

The match reading — treating an already-bound `v` as a comparison, as Erlang
and Prolog do — was rejected because **no implementation does it and
adopting it would silently change every existing spec that uses the form**.
Measured, all engines of both implementations agree that the form binds:

```fsl
spec Disc {
  type R = 0..1
  state { o: Option<R>, fired: Bool }
  init { o = some(1)  fired = false }
  action act(r: R) { requires r == 0 and (o is some(r))  fired = true }
  invariant NeverFired { not fired }
}
```

Under a match reading `act(0)` is disabled (`o` holds `1`, not `0`) and the
invariant holds. Measured:

    RS explicit   violated {'r': 0}
    RS bmc        violated {'r': 0}
    RS induction  violated {'r': 0}
    PY bmc        violated {'r': 0}

The form is a binder. That is settled, not chosen.

### What was actually undecided, and the divergence it caused

The open question was point 3 — whether the rebinding escapes the guard and
overwrites the action parameter for the body. **The two implementations
disagree, and produce opposite verdicts on the same specification.**

```fsl
spec Escape {
  type R = 0..1
  state { o: Option<R>, w: Map<R,Bool> }
  init { o = some(1)  forall r: R { w[r] = false } }
  action act(r: R) { requires r == 0 and (o is some(r))  w[r] = true }
  invariant NoW0 { not w[0] }
  invariant NoW1 { not w[1] }
}
```

    PY  NoW0  bmc        violated {'r': 0}
    PY  NoW0  induction  violated {'r': 0}
    PY  NoW1  bmc        verified
    PY  NoW1  induction  proved
    RS  NoW0  explicit   proved
    RS  NoW0  bmc        verified
    RS  NoW0  induction  unknown_cti
    RS  NoW1  explicit   violated {'r': 0}
    RS  NoW1  bmc        violated {'r': 0}
    RS  NoW1  induction  violated {'r': 0}

Python has `act(r=0)` write `w[0]`. Rust has the same call write `w[1]`.
Both are internally consistent across their engines; they are opposite to
each other. Issue #1132's premise that "all three engines agree" holds
within the Rust port only.

The decision follows the Python implementation because that behaviour is
**deliberate and coded**, not accidental. `_eval_requires` in
`src/fslc/bmc.py` copies guard bindings back into the body scope only for
names that are not action parameters:

```python
for k, v in b.items():
    if k not in param_binds:
        binds[k] = v
```

The Rust port dropped that condition: `rust/fsl-runtime/src/lib.rs:223` and
`rust/fsl-verifier/src/eval.rs:1636` both do an unconditional
`bindings.insert(name, value)` into the caller's map. So #1132 is a port
regression against a decision this repository had already made, not an open
language question.

Two further consequences of the unconditional insert were measured on the
Rust port, both absent from Python:

- **Type-checking divergence.** With an `Option<Bool>` matched into a
  parameter `r: R`, `extend_pattern_binding`
  (`rust/fsl-core/src/typecheck.rs:495-512`) rebinds `r`'s *type* to `Bool`
  and the body's `w[r]` is then rejected:
  `error semantics: expression of type Bool is not assignable to Named("R")`.
  Python accepts the same spec (`ok`) and verifies it (`violated {'r': 1}`),
  because `r` is still the parameter in the body. Rust rejects a spec Python
  accepts.
- **Internal error on a failed match.** `requires not (o is some(r))` with
  `o = none`: `rust/fsl-verifier/src/eval.rs:1635` inserts the binding from
  the `present`-false branch too, so the symbolic `r` becomes the unconstrained
  payload of a `none` while the explicit evaluator leaves `r` alone. Measured:
  explicit `violated {'r': 1}`, bmc and induction
  `error internal: trace state mismatch at step 1`. Python bmc returns
  `violated {'r': 1}`. The same spec with a fresh pattern name, and the same
  spec with `o = some(1)`, are both fine on all engines — the error needs the
  shadowing *and* a failed match.

### Why confine rather than let it escape

Beyond following the reference implementation, letting a pattern binding
overwrite an enclosing name makes `and` non-commutative in a way nothing
else in the language is. Same spec, conjuncts swapped:

    r == 0 and (o is some(r))    ->  violated (all engines, both impls)
    (o is some(r)) and r == 0    ->  proved / verified (all engines, both impls)

Binding *availability* is legitimately order-dependent — `docs/manual/LANGUAGE.md`
already says the binding is available on the right-hand side of `and`. The
*meaning of an already-bound name* changing with conjunct order is a
different thing, and it is what confinement removes.

### Work this decision implies (not done here)

- Restore the parameter protection in the Rust port (runtime, verifier, and
  the type checker's `extend_pattern_binding`) so it agrees with Python.
- Add the located `check`-time shadowing error (point 4). Done in #1259: the
  error also covers state variables, constants, enum members and an earlier
  pattern binding of the same name, not only enclosing binders.
- Then write points 1–4 into `docs/manual/LANGUAGE.md`'s Option bullet.

These are behaviour changes to the semantics and were deliberately left out
of this analysis task. Until the port is fixed, the Rust internal error
above is a live defect.

### What would overturn this

A measured case where confining the binding breaks the §9 idiom — that is, a
spec with a *fresh* pattern name whose body stops seeing the payload under
the Python rule.

## 2. The `def` capture check (#1139)

### Decision

**Reading 3.** The check defends a case #1119 did not fix and could not have
fixed. It stays as a semantics error, and its message is correct as written.

### Why the issue's premise is wrong

#1139 assumes the check exists because "the runtime used to leak a
quantifier's binder into the caller's scope". It does not. The check guards
**capture-avoiding substitution at `def` expansion time**, which is a
frontend AST rewrite, not a runtime scope question. `substitute()`
(`rust/fsl-core/src/lib.rs:1626`) and `_substitute()`
(`src/fslc/predicates.py`) both replace a parameter with the argument
expression **without alpha-renaming**. If the argument's free variables
collide with a binder in the definition body, naive substitution changes
the meaning. #1119 changed the runtime's binding maps; it did not touch
either substituter, so it cannot have made this check redundant.

`docs/design/DESIGN-def.md` already records the rule as intended design:

> If substituting a call argument would capture one of its free variables,
> the call is rejected with a source-level diagnostic asking the author to
> rename the binder. The implementation never creates a synthetic binder
> name. […] The last rule favors an explicit local repair over silently
> changing meaning or leaking compiler-generated names into counterexamples.

### The measurement

```fsl
spec Cap {
  type R = 0..2
  state { a: Map<R,Bool>, done: Bool }
  init { forall r: R { a[r] = (r != 0) }  done = false }
  def allBelow(x: R) = forall r: R where r < x { a[r] }
  action act(r: R) { requires r > 0 and allBelow(r)  done = true }
  invariant NeverDone { not done }
}
```

    RS check: error semantics
      predicate 'allBelow' call would capture variable 'r';
      rename the binder in the def at 6:41
    PY check: same message

The call has two possible expansions. Capture-avoiding (`def`'s binder
renamed to `k`):

    RS explicit   proved
    RS bmc        verified
    RS induction  unknown_cti
    PY bmc        verified

Naive, i.e. what the substituters would emit —
`requires r > 0 and (forall r: R where r < r { a[r] })`, where the guard is
vacuously true:

    RS explicit   violated {'r': 1}
    RS bmc        violated {'r': 1}
    RS induction  violated {'r': 1}
    PY bmc        violated {'r': 1}

Opposite verdicts. The check is the only thing standing between the author
and the second one.

Calibration: with the check disabled in `src/fslc/predicates.py`
(`captured = []`), `fslc check` on `Cap` returns `ok` and
`fslc verify --engine bmc` returns `violated {'r': 1}` — the wrong meaning,
silently. Both capture tests in `tests/test_named_predicates.py` fail; all
other tests in the file pass.

### The message

`predicate 'allBelow' call would capture variable 'r'; rename the binder in
the def` is accurate. Capture genuinely would occur, and renaming the
definition's binder genuinely fixes it. No change.

### Scope of the check

It is narrow and stays narrow: it fires only when the free variables of an
argument expression intersect the binders of the definition body
(`bound_vars` ∩ `free_vars`). `allBelow(1)` and arguments naming state
variables check fine; the issue's observation that `other(o)` passes while
`other(r)` fails is this rule working as specified, not an inconsistency.

### What would overturn this

Making `def` expansion capture-avoiding — an alpha-renaming substituter —
which `docs/design/DESIGN-def.md` rejects on the ground that synthetic binder names
would surface in counterexamples. Overturning this decision means
overturning that one, with evidence that generated names in traces are
acceptable.

## 3. True-continuation enforcement (#1547)

This amendment makes the native check boundary enforce the language reference's
true-continuation rule. The measurements and pending-work descriptions above
are historical; the blanket reference to a match occurring in `requires` does
not authorize exporting a binding through a disjunction.

### Invariant and boundary

One operator policy governs both the bindings exported by a true whole
expression and those available while visiting a binary expression's right
operand:

- Whole-expression export traverses `and` only. A true disjunction or
  implication does not establish that its pattern matched.
- The right operand may use the left operand's true-result bindings for
  `and` and `=>` only.
- A `requires` guard exports only these whole-expression bindings into later
  guards and the action body; no pattern name becomes global.

Visibility is not collision reservation. `ValidationPhase.collisions` and the
Renamer's `Scope.locals` preserve the base's reservation inventory separately
from readable names and rename mappings. The shared `extend_pattern_names`
collector has a visible mode (the helper's rule) and a collision mode
(`Is(Some)` and all `Binary` children). Only the latter feeds collision checks;
it never supplies types or makes a name readable. Typed model validation carries
the collision phase through later guards, lets, statements, lvalue indices,
ensures and binder children. Component rewriting uses this phase only within
each expression: every action item starts a fresh normalized phase, and the
readable scope carried across `requires` items contains only conjunctive
exports. Subsequent model validation checks cross-item collisions; sync source
hygiene separately carries its reserved locals. Together these checks prevent
a narrower readable environment from admitting repeated names that the
first-binding-wins evaluators would interpret incorrectly. Evaluators were
unchanged in the #1547 amendment; see the subsequent #1622 repair below.

This inventory intentionally preserves the base boundary rather than claiming
to inventory every possible evaluator binding. The existing not/if reuse
problem is separate work in #1573. Adding its constructor cases to this one
collector can extend reservations without introducing another validator.

Previously accepted uses on the right of `or`, after a disjunctive or
implicational antecedent, across comparison operands or out of comparison
results, and disjunctive flow into quantifier/aggregate bodies and filters are
breaking removals. Escapes *outside* negation, quantifiers and aggregates, and
names used in an `if` arm from its condition, were already refused on base:
their controls preserve existing refusals, not new removals.
An out-of-scope `v` reports `kind: "semantics"`, exit 2, with the existing message
`invalid model expression: public Kernel cannot type identifier 'v' at <line>:<column>`.
Its location is the containing clause or `if` arm. Guards/postconditions retain
the existing action-level fallback; an expression's more specific arm span can
take precedence. More precise guard/postcondition clause locations remain
separate work in #1550.

The original baseline accepted
`((o is some(v)) or x == 0) => v == 1` with `o = none` and `x = 0`;
BMC and induction could report `violated` using the unbound payload name.
Rejecting the source is necessary: that verdict cannot establish a valid
counterexample for an ill-scoped formula. That amendment did not change evaluator
semantics; source refusals remain in force after the later lifetime repair.

### Shared policy and sibling consumers

#### Evaluator lifetime repair (#1622 / #1577)

Symbolic values and concrete evaluation now use closed expression boundaries
and explicit true-continuation entry points. Only a match and a conjunctive
whole result can export payloads. Binary RHS visibility follows the same
`pattern_binding_flows` policy; reaching an `or` RHS does not inherit the left
environment. A rewritten local match may shadow an outer binder within its
continuation, but closed evaluation restores the complete outer environment,
including independently stored declared type metadata, also on errors.

Ordinary/raw and expected-type conditionals isolate condition and both arms;
comparison operands, compound values, access, arguments and candidate-local
quantifier/aggregate evaluation also close their children. Existing checked
inference and collection materialization remain authoritative. Guard value and
definedness walks explicitly export conjunctive `requires` bindings to later
guards/body, while a `let` RHS stays closed. Init diagnostic reachability and
ranked-measure traversal use the same RHS boundary without altering short-circuit
reachability, partial-operation order or bounds priority.

The concrete runtime preserves lazy logical/conditional evaluation and remains
solver-independent. Monitor, solver-free BFS, BMC and induction-with-bounded-base
controls assert actual truth, not agreement with historical incorrect concrete
results. Shared evaluators also correct cross-statement/composed-init payload
leakage. The frontend's #1573 reservation (including then-to-else), #1595
refinement refusals and known composed-init reservation gaps are unchanged; the
repair is not permission to reuse a reserved name in source.

Direct evaluator controls bypass only collision admission, retain checked
expression typing, and assert restored environments and true/false results.
Separate CLI controls preserve impossible/reached progress outcomes and pin
later-payload init values. No solver, substitution, public envelope or grammar
change is part of this repair.

Diagnostic observation lifetime is distinct from lexical binding lifetime.
`violating_bindings` seeds a fresh evidence frame with each candidate's original
binder assignments after enumerating its source, then observes its selected
filter and closed body exactly once. A failed selected candidate publishes that
frame; filtered and successful candidates discard it. Each successful `is some`
records its payload directly, retaining the first value for each key, independently
of lexical shadowing. Evidence is never identifier lookup or continuation authority.

Every actually executed child shares the frame, including `not`, `or`, implication,
conditional conditions/selected arms, eager operands/arguments and optimized access
indices. Short-circuited operands and unselected arms are not evaluated or observed.
Nested quantifier/aggregate sources, filters and bodies remain isolated; no nested
local captures merge into the parent frame. Nonquantified diagnostic fallback also
observes closed evaluation, while searched conjuncts have separate frames. Ordinary
evaluation still closes its children and `or`/implication export no whole-result
bindings. Top-level and blame packets preserve reached payloads without widening
read visibility or changing truth/errors. Native projection remains one candidate
level and blame renders a whole expression; historical descriptions of recursive
nested-forall projection or conjunct splitting are not implemented by this repair
(#1437).

`fsl-core::pattern_binding_flows(op, right_operand)` is the single operator
decision. The initial eight-consumer estimate omitted the separate free-name
walk, and review found the Renamer as a tenth logical consumer:

| Consumer | Source | Use |
|---|---|---|
| Typed export collector | `typecheck.rs` | Whole-expression export |
| Typed expression validation | `typecheck.rs` | RHS flow |
| Public Kernel serialization | `public_kernel.rs` | RHS flow |
| Partial-operation classification | `partial_operation.rs` | RHS flow |
| Component rewrite | `compose.rs` | RHS flow |
| Compose alias resolution | `compose.rs` | RHS flow |
| Def expansion | `lib.rs` | RHS flow |
| Free-name analysis | `model.rs` | RHS flow |
| Name export collector | `compose.rs` | Whole-expression export |
| Source-owned Renamer | `compose/hygiene.rs` | RHS mapping flow and whole-result mapping export |
| Symbolic evaluator and status | `fsl-verifier/src/eval.rs` | Closed children, RHS visibility and whole-result export |
| Init diagnostic reachability | `fsl-verifier/src/init_definedness.rs` | RHS visibility |
| Ranked-measure traversal | `fsl-verifier/src/induction.rs` | RHS visibility |
| Concrete evaluator | `fsl-runtime/src/lib.rs` | Closed children, RHS visibility and whole-result export |

The whole-export and RHS decisions in both DDD paths, `domain_lowering.rs` and
`domain.rs`, also delegate to the same helper, replacing four equivalent
frontend copies. Collision propagation is a separate reservation policy, not
another visibility operator rule.

The compose guard-name set is a syntactic capture-protection inventory, not a
visible-value scope. It uses `extend_pattern_names` in collision mode, retaining
the base's `Is(Some)`/all-`Binary` inventory. Using the broader general collector
here incorrectly reserved names inside not/if/quantifier/aggregate children and
introduced `__v_substitution_N` into accepted compose output. Full base-generated
`explain` regressions for sibling negated guards and a conditional sync guard
reject that regression. Acceptance checks alone do not detect this output drift.
Six focused sync/foreign-argument controls cover ordinary, disjunctive, negated,
quantified, conditional and aggregate patterns; they are bounded evidence, not
an exhaustive admission-equivalence proof.

The position/reuse/public-entry matrices include rejecting controls and legal
`and`/`=>`/`requires` preservation controls. Twelve isolated inversions cover the
original nine consumers, collision propagation, and both Renamer decisions;
each detected inversion produces cargo exit 101 in two runs with byte-exact
source restoration between sites. The Renamer export inversion initially
survived. A separate test-only legal two-source `and`-guard-to-`ensures` control
forces a fresh name and detects its lost mapping; that control checks on base,
the earlier implementation, and normal head, but fails under the inversion.
The earlier def-RHS witness gap was similarly exposed and repaired with two
def-call cases. Neither survivor is treated as a successful detector.
Publication/admission controls detect some inversions but are not independent
negative-admission semantic lineages.

### Diagnostic priority and intentional location changes

An earlier newly illegal reference can be reported before a later repeated-name
collision. Such inputs still reject with exit 2 and `kind: "semantics"`.
Message priority is an intentional change, not permission to reuse names: for
every changed collision-family message in the sweep, changing only the reported
reference `v` to the existing `x` restores the complete original base collision
packet, twice on both binaries. Pattern declarations remain unchanged.

Expression-if arm inference/assignability can report an arm reference error
before freshness validation of the condition. With
`requires (o is some(v)) => true` followed by
`y = if (p is some(v)) then v else 0`, base reports the condition collision at
`6:12`; head reports the unbound arm reference at `6:32`. The else-arm variant
reports `6:39`, and the nested-if variant `6:48`. Direct and compose controls
pin the entire message, exit 2, kind and these locations. Renaming the arm
reference restores the base packet at `6:12`; rejection is preserved.

When a `def` body's free name would be captured only by an out-of-continuation
pattern at its call site, the name is now reported as an unbound reference in
the containing clause instead of as predicate capture. This is a diagnostic
class, not a list of three special cases: messages and possibly locations
change while exit 2 and kind semantics remain. Independent review exercised
11 such shapes, including invariant comparisons and disjunctive action guards.

The historical sweep below measured 18 message changes: 15 collision-family cases
(eight unchanged locations and seven explicitly tested arm-location changes)
and three examples of this predicate-capture class. All 75 base local-name
collision inputs remain rejected. Among those examples, direct `def-call-or-free-name` moves
from an absent loc to `7:3`, its compose route from use loc `2:3` to `7:3`, and
the reviewed `k()` case from an absent loc to `6:3`; all retain exit 2 and kind
semantics. They are outside the pattern-shadow collision family, not silently
excluded changes. Thus neither universal diagnostic equality nor universal
location equality is claimed.

### Repair by intended absence policy

The undefined `(o is some(v)) or v == 1` has no sound equivalence claim. For
the policy "absence is allowed; a present payload must equal 1", write
`(o is some(v)) => v == 1`, equivalently
`o == none or (o is some(v) and v == 1)`. Both total formulas agree over `none`,
`some(0)` and `some(1)`: their equivalence checks and is BMC-verified to depth 3,
inductively proved and explicit-state proved. Repeating with `and` alone in
place of the explicit absence allowance produces a violation at initial
`none` in all three engines. Thus choosing `and` alone changes the policy; it
is not an automatic repair when absence must be accepted.

### Measured breaking removals

The sweep ran `<binary> check <path>` in each commit's own archived tree:
BASE `c32e7f5a88b412056210ee66f0cff9765001fed9`, measured
HEAD `b9751830e76ba64ad402c982387232a39ff87c9a`. Base binary SHA256 is
`c37d14304ababaa57d1b9637d93653f610078421ed3c3c805f2c3fcd8f268678`;
head is `7d9eb9a60553f22fb0aaf4d99a4e6581396a341186f4bef9b385060a02f40a13`.
Two complete runs compared stdout, stderr and exit code, with no excluded fields
and zero repeat differences. Each SHA has its own materialized tree, executed
at the same absolute address to control source/provenance paths without
dropping fields. These are historical observations at the named measured HEAD,
not a sweep of later native revisions. Diagnostic variations above are recorded
contract changes, not fields removed from the comparison.

| Group | Forms/routes | Base exits 0 / 1 / 2 | Head exits 0 / 1 / 2 | `ok`→`error` | Full output changes |
|---|---:|---:|---:|---:|---:|
| Corpus: `specs/`, `examples/`, `rust/fslc/tests/fixtures/` | 597 | 400 / 6 / 191 | 400 / 6 / 191 | 0 | 0 |
| Existing static complete Rust-test document literal occurrences | 1,750 | 1,000 / 0 / 750 | 1,000 / 0 / 750 | 0 | 0 |
| Authored inputs, including reuse/priority/counterfactual controls | 146 | 72 / 0 / 74 | 15 / 0 / 131 | 57 | 69 |
| Independent-review inputs | 110 | 72 / 0 / 38 | 48 / 0 / 62 | 24 | 26 |
| Original direct/compose priority probes | 4 | 0 / 0 / 4 | 0 / 0 / 4 | 0 | 4 |

The existing-test inventory scans 532 base / 535 head Rust test source files
(integration sources plus test-attributed unit scopes). It decodes raw/escaped
literals and static escaped format braces, but does not substitute interpolated
placeholders or complete partial templates. Another 1,766 document-like
candidates are inventoried, not executed: 222 interpolated formats, 1,543
incomplete fragments and one unterminated FSL string/comment fragment. Negative
and invalid complete literals are checked, not silently dropped. This bounded
sweep does not reconstruct arbitrary runtime-generated sources or sibling files
that a test would create. Source-line movement in the inventories is recorded
separately from CLI output; the literal contents are unchanged.

Each of the following **28 cells has two breaking removals**, direct and compose
`0`→`2`. Each removal's complete head packet also matches its authored check
expectation, including message, location and version envelope:

| Rejected flow | Authored cells (each direct and compose) |
|---|---|
| Disjunction | `type-or-right`, `type-nested-and-or-right`, `type-not-inside`, `issue-exact` |
| Comparison escape | `type-equality-escape`, `type-inequality-escape`, `type-equality-right-operand` |
| Implication/export escape | `type-implies-left-or`, `type-implies-left-implies`, `type-implies-escape-to-and`, `false-verdict-implies-left-or` |
| Disjunctive flow into quantifier/aggregate/method | `type-forall-body`, `type-exists-body`, `type-forall-where`, `type-sum-body`, `type-sum-where`, `type-count-where`, `type-method-argument` |
| Disjunctive flow into partial operations | `partial-seq-index`, `partial-map-key` |
| Requires export | `requires-or-value`, `requires-or-ensures`, `requires-or-later-requires`, `requires-implies-value` |
| Def expansion/caller | `def-body-or`, `def-argument-or`, `def-caller-or-right` |
| Compose sync alias | `compose-sync-alias-or` |

The two-source `h1` adds one compose-only removal, for 57 authored routes.
The 24 reviewed removals are `act/a07`, `comp/c05`, `docs/n4,n7,n8`,
`guards/a3,h1,h2`, and `inv/i01,i03,i04,i05,i06,i09,i11,i12,i13,i15,i16,i17,i20,i21,i22,i25`.
They exercise the same forbidden disjunctive/comparison/implication exports,
including uses inside negation, quantifiers, filters and def arguments rather
than previously rejected outer escapes. These are 81 observed removals total;
the corpus and existing literal transition sets are empty. There are no
observed acceptance expansions or unapproved parity changes in this bounded
sweep, not a claim of exhaustive language-wide admission equivalence.

### Guard-reservation output recheck

The subsequent native revision `8eef04058d51105c7788e17eb0c7baf328439ff3`
restores the base guard-name collector and consolidates the remaining domain
operator copies. Output was measured at its documentation-only successor
`5ec10a0f00fd4cbfb5842ab94f0aa0ee88917f8b`, with head binary SHA256
`66e7d99c10d195398b2d1c1897ed0c9a9569889cd46c5089086ef950ba235d80`
and the same named c32 base binary above. Both binaries read the same source
paths; their hashes and the input hashes stayed fixed throughout two runs.

| Group | Commands | Two-run comparisons | Raw byte-equal | Timing-only differences |
|---|---|---:|---:|---:|
| Three reservation probes: component, sibling negated guards, conditional sync guard | `check`, `explain`, `analyze`, `html --no-timing`, `verify --engine explicit --depth 8 --no-cache` | 30 | 24 | 6 |
| Every top-level compose in the three corpus roots: 20 existing and two new regression fixtures | `check`, `explain --depth 2`, `analyze` | 132 | 128 | 4 |

The corpus inventory contains 22 compose files: two under `specs/`, one under
`examples/`, and 19 under `rust/fslc/tests/fixtures/`. Invalid compose inputs are
compared too, not excluded. The initial default-depth corpus attempt timed out
after 180 seconds in the unchanged base's `explain specs/order_system.fsl`.
Depth 2 is therefore an explicit uniform bound for corpus explanation, not a
per-file bypass or a claim of default-depth completion. The reservation probes
and both full base-generated regression goldens retain default explain depth 8;
the guard/action rendering exercised by this fix is not pruned by search depth.

Raw bytes, stderr and exits were retained. Observed wall-clock timers alone
prevent byte equality for the six verification comparisons and the four
bank/order explanation comparisons. The exact observed timer paths are:

- `/cost/elapsed_s` for each of the three reservation probes' `verify` results;
- `/counterfactuals/i/violation/cost/elapsed_s` for `specs/bank_system.fsl`,
  only at indices `0, 1, 2, 4, 5, 6`;
- the same counterfactual path for `specs/order_system.fsl`, only at indices
  `1, 2, 3, 4`.

These are measured elapsed times, not semantic outputs. Every exact timer key
must exist and be numeric/nonnegative on both binaries; removing it from both
sides is rejected (52 missing-key negative controls across two runs). The entire
remaining JSON packet is compared, not a selected field list. No verdict,
location, assurance, trace, exit, or other cost field is excluded. All stable
outputs also agree across repetitions; there are no other observed differences.
Six sync/foreign-argument guard controls additionally remain exit 0 with full
byte equality in two runs. The full historical check sweep above was not rerun
for this revision and must not be relabeled as current evidence.

### Synchronization measurement on e0b4cbae

The accepted Rust topic patch was transplanted unchanged onto BASE
`e0b4cbae39569de8008a8eb049588aa84d8c1a42`, measured HEAD
`3ae20a93296d36089f7860957a0a0b601f96932a`. Base binary SHA256 is
`3b72163b471ae00c304b86fbdc8c8d9e724391c377dc2a2088eae0d5c111fd28`;
head is `a874111d79e4835c874d86f03f8fa5bdf5037392771b0f3ea0f4271f24bd155e`.
`<binary> check <path>` ran twice in each SHA's own archived tree at the same
absolute execution address. Complete stdout, stderr and exits were compared,
with no excluded fields and zero repeat differences.

| Group | Forms/routes | Base exits 0 / 1 / 2 | Head exits 0 / 1 / 2 | `ok`→`error` | Full output changes |
|---|---:|---:|---:|---:|---:|
| Corpus across the three required roots | 601 | 402 / 6 / 193 | 402 / 6 / 193 | 0 | 0 |
| Existing complete static Rust-test document literals | 1,777 | 1,024 / 0 / 753 | 1,024 / 0 / 753 | 0 | 0 |
| Authored inputs | 147 | 72 / 0 / 75 | 15 / 0 / 132 | 57 | 70 |
| Independent-review inputs | 110 | 72 / 0 / 38 | 48 / 0 / 62 | 24 | 26 |
| Original priority probes | 4 | 0 / 0 / 4 | 0 / 0 / 4 | 0 | 4 |

The corpus comprises 598 baseline files plus the three new reservation fixtures
`a.fsl`, `c.fsl`, `c_if.fsl` under
`rust/fslc/tests/fixtures/issue_1547_guard_reservations/`. Those three authored
inputs were explicitly materialized in the base tree too; no existing baseline
input or Rust source was replaced. The static inventory scans 540 base / 544
head test sources; 1,783 further candidates (230 interpolated formats, 1,552
incomplete fragments and one unterminated FSL string/comment) are inventoried,
not executed. Runtime-generated templates and test-created sibling files still
have the bounded coverage described above.

All 81 breaking removals are the same shapes/routes enumerated above; the
corpus and existing-static-literal transition sets remain empty. All 75 base
collision inputs still reject, with zero acceptance expansions. The original
18 message-change rows match the historical sweep's complete base/head packets
exactly, including all seven location changes. A nineteenth row is the authored
direct-only `def-call-implies-free-name` occurrence: its source and both packets
are byte-identical to the already-reviewed `review/def/d06.fsl` example. This
increases authored routes from 146 to 147 and message-change occurrences from
18 to 19, not the diagnostic classes or removals. All 15 collision-return
controls still restore the entire original base packet twice on both binaries.

The newer baseline adds AI-component validation routes, constant/enum lookup
for helpful arguments, an initial-state bound pre-pass, lazy mutant
materialization, and static filtered/collection init-forall coverage. The named
walks (`validate_ai_component`, `helpful_matches`, `check_state_bounds`,
`BuiltinMutant::materialize`, and `explicit::walk_init`/
`init_binder_static_values`) add no Option-payload visibility decision or
pattern-name collector. Init coverage tracks forall keys and static values;
its existing dependency/filter walkers do not replace the checked pattern-scope
boundary. No extra scope implementation was introduced during synchronization.

The independent review's 100 two-source compose inputs were also compared
afresh on these same binaries through default-depth `explain`, `analyze`, and
`html --no-timing`, twice: all 600 base/head comparisons are byte-identical,
including stderr and exit 0. Both repetitions agree in full, with no excluded
fields; the input and binary hashes remain fixed. The earlier bounded corpus
explanation measurements above are not substituted for this complete set.
Both whole native rechecks pass (8 focused, 133 core and 288 CLI tests per run),
including S1/d06/collision/location pins and the newer baseline's explicit/init
coverage controls; workspace Clippy, fmt, native build and generated-page
freshness pass twice. Six guard-inventory controls additionally agree in full
on the final normal binary in two runs.

## 4. Executable locals may not shadow constants (#1515)

### Decision and breaking removal

`check` rejects an executable local binding whose name equals a const name,
including non-integer constants. This is a breaking `ok`→`error` restriction,
not a change to engine range evaluators. Symbolic range bounds previously
resolved the name as the global const while concrete lexical evaluation used
the local. Both false violations and false passes resulted. Rejecting the
ambiguous source before selecting an engine prevents that disagreement.

The shared diagnostic names the local and says it `shadows a constant`;
`check`, BMC, induction and explicit return identical complete `semantics`
error envelopes, exit 2. No verdict, assurance or location fields are excluded
from that refusal comparison. The existing broader pattern-binding collision
rule in section 1 is preserved, not replaced by this narrower const rule.

### Binding inventory and location precision

The location is the existing AST owner's span, **not a newly recovered name
token**. No parser span carrier or source rereading is added.

| Source-visible introduction or compiler exception | New const refusal | Diagnostic owner / reason |
|---|---|---|
| `forall`/`exists`, typed/range/collection | yes | enclosing expression or declaration retaining a span |
| `count`, `sum`, `unique`, `exactlyOne` | yes | enclosing expression/declaration, before aggregate normalization |
| statement `forall` in init/action | yes | `forall` statement |
| outer `leadsTo` binder | yes | `leadsTo` declaration; existing collection/filter restrictions remain |
| action typed/range param and `let` | yes | action declaration |
| def param and binders in its body | yes | def declaration before substitution or dropping unused defs |
| component versions of these forms | yes | parent's `use`; component owner coordinate retained in causal message |
| requirements action/branch params and lets | yes | source action retained by lowering |
| requirements transition/event inputs | yes | transition retained by generated action |
| requirements indexed age binder | yes | age keyword retained by generated init/tick forall |
| legacy process implicit c; next entity instance first-letter name | yes | source process/entity declaration for generated init, including when no local declaration is written |
| relation event implicit actor-instance parameter | yes | source event declaration; matching const must be renamed |
| hidden KPI projection implementation binder | no | on collision only, use existing fresh_binding_name and rename the generated index consistently |
| `is some(v)` pattern | existing refusal | existing owning-declaration precision, unchanged |
| compose glue/sync locals using `part.N` | no same-name collision | bare local `N` and qualified const `part.N` are distinct declarations |
| refinement indexed-map binder | no additional gate | concrete correspondence binds keys locally before const lookup |
| refinement action-correspondence formals | no additional gate | formals mirror the source action; that action still obeys ordinary checking |
| business transition inputs | no new reachable gate | data-carrying `with` is already unsupported and rejected |
| business Responds binders | shared leadsTo rule after lowering | business has no authored const declaration to collide with |
| requirements imported as compose component | no new gate | compose already requires a `spec` component |

Component source checks happen before constants acquire component prefixes and
before local hygiene/substitution can erase a collision. Def parameters and
body binders are checked before inlining; body-name diagnostic selection is
deterministic. Requirements aliases retain their existing owner spans through
lowering, rather than claiming a token-level position they do not store.

Visible implicit instances are part of the source interface: `stage(c)` can
refer to a legacy process's instance; next-edition Request uses r and a relation
to Agent takes a. A const with one of these local names is rejected even when
the user never declares the local. For example, the generated init for
`requirements R { const c = 2 process Case { ... } }` locates the process
declaration (line 3:3 in the regression fixture), not a nonexistent local token.
Rename the const and its constant references while retaining instance uses.
This is distinct from the hidden KPI Count binder, which is not source-writable.

For example, the following source declares no local c, but its generated init
is refused at **3:3**, the process line. Rename `const c` to `const LIMIT`;
there is no local declaration token to edit.

```fsl
requirements R {
  const c = 2
  process Case {
    stages Idle, Done
    initial Idle
  }
  action idle() { }
  invariant P { true }
}
verify { instances Case = 1 }
```

### Behavior notes: erased arguments and hidden KPI names

Def-call arguments are checked for constant-shadowing binders **before any
argument is expanded or substituted**. With const N, a call such as
`ignore(forall N in 0..1 { true })` is refused even when ignore's unused formal
would erase the argument. Retaining, duplicating or passing that argument
through another def has the same message and original property/action owner,
or the existing call span where there is no such owner. The
common diagnostic conversion preserves source origins; no token lookup or
engine-specific rule is added.

Only an actual const collision freshens a hidden KPI projection binder. The
in-memory projection expression and its **expression-level Kernel AST JSON**
then use a reserved generated name such as `__c_substitution_0`; without a
collision, c and its JSON remain unchanged. This is not a public Kernel schema
change. Exported public Kernel v1/v2 documents do not include KPI projections,
so that hidden identifier does not appear there. Native traces contain the
event's visible instance parameter, not this projection-local name; `explain`
publishes KPI name/entity/stage metadata without its implementation binder.
Projection values and verdicts are preserved. Do not classify the implicit
process c or an authored age binder as hidden merely because lowering generated
their enclosing statement/action.

### Migration and preserved behavior

Rename the local declaration and its uses. With `const N = 2`, replace
`forall N in 0..1 { N <= 1 }` with
`forall k in 0..1 { k <= 1 }`; likewise rename an action/def parameter or let.
This does not promise general dependent ranges (#1480): alpha-renaming a
dependent bound alone may expose the existing static-bound limitation. Use
supported static domains and, where supported, a `where` membership guard.

State-name shadowing remains lexical and valid. Qualified component const
accesses in glue and sync remain valid. Existing pattern errors and temporal
capability boundaries remain unchanged: in particular, explicit still refuses
supported-source `leadsTo` verification. The all-engine comparison above is
for the new source errors, not a claim that successful engine-specific proof
reports or supported temporal capabilities are identical.

### Measured removal inventory and diagnostic priority

The retained original pre-migration inline inventory is listed below. Counts are
case-addressed forms; shared CLI/LSP/Worker consumers and repeated test products
are counted once, not presented as byte-unique programs.

| Regression family | `ok`→`error` forms | Representative original form | Migration |
|---|---:|---|---|
| Compose binding capture (#1442) | 126: 63 cells × standalone/composed | `const x = false` with `action go(x: Bool) { requires x }` | Rename local `x` and its uses; keep legal fresh/state/member controls. Original const-collision cells now assert located refusal. |
| Nested aggregate bounds (#1463) | 24, shared across CLI/LSP/Worker | `const x = false` with `sum(x in 0..1 of count(y in 0..x)) == 3` | Rename the local and uses subject to the existing dependent-bound limitation; original collision sources remain refusal detectors. |
| Per-binding helpful arguments (#1333) | 2: standalone/composed | `const c = 0` with `forall c: Case`, action parameter `c`, and `helpful step(c)` | Rename local `c` and uses. The regression uses a legal initialized state decoy to retain its helpful-argument capture control. |

Total **152** removed inline forms. Of the 63 compose cells, 42 combine a
typed/range parameter or let with seven expression/body positions and two
orders; 21 cover nested quantifiers, aggregates and statement forall across
typed/range/collection domains. All 126 fresh standalone/composed controls
still check. These are deliberate capture decoys, not dispensable constants
removed merely to make tests green.

Five partial-init sources (#1481) are a separate **error→error diagnostic
priority change**, not additional acceptance removals. Three have same-named
`flag` consts and outer collection/range binders whose false inner filter must
not establish coverage; two have `limit` consts and outer binders used in inner
range/key bounds. The frontend const-shadow error now precedes the ownership
refusal, at the outer forall owner. This also supersedes the former unlocated
Monitor/refine ownership diagnostics with located frontend errors. The source
strings remain unchanged, and other partial-init ownership controls remain.

Additional admission removals include visible implicit process/entity/relation
locals and the nested-init source with `const i = 1` and
`forall i in 0..0 { forall j in 0..i { m[j] = true } }`. That source checked on
base 46031415; the refusal locates the outer forall at **5:10**. A local-name
rewrite must update its bound uses; it does not add general dependent ranges.

### Verification snapshot

Native `check <form>` compared base
`46031415d5eaec6297280ae151c6695e64aebb14` with measurement commit
`65bf28765e89dfe6c6312110ac150cabb41dd156`, each in its own archived tree,
with cache off and no ambient unstable features. All **603 corpus forms** in
`specs/` (23), `examples/` (191) and `rust/fslc/tests/fixtures/` (389) have
byte-identical exit/stdout/stderr triples: **0 `ok`→`error` and 0 other output
differences**, without exclusions. The increase from 601 is the two added
`rust/fslc/tests/fixtures/ai_project_inline_invalid_rule.fsl` and
`rust/fslc/tests/fixtures/ai_project_later_invalid_rule.fsl` files, not an
acceptance change. The earlier three added
`rust/fslc/tests/fixtures/issue_1547_guard_reservations/` files `a.fsl`, `c.fsl`
and `c_if.fsl` and the earlier added
`rust/fslc/tests/fixtures/issue_1425/interleaved.fsl` remain included.
Both sides have 402 exit-0 inputs, 195 exit-2 inputs and 6 exit-1 inputs.

The original pre-migration inline sources were rematerialized independently
of the migrated tests: **126+24+2=152** `ok`→`error` forms, **126** fresh forms
remaining `ok`, and **5** `error`→`error` diagnostic-priority changes. Additional
specimens extracted from the fixed regression source give **70** more removal
witnesses: **6** implicit-name/bound-use/nested-init forms, **60** argument forms
(six expressions × five use/erase/duplicate bodies × direct/composed), and
**4** unused-definition/body/parameter forms. Their **70** const-renamed controls
and **2** hidden-KPI collision/control sources remain `ok`. These are bounded,
case-addressed witnesses, not byte-unique programs or a claim that every check
was newly introduced by the latest repair.

A no-collision KPI source also has **byte-identical exported Kernel v1/v2**
across the binaries, twice, with the same input file identity, cwd and arguments;
no fields are removed or normalized. Kernel v2's file/provenance fields make
matching source identity essential to that full-byte comparison.

The measurement commit is an ancestor of this documentation update. Its Rust
implementation sources and swept corpus inputs are identical to those of the
documentation update; no rebase-only hash is substituted for an unperformed
run. This bounded corpus result is separate from the inline removals and is
not an exhaustive inventory of every removed program.

## 5. Retained pattern reservations (#1573)

This amendment supersedes section 3's deliberately limited Is/Binary collision
inventory and guard-name preservation decision. Visibility is unchanged. The
checker and compose hygiene in the #1573 amendment compensated for the then-existing
evaluator binding lifetime; that amendment did not repair the evaluators. Its
pending evaluator work (#1622/#1577) is addressed by the subsequent
[evaluator lifetime repair](#shared-policy-and-sibling-consumers) above. The
reservations remain in force; more precise clause locations remain separate
work in #1550.

### Invariant and shared implementation

For sync actions and the non-init paths covered here, the invariant is:

**合わせた kernel を syntax に戻して検査し直すツールは、source の判断に頼らない。
syntax の上で名前を離す。**

In those paths, compose must leave distinct binding identities in the lowered
syntax, not only in transient source-check decisions. Structural consumers discard those
decisions when converting a Kernel back to syntax: reachable/invariant
counterfactuals used by explain and HTML, and mutation oracles that build a
materialized candidate again. Preserving an original checked model alone does
not repair the changed candidates, and skipping their checks is not a repair.
Their source capture protection therefore uses the same widened name collector
as collision reservation. There is no separate inventory mode or explain bypass.
The existing hygiene `Scope.locals` remains the reservation set; moving it into
another phase is unnecessary because it already uses the shared collector.

`extend_pattern_names` extends its collision traversal through not and all
expression-if children, without changing its visible-export mode.
`extend_statement_pattern_names` summarizes assignment values and the condition
and both arms of statement-if, delegating expression names to that collector.
The summary performs no rejection and exports nothing from statement-forall
candidates. Native validation and hygiene consume those same summaries while
advancing statement cursors. Each opposite statement arm starts from its own
copy of the condition's reservations, rather than the previous arm's cursor.
Ensures retain the final guard phase, not the action body's statement cursor.
Direct model construction carries one phase through the logical init sequence
and invokes the same statement validator after its existing assignment checks.
Its per-statement wrapper still owns init/inline-initializer, const/map-key and
component-origin diagnostics. Public Kernel revalidation is not the only init
gate. The native borrowed-AST checker and owned-AST source rewriter retain their
separate condition/where/cursor wiring and diagnostic-owner loops; extraction
and outward statement summaries, not all traversal wiring, are shared once.

The direct guarantee includes all init blocks and inline initializers in one
logical sequence (all inline initializers before explicit init). Two direct
regressions cover separate init blocks and inline→init; renaming only the later
binding/read restores acceptance and genuine four-consumer violations of P.
Their composed false verified results are deliberately not regression expectations.

Compose still has a distinct same-source init check gap: per-fragment hygiene
resets its Scope, while name-keyed Fresh decisions can bypass the later model
cursor. Inline→init is affected as well. Common compose init can inherit an
unrelated component action's Fresh decision even in a single block. These are
illegal rebindings, not an exception to the language rule. At the #1573 amendment,
their false verified results were deferred to the evaluator-lifetime family in
#1622. The subsequent [lifetime repair](#shared-policy-and-sibling-consumers)
corrects evaluation without closing these admission gaps. The #1573 scope
decision deferred compose init-fragment reservations to a separate issue after
merge, independently of the init evaluation defect described below.

Independent-source init separation was outside the #1573 amendment. At that time,
concatenating A's init match of some(1) and B's same-named match of some(2) could
make b.P incorrectly violated even though B alone verified. This pre-existing
reproduction was deferred to #1622, alongside the sync-action capture that #1573
hygiene compensated for. It was not a preserved bad-verdict oracle: the
later-name-only repair checked conformance, while the raw unresolved fixture
checked only acceptance. The subsequent
[evaluator lifetime repair](#shared-policy-and-sibling-consumers) now tests actual
later-payload truth in symbolic and concrete composed initializers; it does not
relax the source reservations or close the separate compose admission gap.

### Breaking removals and repair

The following are breaking removals of previously accepted name reuse. A
collision reservation does not make the name readable.

| Reservation flow | Rejected reuse | Repair |
|---|---|---|
| not / expression-if → later operand, requires, body or ensures | A later match binds the earlier retained name | Rename the later declaration and its reads |
| Expression-if condition → either arm; **then → else** | The arms bind the same name, including inside a def or argument expression | Rename one arm's binding |
| Quantifier/aggregate where → body/of | A candidate's body binds a name already reserved by its filter | Rename the body/of binding |
| Statement-if condition → either arm or a later statement | A later match reuses the condition's name | Rename the later binding |
| **Inside either statement-if arm → later statement**, including nested arms | A later statement reuses a possibly retained arm binding | Rename the later binding |

Four further breaking-removal routes retain the two original generated-inline
#1442 nested-if examples, each standalone and composed: an outer condition's
`u` is rebound in a nested then statement or expression. They now reject with
the complete collision packet at 5:39/5:53. Separate controls change only the
inner declaration to `w`; they remain accepted and genuinely violate Seen in
BMC, induction, explicit and Monitor. They are preservation controls, not the
old rebinding-dependent visibility detectors, and the original sources remain
in the base/head sweep.

For example, replace the later `(p is some(v)) and v == 2` with
`(p is some(w)) and w == 2`. Do not replace a binding with an out-of-scope read.
The then-to-else reservation of an **expression** if is intentional. It was
introduced for the symbolic evaluator's shared arm binding map (#1577) and is
retained after the subsequent [lifetime repair](#shared-policy-and-sibling-consumers).
Statement arms are separate evaluator contexts and may still bind the same
name independently. Quantifier/aggregate candidates also remain independent.
Count, unique and exactlyOne have only a filter, not an independent body/of;
their repeated filter bindings were already rejected.

Range/Collection count and sum normalization builds repeated conditional copies.
Authored expressions are checked first; only the generated re-check suppresses
duplicate-binding rejection via the existing substitution phase. It still checks
types. This is not permission to reuse a name in the authored where/body pair.

The collision diagnostic remains exit 2, kind semantics, with the existing
`pattern binding 'v' in 'is some(v)' shadows a name already in scope ...; rename
the pattern variable` message. The original and renamed acceptance pairs pin
the full message and containing clause/arm location, including action-level
requires/ensures locations. The existing #1547 earlier-error controls are preserved.
With #1515's const-shadow gate, an earlier newly rejected pattern reuse may now
precede a later const-shadow binder or let: the direct diagnostic changes from
const to pattern at the same clause/action location. A component expression can
move from the parent's use location to the source pattern's clause location.
The inverse order, pre-expansion erased-argument/unused-def checks, and leadsTo
definition capture retain their complete existing packets. The mixed-priority
fixture pins all twelve direct/compose routes; these are error-to-error priority
changes, not additional acceptance removals or new token-level span precision.
For each of the three changed routes, alpha-renaming only the later pattern
declaration and its bound reads restores the complete base const-shadow packet.
The repair controls preserve the wrapped component use location 2:3 as well as
the message; a live count requires all three controls to execute.

With #1638's preserved-progress capture gate, the existing map-expression type
check runs before the progress preflight. A map with retained-name reuse and a
mapping/abstract-binder capture therefore changes from the declaration's capture
error at 1:130 (span 129..136) to the earlier map-expression collision. Both remain
kind type/exit 2. When that expression error has only the unknown zero-width span,
it now uses the map declaration span when available: the regression's map keyword at 1:33,
span 32..35. Meaningful expression spans are preserved. Renaming only the later
q-pattern/read z→w restores the complete base capture packet, including 1:130 and
its nonempty span. This fourth priority row is separate from acceptance removals.
The same origin repair updates the existing enum-to-numeric-set map rejection
from the unknown 1:1 origin to its authored map keyword at 4:3, span 40..43;
the error message, type kind and exit 2 remain unchanged.

### Source-separated syntax is an output contract change

The two guard-output fixtures under
`rust/fslc/tests/fixtures/issue_1547_guard_reservations/` now render independent
bindings as `__v_substitution_0` and `__v_substitution_1`, instead of both using
the authored `v`. Their explain result remains explained and their ordinary
verification verdict remains verified. Full explain and HTML comparisons permit
only this observed name substitution, not loss of counterfactuals or witnesses.
The two explain goldens are regenerated from the native output.

Mutation is also an acceptance anchor, not just a successful CLI exit. Against
the recalibrated baseline `299ac3df`, c.fsl has killed 14 / survived 12 / invalid 0
and c_if.fsl has killed 7 / survived 8 / invalid 0 at depth 4. Keeping the old
narrow source
inventory makes otherwise valid candidates fail their new collision check:
invalid becomes 24/13. The regression test checks the outcome counts,
and the fault that removes not/conditional from the source inventory must fail
that test. These exact counts are baseline-specific and require recalibration
when the mutation oracle or baseline changes.

### Independent-source payloads: before and after

The source inventory also includes statement summaries before sync merges the
sources. Without that protection, source A's `some(1) is some(v)` can leave its
payload for source B's independently authored match of some(2). Check still
accepts the composition, but all four consumers can falsely report that B's
`v == 2` property is violated. With distinct syntax-level names, B receives 2
and the property holds: BMC verified, induction/explicit proved, Monitor
conformant with final `b.flag == true` after the joined action.

The check envelope is a preservation control; it is not a detector of this
payload capture. The semantic detector is red even on base. A genuinely false
B condition (`v < 1`) remains violated/nonconformant on base and after hygiene,
so the change is not an unconditional promotion to holds. Removing only the
source inventory's statement summary must make the positive detector fail.
These are the same binding-lifetime effects for which statement-if reservations
compensate; evaluator behavior is not changed here.

The executable contracts are in `issue_1573_pattern_reuse.rs`: the original
direct/compose matrix is retained, nested arm-to-later cases are added,
Range/Collection normalization and two-source check envelopes are compared in
full, and a conditional-source/conjunctive-source probe requires the foreign
guard to enable the action. New-base product verification must repeat the
base/head corpus, static-literal and authored-input comparisons rather than
reusing observations made before a rebase.

## 6. Requirements field-pattern freshness (#1561)

Decision 4 also applies to source entity fields and relation roles where
requirements lowering replaces their bare names with generated state-map reads.
The same admitted pattern and continuation must have the same binding identity
in event lowering and the rule-guard/solver copy. Renaming the field to
`claim_amount` before checking the authored pattern `amount` lost that collision.
Reject the ambiguous declaration before substitution and guard-conflict analysis;
do not add pattern-over-field shadow semantics or change the evaluators.

The source preflight reuses the complete pattern-name collector and the shared
`reject_pattern_binding_name` rule. It covers legacy transition guards, event
allow/never guards, assignment RHS, rule-property and measure conditions,
decision assumptions and rows, and concept conditions even when unused.
Relation roles join the existing field inventory. Event Typed and
Range input names use one extraction helper for admission, lowering and guard
spelling; typed solver-domain metadata remains separate. No Range event grammar
is added. Ordinary quantifier/aggregate field shadowing remains legal.

The diagnostic is exit 2, kind semantics, with the existing message shape:
`pattern binding 'amount' in 'is some(amount)' shadows an entity field of the same name; rename the pattern variable`.
The retained owning span, rendered through the shared type-diagnostic conversion,
supplies the source-file suffix and JSON loc: rule statement for allow/never,
transition/event for guards and assignments, property statement, measure or
concept declaration, and decision/row owner for those siblings. No pattern-token
span is reconstructed. Used concepts retain their earlier event/property/measure
owner when that lowered use is checked first.
The original #1552 helper now locates its allow statement at 12:5 instead of
its event at 10:3. Check and all three verification engines agree in full on the
new refusal packets. These errors precede solver conflicts intentionally.

Authored arguments are scanned before def expansion can erase or duplicate them.
Definition-local patterns introduced only after field substitution are not
source field aliases: the def-local `some(x) is some(amount) and amount == 1`
still reads its payload and enables the event. A duplicated argument can still
fail the existing repeated-pattern rule after renaming; a field repair is not
permission to bind the fresh name twice. The dedicated CLI regression covers
these boundaries, nested expression children and each substitution sibling.

### Measured removals and state identity

The comparison ran `FSLC_UNSTABLE=fsl5 FSLC_CACHE=off <binary> check <form>` twice
in each own materialized tree. BASE is
`f8e3726e2ead231c89cd4f52eef3dd5d652c774d`, binary SHA256
`e34ea5b8bddbf8573ec7b1d8e4e55d3b4c516d65b18f39d610eeafef6d3ec3e6`.
Measured HEAD is `13676f0bca83b2433cc974468bbb00c59aef25a9`, built afresh after
the source/test commit. Its production Rust diff SHA256 is
`1f84fd80a10a86e8ac447d3334223017fc90230132ed703028bcf4779cb93c8d`,
full tracked Rust diff SHA256
`019dd0e14f26a6e34b60072086a85343a64d37d480d1453fcd2537b4209489bd`,
and binary SHA256
`0cab1c914a05344d8958a6f9bd5ce5a17618f87f9f7e74e279b449fbca47f314`.
These hashes and every input stayed fixed across both repetitions. This
measurement must not be relabeled as a run on an unmeasured later commit.

| Group | Forms | ok→error | Other full-output changes |
|---|---:|---:|---:|
| All specs/examples/CLI fixtures | 610 (23/195/392) | 0 | 0 |
| All original #1552 source-helper products | 63 | 1 | 3 |
| All prior #1561 authored products | 49 | 23 | 1 |
| Head-added #1552 fresh coverage control | 1 | 0 | 0 |
| Decision/concept revision products | 6 | 2 | 1 |
| Exact review reproduction and fresh repair | 2 | 1 | 0 |

All original helper products were retained: 23 detector pairs including event-input
controls, ten preservation controls, five literal calls and two loop cells.
All 49 authored products of the initial implementation were byte-preserved,
including fresh controls. The revised manifest adds the six decision/concept
products, the head-added fresh control and the exact two-source review pair.
No other existing required test input was rewritten, removed or skipped. The
2,924 executions (731 forms, both binaries, twice) compare full stdout/stderr/exit triples with no excluded fields
and zero repeat differences. Corpus removal set is empty, not an assumption from
the narrower specs/examples CI sweep.

The **27 case-addressed inline removals** (not necessarily byte-unique programs)
are enumerated below. All are exit 0→2; rename the colliding declaration and its
payload reads, not ordinary field accesses. The fresh repair of the original
`some(1) is some(amount) and amount == 1` uses Kernel `var v`, and approve is covered.

| Removed original/authored case | Count / source shape |
|---|---|
| `control_is_pattern_field_name_satisfiable_matches_base` | 1: direct some(1)/amount continuation |
| `expression-0`, `expression-1`, `expression-2`, `expression-3`, `expression-4`, `expression-5`, `expression-6`, `expression-7`, `expression-8`, `expression-9`, `expression-10`, `expression-11`, `expression-12` | 13: and, nested and, implication, or, not, if then/condition/else, Set/method, quantified body/filter, sum value, count filter |
| `argument-1`, `argument-4`, `argument-9` | 3: retained argument p, erased argument true, indirect erasure ignore(p) |
| `event-set`, `when`, `set`, `never`, `property`, `measure`, `role` | 7: each field-substitution sibling, including relation role |
| `decision-assuming`, `unused-concept`, `review-original-collision` | 3: assumption with/without the extra rule fixture and an unused concept |

Five error→error packet changes are separate: the original and, implication and
quantified-body contradictions, authored `argument-7` (duplicated p and p), and
authored `decision-row`. They now report the earlier source-field collision;
they are not extra removals. All remaining outputs, including fresh controls,
agree in full; no acceptance expansions were observed in this bounded inventory.

Disabling only the source preflight makes the satisfiable refusal detector fail
with cargo exit 101 twice: produced check ok/0 versus expected error/2. Exact
production diff-hash equality before/after restoration and subsequent normal
green controls distinguish the mutation from the measured implementation.
Fresh-name coverage, def-local payload behavior and existing collision/scope
controls are preservation evidence, not detectors of the new admission boundary.

### Decision/concept sibling inventory and revision control

Independent review found the missing decision-assumption path at the initial
immutable implementation `d8cb4bffb8216058b2ea1a27294a536d34a2249b`:
`decide D "Decision" for Draft Claim { otherwise => Manager approves }
assuming some(1) is some(amount)` still checked ok/0, twice; fresh v also did.
The assumption becomes an invariant independently of the event's guards, so
checking transition guards alone cannot cover it. The new refusal test was red
before changing source, with produced ok/0 versus required error/2.

The revised preflight inventories all next requirements expression carriers:
properties, measures, decisions' assumptions/rows and concept conditions have
entity-field context. Roles join the process fields; events and sets survive as
process transitions. Resources and field initializers have no implicit field-read
substitution context; Common/standalone actions do not acquire one merely because
an entity declares that field. Goal/policy forms here carry entity/stage names,
not additional authored conditions. Remaining next metadata contains no Expr.
This extends the same collector/diagnostic, not a second AST validator.

Omitting only decision-assumption scanning fails the new detector twice with
cargo101, producing check ok/0. The entire tracked Rust patch was captured before
that isolated fault and compared byte-for-byte after restoration (SHA256
`9c6612850500294ee893142b2a557d8d8cba9f2a41013e7223e170c801ee6a69`
relative to the initial implementation). The restored revised source was then
committed, rebuilt and swept; no active-fault or old-head observation was retagged.

The final head build explicitly pins `RUSTUP_TOOLCHAIN=1.98.0-aarch64-apple-darwin`
and the 1.98 toolchain's rustc/rustdoc paths through `RUSTC`/`RUSTDOC`.
The same locked wrapper reports cargo/rustc/rustdoc 1.98.0 and Clippy 0.1.98;
final focused and guard controls pass twice, and target Clippy passes.
The preceding target metadata recorded rustc 1.95 after an unpinned Clippy run.
Thus earlier PATH-only probes do not prove a 1.98 Clippy/compiler invocation;
they also do not prove that every earlier build used 1.95. The earlier named-head
binary `6d01651607b84e8d3f5eeffbb29024afbbed9821266b9ea07470303cf02c64e8`
and its measurement are retained, not relabeled. A fresh final sweep of the
explicitly pinned head build reproduces all 2,924 raw stdout/stderr/exit triples
exactly, with the same 27 removals and five diagnostic changes. The base remains
the parent-supplied immutable binary identified above; this does not invent a
compiler identity for its original build or claim a new whole-workspace CI run.
