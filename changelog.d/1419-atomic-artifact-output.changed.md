Changed (#1419): native artifact file output now requires a writable parent
directory, including to replace a writable existing file, and refuses final
symlinks and non-regular destinations such as `-o /dev/stdout` with exit 2.
Replacement changes the inode and does not guarantee ownership/ACL/xattr
preservation.
