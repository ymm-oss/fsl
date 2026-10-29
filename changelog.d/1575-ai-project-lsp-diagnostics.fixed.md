Fixed (#1575): the native language server publishes AI-project component errors
for undeclared authority tools, unknown hard rules and missing tools at their
whole-document UTF-16 positions, and clears them after valid edits. Validation
is shared with CLI/Worker checking without changing their error envelopes.
