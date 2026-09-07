Documented (#926): recorded the 2026-09-04 re-measurement of the Actions cache budget audit
(9.07 GiB / 91%, above the 85% threshold) in `docs/DESIGN-ci.md`, "Actions cache budget" --
the issue's originally-reported generation coexistence (`semantic-mutation`/`rust-workspace`/`wasm`)
no longer reproduces, but the same mechanism now affects `rust-native-z3` Darwin/arm64 instead,
driven by the GitHub-hosted runner's ambient installed-toolchain list rather than by anything this
repository controls. Recorded why a coexistence-detecting audit rule was rejected (its easiest
passing implementation is the prohibited manual deletion), why `shared-key` does not apply (only one
job builds this cache), and that the cache's size reflects the vendored Z3 C++ build rather than
accumulated debt.
