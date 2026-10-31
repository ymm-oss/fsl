Fix causal observation log provenance to hash the same captured UTF-8 input
used for replay evaluation, rather than rereading its path. Preserve ordinary
JSON outputs, errors, assurance, replay parsing, and artifact/lifecycle digest
canonicalization. Observe/verify already use the supplied causal source; this
fix addresses the remaining log reread in #1423.
