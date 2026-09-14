Fixed (#1027, #1029): `docs/RELEASE.md` §2 promotion branch names now match the
`production-policy` required check (`release/vX.Y` instead of
`release/vX.Y.Z-candidate`), and pre-merge pin verification requires tree
equivalence plus candidate identity as either HEAD or first parent (covering both
a direct pin and a sanctioned `-s ours --no-ff` preparation). The v4.4.1
carry-forward section is removed; production (`99f150e9`) again contains
`docs/DESIGN-nested-option-support.md` natively after the v4.5.0 promotion
completed the carry-forward.
