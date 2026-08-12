Fixed (#747): `main`'s scheduled product gate is no longer cancelled by a
self-perpetuating cache-budget loop. PR #752 restricted `Swatinem/rust-cache`
saving in `ci.yml` to non-pull-request events so pull requests could not keep
evicting `main`'s caches, but `merge-readiness.yml`'s two `rust-cache` steps
(`rust-compile`, `core-contracts`) carried no such guard. Because that
workflow runs on every pull request, each one kept saving a fresh
ref-scoped copy of both keys. Measured on 2026-08-11: 15 pull-request refs
(#743-#792, all closed or merged by then) still held 29 entries (~1.90 GiB)
on those two keys alone, part of 38 entries / 10.03 GiB total against the
repository's 10 GiB budget -- retained not because anything kept restoring
them, but because GitHub only evicts an unused cache after 7 days without a
read, and measurement landed inside that window. The LRU evictor does not
distinguish "small but many" from "few but large": it had removed
`refs/heads/main`'s `native Z3 4.16 (windows-latest)` cache down to zero
entries. `merge-readiness.yml` has no `push` trigger, so the `ci.yml`-style
`save-if: ${{ github.event_name != 'pull_request' }}` guard would not close
this path -- it would leave every pull request permanently cold, since
nothing would ever save a `main` copy under this workflow's own key for it to
restore. Both `Swatinem/rust-cache` steps instead go restore-only against
`ci.yml`'s own `rust-workspace` key (`shared-key: rust-workspace`,
`save-if: false`): the two workflows' jobs share a toolchain, runner, and
checkout, so the derived cache key is expected to match, confirmed from the
first post-merge run's `full match: true` restore log rather than asserted
from an unverifiable prior observation. Separately, `rust-native-z3`'s
`windows-latest`/`macos-15` matrix had the same cancel-skips-save deadlock
already fixed for the `semantic-mutation` lanes: a cold build (measured warm
at 27–33 min) exceeded the 40-minute budget, the job was cancelled on all six
consecutive scheduled runs from 2026-08-07 through 2026-08-11
(`9 skipped Post Run Swatinem/rust-cache@v2`, run 31527197290), and the
skipped post step meant no cache was ever written to recover from. That step
now carries `cache-on-failure: true`. `Swatinem/rust-cache`'s own
`post-if: success() || env.CACHE_ON_FAILURE == 'true'` reads as supporting a
save through cancellation, not only ordinary failure, but this repository has
not directly observed a cache written specifically after a timeout
cancellation: the semantic-mutation lanes' `cancelled` recurrences (issues
#721, #678) stopped after commit `877fe8c` added the same flag, but that
commit simultaneously raised those lanes' timeouts, confounding the two
causes, and the eventual recovery run saved after reaching `success`, not
after a cancellation. `timeout-minutes` is separately raised from 40 to 60,
diagnosed from measured warm/cold durations rather than raised blind. See
`docs/DESIGN-ci.md`, "Actions cache budget".
