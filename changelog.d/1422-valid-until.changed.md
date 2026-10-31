Changed (#1422): Causal evidence ingestion now rejects present
`period.valid_until` values unless they are strings with the exact ASCII
`YYYY-MM-DD` shape and a valid Gregorian date, after checking the artifact
digest. Missing freshness remains accepted but excluded from current support;
start/end timing exclusions and review-only assurance are unchanged.
