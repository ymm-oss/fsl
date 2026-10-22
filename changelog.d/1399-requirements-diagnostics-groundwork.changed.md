Changed (#1399): the requirements groundwork derives the terminal states of
an entity-based requirements document from its lifecycles, as a business
document does from its process stages (a document that declares its own
terminal states keeps them); the identifier groundwork's lexer warnings (a
full-width space, an identifier not in Unicode normalization form C) are
reported by `check`, the browser Worker and `lint`, as the language server
already showed them; a name that is not ASCII in a document without a
glossary is rejected as `canonical_name_not_ascii` in every dialect; and an
inline `implements` whose layers declare the same enum with other members or
in another order is reported with both member lists and how to reorder the
lifecycle lines. Parsing, diagnostics, and command output of published
documents are unchanged.
