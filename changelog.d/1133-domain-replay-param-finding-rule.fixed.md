Fixed (#1133): `fslc domain replay` no longer reports a **type-mismatched log
parameter** as a guard rejection that blames the FSL model. Since #1116 a
value that is not of its declared type fails closed, but the conversion
failure was folded into the same verdict the model's own refusal produces, so
`{"value":"garbage"}` on an integer input came out as
`command_rejected_by_model`/`runtime_command_must_be_enabled_by_domain_model`
with the repair *"change the implementation command path or update the FSL
decide/evolve model"* — advice to edit a `decide`/`evolve` that was never
consulted, since the row was refused before any guard ran. The conversion's
own message was computed and discarded. A conversion failure now takes its
own `failed_rule` on the same `kind` —
`runtime_command_parameters_match_declared_types` for a `command` row,
`effect_completion_parameters_match_declared_types` for an
`effect_completion` row, including the correlation value filled in from
`correlation_id` — and the discarded message is carried in the witness as
`parameter_error` (`parameter 'value' must be an integer`). The repair names
the row and the two things that can actually disagree: the value the
implementation logged, and the parameter's declared type. A parameter the
command does not declare takes the same rule, for the same reason. Genuine
guard rejections and genuine `effect_completion` lifecycle mismatches keep
their existing rules and repair text, and every conformant log is unchanged:
`examples/domain/order_async_effect_replay.jsonl` and the `issue_518_*`
fixtures produce byte-identical output to v4.7.0. The finding `kind` enum in
`schemas/fslc/domain/finding.v0.schema.json` is unchanged; a consumer that
branches on `kind` alone still cannot tell the two causes apart, which is the
new-`kind` option deliberately left to #1133's own follow-up.
