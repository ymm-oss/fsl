Fixed (#1580): induction resolves constants and bare enum members in `helpful`
arguments, including integer arithmetic, instead of refusing them as unknown names.
State-dependent arguments remain refused; `check` and BMC behavior is unchanged.
