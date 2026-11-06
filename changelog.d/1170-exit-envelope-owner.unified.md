Unified (#1170): in `fslc`'s `main.rs`, `causal.rs`, `refinement.rs`, and
`verification.rs`, a pair of `error_output(..)` or `io_error_output(..)`
with a hand-written `2` or `3` that equals the `Verdict` table's code now
takes its exit from `fslc_rust::outcome::error_exit` (266 sites). Other
shapes keep their literal: a bare `(output, 2)`, pairs built from
`surface_parse_error_output` or `semantic_error_output`, `process::exit`,
`"exit_code"` fields, and `run_document_claims`' `internal` error with 2.
The #1170 ratchet counts what is left (`main.rs` 170, `verification.rs` 29,
`causal.rs` 14, `refinement.rs` 3, and the other files' literals). `chain`
builds its nested error details through the `cli_output.rs` envelope owner
and its layer input-error records through `chain_step_input_error`; its
failed refine-run and `[impl]` command records are unchanged. Command
output, error `kind`s, messages, and exit codes are unchanged.
