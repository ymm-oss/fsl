Fixed (#1011): `civil_days` now rejects impossible calendar dates such as
`2026-02-31` and non-leap `2026-02-29` instead of normalizing them, so timing
eligibility cannot admit unparsable evidence periods; leap-year and valid-date
controls confirm the fix does not over-reject.
