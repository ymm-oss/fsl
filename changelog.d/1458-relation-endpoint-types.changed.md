Changed (#1458, breaking): relation `contains`, `add`, `remove`, and `reachable` now
check source and target arguments against their independently declared endpoint
types. This breaking removal rejects previously accepted mismatched endpoints
before evaluation with `kind: "semantics"`, exit 2, the existing assignability
message and `hint: use endpoints matching the relation's declared source and
target types`. `domain(r)` is now `Set<A>` and `range(r)` is `Set<B>` for
`r: relation A -> B`, including collection binders and public Kernel type
annotations. This restores valid enum-relation idioms and rejects enum/Int
confusion. Enum nominality, numeric/range compatibility, public Kernel schemas
and runtime semantics are unchanged; LSP/Worker inherit the shared checker.
The restacked boundary sweep used BASE
`e8cf78f0162530e264128f588981d49790f2767f`: zero `ok` to `error` transitions
among all 591 corpus forms. All 122 inline fixture occurrences were checked;
#1458 removes 23 occurrences (21 distinct sources) in both modes, 46 mode-form
transitions. Each mode/tree ran twice, full outputs without exclusions. See
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
