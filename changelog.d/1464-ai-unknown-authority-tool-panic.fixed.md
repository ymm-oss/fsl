Fixed (#1464): Unknown tools in AI component authority clauses now return an
authored lowering error instead of panicking in document-taking CLI commands
and the language server. Editor diagnostics retain the authority clause's
source position; existing check and Worker error envelopes are unchanged.
Empty domain membership now renders as false without panicking in domain
analyze, check, expand, or mutation after a verified baseline; domain check
still reports a violation. Kernel projection of `at()` without an index now
returns the existing semantics error instead of panicking, while check
acceptance and valid `at(0)` projections are unchanged.
