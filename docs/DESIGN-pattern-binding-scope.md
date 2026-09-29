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
   the action body. That is the existing `docs/LANGUAGE.md` §9 idiom and it
   is not changed.
3. When `v` collides with an enclosing binder — an action parameter, a
   quantifier binder, a `let` — the shadowing is **confined to the
   expression in which the match occurs**. The enclosing name is restored
   outside it. This is the rule `docs/LANGUAGE.md` already states for
   quantifier binders: *"when it shadows an enclosing name (an action
   parameter, say) the shadowing is confined to that scope"*.
4. The collision should additionally be a **located `check`-time error**
   ("pattern variable `v` shadows an action parameter; rename it"), for the
   reason recorded in `docs/DESIGN-def.md`: this repository prefers an
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

Binding *availability* is legitimately order-dependent — `docs/LANGUAGE.md`
already says the binding is available on the right-hand side of `and`. The
*meaning of an already-bound name* changing with conjunct order is a
different thing, and it is what confinement removes.

### Work this decision implies (not done here)

- Restore the parameter protection in the Rust port (runtime, verifier, and
  the type checker's `extend_pattern_binding`) so it agrees with Python.
- Add the located `check`-time shadowing error (point 4).
- Then write points 1–4 into `docs/LANGUAGE.md`'s Option bullet.

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

`docs/DESIGN-def.md` already records the rule as intended design:

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
which `docs/DESIGN-def.md` rejects on the ground that synthetic binder names
would surface in counterexamples. Overturning this decision means
overturning that one, with evidence that generated names in traces are
acceptable.
