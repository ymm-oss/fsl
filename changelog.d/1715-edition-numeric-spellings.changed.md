Changed (#1437): `--edition 4` and `--edition 5` are now advertised beside
`current` and `next` on every command that takes `--edition` (`check`,
`verify`, `domain check`, `fmt`, `lint` and `migrate`): in
`cli-contract.json`'s choices, in each command's `--help` text, in the
`fmt`, `lint` and `migrate` usage lines, and on the CLI reference page. A
value outside the four now reports `--edition must be current, next, 4, or 5`
(it said `--edition must be current or next`), still `kind:"usage"`, exit 2.
What `--edition` accepts is unchanged, and outputs still spell the edition
`"current"` / `"next"`.
