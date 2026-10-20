Changed (#984): the Rust workspace's `dev` profile (and `test`, which
inherits it) now emits only line tables for workspace crates and no debug
info for dependencies, cutting test-executable size and link time. Panic
messages and backtraces still show `file:line` for workspace code; a debugger
no longer sees local variables. The `release` profile, command output, and
exit codes are unchanged.
