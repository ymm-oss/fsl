Fixed (#1163): the browser Worker's `check` no longer answers `ok` for specs
native `fslc check` rejects. It now runs the same validity stages in the same
order as the CLI -- `dbsystem`/`ai_component` validation, the
source-diagnostic preflight, Agent analysis, and init write ownership, which
were missing -- through one shared `fslc_rust::check_stages` function, so the
two stage lists cannot drift apart again. The Worker's `verify` also rejects
an init that writes one variable twice before it reaches the solver, as native
`verify` does. Native CLI output is unchanged.
