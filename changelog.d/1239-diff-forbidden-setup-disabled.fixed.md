Fixed (#1239, breaking): `fslc diff` no longer reports an OLD `forbidden` as
preserved when a NEW guard disables one of its setup steps. NEW then never
runs the final step, so nothing shows that NEW still rejects it; the forbidden
is now `unknown` / `forbidden_replay_failed` with `step`, `action`, and
`detail: "NEW forbidden setup was not enabled"`, like a NEW setup step that
violates (#1213) and an OLD setup step that is not enabled. Only a NEW final
step that its guard disables preserves the OLD rejection. The same judgment
serves `diff`, `diff --git`, and `approval diff`; no other command changes.
Migration: a diff that passed `--forbid unknown` (or
`--forbid forbidden_relaxed,unknown`) may now exit 1 — for example, a NEW that
adds `requires amount != 10` to the setup `withdraw(10)` and drops the
overdraw guard the forbidden checks. `--forbid forbidden_relaxed` alone still
exits 0 on it (the final step was never run, so it is not a relaxation); list
`unknown` in the gate to catch it. Fix the forbidden's setup so NEW can run it,
or confirm the change and update the forbidden. Corpus sweep: no self-diff
changes (`docs/design/DESIGN-forbidden.md` §2.1 and §6).
