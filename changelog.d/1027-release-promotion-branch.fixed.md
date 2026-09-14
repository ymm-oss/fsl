Fixed (#1027, #1029): `docs/RELEASE.md` §2 promotion branch names now match the
`production-policy` required check (`release/vX.Y` instead of
`release/vX.Y.Z-candidate`), and pre-merge pin verification uses tree equivalence
with first-parent candidate identity instead of exact commit SHA equality. The
v4.4.1 carry-forward section now states its drop condition explicitly.
