Fixed (#1117): `fslc domain replay` now checks each `domain_event` log row
against the event the model actually raised at that point — the one-hot
`event_<Event>` flag left by the last accepted `command`/`effect_completion`
transition — instead of only checking that the name is declared somewhere in
the domain. An event the preceding command's `decide` does not emit, a
`domain_event` with no preceding transition, and a stale event re-logged
after a later accepted transition are all reported as
`unknown_domain_event`/`runtime_event_emitted_by_preceding_transition`
(`nonconformant`, exit 1) rather than returning `conformance_checked`.
Conformant logs, including outcome events raised by an `effect_completion`,
are unchanged. A mismatch names its cause rather than guessing: an event only
a saga step, step timeout, or compensation emits gets
`failed_rule:"runtime_event_reachable_by_replayed_transition"` and repair
text saying no log can match it and the FSL model must not be changed; a
mismatch after a transition the model **rejected** names that rejection; and
only a genuine ordering or model gap proposes adding the event to the
transition the log ran. The finding `kind` enum in
`schemas/fslc/domain/finding.v0.schema.json` is unchanged. What the check
does **not** cover, and what therefore keeps #1117 open: **payloads are not
compared** — `command SetN {v:1}` followed by `domain_event NSet {v:0}` still
conforms, because only the event name is matched; the **`aggregate` field of
a `domain_event` row is not read**, so a row naming the wrong aggregate
conforms as long as the event name matches (the flag is keyed by event name
across the whole domain, and a domain where two aggregates declare the same
event name is already rejected at lowering with `duplicate state variable`);
and a **rejected** `command`/`effect_completion` rewrites nothing, so a stale
event row after one still conforms — the rejection itself is the reported
finding. `events_observed` changes meaning accordingly: a `domain_event` row
enters it only once its occurrence flag matched, so it lists the events the
model raised rather than every declared name the log mentioned.
