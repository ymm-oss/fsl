Changed (#1322): development and test builds now compile the vendored Z3 with
optimisation (`[profile.dev.package.z3-sys] opt-level = 2`), so cmake builds it
as `Release` (`-O3 -DNDEBUG`) — the same Z3 configuration release builds ship —
instead of `Debug` (`-O0 -g`, Z3 internal assertions). Solver-heavy test suites
run about 4.5× faster and the build directory shrinks from about 26 GB to 11 GB;
verdicts are unchanged (specs and examples swept with check / verify /
induction). Release builds are unaffected. No migration is needed.
