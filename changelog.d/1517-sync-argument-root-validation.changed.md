Changed (#1517, breaking removal): root synchronized arguments are typed against their formals before substitution, including arguments for formals the component action never reads, in root header parameter, state, constant and enum context. Numeric carriers stay compatible, and a range mismatch stays a verification result only when the component action reads the argument. Root errors point at the argument with a bare message; action and Kernel metadata do not change.

These forms passed `check` before and are now semantic errors (exit 2):

- Unused formal, bad argument: `true` or `part.h[0]` for `a: K`; an unresolved name (`nosuch`), including one bound only by an additional-item `let`; `part.Red` or `Color.Red` (write `Red`); another component's enum value (`other.tone`) for a `part.Color` formal; a non-Bool condition (`if 1 then 0 else 0`); a root `def` with a badly typed argument (`rp(true)` for `def rp(v: Int)`).
- Used formal whose substituted body happened to type: `put(a in 0..1) { flag = a }` or `put(a: K) { flag = a }` with `flag: Bool` and `true`; `put(a: K) { m[a] = true }` with `m: Map<C, Bool>` and enum `R`.
- Duplicate formals `(a: K, a: K)` with `true, 0`.
- `internal part.put` with `part.put(0)` where the component declares `a: Int`, `a in 1..0`, `a: Nope` or a struct-typed `a: T`.

Migration: pass a value of the formal's type, using a header parameter when it comes from outside the root, or correct the component. There is no automatic migration.

The 610-form on-disk corpus has an empty transition set: base `5564b25ccb37e3477311fde1dae5eb6298eb6b2d`, head `d19e9939d8079042ddc768afe8550c4f402743dc`, `fslc check` stable and FSL5 opt-in, and `fslc kernel --kernel-version 1|2`, twice per side. DESIGN-sync-argument-root-validation.md has the command and the exact forms.

A conditional with a Bool, ranged or named expected type now checks its arms against that type, so `m[if c then 0 else true]` with `m: Map<A, Bool>` names `Named("A")` instead of `Range(0, 1)`. Other conditionals, including an `Int` expectation, still check arms against the first arm's type. This changed no admission result on the measured forms.

Rust API: public `SyncRef` requires `arg_spans`, and fallible `into_validated_syntax` validates before raw conversion. The three migrated CLI callers each add one full core validation.
