Changed (#9985): the identifier groundwork's lexer warnings (a full-width
space, an identifier not in Unicode normalization form C) are reported by
`check` and the browser Worker in `warnings` and by `lint` as findings with
the replacement text, as the language server already showed them. Parsing,
diagnostics, and command output of published documents are unchanged.
