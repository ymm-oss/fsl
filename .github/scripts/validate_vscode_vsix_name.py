#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0

"""Keep the documented VS Code release asset name aligned with packaging."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RELEASE = ROOT / ".github/workflows/release.yml"
DOCUMENTS = (ROOT / "editors/vscode/README.md", ROOT / "README.md")


def validate(release: str, documents: dict[str, str]) -> list[str]:
    build_step = re.search(
        r"(?ms)^\s*- name: Build \.vsix\s*\n(?P<step>.*?)(?=^\s*- name:|\Z)",
        release,
    )
    if build_step is None:
        return ["release.yml: could not locate the Build .vsix packaging step"]
    outputs = re.findall(r"(?:^|\s)--out\s+([^\s]+)", build_step.group("step"))
    if len(outputs) != 1:
        return [f"release.yml: expected one --out name in Build .vsix, found {len(outputs)}"]
    expected = outputs[0]
    errors = []
    for name, text in documents.items():
        if expected not in text:
            errors.append(f"{name}: missing published VSIX name {expected!r}")
        if re.search(r"fsl-vscode-<version>\.vsix|fsl-vscode-[^\s`]+\.vsix", text):
            errors.append(f"{name}: contains a versioned VSIX name")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", type=Path, default=RELEASE)
    parser.add_argument("--vscode-readme", type=Path, default=DOCUMENTS[0])
    parser.add_argument("--root-readme", type=Path, default=DOCUMENTS[1])
    args = parser.parse_args()
    documents = {
        "editors/vscode/README.md": args.vscode_readme.read_text(encoding="utf-8"),
        "README.md": args.root_readme.read_text(encoding="utf-8"),
    }
    errors = validate(args.release.read_text(encoding="utf-8"), documents)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("VS Code VSIX documentation matches the release packaging name")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
