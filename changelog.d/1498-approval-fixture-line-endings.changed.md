Changed (#1498): isolate error-envelope approval fixtures from runner Git
line-ending defaults, preserving strict stderr assertions on Windows. Run all
Windows native solver, verifier, and CLI test binaries with `--no-fail-fast` so
the next main-push product gate reports every remaining failing binary.
