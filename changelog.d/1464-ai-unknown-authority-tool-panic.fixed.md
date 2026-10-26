Fixed (#1464): Lowering now returns an authored error instead of panicking
when the generated kernel needs an undeclared AI authority tool reference,
including in document-taking CLI commands and the language server. Editor
diagnostics point at the authority clause that names the tool; existing check
and Worker error envelopes are unchanged.
Empty domain membership now renders as false without panicking in domain
analyze, check, expand, or mutation after a verified baseline; domain check
still reports a violation. Kernel projection of `at()` without an index now
returns the existing semantics error instead of panicking, while check
acceptance and valid `at(0)` projections are unchanged.
