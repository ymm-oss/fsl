Unified (#1877): the verify cache's format tags -- the key salt, the entry
directory, the cross-depth pointer directory, the entry schema and the
pointer schema -- are each one named constant in
`rust/fslc/src/verification.rs`, used by every reader and writer, and the
pointer directory is spelled as a subdirectory of the entry directory, so
the two cannot move apart. `chain --jobs` groups steps by asking
`chain_steps_may_share_verify_cache_entry`, beside the key derivation,
whether two steps can share an entry, instead of comparing root-file bytes
itself; the answer is unchanged (equal root bytes, `spec` steps only). The
key derivation no longer takes the unused source path. Cache keys, the cache
layout, entry and pointer contents and command output are unchanged.
