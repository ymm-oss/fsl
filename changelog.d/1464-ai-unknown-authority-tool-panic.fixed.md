Fixed (#1464): Unknown tools in AI component authority clauses now return an
authored lowering error instead of panicking in document-taking CLI commands
and the language server. Editor diagnostics retain the authority clause's
source position; existing check and Worker error envelopes are unchanged.
