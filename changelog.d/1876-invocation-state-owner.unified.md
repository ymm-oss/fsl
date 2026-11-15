Unified (#1876): one command invocation's state -- the dependency findings of
its open `DependencyScope`, the files it read as inputs (the output-overwrite
refusal) and the requirements solver's answers -- has one owner in
`rust/fslc/src/invocation.rs` instead of a thread-local and two process-wide
`Mutex` statics with different scopes. A thread works in an invocation only
when it is handed one (`Handoff::current`, nested; `.outside_scope()`, outer):
the chain steps, the chain workers and mutate's repeated baseline keep their
existing outer behavior, and `main` hands the command's invocation to the
`fslc` worker thread. A requirements question no longer holds a lock shared
with other questions while the solver runs; threads asking the same question
still wait for one answer. Rust API: `DependencyScope` moved out of
`fslc_rust::migration`, `DependencyRecords` is a `Send` handle read through
`lock()`, and `DependencyResolver::native` takes the scope's records. Command
output is unchanged.
