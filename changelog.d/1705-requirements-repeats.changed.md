Breaking removal (#1705): in next-edition requirements, `check` and every route that loads the document refuse three repeats, with exit 2 and `kind: "parse"`. For an ID, the ledger used to keep one row, with one title and one status, for the two declarations, or one text for two `covers`.

These forms passed `check` before:

- A second `actor` of one name, on its own line or in a list (`duplicate actor 'Agent'`), located at the repeated name. Its output equalled one copy's.
- A second declaration owning a requirement ID that a `rule`, `decide`, `scenario`, `forbidden`, `goal` or `policy` already owns, located at its ID and naming both kinds (`duplicate requirement id 'REQ-DESK-001': this rule reuses the ID of the rule at 19:8`). That includes two `rule`s of one ID, which lost a title from the ledger, and a `rule` reusing a `decide` ID, whose title reached neither the ledger nor the documents. A second `scenario`, `forbidden`, `goal` or `policy` of its own kind keeps the refusal it had.
- Two `covers` of an ID that no declaration owns, with different texts, anywhere in the document: two events, two rows of one `decide`, rows of two tables, or an event and a row. The refusal is located at the `covers` keyword of the later one and names the earlier one (`requirement ID 'REQ-EV-1' is covered as "withdrawing" here and as "closing" at 15:5`). One ID with one text stays accepted, and the ledger shows it once. An event and a row covering one ID with different texts on one event were refused before as a conflicting annotation with `kind: "semantics"`; they now get this message with `kind: "parse"`, at the same location.

The current edition shares IDs the same way and is unchanged.

Migration: rename one of the two IDs, merge the two declarations, or give every `covers` of an ID one text; delete the repeated actor. There is no automatic migration.

The 651-form on-disk corpus has an empty transition set: base `56354e477544f9be1f4477ddf036e5855d514bb5`, head `c2bd60208cb3102e63b2b3c5ded6ee7fb58a7800`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

Changed ledger output (#1705): in next-edition requirements, a decide row's or an event's `covers` of an ID that a `rule`, `decide`, `scenario`, `forbidden`, `goal` or `policy` owns stays accepted and linked, whatever its text, and now shows the owner's title. The ledger row for the ID used to take the `covers` text and drop the owner's title. With an event's `covers` of a rule's ID, the rule's title also reached neither `document claims` nor `document generate`. The ledger row, the document's rule section and requirement text, and `document claims` now show the owner's title once, cited at the owner. The `covers` text is no longer shown. One case stays refused, as before: a row covering its own table's ID with another text (`requirement annotation 'REQ-2-3' has conflicting text`, `kind: "semantics"`).

The design note `docs/design/DESIGN-dialects.md` holds the measured forms. The language references take this text when the next edition's reference is written (#1437).
