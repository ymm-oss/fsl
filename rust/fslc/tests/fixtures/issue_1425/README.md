# #1425 pre-fix mutation stdout

These are raw, unmodified stdout captures from native `fslc` built at main
`45a2de330d34aa1cbe3e08525bdbd416b66a51e3`, without production changes.
The binary was `rust/target/debug/fslc` (debug, macOS), SHA-256:

```text
751d19897ed319c9499aa7f9b5a0a749e9bca8521ee72191b61a5f4dcc6a8b07
```

Build provenance: `cargo build --manifest-path rust/Cargo.toml --locked --offline
-p fslc-rust --bin fslc`, Rust 1.98.0, run by the lane before this test-authoring
task. Only new integration tests and these fixtures were added during capture.

From the repository root, with `FSLC_CACHE=off`, each capture used:

```text
rust/target/debug/fslc mutate <spec> --depth <depth> [--max-mutants <cap>]
```

`default` means the option was omitted; the other captures explicitly pass `0`,
`1`, and `2`. All 28 commands exited 0 and returned `result: "mutated"`.

| Capture prefix | Spec | Depth | Default capture time (seconds) |
|---|---|---:|---:|
| issue_524 | rust/fslc/tests/fixtures/issue_524_mutant_cardinality.fsl | 2 | 1.778 |
| issue_848 | rust/fslc/tests/fixtures/issue_848_bank_healthy.fsl | 8 | 0.537 |
| bounded_counter | examples/gallery/valid/tiny_bounded_counter.fsl | 4 | 0.086 |
| traffic_light | examples/gallery/valid/tiny_traffic_light.fsl | 4 | 0.117 |
| turnstile | examples/gallery/valid/tiny_turnstile.fsl | 4 | 0.153 |
| option_equality | examples/gallery/valid/tiny_option_structural_equality.fsl | 4 | 0.060 |
| guarded_cancel | examples/gallery/valid/small_forbidden_guarded_cancel.fsl | 4 | 0.626 |

The five examples cover bounds, guards, enum swaps, nested branches, assignment
removal, ensures, option payloads, and requirements/forbidden trace oracles.
The 524 fixture exceeds the default cap (574 discovered, 200 evaluated, 374
dropped). The new identity test compares complete stdout bytes without field
exclusions or normalization; these are preservation controls, not memory
detectors. Do not regenerate them from the post-fix binary merely to make a
changed output pass.
