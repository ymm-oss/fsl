# FSL — Integer division `/` and modulo `%` implementation design

Motivation: in the standard technique for flattening two-dimensional
data into a single key, recovering the axes from a cell (`c / SLOTS`, `c % SLOTS`)
could not be written, forcing boundaries to be hard-coded. This completes arithmetic
with `+ - * / %`.

## 1. Syntax

- Binary operators `/` (integer division) and `%` (modulo). Precedence is the same
  product tier as `*` (left-associative). Usable in all expression contexts.
- Caveat (documentation only): writing `a//b` without a space causes `//` to be
  lexically interpreted as the start of a comment, leaving only `a` (the same trap as
  in C). Put spaces on both sides of the operator. Add a one-line caveat in
  LANGUAGE.md §3.

## 2. Semantics (most important — exact agreement of the two evaluators)

1. **Division by zero is totally defined**: `a / 0 = 0`, `a % 0 = 0`.
   - Z3's Int div/mod leaves division by zero uninterpreted (model-dependent), so the
     encoding **explicitly pins** it as `If(b == 0, 0, div(a,b))` / `If(b == 0, 0, mod(a,b))`.
     This guarantees the Z3 side and the concrete side always agree, keeping the
     oracle/BFS safe as well.
2. **In action contexts, an implicit partial_op check applies**: for each `/` and `%`
   appearing in the body, requires, or ensures, the transition is checked for
   `divisor != 0` (same mechanism as Seq's pop/head/at — path-condition aware,
   `violation_kind: "partial_op"`, invariant name `_partial_<action>`, hint
   "guard the division: requires y != 0"). Even though /0=0 is defined, **a spec that
   silently relies on 0 is reported as violated** (G5).
3. **No check in property contexts (invariant/reachable/leadsTo/mapping expressions)**:
   /0 evaluates to 0 (total, so not an indeterminate value). Document the guard idiom
   `y != 0 => P(x / y)` as a recommendation.
4. **Negative numbers follow SMT-LIB (Euclidean)**: when `b != 0`,
   `a = b * (a / b) + (a % b)` and `0 <= a % b < |b|`.
   - Z3's Int div/mod use this definition (used as-is).
   - **The concrete evaluator (runtime) disagrees with Python's `//`/`%` (floor) for b<0**,
     so it is implemented with explicit expressions:
     ```python
     def _euc_div(a, b):
         if b == 0: return 0
         q = a // b
         if a - b * q < 0: q += 1   # Python floor → Euclidean correction (when b<0)
         return q
     def _euc_mod(a, b):
         if b == 0: return 0
         r = a % b
         if r < 0: r += abs(b)      # always 0 <= r < |b|
         return r
     ```
   - For typical specs (non-negative domains) this is equivalent to Python, but **a
     property test pins** that both evaluators agree even for negative numbers (§4).

## 3. Ripple effects

- grammar.py: `/` `%` in the product tier. AST `("bin","/",a,b)` / `("bin","%",a,b)`.
- bmc.py: div/mod in eval_expr's bin (the ite pinning of §2.1). partial_op collection
  (add the divisor expression to the existing Seq mechanism; path conditions are the
  same as existing).
- runtime.py: `_euc_div`/`_euc_mod` (§2.4). The short-circuiting of enabled()
  (BUG-020 fix) still works (the body is not evaluated until requires holds).
- Types: the result is Int. Assignment to a bounded variable is still protected by the
  existing automatic bounds check.
- dialects/refine/compose: no extra work since the expression mechanism is shared
  (regression only).

## 4. Tests

1. Basics: a spec using quotient and remainder is verified/proved (e.g., a meeting-room
   spec writing "room r is full" as `c / SLOTS == r` from a flattened cell — a fixture
   based on a meeting-room booking spec that uses a flattened cell domain).
2. **Agreement of the two evaluators (most important)**: for every pair in
   a ∈ [-7..7] × b ∈ [-3..3] (including b=0), the evaluated value of the Z3 encoding
   and the runtime's `_euc_div`/`_euc_mod` agree (either via a small spec or by
   comparing the evaluators directly). Add one division-bearing spec to the existing
   witness-replay diff test as well.
3. partial_op: unguarded `x = 10 / d` (where d may become 0) → violated/partial_op.
   With `requires d != 0` → verified. Also the if-guard form. Confirm that /0 in
   property contexts is not checked (evaluates to 0).
4. Euclidean: pin the negative-number cases (e.g., `-7 / 2 == -4`? → under Euclidean,
   -7 = 2*(-4)+1, so q=-4, r=1. `-7 % 2 == 1`. `7 / -2`: 7 = -2*(-3)+1 → q=-3, r=1).
   Choose values that a wrong implementation (truncation/floor) would fail on.
5. Totality of division by zero: `x / 0 == 0` holds inside an invariant (confirmed with
   an intentional spec).
6. All existing tests (301 passed / 69 skipped) green without modification.

## 5. Documentation

- LANGUAGE.md §3 operator row + the `a//b` comment-trap caveat + add division's
  partial_op to the §6 automatic-check table. Also sync skills/fsl (the SKILL.md
  partial_op rule line, reference.md §3/§6). The standard technique for two-dimensional
  data (F-A) is added separately as an idiom section (after this design is accepted).
