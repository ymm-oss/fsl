Fixed (#741): `tools/build_site_reference.py` now rejects a `docs/LANGUAGE.ja.md` that
reorders `## ` sections relative to `docs/LANGUAGE.md` while keeping the section count equal.
`render_language_tree()` previously compared only `len(ja_sections) == len(en_sections)` and
then paired the two files positionally, so a same-count reorder passed generation silently and
attached a Japanese section body to the wrong English anchor and blurb — exactly the drift
`docs/DESIGN-docs-site.md` D7 and this tool's own docstring already claimed was caught. The
fix checks each positional pair's leading numeric section prefix (`"2"` from `"2. Types"`/
`"2. 型"`) and raises `SystemExit` naming both headings and the position on a mismatch, needing
no new maintained ja/en heading-name table. That per-position check is only sound if
`docs/LANGUAGE.md`'s own numbers are unique, so a second assertion now rejects a duplicated
English section number directly, naming the duplicate, instead of leaving uniqueness an
unstated precondition. Rejecting controls (a same-count reordering fixture and a duplicate-number
fixture) and an accepting control (the real, untouched files) are all in
`tests/test_site_reference_snapshot.py`.
