Changed (#9996): the requirements parser reads its keywords through one
internal keyword table, which pairs each keyword with a second spelling, and
can read phrases whose words are marked by role, in any order. The checks of
glossary aliases against keyword spellings read the same table. Parsing,
diagnostics, and command output of published documents are unchanged.
