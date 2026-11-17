Unified (#1875): `check` and nine single-root commands admit their root
through `spec_loading`'s owner. `check` admits with `admit_root_source` and
runs in `run_prepared_roots` instead of entering the dependency scope and
calling `migration::admit_source` itself; a requirements read failure under
`--strict-tags` still reports without the dependency findings and edition
metadata. `replay`, `scenarios`, `kernel`, `conformance`, `testplan`,
`explain`, `typestate`, `testgen` and `approval diff` call
`run_admitted_root` instead of expanding `run_admitted_roots` with a
one-element vector by hand. The native scoped, replay and resolver-backed loaders share one
parse/lower, build and requirements-gate composition with explicit scope and
replay policies, and `check --strict-tags` reuses its checked model instead of
loading it a second time.
Command output and exit codes are unchanged.
