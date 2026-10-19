Changed (#1171): `fslc`'s shared envelope, error, spec-loading, and display
helpers moved out of `main.rs` into their own modules, so the verification,
causal, and code-audit modules no longer reach back into `main.rs`; each
command's option parsing is now a typed option struct with one parser per
command. Command output, exit codes, usage messages, and help text are
unchanged.
