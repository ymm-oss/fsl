Changed (#1170): `fslc`'s file-IO error classification has one owner,
`spec_loading.rs`, and every command arm that reported an `io` error now
builds it there. A new test counts the hand-written IO, error-envelope,
and error-exit sites left outside their owners and fails if one is added.
Command output, error `kind`s, messages, and exit codes are unchanged.
