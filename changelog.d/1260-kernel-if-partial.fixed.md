Fixed (#1260): `fslc kernel` listed a partial operation in the body of a
statement-level `if` with an unguarded failure condition, so for
`if gate { y = 2 / x }` it printed `x == 0` although the division is never
evaluated when `gate` is false (verify and the runtime already agreed it is
not). The Public Kernel's `partial_operations[].failure_condition` is now
guarded by the `if` condition, `gate and x == 0`, as an expression-level
`if-then-else` already was; an operation in the `else` branch is guarded by
`not gate`, and nested statement `if`s and `forall`s stack their guards
(`forall k: K { if k > 0 { m[k] = 2 / x } }` lists one entry per candidate,
each guarded by `k > 0` with `k` replaced by the candidate). An operation in
the `if` condition itself stays unguarded by it. Kernel consumers that read
`failure_condition` (`testgen`, `testplan`, `document`) see the narrower
condition. Verification verdicts are unchanged, and no spec or fixture in
the repository corpus changes its `fslc kernel` output.
