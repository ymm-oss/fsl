Fixed (#1807): `install.sh` prints the `FSL_INSTALL_DIR` deprecation warning
on stderr, like its other warnings and the notice for a skill link another tool
placed. It used to print it on stdout, so a script that captured the
installer's stdout got the warning mixed in. A test runs the installer against
a local release and checks the warning's channel, and that `FSL_DATA_DIR` still
takes precedence over `FSL_INSTALL_DIR` without a warning.
