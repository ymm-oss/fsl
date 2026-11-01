<!-- SPDX-License-Identifier: Apache-2.0 -->

# Complete per-file artifact replacement

Status: accepted (#1419).

## Problem and decision

Writing a generated artifact directly with `std::fs::write` truncates an
existing file before the new bytes are complete. A later write error can leave
an approval record or evidence artifact partial even though the command reports
an I/O failure.

The native CLI uses one private, std-only publication helper in
`rust/fslc/src/output_publication.rs`. Rendering, validation, serialization,
signing, and digest calculation stay with the existing callers. The helper
receives their completed bytes, creates a same-directory temporary file with
`create_new`, writes all bytes, applies existing regular-file permissions when
applicable, closes the temporary file, then renames it onto the destination.
It reports success only after replacement succeeds.

Before replacement, an error leaves the original destination bytes intact (or
the destination absent if it did not exist). Cleanup of a temporary file created
by this call is best-effort. If cleanup also fails, that file may remain, but
the original publication error is returned. Failure to create a temporary file
does not authorize deleting a pre-existing entry at that temporary path.
Temporary names use the process id and a per-process sequence. If the generated
basename equals the destination basename, append one fixed `.tmp` suffix before
`create_new` so preparation never writes at the destination. There is no
filesystem probing or automatic retry service.

## Covered writers and preserved behavior

The finite scope is:

- Approval records: `write_record` and `write_record_v2`, including signed
  records after serialization.
- `document generate` and `document claims` file output.
- `db import` file output.
- `domain expand` and each file from `domain generate`.
- `generated_content_result`: domain testgen, testgen, HTML reports, and ledger
  artifacts.
- `counterexample export` after reproducer serialization.
- Causal observation evidence and lifecycle files.

Caller JSON envelopes, failure classifications, locations, error framing, and
exit codes are unchanged. Publication failures use the existing I/O failure
paths (exit 2). Successful artifact bytes, newlines, signatures, and digests
are unchanged. In particular, successful counterexample export retains its
verification verdict and exit status, rather than converting every completed
write into an exit-0 verdict.

Output path interpretation and parent-directory creation policy remain with
the callers. The helper does not create missing parents, canonicalize paths,
choose default destinations, or restrict intermediate directory symlinks.
Commands that return generated content without writing a file retain that
behavior. Domain generation still creates its existing output directories and
publishes files in its existing iteration order. Causal output still publishes
evidence before the corresponding lifecycle file. A later file's failure does
not roll back earlier completed outputs; this is not a file-set transaction.

Literate extraction, development mutants configuration, migration write-set
rollback, and verification cache publication are separate responsibilities and
are not routed through this helper. Frozen Python, Worker, public Kernel,
schemas, and language semantics are unchanged.

## Permissions and intentional compatibility boundaries

- New files use ordinary creation permissions subject to the process umask.
- Existing regular files retain their permission bits. Read-only existing
  files are refused even if their parent directory permits replacement. An
  existing file is also opened for writing without truncation before preparing
  its replacement, so rename is not used to bypass its write-access denial.
- The parent directory must permit temporary-file creation and replacement.
  Previously, a writable existing file could be overwritten in a non-writable
  parent directory. Such output now fails through the existing I/O path and
  keeps the old bytes.
- A final-component symlink, including a dangling one, is refused before
  changing the link or its referent. Direct writes previously followed such
  links. Existing non-regular destinations (directories, FIFOs, devices, and
  similar entries) are also refused rather than replaced.
- Rename replaces the inode. Other hard-link names continue to refer to the
  old inode and bytes. Beyond permission bits, preservation of existing inode
  attributes is not guaranteed: ownership, ACLs, extended attributes, and
  similar metadata are outside this contract. No OS-specific attribute-copying
  machinery is added.

The guarantee is complete per-file publication, not power-loss durability.
There is no file `fsync`, parent-directory sync, backup set, or rollback layer.
There is also no new guarantee against concurrent path mutation or a hostile
directory; this change does not introduce a link/race framework.

## Acceptance obligations

Acceptance must observe successful new output and replacement, an actual write
failure after partial temporary-file output, preservation of old destination
bytes, and owned-temporary cleanup. It must also cover read-only refusal,
regular permission bits, new-file umask, final symlinks, non-regular targets,
parent permissions, and multi-output ordering with a later failure. Compare
complete successful artifacts and caller JSON/exit behavior, including
signatures and digests, rather than weakening expected values to fit the
implementation. Runtime acceptance and performance evidence are separate from
this design decision.

Cover a destination basename equal to the generated temporary basename: existing
output must be replaced successfully, and an initially absent destination must
remain absent after a partial-write failure with owned-temporary cleanup. The
latter is a post-failure preservation control, not a transient-visibility test.
