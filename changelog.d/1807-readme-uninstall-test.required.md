Required (#1807): the README's uninstall block is tested. A test copies it
verbatim from `README.md`, and fails when it cannot find it, then runs it
against installs from a local release: a default install, one placed with
`FSL_DATA_DIR` and `FSL_BIN_DIR` and one with the deprecated `FSL_INSTALL_DIR`.
It checks that the block keeps the skill backups, a skill link another tool
placed and a real directory under a skill name, and that it removes nothing when
the data directory lacks a `current` link or a `releases` directory. The test
also fails when the block does not name every skill the release bundle ships.
