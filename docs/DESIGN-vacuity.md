# FSL — vacuity check implementation design

Motivation: issue #4 (category 5 of roadmap #1). Each of the following specs becomes verified
yet checks nothing: an implication invariant whose antecedent is unreachable (`P => Q` where P
never happens), a leadsTo whose trigger is unreachable (`P ~> Q` where P never happens), and a
requires clause that is always true in reachable states (a dead ornament). Vacuous verified, on
par with under-constraint, is the biggest source of missed bugs. Conventionally `kind:"vacuous"`
referred only to init unsatisfiability.

## 1. CLI

`fslc verify <f> [--vacuity warn|error|ignore]` (default warn, the same shape as deadlock).
- `warn`: list in warnings (the result stays verified / proved)
- `error`: on detection `{"result":"error","kind":<detected kind>,"findings":[…]}` → **exit 2**
  (no counterexample trace, so not violated/exit 1, in the same family as init-unsatisfiable
  `vacuous`)
- `ignore`: skip the check

## 2. Three checks (only on the verified / proved path)

1. **`vacuous_implication`**: the antecedent of a **user invariant** with a single `=>`
   directly under `forall*` does not become sat within depth K. The existential closure of the
   antecedent is fed to the existing `eval_expr` by wrapping the AST with
   `("exists", binder, A)` (no new evaluator). Implicit `_bounds_*` are out of scope (Seq
   live-prefix is in implication form, and warnings on auto-generated items would be noise).
2. **`vacuous_leadsto`**: check the leadsTo trigger P with the same existential closure.
3. **`always_true_requires`**: for each requires clause j, warn if **with the context of the
   preceding clauses** `sat(clause 1..j-1 ∧ ¬clause j)` is unsat over all reachable states ×
   all instances. The reasons for using context are (a) consistency with Monitor short-circuit
   (BUG-020), (b) detecting redundant clauses too (`st!=Cancelled` after `st==Paid`), and
   (c) spurious sat from the whole-domain Z3 encoding of a let-internal partial op works only in
   the "do not emit a warning" direction and is safe. **Coverage-false actions** (already warned
   never-enabled) and **compose-synchronized actions** are out of scope.

### Reason for excluding compose-synchronized actions (important)

`deposit_audited = bank.submit_deposit || audit.deposit` inherits `requires a > 0` from both
bank and audit. This is duplication exactly as designed ("each component defends its own
contract"), not a removable redundancy (it is naturally required for audit_log alone). Excluded
not by name guessing but by the sync marker that compose expansion sets (the action dict's
`sync`). Each clause is checked in the right context by the verify of the component spec alone,
so there is zero detection loss.

## 3. Ripple (the verification engine core is unmodified; only a piggyback onto `_bmc_explore`)

- bmc.py: `pending_vacuity` (implication antecedents + leadsTo triggers), isomorphic to the
  `pending_reachables` loop, piggybacks on a single expansion. requires tautology adds the sat
  of "preceding clauses ∧ ¬clause" to the coverage loop. Context-bearing candidates are
  pre-filtered to only "clauses logically implied by the preceding clauses over the type space"
  (`_requires_clause_locally_implied`), excluding bounded false positives from capacity-guard
  families.
- Output: `{kind, name(display name), loc, requirement, message, hint}` in warnings. prove()
  passes warnings through transparently from the base verify. scenarios uses
  `vacuity_mode="ignore"`.
- Successful BMC output remains explicitly bounded via `completeness:"bounded"`
  and `checked_to_depth`. When normal exploration first witnesses a
  reachable/vacuity/coverage fact at the final depth K, `verified` includes a
  saturation hint; this is separate from vacuity findings and only says the bound
  has not obviously reached a fixpoint.
- The hint avoids misdirecting the repair: a tautological requires says not "delete the clause"
  but "judge whether the model is lacking or redundant" (it may take effect at greater depth or
  under induction).

## 4. Tests (tests/test_vacuity.py)

The three warning kinds (display name, loc, requirement) / forall wrapping / context-bearing
redundant clause / two suppressions (coverage false, sync) / not shown on the violated path /
induction pass-through / error (exit 2) and ignore / **corpus zero-false-positive gatekeeper**
(specs/ + examples/ + gallery/valid in one batch). Gallery
`vacuous_implication_warning.fsl` (`--vacuity error`).

## 5. Related

Complementary to #3 forbidden and #6 mutate in detecting under-constraint and empty
formalization. Because it is a bounded check, the warning wording makes "within depth K"
explicit. Roadmap #1.
