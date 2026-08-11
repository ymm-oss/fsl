Fixed (#747): `main`'s scheduled product gate is no longer cancelled by a
self-perpetuating cache-budget loop. PR #752 restricted `Swatinem/rust-cache`
saving in `ci.yml` to non-pull-request events so pull requests could not keep
evicting `main`'s caches, but `merge-readiness.yml`'s two `rust-cache` steps
(`rust-compile`, `core-contracts`) carried no such guard. Because that
workflow runs on every pull request, each one kept saving a fresh
ref-scoped copy of both keys, and a cache's last-accessed time refreshes on
restore even without resaving, so none of them expired quickly either.
Measured on 2026-08-11: 15 open pull requests held 29 entries (~1.90 GiB) on
those two keys alone, part of 38 entries / 10.03 GiB total against the
repository's 10 GiB budget, and the LRU evictor had removed
`refs/heads/main`'s `native Z3 4.16 (windows-latest)` cache down to zero
entries. `merge-readiness.yml` now carries the same
`save-if: ${{ github.event_name != 'pull_request' }}` guard as every
`Swatinem/rust-cache` step in `ci.yml`. Separately, `rust-native-z3`'s
`windows-latest`/`macos-15` matrix had the same cancel-skips-save deadlock
already fixed for the `semantic-mutation` lanes: a cold build (measured warm
at 27–33 min) exceeded the 40-minute budget, the job was cancelled on all six
consecutive scheduled runs from 2026-08-07 through 2026-08-11
(`9 skipped Post Run Swatinem/rust-cache@v2`, run 31527197290), and the
skipped post step meant no cache was ever written to recover from. That step
now carries `cache-on-failure: true` — confirmed from `Swatinem/rust-cache`'s
own `post-if: success() || env.CACHE_ON_FAILURE == 'true'`, which does not
distinguish a cancelled conclusion from a failed one, and from this
repository's own history (issues #721 and #678 stopped recurring as
`cancelled` immediately after commit `877fe8c` added the same flag to the
semantic-mutation lanes) — and its `timeout-minutes` is raised from 40 to 60
to cover a genuine cold vendored-Z3 build, diagnosed rather than raised
blind. See `docs/DESIGN-ci.md`, "Actions cache budget".
