Changed (#1398): the repository holds a draft set of layer skills outside the
installable skill bundle, and its tests keep that draft checked: every example
in it must check, its keyword table must match the parser's, and the release
must not package it. `tools/check-doc-fences.py` gains `--every-fence`, which
fails an unmarked fence instead of skipping it. The installed skills, parsing,
diagnostics, and command output are unchanged.
