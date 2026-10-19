Changed (#9999): the lexer reads identifiers through one selectable rule whose
diagnostics carry their own codes, and the language server's rename check and
the browser harness's import scan use the same rule. The VS Code grammar now
also highlights a declaration name written with non-ASCII letters. Parsing,
diagnostics, and command output are unchanged.
