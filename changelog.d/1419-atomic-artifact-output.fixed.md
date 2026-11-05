Fixed (#1419): native generated artifacts and approval/causal records are
written completely to a same-directory temporary file before replacement,
preserving existing output bytes on publication failure. Existing regular-file
permission bits and new-file umask are retained; setuid and setgid are not. Partial
bytes stay owner-only, though abrupt termination can leave that temporary file.
Multi-file output keeps its existing order without rollback or added power-loss
durability. Temporary names include a clock component and exclude the destination
basename, and an entry already at that name fails with an error naming its path.
