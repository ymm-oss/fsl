Changed (#1419): native artifact file output now requires a writable parent
directory, including to replace a writable existing file, and refuses final
symlinks and every non-regular destination with exit 2. This is breaking for
`-o /dev/null`, `-o /dev/stdout` and process substitution such as `/dev/fd/63`.
Replacement changes the inode and does not guarantee ownership/ACL/xattr
preservation.
