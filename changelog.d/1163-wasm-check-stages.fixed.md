Fixed (#1163): the browser Worker's `check` no longer answers `ok` for specs
native `fslc check` rejects. It now runs the same validity stages in the same
order as the CLI -- `dbsystem`/`ai_component` validation, the
source-diagnostic preflight, Agent analysis, and init write ownership, which
were missing -- through one shared `fslc_rust::check_stages` function, so the
two stage lists cannot drift apart again. The Worker's `verify` now runs
native `verify`'s pre-solver stages in native order: it rejects an Agent
document, validates `dbsystem`/`ai_component` documents, reports an
`implements` failure before solving, and rejects an init that writes one
variable twice. `examples/ai/recursive_support_agent.fsl` is now compared
native-to-Worker in the browser parity harness instead of being excluded.
Native CLI output is unchanged.
