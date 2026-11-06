Changed (#1170, #1709): an input file that `fslc` cannot read (missing, a
directory, no permission, not UTF-8) is now reported the same way on the
routes below: `kind:"io"`, exit 2, and the message `<path>: <error>`. Routes that
change: `approval create --kind requirements_document` with an unreadable
`--artifact`, `--glossary` or `--evidence` (was `kind:"usage"`); the spec of
`domain analyze`/`check`/`expand`/`generate`/`replay`/`testgen` and
`db observe` (was `kind:"semantics"`, retiring the #780-era exception); the
bare OS error without the path from the spec of `ai check`/`compat`/`drift`/
`eval`/`regress`/`replay`, `db import`, `diff` (both specs and `--mapping`),
`refine` (the mapping), `fmt`, `replay --trace` and
`--from-log --mapping`, `approval check`/`diff --record` and `--trust-key`,
`approval create --artifact` and `--signing-key`, `ledger --impl-log`/
`--evidence`/`--approval`, `document check` (the document), `mutate --from`,
`verify --from-state` and `--requirements`, and `causal observe-expectations
--from-log`/`--mapping`/`--scope` and `causal ledger --plans`; and the causal
family's `cannot read <path>: <error>` spelling (the spec of every `causal`
subcommand and `causal analyze`/`ledger --evidence`). A file read from
a `diff --git` or `approval diff` checkout is named by the caller's path or
`<commit>:<path>`, not the temporary tree. A missing file that already
reported `file not found: <path>` keeps that message, and only a missing one:
any other `chain` manifest read failure is `<path>: <error>`. An unreadable
import or `compose` component stays `kind:"semantics"` with its `loc`, and `ai` log
and record inputs, `ai compare --from` and `db observe --trace` still report
an unreadable file as `kind:"parse"`. A new test pins one case per changed
route.
