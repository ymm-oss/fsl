Fixed (#761): the legacy replay trace object wrapper is now exactly `{"events": [...]}` —
a misspelled key (for example `eventz`), a non-array `events` value, or extra keys are
rejected with the wrapper diagnostic instead of being silently tolerated. Focused native
accepting and rejecting contracts pin the wrapper and its full public projection, and a
new liveness-witness replay matrix rejects isolated state, action, and loop corruptions
of every `leadsTo` lasso case the retired Python comparison covered.
