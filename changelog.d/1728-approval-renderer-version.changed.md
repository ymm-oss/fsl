Changed (#9999): approval records name the target kind's renderer instead of
the `fslc` release. A new record stores `"<renderer-id>/<semver>"` in
`target.generator_version`: `fslc-ledger-renderer/1.0.0`,
`fslc-html-renderer/1.0.0`, `fslc-scenarios-renderer/1.0.0`, or
`fslc-document-renderer/1.3.0`. `approval check` and `ledger --approval`
report `renderer_changed` only when that renderer version changes, so
upgrading `fslc` alone no longer drifts an approval. A 4.x record keeps its
bare crate version (exactly `X.Y.Z`). For it only the renderer comparison is
skipped, so it stays `approved` while every specification, rendering,
claim-set and reviewed-artifact digest matches. Before, it reported
`renderer_changed` on any other `fslc` version. Other values, such as `4.12`
or `v4.12.0`, are still compared. Migration: a consumer that read
`generator_version` as the `fslc` version must read `fslc --version` instead.
