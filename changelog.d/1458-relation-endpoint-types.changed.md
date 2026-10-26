Changed (#1458, breaking): relation `contains`, `add`, `remove`, and `reachable` now
check source and target arguments against their independently declared endpoint
types. This breaking removal rejects previously accepted mismatched endpoints
before evaluation with `kind: "semantics"`, exit 2, the existing assignability
message and `hint: use endpoints matching the relation's declared source and
target types`. `domain(r)` is now `Set<A>` and `range(r)` is `Set<B>` for
`r: relation A -> B`, including binders over those projections and their public
Kernel projection/binder annotations. Enum-typed projection uses are accepted;
the enum-binder/cycle idioms `not (exists x in domain(r): reachable(r, x, x))`,
`forall x in domain(r): not r.contains(x, Blue)`, and
`forall y in range(r): not r.contains(1, y)` remain compatible with released main.
The change rejects enum/Int confusion. Enum nominality, numeric/range
compatibility, public Kernel schemas
and runtime semantics are unchanged; LSP/Worker inherit the shared checker.
The restacked boundary sweep used BASE
`8e54e862cb809121bcd33de53645795bf94a1df5` and HEAD
`870d03e7aa0be837a43c4c5c15f10d32853941bb`: zero `ok` to `error` transitions
among all 597 corpus forms. All 134 inline fixture occurrences were checked;
#1458 removes 24 occurrences (22 distinct sources) in both modes, 48 mode-form
transitions. The former 46-transition subset is unchanged; the expanded
inventory additionally counts numeric Set assignment below. Three counted forms
(count, enum set-literal equality and matching enum Set assignment) add six
error-to-ok pairs. Each mode/tree ran twice: 5,848 checks, full outputs without
exclusions. See
`docs/design/DESIGN-nested-option-support.md`, section #1458, for commands,
identities and measurements. Removed numeric `N -> N` invariants are
`not reachable(r, Red, Blue)`, `not reachable(r, Red, 1)`,
`not reachable(r, 1, Blue)`, `not r.contains(Red, Blue)`,
`not r.contains(Red, 1)`, `not r.contains(1, Blue)`,
`r.add(Red, Blue).contains(Red, Blue)`, `r.add(Red, 1) != r`,
`r.add(1, Blue) != r`, `r.remove(Red, Blue) == r`,
`r.remove(Red, 1) == r`, `r.remove(1, Blue) == r`;
for `N -> Color`: `not r.contains(1, 2)`, `r.add(1, 2) != r`,
`r.remove(1, 2) == r`. `Red` and `Blue` belong to enum `Color`.
LSP and Worker add duplicate target-only contains/reachable occurrences.
Projection typing additionally removes, for `s: relation Color -> Color` and
`r: relation N -> N`, `forall x in domain(s): not r.contains(x, x)`,
`forall x in range(s): not r.contains(x, x)`, `not domain(s).contains(0)`,
`not range(s).contains(0)`, `forall x in domain(s): x != 0`, and
`forall x in range(s): x != 0`.
The expanded inventory also rejects `xs = domain(s)` for
`s: relation Color -> Color`, `xs: Set<N>` (assignment origin 8:19).
General breaking rule: projection elements and sets take the endpoint type.
Enum or Bool elements are rejected anywhere Int is expected, except for the
pre-existing abs operand-check gap tracked in #1459; their sets are rejected
where numeric Sets are expected: comparison, ordering, arithmetic, min/max,
membership, set-literal equality, Option and pattern payloads, numeric Set state
assignment and refinement maps. The measured fixture list is not exhaustive.
Newly accepted families include count over finite endpoint-typed domain and
range, sum over numeric projections, enum set-literal equality, enum-typed where
filters and conditional projection membership, let-bound projections consumed
as matching typed sets, matching enum Set assignments and refinement maps into
matching enum Sets. Unique over enum domain/range was already accepted on released
main and remains accepted. These additions do not guarantee invariant truth.
