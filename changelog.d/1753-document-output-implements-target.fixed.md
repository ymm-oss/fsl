Fixed (#1753): `fslc document generate`, `document claims` and
`approval create --kind requirements_document` refuse an `-o` destination that
is the abstraction a requirements document names in `implements ... from`, also
through `./`, a symlink or a hard link, although they never read that file:
`document claims req.fsl -o abs.fsl` no longer replaces the abstract spec with
the claims JSON. The refusal is the #1737 `kind: "io"` error with exit 2,
before anything is written.
