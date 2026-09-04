Required (#761): `tools/check_ci_validator_inventory.py` now discovers and requires classification for
`tools/check_rust_*.py` in addition to `tests/test_*.py`, reserved as future scope by slice 1
(`docs/DESIGN-ci-validator-inventory.md`, "Scope boundaries"). A new, unclassified harness now fails
`check` closed with `untracked validator module`, exactly as an unclassified `tests/test_*.py` module
already did -- the shape #761 itself demonstrated is otherwise possible (17 harnesses existed with no
record of why any were unwired). Three new `exempt_reason` values (plus the existing
`frozen-python-compatibility`, reused) distinguish the actual reasons found in issue #761's
classification table instead of collapsing them into one:
`manual-developer-run` (5 self-declared "Optional developer-run" harnesses), `frozen-python-compatibility`
(reused; the 7 F1-F7 parity harnesses, whose precise pipeline stage remains `docs/RUST-PORTING.md`'s
record, not this inventory's), `parked-pending-unrelated-work` (1 harness blocked on an unrelated,
currently-parked feature), and `pending-native-migration` (3 harnesses blocked on a tracked,
not-yet-complete migration). This tool establishes only that a classification was recorded, not that
it is correct; `docs/DESIGN-ci-validator-inventory.md` states that guarantee boundary explicitly. The
16 harnesses discovered today were seeded via explicit `--exempt path:reason` pairs matched to #761's
own classification table, not via the filename-pattern default (`default_exempt_reason`), which was
not relied on here. That default is reached both via `--bootstrap` for a genuinely new module and,
pre-existing and unrelated to this change, via an ordinary `generate` for a module whose `wiring`/
`prior` tier falls through every more specific branch (for example a previously `required` module
that is no longer wired anywhere) -- confirmed directly, not merely inferred from the source.
