Fixed (#989): causal evidence whose timing eligibility cannot be established
is now excluded from current support, as the applicability rule already
required. An unconvertible observation window, or a claim whose `lag` is
`unknown` so there is no `lag_min` to compare against, previously recorded a
`not_evaluable` entry while the same evidence/claim edge stayed
`applicable: true` and cast its vote, so an artifact whose timing could not be
checked could still decide a claim's `causal_support` between `supported`,
`mixed` and `challenged`. The edge now excludes with
`evidence_timing_not_evaluable`, and the record and the vote agree.

Each independent cause is named rather than collapsed into one sentence:
`period_start_missing`, `period_end_missing`, `period_date_unparsable`,
`period_end_before_start`, `timebase_not_convertible`,
`week_window_not_whole_weeks`, and `claim_lag_unknown`. A window that cannot
convert for a claim whose lag is also unknown reports both, which the previous
single branch dropped. The records now also travel on the evidence/claim edge
itself in `causal_evidence_graph` output, not only in the envelope-level
`not_evaluable` array.

Unchanged: `w == lag_min` still passes and votes, `w < lag_min` still excludes
with `evidence_window_shorter_than_lag`, and no support-vocabulary value is
new -- a claim left with no applicable artifact is
`unsupported_by_current_evidence` exactly as before.

Known limit, unchanged by this fix: a period date is parsed as `YYYY-MM-DD` with a month in
1..12 and a day in 1..31, and day-of-month validity against the month is not checked, so an
impossible date such as `2026-02-31` converts as its normalized value instead of reporting
`period_date_unparsable` — the edge then votes on a window computed from that normalization.
That is pre-existing `civil_days` behaviour, it also governs `valid_until` staleness, and it is
tracked as #1011 rather than widened into this fix.
