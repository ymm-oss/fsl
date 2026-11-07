Changed (#1437): the installed Agent Skills now describe FSL 5. The layer
skills `fsl-business`, `fsl-requirements`, `fsl-design`, `fsl-design-review`,
`fsl-from-code`, `fsl-delivery` and `fsl-verify` are replaced by their FSL 5
versions, which write files headed `fsl 5`. Each one also notes that a headerless
document is FSL 4 and points to the migration guide in `docs/manual/LANGUAGE.md`.
`fsl-requirements/references/` adds the FSL 5 requirements syntax, the English /
Japanese keyword table and the glossary guide.

The shared `fsl` skill keeps its FSL 4 and kernel guidance. It adds the FSL 5
route, the header placement, and compact sections on annotations and reusable
predicates, Option payload scope, the ranked-response recipe, and compose and
refinement. It also adds the per-file edition rule and the scoped-liveness
procedure to `commands.md`. The unpackaged `skills-next/` directory is removed;
`fsl-requirements-document` is unchanged.
