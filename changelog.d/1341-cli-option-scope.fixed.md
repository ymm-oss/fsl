Fixed (#1341, breaking): `explain` and `typestate` reject shared options that do not
apply to them, and `testgen` rejects its unused `--engine` option. These
options now report an unknown-option usage error (exit 2), before reading
any option value. This is part of the #1446 CLI contract change.
