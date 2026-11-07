Changed (#1700, breaking): a repeated `domain` declaration that loses a copy is refused at the repeat on `check`, `verify` and every `fslc domain` route (exit 2, located), and `domain expand` writes nothing.

These forms passed `check` before and kept one copy:

- `kind: "name"`: a second `evolve` for one event, which flipped `verify` from `violated` to `verified` by the order of two copies; a second `command` of one name; an `event` that declares a field twice, which let the order of the copies decide `check`.
- `kind: "parse"`, a second copy of a clause that holds one value: `implementation_profile`; aggregate `id`; projection `from`; effect `reliable`, `irreversible`, `idempotency_key`, `correlation_id`, `handles`, `request_event`, `success_event`, `failure_event`, `timeout_event`, `retry`, `timeout`, `outbox`, `inbox` or a field of an effect; `max_attempts` or `backoff` in a `retry`; saga `starts_on`; step `awaits` or `timeout`.

An event field an effect or saga action takes as a formal was already refused; it stays refused at the same place, with the message `duplicate event field 'E.a'` and `kind: "name"` instead of `duplicate action parameter 'a'` and `kind: "semantics"`. An event field no action takes was accepted by #1680 and is now refused. Through an inline `implements` the message names the other file, with `kind: "type"`.

Kinds that read every copy stay accepted, such as two `requires` or two sagas of one name.

Migration: delete the copy that was lost, or merge the two. There is no automatic migration.

The 650-form on-disk corpus has an empty transition set: base `b2cdb394fedcba015261df05587c4721583329e3`, head `7f0b8e43af11f96786e2a72238c70a4a521b4cae`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared. See `docs/design/DESIGN-domain.md`.
