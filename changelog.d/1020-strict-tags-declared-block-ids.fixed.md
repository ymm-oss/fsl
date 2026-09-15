Fixed (#1020): `--strict-tags`'s `Declared` side now auto-collects requirement-block
IDs from the requirements dialect, not only the lines of an optional
`--requirements ids.txt` file. `docs/DESIGN-strict-tags.md` section 2 defines
`Declared` as `--requirements` union the requirements dialect's requirement-block
IDs, and calls that union "essential" for catching an empty requirement block --
one declared but never formalized, whose trace disappears after expansion. The
native implementation collected only the file half, so without `--requirements`
the `Declared` side never ran at all and an unformalized empty requirement block
passed silently. `check`/`verify` (independent option-parsing paths for
`--strict-tags`/`--requirements`) now both report such a block as
`unreferenced_requirement` with no `--requirements` flag needed. Exit codes are
unchanged: no path in the codebase branches exit status on the `warnings` array's
contents, so a spec that starts warning here still exits the same as before. An
existing `--requirements` file's already-referenced IDs keep their original
file-line warning order; only IDs that reach `Declared` solely through
auto-collection are appended afterward, so established exact-match warning-array
expectations are undisturbed.
