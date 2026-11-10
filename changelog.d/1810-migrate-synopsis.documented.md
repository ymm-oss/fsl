Documented (#1810): the `fslc migrate` synopsis in `LANGUAGE.md` §7, its
Japanese copy and `skills/fsl/references/commands.md` shows `--edition` as
optional, `[--edition current|next|4|5]` with default `next`, as
`fslc migrate --help` does. Three other synopsis rows now match their `--help`
too: `counterexample export` requires `-o`, and `sweep --instances`/`--depth`
and `domain generate --target` are optional. `DESIGN-migration.md` no longer
says the migrator adds "only" metadata, mapping and default edits: it lists the
`fsl 5` header, the process-form conversion and the derived terminal
(`derived_terminal_added`) as well. No behavior change.
