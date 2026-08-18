Unified (#819): the two-inode FIFO read-count oracle used by all six #796/#808
CLI-level snapshot controls now lives once, in
`rust/fslc/tests/support/fifo_snapshot.rs`, carrying the cleanup hardening
`issue_796_domain_command_validation.rs`'s independent copy grew during its
own review: nonblocking cleanup reader descriptors, a
`WriterOutcome`/`WriterMode`-driven writer thread, and a `ReapedChild` guard
that reaps unconditionally even when `kill` itself fails. The previously
unhardened shared copy could hang instead of fail when the CLI under test
exited before ever opening the FIFO path -- the exact incident during #813's
development that cancelled a CI job after 30 minutes and hung a local run for
1h33m. A new negative control in `fifo_snapshot_hardening.rs`,
`release_writer_completes_when_cli_never_opens_the_path`, reproduces that
scenario directly and proves cleanup now terminates instead of hanging.
