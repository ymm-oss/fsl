Changed (#9997): the requirements parser can desugar rule declarations into
guards and properties of the existing process items, the core can decide
over finite type domains whether such guards can hold together, and the
names those properties generate come from the core naming module. Parsing,
diagnostics, and command output of published documents are unchanged.
