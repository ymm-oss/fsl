Fixed (#854): CI pins `dtolnay/rust-toolchain@1.98.0` across all eleven audited
workflow references instead of the floating `@stable`, so an upstream Rust
release can no longer turn `main` red with no repository change — the 2026-08-20
breakage where rustc 1.98.0 shipped and the scheduled product gate began failing
on a zero-line diff. `release.yml` stays exempt because it pins the MSRV from
`rust/Cargo.toml`'s `rust-version` by action commit SHA. A new
`audit-toolchain-pin` control runs in the required merge-readiness automation
lane and rejects a reverted reference, a floating channel, a split toolchain
across workflows (which would break `merge-readiness.yml`'s restore-only
cache-key match against `ci.yml`), and a comment left citing the old ref.
