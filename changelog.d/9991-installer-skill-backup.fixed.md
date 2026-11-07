Fixed (#9991): `install.sh` no longer keeps the copy of a skill it replaces
inside `~/.claude/skills/`. Claude Code loads every directory there as a
skill, so a backup named `<name>.pre-native-v3` was offered next to the live
skill as a second, stale copy. A replaced copy now goes to
`<data>/skill-backups/claude/<name>-<UTC time>` under the FSL data directory,
and the installer prints the path. On its next run with skills enabled, the
installer moves each `fsl*.pre-native-v3` that an earlier version left in
`~/.claude/skills/` into the same backup directory, unchanged and under the
same name, and prints each move. It removes nothing, and it stops before
linking anything when a backup path already exists. The README describes the
backup location. Its uninstall command now honours `FSL_DATA_DIR`,
`XDG_DATA_HOME` and `FSL_BIN_DIR`, and it removes a command or skill link only
when the link points into the FSL data directory. It used to delete a skill
link another tool placed, and a real directory under a skill name, both of
which the installer itself leaves alone. The README's macOS
`xattr -d com.apple.quarantine` step no longer fails when the file has no such
attribute.
