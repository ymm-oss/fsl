Fixed (#1419): native generated artifacts and approval/causal records are
written completely to a same-directory temporary file before replacement,
preserving existing output bytes on publication failure. Existing regular-file
permission bits and new-file umask are retained. Output now requires a writable
parent directory and refuses final symlinks and non-regular destinations;
replacement changes the inode and does not guarantee ownership/ACL/xattr
preservation. Multi-file output keeps its existing order without rollback or
added power-loss durability.
