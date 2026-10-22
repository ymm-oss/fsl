Changed (#9989): the requirements document renderer reads its source under
the same parse options as the rest of the command, through new option-taking
entries beside the projection and rendering entries, and can add reference
views (a lifecycle diagram per entity, event and decision tables, rule
statements and the glossary) for a further declaration form. Parsing,
diagnostics, and command output of published documents are unchanged.
