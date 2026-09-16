Fixed (#1061): the #1023 cache-key regression test no longer collides with
itself. Its temporary fixture roots were separated only by a `SystemTime`
nonce, and `{name}` (`inside`, `outside`) is shared by a matrix column's
tests while `process::id()` is constant within a test binary. The clock has
microsecond resolution, so two tests that start concurrently read the same
value, build the same root, and -- because `create_dir_all` succeeds on an
existing directory -- share one tree silently: one test's write overwrites
the other's input and the first `Drop` deletes the other's spec mid-run,
surfacing as an unrelated `io` error. The root now carries a process-wide
`AtomicU64` counter, so uniqueness comes from construction rather than from
clock resolution, and it is created with `create_dir`, so a future
collision fails where it happens instead of several assertions later.
Measured on the same binary and harness: 3 failures in 30 runs before, 0 in
30 after, with the distinct-root count observed from outside the process
staying at the expected 2 (`inside`) and 3 (`outside`) in every run after
the fix. No product code changed.
