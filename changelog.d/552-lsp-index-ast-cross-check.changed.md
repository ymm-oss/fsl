Changed (#552): the language server's document index is now checked against
the parsed declarations (name, role, and owning scope) over the corpus, so an
index heuristic that gives a name the wrong role or owner fails the tests. The
index and the server's responses are unchanged.
