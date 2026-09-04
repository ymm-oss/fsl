Changed (#926): the Actions cache budget audit reports a `generation-coexistence-partial-explanation`
finding alongside `budget-exhausted` when at least one `refs/heads/main` `{sharedKey, platform}` pair
holds more than one live generation (measured 2026-09-04: `rust-native-z3` Darwin/arm64 held two,
driven by `Swatinem/rust-cache` hashing the runner's entire installed-toolchain list rather than
anything this repository controls). This is diagnostic only: `budget-exhausted` itself continues to
judge the same raw physical total it always has (`Math.max(usageBytes, rawSummed)`, unchanged),
because GitHub's budget and least-recently-used eviction act on physical bytes regardless of which
generation this repository considers current. An earlier version of this change instead subtracted
the superseded generation's bytes from judgment; independent review executed two counterexamples
against it (a same-identity pair physically filling the whole budget judged as half; a listing-derived
subtraction applied to an independently-observed usage total already higher than the listing, on the
unproven assumption the two non-atomic observations share the same bytes) and it was reverted before
merge. A `generation-coexistence` entry in the always-reported, non-gating `informational` array
separately states the raw entry count and superseded-generation byte count whether or not the budget
is exhausted. Every non-main-branch entry, including any `refs/pull/*` cache, is still counted
individually and in full for every existing rule, so the original #747 incident shape (many
different refs each holding their own cache) remains fully detected.
