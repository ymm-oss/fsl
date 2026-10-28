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
4. The collision should additionally be a **located `check`-time error**
   ("pattern variable `v` shadows an action parameter; rename it"), for the
   reason recorded in `docs/design/DESIGN-def.md`: this repository prefers an
   explicit local repair over silently changing meaning. See "Work this
   decision implies" below — the error is not implemented yet.

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
  guards and the action body. Existing lexical scopes and collision checks
  remain in force; no pattern name becomes global.

Previously accepted uses on the right of `or`, after a disjunctive or
implicational antecedent, across comparison operands or out of comparison
results, and bindings escaping negation, quantifiers or aggregates are breaking
removals. Pattern names from an `if` condition are unavailable in both arms.
An out-of-scope `v` reports `kind: "semantics"`, exit 2, with the existing message
`invalid model expression: public Kernel cannot type identifier 'v' at <line>:<column>`.
Its location is the containing clause or `if` arm; inside `requires` and
`ensures`, it is the action location, matching existing type errors. More precise
guard/postcondition clause locations remain separate work in #1550.

The original baseline accepted
`((o is some(v)) or x == 0) => v == 1` with `o = none` and `x = 0`;
BMC and induction could report `violated` using the unbound payload name.
Rejecting the source is necessary: that verdict cannot establish a valid
counterexample for an ill-scoped formula. Evaluator semantics are unchanged.

### Shared policy and sibling consumers

`fsl-core::pattern_binding_flows(op, right_operand)` is the single operator
decision. The initial eight-consumer estimate omitted the separate free-name
walk; all nine consumers are now coupled:

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

The compose guard-name set is a syntactic capture-protection inventory, not a
visible-value scope. It uses the existing general pattern-name collector so
restricting visible bindings does not remove capture protections. Six focused
sync/foreign-argument controls (ordinary, disjunctive, negated, quantified,
conditional and aggregate patterns) stayed accepted on base/head in two runs;
this is bounded evidence, not an exhaustive admission-equivalence proof.

The 45-source position matrix, 18 public-entry probes and two supplemental
def-call sources include rejecting controls and legal `and`/`=>`/`requires`
preservation controls. On base `091cba49`, inverting each consumer separately
was detected, with exact source restoration between runs. The original def-RHS
mutation survived the first matrix; the two supplemental def-call sources then
detected it. Publication/AST preservation controls detect some inversions but
are not independent negative-admission semantic lineages.

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
BASE `1844fa74a717ba9229e0b292d9421c0939016e5e`, implementation/test/comment
HEAD `b47b72f06ec214038f57205699830e91a11b058a`. Base binary SHA256 is
`1a4c1dc20c31fda70f76164fe2af40b7ad8be7e7a11d7438c2c4d135f574b599`;
head is `c7037351f28169855eb7963a5119f3e244001a15a4a0d94b4f8c5ea227783807`.
Two complete runs compared stdout, stderr and exit code, with no excluded fields
and zero repeat differences. Documentation-only successors do not change these
measured native sources.

| Group | Forms/routes | Base exits 0 / 1 / 2 | Head exits 0 / 1 / 2 | `ok`→`error` | Full output changes |
|---|---:|---:|---:|---:|---:|
| Corpus: `specs/`, `examples/`, `rust/fslc/tests/fixtures/` | 597 | 400 / 6 / 191 | 400 / 6 / 191 | 0 | 0 |
| Existing static complete Rust-test document literal occurrences | 1,713 | 982 / 0 / 731 | 982 / 0 / 731 | 0 | 0 |
| New authored sources: 45 main + 2 def-call, direct/compose | 94 | 66 / 0 / 28 | 10 / 0 / 84 | 56 | 58 |

The existing-test inventory scans 522 base / 524 head Rust test source files
(integration sources plus test-attributed unit scopes). It decodes raw/escaped
literals and static escaped format braces, but does not substitute interpolated
placeholders or complete partial templates. Another 1,737 document-like
candidates are inventoried, not executed: 214 interpolated formats, 1,522
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

The other two full-output changes are `def-call-or-free-name`, direct and
compose: both stay `2`→`2`, but report the out-of-scope `v` at `7:3` rather than
the earlier call-capture diagnostic. The conjunctive def-call capture control
retains its existing diagnostic. There are no observed acceptance expansions
or removals outside the approved scope.
