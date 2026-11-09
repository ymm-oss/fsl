Fixed (#1764): the `migrate` multi-file refusal test now reaches the refusal.
Its "safe" input failed `check`, so `migrate` exited 2 before planning the
refused input and the test passed for the wrong reason. The safe input now
checks and is rewritten when migrated alone; the test asserts the
`migration_refused` result, the refused file's `unsupported_double_ampersand`
finding and `written: false`, and fails when the all-or-nothing guard is
removed. Test-only; `migrate` behavior is unchanged.
