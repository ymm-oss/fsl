Fixed (#1392): `fslc fmt` and `fslc migrate` print requirement-style IDs
the way they are written. They used to put spaces around each `-` of an ID
outside `business` documents, so `requirement REQ-1`, `acceptance AC-1`,
`forbidden FB-1`, `covers REQ-1` and governance references such as
`owns CTRL-1` or `require CTRL-1` came out as `REQ - 1`, `AC - 1`, `CTRL - 1`.
The ID values and the checked model did not change, but the written file no
longer read as its author wrote it. Subtraction is still printed with spaces
(`balance - 1`). An ID written with spaces (`REQ - 1`) is now printed as its
value (`REQ-1`). If the formatter would ever print an ID differently from its
value, `fmt` and `migrate` now stop with an error instead of writing the file.
Migration: a file already written by `fmt` or `migrate --write` with spaced
IDs is reported as changed by `fslc fmt --check`; run `fslc fmt` on it (or
restore the IDs by hand) to get `REQ-1` back.
