the `LANGUAGE.md` §7 synopsis, its Japanese copy and
`skills/fsl/references/commands.md` list every option each row's `--help`
lists, including `explain --lowering`/`--names`, the six `domain testgen`
targets, `check --strict-tags`/`--requirements`/`--edition` and `--deadlock`
on `html`, `scenarios` and the other commands that take it; the
`counterexample export` engines stay `bmc|explicit|auto`. The
`explain --readable` description says the text computes no counterfactual or
witness, that its `Counterfactual scope` line describes the JSON analysis and
that `--depth` bounds only its `Implements:` check. `DESIGN-explain.md`
describes the Rust implementation and narrows what the #1838 equality test
checks, §13.2 says `readings` follow `--names`, and the
`tools/aggregate_changelog.sh` usage says `check-pr` reads `BASE_SHA` and
`HEAD_SHA`. No behavior change.
