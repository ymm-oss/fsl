# FSL — `forbidden` (negative acceptance criteria / must-forbid) implementation design

Motivation: issue #3 (category 4/6 of validation roadmap #1). `acceptance` (must-allow)
re-checks at check time that "this operation sequence passes," but there was no way to express
that "this operation sequence **must be rejected**" (must-forbid). **Under-constraint** such as
a missing guard accepts an "operation that should be forbidden" without breaking a single safety
invariant, so verify stays silent about it. `forbidden` is an independent channel that breaks
that silence (a receiver for cross-validation in which a separate agent writes positive and
negative traces from the NL and action signatures alone).

## 1. Syntax (requirements dialect)

```fsl
forbidden FB-1 "cancellation after shipping is rejected" {
  pay(0)  ship(0)        // premise (setup): all enabled and ok
  cancel(0)              // last step: expected to be rejected
  expect rejected
}
```

A copy of `acceptance_def`. `expect rejected` is an inline marker (unlike `acceptance`'s
`expect <expr>`, it does not evaluate a state predicate). `FB-1` matches the `REQ_ID` token.

## 2. Semantics (concrete Monitor replay, at check time)

- The premise steps `steps[0..n-2]` must all be `ok` (enabled and no violation).
- Success if the **last step is rejected**. Rejection has two forms:
  - (a) **not-enabled** (`requires_failed` / out-of-range `bad_call`) — the **primary use**.
    A "correct prohibition by a guard" that is invisible as a safety invariant.
  - (b) **violation on execution** (`invariant` / `type_bound` / `partial_op` / `ensures`).
    But a reachable violating state ⇒ means **the spec itself is violated under verify**
    (case b is the signal "forbidden is satisfied but the spec is buggy"). The output's
    `rejected_by` carries this distinction.
- Last step `ok` (= accepted) → `kind: "forbidden"` error + `accepted_trace`.
- A premise step is not `ok` → `kind: "forbidden_setup"` (the trace is malformed; not treated
  as success).
- Zero steps → `kind: "forbidden"` error (at least one step is required).

## 3. Ripple (verification engine and Monitor unmodified)

- grammar.py: `forbidden_def` (`expect rejected` inline) + transformer.
- dialects.py: `("__forbidden", …)` collection. model.py: store into `spec["forbidden"]`.
- acceptance.py: `replay_forbidden` / `validate_forbidden`. A copy of `replay_acceptance`,
  differing in "premise all ok / last expected to be ok:False / no `expect` state evaluation."
  Because `Monitor.step()` returns `ok:False` + `kind` for rejection via requires_failed /
  invariant / type_bound / partial_op / ensures, both (a) and (b) are decided from step()'s
  return alone.
- cli.py: `_forbidden_error` wired into both the check and verify paths. bmc.py: emits
  `forbidden_<ID>` (with `rejected_by`) into `scenarios` → for testgen's negative tests.

## 4. Tests (tests/test_forbidden.py)

Case-a satisfied + scenario / accepted → `kind:"forbidden"` + accepted_trace / broken setup →
`forbidden_setup` / case b (rejected_by=type_bound and verify violated) / empty steps / the
verify gate fires before BMC. Gallery positive example (`small_forbidden_guarded_cancel.fsl` →
verified) and incorrect example (`forbidden_op_accepted.fsl` → error/forbidden).

## 5. Related

The dual of `acceptance` (DESIGN-bridge / DESIGN-dialects). Detecting
under-constraint is complementary to #4 vacuity (`always_true_requires`) and #6
mutate. It is the validation workflow's independent negative-example channel.
