Changed (#961): the `no_user_invariants` model warning is suppressed only for
safety-bearing declarations (`invariant`, `trans`, `forbidden`, `implements`);
`reachable`, `leadsTo`, and `acceptance` alone no longer suppress it. Shared
finalization in `fsl-core` replaces frontend message-string filters.
