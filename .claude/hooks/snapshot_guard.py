# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""PreToolUse hook: block hand-edits to the corpus snapshot.

``tests/snapshots/corpus_snapshot.json`` is a generated golden file and the repo's
core behavior-preservation safety net. It must only change via
``FSLC_SNAPSHOT_UPDATE=1 pytest tests/test_corpus_snapshot.py`` — a direct Edit/Write is
almost always a mistake (hollowing the net to dodge a diff). This hook blocks such a
write (exit 2) and points at the regeneration command. The legitimate regeneration path
goes through pytest, not an editing tool, so it is unaffected.
"""
import json
import os
import sys

TARGET = os.path.join("tests", "snapshots", "corpus_snapshot.json")


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        return 0
    path = (data.get("tool_input") or {}).get("file_path") or ""
    if not path:
        return 0
    if os.path.normpath(path).endswith(TARGET):
        sys.stderr.write(
            "Refusing to hand-edit the corpus snapshot ({}).\n".format(TARGET)
            + "Regenerate it only after an intended behavior change (review the diff first):\n"
            + "  FSLC_SNAPSHOT_UPDATE=1 .venv/bin/python -m pytest tests/test_corpus_snapshot.py -q\n"
        )
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
