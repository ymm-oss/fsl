Breaking removal (#1705): in next-edition requirements, `check` and every route that loads the document refuse three repeats with exit 2, `kind: "parse"` and a location at the repeat. For an ID, the ledger used to keep one row, with one title and one status, for the two declarations.

These forms passed `check` before:

- A second `actor` of one name, on its own line or in a list (`duplicate actor 'Agent'`). Its output equalled one copy's.
- A second declaration owning a requirement ID that a `rule`, `decide`, `scenario`, `forbidden`, `goal` or `policy` already owns, at its ID, naming both kinds (`duplicate requirement id 'REQ-DESK-001': this rule reuses the ID of the rule at 19:8`). That includes two `rule`s of one ID, which lost a title from the ledger, and a `rule` reusing a `decide` ID, whose title reached neither the ledger nor the documents. A second `scenario`, `forbidden`, `goal` or `policy` of its own kind keeps the refusal it had.
- Two rows of one `decide` covering one ID with different texts. One ID with one text stays accepted, and the ledger shows it once.

The current edition shares IDs the same way and is unchanged.

Migration: rename one of the two IDs, or merge the two declarations, and delete the repeated actor. There is no automatic migration.

The 651-form on-disk corpus has an empty transition set: base `56354e477544f9be1f4477ddf036e5855d514bb5`, head `779408fa52110cb3ca7b2adf7394d489513c5cb7`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

The design note `docs/design/DESIGN-dialects.md` holds the measured forms. The language references take this text when the next edition's reference is written (#1437).
