Changed (#1607): symbolic CLI and Worker JSON messages for binder init failures
replace `undefined evaluation` and `integer overflow in sum or undefined summand`
with the concrete source, filter, body, or summand failure, or `integer overflow
in sum` for accumulation overflow. Runtime messages, the ` in init at L:C` suffix,
error kinds, exit codes, and JSON fields are unchanged.
