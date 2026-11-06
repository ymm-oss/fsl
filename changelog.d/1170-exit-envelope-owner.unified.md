Unified (#1170): `fslc`'s command arms in `main.rs`, `causal.rs`,
`refinement.rs`, and `verification.rs` take their error exit from
`fslc_rust::outcome::error_exit`, which reads it off the `Verdict` table,
instead of a hand-written `2` or `3`, and `chain` builds its nested error
details and failed-layer records through the `cli_output.rs` envelope owner.
The #1170 ratchet's exit-literal and envelope pins drop to the counts left.
Command output, error `kind`s, messages, and exit codes are unchanged.
