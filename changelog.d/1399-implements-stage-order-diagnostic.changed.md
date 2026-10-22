Changed (#1399): in the requirements groundwork, an inline `implements` whose
two layers declare the same enum with its members in another order, or with
other members, is reported at the `implements` declaration with both member
lists, the members only one side has, and for a lifecycle's stage enum how
to reorder the lifecycle lines. Parsing, diagnostics, and command output of
published documents are unchanged.
