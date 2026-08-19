Fixed (#741): `tools/build_site_reference.py` now rejects a `docs/LANGUAGE.ja.md` that
reorders `## ` sections relative to `docs/LANGUAGE.md` while keeping the section count equal.
`render_language_tree()` previously compared only `len(ja_sections) == len(en_sections)` and
then paired the two files positionally, so a same-count reorder passed generation silently and
attached a Japanese section body to the wrong English anchor and blurb — exactly the drift
`docs/DESIGN-docs-site.md` D7 and this tool's own docstring already claimed was caught. The
fix checks each positional pair's leading numeric section prefix (`"2"` from `"2. Types"`/
`"2. 型"`) and raises `SystemExit` naming both headings and the position on a mismatch, needing
no new maintained ja/en heading-name table. A rejecting control (a same-count reordering
fixture) and an accepting control (the real, untouched files) are both in
`tests/test_site_reference_snapshot.py`.
