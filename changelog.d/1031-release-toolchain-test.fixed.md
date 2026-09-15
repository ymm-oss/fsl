Fixed (#1031): the release workflow now runs `cargo test --release --locked -p fslc-rust
-p fsl-lsp` on the MSRV toolchain (`1.88.0`) before building release binaries, so shipped
artifacts are no longer built from a toolchain that never exercised their tests; library crates
remain covered by PR/main CI at `1.98.0`.
