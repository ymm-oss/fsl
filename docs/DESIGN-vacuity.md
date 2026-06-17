# FSL — vacuity check implementation design

Motivation: issue #4 (category 5 of roadmap #1). Each of the following specs becomes verified
yet checks nothing: an implication invariant whose antecedent is unreachable (`P => Q` where P
never happens), a leadsTo whose trigger is unreachable (`P ~> Q` where P never happens), a
requires clause that is always true in reachable states (a dead ornament), a frozen ghost
invariant, and a requirements `deadline` whose generated `tick` action is dead because urgency
freezes time. Vacuous verified, on par with under-constraint, is the biggest source of missed
bugs. Conventionally `kind:"vacuous"` referred only to init unsatisfiability.

## 1. CLI

`fslc verify <f> [--vacuity warn|error|ignore]` (default warn, the same shape as deadlock).
- `warn`: list in warnings (the result stays verified / proved)
- `error`: on detection `{"result":"error","kind":<detected kind>,"findings":[…]}` → **exit 2**
  (no counterexample trace, so not violated/exit 1, in the same family as init-unsatisfiable
  `vacuous`)
- `ignore`: skip the check

## 2. Vacuity lanes (only on the verified / proved path)

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
4. **`tautology_over_frozen`**: a user invariant that depends only on state variables no action
   ever assigns to, and is dynamically always true over those frozen values, is a hollow
   invariant. This is a static pre-filter plus Z3 check; it remains pending until final warning
   emission.
5. **`urgency_freeze`**: for requirements `time`/`deadline`, warn only when the generated
   deadline invariants exist, the generated `tick` action has the structural guard
   `requires not(urgent_enabled)`, the deadline age variables are not assigned by non-`tick`
   actions, and Z3 proves `urgent_enabled` holds in every initial state and is preserved by
   every action. This is depth-independent and intentionally incomplete: if the initial or
   inductive proof fails, no warning is emitted.

### Reason for excluding compose-synchronized actions (important)

`deposit_audited = bank.submit_deposit || audit.deposit` inherits `requires a > 0` from both
bank and audit. This is duplication exactly as designed ("each component defends its own
contract"), not a removable redundancy (it is naturally required for audit_log alone). Excluded
not by name guessing but by the sync marker that compose expansion sets (the action dict's
`sync`). Each clause is checked in the right context by the verify of the component spec alone,
so there is zero detection loss.

## 3. Ripple (the verification engine core is unmodified; only a piggyback onto `_bmc_explore`)

- bmc.py: `pending_vacuity` contains dynamic candidates (implication antecedents + leadsTo
  triggers), isomorphic to the `pending_reachables` loop, plus static candidates
  (`tautology_over_frozen`, `urgency_freeze`) that are proven before exploration and carried to
  finalization. requires tautology adds the sat of "preceding clauses ∧ ¬clause" to the coverage
  loop. Context-bearing candidates are pre-filtered to only "clauses logically implied by the
  preceding clauses over the type space" (`_requires_clause_locally_implied`), excluding bounded
  false positives from capacity-guard families.
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

The warning kinds (display name, loc, requirement) / forall wrapping / context-bearing
redundant clause / two suppressions (coverage false, sync) / frozen-ghost tautology /
urgency-freeze positive and deadline-urgency-pattern negative / not shown on the violated path /
induction pass-through / error (exit 2) and ignore / **corpus zero-false-positive gatekeeper**
(specs/ + examples/ + gallery/valid in one batch). Gallery
`vacuous_implication_warning.fsl` (`--vacuity error`).

## 5. Related

Complementary to #3 forbidden and #6 mutate in detecting under-constraint and empty
formalization. The reachability-based lanes make "within depth K" explicit; `urgency_freeze`
is reported only after an initial + inductive proof of the generated urgent condition. Roadmap #1.
