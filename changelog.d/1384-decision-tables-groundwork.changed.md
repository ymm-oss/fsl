Changed (#9996): the requirements parser can fold a further declaration form
(decision tables) into the guards of existing process transitions, and the
core checks that such a table's rows cover and partition the values of the
fields they read, enumerating finite domains and handing larger ones to the
solver in `check`. Parsing, diagnostics, and command output of published
documents are unchanged.
