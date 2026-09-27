Fixed (#1117): `fslc domain replay` now checks each `domain_event` log row
against the event the model actually raised at that point — the one-hot
`event_<Event>` flag left by the last accepted `command`/`effect_completion`
transition — instead of only checking that the name is declared somewhere in
the domain. An event the preceding command's `decide` does not emit, a
`domain_event` with no preceding transition, and a stale event re-logged
after a later transition are all reported as
`unknown_domain_event`/`runtime_event_emitted_by_preceding_transition`
(`nonconformant`, exit 1) rather than returning `conformance_checked`.
Conformant logs, including outcome events raised by an `effect_completion`,
are unchanged.
