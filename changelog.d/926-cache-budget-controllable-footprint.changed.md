Changed (#926): the Actions cache budget audit's `budget-exhausted` finding now judges a
de-duplicated, `refs/heads/main`-scoped footprint -- at most one generation per `{sharedKey,
platform}` pair, keeping only the most recently created generation -- instead of the raw listing
total. A stale generation left behind by a GitHub-hosted runner-image update (measured 2026-09-04:
`rust-native-z3` held two generations on both platforms, driven by `Swatinem/rust-cache` hashing the
runner's entire installed-toolchain list) is self-healing and excluded from judgment; deleting it
changes nothing the audit judges, closing the incentive the issue's own experiment showed (deleting
a stale generation to silence a false alarm produced `main-cache-absent` instead). Every non-main-branch
entry, including any `refs/pull/*` cache, is still counted individually and in full, so the original
#747 incident shape (many different refs each holding their own cache) remains fully detected. The
raw total and superseded-generation count are still reported, labeled `informational/*` and never
affecting pass/fail.
