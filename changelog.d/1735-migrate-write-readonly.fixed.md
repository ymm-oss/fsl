Fixed (#1735): `fslc migrate --write` no longer rewrites a read-only input.
Since `migrate` was added (3.0.0; observed on 4.12.0), it replaced a changed
mode-0444 file through a sibling temporary file and a rename, reported
`"written": true` and exited 0, leaving the new bytes read-only. It now refuses
such an input with the same `io` error as a read-only `-o` target (#1419):
`output target is read-only`, exit 2. The check runs over the whole write set
before any temporary file is prepared, so one read-only input leaves every input
unchanged; a temporary file prepared for an earlier input is also removed when a
later input's temporary file cannot be created. Writable inputs are rewritten
as before, keeping their permission bits.
