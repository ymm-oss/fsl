Changed (#1473): DB lowering rejects unknown compatibility-rule names with an
authored semantics error before emitting a model or artifact. Kernel export no
longer accepts these malformed documents; file-based fmt, lint, and migrate now
return exit 2 instead of exit 0. Commands retain their established lowering-error
locations, including positions in the message rather than a loc field for
fmt/lint/migrate. Check and Worker specialized-validator envelopes, all twelve
valid rule names, default selection, repair-tool carve-outs, and DB-only findings
remain unchanged. The rule vocabulary is shared by fsl-core and fsl-tools.
The base/head check sweep covered 595 file-backed forms with no ok-to-error
transitions or output/exit drift; exact commits, binary hashes, and commands are
recorded in docs/design/DESIGN-db.md.
