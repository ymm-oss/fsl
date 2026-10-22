Fixed (#1284): diagnostics and `fslc explain` spell a compose component
declaration by its public name. A type-check or model-build error names
`acct.MAXX` (was `acct__MAXX`) and prints a component type as
`Named("t.Amount")` (was `Named("t__Amount")`); readable `explain` prints
parameter and state types as `bank.Amount` (was `bank__Amount`), and the blame
effect text, `explain`'s JSON and `html`'s embedded copy of it, and the
`--export tag-review` effect targets print the assignment root as
`audit.log = audit.log.push(a)` (was `audit__log = …`). A component state or
property is published from the compose structure rather than by rewriting the
physical name. Synthetic names such as `_bounds_<state>` are published from
their parts, so a state that starts with `_` no longer moves the `.`. Single
specs are unaffected. Migration: a consumer that matched an internal
`alias__name` in these messages or texts must match `alias.name`.
