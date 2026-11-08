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
Temporary names use the process id, a nanosecond clock reading and a per-process
sequence, so a stale name left by an earlier process with the same id (for example
a fixed container PID) does not repeat. If the generated basename equals the
destination basename, append one fixed `.tmp` suffix before `create_new` so
preparation never writes at the destination. There is no filesystem probing or
automatic retry service: an entry already at the temporary name fails with an error
naming that path.

Abrupt termination (a signal, or a file-size limit that ends the process) can leave
a `.fslc-output-*.tmp` file with partial content beside the destination. The destination
keeps its old bytes, and that temporary file is owner-only (mode 0600 on Unix).

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

Development mutants configuration, migration write-set rollback, and
verification cache publication are separate responsibilities and are not routed
through this helper. Migration write sets do reuse its existing-file writability
check (`ensure_writable`), so `migrate --write` refuses a read-only input with the
same `io` error before preparing any replacement. Frozen Python, Worker, public Kernel,
schemas, and language semantics are unchanged.

## Permissions and intentional compatibility boundaries

- New files use ordinary creation permissions subject to the process umask.
- Existing regular files retain their permission bits (`0o777`). Setuid, setgid and
  sticky bits are not restored. The replacement is created owner-only (`0600`) and
  receives its final bits before the rename, so partial bytes are never wider than the
  owner. A new output is created with the default mode to learn its umask-derived bits,
  then narrowed to `0600` before any byte is written. Read-only existing
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
  similar entries) are also refused rather than replaced. This is breaking for
  `-o /dev/null`, `-o /dev/stdout` and process substitution such as `-o >(cmd)`, which
  shells pass as a path like `/dev/fd/63`; those callers should omit `-o` where the
  command prints its artifact, or write a regular file.
- Rename replaces the inode. Other hard-link names continue to refer to the
  old inode and bytes. Beyond permission bits, preservation of existing inode
  attributes is not guaranteed: ownership, ACLs, extended attributes, and
  similar metadata are outside this contract. No OS-specific attribute-copying
  machinery is added.

## Amendment: an input is never an output target

`fslc html s.fsl -o s.fsl` used to replace the spec with the report. The helper
now refuses, before anything is written, a target that is the same file as one
this process read as a command input. Identity is the file, not its spelling:
device and inode on Unix, so `./s.fsl`, `../dir/s.fsl`, a symlink to `s.fsl`
and a hard link to it all match; elsewhere a canonicalized path, which does not
detect a second hard-link name. The refusal is an `io` error with exit 2 and
the message `output target is an input of this command`, in the same envelope
as the other refusals here (including `testgen`'s envelope without `kind`).

The inputs are the files read through the shared read helpers: the spec or
document argument, every file an import, `use ... from`, `implements ... from`
or refinement reference loads through the native filesystem resolver, and
explicit file options such as `--evidence`, `--glossary`, `--approval`,
`--trust-key`, `--signing-key`, `--artifact`, `--impl-log`, `--scope`,
`--mapping` and `--from-log`. One file counts without being read: the
abstraction a `requirements` document names in `implements ... from`, which
`document generate`/`claims` (and the `requirements_document` approval
rendering) project without loading (#1753). It is the only file a `spec` or
`requirements` document declares, the only dialects those commands accept, and
its path resolves against the spec's directory as the resolver's would; a
missing abstraction records nothing, so `-o` may create it. `domain generate`
and `causal observe-expectations` check every destination before their first
write. The prior artifact that an
approval record names as its reviewed target is read to compare digests, not
as an input, so regenerating that artifact in place stays allowed.
`migrate --write` and the verification cache do not use this helper and are
unchanged.

Recording starts in the native CLI's `main`, which runs one command per
process; code that calls a command function in-process (unit tests) records
nothing, so one test's inputs never refuse another test's outputs.

Limits: apart from that `implements ... from` target, a file is recorded only
after it was read, and the check is not a guard against concurrent path
mutation. Reads outside the shared helpers (for
example a resolver built inside `fsl-tools`) are not recorded; each such
command still records its primary input, which is the file those resolvers
start from.

The guarantee is complete per-file publication, not power-loss durability.
There is no file `fsync`, parent-directory sync, backup set, or rollback layer.
There is also no new guarantee against concurrent path mutation or a hostile
directory; this change does not introduce a link/race framework.

## Acceptance obligations

Acceptance must observe successful new output and replacement, an actual write
failure after partial temporary-file output, preservation of old destination
bytes, and owned-temporary cleanup. It must also cover read-only refusal,
regular permission bits, new-file umask, final symlinks, non-regular targets,
parent permissions, setuid removal, the owner-only temporary file left by abrupt
termination, `/dev/null`, and multi-output ordering with a later failure. Compare
complete successful artifacts and caller JSON/exit behavior, including
signatures and digests, rather than weakening expected values to fit the
implementation. Runtime acceptance and performance evidence are separate from
this design decision.

Cover a destination basename equal to the generated temporary basename at the name
function, since the clock component makes that name unpredictable from the command line.
