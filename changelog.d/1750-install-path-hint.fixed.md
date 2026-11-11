Fixed (#1750): the `install.sh` PATH hint names the directory that holds the
command links. With `FSL_BIN_DIR` set, the suggested `export PATH=...` line
used to add `$HOME/.local/bin`, so `fslc` stayed off `PATH` for a user who
followed it. A test runs the installer against a local release and runs the
suggested line, also for a directory whose name holds a space, `$`, `"`, a
backtick and a backslash. The installer no longer carries the v3.0.0 path that
took the skills from a source archive: it only installs the latest release,
which has a checksummed skill bundle. The README uninstall command runs in a
subshell, so pasting it leaves no `data_dir`, `bin_dir`, `skills_dir` or
`link` variable set; a test checks that.
