Unified (#1875): `check` and nine single-root commands admit their root
through `spec_loading`'s owner. `check` admits with `admit_root_source` and
runs in `run_prepared_roots` instead of entering the dependency scope and
calling `migration::admit_source` itself; a requirements read failure under
`--strict-tags` still reports without the dependency findings and edition
metadata. `replay`, `scenarios`, `kernel`, `conformance`, `testplan`,
`explain`, `typestate`, `testgen` and `approval diff` call
`run_admitted_root` instead of expanding `run_admitted_roots` with a
one-element vector by hand. The `--strict-tags` second model load stays.
Command output and exit codes are unchanged.
