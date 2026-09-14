Fixed (#1031): the release workflow now runs `cargo test --release --locked` on the
MSRV toolchain (`1.88.0`) before building release binaries, so shipped artifacts are no
longer built from a toolchain that never exercised the test suite.
