Changed (#1396): the language server can read a requirements document
through the internal keyword table and glossary: aliases resolve to their
canonical names for navigation, references, hover and rename, keywords and
role-marking words of either spelling are not indexed as names, and
completion and hover show each keyword's other spelling. The editor
behaviour and the command output for published documents are unchanged.
