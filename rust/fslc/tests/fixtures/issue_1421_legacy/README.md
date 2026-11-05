# Genuine legacy approvals for #1421

The eleven files listed in `capture.json` were produced/read by the actual retained
base CLI, SHA-256 `88a4e85b77f7fd973070f657f92febd706b515d44b8d5338a9e80c41cded72cc`.
`capture.json` has SHA-256
`21927ff97ae3ddd91ee6759b6f8a22ffd62c169e96441f26a2d877f06841a77f`.
Its producer fields distinguish the production base from the materialized
acceptance-test commit. No base executable is shipped with these fixtures.

Do not relabel, re-sign, rebind Git identity, or normalize timestamps/version fields
in these original records. The compatibility test reconstructs their exact Git
commit from the captured LF source and fixed author/committer metadata, and copies
the original bytes unchanged. `nested/` is recreated for the literal
`nested/../evidence-a.json` input; Git does not store empty directories.

The public key is deterministic test-only seed-7 material. A private key is derived
only inside temporary test repositories. Independent canonical signing is
calibrated against both genuine v2/v4 signatures; production signing/validation is
not the test oracle.

Captured renderer version fields remain `4.12.0`. A future renderer/package change
may intentionally change the complete expected approval output. Such a change
requires explicit compatibility review; these tests do not mask that drift or
rewrite the genuine signed records to suppress it.
