Fixed (#1342): `db check` validates each `--deadlock` value as `warn`,
`error`, or `ignore` before loading the source. Invalid values report a
usage error (exit 2), even when followed by a valid repeated value.
