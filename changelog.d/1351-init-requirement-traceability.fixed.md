Fixed (#1351): `fslc analyze` and enabled LSP structural diagnostics no longer
report an init-only requirement as disconnected, matching `check --strict-tags`
and lint. Init carrying requirement relations now has an `init` graph node with
requirement metadata and `declares`/`covers` edges, retained in requirement and
impact projections, manifest traceability graphs, and DOT/Mermaid exports.
Init without requirement relations leaves graph output unchanged. Genuine
orphan requirements remain review findings, not verifier violations.
